//! Sound effect clips (`assets/audio/sfx/*.ogg`, built by `assets/soundcraft/make_sfx.py`),
//! embedded in the binary, and the rules that pick them: which loop an interaction plays, and
//! which one-shot an event or a footstep gets.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::audio::Clip;

macro_rules! sfx {
    ($($id:literal),* $(,)?) => {
        /// Every embedded sound effect id.
        pub const IDS: &[&str] = &[$($id),*];
        fn table() -> HashMap<&'static str, Clip> {
            HashMap::from([$(($id, Arc::from(&include_bytes!(concat!("../../../assets/audio/sfx/", $id, ".ogg"))[..]))),*])
        }
    };
}

sfx![
    "step_a", "step_b", "door_open", "door_close", "ui_click", "ui_open", "ui_close", "notify", "place", "pickup", "sell",
    "demolish", "build_wall", "cash", "shower", "faucet", "chop", "type", "stir", "scrub", "tv", "treadmill", "snore",
];

/// The clip for a sound id, or None for an unknown id.
pub fn clip(id: &str) -> Option<Clip> {
    static TABLE: OnceLock<HashMap<&'static str, Clip>> = OnceLock::new();
    TABLE.get_or_init(table).get(id).cloned()
}

/// The loop played while performing: the interaction's own `sfx` (`""` = none), else one implied
/// by the body clip.
pub fn loop_for(sfx: Option<&str>, anim: Option<&str>) -> Option<&'static str> {
    if let Some(id) = sfx {
        return IDS.iter().copied().find(|s| *s == id);
    }
    Some(match anim? {
        "chop" => "chop",
        "stir" => "stir",
        "scrub" => "scrub",
        "type" => "type",
        "watch" | "watch_floor" => "tv",
        "shower" => "shower",
        "sleep" => "snore",
        _ => return None,
    })
}

/// The sting for a new log entry, if its kind gets one (chatty kinds stay quiet).
pub fn for_log(kind: sim::log::LogKind) -> Option<&'static str> {
    use sim::log::LogKind::*;
    match kind {
        Money => Some("cash"),
        Career | Life | Death => Some("notify"),
        Info | Funny | Social | Warning | Debug => None,
    }
}

/// Gain for a sound at `dist` metres from the camera: full within 8 m, silent past 48 m
/// (the same falloff as voices).
pub fn falloff(dist: f32) -> f32 {
    (1.0 - (dist - 8.0) / 40.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_clip_decodes() {
        for id in IDS {
            let c = clip(id).unwrap();
            let dec = rodio::Decoder::new(std::io::Cursor::new(c)).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert!(dec.take(100).count() == 100, "{id} is empty");
        }
    }

    #[test]
    fn every_interaction_sfx_exists() {
        let data = sim::data::GameData::embedded();
        for obj in &data.objects {
            for i in &obj.interactions {
                if let Some(s) = i.sfx.as_deref().filter(|s| !s.is_empty()) {
                    assert!(clip(s).is_some(), "{}:{} names unknown sfx {s}", obj.id, i.id);
                }
            }
        }
    }

    #[test]
    fn loops_come_from_clip_unless_overridden() {
        assert_eq!(loop_for(None, Some("chop")), Some("chop"));
        assert_eq!(loop_for(Some("faucet"), Some("scrub")), Some("faucet"));
        assert_eq!(loop_for(Some(""), Some("scrub")), None);
        assert_eq!(loop_for(None, Some("fix")), None);
    }
}
