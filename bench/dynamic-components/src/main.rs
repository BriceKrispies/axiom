//! Realistic micro-benchmark: typed access to *app-blind* (type-erased)
//! dynamic components, three ways.
//!   1. `downcast`  — safe `std::any::Any::downcast_ref` -> `&T`. Fast, but its
//!                    unreachable `None` arm fails the engine's 100% gate.
//!   2. `unsafe`    — a `TypeId`-checked `unsafe` pointer cast -> `&T`. Same
//!                    borrowed access with no checked arm (coverable, needs
//!                    `unsafe`, which the engine forbids).
//!   3. `bytes`     — safe + coverable: components stored as `Reflect`-serialized
//!                    bytes; each read deserializes to an owned `T`.
//! Workloads: a hot "read a field of every component and sum it" loop (the
//! dominant ECS access pattern), and the one-time build/insert cost. The point
//! is the ratio, run in `--release`.

use std::any::{Any, TypeId};
use std::collections::{BTreeMap, HashMap};
use std::hint::black_box;
use std::time::{Duration, Instant};

use axiom_kernel::{BinaryReader, BinaryWriter, EntityId, Reflect};
use axiom_math::{Quat, Transform, Vec3};

const N: usize = 100_000; // entities (components) per column
const FRAMES: usize = 100; // times the read-sum loop runs
const TRIALS: usize = 7; // measurement repeats; we report the fastest

/// Build a deterministic, varied set of transforms so nothing folds to a const.
fn make_transforms() -> BTreeMap<EntityId, Transform> {
    let mut map = BTreeMap::new();
    for i in 0..N as u64 {
        let f = i as f32;
        let t = Transform {
            translation: Vec3::new(f * 0.5, f - 1.0, f * 0.25),
            rotation: Quat::new(0.0, 0.0, (f * 0.001).sin(), (f * 0.001).cos()),
            scale: Vec3::new(1.0, 1.0, 1.0),
        };
        map.insert(EntityId::from_raw(i + 1), t);
    }
    map
}

fn min_time(mut f: impl FnMut() -> f32) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..TRIALS {
        let start = Instant::now();
        let acc = f();
        let elapsed = start.elapsed();
        black_box(acc);
        if elapsed < best {
            best = elapsed;
        }
    }
    best
}

struct DowncastStore {
    columns: HashMap<TypeId, Box<dyn Any>>,
}
impl DowncastStore {
    fn insert_column<T: 'static>(&mut self, col: BTreeMap<EntityId, T>) {
        self.columns.insert(TypeId::of::<T>(), Box::new(col));
    }
    fn column<T: 'static>(&self) -> &BTreeMap<EntityId, T> {
        self.columns
            .get(&TypeId::of::<T>())
            .unwrap()
            .downcast_ref::<BTreeMap<EntityId, T>>()
            .unwrap() // <- the arm the 100% gate can't cover
    }
}

struct UnsafeStore {
    columns: HashMap<TypeId, Box<dyn Any>>,
}
impl UnsafeStore {
    fn insert_column<T: 'static>(&mut self, col: BTreeMap<EntityId, T>) {
        self.columns.insert(TypeId::of::<T>(), Box::new(col));
    }
    fn column<T: 'static>(&self) -> &BTreeMap<EntityId, T> {
        let boxed = self.columns.get(&TypeId::of::<T>()).unwrap();
        debug_assert!(boxed.is::<BTreeMap<EntityId, T>>());
        // SAFETY: keyed by TypeId::of::<T>(); the stored box is a
        // BTreeMap<EntityId, T>. This is what `downcast_ref` does minus the
        // (here-redundant) TypeId branch — exactly the trick mainstream ECS
        // libs use for borrowed dynamic access.
        unsafe { &*(boxed.as_ref() as *const dyn Any as *const BTreeMap<EntityId, T>) }
    }
}

struct BytesStore {
    columns: HashMap<TypeId, BTreeMap<EntityId, Vec<u8>>>,
}
impl BytesStore {
    fn insert_column<T: Reflect + 'static>(&mut self, col: &BTreeMap<EntityId, T>) {
        let bytes: BTreeMap<EntityId, Vec<u8>> = col
            .iter()
            .map(|(e, v)| {
                let mut w = BinaryWriter::new();
                v.reflect_write(&mut w);
                (*e, w.into_bytes())
            })
            .collect();
        self.columns.insert(TypeId::of::<T>(), bytes);
    }
    fn column_bytes<T: 'static>(&self) -> &BTreeMap<EntityId, Vec<u8>> {
        self.columns.get(&TypeId::of::<T>()).unwrap()
    }
}

fn main() {
    let transforms = make_transforms();

    let mut downcast = DowncastStore { columns: HashMap::new() };
    downcast.insert_column(transforms.clone());
    let mut unsafe_store = UnsafeStore { columns: HashMap::new() };
    unsafe_store.insert_column(transforms.clone());
    let mut bytes = BytesStore { columns: HashMap::new() };

    let t_build_typed = {
        let start = Instant::now();
        let mut s = DowncastStore { columns: HashMap::new() };
        s.insert_column(black_box(transforms.clone()));
        black_box(&s);
        start.elapsed()
    };
    let t_build_bytes = {
        let start = Instant::now();
        bytes.insert_column(black_box(&transforms));
        start.elapsed()
    };

    let bytes_per_component = bytes.column_bytes::<Transform>().values().next().unwrap().len();

    let downcast_read = min_time(|| {
        let mut acc = 0.0f32;
        for _ in 0..FRAMES {
            for t in downcast.column::<Transform>().values() {
                acc += t.translation.x;
            }
        }
        acc
    });
    let unsafe_read = min_time(|| {
        let mut acc = 0.0f32;
        for _ in 0..FRAMES {
            for t in unsafe_store.column::<Transform>().values() {
                acc += t.translation.x;
            }
        }
        acc
    });
    let bytes_read = min_time(|| {
        let mut acc = 0.0f32;
        for _ in 0..FRAMES {
            for b in bytes.column_bytes::<Transform>().values() {
                let t = Transform::reflect_read(&mut BinaryReader::new(b)).unwrap();
                acc += t.translation.x;
            }
        }
        acc
    });

    let accesses = (N * FRAMES) as f64;
    let ns_per = |d: Duration| d.as_secs_f64() * 1e9 / accesses;
    let per_frame_us = |d: Duration| d.as_secs_f64() * 1e6 / FRAMES as f64;

    println!("dynamic-component access benchmark");
    println!("  component   : Transform ({bytes_per_component} bytes serialized)");
    println!("  entities    : {N}");
    println!("  read frames : {FRAMES}  (={} total accesses)", N * FRAMES);
    println!("  trials      : {TRIALS} (reporting fastest)\n");

    println!("read-sum (hot loop): ns per component access, and per-frame cost for {N} components");
    let base = ns_per(downcast_read);
    for (label, d) in [
        ("downcast (safe &T)", downcast_read),
        ("unsafe   (&T)", unsafe_read),
        ("bytes    (owned T)", bytes_read),
    ] {
        println!(
            "  {label:<20} {:>7.3} ns/access   {:>8.1} us/frame   {:>5.1}x",
            ns_per(d),
            per_frame_us(d),
            ns_per(d) / base
        );
    }

    println!("\nbuild/insert {N} components (one-time):");
    println!("  typed (move)         {:>8.1} us", t_build_typed.as_secs_f64() * 1e6);
    println!(
        "  bytes (serialize)    {:>8.1} us   {:>5.1}x",
        t_build_bytes.as_secs_f64() * 1e6,
        t_build_bytes.as_secs_f64() / t_build_typed.as_secs_f64().max(f64::MIN_POSITIVE)
    );
}
