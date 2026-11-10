# Why the peons look "off": a critique from a Sims fan's point of view

A player who loves The Sims looks at our peons and bounces off. They can't say why; it just "looks
wrong." This doc puts that feeling into specific, fixable words. It's based on the renders in
`out/` (`lineup_v3.png`, `heads.png`, `laugh.png`, `ui_creator_face.png`) from 2026-11-09, around
commit 874db1b. Re-check each point against fresh renders before acting on it.

## The one-sentence version

**They're stuck between styles.** The bodies and faces are realistic enough that we judge them as
people, but too crude to hold up as people. And they aren't stylised enough to be read as
deliberately cartoony. That gap is the uncanny valley, and most of the points below are ways of
landing in it.

Sims characters stay out of that valley on purpose. They're idealised and slightly caricatured
(big expressive eyes, soft and appealing shapes, clean silhouettes, clothes that look like
clothes), and the camera almost never shows a flaw. The player is looking for *appeal*, and our
models give them *anatomy without appeal*.

## What the player is reacting to, in words they might use

### 1. "Their faces are dead": eyes and expression

- **The eyes are small, flat and stare.** Irises sit like painted dots in white sclera, with no
  wet highlight (specular catchlight), no shadow from the upper lid, and no visible eyelashes. Real
  and stylised eyes both rely on catchlights to look alive, and ours have none.
- **The gaze doesn't land anywhere.** In `heads.png` the eyes look slightly past the viewer or
  diverge a little. Mismatched or unfocused eyes are the strongest uncanny-valley trigger there is.
- **The resting face is blank or slightly alarmed.** Brows sit high and straight, the lids are wide
  open, and the mouth is a small closed line. Read together, that's mild shock. Sims have a soft,
  pleasant resting face: relaxed lids, a hint of a smile at the mouth corners.
- **Expressions only change the mouth.** In `laugh.png` the mouth opens into a thin white crescent,
  but the cheeks don't lift and the eyes don't squint. A smile that doesn't reach the eyes reads as
  fake or creepy (a "non-Duchenne smile").
- **The mouth is a slot.** Lips are a thin tinted band with no volume, no philtrum and no
  corners, so even a neutral face looks pinched.

### 2. "They look like mannequins": surface and shading

- **The skin is one flat colour.** There's no variation: no warmer cheeks, nose and ears, no
  darker lips, eye sockets or knuckles. Uniform skin reads as plastic or clay. The subsurface work
  in 874db1b helps with light bleed, but not with colour variation across the face.
- **Dark skin tones lose the face entirely.** On darker peons (the man in green in `heads.png`,
  the red-haired peon in `laugh.png`) the features vanish into one brown shape. The lighting and
  material don't give darker skin enough highlight and contrast, which is also a representation
  problem players will notice.
- **There's no fine detail where the eye expects it.** No eyebrow hairs, no lashes, no nostrils
  with depth, no fingernails. Each one alone is minor, but all of them together make the faces
  look unfinished.

### 3. "Their heads are wrong": proportions and silhouette

- **The heads are realistically small.** Sims use heads about 10–15% larger than real life, and
  eyes larger still, because players look at characters from a zoomed-out camera and read emotion
  from the face. At gameplay zoom (`lineup_v3.png`) our faces are a few pixels across and say
  nothing.
- **The necks are long and thick, and the shoulders slope.** The head sits on a column of neck
  that runs straight into the trapezius, which reads as gaunt or strained.
- **Male and female bodies don't read apart.** The silhouettes share one torso with sliders on it,
  so many adults look androgynous in an unintended way. The Sims pushes clear base shapes (waist,
  hips, shoulder width) before sliders.
- **Bodies look stiff and doll-like.** Arms hang dead straight with the hands at mid-thigh, and the
  legs are parallel tubes. There's no weight shift and no contrapposto in the idle, so standing
  peons look like they're on display stands.

### 4. "The clothes are painted on, and torn": garments

This is probably the most concrete and the most fixable.

- **Every garment is skin-tight.** Clothes are the body's surface pushed out a little
  (`peon_body.GARMENTS`), so a shirt is a coloured body: no drape, no folds, no looseness at the
  waist or sleeves. That's why the green shirt in `heads.png` shows every belly contour.
- **The edges are ragged.** Necklines and hems follow the body's triangles, so they come out as
  jagged, saw-tooth edges (the red and green tops in `heads.png`, the yellow neckline in
  `ui_creator_face.png`). To a player that looks like a rendering bug or a torn costume.
- **Necklines are odd.** Wide, low, straight-across cuts show a band of bare chest and shoulder, so
  ordinary t-shirts read as strapless tops or tube tops.
- **Clothes have no thickness, no seams and no texture.** No collars, cuffs, hems, waistbands or
  fabric weave. Plain matte colour on a skin-tight shell looks like body paint.

### 5. "Their hair is a helmet"

- **Hair is a smooth shell sitting on the skull** with a stripe pattern for strands. No
  silhouette breakup, no strands lifting off the head, no wisps at the hairline.
- **The white curly style** in `ui_creator_face.png` reads as a shower cap or a bathing cap,
  not as hair.
- **Facial hair looks pasted on.** The moustache and beard are flat, hard-edged black or white
  shapes floating on the face, like a costume-shop disguise.

### 6. "They move like puppets": animation

- **Hands are clenched lumps in poses** (`laugh.png`) and don't grip or gesture.
- **There's little secondary motion:** no breathing, blinking, head turns, or weight shifting
  in the idle. Sims are always alive (fidgeting, glancing, shifting), and a still peon reads as a
  model, not a person.

## What actually matters most

If we fix only a few things, these do the most for the "looks wrong" reaction, roughly in order:

1. **Eyes:** catchlights, lashes, a lid shadow, focused gaze, slightly larger eyes. This is
   cheap, and it's the biggest step toward "alive."
2. **Clean garment edges and looser clothes:** smooth hem and neckline curves instead of
   triangle-following cuts, plus a little drape. This removes the "broken" read.
3. **Pick a style and push it:** larger heads and eyes and softer, idealised shapes (toward Sims
   appeal), instead of creeping further toward realism with crude detail.
4. **Resting face and eye-involving expressions:** a pleasant default, and smiles that squint the
   eyes and lift the cheeks.
5. **Skin variation, especially for darker tones:** blush zones, lip and socket contrast, and
   enough specular highlight that the features read.
6. **Idle life:** blinking, breathing and weight shift.

## Progress

- **Eyes (done):** larger eyes, an emissive catchlight on each cornea (`glint`), a dark upper
  lash fringe, the white shaded under the upper lid (`eye_shade`), a relaxed lid line, and a
  slight resting smile (`assets/blender/peon_head.py`).
- **Garment edges (done):** every region boundary on the upper body is now a real plane cut:
  the armhole, collar, strap edges, and a diagonal tank armhole. Inner-arm faces are told apart
  from the torso's flank by their normals. Two new skin regions, `YOKE` (11) and `STRAP` (12),
  give tank tops and dresses real straps and tees a crew-ish neck. Garment open edges are folded
  back into the body (`turn_hems`), so no gap shows along hems (`assets/blender/peon_body.py`,
  `crates/game/src/peons.rs`).
- Still open: looser clothes and drape, head and eye proportions, eye-involving expressions,
  skin variation on darker tones, idle life, hair and facial hair.

Hair shells and facial hair come after these. They're noticeable, but players forgive stylised
hair much more readily than dead eyes or torn clothing.
