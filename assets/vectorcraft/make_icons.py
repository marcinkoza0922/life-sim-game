#!/usr/bin/env python3
"""UI icon set in the hearth_ui toy-box style: candy fills, chunky plum outlines, glossy highlights.

Writes one 64x64 SVG per icon to assets/vectorcraft/icons/ (plus sheet.svg, a contact sheet), then
rasterises each with VectorCraft (`vectorcraft-cli run`) to assets/ui/icons/<name>.png at 2x
(128 px). The game embeds the PNGs and downsizes them on load to the size they're drawn at.

    python3 assets/vectorcraft/make_icons.py            # SVGs + PNGs
    python3 assets/vectorcraft/make_icons.py --svg-only

Colours are the hearth_ui theme (crates/hearth_ui/src/theme.rs).
"""

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SVG_DIR = ROOT / "assets/vectorcraft/icons"
PNG_DIR = ROOT / "assets/ui/icons"

INK = "#3B2F5C"
CREAM = "#FFF8EC"
CREAM_DEEP = "#FBE7CB"
PAPER = "#FFFFFF"
TEAL = "#2EC4B6"
TEAL_LIGHT = "#7BDDD4"
CORAL = "#FF6B6B"
CORAL_LIGHT = "#FF9A9A"
SUN = "#FFC94D"
SUN_DEEP = "#E9A93A"
GRASS = "#6CCB7A"
GRASS_DEEP = "#4FA65E"
SKY = "#5BB8F5"
SKY_PALE = "#C4E7FF"
LILAC = "#B59CF0"
PEACH = "#FFAE7A"
PEACH_LIGHT = "#FFCBA6"
WOOD = "#E08D5A"
DIRT = "#C98A5E"
STEEL = "#C3CBDD"

W = 3.5  # outline width


def line_style(color=INK, width=W, opacity=1.0):
    op = f' stroke-opacity="{opacity}"' if opacity < 1 else ""
    return f'fill="none" stroke="{color}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round"{op}'


def outlined(fill, width=W):
    return f'fill="{fill}" stroke="{INK}" stroke-width="{width}" stroke-linejoin="round" stroke-linecap="round"'


def rect(x, y, w, h, fill, rx=0, width=W):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" {outlined(fill, width)}/>'


def circle(cx, cy, r, fill, width=W):
    return f'<circle cx="{cx}" cy="{cy}" r="{r}" {outlined(fill, width)}/>'


def path(d, fill, width=W):
    return f'<path d="{d}" {outlined(fill, width)}/>'


def stroke(d, color=INK, width=W, opacity=1.0):
    return f'<path d="{d}" {line_style(color, width, opacity)}/>'


def gloss(d, width=3.5, opacity=0.6):
    """A white highlight streak."""
    return stroke(d, PAPER, width, opacity)


def blob(x, y, w, h, opacity=0.5):
    """A white highlight pill."""
    r = min(w, h) / 2
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="{PAPER}" fill-opacity="{opacity}"/>'


BADGE = [  # "remove": a coral disc with a white cross, bottom right
    circle(49, 49, 11, CORAL),
    stroke("M44.5 44.5 L53.5 53.5 M53.5 44.5 L44.5 53.5", PAPER, 3.5),
]

# ------------------------------------------------------------------------------------------------
# Build tools

WALL = [
    rect(6, 14, 52, 38, CORAL, 4),
    stroke("M6 26.5 H58 M6 39.5 H58", INK, 2.5),
    stroke("M23 14 V26.5 M41 14 V26.5 M14 26.5 V39.5 M32 26.5 V39.5 M50 26.5 V39.5 M23 39.5 V52 M41 39.5 V52", INK, 2.5),
    blob(10, 17.5, 9, 3.5), blob(27, 17.5, 10, 3.5), blob(18, 30.5, 10, 3.5), blob(36, 30.5, 10, 3.5),
]

FLOOR = [
    path("M6 34 L32 49 L58 34 V40 L32 55 L6 40 Z", SUN_DEEP),
    path("M6 34 L32 19 L58 34 L32 49 Z", SUN),
    path("M32 19 L45 26.5 L32 34 L19 26.5 Z", PEACH, 2.5),
    path("M32 34 L45 41.5 L32 49 L19 41.5 Z", PEACH, 2.5),
    gloss("M14 32 L22 27.5", width=3, opacity=0.7),
]

ROOF = [
    rect(41, 12, 8, 16, PEACH, 2),
    rect(14, 38, 36, 18, CREAM_DEEP, 2),
    rect(27, 43, 10, 8, SKY, 1.5, 2.5),
    path("M4 40 L32 12 L60 40 Z", CORAL),
    stroke("M20 26 H44 M12 33.5 H52", INK, 2.5, 0.45),
    gloss("M13 34 L29 18", width=3.5, opacity=0.6),
]

ICONS = {
    "room": [
        rect(8, 8, 48, 48, SUN, 3),
        stroke("M8 24 H56 M8 40 H56 M24 8 V56 M40 8 V56", INK, 1.8, 0.3),
        path("M8 8 H56 V56 H40 V49 H49 V15 H15 V49 H24 V56 H8 Z", PEACH),
        blob(12, 10, 22, 3),
    ],
    "wall": WALL,
    "knock_down": WALL + BADGE,
    "door": [
        stroke("M6 58 H58", INK, W),
        rect(14, 6, 36, 52, CREAM_DEEP, 3),
        rect(19, 11, 26, 47, PEACH, 2),
        rect(23, 15, 18, 15, "none", 2, 2.2),
        rect(23, 35, 18, 17, "none", 2, 2.2),
        circle(40, 33, 2.8, SUN, 2),
        blob(20.5, 13, 3.5, 40, 0.4),
    ],
    "window": [
        rect(8, 8, 48, 44, CREAM, 5),
        rect(13, 13, 38, 34, SKY, 2),
        stroke("M32 13 V47 M13 30 H51", INK, W),
        gloss("M17 25 L23 18 M17 41 L28 34", width=3, opacity=0.75),
        rect(4, 50, 56, 7, PEACH, 3.5),
    ],
    "floor": FLOOR,
    "rip_floor": FLOOR + BADGE,
    "stairs": [
        path("M6 58 V46 H19 V34 H32 V22 H45 V10 H58 V58 Z", PEACH),
        stroke("M6 46 H58 M19 34 H58 M32 22 H58", INK, 2, 0.25),
        gloss("M9 49.5 H16 M22 37.5 H29 M35 25.5 H42 M48 13.5 H55", width=2.8, opacity=0.75),
    ],
    "dig": [
        '<g transform="rotate(28 32 38)">',
        stroke("M32 6 V34", INK, 8),
        stroke("M32 6 V34", WOOD, 3.5),
        stroke("M25 6 H39", INK, 8),
        stroke("M25 6 H39", WOOD, 3.5),
        path("M23 31 H41 V44 Q41 54 32 59 Q23 54 23 44 Z", STEEL),
        gloss("M27 36 V45", width=3, opacity=0.8),
        "</g>",
        path("M3 59 Q10 46 32 47 Q54 46 61 59 Z", DIRT),
        f'<circle cx="18" cy="54" r="1.8" fill="{INK}" fill-opacity="0.35"/>',
        f'<circle cx="44" cy="53" r="1.8" fill="{INK}" fill-opacity="0.35"/>',
        f'<circle cx="31" cy="55.5" r="1.5" fill="{INK}" fill-opacity="0.35"/>',
    ],
    "roof": ROOF,
    "no_roof": ROOF + BADGE,
    "terrain": [
        circle(14, 13, 7, SUN),
        path("M2 54 Q19 0 38 54 Z", GRASS_DEEP),
        path("M20 56 Q42 6 62 56 Z", GRASS),
        rect(2, 52, 60, 7, DIRT, 3.5),
        gloss("M31 42 Q35 32 40 29", width=3.2, opacity=0.6),
    ],
    # --------------------------------------------------------------------------------------------
    # Buy categories
    "seating": [
        rect(11, 48, 5, 9, WOOD, 2, 2.5),
        rect(48, 48, 5, 9, WOOD, 2, 2.5),
        rect(14, 8, 36, 32, TEAL, 9),
        rect(14, 34, 36, 14, TEAL_LIGHT, 5),
        rect(5, 26, 13, 25, TEAL, 6.5),
        rect(46, 26, 13, 25, TEAL, 6.5),
        blob(19, 12, 15, 4),
        blob(8, 29, 4, 10, 0.45),
    ],
    "sleeping": [
        rect(51, 44, 5, 12, WOOD, 2, 2.5),
        rect(52, 28, 8, 26, PEACH, 3.5),
        rect(8, 34, 48, 13, CREAM, 4),
        path("M26 30 Q26 27 29 27 H52 Q56 27 56 31 V47 H26 Z", SKY),
        stroke("M30 37 H52", INK, 2, 0.35),
        rect(13, 25, 15, 11, PAPER, 5.5),
        rect(4, 12, 11, 44, PEACH, 4),
        blob(6.5, 16, 3.5, 18, 0.5),
        blob(30, 30, 14, 3, 0.55),
    ],
    "surfaces": [
        rect(10, 26, 7, 31, WOOD, 2),
        rect(47, 26, 7, 31, WOOD, 2),
        rect(37, 8, 12, 14, SKY, 2.5),
        stroke("M49 12 Q55 12 55 15.5 Q55 19 49 19", INK, 2.8),
        rect(4, 21, 56, 10, PEACH, 4),
        blob(8, 23.5, 22, 3),
    ],
    "kitchen": [
        stroke("M21 19 Q17 14 21 9 M32 17 Q28 12 32 7 M43 19 Q39 14 43 9", INK, 3, 0.45),
        rect(2, 34, 10, 7, INK, 3.5, 0),
        rect(52, 34, 10, 7, INK, 3.5, 0),
        path("M10 28 H54 V45 Q54 56 43 56 H21 Q10 56 10 45 Z", CORAL),
        rect(6, 24, 52, 8, CORAL_LIGHT, 4),
        blob(15, 36, 4, 14, 0.5),
    ],
    "bathroom": [
        circle(16, 55, 3.5, SUN, 2.5),
        circle(48, 55, 3.5, SUN, 2.5),
        path("M7 30 H57 V38 Q57 52 43 52 H21 Q7 52 7 38 Z", PAPER),
        circle(18, 22, 6, SKY_PALE, 2.5),
        circle(29, 17, 7.5, SKY_PALE, 2.5),
        circle(41, 21, 6, SKY_PALE, 2.5),
        circle(51, 24, 4, SKY_PALE, 2.5),
        rect(4, 26, 56, 7, PAPER, 3.5),
        stroke("M15 41 Q17 46 23 47", SKY, 3),
        f'<circle cx="27" cy="14" r="2" fill="{PAPER}"/>',
    ],
    "electronics": [
        stroke("M32 17 L21 6 M32 17 L43 6", INK, 3),
        circle(21, 6, 2.6, CORAL, 2),
        circle(43, 6, 2.6, CORAL, 2),
        stroke("M15 52 L11 58 M49 52 L53 58", INK, W),
        rect(5, 16, 54, 37, LILAC, 8),
        rect(11, 22, 34, 25, SKY, 4),
        circle(52, 28, 3, SUN, 2.5),
        circle(52, 39, 3, SUN, 2.5),
        gloss("M16 34 L24 26 M17 41 L19 39", width=3, opacity=0.75),
    ],
    "fun": [
        path("M15 20 H49 Q61 20 61 35 Q61 51 52 51 Q46 51 42 43 H22 Q18 51 12 51 Q3 51 3 35 Q3 20 15 20 Z", LILAC),
        f'<rect x="11" y="31" width="14" height="5" rx="1.5" fill="{INK}"/>',
        f'<rect x="15.5" y="26.5" width="5" height="14" rx="1.5" fill="{INK}"/>',
        circle(44, 29, 3.4, CORAL, 2.2),
        circle(51, 35.5, 3.4, SUN, 2.2),
        circle(37, 35.5, 3.4, TEAL, 2.2),
        circle(44, 42, 3.4, SKY, 2.2),
        gloss("M10 26 Q15 23.5 22 23.5", width=3, opacity=0.6),
    ],
    "kids": [
        f'<ellipse cx="35" cy="44" rx="24" ry="12" {outlined(SUN)}/>',
        path("M50 37 Q62 30 60 42 Z", SUN),
        path("M33 41 Q43 35 51 41 Q45 50 36 48 Z", SUN_DEEP, 2.5),
        circle(24, 23, 11.5, SUN),
        path("M14 23 Q3 24 6 30 Q12 32 16 28 Z", PEACH, 2.5),
        f'<circle cx="22" cy="20" r="2.2" fill="{INK}"/>',
        blob(25, 14, 7, 3.5, 0.6),
        blob(18, 38, 10, 3.5, 0.5),
    ],
    "decor": [
        path("M32 37 Q21 23 32 5 Q43 23 32 37 Z", GRASS),
        path("M30 38 Q11 34 6 18 Q24 18 30 38 Z", GRASS),
        path("M34 38 Q53 34 58 18 Q40 18 34 38 Z", GRASS),
        stroke("M32 32 V14 M27 34 L14 23 M37 34 L50 23", INK, 2, 0.4),
        path("M18 39 H46 L42 58 H22 Z", CORAL),
        rect(15, 34, 34, 8, CORAL_LIGHT, 3.5),
        blob(22, 44, 4, 10, 0.5),
    ],
    "lighting": [
        stroke("M5 13 L10 17 M59 13 L54 17 M3 27 H9 M61 27 H55", INK, W),
        path("M32 5 Q51 5 51 24 Q51 34 42 40 V45 H22 V40 Q13 34 13 24 Q13 5 32 5 Z", SUN),
        stroke("M26 32 L29 25 L32 32 L35 25 L38 32", INK, 2.4, 0.55),
        rect(22, 45, 20, 6, STEEL, 2, 3),
        rect(24, 51, 16, 5, STEEL, 2, 3),
        path("M28 56 H36 L34 60 H30 Z", INK, 2),
        gloss("M20 18 Q22 11 29 10", width=3.5, opacity=0.75),
    ],
    "misc": [
        path("M44 24 L57 15 V47 L44 56 Z", WOOD),
        path("M7 24 L20 15 H57 L44 24 Z", PEACH_LIGHT),
        rect(7, 24, 37, 32, PEACH, 1),
        path("M21 24 L34 15 H42 L29 24 Z", CREAM_DEEP, 2.2),
        rect(21, 24, 8, 12, CREAM_DEEP, 0, 2.2),
        blob(10, 27, 8, 3, 0.5),
    ],
    "sell": [
        circle(32, 32, 25, SUN),
        f'<circle cx="32" cy="32" r="18.5" fill="none" stroke="{SUN_DEEP}" stroke-width="3"/>',
        stroke("M38.5 24.5 Q36.5 20.5 32 20.5 Q25.5 20.5 25.5 26 Q25.5 31 32 32 Q38.5 33 38.5 38 Q38.5 43.5 32 43.5 Q26.5 43.5 25 39.5", INK, 4),
        stroke("M32 15.5 V20.5 M32 43.5 V48.5", INK, 4),
        gloss("M13 26 Q16 17 24 12", width=3.5, opacity=0.7),
    ],
}


def svg(body, size=64):
    inner = "\n  ".join(body)
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 64 64">\n  {inner}\n</svg>\n'


def sheet(names, cols=6, cell=80):
    rows = (len(names) + cols - 1) // cols
    parts = [f'<rect width="{cols * cell}" height="{rows * cell}" fill="{CREAM}"/>']
    for i, n in enumerate(names):
        x, y = (i % cols) * cell + 8, (i // cols) * cell + 8
        parts.append(f'<g transform="translate({x} {y})">' + "".join(ICONS[n]) + "</g>")
    inner = "\n  ".join(parts)
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{cols * cell}" height="{rows * cell}" viewBox="0 0 {cols * cell} {rows * cell}">\n  {inner}\n</svg>\n'


def main():
    SVG_DIR.mkdir(parents=True, exist_ok=True)
    PNG_DIR.mkdir(parents=True, exist_ok=True)
    for name, body in ICONS.items():
        (SVG_DIR / f"{name}.svg").write_text(svg(body))
    (SVG_DIR / "sheet.svg").write_text(sheet(list(ICONS)))
    print(f"wrote {len(ICONS)} SVGs + sheet.svg to {SVG_DIR.relative_to(ROOT)}")
    if "--svg-only" in sys.argv:
        return
    for name in ICONS:
        out = PNG_DIR / f"{name}.png"
        r = subprocess.run(["vectorcraft-cli", "run", "--in", str(SVG_DIR / f"{name}.svg"), "--export", str(out), "--scale", "2"], capture_output=True, text=True)
        if r.returncode != 0 or not out.exists():
            sys.exit(f"vectorcraft-cli failed on {name}:\n{r.stdout}\n{r.stderr}")
    print(f"rasterised {len(ICONS)} PNGs (128 px) to {PNG_DIR.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
