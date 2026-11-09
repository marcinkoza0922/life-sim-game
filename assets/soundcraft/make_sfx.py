#!/usr/bin/env python3
"""Sound effects with no good CC0 recording, built headless with SoundCraft and encoded to Ogg.

Downloaded CC0 sounds come first (`assets/audio/fetch_sfx.py`); this only builds the rest.

Each sound is a small SoundCraft session: synth/drum notes (MIDI on built-in instruments) and
generated noise, shaped with inserts and fades. For every entry in SOUNDS this writes
`<work>/<id>.txt` (the engine command script), bounces `<work>/<id>.wav`, saves the session
to `<work>/sessions/<id>.scraft` (open it in SoundCraft to audition or tweak), then normalises to mono 44.1 kHz and encodes
`assets/audio/sfx/<id>.ogg`.

    python3 assets/soundcraft/make_sfx.py            # all sounds
    python3 assets/soundcraft/make_sfx.py door_open  # just these

Loops (ids in LOOPS) are cut to exact length with no fades so they repeat seamlessly; keep
their content steady or rhythmic on the loop length. Needs `soundcraft-cli`, `sox`, `ffmpeg`.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
WORK = Path.home() / "CreativeWork/life-sim-game/soundcraft/sfx"
SESSIONS = WORK / "sessions"  # reproducible from this script, so not checked in
OUT = REPO / "assets/audio/sfx"
SR = 44100
TICKS_PER_SEC = 1920  # 960 PPQ at the session's default 120 bpm

# GM drum notes on the built-in drum_synth
KICK, SNARE, HAT, OPEN_HAT, TOM_LO, TOM_MID, TOM_HI, CYMBAL = 36, 38, 42, 46, 45, 47, 50, 49

# Plugin choice indices
WAVES = ["Saw", "Square", "Sine", "Triangle"]  # subtractive_synth osc1_wave/osc2_wave
NOISE_WAVES = ["Sine", "Square", "Saw", "Triangle", "White Noise", "Pink Noise"]  # signal_generator
# Units (from `soundcraft-cli plugins`): times in ms, mix/resonance/sustain/width in %, tunes in
# semitones, detune in cents, levels in dB.
LOW_PASS, HIGH_PASS, BAND_PASS, PEAK = 4, 3, 6, 0


class Sound:
    """Builds one session's command script, predicting the ids SoundCraft assigns."""

    def __init__(self, length):
        self.length = length
        self.cmds = [("session.new", {"name": "sfx", "sample_rate": SR})]
        self.next_id = 1
        self.n = 0

    def _id(self):
        i = self.next_id
        self.next_id += 1
        return i

    def _name(self, kind):
        self.n += 1
        return f"{kind}{self.n}"

    def fx(self, track, *inserts):
        for slot, (plugin, params) in enumerate(inserts):
            self.cmds.append(("mix.insert", {"track": track, "slot": slot, "plugin": plugin, "params": params}))

    def notes(self, instrument, notes, params=None, fx=(), db=0.0):
        """notes: [(pitch, start_s, len_s, velocity)]."""
        t = self._name("i")
        self.cmds.append(("track.new", {"kind": "instrument", "name": t, "instrument": instrument}))
        self._id()
        if params:
            params = {k: WAVES.index(v) if k.endswith("_wave") else v for k, v in params.items()}
            self.cmds.append(("mix.instrument", {"track": t, "plugin": instrument, "params": params}))
        self.cmds.append(("midi.new_clip", {"track": t, "start": {"seconds": 0}, "length": {"seconds": self.length}}))
        clip = self._id()
        for pitch, start, length, vel in notes:
            self.cmds.append(("midi.note_add", {"clip": clip, "pitch": pitch, "start_ticks": round(start * TICKS_PER_SEC),
                                                "length_ticks": max(1, round(length * TICKS_PER_SEC)), "velocity": vel}))
        self.fx(t, *fx)
        if db:
            self.cmds.append(("mix.volume", {"tracks": [t], "db": db}))
        return self

    def synth(self, notes, fx=(), db=0.0, **params):
        return self.notes("subtractive_synth", notes, params, fx, db)

    def drums(self, notes, fx=(), db=0.0, **params):
        return self.notes("drum_synth", notes, params, fx, db)

    def noise(self, start, end, fx=(), wave="Pink Noise", level=-6, fade_in=0.0, fade_out=0.0, db=0.0, freq=440):
        t = self._name("n")
        self.cmds.append(("track.new", {"kind": "audio", "name": t, "format": "mono"}))
        self._id()
        self.cmds.append(("edit.select", {"tracks": [t], "start": {"seconds": start}, "end": {"seconds": end}}))
        self.cmds.append(("audiosuite.process", {"process": "signal_generator", "tracks": [t],
                                                 "params": {"waveform": NOISE_WAVES.index(wave), "level": level, "freq": freq}}))
        self._id()  # the generated source file
        clip = self._id()
        for side, length in (("in", fade_in), ("out", fade_out)):
            if length > 0:
                self.cmds.append(("edit.fades_create", {"clip": clip, "side": side, "length": round(length * SR),
                                                        "shape": "exponential"}))
        self.fx(t, *fx)
        if db:
            self.cmds.append(("mix.volume", {"tracks": [t], "db": db}))
        return self

    def script(self):
        return "\n".join(f"{c} {json.dumps(p)}" for c, p in self.cmds) + "\n"


def _eq(hpf=None, lpf=None, peak=None):
    """eq_7band with 24 dB/oct filters (the 1-band EQ's 12 dB/oct is too gentle for noise)."""
    p = {"hpf_on": 1 if hpf else 0, "lpf_on": 1 if lpf else 0, "hpf_slope": 3, "lpf_slope": 3}
    if hpf:
        p["hpf_freq"] = hpf
    if lpf:
        p["lpf_freq"] = lpf
    if peak:
        p.update(mid_freq=peak, mid_gain=6, mid_q=1.5)
    return ("eq_7band", p)


def lp(freq, q=None):
    return _eq(lpf=freq)


def hp(freq, q=None):
    return _eq(hpf=freq)


def bp(freq, q=1.5):
    """Band around freq: width shrinks as q grows (q 1 ≈ an octave either side)."""
    k = 2 ** (1 / q)
    return _eq(hpf=freq / k, lpf=freq * k, peak=freq)


# ---------------------------------------------------------------------------------------------
# Loops (seamless at their length)


def stir():
    # a spoon going round a pot, twice a second (auto-pan at 120 bpm-ish rate)
    return (Sound(1.0)
            .noise(0, 1.0, fx=[bp(1800, 2.5), ("auto_pan", {"rate": 120, "width": 100})], level=-10)
            .drums([(HAT, 0.1, 0.01, 40), (HAT, 0.6, 0.01, 35)], hat_decay=20))


def scrub():
    s = Sound(1.0)
    for i in range(4):
        a = i * 0.25
        s.noise(a, a + 0.2, fx=[bp(3200, 1.0)], level=-8, fade_in=0.05, fade_out=0.08)
    return s


def tv():
    # muffled, chattery programme: babbling synth through a band-pass over a low hum
    pitches = [62, 65, 60, 67, 64, 62, 69, 60, 65, 63, 67, 62]
    babble = [(p, i * 0.25, 0.18, 60 + (i * 13) % 30) for i, p in enumerate(pitches)]
    return (Sound(3.0)
            .synth(babble, osc1_wave="Saw", osc2_wave="Square", osc2_detune=20, cutoff=900, resonance=40,
                   amp_attack=20, amp_decay=100, amp_sustain=50, amp_release=50, glide=50,
                   fx=[bp(800, 0.8), ("lofi", {"bits": 10, "mix": 50}), lp(2500), lp(2500)], db=-6)
            .noise(0, 3.0, fx=[lp(500)], level=-28))


def treadmill():
    steps = [(KICK, t, 0.05, 90) for t in (0, 0.33, 0.67)]
    return (Sound(1.0)
            .drums(steps, kick_tune=5, kick_decay=70)
            .noise(0, 1.0, fx=[bp(180, 2.0)], level=-14, wave="Saw", freq=60)
            .noise(0, 1.0, fx=[bp(1500, 1.0)], level=-24))


def snore():
    return (Sound(3.0)
            .noise(0.0, 1.3, fx=[lp(500), ("ring_mod", {"effect": 0, "frequency": 45, "mix": 60})],
                   level=-8, fade_in=0.6, fade_out=0.3)
            .noise(1.5, 2.6, fx=[hp(1500), lp(4000)], level=-22, fade_in=0.3, fade_out=0.5))


SOUNDS = {
    "stir": stir,
    "scrub": scrub,
    "tv": tv,
    "treadmill": treadmill,
    "snore": snore,
}
LOOPS = set(SOUNDS)


def build(sid):
    snd = SOUNDS[sid]()
    WORK.mkdir(parents=True, exist_ok=True)
    SESSIONS.mkdir(parents=True, exist_ok=True)
    OUT.mkdir(parents=True, exist_ok=True)
    script, wav = WORK / f"{sid}.txt", WORK / f"{sid}.wav"
    script.write_text(snd.script())
    r = subprocess.run(["soundcraft-cli", "script", str(script), "--bounce", str(wav), "--start", "0",
                        "--end", str(snd.length), "--save", str(SESSIONS / f"{sid}.scraft")],
                       capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"{sid}: soundcraft failed\n{r.stdout[-2000:]}{r.stderr[-2000:]}")
    norm = WORK / f"{sid}.norm.wav"
    peak = -9 if sid in LOOPS else -3
    subprocess.run(["sox", "-q", str(wav), "-c", "1", "-r", str(SR), str(norm), "norm", str(peak)], check=True)
    ogg = OUT / f"{sid}.ogg"
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(norm), "-c:a", "libvorbis", "-q:a", "4", str(ogg)],
                   check=True)
    print(f"{sid:12} {snd.length:4.2f}s  {os.path.getsize(ogg) // 1024:3} KB")


def main():
    ids = sys.argv[1:] or list(SOUNDS)
    for sid in ids:
        if sid not in SOUNDS:
            sys.exit(f"unknown sound {sid}; have {', '.join(SOUNDS)}")
        build(sid)


if __name__ == "__main__":
    main()
