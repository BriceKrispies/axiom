//! **The Burnt Rubber golden regression** — the agent plays the shipping course
//! and five checkpoints of that one race are pinned as committed golden bytes.
//!
//! ```text
//! cargo test -p axiom-burnt-rubber --test agent_golden
//! ```
//!
//! # What this proves, and why the app's other tests do not
//!
//! `tests/agent_race.rs` proves the agent *can* finish and that two runs agree
//! **within one process**. That catches a regression only across an
//! `assert_eq!`; it cannot catch a change that alters the game *across commits*,
//! because both sides of that comparison are rebuilt from the changed code and
//! still agree. `src/capture.rs`'s `every_slice_renders_identically_twice` has
//! the same limit, and its slices are teleported poses rather than a race.
//!
//! This file closes both gaps. Each checkpoint of [`golden`](axiom_burnt_rubber::golden)'s
//! single agent-driven race is serialized into canonical little-endian bytes and
//! pinned as a **committed golden file** under `tests/golden/`, hash-pinned in
//! `apps/burnt-rubber/slice.toml` and enforced by `cargo xtask check-slices`.
//! Two artifacts per checkpoint, compared independently so a diff localizes:
//!
//! * `..._state.bin` — the **simulation**: where the car is, how fast, how much
//!   boost it has, how many cars it threaded, where the camera is.
//! * `..._render.bin` — the **render boundary**: the draws (including their
//!   emissive and specular lanes), the camera matrix, the clear colour, the
//!   lights, and the authored render *look* (ambient, fog, sky, bloom, grade)
//!   that the GPU and Canvas 2D backends both consume.
//! * `..._resources.bin` — the **generated resources**: a content fingerprint of
//!   every uploaded mesh and every uploaded texture.
//!
//! All three are needed, and the third is the one this app most needs.
//! `FrameOutcome` carries a mesh *id*, never its vertices: geometry and texels
//! are uploaded once at bind (`modules/axiom/src/app/resources.rs`, and see the
//! note at `src/render/chunks.rs:1-14` explaining why they must be). So a road
//! chunk built from a stale track, an off-by-one sample range, a seam that stops
//! being bit-identical (`src/render/road_mesh.rs:126`), or a moved constant in
//! `asphalt_albedo` all render a *visibly* different game while leaving the draw
//! list byte-identical. Without the resources artifact this fixture would be
//! sensitive to mesh-id churn and blind to mesh content — the worst possible
//! combination for a baseline whose job is to prove that moving generation
//! earlier in the lifecycle changed nothing.
//!
//! A change that leaves the simulation identical but drops a draw moves only the
//! render bytes; one that alters the driver but frames the same scene moves only
//! the state bytes; one that rebuilds the same scene from wrong geometry moves
//! only the resource bytes.
//!
//! # Regenerating (the only sanctioned update path)
//!
//! Goldens are never hand-edited. A *missing* golden is captured on the next run
//! (written, test passes), so adding a checkpoint bootstraps its baseline; an
//! *existing* golden must then match byte-for-byte forever. To re-capture after
//! an intended change, force a rewrite, review the diff as the evidence the
//! change is what was intended, and **repin the SHA-256 in `slice.toml`**:
//!
//! ```text
//! AXIOM_REGOLD=1 cargo test -p axiom-burnt-rubber --test agent_golden
//! cargo run -p xtask -- check-slices        # reports the new hashes
//! ```
//!
//! An unexplained golden diff with no corresponding intended change is a
//! determinism bug, exactly as a coverage drop is.

use std::path::PathBuf;

use axiom::prelude::{FrameOutcome, TextureSampling};
use axiom_burnt_rubber::golden::{
    self, GoldenCheckpoint, GoldenState, GoldenStop, CHECKPOINTS, GOLDEN_STEP_LIMIT,
};
use axiom_kernel::StableHash;

// --- canonical encoders -----------------------------------------------------
//
// Each appends a fixed sequence of little-endian primitives, so the same input
// always yields the same bytes. Collections are length-prefixed (a u32 count) so
// a structural change (an extra draw, an extra light) shifts the bytes
// detectably rather than sliding silently.
//
// `f32` is written as its exact IEEE-754 bit pattern. That is deterministic
// across runs and platforms for the finite values these records hold, and is the
// same stance the rest of the repository takes (see
// `apps/axiom-rotating-cube/tests/render_determinism.rs`).

fn push_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_f32s(out: &mut Vec<u8>, vs: &[f32]) {
    vs.iter().for_each(|&v| push_f32(out, v));
}

fn encode_state(s: &GoldenState) -> Vec<u8> {
    let mut out = Vec::new();
    push_u32(&mut out, s.steps);
    push_u64(&mut out, s.sim_steps);
    push_u32(&mut out, s.phase);
    push_f32(&mut out, s.elapsed_seconds);
    push_f32(&mut out, s.distance);
    push_f32(&mut out, s.lateral);
    push_f32(&mut out, s.yaw);
    push_f32(&mut out, s.speed);
    push_f32s(&mut out, &s.position);
    push_f32(&mut out, s.progress);
    push_u32(&mut out, s.section);
    push_f32(&mut out, s.boost_charge);
    out.push(u8::from(s.boost_active));
    push_u32(&mut out, s.near_misses);
    push_u32(&mut out, s.impacts);
    push_f32(&mut out, s.top_speed);
    push_f32s(&mut out, &s.camera_eye);
    push_f32s(&mut out, &s.camera_target);
    push_f32(&mut out, s.camera_fov_degrees);
    push_f32(&mut out, s.camera_roll);
    // Presence byte then the value — never a NaN sentinel (see `GoldenState`).
    out.push(u8::from(s.ghost_delta.is_some()));
    push_f32(&mut out, s.ghost_delta.unwrap_or(0.0));
    out
}

/// The bytes of `encode_state` that are *not* pinned by the checkpoint's own
/// definition.
///
/// `steps` (u32) and `sim_steps` (u64) lead the encoding and are distinct across
/// the five checkpoints **by construction** — 0, 700, 2200, 3800, ~5419. An
/// inequality assertion over the whole record therefore cannot fail whatever the
/// game does. Comparing from here on asks the question that actually matters: do
/// the five checkpoints describe five different *races*?
const STATE_PREFIX_BYTES: usize = 4 + 8;

fn encode_frame_outcome(f: &FrameOutcome) -> Vec<u8> {
    let mut out = Vec::new();
    push_u64(&mut out, f.tick());
    push_u32(&mut out, f.command_count() as u32);
    push_f32s(&mut out, &f.clear_color());
    push_f32s(&mut out, &f.camera_view_proj());
    push_f32s(&mut out, &f.light_view_proj());
    // Draws, in submission order (deterministic scene order).
    //
    // `emissive` and `specular` are encoded deliberately. They are the two lanes
    // `FrameOutcome::instance_floats` carries that the colour lane cannot — the
    // shader multiplies colour by the light, so self-illumination lives only in
    // the emissive lane. In this game that is not a detail: the boost pickups'
    // three tiers are told apart almost entirely by their emissive channels, so
    // a golden that omitted them would call a green pickup and a blue one the
    // same frame. Encoding them here makes this artifact a strict superset of
    // what `capture.rs`'s `every_slice_renders_identically_twice` compares.
    push_u32(&mut out, f.draws().len() as u32);
    f.draws().iter().for_each(|d| {
        push_f32s(&mut out, &d.mvp());
        push_f32s(&mut out, &d.world());
        push_f32s(&mut out, &d.color());
        push_f32s(&mut out, &d.emissive());
        push_f32(&mut out, d.specular().get());
        push_u64(&mut out, d.mesh_id());
        push_u64(&mut out, d.material_id());
        out.push(u8::from(d.casts_contact_shadow()));
    });
    // Lights, in scene order.
    push_u32(&mut out, f.lights().len() as u32);
    f.lights().iter().for_each(|l| {
        push_u32(&mut out, l.kind());
        push_f32s(&mut out, &l.vec());
        push_f32s(&mut out, &l.color());
        push_f32(&mut out, l.intensity());
    });

    // The authored render *look*. `RaceScene::install` sets an ambient
    // hemisphere, a depth fog, a sky, a bloom and a colour grade
    // (`src/render/mod.rs:194,229,279,362,373`), and all five ride onto the
    // `FrameOutcome` for both backends to present. They are every pixel's
    // final say: swap `GRADE` from `sunlit()` to `cinematic()`, move the fog
    // extinction, or repaint the sky, and the whole frame changes while not one
    // draw moves. A golden that stopped at the draw list would call those the
    // same frame.
    push_f32s(&mut out, &f.ambient().sky());
    push_f32s(&mut out, &f.ambient().ground());
    // Each optional record as a presence byte then its fields, so "absent" and
    // "present with default values" are different bytes.
    push_optional(&mut out, f.depth_fog(), |out, fog| {
        push_f32(out, fog.near().get());
        push_f32(out, fog.far().get());
        push_f32(out, fog.strength().get());
        push_f32(out, fog.extinction().get());
        push_f32s(out, &fog.color());
    });
    push_optional(&mut out, f.postprocess(), |out, grade| {
        push_f32(out, grade.exposure().get());
        push_f32s(out, &grade.white_balance());
        push_f32(out, grade.contrast().get());
        push_f32(out, grade.saturation().get());
        push_f32(out, grade.black_point().get());
    });
    push_optional(&mut out, f.sky(), |out, sky| {
        push_f32s(out, &sky.zenith());
        push_f32s(out, &sky.horizon());
        push_f32s(out, &sky.body_direction());
        push_f32(out, sky.body_angular_radius().get());
        push_f32s(out, &sky.body_color());
        push_f32(out, sky.halo_falloff().get());
        push_f32(out, sky.halo_strength().get());
        push_f32(out, sky.cloud_coverage().get());
        push_f32(out, sky.cloud_scale().get());
        push_f32(out, sky.haze_height().get());
    });
    push_optional(&mut out, f.bloom(), |out, bloom| {
        push_f32(out, bloom.threshold().get());
        push_f32(out, bloom.knee().get());
        push_f32(out, bloom.intensity().get());
        push_f32(out, bloom.radius().get());
    });
    out
}

/// A presence byte, then `encode` if the value is present.
fn push_optional<T>(out: &mut Vec<u8>, value: Option<T>, encode: impl Fn(&mut Vec<u8>, &T)) {
    out.push(u8::from(value.is_some()));
    value.iter().for_each(|v| encode(out, v));
}

/// Fingerprint the **generated resources** — every uploaded mesh and texture.
///
/// This is the artifact the startup-preparation migration will actually stress,
/// and without it the fixture is sensitive to mesh-id churn and blind to mesh
/// *content*. `FrameOutcome` carries a mesh id, never its vertices: geometry and
/// texels are uploaded once at bind (`modules/axiom/src/app/resources.rs`; see
/// `src/render/chunks.rs:1-14` for why they must be). So a road chunk built from
/// a stale track, an off-by-one sample range, a seam that stops being
/// bit-identical, or a moved constant inside `asphalt_albedo` all produce a
/// visibly different game while leaving the draw list byte-identical.
///
/// Contents rather than a bare count, and a hash rather than the raw floats: the
/// mesh set for this course is tens of megabytes, and a `StableHash` (the
/// kernel's platform-stable FNV-1a) over the exact IEEE-754 bit patterns is a
/// faithful fingerprint at a few hundred bytes.
fn encode_resources(app: &mut axiom_burnt_rubber::BurntRubber) -> Vec<u8> {
    let mut out = Vec::new();
    let meshes = app.running().mesh_set();
    push_u32(&mut out, meshes.len() as u32);
    meshes.iter().for_each(|(id, verts, indices)| {
        push_u64(&mut out, *id);
        push_u32(&mut out, verts.len() as u32);
        push_u32(&mut out, indices.len() as u32);
        push_u64(&mut out, hash_f32s(verts));
        push_u64(&mut out, hash_u32s(indices));
    });
    let textures = app.running().material_textures();
    push_u32(&mut out, textures.len() as u32);
    textures.iter().for_each(|t| {
        push_u64(&mut out, t.material_id());
        push_u32(&mut out, t.width());
        push_u32(&mut out, t.height());
        push_u32(&mut out, sampling_index(t.sampling()));
        push_u64(&mut out, StableHash::of_bytes(t.pixels()).raw());
    });
    out
}

fn hash_f32s(values: &[f32]) -> u64 {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    StableHash::of_bytes(&bytes).raw()
}

fn hash_u32s(values: &[u32]) -> u64 {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    StableHash::of_bytes(&bytes).raw()
}

/// A stable integer for the sampling mode, so a texture that silently stops
/// being anisotropic is a byte diff rather than a shimmer nobody notices.
fn sampling_index(sampling: TextureSampling) -> u32 {
    match sampling {
        TextureSampling::Crisp => 0,
        TextureSampling::Anisotropic => 1,
    }
}

// --- golden machinery -------------------------------------------------------

fn golden_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("golden");
    p.push(format!("{name}.bin"));
    p
}

fn assert_golden(name: &str, actual: &[u8]) {
    let path = golden_path(name);
    // Compared against `"1"`, not merely present: `AXIOM_REGOLD=0` reading as
    // "yes, re-bless everything" is the kind of footgun that silently destroys a
    // baseline.
    let force = std::env::var("AXIOM_REGOLD").as_deref() == Ok("1");
    match std::fs::read(&path).ok() {
        Some(expected) if !force => assert_eq!(
            actual,
            expected.as_slice(),
            "golden mismatch for `{name}` ({} bytes actual vs {} bytes golden): the Burnt Rubber \
             agent-played run drifted. If intended, re-capture (AXIOM_REGOLD=1) and repin its \
             SHA-256 in apps/burnt-rubber/slice.toml.",
            actual.len(),
            expected.len(),
        ),
        _ => {
            std::fs::create_dir_all(path.parent().expect("golden dir has a parent"))
                .expect("create golden dir");
            std::fs::write(&path, actual).expect("write golden");
        }
    }
}

/// The three artifacts of one checkpoint: simulation state, render boundary,
/// generated resources.
struct Artifacts {
    state: Vec<u8>,
    render: Vec<u8>,
    resources: Vec<u8>,
}

/// Drive the run to `checkpoint` once and produce all three artifacts.
///
/// One drive per checkpoint, not three: they must describe the *same* app
/// instance, or a determinism bug could hide between them.
fn artifacts(checkpoint: &GoldenCheckpoint) -> Artifacts {
    let (mut app, steps) = golden::driven_with_count(checkpoint.stop);
    let state = encode_state(&golden::state_of(&app, steps));
    let render = encode_frame_outcome(&app.present());
    let resources = encode_resources(&mut app);
    Artifacts {
        state,
        render,
        resources,
    }
}

// --- the pinned baseline ----------------------------------------------------

/// Every checkpoint's three artifacts, against the committed bytes. One test so
/// the whole run is driven once per checkpoint rather than three times over.
#[test]
fn the_golden_run_matches_the_committed_baseline() {
    CHECKPOINTS.iter().for_each(|checkpoint| {
        let a = artifacts(checkpoint);
        assert_golden(&format!("agent_{}_state", checkpoint.name), &a.state);
        assert_golden(&format!("agent_{}_render", checkpoint.name), &a.render);
        assert_golden(&format!("agent_{}_resources", checkpoint.name), &a.resources);
    });
}

/// POSITIVE: the run replays byte-equal, in all three artifacts, at every
/// checkpoint. This is the property the committed goldens rest on — without it
/// they would be pinning noise.
#[test]
fn the_golden_run_replays_byte_equal() {
    CHECKPOINTS.iter().for_each(|checkpoint| {
        let first = artifacts(checkpoint);
        let second = artifacts(checkpoint);
        assert_eq!(
            first.state, second.state,
            "checkpoint `{}`: the simulation state is not byte-identical across two runs",
            checkpoint.name
        );
        assert_eq!(
            first.render, second.render,
            "checkpoint `{}`: the render boundary is not byte-identical across two runs",
            checkpoint.name
        );
        assert_eq!(
            first.resources, second.resources,
            "checkpoint `{}`: the generated resources are not byte-identical across two runs",
            checkpoint.name
        );
    });
}

/// NEGATIVE: the five checkpoints are five *different* moments.
///
/// The state comparison deliberately skips [`STATE_PREFIX_BYTES`]. `steps` and
/// `sim_steps` lead the encoding and are distinct across the checkpoints by
/// construction, so an inequality over the whole record could never fail
/// whatever the game did — it would be exactly the vacuous guard this test
/// exists to avoid being. Comparing past them asks the real question: do the
/// five checkpoints describe five different races?
#[test]
fn the_checkpoints_are_all_different_frames() {
    let all: Vec<Artifacts> = CHECKPOINTS.iter().map(artifacts).collect();
    (0..all.len()).for_each(|i| {
        ((i + 1)..all.len()).for_each(|j| {
            assert_ne!(
                all[i].state[STATE_PREFIX_BYTES..],
                all[j].state[STATE_PREFIX_BYTES..],
                "checkpoints `{}` and `{}` describe the identical race state",
                CHECKPOINTS[i].name,
                CHECKPOINTS[j].name
            );
            assert_ne!(
                all[i].render, all[j].render,
                "checkpoints `{}` and `{}` render the identical frame",
                CHECKPOINTS[i].name, CHECKPOINTS[j].name
            );
        });
    });
    // The resources are deliberately NOT asserted distinct: the mesh and texture
    // set is installed once at construction and is the *same* for all five
    // checkpoints. That it stays identical across a whole race is itself the
    // claim — nothing is generated after the scene is built (see
    // `src/render/chunks.rs:1-14`), so five identical resource artifacts are the
    // expected and correct result.
    let all_same = all
        .windows(2)
        .all(|pair| pair[0].resources == pair[1].resources);
    assert!(
        all_same,
        "the resource set must not change during a race — nothing may be generated after install"
    );
}

/// NEGATIVE: the baseline is sensitive to the *course*. A different seed
/// produces a different road, and therefore different bytes in all three
/// artifacts — so the goldens would genuinely catch a change to what is
/// generated, rather than passing because the encoder is blind.
#[test]
fn a_different_course_produces_different_bytes() {
    let baseline = artifacts(&CHECKPOINTS[1]);
    let other = driven_variant(
        golden::GOLDEN_SEED ^ 0x9E37_79B9_7F4A_7C15,
        &golden::GOLDEN_DRIVER,
        700,
    );

    assert_ne!(
        baseline.state, other.state,
        "a different course seed must move the simulation bytes"
    );
    assert_ne!(
        baseline.render, other.render,
        "a different course seed must move the render bytes"
    );
    assert_ne!(
        baseline.resources, other.resources,
        "a different course seed must move the generated road geometry"
    );
}

/// NEGATIVE: the baseline is sensitive to the *driver*.
///
/// `GOLDEN_DRIVER` is asserted to be a pinned constant, but pinning it proves
/// nothing unless the bytes actually move when it changes.
///
/// The knob is `steer_gain_milli` — the proportional term of the control law
/// `axiom-agent` is handed — and the choice is not arbitrary. The first version
/// of this test perturbed `grip_usage` by 0.01 and **the bytes did not move at
/// all**, which is a true and slightly surprising fact about this course rather
/// than a fault in the fixture: the road is flat out end to end (its sharpest
/// corner is well inside what the chassis holds at top speed — see
/// `agent::choose_line`), so `plan_speed` saturates against the car's top speed
/// everywhere and the cornering-limit term never binds. A driver parameter that
/// only shapes braking is therefore invisible on this road, and pinning it is
/// decorative.
///
/// `steer_gain_milli` is emitted as a `move_axis` intent on **every one of the
/// 5 419 steps**, so a change to it changes the car's line immediately and
/// keeps changing it. Perturbing it proves both that the goldens track the
/// driver and that the agent is genuinely in the loop.
#[test]
fn a_different_driver_produces_different_bytes() {
    let baseline = artifacts(&CHECKPOINTS[2]);
    let twitchier = axiom_burnt_rubber::agent::DriverTuning {
        steer_gain_milli: golden::GOLDEN_DRIVER.steer_gain_milli + 500,
        ..golden::GOLDEN_DRIVER
    };
    let other = driven_variant(golden::GOLDEN_SEED, &twitchier, 2200);

    assert_ne!(
        baseline.state, other.state,
        "a change to the driver's technique must move the simulation bytes"
    );
    assert_ne!(
        baseline.render, other.render,
        "a change to the driver's technique must move the render bytes"
    );
    // The resources are course-derived, not driver-derived, so they must NOT
    // move — which also proves the three artifacts are genuinely independent
    // rather than three views of one blob.
    assert_eq!(
        baseline.resources, other.resources,
        "the driver's technique must not change the generated road"
    );
}

/// Drive a deliberately-perturbed variant of the golden run for `steps` steps.
/// Shared by the two negative tests above so each states only its perturbation.
fn driven_variant(
    seed: u64,
    driver: &axiom_burnt_rubber::agent::DriverTuning,
    steps: u32,
) -> Artifacts {
    let mut app = axiom_burnt_rubber::BurntRubber::with_profile(
        seed,
        golden::GOLDEN_TUNING,
        golden::GOLDEN_WIDTH,
        golden::GOLDEN_HEIGHT,
        golden::GOLDEN_PROFILE,
    );
    (0..steps).for_each(|step| {
        let (command, _) =
            axiom_burnt_rubber::agent::drive_one_step(app.sim(), driver, u64::from(step));
        app.advance_steps(1, command);
    });
    let state = encode_state(&golden::state_of(&app, steps));
    let render = encode_frame_outcome(&app.present());
    let resources = encode_resources(&mut app);
    Artifacts {
        state,
        render,
        resources,
    }
}

/// The run reaches the finish under the agent, inside its cap, and the final
/// checkpoint is a completed race rather than a timeout.
#[test]
fn the_run_is_a_completed_agent_race() {
    let (app, steps) = golden::driven_with_count(GoldenStop::Finish);
    let state = golden::state_of(&app, steps);
    assert!(
        steps < GOLDEN_STEP_LIMIT,
        "the agent hit the {GOLDEN_STEP_LIMIT}-step cap without finishing"
    );
    assert!(
        state.progress > 0.99,
        "the run ended at {:.1}% of the course",
        state.progress * 100.0
    );
    assert!(
        state.near_misses > 60,
        "only {} near misses — the agent is not racing the way the baseline recorded",
        state.near_misses
    );
    println!(
        "burnt-rubber golden run: finished in {steps} steps ({:.2} s), \
         {} near misses, {} impacts, top speed {:.1} m/s",
        state.elapsed_seconds, state.near_misses, state.impacts, state.top_speed
    );
}
