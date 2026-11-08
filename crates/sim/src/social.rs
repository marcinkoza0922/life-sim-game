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
}

impl Rel {
    pub fn label(&self) -> &'static str {
        if self.married {
            "Spouse"
        } else if self.partners {
            "Partner"
        } else if self.romance >= 40.0 {
            "Sweetheart"
        } else if self.romance >= 15.0 {
            "Crush"
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
    Ok(())
}

/// Probability the target goes along with it.
pub fn accept_chance(world: &World, def: &SocialDef, a: Entity, b: Entity) -> f32 {
    if def.care {
        return def.accept;
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
    }
    p.clamp(0.05, 0.98)
}
