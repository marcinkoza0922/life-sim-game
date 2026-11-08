use hearth_ecs::{Entity, World};

use crate::log::LogKind;
use crate::objects::ObjectInstance;
use crate::sim::{data, log};
use crate::time::{Clock, Settings};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Household {
    pub name: String,
    pub funds: i64,
    pub members: Vec<Entity>,
    pub unpaid: i64,
    pub unpaid_since: Option<u64>,
}

impl Default for Household {
    fn default() -> Self {
        Household { name: "Household".into(), funds: 20_000, members: Vec::new(), unpaid: 0, unpaid_since: None }
    }
}

pub fn lot_value(world: &World) -> i64 {
    world.query::<&ObjectInstance>().iter().map(|o| o.price_paid).sum::<i64>()
        + world.res::<crate::lot::Lot>().walls.len() as i64 * 70
        + world.res::<crate::lot::Lot>().floors.len() as i64 * 10
}

/// Bills arrive every three days at 9:00.
pub fn bills(world: &mut World) {
    let clock = *world.res::<Clock>();
    if !world.res::<Settings>().bills || clock.minute_of_day() != 9 * 60 || clock.day() % 3 != 2 {
        return;
    }
    let amount = 40 + lot_value(world) / 100;
    let mut hh = world.res_mut::<Household>();
    let total = amount + hh.unpaid;
    if hh.funds >= total {
        hh.funds -= total;
        hh.unpaid = 0;
        hh.unpaid_since = None;
        drop(hh);
        log(world, LogKind::Money, None, format!("Bills paid: ${total}. The electric company sends its regards."));
    } else {
        hh.unpaid = total;
        let since = *hh.unpaid_since.get_or_insert(clock.day());
        drop(hh);
        log(world, LogKind::Warning, None, format!("Couldn't pay ${total} in bills! The envelopes are getting redder."));
        if clock.day() >= since + 6 {
            repossess(world);
        }
    }
}

fn repossess(world: &mut World) {
    let data = data(world);
    let target = world
        .query::<(Entity, &ObjectInstance)>()
        .iter()
        .max_by_key(|(e, o)| (o.price_paid, std::cmp::Reverse(*e)))
        .map(|(e, o)| (e, data.object(o.def).name.clone(), o.price_paid));
    if let Some((e, name, price)) = target {
        world.despawn(e);
        let mut lot = world.res_mut::<crate::lot::Lot>();
        lot.occupancy.retain(|_, v| *v != e);
        lot.rooms_dirty = true;
        lot.version += 1;
        drop(lot);
        let mut hh = world.res_mut::<Household>();
        hh.unpaid = (hh.unpaid - price).max(0);
        if hh.unpaid == 0 {
            hh.unpaid_since = None;
        }
        drop(hh);
        log(world, LogKind::Funny, None, format!("The Repo-Gnome took the {name}. He tipped his tiny hat. It did not help."));
    }
}
