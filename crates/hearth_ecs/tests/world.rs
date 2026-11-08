use hearth_ecs::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Pos(f32, f32);
#[derive(Debug, Clone, Copy, PartialEq)]
struct Vel(f32, f32);
#[derive(Debug, Clone, PartialEq)]
struct Name(String);
#[derive(Debug, Clone, Copy, PartialEq)]
struct Marker;

struct DropCounter(Arc<AtomicUsize>);
impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn spawn_get_despawn() {
    let mut w = World::new();
    let a = w.spawn((Pos(1.0, 2.0), Vel(0.5, 0.0)));
    let b = w.spawn((Pos(3.0, 4.0), Name("bob".into())));
    assert_eq!(*w.get::<Pos>(a).unwrap(), Pos(1.0, 2.0));
    assert_eq!(w.get::<Name>(b).unwrap().0, "bob");
    assert!(w.get::<Vel>(b).is_none());
    assert!(w.despawn(a));
    assert!(!w.contains(a));
    assert!(w.get::<Pos>(a).is_none());
    assert!(!w.despawn(a));
    // index reused with new generation; old handle stays dead
    let c = w.spawn((Pos(9.0, 9.0),));
    assert_eq!(c.index(), a.index());
    assert_ne!(c.generation(), a.generation());
    assert!(w.get::<Pos>(a).is_none());
    assert_eq!(*w.get::<Pos>(b).unwrap(), Pos(3.0, 4.0));
}

#[test]
fn query_iterates_matching() {
    let mut w = World::new();
    for i in 0..100 {
        let e = w.spawn((Pos(i as f32, 0.0),));
        if i % 2 == 0 {
            w.insert_one(e, Vel(1.0, 1.0));
        }
        if i % 3 == 0 {
            w.insert_one(e, Marker);
        }
    }
    for (p, v) in w.query::<(&mut Pos, &Vel)>().iter() {
        p.0 += v.0;
        p.1 += v.1;
    }
    let moved = w.query::<&Pos>().iter().filter(|p| p.1 == 1.0).count();
    assert_eq!(moved, 50);
    assert_eq!(w.query_filtered::<&Pos, With<Marker>>().count(), 34);
    assert_eq!(w.query_filtered::<&Pos, (With<Vel>, Without<Marker>)>().count(), 33);
    let with_opt = w.query::<(Entity, Option<&Vel>)>().iter().filter(|(_, v)| v.is_some()).count();
    assert_eq!(with_opt, 50);
}

#[test]
fn insert_replace_and_remove() {
    let mut w = World::new();
    let e = w.spawn((Pos(0.0, 0.0), Name("x".into())));
    let f = w.spawn((Pos(5.0, 5.0), Name("y".into())));
    w.insert_one(e, Vel(1.0, 2.0));
    w.insert_one(e, Pos(7.0, 7.0)); // replace in place
    assert_eq!(*w.get::<Pos>(e).unwrap(), Pos(7.0, 7.0));
    assert_eq!(w.remove::<Name>(e), Some(Name("x".into())));
    assert!(!w.has::<Name>(e));
    assert_eq!(*w.get::<Vel>(e).unwrap(), Vel(1.0, 2.0));
    // f was swapped within the original archetype and must still be intact
    assert_eq!(w.get::<Name>(f).unwrap().0, "y");
    assert_eq!(*w.get::<Pos>(f).unwrap(), Pos(5.0, 5.0));
    assert_eq!(w.remove::<Name>(e), None);
}

#[test]
fn drops_are_balanced() {
    let count = Arc::new(AtomicUsize::new(0));
    {
        let mut w = World::new();
        let a = w.spawn((DropCounter(count.clone()), Pos(0.0, 0.0)));
        let _b = w.spawn((DropCounter(count.clone()),));
        w.insert_one(a, Vel(0.0, 0.0)); // move between archetypes: no drop
        assert_eq!(count.load(Ordering::SeqCst), 0);
        w.insert_one(a, DropCounter(count.clone())); // replace: old dropped
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let removed = w.remove::<DropCounter>(a).unwrap(); // moved out: not dropped yet
        assert_eq!(count.load(Ordering::SeqCst), 1);
        drop(removed);
        assert_eq!(count.load(Ordering::SeqCst), 2);
        w.despawn(a);
    }
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[test]
#[should_panic(expected = "conflicts")]
fn conflicting_queries_panic() {
    let mut w = World::new();
    w.spawn((Pos(0.0, 0.0),));
    let _q1 = w.query::<&mut Pos>();
    let _q2 = w.query::<&Pos>();
}

#[test]
fn nonconflicting_queries_coexist() {
    let mut w = World::new();
    let e = w.spawn((Pos(0.0, 0.0), Vel(1.0, 0.0)));
    let mut q1 = w.query::<&mut Pos>();
    let mut q2 = w.query::<&Vel>();
    let v = *q2.get(e).unwrap();
    q1.get(e).unwrap().0 += v.0;
    drop((q1, q2));
    assert_eq!(w.get::<Pos>(e).unwrap().0, 1.0);
    // borrows released: a mutable query works again
    assert_eq!(w.query::<&mut Pos>().count(), 1);
}

#[test]
fn resources_and_scope() {
    let mut w = World::new();
    w.insert_resource(10u32);
    *w.res_mut::<u32>() += 5;
    assert_eq!(*w.res::<u32>(), 15);
    let e = w.spawn((Pos(0.0, 0.0),));
    w.resource_scope::<u32, _>(|w, r| {
        w.despawn(e);
        *r += 1;
    });
    assert_eq!(*w.res::<u32>(), 16);
    assert_eq!(w.remove_resource::<u32>(), Some(16));
    assert!(w.get_resource::<u32>().is_none());
}

#[test]
fn deferred_commands() {
    let mut w = World::new();
    for i in 0..10 {
        w.spawn((Pos(i as f32, 0.0),));
    }
    for (e, p) in w.query::<(Entity, &Pos)>().iter() {
        if p.0 >= 5.0 {
            w.defer_despawn(e);
        } else {
            w.defer_insert(e, (Marker,));
        }
    }
    w.flush();
    assert_eq!(w.entity_count(), 5);
    assert_eq!(w.query_filtered::<&Pos, With<Marker>>().count(), 5);
}

#[test]
fn events_live_two_updates() {
    let mut w = World::new();
    w.add_event::<u32>();
    let mut r = w.res::<Events<u32>>().reader();
    w.send_event(1u32);
    w.update_events();
    w.send_event(2u32);
    let seen: Vec<u32> = r.read(&w.res::<Events<u32>>()).copied().collect();
    assert_eq!(seen, vec![1, 2]);
    w.update_events();
    w.update_events();
    assert!(w.res::<Events<u32>>().is_empty());
}

#[test]
fn schedule_runs_in_order_and_flushes() {
    fn spawner(w: &mut World) {
        w.defer(|w| {
            w.spawn((Marker,));
        });
    }
    fn counter(w: &mut World) {
        let n = w.query::<&Marker>().count() as u32;
        *w.res_mut::<u32>() = n;
    }
    let mut w = World::new();
    w.insert_resource(0u32);
    let mut s = Schedule::new();
    s.add_stage("a").add_stage("b");
    s.add_system("b", counter).add_system("a", spawner);
    s.run(&mut w);
    s.run(&mut w);
    assert_eq!(*w.res::<u32>(), 2);
    let t = s.timings();
    assert_eq!(t[0].system, "spawner");
}

#[test]
fn inspect_uses_debug() {
    let mut w = World::new();
    w.register_debug::<Pos>();
    let e = w.spawn((Pos(1.0, 2.0), Marker));
    let info = w.inspect(e);
    assert_eq!(info.len(), 2);
    let pos = info.iter().find(|(n, _)| n.ends_with("Pos")).unwrap();
    assert!(pos.1.as_ref().unwrap().contains("1.0"));
}

#[test]
fn stress_random_ops() {
    // simple LCG for determinism
    let mut s: u64 = 12345;
    let mut rnd = || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (s >> 33) as u32
    };
    let mut w = World::new();
    let mut alive: Vec<(Entity, f32)> = Vec::new();
    for step in 0..5000 {
        match rnd() % 5 {
            0 | 1 => {
                let v = step as f32;
                alive.push((w.spawn((Pos(v, v),)), v));
            }
            2 if !alive.is_empty() => {
                let i = rnd() as usize % alive.len();
                w.insert_one(alive[i].0, Vel(1.0, 1.0));
            }
            3 if !alive.is_empty() => {
                let i = rnd() as usize % alive.len();
                w.remove::<Vel>(alive[i].0);
            }
            4 if !alive.is_empty() => {
                let i = rnd() as usize % alive.len();
                let (e, _) = alive.swap_remove(i);
                assert!(w.despawn(e));
            }
            _ => {}
        }
    }
    assert_eq!(w.entity_count(), alive.len());
    for (e, v) in alive {
        assert_eq!(*w.get::<Pos>(e).unwrap(), Pos(v, v));
    }
}

#[test]
fn spawn_at_restores_exact_handles() {
    let mut src = World::new();
    let a = src.spawn((1u32,));
    let b = src.spawn((2u32,));
    src.despawn(a);
    let a2 = src.spawn((3u32,)); // reuses index 0 with a new generation
    assert_ne!(a, a2);

    let mut dst = World::new();
    assert!(dst.spawn_empty_at(b));
    assert!(dst.spawn_empty_at(a2));
    assert!(!dst.spawn_empty_at(b), "index already live");
    dst.insert_one(b, 2u32);
    dst.insert_one(a2, 3u32);
    assert_eq!(*dst.get::<u32>(b).unwrap(), 2);
    assert_eq!(*dst.get::<u32>(a2).unwrap(), 3);
    assert!(!dst.contains(a), "old generation stays dead");
    // fresh spawns never collide with restored handles
    let c = dst.spawn((4u32,));
    assert!(c != a2 && c != b);
    assert_eq!(dst.entity_count(), 3);
    dst.despawn(a2);
    assert!(!dst.contains(a2));
}
