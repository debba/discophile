use super::*;
use discophile_core::connect::services::PairedDevice;
use discophile_core::connect::{Account, AccountInfo, Entitlement, Plan, PlanSource, Tier};
use discophile_core::l10n::{self, Language};
use uuid::Uuid;

pub(in crate::ui::connect) fn fixture() -> App {
    let mut app = App::connect_test_app();
    app.connect.account = Some(AccountInfo {
        instance_id: "fixture".into(),
        account: Account {
            id: "fixture".into(),
            name: "Fixture".into(),
            email: None,
            email_verified: true,
            avatar_url: None,
        },
        plan: Plan {
            tier: Tier::Free,
            name: "Free".into(),
            source: PlanSource::Default,
            expires_at: None,
            entitlements: vec![],
        },
    });
    app.connect.services.paired = true;
    app.connect.services.devices = [("Linux", true, false), ("Studio Mac", false, false), ("Office", false, true)]
        .into_iter()
        .map(|(name, online, suspended)| PairedDevice {
            id: Uuid::new_v4(),
            name: name.into(),
            online,
            suspended,
            state: None,
        })
        .collect();
    let id = app.connect.services.devices[0].id;
    app.connect.services.states.insert(
        id,
        serde_json::from_value(serde_json::json!({
            "title": "Call It Stormy Monday · Albert King & Stevie Ray Vaughan", "playing": true,
            "position": 42.0, "capturedAt": 0, "canHandoff": true, "volume": 0.5, "duration": 300.0,
        }))
        .unwrap(),
    );
    app
}

pub(in crate::ui::connect) fn setup(app: &App, dark: bool, zoom: f32) -> egui::Context {
    let ctx = app.ctx().clone();
    ctx.enable_accesskit();
    theme::install_fonts(&ctx);
    theme::apply(&ctx, dark);
    ctx.set_zoom_factor(zoom);
    ctx
}

pub(in crate::ui::connect) fn input(ctx: &egui::Context, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(900.0, 600.0) / ctx.zoom_factor())),
        events,
        ..Default::default()
    }
}

fn render(app: &mut App, ctx: &egui::Context, open: bool, events: Vec<egui::Event>) -> (egui::FullOutput, egui::Id) {
    let mut trigger = None;
    let output = ctx.run_ui(input(ctx, events), |_| {
        // Match the real player bar: the picker is anchored near the bottom-right corner.
        egui::Area::new(egui::Id::unique("device-picker-fixture"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-24.0, -24.0])
            .show(ctx, |ui| {
                let response = player_devices(app, ui);
                trigger = Some(response.id);
                if open {
                    egui::Popup::open_id(ctx, egui::Popup::default_response_id(&response));
                }
            });
    });
    (output, trigger.unwrap())
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn free_picker_shows_devices_status_and_plan_action_in_both_languages_and_themes() {
    for language in Language::ALL {
        l10n::set_language(language);
        for dark in [false, true] {
            let mut app = fixture();
            let ctx = setup(&app, dark, 1.0);
            let mut tree = None;
            for frame in 0..3 {
                let (mut output, _) = render(&mut app, &ctx, frame == 0, vec![]);
                output.textures_delta.clear();
                tree = output.platform_output.accesskit_update;
            }
            let tree = tree.unwrap();
            for label in [
                "Linux",
                "Studio Mac",
                "Office",
                &t("Online"),
                &t("Offline"),
                &t("Suspended by the current plan"),
                &t("Playback handoff is included in the Connect plan."),
            ] {
                assert!(
                    tree.nodes.iter().any(|(_, node)| node.label() == Some(label) || node.value() == Some(label)),
                    "missing {label}"
                );
            }
            for label in [t("Explore Connect"), t("Manage playback devices")] {
                let node = tree.nodes.iter().map(|(_, node)| node).find(|node| node.label() == Some(&label)).unwrap();
                assert_eq!(node.role(), egui::Role::MenuItem);
                assert!(!node.is_disabled());
                assert!(node.bounds().unwrap().height() >= 44.0);
            }
            assert!(
                !tree
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some(tf("Continue here from %@", &[&"Linux"]).as_str()))
            );
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, node)| node.role() == egui::Role::Menu && node.label() == Some(t("Playback devices").as_str()))
            );
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, node)| node.has_popup() == Some(egui::accesskit::HasPopup::Menu) && node.is_expanded() == Some(true))
            );
        }
    }
    l10n::set_language(Language::English);
}

#[test]
fn upgrade_updates_open_picker_and_names_handoff_actions_for_each_device() {
    l10n::set_language(Language::English);
    let mut app = fixture();
    let ctx = setup(&app, false, 1.0);
    for frame in 0..3 {
        render(&mut app, &ctx, frame == 0, vec![]).0.drop_without_applying_deltas();
    }
    app.connect.account.as_mut().unwrap().plan.entitlements.push(Entitlement {
        id: "playback.handoff".into(),
        available: true,
        limit: None,
    });
    let (mut output, _) = render(&mut app, &ctx, false, vec![]);
    output.textures_delta.clear();
    let tree = output.platform_output.accesskit_update.unwrap();
    let find = |label: &str| tree.nodes.iter().map(|(_, node)| node).find(|node| node.label() == Some(label)).unwrap();
    assert!(!find("Continue here from Linux").is_disabled());
    assert!(find("Continue here from Studio Mac").is_disabled());
    assert!(find("Continue here from Office").is_disabled());
    assert!(tree.nodes.iter().any(|(_, node)| node.label() == Some("Play on Linux")));
    assert!(!tree.nodes.iter().any(|(_, node)| node.label() == Some("Explore Connect")));
}

#[test]
fn picker_supports_keyboard_opening_navigation_and_escape_focus_restoration() {
    let mut app = fixture();
    let ctx = setup(&app, false, 1.0);
    let (output, trigger) = render(&mut app, &ctx, false, vec![]);
    output.drop_without_applying_deltas();
    ctx.memory_mut(|memory| memory.request_focus(trigger));
    for events in [vec![key(egui::Key::Enter)], vec![key(egui::Key::ArrowDown)], vec![]] {
        render(&mut app, &ctx, false, events).0.drop_without_applying_deltas();
    }
    assert!(egui::Popup::is_id_open(
        &ctx,
        egui::Popup::default_response_id(&ctx.read_response(trigger).unwrap())
    ));
    assert_ne!(ctx.memory(|memory| memory.focused()), Some(trigger));
    assert!(widgets::keyboard_navigation(&ctx));
    render(&mut app, &ctx, false, vec![key(egui::Key::Escape)]).0.drop_without_applying_deltas();
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(trigger));
}

#[test]
fn free_plan_action_opens_the_current_instances_plans_with_the_keyboard() {
    let mut app = fixture();
    let ctx = setup(&app, false, 1.0);
    let (output, trigger) = render(&mut app, &ctx, false, vec![]);
    output.drop_without_applying_deltas();
    ctx.memory_mut(|memory| memory.request_focus(trigger));
    for events in [vec![key(egui::Key::Enter)], vec![key(egui::Key::ArrowDown)], vec![]] {
        render(&mut app, &ctx, false, events).0.drop_without_applying_deltas();
    }
    let (mut output, _) = render(&mut app, &ctx, false, vec![key(egui::Key::Enter)]);
    output.textures_delta.clear();
    assert!(output.platform_output.commands.iter().any(|command| matches!(command,
        egui::OutputCommand::OpenUrl(url) if url.url == app.connect.origin.url("/plans").as_str())));
    assert!(!egui::Popup::is_any_open(&ctx));
}

#[test]
fn device_action_identity_is_preserved_when_another_device_appears() {
    l10n::set_language(Language::English);
    let mut app = fixture();
    app.connect.account.as_mut().unwrap().plan.entitlements.push(Entitlement {
        id: "playback.handoff".into(),
        available: true,
        limit: None,
    });
    let ctx = setup(&app, false, 1.0);
    let mut before = None;
    for frame in 0..3 {
        let (mut output, _) = render(&mut app, &ctx, frame == 0, vec![]);
        output.textures_delta.clear();
        before = output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .into_iter()
            .find(|(_, node)| node.label() == Some("Continue here from Linux"))
            .map(|(id, _)| id);
    }
    app.connect.services.devices.insert(
        0,
        PairedDevice {
            id: Uuid::new_v4(),
            name: "New desktop".into(),
            online: true,
            suspended: false,
            state: None,
        },
    );
    let (mut output, _) = render(&mut app, &ctx, false, vec![]);
    output.textures_delta.clear();
    let after = output
        .platform_output
        .accesskit_update
        .unwrap()
        .nodes
        .into_iter()
        .find(|(_, node)| node.label() == Some("Continue here from Linux"))
        .map(|(id, _)| id);
    assert!(before.is_some());
    assert_eq!(before, after, "a device refresh must not move focus to a different destination");
}

#[test]
fn long_device_names_and_many_devices_keep_footer_actions_inside_viewport_at_large_text_sizes() {
    for zoom in [1.0, 1.5, 2.0] {
        let mut app = fixture();
        let long_name = "Studio con un nome molto lungo, accenti è e caratteri 日本語".repeat(3);
        app.connect.services.devices[0].name = long_name.clone();
        for index in 0..30 {
            app.connect.services.devices.push(PairedDevice {
                id: Uuid::new_v4(),
                name: format!("Computer {index}"),
                online: false,
                suspended: false,
                state: None,
            });
        }
        let ctx = setup(&app, false, zoom);
        let mut tree = None;
        for frame in 0..3 {
            let (mut output, _) = render(&mut app, &ctx, frame == 0, vec![]);
            output.textures_delta.clear();
            tree = output.platform_output.accesskit_update;
        }
        let tree = tree.unwrap();
        assert!(
            tree.nodes.iter().any(|(_, node)| node.value() == Some(&long_name)),
            "keep full device names available to assistive technology"
        );
        for label in [t("Explore Connect"), t("Manage playback devices")] {
            let node = tree.nodes.iter().map(|(_, node)| node).find(|node| node.label() == Some(&label)).unwrap();
            let bounds = node.bounds().unwrap();
            assert!(
                bounds.x0 >= 0.0 && bounds.y0 >= 0.0 && bounds.x1 <= 900.0 / zoom as f64 && bounds.y1 <= 600.0 / zoom as f64,
                "zoom {zoom}, {label}: {bounds:?}"
            );
            assert!(bounds.width() <= widgets::ACCOUNT_MENU_WIDTH as f64);
        }
    }
}

#[test]
fn picker_text_and_focus_colors_meet_minimum_contrast_in_both_themes() {
    fn luminance(color: egui::Color32) -> f32 {
        let linear = |channel: u8| {
            let value = channel as f32 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
    }
    for palette in [Palette::light(), Palette::dark()] {
        for background in [palette.surface, palette.surface_raised] {
            for foreground in [palette.text, palette.secondary, palette.accent] {
                let (a, b) = (luminance(foreground), luminance(background));
                assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5);
            }
        }
    }
}
