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

/// One tick's `FramePacket`, carrying every draw's `surface_program` and the
/// frame's engine time.
pub fn packet_of(outcome: &FrameOutcome, width: u32, height: u32) -> FramePacket {
    let draws: Vec<FrameDrawItem> = outcome
        .draws()
        .iter()
        .enumerate()
        .map(|(index, draw)| {
            FrameDrawItem::new(
                index as u64,
                draw.mesh_id(),
                draw.material_id(),
                draw.world(),
                draw.mvp(),
                draw.color(),
                draw.casts_contact_shadow(),
            )
            .with_emissive(draw.emissive())
            .with_specular(draw.specular())
            // **The lane this whole app is about.** Without it, every station
            // renders the neutral constant fallback and the demonstration is of
            // nothing.
            .with_surface_program(draw.surface_program())
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
        outcome.light_view_proj(),
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
        assert_eq!(programs.len(), 13);
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
