//! Aging, life-stage transitions and death.

use hearth_ecs::{Entity, Without, World};

use crate::actions;
use crate::components::*;
use crate::data::{LifeStage, Need, Skill};
use crate::economy::Household;
use crate::log::LogKind;
use crate::rng::SimRng;
use crate::sim::{add_moodlet, data, log, name_of, now};
use crate::social::Relationships;
use crate::time::{Settings, TICKS_PER_DAY};

pub fn aging(world: &mut World) {
    let settings = world.res::<Settings>().clone();
    if !settings.aging {
        return;
    }
    let step = 1.0 / TICKS_PER_DAY as f32;
    let mut grow = Vec::new();
    let mut elders_due = Vec::new();
    for (e, age) in world.query_filtered::<(Entity, &mut Age), Without<Dead>>().iter() {
        age.days_in_stage += step;
        age.total_days += step;
        if age.days_in_stage >= age.stage_len {
            if age.stage == LifeStage::Elder {
                elders_due.push(e);
            } else {
                grow.push(e);
            }
        }
    }
    for e in grow {
        grow_up(world, e);
    }
    for e in elders_due {
        // on average ~2 extra days once past the stage length
        if world.res_mut::<SimRng>().chance(1.0 / (2.0 * TICKS_PER_DAY as f32)) {
            die(world, e, "old age");
        }
    }
}

pub fn grow_up(world: &mut World, e: Entity) {
    let data = data(world);
    let scale = world.res::<Settings>().lifespan_scale;
    let jitter = world.res_mut::<SimRng>().range_f32(0.9, 1.1);
    let new_stage = {
        let mut age = world.get_mut::<Age>(e).unwrap();
        let Some(next) = age.stage.next() else { return };
        age.stage = next;
        age.days_in_stage = 0.0;
        age.stage_len = next.base_days() * scale * jitter;
        next
    };
    // gain a trait at milestones
    let target_traits = match new_stage {
        LifeStage::Toddler | LifeStage::Infant => 1,
        LifeStage::Child => 2,
        LifeStage::Teen => 2,
        _ => 3,
    };
    let have = world.get::<Personality>(e).unwrap().traits.clone();
    if have.len() < target_traits {
        let pick = {
            let mut rng = world.res_mut::<SimRng>();
            let opts: Vec<_> = data
                .traits
                .iter()
                .filter(|t| !have.contains(&t.id) && !have.iter().any(|h| t.conflicts.contains(h)))
                .collect();
            rng.pick(&opts).map(|t| (t.id.clone(), t.name.clone(), t.grants))
        };
        if let Some((id, tname, grants)) = pick {
            world.get_mut::<Personality>(e).unwrap().traits.push(id);
            if let Some(n) = grants {
                world.get_mut::<Needs>(e).unwrap().active[n.idx()] = true;
            }
            log(world, LogKind::Life, Some(e), format!("{} has developed a new trait: {}.", name_of(world, e), tname));
        }
    }
    if new_stage == LifeStage::Elder {
        world.get_mut::<Appearance>(e).unwrap().hair = [0.82, 0.82, 0.84];
    }
    add_moodlet(world, e, "birthday");
    let name = name_of(world, e);
    let line = match new_stage {
        LifeStage::Toddler => format!("{name} is now a Toddler and has immediately learned the word 'no'."),
        LifeStage::Child => format!("{name} grew into a Child! They have opinions about dinosaurs now."),
        LifeStage::Teen => format!("{name} is a Teen! They are already embarrassed by everyone in this house."),
        LifeStage::YoungAdult => format!("{name} is a Young Adult. Rent is a concept they are about to learn about."),
        LifeStage::Adult => format!("{name} is now an Adult. Their back has started making noises."),
        LifeStage::Elder => format!("{name} became an Elder and now tells everyone about 'the good old days' (last Tuesday)."),
        LifeStage::Infant => format!("{name} was born."),
    };
    log(world, LogKind::Life, Some(e), line);
    // stage-gated actions in the queue may no longer be valid; let autonomy re-think
    actions::abort_current(world, e);
}

/// Death, with a chance to haggle with the Reaper.
pub fn die(world: &mut World, e: Entity, cause: &str) {
    if world.has::<Dead>(e) {
        return;
    }
    let name = name_of(world, e);
    // the Reaper is open to negotiation, once
    let charisma = world.get::<Skills>(e).map(|s| s.get(Skill::Charisma)).unwrap_or(0.0);
    let already = world.get::<Mood>(e).is_some_and(|m| m.moodlets.iter().any(|x| x.id == "cheated_death"));
    let haggle = 0.1 + charisma * 0.04;
    if !already && cause != "old age" && world.res_mut::<SimRng>().chance(haggle) {
        log(world, LogKind::Funny, Some(e), format!("The Reaper came for {name} ({cause}), but lost at rock-paper-scissors. Best two out of three. Then three out of five."));
        add_moodlet(world, e, "cheated_death");
        if let Some(mut n) = world.get_mut::<Needs>(e) {
            for k in Need::CORE {
                let v = n.get(k).max(50.0);
                n.set(k, v);
            }
            n.starving_minutes = 0;
        }
        return;
    }
    actions::abort_current(world, e);
    if let Some(mut q) = world.get_mut::<ActionQueue>(e) {
        q.queue.clear();
    }
    let tick = now(world);
    world.insert_one(e, Dead { cause: cause.into(), tick });
    let line = match cause {
        "old age" => format!("The Reaper arrived for {name}. They went peacefully, after asking the Reaper to wipe his feet."),
        "starvation" => format!("{name} has died of starvation. The fridge was RIGHT THERE."),
        "laughter" => format!("{name} laughed so hard they died. It was a really good joke."),
        other => format!("{name} has died ({other})."),
    };
    log(world, LogKind::Death, Some(e), line);
    // grief
    let rels = world.res::<Relationships>().of(e);
    for (other, rel) in rels {
        if rel.friendship > 20.0 || rel.romance > 20.0 {
            if !world.has::<Dead>(other) {
                add_moodlet(world, other, "grieving");
            }
        }
    }
    world.res_mut::<Household>().members.retain(|m| *m != e);
}

/// Takes a peon off the lot entirely (e.g. social services).
pub fn remove_from_lot(world: &mut World, e: Entity) {
    actions::abort_current(world, e);
    world.res_mut::<Household>().members.retain(|m| *m != e);
    // drop anyone else's references by just despawning; dangling handles are detected
    world.despawn(e);
}
