//! How an automatic advance joins two items, shared by the native and web outputs.

use super::{ContentKind, MediaItem, TransitionSettings};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Plan {
    /// Start the next item after the current one ends.
    Sequential,
    /// Prepare the next item so it starts without a pause.
    Gapless,
    Crossfade(f64),
}

/// How an automatic advance from `current` joins `next` (the README's transition rules).
pub fn plan(current: &MediaItem, current_duration: Option<f64>, next: &MediaItem, settings: &TransitionSettings) -> Plan {
    if current.kind == ContentKind::Radio || next.kind == ContentKind::Radio {
        return Plan::Sequential;
    }
    let gapless = if settings.gapless { Plan::Gapless } else { Plan::Sequential };
    let Some(fade) = settings.crossfade.map(f64::from).filter(|f| f.is_finite() && *f > 0.0) else { return gapless };
    let fade = fade.clamp(1.0, 12.0);
    let music = |item: &MediaItem| item.kind == ContentKind::Music;
    let repeat_one = current.id == next.id;
    let same_album = current.album_key.is_some() && current.album_key == next.album_key;
    let long_enough = current_duration.or(current.duration_hint).is_some_and(|d| d.is_finite() && d >= fade * 3.0);
    let next_long_enough = next.duration_hint.is_none_or(|d| d >= fade * 2.0);
    if music(current) && music(next) && !repeat_one && !(same_album && settings.keep_album_gapless) && long_enough && next_long_enough {
        Plan::Crossfade(fade)
    } else {
        gapless
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioSource;

    fn item(id: &str, seconds: f64) -> MediaItem {
        MediaItem {
            id: id.into(),
            source: AudioSource::Url(format!("memory://{id}")),
            kind: ContentKind::Music,
            duration_hint: Some(seconds),
            replay_gain: None,
            album_key: None,
            server_format: None,
        }
    }

    fn settings(gapless: bool, crossfade: Option<f32>, keep_album_gapless: bool) -> TransitionSettings {
        TransitionSettings { gapless, crossfade, keep_album_gapless }
    }

    #[test]
    fn plan_rules() {
        let a = item("1", 200.0);
        let b = item("2", 200.0);
        let fade = settings(true, Some(5.0), true);
        assert_eq!(plan(&a, None, &b, &settings(true, None, true)), Plan::Gapless);
        assert_eq!(plan(&a, None, &b, &settings(false, None, true)), Plan::Sequential);
        assert_eq!(plan(&a, None, &b, &fade), Plan::Crossfade(5.0));
        // Repeat One.
        assert_eq!(plan(&a, None, &a.clone(), &fade), Plan::Gapless);
        // Podcasts, audiobooks and radio.
        for kind in [ContentKind::Podcast, ContentKind::Audiobook] {
            let mut p = b.clone();
            p.kind = kind;
            assert_eq!(plan(&a, None, &p, &fade), Plan::Gapless);
            assert_eq!(plan(&p, None, &a, &fade), Plan::Gapless);
        }
        let mut radio = b.clone();
        radio.kind = ContentKind::Radio;
        assert_eq!(plan(&a, None, &radio, &fade), Plan::Sequential);
        // Shorter than three fades, or the next shorter than two.
        assert_eq!(plan(&item("1", 14.0), None, &b, &fade), Plan::Gapless);
        assert_eq!(plan(&item("1", 15.0), None, &b, &fade), Plan::Crossfade(5.0));
        assert_eq!(plan(&a, Some(10.0), &b, &fade), Plan::Gapless);
        assert_eq!(plan(&a, None, &item("2", 9.0), &fade), Plan::Gapless);
        // Same album.
        let (mut x, mut y) = (a.clone(), b.clone());
        x.album_key = Some("album".into());
        y.album_key = Some("album".into());
        assert_eq!(plan(&x, None, &y, &fade), Plan::Gapless);
        assert_eq!(plan(&x, None, &y, &settings(true, Some(5.0), false)), Plan::Crossfade(5.0));
        assert_eq!(plan(&x, None, &y, &settings(false, Some(5.0), true)), Plan::Sequential);
    }
}
