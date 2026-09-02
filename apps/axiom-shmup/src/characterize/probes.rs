//! The frozen case list.
//!
//! Not a port of anything.
//!
//! Every probe here is written **once, by the orchestrator, before the fan-out**,
//! and is then frozen. A conversion agent does not invent a probe, does not
//! invent an expected value, and does not hand-copy a number — it calls the probe
//! its recipe already has and asserts against the ledger.
//!
//! Because nobody adds a probe, nobody edits this file, and the last
//! shared-file collision in the fan-out is gone.
//!
//! # Adding a case
//!
//! A case must be **deterministic**: a pinned seed, pinned arguments, and no
//! wall-clock or environment input. And it must observe *everything*, not just
//! the channel the conversion is expected to touch — see [`super`] for why.

use crate::characterize::{Capture, Channel, Fingerprint};
use crate::fx::system::FxSystem;
use crate::world::palette::Surface;

/// The `fx` area.
pub mod fx {
    use super::*;

    /// A stable case name per surface, in `Surface::ALL` order.
    ///
    /// Written out rather than derived from `Debug` so that renaming a variant
    /// cannot silently rename a golden row and turn a real regression into a
    /// "no ledger row" skip.
    const SURFACE_CASE: [&str; 12] = [
        "impact_concrete",
        "impact_metal",
        "impact_wood",
        "impact_dirt",
        "impact_sand",
        "impact_glass",
        "impact_water",
        "impact_foliage",
        "impact_fabric",
        "impact_flesh",
        "impact_rubber",
        "impact_plaster",
    ];

    /// The case names, so a caller can enumerate without touching `Surface`.
    pub const CASES: &[&str] = &SURFACE_CASE;

    /// The seed every impact case runs at.
    ///
    /// One seed for all twelve, deliberately: the surfaces then differ only by
    /// which recipe ran, so a digest that moves points at the recipe rather than
    /// at the seed. A per-surface seed would hide a conversion that accidentally
    /// made two surfaces identical.
    const IMPACT_SEED: u32 = 0x5eed;

    /// The tracer case — the conversion that is already done, kept as the
    /// harness's own control. If this row moves, the harness is wrong, not the
    /// app.
    pub fn tracer() -> Capture {
        let mut sys = FxSystem::test_instance(7);
        crate::fx::tracers::spawn_tracer(&mut sys, (1.0, 2.0, 3.0), (31.0, 6.0, 3.0), 260.0, 0.8);
        observe("tracer", &sys).witness(sys.add.raw(), 3)
    }

    /// One impact case. `index` is into [`Surface::ALL`].
    pub fn impact(index: usize) -> Capture {
        let surface = Surface::ALL[index];
        let mut sys = FxSystem::test_instance(IMPACT_SEED);
        crate::fx::impacts::spawn_impact(
            &mut sys,
            (1.5, 2.25, -3.75),
            (0.0, 1.0, 0.0),
            (0.30, -0.90, 0.30),
            surface,
            1.0,
        );
        observe(SURFACE_CASE[index], &sys).witness(sys.add.raw(), 2)
    }

    /// Every `fx` case, in ledger order.
    pub fn all() -> Vec<Capture> {
        let mut out = vec![tracer()];
        out.extend((0..Surface::ALL.len()).map(impact));
        out
    }
}

/// Fingerprint every channel an `FxSystem` is observable through.
///
/// **All of them, always.** A conversion that emits the right number of
/// particles into the wrong pool passes any assertion that only inspects
/// `add`; `emit_add`, `emit_lit`, `emit_mote`, `emit_view_add` and
/// `emit_view_lit` are five different pools and the mistake is invisible from
/// inside one of them.
///
/// The RNG state goes in last and is the most important row. The random stream
/// is shared across every subsystem in the frame, so a burst that takes one
/// extra draw shifts every later effect — silently, and the frame still looks
/// plausible. That is the failure this whole harness exists to catch, and it is
/// only visible as a state word.
fn observe(case: &'static str, sys: &FxSystem) -> Capture {
    let layers = [
        ("add", &sys.add),
        ("lit", &sys.lit),
        ("motes", &sys.motes),
        ("view_add", &sys.view_add),
        ("view_lit", &sys.view_lit),
    ];
    let mut channels: Vec<Channel> = layers
        .iter()
        .map(|(name, layer)| {
            Channel::new(name, layer.spawned(), Fingerprint::new().f32s(live(layer)))
        })
        .collect();

    channels.push(Channel::new(
        "decals",
        sys.decals.count,
        Fingerprint::new()
            .f32s(sys.decals.raw_positions())
            .f32s(sys.decals.raw_normals())
            .f32s(sys.decals.raw_uvs())
            .f32s(sys.decals.raw_decal_meta()),
    ));

    let lights: Vec<f64> = sys
        .lights
        .slots
        .iter()
        .flat_map(|s| {
            [
                s.x,
                s.y,
                s.z,
                s.r,
                s.g,
                s.b,
                s.distance,
                s.intensity,
                s.peak,
                s.age,
                s.duration,
                s.decay,
                s.priority,
            ]
        })
        .collect();
    channels.push(Channel::new(
        "lights",
        sys.lights.slots.len() as u64,
        Fingerprint::new().f64s(&lights),
    ));

    channels.push(Channel::new(
        "rng",
        0,
        Fingerprint::new().u32s(&sys.rng.state()),
    ));

    Capture::new(case, channels)
}

/// The slots of a layer that have actually been written.
///
/// `ParticleLayer::raw()` hands back the whole preallocated ring, and
/// `particles.rs` states the rule in bold: *"Every reader of this layer must
/// bound its slot loop by [`instance_count`], never by `capacity`. A slot past
/// it has never been written, and a zero-filled record is not inert."*
///
/// A fingerprint is a reader like any other. Taking the whole ring digests
/// 64,000 zeros to describe three particles — not *wrong*, because zeros are
/// stable, but the wrong bound, and exactly the mistake that file warns about.
///
/// It is **not** where the suite's time goes, which is worth writing down
/// because it looks like it should be. `FxSystem::new` bakes a 512x512 particle
/// atlas and a 512x512 decal atlas (`system.rs`, `bake_particle_atlas` /
/// `bake_decal_atlas`), so every case pays ~500k procedurally-generated texels
/// at construction, in a debug build. That is ~2.3s per case and all of the
/// suite's ~30s; bounding the fingerprint changed it by nothing measurable. If
/// this ever needs to be fast, the lever is caching the atlas per seed — it is a
/// pure function of one — not trimming what is hashed.
fn live(layer: &crate::fx::particles::ParticleLayer) -> &[f32] {
    &layer.raw()[..layer.instance_count() * crate::fx::particles::STRIDE]
}
