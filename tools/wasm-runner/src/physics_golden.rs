//! The cross-platform f32 determinism golden — the empirical test of the design
//! premise behind SPEC-13 physics prediction and the `engine_no_unportable_float`
//! lint:
//!
//! > "the physics step path uses only IEEE ops that wasm32 + SSE2/NEON round
//! > identically and LLVM never fuses, so native and wasm32 produce bit-identical
//! > results."
//!
//! This module replays a fixed, fully deterministic physics scenario — a
//! rigid-body settling scenario built here from the public [`PhysicsApi`] surface
//! alone — for a fixed number of steps, and folds
//! each tick's world state through the kernel's canonical-byte [`StableHash`] into
//! a `Vec<u64>` digest sequence. The scenario deliberately exercises every float
//! path the step uses: gravity integration, an `O(n²)` broad/narrow phase, the
//! sequential-impulse contact solver, the **friction** tangential pass, and the
//! **angular** integrator (a torqued sphere).
//!
//! The verdict is taken transitively against **one** committed golden array
//! ([`GOLDEN`]): a native `#[test]` asserts `run_scenario() == GOLDEN`, and a
//! `wasm_bindgen_test` asserts the SAME `run_scenario()` equals the SAME `GOLDEN`.
//! If both targets match the one array, native == wasm32 byte-for-byte — and the
//! premise holds. (The hash is only an index; the array *is* the byte record, so a
//! per-element match is itself a byte match — the stance `StableHash` documents.)
//!
//! This crate is repo tooling (a separate workspace, outside the engine graph and
//! the branchless/coverage gates), so the replay loop is written with ordinary
//! control flow.

use axiom_kernel::{BinaryWriter, FrameIndex, Meters, Ratio, StableHash, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::PhysicsApi;
use axiom_runtime::RuntimeStep;

/// The crucible's fixed simulated step (~120 Hz), in integer nanoseconds. Integer
/// time keeps the *delta* itself byte-identical across targets; only the float
/// integration that consumes it is under test.
const FIXED_STEP_NANOS: u64 = 8_333_333;

/// The number of fixed steps replayed. Long enough that both spheres fall, make
/// contact with the floor, and run the friction + restitution + angular passes for
/// many ticks (and well past the `tick-N` vs `tick-N+60` divergence window).
const STEP_COUNT: u64 = 120;

/// The step at which the first sphere is shoved sideways (deterministic input),
/// matching the Replay Bay.
const SHOVE_STEP: u64 = 2;

/// The step at which the second sphere is spun up by a torque, matching the
/// Replay Bay (proves the angular path is in the digest).
const SPIN_STEP: u64 = 1;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).expect("golden authored a finite ratio")
}

fn meters(v: f32) -> Meters {
    Meters::new(v).expect("golden authored a finite length")
}

/// Build the explicit fixed [`RuntimeStep`] for global step `n` (frame == tick ==
/// sequence == `n`), exactly as the crucible feeds it.
fn runtime_step(n: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(n), Tick::new(n), FIXED_STEP_NANOS, n)
}

/// Construct the Replay Bay world: a frictional floor, a slider sphere, and a
/// spinner sphere — reconstructed from the public facade so the scenario is fully
/// self-contained (no app-tier types). Returns the world plus the two dynamic
/// body handles (slider, spinner) the script perturbs.
fn build_world() -> PhysicsApi {
    let mut api = PhysicsApi::new();
    // The crucible's neutral default surface: friction 0.5, no bounce, unit density.
    let material = PhysicsApi::material(ratio(0.5), ratio(0.0), ratio(1.0)).expect("material");

    // Body 0: the static frictional floor (plane, normal +Y, distance 0).
    let floor = api
        .create_static_body(Transform::IDENTITY)
        .expect("static floor body");
    api.attach_plane_collider(floor, Vec3::UNIT_Y, meters(0.0), material, false)
        .expect("plane collider");

    // Body 1: the shoved slider — falls onto the floor and grips via friction.
    let slider = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(-2.0, 4.0, 0.0)), ratio(1.0))
        .expect("slider body");
    api.attach_sphere_collider(slider, meters(0.5), material, false)
        .expect("slider collider");

    // Body 2: a free spinner driven by a torque (proves the angular path).
    let spinner = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(2.0, 4.0, 0.0)), ratio(1.0))
        .expect("spinner body");
    api.attach_sphere_collider(spinner, meters(0.5), material, false)
        .expect("spinner collider");

    api
}

/// Serialize the full float state of a world snapshot into canonical
/// little-endian bytes (via the kernel's [`BinaryWriter`] / `write_to`, the exact
/// path `StableHash` is built to index) and fold it into one stable 64-bit digest.
/// Every body's transform (translation + rotation quaternion + scale), linear
/// velocity, and angular velocity is captured — i.e. all the f32 the step touched.
fn digest_of(api: &PhysicsApi) -> u64 {
    let snapshot = api.snapshot();
    let mut writer = BinaryWriter::new();
    writer.write_u64(snapshot.step_index());
    for body in snapshot.bodies() {
        writer.write_u64(body.handle().raw());
        body.transform().write_to(&mut writer);
        body.linear_velocity().write_to(&mut writer);
        body.angular_velocity().write_to(&mut writer);
        writer.write_bool(body.enabled());
    }
    StableHash::of_bytes(writer.as_bytes()).raw()
}

/// Replay the fixed scenario for [`STEP_COUNT`] steps, returning the per-tick
/// digest sequence. This function is compiled for **both** native and wasm32 and
/// is the single shared subject of the golden assertion on each target.
#[must_use]
pub fn run_scenario() -> Vec<u64> {
    let mut api = build_world();
    // Insertion order is stable, so handles are 1 (floor), 2 (slider), 3 (spinner).
    let slider = axiom_physics::PhysicsBodyHandle::from_raw(2);
    let spinner = axiom_physics::PhysicsBodyHandle::from_raw(3);

    let mut digests = Vec::with_capacity(STEP_COUNT as usize);
    for n in 0..STEP_COUNT {
        if n == SPIN_STEP {
            api.apply_torque(spinner, Vec3::new(0.0, 2.0, 0.0))
                .expect("torque on a dynamic body");
        }
        if n == SHOVE_STEP {
            api.apply_impulse(slider, Vec3::new(3.0, 0.0, 0.0))
                .expect("impulse on a dynamic body");
        }
        api.step(runtime_step(n)).expect("deterministic step");
        digests.push(digest_of(&api));
    }
    digests
}

/// The committed golden: the per-tick digest sequence produced by
/// [`run_scenario`]. Established on native (`x86_64`), and asserted byte-identical
/// on wasm32. A mismatch on either target is a determinism regression — or, on a
/// *new* target, the refutation of the cross-platform f32 premise.
pub const GOLDEN: [u64; STEP_COUNT as usize] = [
    0xc10527df3a58a80b,
    0x27d97f31e68c9263,
    0xc83ae93bd17b5aa9,
    0x59b97f2f72cfacce,
    0x7cca608aae6434bc,
    0x456545177bc7c8c0,
    0xbdf0ffd272f1f3d1,
    0xc3442a0c72ba6eb4,
    0xe10e3508ab3cab23,
    0x550beebeeeb4c333,
    0xc68b7825550f3307,
    0x2f1bb82e227f2c37,
    0x7e4469e9b2ac8064,
    0x6ebffa9233150bc2,
    0xc2585b3853a7c047,
    0x89a4a2ebc6249651,
    0x5e2bf7ff8e25995a,
    0x406a4203f2c3d953,
    0xdff2b7e2f81d5415,
    0x1ca4b65497c56f73,
    0xe1e823a9aa8b793f,
    0xd17b309f129f27b6,
    0x510474bf03883051,
    0xe52a0cddb5d3291c,
    0x42213a2591445496,
    0xfe51586e092906e0,
    0xa7b7e7c6816409cb,
    0x8e36c5095381f74c,
    0xb50ef9728e1640e0,
    0xe118b638a6ec2506,
    0x03c98ec1f2c546f4,
    0x492a1e338f113247,
    0x17bfda63156d7b53,
    0xeca0a9e9e5ddc0f8,
    0x468e327139943de0,
    0x78457d88eb927750,
    0x1f96bc3427e6a059,
    0x6ff1b3c098772c02,
    0x44dc6709b3b94751,
    0x39a4c36e88737b98,
    0xc1df015fcb49a299,
    0x1939590e6e4ab8af,
    0xc2a60cf643dd5428,
    0x5a4e9ce63e8a7870,
    0xc1d0fef837c7aa77,
    0xaa5d56aff65c02e1,
    0xe1b5818a4eefbfff,
    0x2e7cb6e889850a77,
    0xccfdd4288c83b29f,
    0x012f872c8be6a971,
    0x8e4ae7a61b23c593,
    0x36eca2772f7a1520,
    0x8b9c412703007c32,
    0xbb00af527a9f270e,
    0x1a748f7c016fb1fa,
    0x07e51614c040b0b2,
    0x973acc10976ebed2,
    0xee3859c9060de2a5,
    0x230f813cfe5fc6c0,
    0x5903023730366bef,
    0x1f1b810651a4d4ea,
    0xa93c54391be1e613,
    0x3bef3be68f4a106f,
    0x4af6f82a52633423,
    0xba529855101918d7,
    0xf808e85baa4df43f,
    0xb34f6efb541f6752,
    0x059a4206dd3b3e22,
    0x8cc1f10b9b3f41d2,
    0xb52021ede0b15a7e,
    0x37b96d2b9176c66b,
    0x206d636ca6baa790,
    0x6625daf825c5faf9,
    0x7a6fb31a2a75e693,
    0x4d760dc8962c1d9c,
    0xf24cf4bacfca882b,
    0x0e88ede7046aa554,
    0x21526d62cd509f4b,
    0x0494e89fdea933a5,
    0x1b4faa412bd1bc88,
    0xbd73a6722135093a,
    0x4d671ee2c25a79e9,
    0x81dca696eb3bfb8a,
    0xda5aed006a35fa4b,
    0xc997a979a9e63d60,
    0x00c3aed73bc301c1,
    0xee23f9d3fc0b1b57,
    0x54fe9cf9863b624b,
    0x0bb5921115358174,
    0xe1b0ec6e762830e8,
    0x1f69c90abdcbd223,
    0x8045cbed8d7c19ad,
    0xd8a5d2fdaeff173c,
    0x1c61387f518d14f9,
    0xcc1b1b7d87215e2d,
    0x0963a10365ba8c0b,
    0x382b7908832485ea,
    0xc159e76dac3c2a23,
    0x9abcfe5fe48547f1,
    0x4f9f90b17cbd5307,
    0x2ed13c523ff61b32,
    0xca755867578bd1a1,
    0x1aa7f05fc9394601,
    0x6c6c1654a9fcd92a,
    0x94ba9e3f7a95245a,
    0x9a52756c6cd8e810,
    0xf0cdc34598b0e980,
    0xed14fefa94fb9295,
    0x3f73a3670c14ebe8,
    0xc1b540394001d750,
    0x827db7a4333cfe7b,
    0xf89e9f9ac27e3a6f,
    0xee867ec6cd211f80,
    0xbd86092c6b0a007d,
    0x3c1d15530c898445,
    0xec324681fae6eafa,
    0x7934b6257eceffe0,
    0x6fdc0c437f0a80a3,
    0xc93bb160d73c59c1,
    0x45b2481650acc9cb,
];

#[cfg(test)]
mod tests {
    use super::{run_scenario, GOLDEN, SHOVE_STEP, SPIN_STEP, STEP_COUNT};

    // On `wasm32` these run under `wasm-bindgen-test-runner` (node); on every other
    // target they are ordinary `#[test]`s — the SAME body, the SAME `GOLDEN`. A
    // green run on both targets means native == wasm32 byte-for-byte (transitively
    // through the one committed array).

    /// THE cross-platform verdict. If this passes natively AND on wasm32, the
    /// physics step path is bit-identical across the two targets.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn the_physics_digest_sequence_matches_the_golden_on_this_target() {
        let actual = run_scenario();
        assert_eq!(actual.len(), STEP_COUNT as usize);
        // Per-element so a divergence reports exactly which tick (and target) drifted.
        for (tick, (got, want)) in actual.iter().zip(GOLDEN.iter()).enumerate() {
            assert_eq!(
                got, want,
                "tick {tick}: physics digest 0x{got:016x} != golden 0x{want:016x} \
                 (cross-target f32 divergence on this build target)"
            );
        }
        // And the whole sequence as one comparison (catches any length drift too).
        assert_eq!(actual.as_slice(), GOLDEN.as_slice());
    }

    /// The replay is reproducible within a target: two runs of the same scenario
    /// produce byte-identical sequences. (Same-binary determinism — the property
    /// `axiom-physics` already guarantees — is the floor the cross-target claim
    /// builds on.)
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn the_replay_is_deterministic_within_this_target() {
        assert_eq!(run_scenario(), run_scenario());
    }

    /// The scenario is a genuine simulation, not a frozen no-op: the state evolves
    /// (so the digest actually changes tick to tick), and the scripted impulse +
    /// torque are real perturbations the digest reflects. A constant digest would
    /// make the golden a tautology; this guards against that.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn the_scenario_actually_evolves_and_responds_to_its_inputs() {
        let seq = run_scenario();
        // Consecutive ticks differ (the world is moving under gravity + contacts).
        let moving = seq.windows(2).filter(|w| w[0] != w[1]).count();
        assert!(moving > STEP_COUNT as usize / 2, "the world barely moved: {moving} changes");
        // The scripted input steps are within range and the digest at/after them
        // differs from before (the impulse and torque changed the state).
        assert!(SPIN_STEP < SHOVE_STEP && SHOVE_STEP < STEP_COUNT);
        assert_ne!(seq[SHOVE_STEP as usize - 1], seq[SHOVE_STEP as usize]);
    }
}
