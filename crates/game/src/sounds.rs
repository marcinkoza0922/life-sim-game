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

macro_rules! music {
    ($($id:literal),* $(,)?) => {
        /// Every embedded music loop id (`assets/audio/music/`, Kenney "Music Loops", CC0).
        pub const MUSIC: &[&str] = &[$($id),*];
        fn music_table() -> HashMap<&'static str, Clip> {
            HashMap::from([$(($id, Arc::from(&include_bytes!(concat!("../../../assets/audio/music/", $id, ".ogg"))[..]))),*])
        }
    };
}

music![
    "swinging_pants", "italian_mom", "mischief_stroll", "cheerful_annoyance", "farm_frolics", "polka_train",
    "night_at_the_beach", "flowing_rocks", "space_cadet", "drumming_sticks", "mission_plausible", "time_driving",
    "wacky_waiting",
];

/// The clip for a music loop id.
pub fn track(id: &str) -> Option<Clip> {
    static TABLE: OnceLock<HashMap<&'static str, Clip>> = OnceLock::new();
    TABLE.get_or_init(music_table).get(id).cloned()
}

/// What the music should suit right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Day,
    Night,
    Build,
    Creator,
}

impl Scene {
    pub fn playlist(self) -> &'static [&'static str] {
        match self {
            Scene::Day => &["swinging_pants", "italian_mom", "mischief_stroll", "cheerful_annoyance", "farm_frolics", "polka_train"],
            Scene::Night => &["night_at_the_beach", "flowing_rocks", "space_cadet"],
            Scene::Build => &["drumming_sticks", "mission_plausible", "time_driving", "mischief_stroll"],
            Scene::Creator => &["wacky_waiting", "cheerful_annoyance"],
        }
    }
}

/// Picks the music: each scene has a playlist; a track loops for a minute or so, then the next
/// one cross-fades in, with an occasional quiet break so the short loops don't wear thin.
/// Changing scene (Live → Build, nightfall) switches playlist at once.
pub struct Jukebox {
    scene: Option<Scene>,
    index: usize,
    playing: Option<&'static str>,
    /// Seconds left on the current track, or of the current break.
    left: f32,
    rng: u32,
}

impl Default for Jukebox {
    fn default() -> Self {
        Jukebox { scene: None, index: 0, playing: None, left: 0.0, rng: 0x9e37_79b9 }
    }
}

impl Jukebox {
    pub const TRACK_SECS: (f32, f32) = (60.0, 100.0);
    pub const BREAK_SECS: (f32, f32) = (30.0, 90.0);
    pub const BREAK_CHANCE: f32 = 0.3;

    fn roll(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    fn between(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + (hi - lo) * self.roll()
    }

    fn next_track(&mut self, scene: Scene) {
        let list = scene.playlist();
        self.index = (self.index + 1) % list.len();
        self.playing = Some(list[self.index]);
        self.left = self.between(Self::TRACK_SECS);
    }

    /// Advances by `dt` real seconds in `scene`; returns the track that should be playing.
    pub fn update(&mut self, dt: f32, scene: Scene) -> Option<&'static str> {
        if self.scene != Some(scene) {
            self.scene = Some(scene);
            self.index = (self.roll() * scene.playlist().len() as f32) as usize;
            self.next_track(scene);
            return self.playing;
        }
        self.left -= dt;
        if self.left <= 0.0 {
            if self.playing.is_some() && self.roll() < Self::BREAK_CHANCE {
                self.playing = None;
                self.left = self.between(Self::BREAK_SECS);
            } else {
                self.next_track(scene);
            }
        }
        self.playing
    }
}

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

/// Gain for a looping sound at `dist` metres from the camera. Fiddly little sounds (typing,
/// scrubbing, stirring) are quiet and only carry when you're close; the rest use [`falloff`].
pub fn loop_gain(id: &str, dist: f32) -> f32 {
    let close = |level: f32, full: f32, gone: f32| level * (1.0 - (dist - full) / (gone - full)).clamp(0.0, 1.0);
    match id {
        "type" => close(0.45, 3.0, 12.0),
        "scrub" | "stir" => close(0.6, 4.0, 20.0),
        "snore" => close(0.5, 4.0, 24.0),
        _ => falloff(dist),
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
    fn every_music_track_decodes_and_is_in_a_playlist() {
        for id in MUSIC {
            let c = track(id).unwrap();
            assert!(rodio::Decoder::new(std::io::Cursor::new(c)).is_ok(), "{id}");
            let used = [Scene::Day, Scene::Night, Scene::Build, Scene::Creator].iter().any(|s| s.playlist().contains(id));
            assert!(used, "{id} is in no playlist");
        }
        for s in [Scene::Day, Scene::Night, Scene::Build, Scene::Creator] {
            assert!(s.playlist().iter().all(|id| track(id).is_some()));
        }
    }

    #[test]
    fn jukebox_switches_on_scene_change_and_rotates_with_breaks() {
        let mut j = Jukebox::default();
        let first = j.update(0.1, Scene::Day).unwrap();
        assert!(Scene::Day.playlist().contains(&first));
        assert_eq!(j.update(1.0, Scene::Day), Some(first), "keeps playing");
        let b = j.update(0.1, Scene::Build).unwrap();
        assert!(Scene::Build.playlist().contains(&b), "switches at once");
        // over an hour of Build: several tracks, some breaks, never silent for long
        let (mut tracks, mut silent, mut longest) = (std::collections::HashSet::new(), 0.0, 0.0f32);
        for _ in 0..36_000 {
            match j.update(0.1, Scene::Build) {
                Some(t) => {
                    tracks.insert(t);
                    silent = 0.0;
                }
                None => {
                    silent += 0.1;
                    longest = longest.max(silent);
                }
            }
        }
        assert_eq!(tracks.len(), Scene::Build.playlist().len());
        assert!(longest > 0.0 && longest <= Jukebox::BREAK_SECS.1 + 0.2, "{longest}");
    }

    #[test]
    fn typing_is_only_loud_up_close() {
        assert!(loop_gain("type", 2.0) > 0.4);
        assert!(loop_gain("type", 10.0) < 0.15);
        assert_eq!(loop_gain("type", 20.0), 0.0);
        assert!(loop_gain("tv", 20.0) > 0.5, "the TV still carries");
    }

    #[test]
    fn loops_come_from_clip_unless_overridden() {
        assert_eq!(loop_for(None, Some("chop")), Some("chop"));
        assert_eq!(loop_for(Some("faucet"), Some("scrub")), Some("faucet"));
        assert_eq!(loop_for(Some(""), Some("scrub")), None);
        assert_eq!(loop_for(None, Some("fix")), None);
    }
}
