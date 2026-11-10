//! Relationships between peons.

use std::collections::BTreeMap;

use hearth_ecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::components::{Age, Family, Mood, Personality};
use crate::data::SocialDef;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Rel {
    /// -100..100
    pub friendship: f32,
    /// 0..100
    pub romance: f32,
    pub partners: bool,
    pub married: bool,
    /// Engaged to be married (also `partners`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub engaged: bool,
    /// When the planned wedding starts (see [`crate::romance::weddings`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wedding_at: Option<u64>,
    /// They used to be together.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exes: bool,
}

impl Rel {
    pub fn label(&self) -> &'static str {
        if self.married {
            "Spouse"
        } else if self.engaged {
            "Engaged"
        } else if self.partners {
            "Partner"
        } else if self.romance >= 40.0 {
            "Sweetheart"
        } else if self.romance >= 15.0 {
            "Crush"
        } else if self.exes {
            "Ex"
        } else if self.friendship >= 70.0 {
            "Best Friend"
        } else if self.friendship >= 30.0 {
            "Friend"
        } else if self.friendship <= -50.0 {
            "Nemesis"
        } else if self.friendship <= -20.0 {
            "Enemy"
        } else {
            "Acquaintance"
        }
    }
}

/// Symmetric relationships keyed by the ordered entity pair.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Relationships {
    pub map: BTreeMap<(Entity, Entity), Rel>,
}

fn key(a: Entity, b: Entity) -> (Entity, Entity) {
    if a < b { (a, b) } else { (b, a) }
}

impl Relationships {
    pub fn get(&self, a: Entity, b: Entity) -> Rel {
        self.map.get(&key(a, b)).copied().unwrap_or_default()
    }
    pub fn get_mut(&mut self, a: Entity, b: Entity) -> &mut Rel {
        self.map.entry(key(a, b)).or_default()
    }
    pub fn add(&mut self, a: Entity, b: Entity, friendship: f32, romance: f32) {
        let r = self.get_mut(a, b);
        r.friendship = (r.friendship + friendship).clamp(-100.0, 100.0);
        r.romance = (r.romance + romance).clamp(0.0, 100.0);
    }
    /// Are they a couple (partners, engaged or married)?
    pub fn together(&self, a: Entity, b: Entity) -> bool {
        let r = self.get(a, b);
        r.partners || r.engaged || r.married
    }
    /// Everyone `a` has a relationship with.
    pub fn of(&self, a: Entity) -> Vec<(Entity, Rel)> {
        self.map
            .iter()
            .filter_map(|(&(x, y), r)| {
                if x == a {
                    Some((y, *r))
                } else if y == a {
                    Some((x, *r))
                } else {
                    None
                }
            })
            .collect()
    }
    /// Relationships slowly drift toward neutral.
    pub fn daily_decay(&mut self) {
        for r in self.map.values_mut() {
            r.friendship -= r.friendship.signum() * r.friendship.abs().min(1.0);
            if !r.partners {
                r.romance = (r.romance - 1.0).max(0.0);
            }
        }
    }
}

/// Parents, children and siblings are family (no romance).
pub fn is_family(world: &World, a: Entity, b: Entity) -> bool {
    let (Some(fa), Some(fb)) = (world.get::<Family>(a), world.get::<Family>(b)) else { return false };
    fa.parents.contains(&b)
        || fb.parents.contains(&a)
        || fa.parents.iter().any(|p| fb.parents.contains(p))
}

/// Can `a` start `def` with `b` at all (ignoring randomness)?
pub fn social_allowed(world: &World, def: &SocialDef, a: Entity, b: Entity) -> Result<(), &'static str> {
    social_allowed_with(world, def, a, b, None)
}

pub(crate) fn social_allowed_with(world: &World, def: &SocialDef, a: Entity, b: Entity, commitments: Option<&crate::romance::Commitments<'_>>) -> Result<(), &'static str> {
    let (Some(aa), Some(ab)) = (world.get::<Age>(a), world.get::<Age>(b)) else { return Err("unavailable") };
    if !def.initiator_ok(aa.stage) {
        return Err("too young or too old");
    }
    if !def.target_ok(ab.stage) {
        return Err("not appropriate for target");
    }
    if let Some(t) = &def.requires_trait
        && !world.get::<Personality>(a).is_some_and(|p| p.has(t)) {
            return Err("missing trait");
        }
    let rel = world.res::<Relationships>().get(a, b);
    if rel.friendship < def.min_friendship {
        return Err("not close enough");
    }
    if rel.romance < def.min_romance {
        return Err("not romantic enough");
    }
    if def.romantic && (is_family(world, a, b) || !aa.stage.is_adult() || !ab.stage.is_adult()) {
        return Err("absolutely not");
    }
    social_pair_ok_with(world, def, a, b, commitments)
}

/// The pair-specific gates (sexes, where they stand with each other). When these fail the social
/// isn't offered at all, rather than shown greyed out.
pub fn social_pair_ok(world: &World, def: &SocialDef, a: Entity, b: Entity) -> Result<(), &'static str> {
    social_pair_ok_with(world, def, a, b, None)
}

fn social_pair_ok_with(world: &World, def: &SocialDef, a: Entity, b: Entity, commitments: Option<&crate::romance::Commitments<'_>>) -> Result<(), &'static str> {
    if def.opposite_sex && !crate::romance::opposite_sex(world, a, b) {
        return Err("only for a woman and a man");
    }
    if let Some(req) = def.requires {
        crate::romance::rel_req_ok_with(world, req, a, b, commitments)?;
    }
    Ok(())
}

/// Probability the target goes along with it.
pub fn accept_chance(world: &World, def: &SocialDef, a: Entity, b: Entity) -> f32 {
    accept_chance_with(world, def, a, b, None)
}

pub(crate) fn accept_chance_with(world: &World, def: &SocialDef, a: Entity, b: Entity, commitments: Option<&crate::romance::Commitments<'_>>) -> f32 {
    if def.care {
        return def.accept;
    }
    if def.certain {
        return 1.0;
    }
    let rel = world.res::<Relationships>().get(a, b);
    let mood = world.get::<Mood>(b).map(|m| m.value).unwrap_or(0.0);
    let mut p = def.accept * (0.75 + mood / 400.0) + rel.friendship / 250.0;
    if def.romantic {
        p += rel.romance / 200.0;
    }
    if let Some(pers) = world.get::<Personality>(b) {
        if pers.has("loner") && def.tags.iter().any(|t| t == "social") {
            p -= 0.1;
        }
        if pers.has("social_butterfly") {
            p += 0.1;
        }
        if pers.has("romantic") && def.romantic {
            p += 0.1;
        }
        if def.id == "propose" && (pers.has("hothead") || pers.has("gloomy")) {
            p -= 0.2;
        }
    }
    if def.id == "ask_move_in" && commitments.map_or_else(|| crate::romance::committed_to(world, b), |c| c.get(b)).is_some_and(|c| c != a) {
        // they'd be leaving someone
        p -= 0.5;
    }
    p.clamp(0.05, 0.98)
}

#[cfg(test)]
mod cached_tests {
    use super::*;

    #[test]
    fn decision_lookup_preserves_social_gates_and_acceptance() {
        let mut sim = crate::starter::starter_sim(7);
        sim.populate_neighborhood();
        let peons = sim.world.query::<Entity>().iter().filter(|&e| sim.world.has::<Age>(e)).collect::<Vec<_>>();
        let data = sim.world.res::<crate::sim::Data>().0.clone();
        for phase in 0..2 {
            if phase == 1 {
                // Multiple commitments, a dead partner, and an absent generation.
                let mut rels = sim.world.res_mut::<Relationships>();
                for (i, &a) in peons.iter().enumerate() {
                    for (j, &b) in peons.iter().enumerate().skip(i + 1) {
                        let r = rels.get_mut(a, b);
                        r.friendship = ((i * 13 + j * 7) % 201) as f32 - 100.0;
                        r.romance = ((i * 7 + j * 11) % 101) as f32;
                        r.partners = (i + j) % 5 == 0;
                        r.engaged = (i + j) % 7 == 0;
                        r.married = (i + j) % 11 == 0;
                    }
                }
                rels.get_mut(peons[0], Entity::PLACEHOLDER).partners = true;
                drop(rels);
                sim.world.insert_one(peons[1], crate::components::Dead { cause: "test".into(), tick: 0 });
            }
            let lookup = crate::romance::Commitments::new(&sim.world);
            for &a in &peons {
                // Compare to the original allocating, sorted implementation.
                let mut expected = sim.world.res::<Relationships>().of(a).into_iter()
                    .filter(|(e, r)| (r.partners || r.engaged || r.married) && sim.world.contains(*e) && !sim.world.has::<crate::components::Dead>(*e))
                    .map(|(e, _)| e).collect::<Vec<_>>();
                expected.sort();
                assert_eq!(crate::romance::committed_to(&sim.world, a), expected.first().copied());
                assert_eq!(lookup.get(a), expected.first().copied());
                for &b in &peons {
                    for def in &data.socials {
                        assert_eq!(social_allowed_with(&sim.world, def, a, b, Some(&lookup)), social_allowed(&sim.world, def, a, b));
                        assert_eq!(accept_chance_with(&sim.world, def, a, b, Some(&lookup)).to_bits(), accept_chance(&sim.world, def, a, b).to_bits());
                    }
                }
            }
        }
    }
}
