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
    /// The lot they live on (index into `Neighborhood::lots`).
    #[serde(default = "first_lot")]
    pub home: Option<usize>,
}

fn first_lot() -> Option<usize> {
    Some(0)
}

impl Default for Household {
    fn default() -> Self {
        Household { name: "Household".into(), funds: 20_000, members: Vec::new(), unpaid: 0, unpaid_since: None, home: Some(0) }
    }
}

/// What a lot's build and furniture are worth.
pub fn build_value(lot: &crate::lot::Lot, object_prices: impl Iterator<Item = i64>) -> i64 {
    object_prices.sum::<i64>() + lot.walls.len() as i64 * 70 + lot.floors.len() as i64 * 10
}

/// Value of the active lot.
pub fn lot_value(world: &World) -> i64 {
    build_value(&world.res::<crate::lot::Lot>(), world.query::<&ObjectInstance>().iter().map(|o| o.price_paid))
}

/// Value of the active household's home, wherever they are right now.
pub fn home_value(world: &World) -> i64 {
    let home = world.res::<Household>().home;
    let hood = world.res::<crate::neighborhood::Neighborhood>();
    match home {
        Some(h) if h != hood.active => hood.lots.get(h).map(|l| l.build_value()).unwrap_or(0),
        _ => {
            drop(hood);
            lot_value(world)
        }
    }
}

/// Pays `amount` into the household `e` belongs to (the active one, or a neighbor's).
pub fn credit(world: &World, e: Entity, amount: i64) {
    let mut hh = world.res_mut::<Household>();
    if hh.members.contains(&e) {
        hh.funds += amount;
        return;
    }
    drop(hh);
    let mut hood = world.res_mut::<crate::neighborhood::Neighborhood>();
    if let Some(h) = hood.households.iter_mut().find(|h| h.members.contains(&e)) {
        h.funds += amount;
    }
}

/// Bills arrive every three days at 9:00.
pub fn bills(world: &mut World) {
    let clock = *world.res::<Clock>();
    if !world.res::<Settings>().bills || clock.minute_of_day() != 9 * 60 || clock.day() % 3 != 2 {
        return;
    }
    let amount = 40 + home_value(world) / 100;
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
        // the Repo-Gnome only calls when someone's home
        if clock.day() >= since + 6 && crate::neighborhood::at_home(world) {
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
