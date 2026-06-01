//! Produce an *artifact of evidence* that the kernel `Reflect` trait — the
//! reflection + serialization substrate — actually works across the real
//! engine types it's implemented for.
//!
//! It exercises the genuine claims and writes a plain log to
//! `target/reflection-evidence.log` (also echoed to stdout). The process exits
//! non-zero if any check fails, so the artifact can be trusted / wired into CI.
//!
//! ```sh
//! cargo run -p axiom-demo-rotating-cube --example reflection_evidence
//! ```
//!
//! Claims proven:
//!   1. Round-trip  — every reflected value serializes then deserializes back
//!                    to an equal value (scalars, math types, an ECS column).
//!   2. Schema      — a type's TypeSchema names its real fields and types.
//!   3. Determinism — the same value always serializes to identical bytes.
//!   4. Safety      — every truncated prefix of valid bytes is rejected with an
//!                    error (never a panic or a silently-wrong value).

use std::fmt::Write as _;
use std::fs;

use axiom_demo_rotating_cube::DemoRotatingCubeApi;
use axiom_ecs::{ColumnSet, ComponentColumn, DynamicComponents, ErasedColumn, World};
use axiom_kernel::{BinaryReader, BinaryWriter, EntityId, Reflect};
use axiom_math::{Mat4, Quat, Transform, Vec2, Vec3, Vec4};

const OUT_PATH: &str = "target/reflection-evidence.log";

/// Serialize then deserialize a value; return `(equal, byte_len)`.
fn round_trip<T: Reflect + PartialEq>(value: &T) -> (bool, usize) {
    let mut w = BinaryWriter::new();
    value.reflect_write(&mut w);
    let bytes = w.into_bytes();
    let n = bytes.len();
    let equal = match T::reflect_read(&mut BinaryReader::new(&bytes)) {
        Ok(decoded) => &decoded == value,
        Err(_) => false,
    };
    (equal, n)
}

/// The bytes a value reflects to.
fn bytes_of<T: Reflect>(value: &T) -> Vec<u8> {
    let mut w = BinaryWriter::new();
    value.reflect_write(&mut w);
    w.into_bytes()
}

/// Whether every truncated prefix of `bytes` fails to decode as `T` (no panic,
/// no silently-wrong value).
fn every_prefix_rejected<T: Reflect>(bytes: &[u8]) -> bool {
    (0..bytes.len()).all(|len| T::reflect_read(&mut BinaryReader::new(&bytes[..len])).is_err())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn main() {
    let mut log = String::new();
    let mut checks = 0u32;
    let mut passed = 0u32;

    macro_rules! info {
        ($($arg:tt)*) => {{ let _ = writeln!(log, "INFO {}", format_args!($($arg)*)); }};
    }
    macro_rules! check {
        ($label:expr, $pass:expr) => {{
            checks += 1;
            let pass = $pass;
            if pass {
                passed += 1;
            }
            let _ = writeln!(log, "{} {}", if pass { "PASS" } else { "FAIL" }, $label);
        }};
    }

    // --- 1. Round-trip: scalars. ---
    let (u32_ok, _) = round_trip(&0xABCD_1234u32);
    let (u64_ok, _) = round_trip(&0x0102_0304_0506_0708u64);
    let (f32_ok, _) = round_trip(&(-2.5f32));
    let (bool_ok, _) = round_trip(&true);
    let (eid_ok, _) = round_trip(&EntityId::from_raw(42));
    info!("scalars u32={u32_ok} u64={u64_ok} f32={f32_ok} bool={bool_ok} EntityId={eid_ok}");
    check!("scalars + EntityId round-trip", u32_ok && u64_ok && f32_ok && bool_ok && eid_ok);

    // --- 1b. Round-trip: math value types (the real component building blocks). ---
    let transform = Transform::from_translation(Vec3::new(1.0, -2.0, 3.5));
    let (v2, _) = round_trip(&Vec2::new(1.0, 2.0));
    let (v3, _) = round_trip(&Vec3::new(1.0, 2.0, 3.0));
    let (v4, _) = round_trip(&Vec4::new(1.0, 2.0, 3.0, 4.0));
    let (q, _) = round_trip(&Quat::IDENTITY);
    let (m, _) = round_trip(&Mat4::IDENTITY);
    let (t, _) = round_trip(&transform);
    let transform_bytes = bytes_of(&transform);
    info!(
        "math Vec2={v2} Vec3={v3} Vec4={v4} Quat={q} Mat4={m} Transform={t} (Transform={} bytes)",
        transform_bytes.len()
    );
    check!("math value types round-trip", v2 && v3 && v4 && q && m && t);

    // --- 2. Schema names real fields and types. ---
    let schema = <Transform as Reflect>::SCHEMA;
    let fields: Vec<String> = schema
        .fields()
        .iter()
        .map(|f| format!("{}: {}", f.name(), f.type_name()))
        .collect();
    info!("schema {} {{ {} }}", schema.name(), fields.join(", "));
    let schema_ok = schema.name() == "Transform"
        && schema.fields().len() == 3
        && schema.fields()[0].name() == "translation"
        && schema.fields()[0].type_name() == "Vec3"
        && schema.fields()[1].name() == "rotation"
        && schema.fields()[2].name() == "scale";
    check!("Transform schema names its real fields/types", schema_ok);

    // The live cube world's component types, self-described via the facade.
    for s in DemoRotatingCubeApi::new().component_schemas() {
        let fs: Vec<String> = s
            .fields()
            .iter()
            .map(|f| format!("{}: {}", f.name(), f.type_name()))
            .collect();
        info!("world-component schema {} {{ {} }}", s.name(), fs.join(", "));
    }
    check!(
        "the world exposes >=4 component schemas",
        DemoRotatingCubeApi::new().component_schemas().len() >= 4
    );

    // --- 3. Determinism: identical value -> identical bytes. ---
    let a = bytes_of(&transform);
    let b = bytes_of(&transform);
    let deterministic = a == b;
    info!("determinism Transform bytes identical={deterministic} hex={}", hex(&a));
    check!("serialization is deterministic", deterministic);

    // --- 4. Safety: every truncated prefix is rejected. ---
    let transform_safe = every_prefix_rejected::<Transform>(&transform_bytes);
    info!(
        "safety Transform truncations rejected={transform_safe} (over {} prefixes)",
        transform_bytes.len()
    );
    check!("truncated Transform bytes are rejected, not mis-decoded", transform_safe);

    // --- 5. ECS column: the world's per-type storage round-trips. ---
    let mut column: ComponentColumn<Transform> = ComponentColumn::new();
    column.insert(EntityId::from_raw(1), Transform::IDENTITY);
    column.insert(EntityId::from_raw(7), transform);
    let mut col_writer = BinaryWriter::new();
    column.reflect_write(&mut col_writer);
    let col_bytes = col_writer.into_bytes();
    let col_ok = match ComponentColumn::<Transform>::reflect_read(&mut BinaryReader::new(&col_bytes)) {
        Ok(decoded) => {
            decoded.len() == 2
                && decoded.get(EntityId::from_raw(1)) == column.get(EntityId::from_raw(1))
                && decoded.get(EntityId::from_raw(7)) == column.get(EntityId::from_raw(7))
        }
        Err(_) => false,
    };
    let col_safe = (0..col_bytes.len()).all(|len| {
        ComponentColumn::<Transform>::reflect_read(&mut BinaryReader::new(&col_bytes[..len])).is_err()
    });
    info!(
        "ecs ComponentColumn<Transform> entries=2 bytes={} roundtrip={col_ok} truncations_rejected={col_safe}",
        col_bytes.len()
    );
    check!("an ECS component column round-trips", col_ok);
    check!("truncated column bytes are rejected", col_safe);

    // --- 6. Whole-world: a World serializes, reloads, and self-describes via
    //        erased columns (no per-type code at the call site). ---
    #[derive(Default)]
    struct DemoWorld {
        transforms: ComponentColumn<Transform>,
        tags: ComponentColumn<u32>,
    }
    impl ColumnSet for DemoWorld {
        fn columns(&self) -> Vec<(&'static str, &dyn ErasedColumn)> {
            vec![("transforms", &self.transforms), ("tags", &self.tags)]
        }
        fn columns_mut(&mut self) -> Vec<(&'static str, &mut dyn ErasedColumn)> {
            vec![("transforms", &mut self.transforms), ("tags", &mut self.tags)]
        }
    }

    let mut world: World<DemoWorld> = World::new();
    let a = world.spawn();
    let b = world.spawn();
    world.storage_mut().transforms.insert(a, Transform::IDENTITY);
    world
        .storage_mut()
        .transforms
        .insert(b, Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)));
    world.storage_mut().tags.insert(a, 7);

    for (name, schema, count) in world.describe() {
        let f: Vec<String> = schema
            .fields()
            .iter()
            .map(|f| format!("{}: {}", f.name(), f.type_name()))
            .collect();
        info!("world-column {name}: {} {{ {} }} entries={count}", schema.name(), f.join(", "));
    }

    let mut ww = BinaryWriter::new();
    world.serialize(&mut ww);
    let world_bytes = ww.into_bytes();
    let mut loaded: World<DemoWorld> = World::new();
    let load_ok = loaded.deserialize(&mut BinaryReader::new(&world_bytes)).is_ok();
    let world_roundtrips = load_ok
        && loaded.storage().transforms.len() == 2
        && loaded.storage().transforms.get(b) == world.storage().transforms.get(b)
        && loaded.storage().tags.get(a) == Some(&7);
    info!(
        "whole-world serialize bytes={} reload_ok={load_ok} roundtrip={world_roundtrips}",
        world_bytes.len()
    );
    check!("a whole world serializes, reloads, and matches", world_roundtrips);

    // --- 7. App-blind dynamic components: a store that was never told about
    //        these types holds and returns them by type (safe, owned). ---
    let mut dynamic = DynamicComponents::new();
    let de = EntityId::from_raw(1);
    dynamic.insert(de, Transform::from_translation(Vec3::new(9.0, 0.0, 0.0)));
    dynamic.insert(de, 1234u32);
    for (name, schema, count) in dynamic.describe() {
        info!("dynamic-component {name} (schema {}) entries={count}", schema.name());
    }
    let transform_x = dynamic.get::<Transform>(de).unwrap().map(|t| t.translation.x);
    let tag = dynamic.get::<u32>(de).unwrap();
    let dynamic_ok = transform_x == Some(9.0) && tag == Some(1234);
    info!("dynamic typed get: Transform.x={transform_x:?} tag={tag:?} ok={dynamic_ok}");
    check!("app-blind dynamic components round-trip by type (safe, owned)", dynamic_ok);

    let all_pass = passed == checks;
    let _ = writeln!(
        log,
        "RESULT {} checks={passed}/{checks}",
        if all_pass { "PASS" } else { "FAIL" }
    );

    fs::write(OUT_PATH, &log).expect("write evidence artifact");
    print!("{log}");
    eprintln!("wrote {OUT_PATH}");
    if !all_pass {
        std::process::exit(1);
    }
}
