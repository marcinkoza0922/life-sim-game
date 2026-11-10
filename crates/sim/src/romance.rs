//! Romance past the first kiss: proposals, engagements, weddings (guests, a cake, a little chaos),
//! breakups and divorce, jealousy, and the same thing happening down the street on its own.
//!
//! Engagements and weddings are between a woman and a man (`SocialDef::opposite_sex`); moving in
//! together ([`crate::neighborhood::move_in_with`]) is open to any two adults.

use hearth_ecs::{Entity, With, Without, World};

use crate::actions;
use crate::components::*;
use crate::data::{LifeStage, Need, RelReq};
use crate::family;
use crate::log::LogKind;
use crate::lot::Lot;
use crate::neighborhood::{self, Hh};
use crate::rng::SimRng;
use crate::sim::{add_moodlet, data, log, name_of, now, present_peons, set_bubble};
use crate::social::{Relationships, is_family};
use crate::time::{Clock, TICKS_PER_DAY};

/// What planning a wedding costs the household (a cake and folding chairs).
pub const WEDDING_COST: i64 = 300;
/// The cake that turns up for a wedding.
pub const WEDDING_CAKE: &str = "wedding_cake";
/// Most guests invited to a wedding.
const MAX_GUESTS: usize = 6;

// ------------------------------------------------------------------------------------------------
// queries

fn alive(world: &World, e: Entity) -> bool {
    world.contains(e) && !world.has::<Dead>(e)
}

/// Whoever (alive) `e` is partnered with, engaged to or married to.
pub fn committed_to(world: &World, e: Entity) -> Option<Entity> {
    world.res::<Relationships>().map.iter().filter_map(|(&(x, y), r)| {
        if !(r.partners || r.engaged || r.married) { return None; }
        let other = if x == e { y } else if y == e { x } else { return None };
        alive(world, other).then_some(other)
    }).min()
}

/// Immutable partner lookup for one read-only autonomy decision. No cache survives
/// a world mutation; malformed multiple partnerships still choose the lowest entity.
pub(crate) struct Commitments(std::collections::BTreeMap<Entity, Entity>);

impl Commitments {
    pub(crate) fn new(world: &World) -> Self {
        let mut partners = std::collections::BTreeMap::<Entity, Entity>::new();
        for (&(a, b), r) in &world.res::<Relationships>().map {
            if !(r.partners || r.engaged || r.married) { continue; }
            for (actor, partner) in [(a, b), (b, a)] {
                if alive(world, partner) {
                    partners.entry(actor).and_modify(|p| *p = (*p).min(partner)).or_insert(partner);
                }
            }
        }
        Self(partners)
    }

    pub(crate) fn get(&self, e: Entity) -> Option<Entity> { self.0.get(&e).copied() }
}

/// When `e`'s own wedding is planned for, if it is.
pub fn own_wedding(world: &World, e: Entity) -> Option<u64> {
    world.res::<Relationships>().of(e).into_iter().filter(|(_, r)| r.engaged).filter_map(|(_, r)| r.wedding_at).min()
}

/// When `e` is marrying someone in the played household (so the wedding is on their lot).
pub fn wedding_with_household(world: &World, e: Entity) -> Option<u64> {
    world.res::<Relationships>().of(e).into_iter().filter(|(o, r)| r.engaged && world.has::<Member>(*o)).filter_map(|(_, r)| r.wedding_at).min()
}

pub fn opposite_sex(world: &World, a: Entity, b: Entity) -> bool {
    matches!((world.get::<Sex>(a).map(|s| *s), world.get::<Sex>(b).map(|s| *s)), (Some(x), Some(y)) if x != y)
}

/// Does the pair stand where `req` needs them to?
pub fn rel_req_ok(world: &World, req: RelReq, a: Entity, b: Entity) -> Result<(), &'static str> {
    rel_req_ok_with(world, req, a, b, None)
}

pub(crate) fn rel_req_ok_with(world: &World, req: RelReq, a: Entity, b: Entity, commitments: Option<&Commitments>) -> Result<(), &'static str> {
    let partner = |e| commitments.map_or_else(|| committed_to(world, e), |c| c.get(e));
    let r = world.res::<Relationships>().get(a, b);
    let ok = match req {
        RelReq::Unattached => partner(a).is_none() && partner(b).is_none(),
        RelReq::Engaged => r.engaged && r.wedding_at.is_none(),
        RelReq::Betrothed => r.engaged,
        RelReq::Together => r.partners || r.engaged || r.married,
        RelReq::LiveApart => {
            let (ha, hb) = (neighborhood::household_of(world, a), neighborhood::household_of(world, b));
            ha.is_some() && hb.is_some() && ha != hb
        }
    };
    if ok {
        Ok(())
    } else {
        Err(match req {
            RelReq::Unattached => "one of them is spoken for",
            RelReq::Engaged | RelReq::Betrothed => "they aren't engaged",
            RelReq::Together => "they aren't together",
            RelReq::LiveApart => "they already live together",
        })
    }
}

// ------------------------------------------------------------------------------------------------
// proposals and weddings

/// A proposal was accepted.
pub fn engage(world: &World, a: Entity, b: Entity) {
    {
        let mut rels = world.res_mut::<Relationships>();
        let r = rels.get_mut(a, b);
        r.engaged = true;
        r.partners = true;
        r.exes = false;
        r.wedding_at = None;
    }
    add_moodlet(world, a, "engaged");
    add_moodlet(world, b, "engaged");
    const LINES: [&str; 4] = [
        "{a} got down on one knee and proposed to {b}. {b} said yes before the question was finished!",
        "{a} proposed to {b} with a ring hidden in a sandwich. {b} found it the hard way, and said yes.",
        "{a} asked {b} to marry them. {b} said yes, then cried, then said yes again to be sure.",
        "{a} proposed to {b}. The ring was a little big. The answer was not: yes!",
    ];
    let i = world.res_mut::<SimRng>().below(LINES.len());
    log(world, LogKind::Life, Some(a), LINES[i].replace("{a}", &name_of(world, a)).replace("{b}", &name_of(world, b)));
}

/// When a wedding planned now would start: in three hours, kept to the afternoon or evening
/// (guests only come over 8:00–21:00).
fn wedding_time(tick: u64) -> u64 {
    let t = tick + 3 * 60;
    let m = Clock { tick: t }.minute_of_day();
    if m < 11 * 60 {
        t + (11 * 60 - m)
    } else if m > 19 * 60 {
        t + (TICKS_PER_DAY - m) + 14 * 60
    } else {
        t
    }
}

/// "Plan the Wedding": sets the date, invites friends and family, and orders a cake.
pub fn plan_wedding(world: &mut World, a: Entity, b: Entity) {
    let at = wedding_time(now(world));
    world.res_mut::<Relationships>().get_mut(a, b).wedding_at = Some(at);
    let mut guests: Vec<(Entity, f32)> = world
        .query_filtered::<(Entity, &Age), (With<Offlot>, Without<Dead>, Without<Member>)>()
        .iter()
        .filter(|(g, age)| age.stage >= LifeStage::Teen && *g != a && *g != b)
        .map(|(g, _)| g)
        .collect::<Vec<_>>()
        .into_iter()
        .filter_map(|g| {
            let rels = world.res::<Relationships>();
            let f = rels.get(a, g).friendship.max(rels.get(b, g).friendship);
            let kin = is_family(world, a, g) || is_family(world, b, g);
            (f >= 25.0 || kin).then_some((g, if kin { 200.0 } else { f }))
        })
        .collect();
    guests.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
    guests.truncate(MAX_GUESTS);
    for &(g, _) in &guests {
        world.get_mut::<Offlot>(g).unwrap().visit_at = at.saturating_sub(40);
    }
    expect_couple(world, a, b, at);
    let paid = crate::economy::debit(world, a, WEDDING_COST);
    let cake = paid && place_cake(world, a);
    let (na, nb) = (name_of(world, a), name_of(world, b));
    let clock = Clock { tick: at };
    let when = format!("{} {:02}:{:02}", clock.weekday_name(), clock.hour(), clock.minute_of_day() % 60);
    let extra = match (paid, cake) {
        (true, true) => format!(" The cake has arrived (${WEDDING_COST}); it is already leaning."),
        (true, false) => format!(" ${WEDDING_COST} went on folding chairs; there was nowhere to put a cake."),
        (false, _) => " They can't afford a cake. It'll be a 'rustic' affair.".to_string(),
    };
    log(world, LogKind::Life, Some(a), format!("{na} and {nb} are getting married {when}! {} guests invited.{extra}", guests.len()));
}

/// A neighbor bride or groom stays for their wedding at `at` (or comes back in time).
fn expect_couple(world: &mut World, a: Entity, b: Entity, at: u64) {
    for e in [a, b] {
        if let Some(mut v) = world.get_mut::<Visitor>(e) {
            v.leave_at = v.leave_at.max(at + 120);
        } else if let Some(mut o) = world.get_mut::<Offlot>(e)
            && !world.has::<Member>(e)
        {
            o.visit_at = at.saturating_sub(40);
        }
    }
}

/// The player took over a household: weddings its members had set off-stage keep their date
/// (moved to the afternoon, and never sooner than a wedding planned now), and a neighbor
/// fiancé(e) is expected over for it.
pub fn took_over(world: &mut World, members: &[Entity]) {
    let tick = now(world);
    let dated: Vec<(Entity, Entity, u64)> = world
        .res::<Relationships>()
        .map
        .iter()
        .filter(|&(&(a, b), r)| r.engaged && (members.contains(&a) || members.contains(&b)))
        .filter_map(|(&(a, b), r)| r.wedding_at.map(|t| (a, b, t)))
        .collect();
    for (a, b, at) in dated {
        let at = wedding_time(at.saturating_sub(3 * 60)).max(wedding_time(tick));
        world.res_mut::<Relationships>().get_mut(a, b).wedding_at = Some(at);
        expect_couple(world, a, b, at);
    }
}

/// Puts a free wedding cake on a clear tile near `e`. False if there's no room.
fn place_cake(world: &mut World, e: Entity) -> bool {
    let Some(pos) = world.get::<Transform>(e).map(|t| t.pos) else { return false };
    let (level, tile) = (Lot::level_of(pos), Lot::tile_of(pos));
    for r in 1..7i32 {
        for dz in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dz.abs() != r {
                    continue;
                }
                let t = tile + glam::IVec2::new(dx, dz);
                if crate::sim::place_object(world, WEDDING_CAKE, level, t, 0, false).is_ok() {
                    return true;
                }
            }
        }
    }
    false
}

/// Starts weddings that are due. If both are on the lot they exchange vows in front of whoever
/// came; if neither lives in the played household it happens off-stage (a few hours later if one
/// of them is out visiting); otherwise it's put off until the player plans another.
pub fn weddings(world: &mut World) {
    let tick = now(world);
    let due: Vec<(Entity, Entity)> = world
        .res::<Relationships>()
        .map
        .iter()
        .filter(|(_, r)| r.engaged && r.wedding_at.is_some_and(|t| t <= tick))
        .map(|(&k, _)| k)
        .collect();
    for (a, b) in due {
        world.res_mut::<Relationships>().get_mut(a, b).wedding_at = None;
        if !alive(world, a) || !alive(world, b) {
            continue;
        }
        let here = present_peons(world);
        let townie = |e: Entity| !world.has::<Member>(e) && world.has::<Offlot>(e);
        if here.contains(&a) && here.contains(&b) {
            start_ceremony(world, a, b);
        } else if townie(a) && townie(b) {
            wed(world, a, b, false);
        } else if !world.has::<Member>(a) && !world.has::<Member>(b) {
            world.res_mut::<Relationships>().get_mut(a, b).wedding_at = Some(tick + 3 * 60);
        } else {
            let missing = if here.contains(&a) { b } else { a };
            log(world, LogKind::Funny, Some(missing), format!(
                "The wedding is off for today: {} didn't show up. The cake waited. The guests ate it. They're still engaged; plan another one.",
                name_of(world, missing)
            ));
        }
    }
}

fn start_ceremony(world: &mut World, a: Entity, b: Entity) {
    let data = data(world);
    let Some(vows) = data.socials.iter().position(|s| s.id == "exchange_vows") else { return };
    for e in [a, b] {
        actions::abort_current(world, e);
        if let Some(mut q) = world.get_mut::<ActionQueue>(e) {
            q.queue.clear();
        }
    }
    let label = format!("Exchange Vows with {}", name_of(world, b));
    actions::push_front(world, a, ActionKind::Social { target: b, social: vows }, Source::System, label);
    // the other one waits at the altar (the diapers can wait too)
    actions::push_front(world, b, ActionKind::Idle { minutes: 45 }, Source::System, "Wait at the Altar".into());
    let guests: Vec<Entity> = present_peons(world).into_iter().filter(|&g| g != a && g != b).collect();
    for &g in &guests {
        set_bubble(world, g, Some("<3".into()), None, 30);
    }
    log(world, LogKind::Life, Some(a), format!(
        "The wedding of {} and {} is starting! {} {} here to watch.",
        name_of(world, a),
        name_of(world, b),
        guests.len(),
        if guests.len() == 1 { "person is" } else { "people are" }
    ));
}

/// They're married: moodlets, a story, and moving in together.
pub fn wed(world: &mut World, a: Entity, b: Entity, ceremony: bool) {
    family::marry(world, a, b);
    add_moodlet(world, a, "just_married");
    add_moodlet(world, b, "just_married");
    let (na, nb) = (name_of(world, a), name_of(world, b));
    if ceremony {
        for g in present_peons(world) {
            if g != a && g != b {
                add_moodlet(world, g, "lovely_wedding");
            }
        }
        const LINES: [&str; 6] = [
            "The ring bearer lost the ring, then found it, then swallowed it. They used a twist tie.",
            "Someone objected. It was a pigeon. It was overruled.",
            "{a} said the wrong name. It was the name of a sandwich. {b} let it go.",
            "The cake leaned further and further during the vows, and fell over at 'I do'. Perfect timing.",
            "{b} wrote their own vows. They were eleven pages long and rhymed.",
            "Everyone cried, including a stranger who was just walking past.",
        ];
        let i = world.res_mut::<SimRng>().below(LINES.len());
        let story = LINES[i].replace("{a}", &na).replace("{b}", &nb);
        log(world, LogKind::Life, Some(a), format!("{na} and {nb} are married! {story}"));
    } else {
        log(world, LogKind::Life, Some(a), format!("Down the street, {na} and {nb} got married at the courthouse. There was cake."));
    }
    move_in_together(world, a, b);
}

/// After a wedding: the spouses' households become one. Joining the played household comes first;
/// between neighbors the one from the smaller household moves.
fn move_in_together(world: &mut World, a: Entity, b: Entity) {
    let (ha, hb) = (neighborhood::household_of(world, a), neighborhood::household_of(world, b));
    if ha.is_none() || hb.is_none() || ha == hb {
        return;
    }
    let (mover, host) = if ha == Some(Hh::Played) {
        (b, a)
    } else if hb == Some(Hh::Played) || neighborhood::housemates(world, a).len() <= neighborhood::housemates(world, b).len() {
        (a, b)
    } else {
        (b, a)
    };
    if neighborhood::move_in_with(world, mover, host).is_err() && neighborhood::move_in_with(world, host, mover).is_err() {
        log(world, LogKind::Funny, Some(a), format!(
            "{} and {} will be living apart for now: neither house has room. They call it 'keeping the magic alive'.",
            name_of(world, a),
            name_of(world, b)
        ));
    }
}

/// `runner` got cold feet at the altar, leaving `left`.
pub fn cold_feet(world: &World, left: Entity, runner: Entity) {
    {
        let mut rels = world.res_mut::<Relationships>();
        let r = rels.get_mut(left, runner);
        r.engaged = false;
        r.partners = false;
        r.wedding_at = None;
        r.exes = true;
        r.romance = (r.romance - 30.0).max(0.0);
        r.friendship = (r.friendship - 20.0).max(-100.0);
    }
    add_moodlet(world, left, "left_at_altar");
    add_moodlet(world, runner, "cold_feet");
    set_bubble(world, runner, Some("!!".into()), None, 30);
    log(world, LogKind::Funny, Some(runner), format!(
        "{r} got cold feet and climbed out the bathroom window. {l} was left at the altar holding two rings and a lot of feelings.",
        r = name_of(world, runner),
        l = name_of(world, left)
    ));
}

// ------------------------------------------------------------------------------------------------
// breaking up

/// `a` ends it with `b`. Spouses who live together split up: `a` moves out to a vacant lot if
/// there is one.
pub fn break_up(world: &mut World, a: Entity, b: Entity) {
    let was_married = {
        let mut rels = world.res_mut::<Relationships>();
        let r = rels.get_mut(a, b);
        let married = r.married;
        r.partners = false;
        r.engaged = false;
        r.married = false;
        r.wedding_at = None;
        r.exes = true;
        r.romance = 0.0;
        married
    };
    for (x, y) in [(a, b), (b, a)] {
        if let Some(mut f) = world.get_mut::<Family>(x)
            && f.spouse == Some(y)
        {
            f.spouse = None;
        }
    }
    add_moodlet(world, a, "broke_up");
    add_moodlet(world, b, "heartbroken");
    let (na, nb) = (name_of(world, a), name_of(world, b));
    let line = if was_married {
        format!("{na} and {nb} are getting divorced. They're splitting everything down the middle, including the houseplants.")
    } else {
        format!("{na} broke up with {nb}. {nb} is keeping the hoodie.")
    };
    log(world, LogKind::Life, Some(a), line);
    if neighborhood::household_of(world, a).is_some()
        && neighborhood::household_of(world, a) == neighborhood::household_of(world, b)
        && let Err(why) = neighborhood::move_out(world, a)
    {
        log(world, LogKind::Funny, Some(a), format!("{na} would move out, but {why}. Things are about to get very awkward around the fridge."));
    }
}

// ------------------------------------------------------------------------------------------------
// jealousy

/// `a` flirted with or kissed `b`: anyone watching who's with either of them takes it badly
/// (Theatrical peons make a scene of it).
pub fn jealousy(world: &World, a: Entity, b: Entity, what: &str) {
    if world.res::<Relationships>().together(a, b) {
        return;
    }
    for c in present_peons(world) {
        if c == a || c == b || world.get::<Age>(c).is_none_or(|g| !g.stage.is_adult()) {
            continue;
        }
        let Some((cheat, other)) = [(a, b), (b, a)].into_iter().find(|&(x, _)| world.res::<Relationships>().together(c, x)) else { continue };
        {
            let mut rels = world.res_mut::<Relationships>();
            rels.add(c, cheat, -15.0, -10.0);
            rels.add(c, other, -20.0, 0.0);
        }
        add_moodlet(world, c, "jealous");
        let (nc, nx, no) = (name_of(world, c), name_of(world, cheat), name_of(world, other));
        if world.get::<Personality>(c).is_some_and(|p| p.has("theatrical")) {
            if let Some(mut n) = world.get_mut::<Needs>(c) {
                n.add(Need::Drama, 40.0);
            }
            set_bubble(world, c, Some("!!".into()), Some("HOW COULD YOU".into()), 30);
            log(world, LogKind::Funny, Some(c), format!(
                "{nc} saw {nx} {what} with {no} and made a scene worthy of daytime television, complete with a slow turn and a dramatic exit. Twice."
            ));
        } else {
            set_bubble(world, c, Some(":(".into()), None, 20);
            log(world, LogKind::Social, Some(c), format!("{nc} saw {nx} {what} with {no}. {nc} is Not Happy."));
        }
    }
}

// ------------------------------------------------------------------------------------------------
// every evening

/// At 19:00: couples who can't stand each other split up, single neighbors date (and get engaged),
/// and married neighbors have babies.
pub fn daily(world: &mut World) {
    if world.res::<Clock>().minute_of_day() != 19 * 60 {
        return;
    }
    divorces(world);
    set_dates(world);
    courting(world);
    offlot_babies(world);
    world.flush();
}

fn divorces(world: &mut World) {
    let sour: Vec<(Entity, Entity)> = world
        .res::<Relationships>()
        .map
        .iter()
        .filter(|(_, r)| (r.partners || r.engaged || r.married) && r.friendship <= -50.0)
        .map(|(&k, _)| k)
        .collect();
    for (a, b) in sour {
        if !alive(world, a) || !alive(world, b) || !world.res_mut::<SimRng>().chance(0.25) {
            continue;
        }
        let (x, y) = if world.res_mut::<SimRng>().chance(0.5) { (a, b) } else { (b, a) };
        break_up(world, x, y);
    }
}

/// Engaged neighbors with no wedding date (it was called off) set a new one.
fn set_dates(world: &mut World) {
    let townies = townies(world);
    let undated: Vec<(Entity, Entity)> = world
        .res::<Relationships>()
        .map
        .iter()
        .filter(|&(&(a, b), r)| r.engaged && r.wedding_at.is_none() && townies.contains(&a) && townies.contains(&b))
        .map(|(&k, _)| k)
        .collect();
    for (a, b) in undated {
        let days = world.res_mut::<SimRng>().range_f32(1.0, 3.0);
        let at = now(world) + (days * TICKS_PER_DAY as f32) as u64;
        world.res_mut::<Relationships>().get_mut(a, b).wedding_at = Some(at);
    }
}

/// Grown-up neighbors who aren't in the played household (and aren't on the lot right now).
fn townies(world: &World) -> Vec<Entity> {
    let mut v: Vec<Entity> = world
        .query_filtered::<(Entity, &Age), (With<Offlot>, Without<Dead>, Without<Member>)>()
        .iter()
        .filter(|(_, a)| a.stage.is_adult())
        .map(|(e, _)| e)
        .collect();
    v.sort();
    v
}

fn courting(world: &mut World) {
    let townies = townies(world);
    for &a in &townies {
        if committed_to(world, a).is_some() || !world.res_mut::<SimRng>().chance(0.35) {
            continue;
        }
        let stage = world.get::<Age>(a).unwrap().stage as i32;
        let home = neighborhood::household_of(world, a);
        let candidates: Vec<Entity> = townies
            .iter()
            .copied()
            .filter(|&b| {
                b != a
                    && opposite_sex(world, a, b)
                    && !is_family(world, a, b)
                    && neighborhood::household_of(world, b) != home
                    && committed_to(world, b).is_none()
                    && (world.get::<Age>(b).unwrap().stage as i32 - stage).abs() <= 1
                    && world.res::<Relationships>().get(a, b).friendship > -20.0
            })
            .collect();
        if candidates.is_empty() {
            continue;
        }
        // keep seeing the one they like best; otherwise meet someone new
        let best = candidates.iter().copied().max_by(|&x, &y| {
            let rels = world.res::<Relationships>();
            rels.get(a, x).romance.total_cmp(&rels.get(a, y).romance).then(y.cmp(&x))
        });
        let b = match best.filter(|&b| world.res::<Relationships>().get(a, b).romance > 0.0) {
            Some(b) => b,
            None => candidates[world.res_mut::<SimRng>().below(candidates.len())],
        };
        let gain = world.res_mut::<SimRng>().range_f32(6.0, 14.0);
        world.res_mut::<Relationships>().add(a, b, 5.0, gain);
        if world.res::<Relationships>().get(a, b).romance >= 60.0 {
            engage(world, a, b);
            let days = world.res_mut::<SimRng>().range_f32(1.0, 3.0);
            let at = now(world) + (days * TICKS_PER_DAY as f32) as u64;
            world.res_mut::<Relationships>().get_mut(a, b).wedding_at = Some(at);
        }
    }
}

fn offlot_babies(world: &mut World) {
    let townies = townies(world);
    for &a in &townies {
        let Some(b) = world.get::<Family>(a).and_then(|f| f.spouse) else { continue };
        if !world.res::<Relationships>().get(a, b).married || !townies.contains(&b) || a > b {
            continue;
        }
        let mates = neighborhood::housemates(world, a);
        if !mates.contains(&b) || mates.len() >= family::MAX_HOUSEHOLD {
            continue;
        }
        let kids = mates.iter().filter(|m| world.get::<Age>(**m).is_some_and(|g| g.stage < LifeStage::YoungAdult)).count();
        let carrier = [a, b].into_iter().find(|&e| family::can_carry(world, e));
        let Some(carrier) = carrier else { continue };
        if kids >= 3 || world.has::<Pregnant>(a) || world.has::<Pregnant>(b) || !world.res_mut::<SimRng>().chance(0.12) {
            continue;
        }
        let other = if carrier == a { b } else { a };
        family::conceive(world, carrier, other);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GameData;
    use crate::sim::Sim;
    use crate::starter::starter_sim;

    /// The starter family on a full street.
    fn street(seed: u64) -> Sim {
        let mut sim = starter_sim(seed);
        sim.populate_neighborhood();
        sim
    }

    fn adults(sim: &Sim) -> (Entity, Entity) {
        let hh = sim.world.res::<crate::economy::Household>().members.clone();
        let v: Vec<Entity> = hh.into_iter().filter(|e| sim.world.get::<Age>(*e).unwrap().stage.is_adult()).collect();
        (v[0], v[1])
    }

    #[test]
    fn starter_couple_is_opposite_sex() {
        let sim = starter_sim(3);
        let (a, b) = adults(&sim);
        assert!(opposite_sex(&sim.world, a, b));
    }

    #[test]
    fn proposals_need_an_unattached_opposite_sex_pair() {
        let mut sim = street(5);
        let (a, b) = adults(&sim);
        let data = GameData::embedded();
        let propose = data.social("propose").unwrap();
        // married already
        assert!(crate::social::social_allowed(&sim.world, propose, a, b).is_err());
        // a neighbor of the same sex is never an option, however smitten
        let n = crate::neighborhood::neighbors(&sim.world)
            .into_iter()
            .find(|&n| sim.world.get::<Age>(n).unwrap().stage.is_adult() && committed_to(&sim.world, n).is_none())
            .unwrap();
        let sex = *sim.world.get::<Sex>(n).unwrap();
        crate::romance::break_up(&mut sim.world, a, b);
        sim.world.res_mut::<Relationships>().add(a, n, 80.0, 90.0);
        *sim.world.get_mut::<Sex>(a).unwrap() = sex;
        assert_eq!(crate::social::social_allowed(&sim.world, propose, a, n), Err("only for a woman and a man"));
        *sim.world.get_mut::<Sex>(a).unwrap() = sex.opposite();
        assert_eq!(crate::social::social_allowed(&sim.world, propose, a, n), Ok(()));
    }

    #[test]
    fn divorce_moves_one_out() {
        let mut sim = street(9);
        let (a, b) = adults(&sim);
        let before = crate::neighborhood::household_count(&sim.world);
        break_up(&mut sim.world, a, b);
        sim.world.flush();
        let hh = sim.world.res::<crate::economy::Household>().members.clone();
        assert!(!hh.contains(&a) && hh.contains(&b));
        assert_eq!(crate::neighborhood::household_count(&sim.world), before + 1);
        assert!(!sim.world.has::<Member>(a));
        assert_eq!(sim.world.res::<Relationships>().get(a, b).label(), "Ex");
    }

    #[test]
    fn a_neighbor_who_marries_in_joins_the_household() {
        let mut sim = street(11);
        let (a, b) = adults(&sim);
        break_up(&mut sim.world, a, b);
        sim.world.flush();
        // b stayed home; marry them to a neighbor
        let n = crate::neighborhood::neighbors(&sim.world)
            .into_iter()
            .find(|&n| sim.world.get::<Age>(n).unwrap().stage.is_adult() && committed_to(&sim.world, n).is_none())
            .unwrap();
        wed(&mut sim.world, b, n, false);
        sim.world.flush();
        let hh = sim.world.res::<crate::economy::Household>().members.clone();
        assert!(hh.contains(&n) && sim.world.has::<Member>(n) && !sim.world.has::<Offlot>(n));
        assert_eq!(sim.world.res::<Relationships>().get(b, n).label(), "Spouse");
    }

    /// An unattached adult neighbor, made the opposite sex to `e` if need be.
    fn match_for(sim: &mut Sim, e: Entity) -> Entity {
        let sex = *sim.world.get::<Sex>(e).unwrap();
        let n = crate::neighborhood::neighbors(&sim.world)
            .into_iter()
            .find(|&n| sim.world.get::<Age>(n).unwrap().stage.is_adult() && committed_to(&sim.world, n).is_none())
            .expect("someone on the street");
        sim.world.insert_one(n, sex.opposite());
        n
    }

    #[test]
    fn a_planned_wedding_happens_on_the_lot() {
        let mut sim = street(13);
        let (a, b) = adults(&sim);
        break_up(&mut sim.world, a, b);
        sim.world.flush();
        let n = match_for(&mut sim, b);
        for e in [b, n] {
            sim.world.remove::<Job>(e);
        }
        sim.world.res_mut::<Relationships>().add(b, n, 60.0, 70.0);
        engage(&sim.world, b, n);
        plan_wedding(&mut sim.world, b, n);
        sim.world.flush();
        let cakes = |sim: &Sim| {
            let data = sim.data();
            sim.objects().into_iter().filter(|&o| sim.world.get::<crate::objects::ObjectInstance>(o).is_some_and(|i| data.object(i.def).id == WEDDING_CAKE)).count()
        };
        assert_eq!(cakes(&sim), 1, "a cake turns up");
        for _ in 0..(36 * 60) {
            sim.run_minutes(1);
            if sim.world.res::<Relationships>().get(b, n).married {
                break;
            }
        }
        let r = sim.world.res::<Relationships>().get(b, n);
        let tail: Vec<String> = sim.world.res::<crate::log::SimLog>().entries.iter().rev().take(25).map(|e| e.text.clone()).collect();
        assert!(r.married && !r.engaged, "they got married: {r:?}\n{}", tail.join("\n"));
        assert!(sim.world.has::<Member>(n), "the neighbor moved in");
        assert!(sim.world.res::<crate::economy::Household>().members.contains(&n));
    }

    #[test]
    fn flirting_in_front_of_a_spouse_has_consequences() {
        let mut sim = street(17);
        let (a, b) = adults(&sim);
        let n = match_for(&mut sim, a);
        // bring the neighbor over
        sim.world.remove::<Offlot>(n);
        let before = sim.world.res::<Relationships>().get(a, b).friendship;
        jealousy(&sim.world, a, n, "flirting");
        assert!(sim.world.res::<Relationships>().get(a, b).friendship < before);
        assert!(sim.world.get::<Mood>(b).unwrap().moodlets.iter().any(|m| m.id == "jealous"));
    }

    #[test]
    fn the_street_pairs_up_on_its_own() {
        let mut sim = street(21);
        sim.run_days(40);
        let rels = sim.world.res::<Relationships>();
        let new_couples = rels
            .map
            .iter()
            .filter(|&(&(a, b), r)| (r.engaged || r.married) && !sim.world.has::<Member>(a) && !sim.world.has::<Member>(b))
            .count();
        assert!(new_couples > 0, "nobody down the street got engaged or married in 40 days");
        // and every engagement or marriage made since the start is between a woman and a man
        for (&(a, b), r) in rels.map.iter() {
            if r.engaged {
                assert!(opposite_sex(&sim.world, a, b));
            }
        }
    }

    #[test]
    fn a_visiting_neighbor_has_her_baby_at_home() {
        let mut sim = street(23);
        let (a, _) = adults(&sim);
        let n = match_for(&mut sim, a);
        sim.world.insert_one(n, Sex::Female);
        sim.world.remove::<Offlot>(n); // over visiting
        sim.world.insert_one(n, Pregnant { other: a, since: 0, due: 0 });
        let before = sim.world.res::<crate::economy::Household>().members.len();
        crate::family::pregnancies(&mut sim.world);
        sim.world.flush();
        assert!(sim.world.has::<Pregnant>(n), "she waits until she's home");
        assert_eq!(sim.world.res::<crate::economy::Household>().members.len(), before, "no baby left with the hosts");
    }

    #[test]
    fn taking_over_a_household_keeps_its_wedding() {
        let mut sim = street(25);
        let (home, m) = {
            let hood = sim.world.res::<crate::neighborhood::Neighborhood>();
            hood.households
                .iter()
                .find_map(|h| {
                    let m = h.members.iter().copied().find(|&m| sim.world.get::<Age>(m).unwrap().stage.is_adult() && committed_to(&sim.world, m).is_none())?;
                    Some((h.home?, m))
                })
                .expect("an unattached neighbor adult")
        };
        let n = {
            let theirs = &sim.world.res::<crate::neighborhood::Neighborhood>().households.iter().find(|h| h.members.contains(&m)).unwrap().members.clone();
            let n = crate::neighborhood::neighbors(&sim.world)
                .into_iter()
                .find(|&n| !theirs.contains(&n) && sim.world.get::<Age>(n).unwrap().stage.is_adult() && committed_to(&sim.world, n).is_none())
                .expect("someone else on the street");
            let sex = *sim.world.get::<Sex>(m).unwrap();
            sim.world.insert_one(n, sex.opposite());
            n
        };
        for e in [m, n] {
            sim.world.remove::<Job>(e);
        }
        sim.world.res_mut::<Relationships>().add(m, n, 60.0, 70.0);
        engage(&sim.world, m, n);
        // set off-stage for the small hours (3:00 tomorrow)
        let t = now(&sim.world);
        let small_hours = t + TICKS_PER_DAY - Clock { tick: t }.minute_of_day() + 3 * 60;
        sim.world.res_mut::<Relationships>().get_mut(m, n).wedding_at = Some(small_hours);
        sim.play_household(home).unwrap();
        let at = sim.world.res::<Relationships>().get(m, n).wedding_at.expect("still dated");
        assert!((11 * 60..=19 * 60).contains(&Clock { tick: at }.minute_of_day()), "moved to the afternoon");
        assert_eq!(sim.world.get::<Offlot>(n).map(|o| o.visit_at), Some(at - 40), "the fiancé(e) is expected");
        for _ in 0..(48 * 60) {
            sim.run_minutes(1);
            if sim.world.res::<Relationships>().get(m, n).married {
                break;
            }
        }
        let tail: Vec<String> = sim.world.res::<crate::log::SimLog>().entries.iter().rev().take(25).map(|e| e.text.clone()).collect();
        assert!(sim.world.res::<Relationships>().get(m, n).married, "they got married\n{}", tail.join("\n"));
    }

    #[test]
    fn weddings_are_in_the_afternoon() {
        for h in 0..24u64 {
            // ticks count from 07:00
            let tick = 10 * TICKS_PER_DAY + h * 60;
            let at = wedding_time(tick);
            let m = Clock { tick: at }.minute_of_day();
            assert!((11 * 60..=19 * 60).contains(&m), "planned at tick {tick}: {}", Clock { tick: at }.label());
            assert!(at >= tick + 3 * 60 && at < tick + 2 * TICKS_PER_DAY);
        }
    }
}
