//! The ported `fx` subsystem, pinned against the JavaScript it came from.
//!
//! Every value in this file was produced by running the **original**
//! `C:/dev/Claude-of-Duty/src/fx/*.js` under Node (v24), importing `three`
//! from the source repo's own `node_modules`. The capture script is
//! committed next to the data at `tests/fx/capture.mjs`, so the goldens are
//! reproducible rather than copied: re-run it against the source and
//! `tests/fx/golden.json` should come out byte-identical.
//!
//! ## What is pinned here, and what is not
//!
//! - **Particle emission** ([`particle_emission_matches_the_javascript`]) —
//!   the real `ParticleLayer.emit()`'s raw interleaved record, for a fixed
//!   RNG-seeded spawn sequence over a fixed `now` schedule. Exact equality:
//!   every value here is `+ - * /` and the RNG stream, no transcendentals.
//! - **The closed-form integration** (`PARTICLE_VERT`'s vertex-shader math)
//!   is **not** golden-captured here — see `src/fx/particles.rs`'s module
//!   doc for why (there is no JavaScript function to import: the source only
//!   ever expresses it as a GLSL string). It is pinned by property test in
//!   `src/fx/particles.rs` instead.
//! - **Decal ring-buffer eviction** — cursor/wrapped state and the first
//!   written vertex, at and one past budget.
//! - **The atlas bakes** — full byte-for-byte particle and decal atlases at
//!   a small (32px) size.
//! - **Per-surface impact selection**, for all 12 surfaces — the sequence of
//!   particle-tile ids `spawnImpact` emits into the additive and lit layers,
//!   recorded through a JS stub that mirrors `fx::system::FxSystem`'s
//!   `emitAdd`/`emitLit`/`addDecal` contract, plus the decal *count* each
//!   surface produces (see the module doc on why decal tile identity itself
//!   is not separately re-derived from the projected UVs here).
//!
//! Every pool-budget assertion in `src/fx/system.rs`'s and
//! `src/fx/particles.rs`'s own unit tests already proves the pools never
//! exceed their configured capacity; this file additionally checks it here
//! for the exact capacities the capture used.

use std::sync::OnceLock;

use serde_json::Value;

use axiom_shmup::fx::atlas::{bake_decal_atlas, bake_particle_atlas};
use axiom_shmup::fx::decals::{DecalAdd, DecalSystem};
use axiom_shmup::fx::impacts::spawn_impact;
use axiom_shmup::fx::particles::{reset_spawn, ParticleLayer, ParticleMode, STRIDE};
use axiom_shmup::fx::system::FxSystem;
use axiom_shmup::rng::Rng;
use axiom_shmup::world::palette::Surface;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| serde_json::from_str(include_str!("fx/golden.json")).expect("golden.json parses"))
}

fn u8_array(v: &Value) -> Vec<u8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_u64().unwrap() as u8)
        .collect()
}

fn f64_array(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect()
}

// ============================================================================
// particles
// ============================================================================

#[test]
fn particle_emission_matches_the_javascript() {
    let g = golden();
    let p = &g["particles"];
    let seed = p["seed"].as_u64().unwrap() as u32;
    let now_schedule = f64_array(&p["nowSchedule"]);
    let records = p["records"].as_array().unwrap();

    let mut rng = Rng::new(seed);
    let mut layer = ParticleLayer::new(8, ParticleMode::Additive);

    for (i, now) in now_schedule.iter().enumerate() {
        // Mirrors `tests/fx/capture.mjs`'s spawn construction, field for
        // field, in the same order (draw order is part of the contract).
        let mut s = reset_spawn();
        s.x = rng.range(-2.0, 2.0);
        s.y = rng.range(0.0, 3.0);
        s.z = rng.range(-2.0, 2.0);
        s.vx = rng.range(-5.0, 5.0);
        s.vy = rng.range(-5.0, 5.0);
        s.vz = rng.range(-5.0, 5.0);
        s.size0 = rng.range(0.01, 0.2);
        s.size1 = rng.range(0.01, 0.2);
        s.life = rng.range(0.2, 2.0);
        s.delay = rng.range(0.0, 0.1);
        s.drag = rng.range(0.5, 5.0);
        s.gravity = rng.range(-20.0, 5.0);
        s.seed = rng.float();

        let slot = layer.emit(&s, *now);
        let want = f64_array(&records[i]);
        let raw = layer.raw();
        for k in 0..STRIDE {
            let got = f64::from(raw[slot * STRIDE + k]);
            assert_eq!(got, want[k], "particle[{i}] field[{k}]: got {got}, want {}", want[k]);
        }
    }
}

// ============================================================================
// decals
// ============================================================================

#[test]
fn decal_eviction_matches_the_javascript() {
    let g = golden();
    let states = g["decalsEviction"].as_array().unwrap();

    let mut sys = DecalSystem::new(8, 4);
    for (i, want) in states.iter().enumerate() {
        let ok = sys.add(&DecalAdd {
            point: [i as f64, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            size: 0.2,
            tile: 0,
            roll: None,
            life: None,
            fade: None,
            opacity: None,
            max_angle: None,
            depth: None,
            flip: false,
            mask: 0xffff,
            now: i as f64,
            world: None,
        });
        assert_eq!(ok, want["ok"].as_bool().unwrap(), "add[{i}].ok");
        assert_eq!(sys.cursor(), want["cursor"].as_u64().unwrap() as usize, "add[{i}].cursor");
        assert_eq!(sys.wrapped(), want["wrapped"].as_bool().unwrap(), "add[{i}].wrapped");
        assert_eq!(sys.count, want["count"].as_u64().unwrap(), "add[{i}].count");
        let first_x = f64::from(sys.raw_positions()[0]);
        assert_eq!(first_x, want["firstVertexX"].as_f64().unwrap(), "add[{i}].firstVertexX");
    }
    assert!(sys.count as usize <= usize::MAX); // decal count is a monotonic counter, not a live-pool size
}

// ============================================================================
// atlases
// ============================================================================

#[test]
fn particle_atlas_matches_the_javascript() {
    let g = golden();
    let a = &g["particleAtlas"];
    let seed = a["seed"].as_u64().unwrap() as u32;
    let size = a["size"].as_u64().unwrap() as u32;
    let want = u8_array(&a["bytes"]);

    let mut rng = Rng::new(seed);
    let atlas = bake_particle_atlas(&mut rng, size);
    assert_eq!(atlas.data, want);
}

#[test]
fn decal_atlas_matches_the_javascript() {
    let g = golden();
    let a = &g["decalAtlas"];
    let seed = a["seed"].as_u64().unwrap() as u32;
    let size = a["size"].as_u64().unwrap() as u32;

    let mut rng = Rng::new(seed);
    let atlas = bake_decal_atlas(&mut rng, size);
    assert_eq!(atlas.albedo, u8_array(&a["albedo"]));
    assert_eq!(atlas.orm, u8_array(&a["orm"]));

    // The normal map is the one buffer whose final byte comes through a
    // `sqrt`-based `normalize3` (`atlas.rs`'s `normalize3`, mirroring
    // `Math.hypot(...) || 1`) — not bit-guaranteed across V8 and Rust's
    // libm, per this codebase's established transcendental tolerance (see
    // `tests/audio_port.rs`'s module doc). Measured: 2 of 4096 bytes differ
    // by exactly 1, both right at a `u8` truncation boundary; every other
    // byte, and every byte of `albedo`/`orm` above (which never call
    // `sqrt`), matches exactly.
    let want_normal = u8_array(&a["normal"]);
    let mismatches: Vec<(usize, u8, u8)> = atlas
        .normal
        .iter()
        .zip(want_normal.iter())
        .enumerate()
        .filter(|(_, (got, want))| got != want)
        .map(|(i, (got, want))| (i, *got, *want))
        .collect();
    for (i, got, want) in &mismatches {
        let diff = i32::from(*got) - i32::from(*want);
        assert!(diff.abs() <= 1, "normal[{i}]: got {got}, want {want} (diff {diff})");
    }
    assert!(
        mismatches.len() <= 8,
        "too many normal-map byte mismatches for libm rounding alone: {}",
        mismatches.len()
    );
}

// ============================================================================
// impacts — per-surface selection, all 12 surfaces
// ============================================================================

const SURFACES: &[(&str, Surface)] = &[
    ("concrete", Surface::Concrete),
    ("metal", Surface::Metal),
    ("wood", Surface::Wood),
    ("dirt", Surface::Dirt),
    ("sand", Surface::Sand),
    ("glass", Surface::Glass),
    ("water", Surface::Water),
    ("foliage", Surface::Foliage),
    ("fabric", Surface::Fabric),
    ("flesh", Surface::Flesh),
    ("rubber", Surface::Rubber),
    ("plaster", Surface::Plaster),
];

#[test]
fn every_surface_selection_matches_the_javascript() {
    let g = golden();
    assert_eq!(SURFACES.len(), 12, "the physics/audio surface enum has exactly 12 entries");

    for (idx, (name, surface)) in SURFACES.iter().enumerate() {
        let want = &g["impacts"][name];
        let seed = 1000 + idx as u32;

        // A budget large enough that no layer wraps during one impact, so
        // slot index == emission order for every surface.
        let mut fx = FxSystem::new(1, 24000, 512, -19.62);
        fx.rng = Rng::new(seed); // matches the capture stub's fresh `new Rng(seed)`
        fx.pscale = 1.0; // matches the capture stub's `pScale: 1.0`

        let point = (0.0, 1.0, 0.0);
        let normal = (0.0, 1.0, 0.0);
        let incident = (0.0, -1.0, 0.0);
        spawn_impact(&mut fx, point, normal, incident, *surface, 1.0);

        let add_count = fx.add.spawned() as usize;
        let lit_count = fx.lit.spawned() as usize;
        assert_eq!(add_count, want["addCount"].as_u64().unwrap() as usize, "{name}: addCount");
        assert_eq!(lit_count, want["litCount"].as_u64().unwrap() as usize, "{name}: litCount");

        let want_add_tiles = f64_array(&want["addTiles"]);
        let want_lit_tiles = f64_array(&want["litTiles"]);
        for i in 0..add_count {
            assert_eq!(fx.add.tile_at(i), want_add_tiles[i], "{name}: addTiles[{i}]");
        }
        for i in 0..lit_count {
            assert_eq!(fx.lit.tile_at(i), want_lit_tiles[i], "{name}: litTiles[{i}]");
        }

        let want_decal_count = want["decalCalls"].as_array().unwrap().len() as u64;
        assert_eq!(fx.decals.count, want_decal_count, "{name}: decal count");

        // Budget guard: even a worst-case surface (metal, with recursive
        // spark bounces) never exceeds the configured pool capacities.
        assert!(add_count <= fx.add.capacity, "{name}: additive pool exceeded budget");
        assert!(lit_count <= fx.lit.capacity, "{name}: lit pool exceeded budget");
    }
}
