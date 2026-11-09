#!/usr/bin/env python3
"""Downloads CC0 sound effects, cuts them to shape and encodes `assets/audio/sfx/<id>.ogg`.

Downloaded sounds are preferred over synthesized ones; `assets/soundcraft/make_sfx.py` only
builds what isn't listed here. Every source is pinned by URL and SHA-256, and this script
rewrites `assets/audio/CREDITS.md` from the table below.

    python3 assets/audio/fetch_sfx.py            # all
    python3 assets/audio/fetch_sfx.py faucet     # just these
    python3 assets/audio/fetch_sfx.py music      # just the music loops

Downloads are cached in ~/CreativeWork/life-sim-game/cc0/dl. One-shots get leading silence
trimmed; loops take the steadiest `loop` seconds of the recording and cross-fade the seam.
Needs `sox`, `ffmpeg` and numpy.
"""

import hashlib
import io
import os
import subprocess
import sys
import urllib.request
import wave
import zipfile
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[2]
CACHE = Path.home() / "CreativeWork/life-sim-game/cc0/dl"
OUT = REPO / "assets/audio/sfx"
SR = 44100
OGA = "https://opengameart.org/sites/default/files/"
# Kenney's All-in-1 bundle (CC0), downloaded by hand from kenney.nl (the site refuses scripts).
KENNEY = Path(os.environ.get("KENNEY_DIR", Path.home() / "Kenney Game Assets All-in-1 3.8.0"))
KENNEY_PAGE = "https://kenney.nl/assets/impact-sounds"
IMPACT = "kenney:Audio/Impact Sounds/Audio/"

# name: (url, sha256, title, author, page). A `kenney:` url is a file in the local KENNEY bundle.
SOURCES = {
    "impact_step_a": (IMPACT + "footstep_wood_000.ogg",
                      "1998aa35434a2418a44a34928131c943e7c0df5bdd011385b42a636734166e81",
                      "Impact Sounds", "Kenney", KENNEY_PAGE),
    "impact_step_b": (IMPACT + "footstep_wood_002.ogg",
                      "8823e6162976292ff15c1fcd99280f16a7ad8ab629406fc0c60a1fe0a99db25d",
                      "Impact Sounds", "Kenney", KENNEY_PAGE),
    "impact_wood": (IMPACT + "impactWood_light_000.ogg",
                    "dced23f4708dbe08ae517902a4a4b2357ab0b88d737477f18560fbffee0a7a31",
                    "Impact Sounds", "Kenney", KENNEY_PAGE),
    "impact_plank": (IMPACT + "impactPlank_medium_000.ogg",
                     "b3ebc3441ac65db4276ddb771ac4819ebcc04346b5fc9e7244bdca37b9bf01ae",
                     "Impact Sounds", "Kenney", KENNEY_PAGE),
    "kenney_rpg": (OGA + "RPGsounds_Kenney.zip",
                   "3ae398ad63e293f9c450bda22d5d81c3af69c74df66fc1400f33c012c0bbc231",
                   "50 RPG sound effects", "Kenney", "https://opengameart.org/content/50-rpg-sound-effects"),
    "kenney_ui": (OGA + "kenney_interfaceSounds.zip",
                  "f2193d072726d6758a5f7871b2dcc54dcce0d5c35c6f0a62f92549b327c81232",
                  "Interface Sounds", "Kenney", "https://opengameart.org/content/interface-sounds"),
    "wood_metal": (OGA + "100-CC0-wood-metal-SFX.zip",
                   "be6eba63b03409ac0c77787a956b1503a7c186403d04aef9725c52644a4b7878",
                   "100 CC0 metal and wood SFX", "rubberduck", "https://opengameart.org/content/100-cc0-metal-and-wood-sfx"),
    "sfx100v2": (OGA + "sfx_100_v2.zip",
                 "0fc61b4494e2e893c0c015ced4877b3f689c7d84a48cb61daecd7ddb52db797b",
                 "100 CC0 SFX #2", "rubberduck", "https://opengameart.org/content/100-cc0-sfx-2"),
    "keyboard": (OGA + "unicae_games_keyboard_soundpack_1_0.zip",
                 "935eae2fa5c3742eacdd38c4ea0e9047f3887faa0701996498492c256db1b351",
                 "Keyboard Soundpack #1", "Unicae Games",
                 "https://opengameart.org/content/keyboard-soundpack-1-typing-and-single-keystrokes"),
    "sink": (OGA + "kitchensink01.wav",
             "430cc057dee80315da844dc74beca88205d1ca30fa89902077db41cc23c63854",
             "General Household Sound Effects (kitchen sink)", "Bret Bernhoft",
             "https://opengameart.org/content/general-household-sound-effects"),
    "board": (OGA + "kitchencuttingboard01.wav",
              "8caa77e924eb2e55fa1b225183d2f25e47be0099adfcb959d2d0d0ebe905c556",
              "General Household Sound Effects (cutting board)", "Bret Bernhoft",
              "https://opengameart.org/content/general-household-sound-effects"),
}

# id: (source, member in the archive or None, options)
#   loop: seconds of steadiest audio to keep as a seamless loop
#   bursts: archive members to cut random bursts from, with pauses between (see burst_loop)
#   hits: re-lay the recording's distinct hits every `hits` seconds instead of cutting a stretch
#   busy: take the loudest stretch instead of the steadiest (rhythmic sources)
#   peak: normalise to this dBFS (default -3 one-shots, -9 loops)
SFX = {
    "step_a": ("impact_step_a", None, {}),
    "step_b": ("impact_step_b", None, {}),
    "door_open": ("kenney_rpg", "OGG/doorOpen_1.ogg", {}),
    "door_close": ("kenney_rpg", "OGG/doorClose_1.ogg", {}),
    "sell": ("kenney_rpg", "OGG/handleCoins.ogg", {}),
    "cash": ("kenney_rpg", "OGG/handleCoins2.ogg", {}),
    "ui_click": ("kenney_ui", "Audio/click_001.ogg", {"peak": -6}),
    "ui_open": ("kenney_ui", "Audio/open_001.ogg", {}),
    "ui_close": ("kenney_ui", "Audio/close_001.ogg", {}),
    "notify": ("kenney_ui", "Audio/confirmation_001.ogg", {}),
    "pickup": ("kenney_ui", "Audio/select_001.ogg", {}),
    "place": ("impact_wood", None, {}),
    "build_wall": ("impact_plank", None, {}),
    "demolish": ("wood_metal", "wood_breaking_01.ogg", {}),
    "shower": ("sfx100v2", "sfx100v2_loop_water_01.ogg", {"loop": 4.0}),
    "faucet": ("sink", None, {"loop": 4.0}),
    "chop": ("board", None, {"loop": 2.4, "hits": 0.3}),
    "type": ("keyboard", "Human Typing/human_vel-*.wav",
             {"loop": 20.0, "bursts": [f"Human Typing/human_vel-{i:03}.wav" for i in range(1, 11)]}),
}


# Music: Kenney "Music Loops" (CC0) copied as-is to assets/audio/music/<id>.ogg.
# id: (file in Audio/Music Loops/Loops, sha256). Which plays when is in game/src/sounds.rs.
MUSIC_DIR = "Audio/Music Loops/Loops/"
MUSIC = {
    "swinging_pants": ("Swinging Pants.ogg", "8f60328d8664e050b7fb5c8c7f97862aca0f190a3f6e31d00f0415726d76d52c"),
    "italian_mom": ("Italian Mom.ogg", "d9392f3a561623c72a5a0a793984272a2264d350d9d61bfcd9bf4bdb8cfdd49b"),
    "mischief_stroll": ("Mishief Stroll.ogg", "83774bba960b6d2192d45e7b1c6c4b07d1b73061672230734dde195972a88a95"),
    "cheerful_annoyance": ("Cheerful Annoyance.ogg", "0e262e050e53eec55ebd1702981fa3944cd558d482580b6e34eea5a01ef54b05"),
    "farm_frolics": ("Farm Frolics.ogg", "3c1a6ae05701a7e32b6b3c19fae046a46d7183785ff0dbab4cd9b0683655dd89"),
    "polka_train": ("Polka Train.ogg", "18fda64504def86792493065b584ff5e762729989f2a9f8fb83284136a483e7b"),
    "night_at_the_beach": ("Night at the Beach.ogg", "8beff4d1153fcea045d1f3d01a241f81c6bb7645d64d5e52511152d1ff19c768"),
    "flowing_rocks": ("Flowing Rocks.ogg", "8563774243e381e827cba402cf6ccdc471b8bed3e3fafdf182f36039c2a8479f"),
    "space_cadet": ("Space Cadet.ogg", "bf1a3b86027ae92c79f425e3f857ee9be74cbdfea46b75b668fe55489598bfbc"),
    "drumming_sticks": ("Drumming Sticks.ogg", "35b3f446a74dfd543503aede4b99e223635b747e90c18821d48abc1c451ed089"),
    "mission_plausible": ("Mission Plausible.ogg", "324bb691a715a51875973079f9e0706fc81168203401725d4131d0fa1528c621"),
    "time_driving": ("Time Driving.ogg", "e7f220dca59444699153f4e44972b632258c32d7ee7e617ded9995791e4beac2"),
    "wacky_waiting": ("Wacky Waiting.ogg", "5b145cb963c583fd9a8728461a7ed3e3d98860b381ac0ef41ee2181e7baf0964"),
}


def build_music():
    out = REPO / "assets/audio/music"
    out.mkdir(parents=True, exist_ok=True)
    for mid, (file, sha) in MUSIC.items():
        path = KENNEY / MUSIC_DIR / file
        if not path.exists():
            sys.exit(f"{path} not found: download Kenney's All-in-1 bundle and set KENNEY_DIR")
        data = path.read_bytes()
        if hashlib.sha256(data).hexdigest() != sha:
            sys.exit(f"{file}: checksum mismatch")
        (out / f"{mid}.ogg").write_bytes(data)
    print(f"music: {len(MUSIC)} loops")


def fetch(name):
    url, sha, *_ = SOURCES[name]
    CACHE.mkdir(parents=True, exist_ok=True)
    path = CACHE / url.rsplit("/", 1)[1]
    if url.startswith("kenney:"):
        path = KENNEY / url.removeprefix("kenney:")
        if not path.exists():
            sys.exit(f"{path} not found: download Kenney's All-in-1 bundle and set KENNEY_DIR")
    elif not path.exists():
        print(f"  downloading {url}")
        req = urllib.request.Request(url, headers={"User-Agent": "life-sim-game asset fetcher"})
        with urllib.request.urlopen(req, timeout=300) as r:
            path.write_bytes(r.read())
    got = hashlib.sha256(path.read_bytes()).hexdigest()
    if got != sha:
        sys.exit(f"{path.name}: checksum mismatch ({got}); the upstream file changed")
    return path


def decode(data, suffix):
    """Any audio file → mono float32 at SR, via sox (reads from a temp file in the cache)."""
    tmp = CACHE / f"_in{suffix}"
    tmp.write_bytes(data)
    raw = subprocess.run(["sox", "-q", str(tmp), "-t", "raw", "-e", "float", "-b", "32", "-c", "1", "-r", str(SR), "-"],
                         check=True, capture_output=True).stdout
    tmp.unlink()
    return np.frombuffer(raw, dtype=np.float32).copy()


def steadiest(x, seconds, busy=False):
    """Start of the `seconds`-long window whose 50 ms loudness varies least (avoids taps on/off),
    or with `busy` the loudest one (most knife hits / keystrokes)."""
    hop = SR // 20
    rms = np.sqrt(np.convolve(x * x, np.ones(hop) / hop, mode="valid")[::hop] + 1e-12)
    n = int(seconds * 20)
    if len(rms) <= n:
        return 0
    med = np.median(rms)
    best, best_i = None, 0
    for i in range(0, len(rms) - n):
        w = rms[i:i + n]
        score = -np.mean(w) if busy else np.std(np.log(w)) + abs(np.log(np.mean(w) / med))
        if best is None or score < best:
            best, best_i = score, i
    return best_i * hop


def make_loop(x, seconds, fade=0.25, busy=False):
    """`seconds` of x whose end cross-fades (equal power) into its start."""
    n, f = int(seconds * SR), int(fade * SR)
    s = steadiest(x, seconds + fade, busy)
    seg = x[s:s + n + f]
    if len(seg) < n + f:
        seg = np.pad(seg, (0, n + f - len(seg)))
    t = np.linspace(0, np.pi / 2, f)
    out = seg[:n].copy()
    out[:f] = seg[:f] * np.sin(t) + seg[n:n + f] * np.cos(t)
    return out


def hit_loop(x, seconds, every, length=0.18):
    """A loop of the recording's distinct hits (onsets) re-laid on a steady beat: a sparse
    recording of knife taps becomes steady chopping."""
    hop = SR // 100
    env = np.sqrt(np.convolve(x * x, np.ones(hop) / hop, mode="same")[::hop])
    thresh = np.percentile(env, 50) * 4
    onsets, last = [], -100
    for i in range(1, len(env)):
        if env[i] > thresh and env[i - 1] <= thresh and i - last > 15:
            onsets.append(i * hop)
            last = i
    if not onsets:
        sys.exit("no hits found")
    n, m = int(seconds * SR), int(length * SR)
    hits = [x[o - SR // 200:o - SR // 200 + m] for o in onsets if o > SR // 200 and o + m < len(x)]
    hits.sort(key=lambda h: -np.max(np.abs(h)))
    hits = hits[:6]
    fade = np.minimum(1, np.linspace(4, 0, m))  # quick tail fade so hits don't click off
    out = np.zeros(n, dtype=np.float32)
    beats = int(seconds / every)
    for k in range(beats):
        h = hits[k % len(hits)] * fade * (1.0 if k % 2 == 0 else 0.8)
        at = int(k * every * SR)
        end = min(n, at + m)
        out[at:end] += h[:end - at]
        if at + m > n:  # wrap the tail of the last hit to the start
            out[:at + m - n] += h[n - at:]
    return out


def burst_loop(takes, seconds, burst=(1.0, 3.5), pause=(0.5, 2.5), seed=1):
    """A `seconds` loop of short bursts cut at random from `takes`, separated by silent pauses
    and varied in level: someone typing, stopping to think, typing again."""
    rng = np.random.default_rng(seed)
    n, edge = int(seconds * SR), int(0.03 * SR)
    out = np.zeros(n, dtype=np.float32)
    at = int(rng.uniform(*pause) * SR / 2)
    while True:
        take = takes[rng.integers(len(takes))]
        m = min(int(rng.uniform(*burst) * SR), len(take))
        if at + m > n - int(pause[0] * SR / 2):
            break
        a = rng.integers(0, len(take) - m + 1)
        b = take[a:a + m] * rng.uniform(0.6, 1.0)
        ramp = np.linspace(0, 1, edge)
        b[:edge] *= ramp
        b[-edge:] *= ramp[::-1]
        out[at:at + m] += b
        at += m + int(rng.uniform(*pause) * SR)
    return out


def trim_one_shot(x):
    """Drops leading/trailing near-silence, keeping a few ms of lead-in."""
    loud = np.nonzero(np.abs(x) > 0.01 * np.max(np.abs(x)))[0]
    if len(loud) == 0:
        return x
    a, b = max(0, loud[0] - SR // 200), min(len(x), loud[-1] + SR // 20)
    return x[a:b]


def encode(x, sid, peak):
    x = x / (np.max(np.abs(x)) + 1e-9) * 10 ** (peak / 20)
    pcm = (np.clip(x, -1, 1) * 32767).astype("<i2").tobytes()
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm)
    OUT.mkdir(parents=True, exist_ok=True)
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "wav", "-i", "-", "-c:a", "libvorbis", "-q:a", "4",
                    str(OUT / f"{sid}.ogg")], input=buf.getvalue(), check=True)


def build(sid):
    src, member, opts = SFX[sid]
    path = fetch(src)
    if "bursts" in opts:
        with zipfile.ZipFile(path) as z:
            takes = [decode(z.read(m), Path(m).suffix) for m in opts["bursts"]]
        x = burst_loop(takes, opts["loop"])
        encode(x, sid, opts.get("peak", -9))
        print(f"{sid:12} {len(x) / SR:5.2f}s  from {SOURCES[src][3]}")
        return
    if member:
        with zipfile.ZipFile(path) as z:
            data = z.read(member)
        suffix = Path(member).suffix
    else:
        data, suffix = path.read_bytes(), path.suffix
    x = decode(data, suffix)
    if "hits" in opts:
        x = hit_loop(x, opts["loop"], opts["hits"])
        peak = opts.get("peak", -9)
    elif "loop" in opts:
        x = make_loop(x, opts["loop"], busy=opts.get("busy", False))
        peak = opts.get("peak", -9)
    else:
        x = trim_one_shot(x)
        peak = opts.get("peak", -3)
    encode(x, sid, peak)
    print(f"{sid:12} {len(x) / SR:5.2f}s  from {SOURCES[src][3]}")


def credits():
    used = {}
    for sid, (src, member, _) in SFX.items():
        url, _, title, author, page = SOURCES[src]
        used.setdefault((title, author, page), []).append((sid, member or url))
    lines = ["# Audio credits", "",
             "Sound effects in `sfx/` downloaded by `fetch_sfx.py`. All sources are **CC0** (public domain);",
             "credit is given as thanks, not as a licence requirement. Sounds not listed here are synthesized by",
             "`assets/soundcraft/make_sfx.py`.", ""]
    for (title, author, page), items in used.items():
        lines.append(f"- **{title}** by {author}, CC0, <{page}>")
        for sid, file in items:
            lines.append(f"  - `{sid}.ogg` ← `{file.rsplit('/', 1)[-1]}`")
    lines += ["", "Music in `music/`: **Music Loops** by Kenney, CC0, <https://kenney.nl/assets/music-loops>", ""]
    lines += [f"- `{mid}.ogg` ← `{file}`" for mid, (file, _) in MUSIC.items()]
    (REPO / "assets/audio/CREDITS.md").write_text("\n".join(lines) + "\n")


def main():
    ids = sys.argv[1:] or list(SFX) + ["music"]
    if "music" in ids:
        build_music()
        ids.remove("music")
    for sid in ids:
        if sid not in SFX:
            sys.exit(f"unknown sound {sid}; have {', '.join(SFX)}")
        build(sid)
    credits()


if __name__ == "__main__":
    main()
