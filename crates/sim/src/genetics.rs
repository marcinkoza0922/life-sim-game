//! Genetics: a small gene vector per peon, inherited per gene from two parents with mutation.
//! [`Genes`] is the genotype; [`Appearance`] is what gets rendered (elders go grey, clothes are chosen).

use crate::components::{Appearance, Genes, Personality};
use crate::data::{GameData, LifeStage};
use crate::rng::SimRng;

pub const EYE_COLORS: [[f32; 3]; 5] = [
    [0.25, 0.15, 0.08], // brown
    [0.12, 0.08, 0.05], // dark brown
    [0.3, 0.45, 0.7],   // blue
    [0.3, 0.5, 0.3],    // green
    [0.5, 0.4, 0.25],   // hazel
];

pub const ELDER_HAIR: [f32; 3] = [0.82, 0.82, 0.84];

/// Builds the rendered appearance for a genotype at a life stage.
pub fn express(g: &Genes, stage: LifeStage, shirt: [f32; 3], pants: [f32; 3], hair_style: u8) -> Appearance {
    Appearance {
        skin: g.skin,
        hair: if stage == LifeStage::Elder { ELDER_HAIR } else { g.hair },
        shirt,
        pants,
        height: g.height,
        build: g.build,
        hair_style,
    }
}

/// Best-effort genotype for peons created without one (older specs, hand-made peons).
pub fn genes_from_appearance(a: &Appearance) -> Genes {
    Genes { skin: a.skin, hair: a.hair, eyes: EYE_COLORS[0], height: a.height, build: a.build }
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
/// dominant), height and build average with noise, plus a small chance of a wild mutation.
pub fn inherit(rng: &mut SimRng, a: &Genes, b: &Genes) -> Genes {
    let lum = |c: [f32; 3]| c[0] + c[1] + c[2];
    let (dark, light) = if lum(a.hair) <= lum(b.hair) { (a.hair, b.hair) } else { (b.hair, a.hair) };
    let hair = if rng.chance(0.6) { dark } else { light };
    let mut g = Genes {
        skin: mix3(rng, a.skin, b.skin, true, 0.02),
        hair: mix3(rng, hair, hair, false, 0.03),
        eyes: mix3(rng, a.eyes, b.eyes, false, 0.0),
        height: ((a.height + b.height) * 0.5 + rng.range_f32(-0.04, 0.04)).clamp(0.88, 1.12),
        build: ((a.build + b.build) * 0.5 + rng.range_f32(-0.15, 0.15)).clamp(0.0, 1.0),
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
        Genes { skin: [skin; 3], hair: [hair; 3], eyes: EYE_COLORS[2], height, build: 0.5 }
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
    fn elders_go_grey_but_keep_their_genes() {
        let genes = g(0.5, 0.1, 1.0);
        let app = express(&genes, LifeStage::Elder, [0.0; 3], [0.0; 3], 0);
        assert_eq!(app.hair, ELDER_HAIR);
        assert_eq!(genes.hair, [0.1; 3]);
    }
}
