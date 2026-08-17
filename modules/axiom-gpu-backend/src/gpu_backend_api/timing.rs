//! The GPU backend's **legibility** surface: what the frame cost, what it ran
//! on, and what it lowered to.
//!
//! A third `impl GpuBackendApi` block rather than a third type — the Module Law
//! gives this crate exactly one facade, and these are that facade's methods. What
//! separates them from [`super`] and from [`super::surfaces`] is that they
//! **produce nothing**: bind-time work lives in one, frame-time work in the
//! other, and every method here only reports a fact one of those already
//! established.
//!
//! ## Why this file exists at all
//!
//! An app running at 30 fps with an idle main thread had no way to ask this
//! backend where the time went. Every render pass passed `timestamp_writes:
//! None`, the bound graphics API reached the page only as a console line an app
//! had to *intercept*, and the batch and pipeline-switch counts the frame adapter
//! computes were thrown away, so an app re-derived them from the packet and hoped
//! it had matched the backend's own sort key. Each of those is one accessor, and
//! each of them replaces a guess with a measurement.

use axiom_host::FramePacket;

use crate::gpu_backend_api::GpuBackendApi;
use crate::gpu_pass_timing::GpuFrameTiming;

impl GpuBackendApi {
    /// **The most recent resolved per-pass GPU time**, or the reason there is
    /// none.
    ///
    /// The reading names each pass the measured frame ran — `shadow`, `main`,
    /// `sdf`, `post`, `draw2d` — with the duration the GPU spent inside it, so a
    /// caller can say *"the shadow pass is 19 ms of your 33 ms frame"* instead of
    /// inferring it from an A/B test. Three properties are load-bearing:
    ///
    /// * **It is gated on the adapter.** `wgpu::Features::TIMESTAMP_QUERY` is
    ///   optional and the browser's WebGL2 fallback cannot do it at all. Where it
    ///   is missing this reports *unavailable, with the reason*
    ///   (`GpuFrameTiming::unavailable_reason`) — never a zero, and never an
    ///   estimate.
    /// * **It is never this frame's.** Resolving a query set means mapping a GPU
    ///   buffer, which completes on a later task; blocking the frame for it would
    ///   cost more than the passes being measured. So this is the last frame that
    ///   *finished* resolving, and `GpuFrameTiming::frame` says which one that
    ///   was.
    /// * **A pass the frame did not run is absent**, not zero — the SDF pass on a
    ///   frame carrying no SDF scene simply does not appear.
    ///
    /// Always unavailable on native: there is no live binding, so there are no
    /// passes to time.
    pub fn gpu_pass_timing(&self) -> GpuFrameTiming {
        #[cfg(target_arch = "wasm32")]
        {
            return self.live.as_ref().map_or_else(
                || GpuFrameTiming::unavailable(crate::gpu_pass_timing::NO_LIVE_BINDING),
                crate::live_gpu_binding::LiveGpuBinding::pass_timing,
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            GpuFrameTiming::unavailable(crate::gpu_pass_timing::NO_LIVE_BINDING)
        }
    }

    /// **Which graphics API this backend actually bound** — `GpuPrimary` for
    /// WebGPU, `GpuFallback` for the WebGL2 fallback — or `None` before
    /// [`Self::initialize`] has succeeded (always, on native).
    ///
    /// `wgpu` decides this at bind time and the binding has always *logged* it;
    /// until now it kept no accessor, so an app that wanted to display it had to
    /// intercept `console.log` and scrape the line. The fact is the engine's, so
    /// the engine reports it.
    pub fn bound_backend(&self) -> Option<axiom_host::BackendKind> {
        #[cfg(target_arch = "wasm32")]
        {
            return self
                .live
                .as_ref()
                .map(crate::live_gpu_binding::LiveGpuBinding::bound_backend);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            None
        }
    }

    /// **What `packet` lowers to**: `(instance batches, pipeline switches)`.
    ///
    /// The first is how many indexed draw calls the main pass will issue — one
    /// per distinct `(surface_program, mesh, material)` — and the second is how
    /// many times it will change pipeline, which is one per distinct *non-zero*
    /// surface program, because the default pipeline is bound once before the
    /// loop and every draw that authors no surface runs under it.
    ///
    /// Both numbers are read off exactly the sort key
    /// [`crate::frame_packet_adapter`] groups on, so this is the backend's own
    /// answer rather than an app's reconstruction of it. An app that re-derived
    /// them from the packet was duplicating that key by hand, and a change to the
    /// grouping would have silently made its diagnostics wrong.
    pub fn packet_batch_counts(&self, packet: &FramePacket) -> (u32, u32) {
        crate::frame_packet_adapter::frame_packet_batch_counts(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu_backend_api::tests::request;
    use axiom_host::{FrameDrawItem, FrameFeatureSet, FrameViewport};

    /// A packet whose draws are `(mesh, material, surface program)` triples.
    fn packet(draws: &[(u64, u64, u64)]) -> FramePacket {
        FramePacket::new(
            1,
            60,
            FrameViewport::new(64, 64),
            [0.0; 4],
            None,
            draws
                .iter()
                .map(|(mesh, material, program)| {
                    FrameDrawItem::new(0, *mesh, *material, [0.0; 16], [0.0; 16], [1.0; 4], false)
                        .with_surface_program(*program)
                })
                .collect(),
            Vec::new(),
            [0.0; 16],
            FrameFeatureSet::new(false, false, 0, 0),
        )
    }

    /// **Native reports unavailable, with a reason — never a zero.** This is the
    /// whole contract of the unavailable state, asserted at the facade a caller
    /// actually holds: two GPU diagnoses in this engine's history were wrong
    /// because a plausible number stood in for a missing measurement.
    #[test]
    fn a_backend_with_no_live_binding_reports_unavailable_and_says_why() {
        let backend = GpuBackendApi::new(&request(320, 240));
        let timing = backend.gpu_pass_timing();
        assert!(!timing.is_available());
        assert!(!timing.unavailable_reason().is_empty());
        assert!(timing.unavailable_reason().contains("no live GPU binding"));
        // No numbers at all — not a zeroed set of passes.
        assert!(timing.passes().is_empty());
        // And nothing bound, so nothing to report about the graphics API.
        assert_eq!(backend.bound_backend(), None);
    }

    /// One draw is one batch, and a frame that authors no surface changes
    /// pipeline **zero** times: the default pipeline is bound before the loop.
    #[test]
    fn an_unsurfaced_packet_costs_no_pipeline_switch() {
        let backend = GpuBackendApi::new(&request(64, 64));
        assert_eq!(backend.packet_batch_counts(&packet(&[])), (0, 0));
        assert_eq!(backend.packet_batch_counts(&packet(&[(1, 1, 0)])), (1, 0));
        // Two draws of the same mesh AND material instance into one batch…
        assert_eq!(
            backend.packet_batch_counts(&packet(&[(1, 1, 0), (1, 1, 0)])),
            (1, 0)
        );
        // …while a different mesh, or a different material, is its own batch.
        assert_eq!(
            backend.packet_batch_counts(&packet(&[(1, 1, 0), (2, 1, 0), (1, 2, 0)])),
            (3, 0)
        );
    }

    /// **A surface program is a pipeline**, so it both splits batches and costs a
    /// switch — and two draws sharing a program cost only one.
    #[test]
    fn each_distinct_surface_program_costs_one_pipeline_switch() {
        let backend = GpuBackendApi::new(&request(64, 64));
        // The same mesh+material under two programs cannot share a batch.
        assert_eq!(
            backend.packet_batch_counts(&packet(&[(1, 1, 7), (1, 1, 9)])),
            (2, 2)
        );
        // Two draws under one program: one switch, and one batch since they also
        // share mesh and material.
        assert_eq!(
            backend.packet_batch_counts(&packet(&[(1, 1, 7), (1, 1, 7)])),
            (1, 1)
        );
        // Mixed: the unsurfaced half runs under the default pipeline (no switch),
        // the surfaced half costs exactly one.
        assert_eq!(
            backend.packet_batch_counts(&packet(&[(1, 1, 0), (2, 1, 7), (3, 1, 7)])),
            (3, 1)
        );
    }
}
