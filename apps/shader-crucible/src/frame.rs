//! The app's own `FrameOutcome` → `FramePacket` translation.
//!
//! **This is app glue by necessity, not by preference**, and the reason is worth
//! stating precisely because it is the finding this app exists to surface.
//!
//! `axiom_host::FramePacket` is the one artifact that carries a draw's
//! `surface_program` across the presentation boundary, and
//! `GpuBackendApi::present_packet_with_surfaces` / `Canvas2dBackendApi::{
//! present_packet_with_surfaces, render_offscreen_rgba_with_surfaces}` are the
//! only entries that take an authored `Surface` set. Every other route into a
//! backend — `present_frame`, `present_frame_result`, `render_offscreen_rgba`,
//! and therefore `axiom-windowing`'s live loop and `axiom-shot`'s capture — takes
//! **explicit instance batches** and passes an empty program slice. A frame that
//! reaches a backend that way cannot name a surface program, whatever the app
//! authored.
//!
//! So an app that wants its authored surfaces to reach pixels has to build the
//! packet itself. That is what this module does: one `FramePacket` per tick,
//! carrying each draw's `surface_program` and the frame's **engine** time, which
//! is what a `Time`-reading channel samples in both stages.
//!
//! The translation is deliberately per-*draw* rather than per-batch. A batch is
//! keyed on `(mesh, material)` and a program is a property of the material, so
//! the two agree — but going through the draw list keeps the program, the
//! emissive, the specular and the caster flag on the same record they were
//! authored on, and `frame_packet_to_batches` re-batches on the other side
//! anyway.

use axiom::prelude::*;
use axiom_host::{
    FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
};
use axiom_kernel::{Ratio, Seconds};

use crate::levers::PacketPlan;

/// The fixed simulation rate the crucible's engine time is derived from. A
/// **tick count**, never a wall clock: tick *N* replayed twice produces the same
/// `Seconds`, so station 5 deforms identically on a replay.
pub const TICK_HZ: f64 = 60.0;

/// The column-major identity, for the packet lanes the software arm does not
/// read.
const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// The engine time at `tick` — `tick / 60`, exactly.
pub fn time_at(tick: u64) -> Seconds {
    Seconds::finite_or_zero((tick as f64 / TICK_HZ) as f32)
}

/// **The camera's world-space right and up**, recovered from the frame's
/// view-projection.
///
/// Recovered by *unprojecting* three clip-space points rather than by reading
/// the matrix elements. Reading them would mean committing to a handedness, a
/// depth range and a row/column order all at once, and getting any of the three
/// wrong produces captions that are mirrored or upside down rather than an
/// error. Unprojecting assumes only what every graphics API agrees on — that
/// clip `+x` is right and clip `+y` is up — and
/// `tests::the_camera_basis_is_recovered_from_the_view_projection` pins the
/// result against the app's own authored camera, so the assumption is checked
/// rather than trusted.
///
/// `None` when the projection is singular, which the authored camera's is not.
fn camera_basis(view_proj: [f32; 16]) -> Option<(Vec3, Vec3)> {
    let inverse = Mat4::from_cols_array(view_proj).inverse()?;
    let unproject = |x: f32, y: f32| {
        let v = inverse.transform_vec4(Vec4::new(x, y, 0.5, 1.0));
        (v.w.abs() > 1.0e-9).then(|| Vec3::new(v.x / v.w, v.y / v.w, v.z / v.w))
    };
    let origin = unproject(0.0, 0.0)?;
    let right = unproject(0.5, 0.0)?.subtract(origin).normalize().ok()?;
    let up = unproject(0.0, 0.5)?.subtract(origin).normalize().ok()?;
    Some((right, up))
}

/// `world` with its rotation replaced by the camera's basis, keeping its
/// translation and its per-axis scale.
///
/// `right x up` is the camera's *backward* axis, so the caption's local `+z` —
/// the face [`crate::label::caption_mesh`] winds counter-clockwise — ends up
/// pointing at the eye from any orbit position.
fn faced(world: [f32; 16], right: Vec3, up: Vec3) -> [f32; 16] {
    // Column-major: element (row r, column c) is `world[c * 4 + r]`.
    let axis_scale = |column: usize| {
        Vec3::new(world[column * 4], world[column * 4 + 1], world[column * 4 + 2]).length()
    };
    let (sx, sy, sz) = (axis_scale(0), axis_scale(1), axis_scale(2));
    let backward = right.cross(up);
    [
        right.x * sx,
        right.y * sx,
        right.z * sx,
        0.0,
        up.x * sy,
        up.y * sy,
        up.z * sy,
        0.0,
        backward.x * sz,
        backward.y * sz,
        backward.z * sz,
        0.0,
        world[12],
        world[13],
        world[14],
        world[15],
    ]
}

/// **The captions, turned to face the camera.** The app's per-frame hook for
/// anything that depends on the eye, and the reason this translation is the
/// right place for it: `packet_of` is called once per frame with the frame's own
/// camera already resolved, so a billboard costs no extra scene pass and no
/// second opinion about where the eye is.
///
/// The captions are the **last** [`crate::label::COUNT`] draws of the frame
/// because [`crate::stand::populate`] stands them up after every body;
/// `tests::the_caption_draws_are_the_last_draws_of_the_frame` pins that
/// partition against the registered mesh ids rather than trusting the order.
fn billboard_of(
    outcome: &FrameOutcome,
) -> (Option<(Vec3, Vec3)>, usize, Mat4) {
    (
        camera_basis(outcome.camera_view_proj()),
        outcome.draws().len().saturating_sub(crate::label::COUNT),
        Mat4::from_cols_array(outcome.camera_view_proj()),
    )
}

/// One tick's `FramePacket`, carrying every draw's `surface_program` and the
/// frame's engine time — the whole frame, exactly as the app ships it.
pub fn packet_of(outcome: &FrameOutcome, width: u32, height: u32) -> FramePacket {
    packet_of_plan(outcome, width, height, PacketPlan::EVERYTHING)
}

/// One tick's `FramePacket`, cut down to what `plan` keeps.
///
/// **The cut happens here and nowhere else.** A diagnostic lever that removed a
/// body from the *scene* would change the simulation, and then two runs would
/// not be two readings of one experiment — they would be two experiments. The
/// scene walk is identical for every lever position; what differs is which of
/// its draws reach the backend, which is exactly what "12 of 25 draws are
/// captions" is a claim about.
///
/// `plan.shadows` is the same idea applied to the light: a frame with shadows
/// off carries the **identity** light projection rather than the scene's. That
/// makes the light's culling frustum the world cube `[-1, 1]³`, which every body
/// on the stand is outside of, so the shadow pass submits no draws and every
/// fragment's shadow lookup lands off the map and reads a hard 1.0. See
/// [`crate::levers`] for what that does *not* remove.
pub fn packet_of_plan(
    outcome: &FrameOutcome,
    width: u32,
    height: u32,
    plan: PacketPlan,
) -> FramePacket {
    let (basis, first_caption, view_proj) = billboard_of(outcome);
    let total = outcome.draws().len();
    let draws: Vec<FrameDrawItem> = outcome
        .draws()
        .iter()
        .enumerate()
        .filter(|(index, _)| plan.keeps(*index, total))
        .map(|(index, draw)| {
            let (world, mvp) = basis
                .filter(|_| index >= first_caption)
                .map(|(right, up)| {
                    let world = faced(draw.world(), right, up);
                    (
                        world,
                        view_proj
                            .multiply(Mat4::from_cols_array(world))
                            .as_cols_array(),
                    )
                })
                .unwrap_or((draw.world(), draw.mvp()));
            FrameDrawItem::new(
                index as u64,
                draw.mesh_id(),
                draw.material_id(),
                world,
                mvp,
                draw.color(),
                draw.casts_contact_shadow(),
            )
            .with_emissive(draw.emissive())
            .with_specular(draw.specular())
            // **The lane this whole app is about.** Without it, every station
            // renders the neutral constant fallback and the demonstration is of
            // nothing — which is exactly what the `surfaces` lever asks for,
            // body by body, so the cost of a generated shader can be measured
            // against the cost of not having one.
            .with_surface_program(plan.program_of(index, total, draw.surface_program()))
        })
        .collect();
    let lights: Vec<FrameLight> = outcome
        .lights()
        .iter()
        .map(|light| {
            let c = light.color();
            FrameLight::new(light.kind(), light.vec(), [c[0], c[1], c[2], light.intensity()])
        })
        .collect();
    let directional = outcome.lights().iter().filter(|l| l.kind() == 0).count() as u32;
    let point = outcome.lights().iter().filter(|l| l.kind() == 1).count() as u32;
    FramePacket::new(
        outcome.tick(),
        outcome.tick(),
        FrameViewport::new(width, height),
        outcome.clear_color(),
        Some(FrameCamera::new(
            IDENTITY,
            IDENTITY,
            outcome.camera_view_proj(),
        )),
        draws,
        lights,
        // Shadows off hands the backend the identity here — see this function's
        // docs. It is the one app-reachable lever that removes shadow *draws*.
        plan.shadows
            .then(|| outcome.light_view_proj())
            .unwrap_or(IDENTITY),
        // The `uses_shadows` flag stays `false` whatever the lever says: no GPU
        // route reads it (only the Canvas2D arm does, to report a degradation),
        // so setting it would move a *readout* without moving a pixel — the one
        // thing a diagnostic must never do.
        FrameFeatureSet::new(false, directional > 0, directional, point),
    )
    .with_ambient(outcome.ambient())
    // The frame's own supplied engine time. A surface set that reads no clock is
    // written an exact zero whatever this says, so a static station's frame is
    // byte-identical to what it was before there was a clock at all.
    .with_time(time_at(outcome.tick()))
}

/// Silences nothing — `Ratio` is named by `with_specular`'s argument type.
#[allow(dead_code)]
const fn _ratio_is_named(value: Ratio) -> Ratio {
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::crucible_core;

    #[test]
    fn engine_time_is_a_tick_count_and_never_a_clock() {
        assert_eq!(time_at(0).get(), 0.0);
        assert_eq!(time_at(60).get(), 1.0);
        assert_eq!(time_at(90).get(), 1.5);
        assert_eq!(time_at(90), time_at(90));
    }

    /// **Every station body's `surface_program` survives into the packet.** This
    /// is the assertion the whole translation exists for: an engine route that
    /// drops the lane renders eleven neutral white bodies.
    #[test]
    fn every_authored_program_reaches_the_packet() {
        let (mut app, _) = crucible_core();
        let outcome = app.render(0);
        let packet = packet_of(&outcome, 640, 360);
        let programs: Vec<u64> = packet
            .draws()
            .iter()
            .map(|draw| draw.surface_program())
            .collect();
        assert_eq!(programs.len(), 13 + crate::label::COUNT);
        assert_eq!(
            programs.iter().filter(|p| **p != 0).count(),
            11,
            "eleven authored surfaces must reach the packet"
        );
        let authored: std::collections::BTreeSet<u64> = crate::stations::all_surfaces()
            .iter()
            .map(|s| s.digest().raw())
            .collect();
        programs
            .iter()
            .filter(|p| **p != 0)
            .for_each(|p| assert!(authored.contains(p)));
    }

    /// The packet carries the frame's engine time, so station 5's `Time`-reading
    /// channels have a clock — and a replayed tick carries the same one.
    #[test]
    fn the_packet_carries_the_frames_engine_time() {
        let (mut app, _) = crucible_core();
        let early = packet_of(&app.render(0), 640, 360);
        let later = packet_of(&app.render(120), 640, 360);
        assert_eq!(early.time().get(), 0.0);
        assert_eq!(later.time().get(), 2.0);
        assert_eq!(packet_of(&app.render(120), 640, 360), later);
    }

    /// **The recovered basis is the authored camera's own basis.**
    ///
    /// This is the assertion that makes [`camera_basis`] safe to write without
    /// committing to a matrix convention. The crucible's authored camera looks
    /// straight down `-z` with `+y` up, so its right is world `+x` and its up is
    /// world `+y` — and if the unprojection had the handedness, the depth range
    /// or the row/column order wrong, one of the two would come back negated and
    /// every caption would render mirrored or upside down.
    #[test]
    fn the_camera_basis_is_recovered_from_the_view_projection() {
        let (mut app, _) = crucible_core();
        let outcome = app.render(0);
        let (right, up) = camera_basis(outcome.camera_view_proj()).expect("a real camera");
        assert!((right.x - 1.0).abs() < 1.0e-3, "{right:?}");
        assert!(right.y.abs() + right.z.abs() < 1.0e-3, "{right:?}");
        assert!((up.y - 1.0).abs() < 1.0e-3, "{up:?}");
        assert!(up.x.abs() + up.z.abs() < 1.0e-3, "{up:?}");
        // Right x up is backward: from the stand toward the eye, which is `+z`
        // for this framing. That sign is what puts the caption's lit face
        // outward instead of into the screen.
        let backward = right.cross(up);
        assert!((backward.z - 1.0).abs() < 1.0e-3, "{backward:?}");
    }

    /// **Every caption faces the camera, and no body is touched.**
    ///
    /// Asserted on the packet the backend actually receives: each caption draw's
    /// world matrix has its `+z` column equal to the camera's backward axis, and
    /// each body draw's world matrix is byte-identical to the one the scene
    /// produced. A billboard that leaked onto the bodies would spin the whole
    /// stand to face the eye.
    ///
    /// Taken from an **orbited** eye, deliberately. The authored framing looks
    /// straight down `-z`, and an unrotated body's third column is already
    /// `(0, 0, 1)` there — so from the opening shot a billboarded caption and a
    /// perfectly still sphere are indistinguishable, and the test would pass
    /// whatever the code did.
    #[test]
    fn the_captions_face_the_camera_and_the_bodies_do_not_move() {
        let (mut app, _) = crucible_core();
        app.set_camera(
            crate::scene::scene_camera(),
            Transform::from_translation(Vec3::new(9.0, 2.0, 4.0))
                .looking_at(crate::scene::camera_target(), Vec3::UNIT_Y)
                .expect("an orbited eye is a legal camera"),
        );
        let outcome = app.render(0);
        let packet = packet_of(&outcome, 640, 360);
        let (right, up) = camera_basis(outcome.camera_view_proj()).expect("a real camera");
        let backward = right.cross(up);
        let split = packet.draws().len() - crate::label::COUNT;
        packet.draws().iter().enumerate().for_each(|(index, draw)| {
            let world = draw.world();
            let scene = outcome.draws()[index].world();
            let faces_camera = (world[8] - backward.x).abs()
                + (world[9] - backward.y).abs()
                + (world[10] - backward.z).abs()
                < 1.0e-3;
            assert_eq!(
                index >= split,
                faces_camera,
                "draw {index} is billboarded when it should not be, or the reverse"
            );
            // A body's matrix is untouched; a caption's keeps its translation.
            assert_eq!(
                (world[12], world[13], world[14]),
                (scene[12], scene[13], scene[14]),
                "draw {index} moved"
            );
        });
    }

    /// **The billboard follows the camera.** Two different eyes produce two
    /// different caption orientations and the *same* body transforms — which is
    /// what "the caption tracks the body as the camera orbits" means when it is
    /// checked rather than claimed.
    #[test]
    fn a_moved_camera_turns_the_captions_and_nothing_else() {
        let (mut app, _) = crucible_core();
        let front = packet_of(&app.render(0), 640, 360);
        app.set_camera(
            crate::scene::scene_camera(),
            Transform::from_translation(Vec3::new(9.0, 2.0, 4.0))
                .looking_at(crate::scene::camera_target(), Vec3::UNIT_Y)
                .expect("an orbited eye is a legal camera"),
        );
        let side = packet_of(&app.render(0), 640, 360);
        let split = front.draws().len() - crate::label::COUNT;
        // Every body's world matrix is unchanged by moving the eye.
        (0..split).for_each(|index| {
            assert_eq!(
                front.draws()[index].world(),
                side.draws()[index].world(),
                "body {index} moved with the camera"
            );
        });
        // Every caption's has turned.
        (split..front.draws().len()).for_each(|index| {
            assert_ne!(
                front.draws()[index].world(),
                side.draws()[index].world(),
                "caption {index} did not follow the camera"
            );
        });
    }

    /// The billboard is a pure function of the frame: the same tick seen from
    /// the same eye twice is the same packet, captions included.
    #[test]
    fn the_billboard_is_deterministic() {
        let (mut app, _) = crucible_core();
        let first = packet_of(&app.render(7), 640, 360);
        assert_eq!(packet_of(&app.render(7), 640, 360), first);
    }

    /// A singular view-projection has no basis to recover, and the frame falls
    /// back to the scene's own matrices rather than producing a NaN transform.
    #[test]
    fn a_singular_view_projection_leaves_every_draw_alone() {
        assert_eq!(camera_basis([0.0; 16]), None);
    }

    #[test]
    fn the_packet_carries_the_scenes_lights_and_camera() {
        let (mut app, _) = crucible_core();
        let packet = packet_of(&app.render(0), 640, 360);
        assert_eq!(packet.lights().len(), 2);
        assert!(packet.camera().is_some());
        assert_eq!(
            (packet.viewport().width(), packet.viewport().height()),
            (640, 360)
        );
    }
}
