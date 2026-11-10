//! Genetics: a small gene vector per peon, inherited per gene from two parents with mutation.
//! [`Genes`] is the genotype; [`Appearance`] is what gets rendered (elders go grey, clothes are chosen).
//! Also the catalogues of shape sliders, hair styles and clothes the bodies are built with
//! (`assets/blender/make_peons.py` names its shape keys and meshes after these ids).

use std::collections::BTreeMap;

use crate::components::{Appearance, Genes, Personality, Sex};
use crate::data::{GameData, LifeStage};
use crate::rng::SimRng;

pub const EYE_COLORS: [[f32; 3]; 5] = [
    [0.25, 0.15, 0.08], // brown
    [0.12, 0.08, 0.05], // dark brown
    [0.3, 0.45, 0.7],   // blue
    [0.3, 0.5, 0.3],    // green
    [0.5, 0.4, 0.25],   // hazel
];

pub fn default_eyes() -> [f32; 3] {
    EYE_COLORS[0]
}

pub fn default_shoes() -> [f32; 3] {
    [0.15, 0.12, 0.1]
}

pub const ELDER_HAIR: [f32; 3] = [0.82, 0.82, 0.84];

/// Face shape sliders (shape key id, label), each -1..1. Order matches `peon_head.FACE_SLIDERS`.
pub const FACE_SLIDERS: [(&str, &str); 18] = [
    ("head_width", "Head width"),
    ("face_length", "Face length"),
    ("jaw", "Jaw"),
    ("chin", "Chin"),
    ("cheeks", "Cheekbones"),
    ("cheek_fat", "Cheeks"),
    ("brow", "Brow ridge"),
    ("eye_size", "Eye size"),
    ("eye_spacing", "Eye spacing"),
    ("eye_tilt", "Eye tilt"),
    ("eye_open", "Eyelids"),
    ("nose_length", "Nose length"),
    ("nose_width", "Nose width"),
    ("nose_bridge", "Nose bridge"),
    ("nose_tip", "Nose tip"),
    ("lips", "Lip fullness"),
    ("mouth_width", "Mouth width"),
    ("ears", "Ear size"),
];

/// Body shape sliders (shape key id, label), each -1..1. `frame` runs feminine (-) to masculine (+).
pub const BODY_SLIDERS: [(&str, &str); 7] = [
    ("frame", "Frame"),
    ("muscle", "Muscle"),
    ("shoulders", "Shoulders"),
    ("bust", "Chest"),
    ("waist", "Waist"),
    ("hips", "Hips"),
    ("belly", "Belly"),
];

/// Hair styles (mesh `hair.<id>`, label), indexed by [`Appearance::hair_style`]. The first four
/// keep the indices of the original styles so older saves look the same.
pub const HAIR_STYLES: [(&str, &str); 12] = [
    ("short", "Short"),
    ("bun", "Bun"),
    ("long", "Long"),
    ("receding", "Receding"),
    ("buzz", "Buzz cut"),
    ("side_part", "Side part"),
    ("ponytail", "Ponytail"),
    ("bob", "Bob"),
    ("curly", "Curly"),
    ("undercut", "Undercut"),
    ("afro", "Afro"),
    ("bald", "Bald"),
];

/// Facial hair (mesh `facial.<id>`; "none" has no mesh), indexed by [`Appearance::facial_hair`].
pub const FACIAL_HAIR: [(&str, &str); 6] =
    [("none", "None"), ("stubble", "Stubble"), ("moustache", "Moustache"), ("goatee", "Goatee"), ("short_beard", "Short beard"), ("full_beard", "Full beard")];

/// Tops (mesh `top_<id>`), indexed by [`Appearance::top`]. A dress covers the bottom too.
pub const TOPS: [(&str, &str); 4] = [("tee", "T-shirt"), ("long", "Long sleeves"), ("tank", "Tube top"), ("dress", "Dress")];

/// Bottoms (mesh `bottom_<id>`), indexed by [`Appearance::bottom`].
pub const BOTTOMS: [(&str, &str); 3] = [("trousers", "Trousers"), ("shorts", "Shorts"), ("skirt", "Skirt")];

pub const DRESS: u8 = 3;

/// Builds the rendered appearance for a genotype at a life stage; clothes and grooming come
/// from `worn`.
pub fn express(g: &Genes, stage: LifeStage, worn: &Appearance) -> Appearance {
    Appearance {
        skin: g.skin,
        hair: if stage == LifeStage::Elder { ELDER_HAIR } else { g.hair },
        height: g.height,
        build: g.build,
        eyes: g.eyes,
        shape: g.shape.clone(),
        ..worn.clone()
    }
}

/// Best-effort genotype for peons created without one (older specs, hand-made peons).
pub fn genes_from_appearance(a: &Appearance) -> Genes {
    Genes { skin: a.skin, hair: a.hair, eyes: a.eyes, height: a.height, build: a.build, shape: a.shape.clone() }
}

/// Sliders that follow the sex of the body rather than the parents' average.
const SEXED: [&str; 2] = ["frame", "bust"];

/// A random face and body: every slider spread around average, frame and chest by sex.
pub fn random_shape(rng: &mut SimRng, sex: Sex) -> BTreeMap<String, f32> {
    let mut shape = BTreeMap::new();
    for (id, _) in FACE_SLIDERS {
        shape.insert(id.to_string(), spread(rng, 0.45));
    }
    for (id, spread_by) in [("muscle", 0.35), ("shoulders", 0.3), ("waist", 0.3), ("hips", 0.3), ("belly", 0.25)] {
        shape.insert(id.to_string(), spread(rng, spread_by));
    }
    sexed_shape(rng, sex, &mut shape);
    // faces lean a little by sex too
    if sex == Sex::Male {
        for (id, by) in [("jaw", 0.35), ("brow", 0.4), ("chin", 0.2), ("lips", -0.2), ("eye_size", -0.15)] {
            let v = shape.get(id).copied().unwrap_or(0.0);
            shape.insert(id.into(), (v + by).clamp(-1.0, 1.0));
        }
    } else {
        for (id, by) in [("jaw", -0.3), ("brow", -0.35), ("lips", 0.25), ("eye_size", 0.15), ("hips", 0.3)] {
            let v = shape.get(id).copied().unwrap_or(0.0);
            shape.insert(id.into(), (v + by).clamp(-1.0, 1.0));
        }
    }
    shape
}

/// Sets the sex-linked sliders: frame and chest.
pub fn sexed_shape(rng: &mut SimRng, sex: Sex, shape: &mut BTreeMap<String, f32>) {
    let (frame, bust) = match sex {
        Sex::Male => (rng.range_f32(0.65, 1.0), rng.range_f32(-0.4, 0.0)),
        Sex::Female => (rng.range_f32(-1.0, -0.65), rng.range_f32(0.25, 0.9)),
    };
    shape.insert("frame".into(), frame);
    shape.insert("bust".into(), bust);
}

/// A value around 0, mostly within ±by (sum of two uniforms: a soft bell).
fn spread(rng: &mut SimRng, by: f32) -> f32 {
    ((rng.range_f32(-1.0, 1.0) + rng.range_f32(-1.0, 1.0)) * by).clamp(-1.0, 1.0)
}

/// A random hairdo and outfit for a new peon of this sex and stage (colours are the caller's).
pub fn random_style(rng: &mut SimRng, sex: Sex, stage: LifeStage, a: &mut Appearance) {
    let idx = |id: &str, list: &[(&str, &str)]| list.iter().position(|(i, _)| *i == id).unwrap_or(0) as u8;
    let hair: &[&str] = match sex {
        Sex::Female => &["long", "bob", "bun", "ponytail", "curly", "side_part", "short", "long", "afro"],
        Sex::Male if stage >= LifeStage::Adult => &["short", "buzz", "side_part", "undercut", "curly", "receding", "afro", "bald", "short"],
        Sex::Male => &["short", "buzz", "side_part", "undercut", "curly", "afro", "short"],
    };
    a.hair_style = idx(rng.pick(hair).unwrap(), &HAIR_STYLES);
    a.facial_hair = if sex == Sex::Male && stage.is_adult() && rng.chance(0.45) { 1 + rng.below(FACIAL_HAIR.len() - 1) as u8 } else { 0 };
    a.top = match sex {
        Sex::Female if rng.chance(0.2) => DRESS,
        _ => rng.below(3) as u8,
    };
    a.bottom = match sex {
        Sex::Female => *rng.pick(&[0u8, 0, 1, 2, 2]).unwrap(),
        Sex::Male => *rng.pick(&[0u8, 0, 0, 1]).unwrap(),
    };
    let shoes = [[0.15, 0.12, 0.1], [0.92, 0.92, 0.9], [0.35, 0.22, 0.12], [0.2, 0.2, 0.25], [0.7, 0.2, 0.2]];
    a.shoes = *rng.pick(&shoes).unwrap();
}

fn mix3(rng: &mut SimRng, a: [f32; 3], b: [f32; 3], blend: bool, jitter: f32) -> [f32; 3] {
    let base = if blend {
        let t = rng.range_f32(0.3, 0.7);
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    } else if rng.chance(0.5) {
        a
    } else {
        b
    };
    let j = rng.range_f32(-jitter, jitter);
    [(base[0] + j).clamp(0.0, 1.0), (base[1] + j).clamp(0.0, 1.0), (base[2] + j).clamp(0.0, 1.0)]
}

/// A child's genes: skin blends, hair and eyes come from one parent (darker hair slightly
/// dominant), height, build and face shape land between the parents' with noise, plus a small
/// chance of a wild mutation. The sex-linked sliders are left out: set them with [`sexed_shape`]
/// once the child's sex is known.
pub fn inherit(rng: &mut SimRng, a: &Genes, b: &Genes) -> Genes {
    let lum = |c: [f32; 3]| c[0] + c[1] + c[2];
    let (dark, light) = if lum(a.hair) <= lum(b.hair) { (a.hair, b.hair) } else { (b.hair, a.hair) };
    let hair = if rng.chance(0.6) { dark } else { light };
    // face and body: each slider somewhere between the parents' (or past them, a little)
    let mut shape = BTreeMap::new();
    for id in a.shape.keys().chain(b.shape.keys()) {
        if shape.contains_key(id) || SEXED.contains(&id.as_str()) {
            continue;
        }
        let (va, vb) = (a.shape.get(id).copied().unwrap_or(0.0), b.shape.get(id).copied().unwrap_or(0.0));
        let t = rng.range_f32(0.2, 0.8);
        shape.insert(id.clone(), (va + (vb - va) * t + rng.range_f32(-0.12, 0.12)).clamp(-1.0, 1.0));
    }
    let mut g = Genes {
        skin: mix3(rng, a.skin, b.skin, true, 0.02),
        hair: mix3(rng, hair, hair, false, 0.03),
        eyes: mix3(rng, a.eyes, b.eyes, false, 0.0),
        height: ((a.height + b.height) * 0.5 + rng.range_f32(-0.04, 0.04)).clamp(0.88, 1.12),
        build: ((a.build + b.build) * 0.5 + rng.range_f32(-0.15, 0.15)).clamp(0.0, 1.0),
        shape,
    };
    // the occasional throwback to a great-grandparent nobody remembers
    if rng.chance(0.05) {
        g.eyes = *rng.pick(&EYE_COLORS).unwrap();
    }
    if rng.chance(0.03) {
        g.hair = [0.6, 0.25, 0.1];
    }
    g
}

/// One starting trait for a newborn: usually one of a parent's, sometimes something new.
pub fn inherit_trait(rng: &mut SimRng, data: &GameData, a: &Personality, b: &Personality) -> Option<String> {
    let pool: Vec<&String> = a.traits.iter().chain(b.traits.iter()).collect();
    if !pool.is_empty() && rng.chance(0.7) {
        return rng.pick(&pool).map(|s| (*s).clone());
    }
    rng.pick(&data.traits).map(|t| t.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(skin: f32, hair: f32, height: f32) -> Genes {
        Genes { skin: [skin; 3], hair: [hair; 3], eyes: EYE_COLORS[2], height, build: 0.5, shape: BTreeMap::new() }
    }

    #[test]
    fn child_genes_stay_between_parents() {
        let mut rng = SimRng::new(3);
        let (a, b) = (g(0.2, 0.1, 0.92), g(0.9, 0.8, 1.08));
        for _ in 0..200 {
            let c = inherit(&mut rng, &a, &b);
            assert!(c.skin[0] >= 0.15 && c.skin[0] <= 0.95, "skin blends: {:?}", c.skin);
            assert!(c.height >= 0.88 && c.height <= 1.12);
            assert!(c.build >= 0.0 && c.build <= 1.0);
        }
    }

    #[test]
    fn identical_parents_mostly_breed_true() {
        let mut rng = SimRng::new(9);
        let a = g(0.5, 0.3, 1.0);
        let same = (0..100).filter(|_| (inherit(&mut rng, &a, &a).hair[0] - 0.3).abs() < 0.05).count();
        assert!(same > 90, "{same}");
    }

    #[test]
    fn children_get_faces_between_their_parents() {
        let mut rng = SimRng::new(4);
        let mut a = g(0.5, 0.3, 1.0);
        let mut b = g(0.5, 0.3, 1.0);
        a.shape.insert("jaw".into(), -0.8);
        b.shape.insert("jaw".into(), 0.8);
        a.shape.insert("nose_length".into(), 0.6);
        a.shape.insert("frame".into(), 0.9);
        for _ in 0..100 {
            let c = inherit(&mut rng, &a, &b);
            let jaw = c.shape["jaw"];
            assert!((-0.8..=0.8).contains(&jaw), "jaw {jaw}");
            assert!(c.shape["nose_length"] > 0.0, "one parent's long nose and an average one");
            assert!(!c.shape.contains_key("frame"), "sex-linked sliders are set by sex");
        }
    }

    #[test]
    fn random_shapes_follow_sex() {
        let mut rng = SimRng::new(11);
        for _ in 0..50 {
            let m = random_shape(&mut rng, Sex::Male);
            let f = random_shape(&mut rng, Sex::Female);
            assert!(m["frame"] > 0.0 && f["frame"] < 0.0);
            assert!(m.values().chain(f.values()).all(|v| (-1.0..=1.0).contains(v)));
            assert!(FACE_SLIDERS.iter().chain(BODY_SLIDERS.iter()).all(|(id, _)| m.contains_key(*id)));
        }
    }

    #[test]
    fn express_keeps_clothes_and_grooming() {
        let worn = Appearance { top: DRESS, bottom: 2, facial_hair: 3, hair_style: 7, shoes: [1.0; 3], ..Default::default() };
        let mut genes = g(0.5, 0.1, 1.0);
        genes.shape.insert("chin".into(), 0.5);
        let app = express(&genes, LifeStage::Adult, &worn);
        assert_eq!((app.top, app.bottom, app.facial_hair, app.hair_style, app.shoes), (DRESS, 2, 3, 7, [1.0; 3]));
        assert_eq!(app.shape("chin"), 0.5);
        assert_eq!(app.skin, genes.skin);
    }

    #[test]
    fn old_appearances_load_with_defaults() {
        let old = r#"(skin: (0.5, 0.4, 0.3), hair: (0.1, 0.1, 0.1), shirt: (1.0, 0.0, 0.0), pants: (0.0, 0.0, 1.0), height: 1.0, build: 0.4, hair_style: 2)"#;
        let a: Appearance = ron::from_str(old).unwrap();
        assert_eq!((a.top, a.bottom, a.facial_hair), (0, 0, 0));
        assert!(a.shape.is_empty());
        assert_eq!(HAIR_STYLES[a.hair_style as usize].0, "long", "old style indices keep their look");
    }

    #[test]
    fn elders_go_grey_but_keep_their_genes() {
        let genes = g(0.5, 0.1, 1.0);
        let app = express(&genes, LifeStage::Elder, &Appearance::default());
        assert_eq!(app.hair, ELDER_HAIR);
        assert_eq!(genes.hair, [0.1; 3]);
    }
}
