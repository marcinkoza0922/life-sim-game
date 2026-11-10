# Character critique: a Sims audience review

Reviewed 2026-11-09 at commit `8d5a620` on `character-models`, using freshly rendered,
embedded game models. These are art-direction judgments about likely player reactions,
not user-research findings or measurements of The Sims' proportions.

## Review verdict before the follow-up changes

The peons are more readable and expressive than the original critique suggests. Catchlights,
larger heads, differentiated body shapes, looser garments and eye-involving smiles are visible.
The remaining impression is of unfinished dolls: glossy, simplified skin, cap-like hair,
blocky facial hair, uneven garment boundaries and awkward hand contact. A Sims fan looking
for an appealing household to dress up and become attached to would likely notice these
before missing pores, individual eyebrow hairs or fingernails.

## Evidence and reproduction

Run `cargo build -p simctl`, then these commands **one at a time**. Concurrent offscreen
renders stalled during this review; sequential renders completed. Images are local review
artifacts under ignored `out/critique-review/`, not checked-in assets.

```sh
target/debug/simctl lineup --time 1 --out out/critique-review/peon-review-lineup.png
target/debug/simctl lineup --target 1.8,1.45,0 --dist 2.8 --pitch 5 --time 1 --out out/critique-review/peon-review-adults.png
target/debug/simctl lineup --target 1.8,1.45,0 --dist 2.8 --pitch 5 --anim laugh --time 1 --out out/critique-review/peon-review-laugh.png
target/debug/simctl lineup --yaw 80 --target 1.2,0.9,0 --dist 3.7 --time 1 --out out/critique-review/peon-review-side.png
```

The lineup covers infant, toddler, child, teen, two adults and elder, with several skin tones,
outfits and builds. Close-ups concentrate on the red-top and green-top adults. This is a
small sample, not coverage of all genetics, hairstyles or wardrobe combinations.
Two lot renders (`render --hour 13 --dist 12`, and a TV order staged around `11,16`) did not
frame usable adult characters, so they provide no evidence about adult readability in play.
The lineup is a studio view, not a substitute for that check. Still images also cannot verify
blink cadence, gaze tracking, animation transitions, foot sliding or temporal stability.

## Findings from the baseline renders

### 1. "That moustache is stuck on"

**Highest priority in the adult close-up.** The green-top adult's moustache is an opaque black,
rectangular patch with stepped ends; it obscures the upper lip and competes with the whole face.
The side view makes its projection especially apparent. Scalp hair is more coherent, but still
reads as a smooth cap with engraved ridges and a hard, jagged hairline. The blonde bun style has
large smooth masses with little silhouette breakup.

Aim for deliberate, sculpted hair clumps and clean, tapered facial-hair boundaries that follow
the face. Stylised hair does not need individual realistic strands. Judge the moustache both
at portrait distance and in profile before adding fine texture.

### 2. "The clothes still look roughly cut"

Drape is a real improvement: the green shirt bridges the belly and trouser legs have volume.
But the red tank's strap tips and armholes have small bites and angular corners; the green
straps have similar stepped edges. The red hem has a rippled, dark border. These still suggest
unfinished geometry rather than a designed, sewn garment.

The green adult's trousers balloon into padded-looking thigh/knee sections. The yellow long
sleeves retain bulges around the arms, so loose fabric can look inflated rather than hanging.
Fabric remains mostly a uniform colour, with little collar, cuff, seam or waistband definition.
Fix smooth boundaries and garment silhouette first, then add restrained seam/fabric detail.
Check front, side and bent-limb poses on both slim and heavy builds; more shell expansion alone
is not enough.

### 3. "Their skin looks polished"

The green adult now has readable eyes, nose and lips on darker skin. The old claim that dark
faces vanish completely is too broad. However, the shoulder highlight is a bright, concentrated
spot, and the arms and face look polished like vinyl. The pale adult also has very smooth,
low-variation skin. The side view shows a broad, simplified ear and little colour separation
around facial planes.

Tune the breadth and intensity of skin highlights across several tones under identical light;
preserve dark-face readability while reducing the toy finish. Add subtle warm/cool colour zones
and lip integration before chasing photorealistic microdetail. This review does not isolate
which shader setting is responsible.

### 4. "The smile is better, but the face is still a mask"

The neutral adults have visible catchlights and a slight smile. In the laugh, the eyes narrow,
cheeks change and heads tilt: the previous "only the mouth changes" complaint is obsolete.
The mouth still reads as a thin bright rim around a dark opening, with a broad, graphic tooth
band. Brows are very thin and the default eye opening/gaze can feel fixed. The profile makes
the large exposed eye and simple lid/eye-socket transition more noticeable.

Refine lid wrapping, mouth corners and the lip/teeth relationship as a coordinated face design.
A single still does not establish divergent gaze or broken tracking. Test an actual look-at
sequence and several expression strengths before calling those defects confirmed.

### 5. "Their hands don't meet naturally"

The laugh pose has separated fingers, so "clenched lumps" no longer describes it. Instead,
the crossing hands create overlapping, crowded fingers and an unclear contact pose. The
neutral hands have long, rather rigid-looking fingers. Broad rounded shoulders, abrupt elbow
changes and bulky lower legs also make anatomy read as assembled soft segments in places.

Author a clear palm/finger contact pose for the laugh and inspect it from both sides. Check
contact throughout the clip; this frame alone cannot tell whether the overlap persists.

### 6. "Do these ages feel different enough?"

The full lineup gives children readable size differences and adults distinct silhouettes;
the old blanket claim that everyone shares an indistinguishable body is outdated. The elder,
however, reads mostly as another adult with light hair in this sample. Age-specific facial
shapes, posture and styling deserve a closer check. The infant's seated silhouette is visible,
but its face is too small here to assess. Avoid signing off either stage from this image.

## Follow-up: moustache and skin finish

The standalone moustache now uses a shallow, tapered shell whose edges sink into the skin,
rather than a raised selection of whole face polygons. Its neutral-face outline is carried
through the existing face morphs. Adult and elder assets are regenerated; other beard styles
still use their previous geometry and need separate review.

Skin retains the extra sheen and sky reflection used to keep deep tones readable, but the
roughness reduction and specular boost on deep tones are smaller. A roughness floor broadens
the highlight instead of concentrating it into a bright shoulder spot.

Fresh `after-adults.png`, `after-laugh.png` and `after-side.png` in `out/critique-review/` use the
same cameras and expressions as the baseline. The adult moustache no longer has the rectangular
drooping ends, stays above the lip in the sampled laugh, and projects less in profile. Shoulder
highlights are softer while the dark adult face remains readable. These are sampled frames;
extreme slider combinations and continuous expression transitions still need visual coverage.
Clothing boundaries, scalp hair and hand contact remain open.

## Follow-up: strap and hem boundaries

Tank-top and dress straps used to stop at a horizontal collar plane on top of the shoulder,
which rises about 1.5 cm above it, and the strap/armhole cuts skipped faces nearest the arm and
neck bones there, so straps ended in notched tabs. Straps now run on over the shoulder to the
back, and those cuts take everything inboard of the armhole plane (`cut_regions`/`region_of` in
`peon_body.py`). Before each region cut, a vertex just beside the plane is slid onto it, so the
cut leaves no sliver row: the faint shading lines at the chest/belly seam and across trouser legs
are gone. `PEON_ZOOM=chest` gives a shoulder close-up in the Blender preview. Verified on
tank, tee, dress and long-sleeve tops (heavy 0.6) and in `simctl lineup`. Still open: sleeves
follow the arm's muscle lumps, the square neckline is plain, and there is no seam/cuff detail.

## Follow-up: sleeves and trouser legs hang as tubes

The normal-only drape bridged hollows but could only push cloth outward from each point, so sleeves
kept the deltoid, biceps and elbow lumps and trousers ballooned into thigh and knee lobes.
Sleeves and legs (shoulder, upper arm, forearm, thigh, knee, shin regions) are now re-shaped
around their bone chain per shape key (`hang_tubes` in `peon_body.py`): each vertex takes the
radius of the widest bulge nearby, tapering by `TUBE_SLOPE` along the limb and `TUBE_TURN` around
it, through a soft maximum so no crease forms where the peak switches. It fades in over
`TUBE_ROOT` from the shoulder/hip. Checked with a heavy (0.8) long-sleeve + trousers Blender
preview and `simctl lineup` standing and sitting. Fits are slightly looser overall. Still open:
the square neckline, seam/cuff/waistband detail, and a faint elbow line on long sleeves.

## Follow-up: beard and goatee outlines

Stubble, goatee, short and full beards were still whole-face selections, so they had a flat
horizontal cheek line, a rectangular hole around the mouth and a staircase down the goatee's
sides. They now use the hairline's approach (`clipped` in `peon_hair.py`): each style is a smooth
field on the neutral face (`beard_field`, `moustache_field`, `goatee_field`, `mouth_gap`), the skin
is clipped along its zero line, and the shell tapers to a lip at the edge. The cheek line sweeps
from the sideburn down to beside the mouth, the mouth opening is an ellipse, and the beard's
moustache part shares the standalone moustache's tapered outline. Checked front and profile in
the Blender head preview (`PEON_ZOOM=head`). Stubble is no longer a solid layer of hair colour:
its draw tints it as skin shaded a fifth of the way toward the hair (`stubble_tint` in `peons.rs`)
with the face's roughness, so dark stubble reads as a jaw shadow and fair stubble on fair skin
nearly vanishes. Checked in `simctl lineup` on deep skin with black hair. Still open: a faint light
line along the stubble edge, because the face gets the skin shader's sheen and the stubble draw
doesn't (tapering the shell to zero only adds z-fighting; a real fix is a per-vertex fade or
drawing stubble through the skin path); beards have no clump or strand breakup; scalp hair
still reads as a smooth cap.

## Next priorities and acceptance checks

1. **Facial hair and hair silhouette:** (moustache, beard and hairline outlines done, see above)
   stubble as a translucent shadow, clumped beard/scalp-hair silhouettes; preserve an appealing
   face in front, three-quarter and profile views.
2. **Garment detail:** (boundaries and tube volume done, see above) necklines, cuffs, seams and
   waistbands; recheck slim builds and extreme slider combinations.
3. **Skin finish and face integration:** softer highlights across tones, subtle colour zones,
   lids that sit around the eye and lips/teeth that belong to the face.
4. **Hand contact and expression polish:** readable, non-intersecting contact poses; assess
   laugh and neutral expressions in motion, not only at one timestamp.
5. **Gameplay and age coverage:** capture visible standing, seated and interacting household
   members at normal play zoom, then inspect infant/elder portraits and idle sequences.

Head size, body-shape differences, eye highlights and idle mechanisms have already received
work. Reassess their results in context instead of repeating the original fix list. Keep
silhouettes, expression and clothing appeal ahead of realistic skin or hair detail.

## Implementation history (not visual sign-off)

- **Eyes (implemented):** larger eyes, an emissive catchlight on each cornea (`glint`), a dark upper
  lash fringe, the white shaded under the upper lid (`eye_shade`), a relaxed lid line, and a
  slight resting smile (`assets/blender/peon_head.py`).
- **Garment edges (implemented):** every region boundary on the upper body is now a real plane cut:
  the armhole, collar, strap edges, and a diagonal tank armhole. Inner-arm faces are told apart
  from the torso's flank by their normals. Two new skin regions, `YOKE` (11) and `STRAP` (12),
  give tank tops and dresses real straps and tees a crew-ish neck. Garment open edges are folded
  back into the body (`turn_hems`), so no gap shows along hems (`assets/blender/peon_body.py`,
  `crates/game/src/peons.rs`).
- **Eye-involving smiles (implemented):** the `x_smile` key now squints like a real (Duchenne) smile:
  the lower lid bows up into a crescent, the upper lid drops a little, and the cheeks bunch up
  under the eyes. The resting hint of a smile doesn't squint. The laugh raises the brows less and
  blinks less, so the squint does the work (`peon_head.py`, `expression` in `peons.rs`).
  `simctl lineup` now settles the eased face before drawing, so renders show expressions at full
  strength.
- **Head and eye proportions (implemented):** heads are about 12% larger relative to height (adult and
  elder 0.17 → 0.19, teen 0.185 → 0.205, child 0.225 → 0.24 in `make_peons.STAGES`), eyes are
  about 10% larger (`eye_r` in `peon_head.py`), and the neck is slimmer (`peon_body.py`). The
  bigger head takes its height from the torso, so the neck is shorter as well.
- **Skin on darker tones (implemented):** skin gets additional specular sheen the darker its
  albedo (`skin_depth` in `hearth_render/src/shader.wgsl`), plus a soft sky reflection at grazing
  angles, so cheekbones, nose and brow read on deep tones instead of merging into one shape.
  Every head also gets subtle socket shading, deeper over the lid crease (`paint_head` in
  `peon_extremities.py`).
- **Idle life (already in place):** breathing, blinks, weight shift and glances live in `life`
  and `expression` in `peons.rs`.
- **Lips on deep skin (implemented):** lips have their own palette slot (`LIPS`), coloured by
  `lip_tint` in `peons.rs`: a darker, rosier skin on light tones, shifting to a berry-mauve on deep
  tones, so the mouth reads by hue where darkening alone vanished into the brown.
- **Male/female base shapes (implemented):** the `frame` key now carries a real hourglass at −1 (waist
  pinched at the sides, hips and seat out) and a V-taper at +1 (broad ribs and lats, narrow outer
  hips, flatter seat), leaving the inner thighs alone (`body_key_offset` in `peon_body.py`). Sexes
  roll frames nearer the ends (men 0.65–1, women −1 to −0.65, with a fuller chest;
  `sexed_shape` in `genetics.rs`), so every adult reads as one or the other before the sliders.
- **Looser clothes and drape (implemented):** garments stand off the skin by a per-region ease (snug at
  the chest and shoulders, looser over the belly, sleeves and lower legs; `EASE`), flare toward
  free-hanging hems (sleeves, shirt and leg hems; `HEM_FLARE`), and are draped for every shape
  key: the shell is smoothed along its normals but never sinks below skin + ease, so cloth bridges
  hollows (under the bust, behind the knee, between belly rolls) instead of following them
  (`drape_shell` in `peon_body.py`). Trousers now hang as straight legs and tees fall from the
  chest. The garment copy also welds the body's unwelded seams so the drape crosses them. Still
  rough: on heavy builds, sleeves keep the arm's convex muscle lumps, and faint shading lines
  remain where the region cuts leave thin rows. The Blender preview now hides covered skin like
  the game does.
