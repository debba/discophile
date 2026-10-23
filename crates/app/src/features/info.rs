//! "About": on-request information about an artist, album, song, podcast or episode, in the side panel
//! (see `discophile_core::music_info`). Sources load first: the server's biography or notes, feed
//! descriptions and, with Online music information on, MusicBrainz and Wikipedia. The assistant's default
//! provider then writes an overview, and follow-up questions about the same subject get short answers.
//! Without a usable provider the sources are shown on their own.
//!
//! Sources and overviews are kept in memory for the session, per subject, language and provider.

use std::collections::HashMap;
use std::sync::Arc;

use discophile_core::assistant::{AssistantProvider, assistant_error_message};
use discophile_core::client::MusicClient;
use discophile_core::lrclib::{SleepFuture, Sleeper};
use discophile_core::music_info::{self, InfoAnswer, InfoExchange, InfoSource, InfoSubject, MusicInfoClient};
use discophile_core::{Album, Artist, PodcastChannel, Song};

use crate::app::App;
use crate::features::{assistant, library};
use crate::nav::SidePanel;
use crate::tasks::update;

/// Subjects whose sources stay cached for the session.
const CACHE_LIMIT: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub enum AnswerStatus {
    Loading,
    Done(InfoAnswer),
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Exchange {
    pub question: String,
    pub answer: AnswerStatus,
}

/// What the side panel shows.
pub struct InfoView {
    pub subject: InfoSubject,
    /// `None` while loading.
    pub sources: Option<Vec<InfoSource>>,
    /// `None` when no provider can answer; see `unavailable`.
    pub overview: Option<AnswerStatus>,
    /// Why the assistant cannot write an overview, such as "Connect ChatGPT…".
    pub unavailable: Option<String>,
    /// Who writes the overview and answers: the assistant's default, or what the person picks in the
    /// header menu. `model` is a ChatGPT model slug, empty for the default.
    pub provider: AssistantProvider,
    pub model: String,
    /// Model name shown above answers.
    pub provider_name: String,
    pub exchanges: Vec<Exchange>,
    pub draft: String,
    pub scroll_to_bottom: bool,
    /// Identifies this view in task results.
    id: u64,
}

#[derive(Default)]
pub struct InfoState {
    pub view: Option<InfoView>,
    sources: HashMap<String, Vec<InfoSource>>,
    overviews: HashMap<String, (InfoAnswer, String)>,
    next_id: u64,
    client: Option<MusicInfoClient>,
}

pub fn start(app: &mut App) {
    let sleeper: Sleeper = Arc::new(|ms| -> SleepFuture { Box::pin(crate::platform::sleep_ms(ms)) });
    app.info.client = Some(MusicInfoClient::new().with_sleeper(sleeper));
}

/// Server sources belong to a library; overviews are kept because they describe public subjects.
pub fn on_library_changed(app: &mut App) {
    app.info.sources.clear();
    app.info.view = None;
    if app.nav.side_panel == SidePanel::Info {
        app.nav.side_panel = SidePanel::None;
    }
}

pub fn about_artist(app: &mut App, artist: &Artist) {
    open(app, InfoSubject::artist(artist));
}

pub fn about_album(app: &mut App, album: &Album) {
    open(app, InfoSubject::album(album));
}

/// Songs, or episodes with their feed description when the podcast page has loaded.
pub fn about_song(app: &mut App, song: &Song) {
    if song.is_podcast()
        && let Some((channel, episode)) = song.id.strip_prefix("podcast:").and_then(|rest| rest.split_once(':'))
        && let Some(channel) = library::find_podcast(app, channel)
        && let Some(episode) = channel.episodes().iter().find(|e| e.id == episode)
    {
        return open(app, InfoSubject::episode(episode, Some(&channel)));
    }
    open(app, InfoSubject::song(song));
}

pub fn about_podcast(app: &mut App, channel: &PodcastChannel) {
    open(app, InfoSubject::podcast(channel));
}

/// Shows the subject beside the player in Now Playing, otherwise in the side panel, and loads it, reusing
/// what this session already found.
pub fn open(app: &mut App, subject: InfoSubject) {
    if app.nav.now_playing {
        crate::ui::now_playing::show_inline(app, crate::ui::now_playing::Inline::Info);
    } else {
        app.nav.side_panel = SidePanel::Info;
    }
    if app.info.view.as_ref().is_some_and(|v| v.subject == subject) {
        return;
    }
    app.info.next_id += 1;
    let (provider, model) = (app.assistant.config.provider, String::new());
    app.info.view = Some(InfoView {
        subject,
        sources: None,
        overview: None,
        unavailable: assistant::unavailable_reason(app, provider),
        provider,
        provider_name: assistant::provider_name(app, provider, &model),
        model,
        exchanges: Vec::new(),
        draft: String::new(),
        scroll_to_bottom: false,
        id: app.info.next_id,
    });
    load_sources(app, false);
}

pub fn close(app: &mut App) {
    app.nav.side_panel = SidePanel::None;
}

fn locale(app: &App) -> &'static str {
    app.settings.effective_language().code()
}

fn source_key(app: &App, subject: &InfoSubject) -> String {
    format!("{}\u{1f}{}\u{1f}{}", subject.cache_key(), locale(app), app.settings.online_music_info)
}

fn overview_key(app: &App, subject: &InfoSubject, provider: AssistantProvider, chatgpt_model: &str) -> String {
    let model = match provider {
        AssistantProvider::Local => app.assistant.config.local.model.clone(),
        AssistantProvider::ChatGpt if chatgpt_model.is_empty() => app.assistant.config.chatgpt_model.clone(),
        AssistantProvider::ChatGpt => chatgpt_model.to_string(),
    };
    format!("{}\u{1f}{provider:?}\u{1f}{model}", source_key(app, subject))
}

/// Gathers the sources (again when `refresh`), then asks for the overview.
fn load_sources(app: &mut App, refresh: bool) {
    let Some(view) = &app.info.view else { return };
    let (id, subject) = (view.id, view.subject.clone());
    let key = source_key(app, &subject);
    if !refresh && let Some(sources) = app.info.sources.get(&key).cloned() {
        apply_sources(app, id, key, sources);
        return;
    }
    let Some(info) = app.info.client.clone() else { return };
    let server: Option<MusicClient> = app.client();
    let online = app.settings.online_music_info;
    let language = locale(app);
    app.tasks.spawn(async move {
        let sources = info.gather(&subject, server.as_ref(), online, language).await.sources;
        update(move |app| apply_sources(app, id, key, sources))
    });
}

fn apply_sources(app: &mut App, id: u64, key: String, sources: Vec<InfoSource>) {
    if app.info.sources.len() >= CACHE_LIMIT && !app.info.sources.contains_key(&key) {
        app.info.sources.clear();
    }
    app.info.sources.insert(key, sources.clone());
    let Some(view) = app.info.view.as_mut().filter(|v| v.id == id) else { return };
    view.sources = Some(sources);
    request_overview(app, false);
}

/// Asks the default provider for an overview, unless one is cached (or `refresh`).
fn request_overview(app: &mut App, refresh: bool) {
    let Some((provider, model)) = app.info.view.as_ref().map(|v| (v.provider, v.model.clone())) else { return };
    let unavailable = assistant::unavailable_reason(app, provider);
    let Some(view) = app.info.view.as_mut() else { return };
    let Some(sources) = view.sources.clone() else { return };
    view.unavailable = unavailable;
    if view.unavailable.is_some() {
        view.overview = None;
        return;
    }
    let (id, subject) = (view.id, view.subject.clone());
    let key = overview_key(app, &subject, provider, &model);
    if !refresh && let Some((answer, name)) = app.info.overviews.get(&key).cloned() {
        let view = app.info.view.as_mut().expect("checked above");
        view.overview = Some(AnswerStatus::Done(answer));
        view.provider_name = name;
        return;
    }
    let plan = match assistant::plan_responder(app, provider, &model) {
        Ok(plan) => plan,
        Err(error) => {
            if let Some(view) = app.info.view.as_mut() {
                view.overview = Some(AnswerStatus::Failed(assistant_error_message(error)));
            }
            return;
        }
    };
    if let Some(view) = app.info.view.as_mut() {
        view.overview = Some(AnswerStatus::Loading);
    }
    let tasks = app.tasks.clone();
    let language = locale(app);
    let compact = provider == AssistantProvider::Local;
    app.tasks.spawn(async move {
        let result = match plan.build(tasks).await {
            Ok((responder, name)) => {
                music_info::describe(&responder, language, &subject, &sources, &[], None, compact).await.map(|a| (a, name))
            }
            Err(error) => Err(error),
        };
        update(move |app| {
            if let Ok((answer, name)) = &result {
                if app.info.overviews.len() >= CACHE_LIMIT {
                    app.info.overviews.clear();
                }
                app.info.overviews.insert(key, (answer.clone(), name.clone()));
            }
            let Some(view) = app.info.view.as_mut().filter(|v| v.id == id) else { return };
            view.overview = Some(match result {
                Ok((answer, name)) => {
                    view.provider_name = name;
                    AnswerStatus::Done(answer)
                }
                Err(error) => AnswerStatus::Failed(assistant_error_message(error)),
            });
        })
    });
}

/// Sends the draft as a follow-up question.
pub fn ask(app: &mut App) {
    let Some(view) = app.info.view.as_mut() else { return };
    let (provider, model) = (view.provider, view.model.clone());
    let Some(question) = music_info::prepare_question(&view.draft) else { return };
    if view.unavailable.is_some() || is_answering(view) {
        return;
    }
    let Some(sources) = view.sources.clone() else { return };
    let history: Vec<InfoExchange> = view
        .exchanges
        .iter()
        .filter_map(|e| match &e.answer {
            AnswerStatus::Done(answer) => Some(InfoExchange { question: e.question.clone(), answer: answer.text.clone() }),
            _ => None,
        })
        .collect();
    view.draft.clear();
    view.exchanges.push(Exchange { question: question.clone(), answer: AnswerStatus::Loading });
    view.scroll_to_bottom = true;
    let (id, index, subject) = (view.id, view.exchanges.len() - 1, view.subject.clone());
    let plan = match assistant::plan_responder(app, provider, &model) {
        Ok(plan) => plan,
        Err(error) => return finish_exchange(app, id, index, Err(error)),
    };
    let tasks = app.tasks.clone();
    let language = locale(app);
    let compact = provider == AssistantProvider::Local;
    app.tasks.spawn(async move {
        let result = match plan.build(tasks).await {
            Ok((responder, _)) => {
                music_info::describe(&responder, language, &subject, &sources, &history, Some(&question), compact).await
            }
            Err(error) => Err(error),
        };
        update(move |app| finish_exchange(app, id, index, result))
    });
}

fn finish_exchange(
    app: &mut App,
    id: u64,
    index: usize,
    result: Result<InfoAnswer, discophile_core::assistant::AssistantError>,
) {
    let Some(view) = app.info.view.as_mut().filter(|v| v.id == id) else { return };
    if let Some(exchange) = view.exchanges.get_mut(index) {
        exchange.answer = match result {
            Ok(answer) => AnswerStatus::Done(answer),
            Err(error) => AnswerStatus::Failed(assistant_error_message(error)),
        };
        view.scroll_to_bottom = true;
    }
}

pub fn is_answering(view: &InfoView) -> bool {
    view.overview == Some(AnswerStatus::Loading) || view.exchanges.iter().any(|e| e.answer == AnswerStatus::Loading)
}

/// Try again: a fresh lookup of every source and a new overview.
pub fn retry(app: &mut App) {
    if let Some(view) = app.info.view.as_mut() {
        view.sources = None;
        view.overview = None;
    }
    load_sources(app, true);
}

/// The header menu: another provider or model writes the overview (cached ones are reused) and answers
/// the next questions. A provider becomes the default of new chats, as in the assistant.
pub fn pick_model(app: &mut App, pick: assistant::ModelPick) {
    use assistant::ModelPick;
    let Some(view) = app.info.view.as_mut() else { return };
    match pick {
        ModelPick::Provider(provider) => {
            view.provider = provider;
            view.model.clear();
            app.assistant.config.provider = provider;
            assistant::save_config(app);
            if provider == AssistantProvider::ChatGpt && app.assistant.chatgpt.models.is_empty() {
                assistant::load_models(app);
            }
        }
        ModelPick::ChatGpt(model) => view.model = model,
        ModelPick::Local(_) => {}
    }
    let Some((provider, model)) = app.info.view.as_ref().map(|v| (v.provider, v.model.clone())) else { return };
    let name = assistant::provider_name(app, provider, &model);
    if let Some(view) = app.info.view.as_mut() {
        view.provider_name = name;
    }
    request_overview(app, false);
}

/// A new overview from the same sources.
pub fn regenerate(app: &mut App) {
    request_overview(app, true);
}

/// Turning Online music information on or off reloads the open view.
pub fn set_online(app: &mut App, enabled: bool) {
    app.settings.online_music_info = enabled;
    app.settings.save();
    if app.info.view.is_some() {
        retry(app);
    }
}
