//! Game audio: one output with Voices, SFX and Music volumes under a master volume.
//!
//! Gibberish voices. Speech bubbles are synthesized on the fly (no audio assets): each syllable is
//! a consonant (noise burst, plosive click or nasal hum) followed by a vowel made from a glottal
//! pulse train run through two formant resonators. Pitch comes from life stage plus a per-peon
//! offset; '!' raises it and '?' bends the end upward.

use std::collections::HashMap;
use std::io::Cursor;
use std::num::NonZero;
use std::sync::Arc;

use rodio::Source;
use serde::{Deserialize, Serialize};

use sim::Entity;
use sim::data::LifeStage;

pub const SAMPLE_RATE: u32 = 22_050;

#[derive(Clone, Copy, Debug)]
pub struct Voice {
    /// Fundamental frequency in Hz.
    pub pitch: f32,
    /// Syllables per second multiplier (1.0 = normal).
    pub speed: f32,
    /// Vibrato depth (elders wobble).
    pub wobble: f32,
}

impl Voice {
    /// A stable voice for a peon: stage sets the range, the entity index picks a spot in it.
    pub fn for_peon(e: Entity, stage: LifeStage) -> Voice {
        let h = (e.index().wrapping_mul(2_654_435_761) >> 16) as f32 / 65_536.0; // 0..1
        let (lo, hi) = match stage {
            LifeStage::Infant => (380.0, 460.0),
            LifeStage::Toddler => (320.0, 390.0),
            LifeStage::Child => (260.0, 320.0),
            LifeStage::Teen => (170.0, 260.0),
            LifeStage::YoungAdult | LifeStage::Adult => (105.0, 230.0),
            LifeStage::Elder => (100.0, 200.0),
        };
        Voice {
            pitch: lo + (hi - lo) * h,
            speed: match stage {
                LifeStage::Infant | LifeStage::Toddler => 0.8,
                LifeStage::Elder => 0.85,
                LifeStage::Child => 1.15,
                _ => 1.0,
            },
            wobble: if stage == LifeStage::Elder { 0.05 } else { 0.012 },
        }
    }
}

/// Formants (F1, F2) for the five vowels.
fn formants(v: char) -> (f32, f32) {
    match v {
        'a' => (750.0, 1200.0),
        'e' => (500.0, 1900.0),
        'i' => (300.0, 2300.0),
        'o' => (480.0, 850.0),
        'u' => (330.0, 900.0),
        _ => (550.0, 1500.0),
    }
}

/// Two-pole resonator (band-pass) at `freq` with bandwidth `bw`.
struct Resonator {
    a1: f32,
    a2: f32,
    gain: f32,
    y1: f32,
    y2: f32,
}

impl Resonator {
    fn new(freq: f32, bw: f32, sr: f32) -> Self {
        let r = (-std::f32::consts::PI * bw / sr).exp();
        let theta = std::f32::consts::TAU * freq / sr;
        Resonator { a1: 2.0 * r * theta.cos(), a2: -r * r, gain: 1.0 - r, y1: 0.0, y2: 0.0 }
    }
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.gain * x + self.a1 * self.y1 + self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Tiny deterministic noise source (no rand dependency, same text → same sound).
struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Cons {
    Hiss,
    Plosive,
    Nasal,
    Liquid,
}

fn consonant_kind(c: char) -> Option<Cons> {
    match c {
        's' | 'z' | 'f' | 'v' | 'h' | 'x' | 'c' | 'j' => Some(Cons::Hiss),
        'p' | 't' | 'k' | 'b' | 'd' | 'g' | 'q' => Some(Cons::Plosive),
        'm' | 'n' => Some(Cons::Nasal),
        'l' | 'r' | 'w' | 'y' => Some(Cons::Liquid),
        _ => None,
    }
}

/// Splits gibberish into (consonant, vowel) syllables; letters other than a/e/i/o/u are consonants.
fn syllables(text: &str) -> Vec<(Option<Cons>, char)> {
    let mut out = Vec::new();
    let mut pending = None;
    for ch in text.to_lowercase().chars() {
        if "aeiou".contains(ch) {
            out.push((pending.take(), ch));
        } else if let Some(k) = consonant_kind(ch) {
            pending = Some(k);
        } else if ch == ' ' && pending.is_some() {
            out.push((pending.take(), 'e'));
        }
    }
    if let Some(p) = pending {
        out.push((Some(p), 'e'));
    }
    out.truncate(14);
    out
}

/// Renders a line of gibberish to mono samples at [`SAMPLE_RATE`].
pub fn synth(text: &str, voice: Voice) -> Vec<f32> {
    let sr = SAMPLE_RATE as f32;
    let syl = syllables(text);
    if syl.is_empty() {
        return Vec::new();
    }
    let excited = text.contains('!');
    let question = text.contains('?');
    let seed = text.bytes().fold(2_166_136_261u32, |h, b| (h ^ b as u32).wrapping_mul(16_777_619)) | 1;
    let mut noise = Noise(seed);
    let mut out = Vec::new();
    let syl_len = (0.13 / voice.speed * sr) as usize;
    let n = syl.len();
    let mut phase = 0.0f32;
    for (k, (cons, vowel)) in syl.iter().enumerate() {
        // intonation: a gentle declination, an upturn for questions, a lift for exclamations
        let t = k as f32 / n.max(1) as f32;
        let mut f0 = voice.pitch * (1.08 - 0.15 * t) * if excited { 1.25 } else { 1.0 };
        if question && k + 2 >= n {
            f0 *= 1.0 + 0.35 * (k + 2 - n + 1) as f32 / 2.0;
        }
        // per-syllable wiggle so it sounds like words, not a beep
        f0 *= 1.0 + noise.next() * 0.06;
        let (f1, f2) = formants(*vowel);
        let mut r1 = Resonator::new(f1, 90.0, sr);
        let mut r2 = Resonator::new(f2, 120.0, sr);
        let cons_len = match cons {
            Some(Cons::Hiss) => syl_len * 2 / 5,
            Some(Cons::Plosive) => syl_len / 8,
            Some(Cons::Nasal) | Some(Cons::Liquid) => syl_len / 4,
            None => 0,
        };
        for i in 0..cons_len {
            let env = (i as f32 / cons_len as f32 * std::f32::consts::PI).sin();
            let s = match cons.unwrap() {
                Cons::Hiss => noise.next() * 0.18 * env,
                Cons::Plosive => noise.next() * 0.45 * (1.0 - i as f32 / cons_len as f32).powi(3),
                Cons::Nasal => (phase * std::f32::consts::TAU).sin() * 0.22 * env,
                Cons::Liquid => (phase * std::f32::consts::TAU).sin() * 0.12 * env,
            };
            phase = (phase + f0 / sr).fract();
            out.push(s);
        }
        let vlen = syl_len;
        for i in 0..vlen {
            let tt = i as f32 / sr;
            let vib = 1.0 + voice.wobble * (tt * 31.0).sin();
            phase += f0 * vib / sr;
            // glottal pulse: a sawtooth-ish ramp with a sharp reset is rich in harmonics
            let pulse = if phase >= 1.0 {
                phase -= 1.0;
                1.0
            } else {
                0.0
            };
            let src = pulse * 1.0 - 0.02 + noise.next() * 0.01;
            let v = r1.tick(src) * 1.6 + r2.tick(src) * 0.9;
            let a = (i as f32 / (vlen as f32 * 0.12)).min(1.0);
            let r = ((vlen - i) as f32 / (vlen as f32 * 0.35)).min(1.0);
            out.push(v * a * r * 2.2);
        }
        // a short gap between words
        if k % 3 == 2 {
            out.extend(std::iter::repeat_n(0.0, syl_len / 4));
        }
    }
    // normalize to a comfortable level
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    let g = 0.5 / peak;
    for s in &mut out {
        *s *= g;
    }
    out
}

/// Encoded sound file (Ogg Vorbis or WAV), shared between plays.
pub type Clip = Arc<[u8]>;

/// Player volume settings, kept in `settings.ron` next to `saves/`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Volumes {
    pub master: f32,
    pub voices: f32,
    pub sfx: f32,
    pub music: f32,
}

impl Default for Volumes {
    fn default() -> Self {
        Volumes { master: 0.8, voices: 0.75, sfx: 0.8, music: 0.5 }
    }
}

impl Volumes {
    pub const FILE: &'static str = "settings.ron";

    pub fn load() -> Volumes {
        std::fs::read_to_string(Self::FILE).ok().and_then(|s| ron::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = ron::ser::to_string_pretty(self, Default::default())
            && let Err(e) = std::fs::write(Self::FILE, s)
        {
            log::warn!("couldn't save {}: {e}", Self::FILE);
        }
    }
}

/// Seconds for a loop or music track to fade in or out.
const LOOP_FADE: f32 = 0.4;
const MUSIC_FADE: f32 = 2.0;

/// A playing loop (or music track) with its own player so it can be faded and stopped.
struct Fader {
    player: rodio::Player,
    gain: f32,
    level: f32,
    /// Fading out; dropped at silence.
    leaving: bool,
}

impl Fader {
    fn new(sink: &rodio::MixerDeviceSink, clip: &Clip, gain: f32) -> Option<Fader> {
        let dec = rodio::Decoder::new_looped(Cursor::new(clip.clone()))
            .map_err(|e| log::warn!("bad sound clip: {e}"))
            .ok()?;
        let player = rodio::Player::connect_new(sink.mixer());
        player.set_volume(0.0);
        player.append(dec);
        Some(Fader { player, gain, level: 0.0, leaving: false })
    }

    /// Steps the fade; false once it has faded out.
    fn step(&mut self, dt: f32, fade: f32, bus: f32) -> bool {
        let d = dt / fade;
        self.level = if self.leaving { self.level - d } else { self.level + d }.clamp(0.0, 1.0);
        self.player.set_volume(self.level * self.gain * bus);
        !(self.leaving && self.level <= 0.0)
    }
}

/// Plays voices, sound effects, looping sounds and music on the default output device.
/// Silently does nothing without one.
pub struct Audio {
    sink: Option<rodio::MixerDeviceSink>,
    pub volumes: Volumes,
    /// Everything silent (M).
    pub muted: bool,
    /// World sounds (voices, loops) silent; music keeps playing.
    pub paused: bool,
    /// Last spoken line per peon, so each bubble is voiced once.
    said: HashMap<Entity, String>,
    budget: f32,
    loops: HashMap<u64, Fader>,
    music: Vec<(Clip, Fader)>,
}

impl Audio {
    pub fn new() -> Audio {
        let sink = match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut s) => {
                s.log_on_drop(false);
                Some(s)
            }
            Err(e) => {
                log::warn!("no audio output: {e}");
                None
            }
        };
        Audio { sink, ..Self::disabled() }
    }

    /// A silent instance that never touches the audio device (tests, screenshots).
    pub fn disabled() -> Audio {
        Audio {
            sink: None,
            volumes: Volumes::default(),
            muted: false,
            paused: false,
            said: HashMap::new(),
            budget: 3.0,
            loops: HashMap::new(),
            music: Vec::new(),
        }
    }

    pub fn available(&self) -> bool {
        self.sink.is_some()
    }

    fn bus(&self, bus: f32, world: bool) -> f32 {
        if self.muted || (world && self.paused) { 0.0 } else { self.volumes.master * bus }
    }

    /// Call once per frame with every visible speaker: (peon, stage, line, loudness 0..1).
    /// New lines are voiced, at most a few per second so ultra speed doesn't turn into a choir.
    pub fn update(&mut self, dt: f32, speakers: &[(Entity, LifeStage, String, f32)]) {
        self.budget = (self.budget + dt * 2.0).min(3.0);
        self.said.retain(|e, _| speakers.iter().any(|s| s.0 == *e));
        let silent = self.bus(self.volumes.voices, true) <= 0.0;
        for (e, stage, line, loud) in speakers {
            if self.said.get(e) == Some(line) {
                continue;
            }
            self.said.insert(*e, line.clone());
            if silent || self.budget < 1.0 || *loud <= 0.05 {
                continue;
            }
            self.budget -= 1.0;
            self.say(line, Voice::for_peon(*e, *stage), *loud);
        }
    }

    pub fn say(&self, line: &str, voice: Voice, loudness: f32) {
        let Some(sink) = &self.sink else { return };
        let mut samples = synth(line, voice);
        let g = self.bus(self.volumes.voices, true) * loudness.clamp(0.0, 1.0);
        for s in &mut samples {
            *s *= g;
        }
        if samples.is_empty() {
            return;
        }
        let buf = rodio::buffer::SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(SAMPLE_RATE).unwrap(), samples);
        sink.mixer().add(buf);
    }

    /// Plays a one-shot sound effect at `gain` (0..1, e.g. by camera distance).
    pub fn play(&self, clip: &Clip, gain: f32) {
        let Some(sink) = &self.sink else { return };
        let g = self.bus(self.volumes.sfx, false) * gain.clamp(0.0, 1.0);
        if g <= 0.01 {
            return;
        }
        match rodio::Decoder::new(Cursor::new(clip.clone())) {
            Ok(dec) => sink.mixer().add(dec.amplify(g)),
            Err(e) => log::warn!("bad sound clip: {e}"),
        }
    }

    /// Sets the looping sounds that should be playing this frame: (key, clip, gain). Keys are
    /// stable per source (e.g. a peon's entity bits); new keys fade in, missing ones fade out.
    pub fn update_loops(&mut self, dt: f32, wanted: &[(u64, Clip, f32)]) {
        if let Some(sink) = &self.sink {
            for (key, clip, gain) in wanted {
                match self.loops.get_mut(key) {
                    Some(f) => {
                        f.gain = *gain;
                        f.leaving = false;
                    }
                    None => {
                        if let Some(f) = Fader::new(sink, clip, *gain) {
                            self.loops.insert(*key, f);
                        }
                    }
                }
            }
        }
        for (key, f) in &mut self.loops {
            if !wanted.iter().any(|w| w.0 == *key) {
                f.leaving = true;
            }
        }
        let bus = self.bus(self.volumes.sfx, true);
        self.loops.retain(|_, f| f.step(dt, LOOP_FADE, bus));
    }

    /// Cross-fades to `track` (None = silence). Calling again with the same track does nothing.
    pub fn update_music(&mut self, dt: f32, track: Option<&Clip>) {
        let current = self.music.iter().find(|(_, f)| !f.leaving).map(|(c, _)| c.clone());
        if current.as_ref().map(|c| c.as_ptr()) != track.map(|c| c.as_ptr()) {
            for (_, f) in &mut self.music {
                f.leaving = true;
            }
            if let (Some(sink), Some(clip)) = (&self.sink, track) {
                if let Some(i) = self.music.iter().position(|(c, _)| Arc::ptr_eq(c, clip)) {
                    self.music[i].1.leaving = false;
                } else if let Some(f) = Fader::new(sink, clip, 1.0) {
                    self.music.push((clip.clone(), f));
                }
            }
        }
        let bus = self.bus(self.volumes.music, false);
        self.music.retain_mut(|(_, f)| f.step(dt, MUSIC_FADE, bus));
    }

    /// Number of loops and music tracks currently held (playing or fading).
    pub fn active(&self) -> (usize, usize) {
        (self.loops.len(), self.music.len())
    }
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_crossings(s: &[f32]) -> usize {
        s.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count()
    }

    #[test]
    fn gibberish_is_audible_and_bounded() {
        let v = Voice { pitch: 150.0, speed: 1.0, wobble: 0.01 };
        let s = synth("Blorp zib nana!", v);
        assert!(s.len() > SAMPLE_RATE as usize / 4, "at least a quarter second");
        assert!(s.len() < SAMPLE_RATE as usize * 4);
        let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.3 && peak <= 0.5 + 1e-4, "{peak}");
        assert!(s.iter().all(|x| x.is_finite()));
        // same text, same sound
        assert_eq!(s, synth("Blorp zib nana!", v));
    }

    #[test]
    fn longer_lines_take_longer() {
        let v = Voice { pitch: 150.0, speed: 1.0, wobble: 0.0 };
        assert!(synth("ba ba ba ba ba ba", v).len() > synth("ba", v).len() * 3);
        assert!(synth("", v).is_empty());
        assert!(synth("...", v).is_empty());
    }

    #[test]
    fn babies_squeak_higher_than_adults() {
        let e = Entity::from_bits(5);
        let baby = Voice::for_peon(e, LifeStage::Infant);
        let adult = Voice::for_peon(e, LifeStage::Adult);
        assert!(baby.pitch > adult.pitch * 1.5);
        // the higher voice has more zero crossings in the same line (rough pitch estimate)
        let a = synth("aaaa", Voice { speed: 1.0, wobble: 0.0, ..adult });
        let b = synth("aaaa", Voice { speed: 1.0, wobble: 0.0, ..baby });
        let rate = |s: &[f32]| zero_crossings(s) as f32 / s.len() as f32;
        assert!(rate(&b) > rate(&a), "{} vs {}", rate(&b), rate(&a));
    }

    #[test]
    fn volumes_round_trip_and_fill_missing_fields() {
        let v = Volumes { master: 0.3, voices: 0.1, sfx: 0.9, music: 0.0 };
        let s = ron::to_string(&v).unwrap();
        assert_eq!(ron::from_str::<Volumes>(&s).unwrap(), v);
        let partial: Volumes = ron::from_str("(music: 0.2)").unwrap();
        assert_eq!(partial, Volumes { music: 0.2, ..Volumes::default() });
    }

    #[test]
    fn muting_and_pausing_silence_the_right_buses() {
        let mut a = Audio::disabled();
        assert!(a.bus(1.0, true) > 0.0);
        a.paused = true;
        assert_eq!(a.bus(1.0, true), 0.0, "world sounds stop while paused");
        assert!(a.bus(1.0, false) > 0.0, "music keeps playing while paused");
        a.muted = true;
        assert_eq!(a.bus(1.0, false), 0.0);
    }

    #[test]
    fn without_a_device_nothing_is_held() {
        let mut a = Audio::disabled();
        let clip: Clip = Arc::from(&b"not a sound"[..]);
        a.play(&clip, 1.0);
        a.update_loops(0.1, &[(1, clip.clone(), 1.0)]);
        a.update_music(0.1, Some(&clip));
        assert_eq!(a.active(), (0, 0));
    }

    /// 16-bit mono PCM WAV of `n` samples of a 440 Hz tone.
    pub(crate) fn wav(n: u32) -> Vec<u8> {
        let mut b = Vec::new();
        let data = n * 2;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        b.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        for i in 0..n {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32).sin() * 8000.0;
            b.extend_from_slice(&(v as i16).to_le_bytes());
        }
        b
    }

    #[test]
    fn wav_clips_decode_and_loop() {
        let clip: Clip = Arc::from(wav(100));
        let once = rodio::Decoder::new(Cursor::new(clip.clone())).unwrap().count();
        assert_eq!(once, 100);
        let looped = rodio::Decoder::new_looped(Cursor::new(clip)).unwrap().take(250).count();
        assert_eq!(looped, 250, "a looped clip keeps going");
    }
}
