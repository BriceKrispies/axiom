//! **What one GPU frame cost, pass by pass** — the pure, target-agnostic half of
//! GPU timing.
//!
//! A frame that runs at 30 fps with an idle main thread is spending its time
//! somewhere on the GPU, and until this existed nothing in the engine could say
//! *where*: every render pass in this backend passed `timestamp_writes: None`, so
//! "not on the main thread: 28 ms" could not be attributed to the shadow pass,
//! the surface programs, the post chain or the 2D compositor. Two GPU diagnoses
//! were made — and were wrong — from black-box A/B tests precisely because there
//! was no measurement to make instead.
//!
//! This module owns the arithmetic and the vocabulary; it touches no GPU object,
//! so every rule below is exercised by the coverage gate on native rather than
//! hidden behind a `wasm32` arm. The wgpu binding that produces its input — the
//! query set, the pass attachments and the asynchronous resolve — is
//! `crate::gpu_pass_clock`, which is compiled only where a real GPU exists.
//!
//! ## Unavailability is a first-class state, never a zero
//!
//! `wgpu::Features::TIMESTAMP_QUERY` is optional, and the browser's WebGL2
//! fallback cannot do it **at all**. A backend that cannot measure therefore
//! reports [`GpuFrameTiming::unavailable_reason`] — a sentence naming why — and
//! reports **no numbers**. It never reports `0.0 ms`, which is indistinguishable
//! from a pass that really did cost nothing, and it never estimates. A pass that
//! a frame did not record (the SDF pass on a frame carrying no SDF scene) is
//! likewise *absent* from [`GpuFrameTiming::passes`] rather than present as a
//! zero.

use axiom_kernel::{FrameIndex, Seconds};

/// How many named passes one recorded GPU frame is measured in.
pub(crate) const PASS_COUNT: usize = 5;

/// What each pass reports itself as, in slot order. The order is the order the
/// frame records them, so a reader of [`GpuFrameTiming::passes`] sees the frame
/// laid out in the sequence the GPU executed it.
///
/// * `shadow` — the directional shadow-map depth pre-pass.
/// * `main` — the lit/textured/shadowed scene pass, surface programs included.
/// * `sdf` — the SDF raymarch composite, on frames that carry an SDF scene.
/// * `post` — the bloom + grade chain when the app authored one, and the plain
///   upscale blit when it did not. Either way it is the whole present-side
///   fullscreen work between the scene and the swap chain.
/// * `draw2d` — the alpha-blended 2D quad pass (a 2D present is its own frame,
///   so this never appears beside the 3D passes).
const PASS_NAMES: [&str; PASS_COUNT] = ["shadow", "main", "sdf", "post", "draw2d"];

/// Why a native build reports no GPU pass timings: it has no live binding at
/// all, so there is no device whose passes could be measured.
pub(crate) const NO_LIVE_BINDING: &str =
    "no live GPU binding: this build presents no pixels, so there are no passes to time";

/// The most recent **resolved** per-pass GPU timings, or the reason there are
/// none.
///
/// Read through [`crate::GpuBackendApi::gpu_pass_timing`]. Two things about it
/// are deliberate and load-bearing:
///
/// * **It is never same-frame.** Resolving a query set is a GPU→CPU buffer
///   map, which on the browser completes on a later task; blocking a frame to
///   wait for it would cost far more than the passes being measured. So this
///   carries the numbers of the most recent frame that *finished* resolving,
///   and [`Self::frame`] says which frame that was. A caller that wants to know
///   how stale a reading is compares that index against its own.
/// * **Absence is explicit.** [`Self::is_available`] is false whenever the
///   adapter cannot time passes or nothing has resolved yet, and
///   [`Self::passes`] is empty. There is no zero standing in for a missing
///   measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuFrameTiming {
    /// Nanoseconds per pass slot. Meaningful only where `recorded` has the bit.
    nanos: [u64; PASS_COUNT],
    /// Bit `p` is set when pass `p` actually wrote both of its timestamps in the
    /// resolved frame. A pass the frame did not run has no bit, and is reported
    /// as absent rather than as zero.
    recorded: u32,
    /// Which frame these numbers were recorded on.
    frame: FrameIndex,
    /// Empty when the timings are real; otherwise the sentence saying why there
    /// are none.
    reason: &'static str,
}

impl GpuFrameTiming {
    /// No timings, and the reason. `reason` must be non-empty — it is the whole
    /// point of this state, and an empty one would read as "available" with no
    /// passes, which is a different fact.
    pub(crate) const fn unavailable(reason: &'static str) -> Self {
        GpuFrameTiming {
            nanos: [0; PASS_COUNT],
            recorded: 0,
            frame: FrameIndex::ZERO,
            reason,
        }
    }

    /// Turn one frame's resolved timestamp ticks into per-pass durations.
    ///
    /// `ticks` is the query set read back verbatim: two entries per pass, begin
    /// then end. `recorded` is the mask of passes that actually attached their
    /// timestamps on that frame — the SDF pass is absent from a frame with no
    /// SDF scene, and reporting a stale or zero number for it would be exactly
    /// the fabrication this whole module exists to refuse. `period_ns` is
    /// `wgpu::Queue::get_timestamp_period`, the nanoseconds one tick represents
    /// on this adapter.
    ///
    /// An end tick below its begin (a wrapped or never-written slot) saturates
    /// to a zero *duration* rather than an enormous one; the pass is only
    /// reported at all when its bit is in `recorded`.
    ///
    /// Compiled wherever a GPU can produce ticks — and under `test`, where the
    /// arithmetic above is measured by the coverage gate on a native machine with
    /// no GPU arm compiled in at all.
    #[cfg(any(test, target_arch = "wasm32", feature = "offscreen"))]
    pub(crate) fn resolved(
        ticks: &[u64],
        recorded: u32,
        period_ns: f32,
        frame: FrameIndex,
    ) -> Self {
        let nanos = std::array::from_fn(|pass| {
            let begin = ticks.get(pass * 2).copied().unwrap_or_default();
            let end = ticks.get(pass * 2 + 1).copied().unwrap_or_default();
            ((end.saturating_sub(begin) as f64) * f64::from(period_ns)) as u64
        });
        GpuFrameTiming {
            nanos,
            recorded,
            frame,
            reason: "",
        }
    }

    /// Whether these are real measurements. False means [`Self::passes`] is
    /// empty and [`Self::unavailable_reason`] says why.
    pub fn is_available(&self) -> bool {
        self.reason.is_empty()
    }

    /// Why there are no timings — a sentence naming the cause (no live binding,
    /// an adapter without `TIMESTAMP_QUERY`, nothing resolved yet). Empty when
    /// [`Self::is_available`].
    pub fn unavailable_reason(&self) -> &'static str {
        self.reason
    }

    /// **Which frame these numbers came from.** Resolution is asynchronous, so
    /// this is behind the frame a caller is currently presenting; comparing it
    /// against the caller's own index is how stale a reading is measured rather
    /// than assumed.
    pub fn frame(&self) -> FrameIndex {
        self.frame
    }

    /// Each pass the resolved frame actually recorded, in execution order, as
    /// `(name, duration)`. Empty when the timings are unavailable, and missing
    /// any pass the frame did not run.
    pub fn passes(&self) -> Vec<(&'static str, Seconds)> {
        (0..PASS_COUNT)
            .filter(|pass| self.recorded & (1 << pass) != 0)
            .map(|pass| (PASS_NAMES[pass], nanos_to_seconds(self.nanos[pass])))
            .collect()
    }

    /// The summed GPU time of every recorded pass — what a caller compares its
    /// own measured frame interval against to see how much of the frame the GPU
    /// owns. Zero when the timings are unavailable, which is why a caller must
    /// consult [`Self::is_available`] first.
    pub fn total(&self) -> Seconds {
        nanos_to_seconds(
            (0..PASS_COUNT)
                .filter(|pass| self.recorded & (1 << pass) != 0)
                .map(|pass| self.nanos[pass])
                .sum(),
        )
    }
}

/// Nanoseconds as a dimensioned duration. Total: a computed scalar, so the
/// sanitizing constructor is the right one — there is no failure to report and
/// no naked float to leak.
fn nanos_to_seconds(nanos: u64) -> Seconds {
    Seconds::finite_or_zero((nanos as f32) * 1.0e-9)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tick stream where pass `p` spans `[p * 100, p * 100 + (p + 1) * 10]`.
    fn ticks() -> [u64; PASS_COUNT * 2] {
        std::array::from_fn(|slot| {
            let pass = slot / 2;
            let begin = (pass as u64) * 100;
            [begin, begin + ((pass as u64) + 1) * 10][slot % 2]
        })
    }

    /// Every pass bit set.
    const ALL: u32 = (1 << PASS_COUNT) - 1;

    /// A duration in nanoseconds, to within a femtosecond of float slack — the
    /// conversion runs through `f32` seconds, so an exact bit comparison would
    /// be asserting on the rounding rather than on the arithmetic.
    fn is_nanos(actual: Seconds, expected: f64) {
        let got = f64::from(actual.get()) * 1.0e9;
        assert!(
            (got - expected).abs() < 1.0e-3,
            "expected {expected} ns, got {got} ns"
        );
    }

    #[test]
    fn an_unavailable_timing_carries_its_reason_and_no_numbers() {
        let timing = GpuFrameTiming::unavailable(NO_LIVE_BINDING);
        assert!(!timing.is_available());
        assert_eq!(timing.unavailable_reason(), NO_LIVE_BINDING);
        assert!(timing.passes().is_empty());
        is_nanos(timing.total(), 0.0);
        assert_eq!(timing.frame(), FrameIndex::ZERO);
        // The reason is a sentence, not a code — it is meant to be printed.
        assert!(NO_LIVE_BINDING.contains("no live GPU binding"));
        assert!(format!("{timing:?}").starts_with("GpuFrameTiming"));
    }

    /// A tick period of exactly one nanosecond makes the arithmetic readable:
    /// pass `p` lasts `(p + 1) * 10` ticks, so `(p + 1) * 10` nanoseconds.
    #[test]
    fn resolved_ticks_become_named_per_pass_durations_in_execution_order() {
        let timing =
            GpuFrameTiming::resolved(&ticks(), ALL, 1.0, FrameIndex::new(42));
        assert!(timing.is_available());
        assert_eq!(timing.unavailable_reason(), "");
        assert_eq!(timing.frame(), FrameIndex::new(42));
        let passes = timing.passes();
        assert_eq!(
            passes.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            vec!["shadow", "main", "sdf", "post", "draw2d"]
        );
        is_nanos(passes[0].1, 10.0);
        is_nanos(passes[4].1, 50.0);
        // 10 + 20 + 30 + 40 + 50 nanoseconds.
        is_nanos(timing.total(), 150.0);
        // Two independently built readings of the same frame compare equal — the
        // property a caller relies on to notice that nothing new has resolved.
        assert_eq!(
            timing,
            GpuFrameTiming::resolved(&ticks(), ALL, 1.0, FrameIndex::new(42))
        );
        assert_ne!(
            timing,
            GpuFrameTiming::resolved(&ticks(), ALL, 1.0, FrameIndex::new(43))
        );
    }

    /// The adapter's tick period is not a nanosecond on every device; it scales
    /// every duration and nothing else.
    #[test]
    fn the_adapter_tick_period_scales_every_duration() {
        let timing = GpuFrameTiming::resolved(&ticks(), ALL, 2.5, FrameIndex::ZERO);
        is_nanos(timing.passes()[0].1, 25.0);
        is_nanos(timing.total(), 375.0);
    }

    /// **A pass the frame did not run is absent, never a zero.** The SDF pass is
    /// the real case: a frame carrying no SDF scene never begins that pass, and
    /// reporting `0.0 ms` for it would be indistinguishable from an SDF pass
    /// that really cost nothing.
    #[test]
    fn a_pass_the_frame_never_recorded_is_absent_rather_than_zero() {
        // Shadow, main and post only — no SDF, no 2D.
        let recorded = 0b0_1011;
        let timing = GpuFrameTiming::resolved(&ticks(), recorded, 1.0, FrameIndex::new(7));
        assert_eq!(
            timing
                .passes()
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            vec!["shadow", "main", "post"]
        );
        // 10 + 20 + 40, with the unrecorded 30 and 50 contributing nothing.
        is_nanos(timing.total(), 70.0);
    }

    /// A frame that recorded nothing at all reports nothing at all — and is
    /// still "available", because the adapter *can* time passes; it simply had
    /// no pass to time.
    #[test]
    fn a_frame_with_no_recorded_pass_reports_no_pass() {
        let timing = GpuFrameTiming::resolved(&ticks(), 0, 1.0, FrameIndex::new(3));
        assert!(timing.is_available());
        assert!(timing.passes().is_empty());
        is_nanos(timing.total(), 0.0);
    }

    /// A short or inverted tick stream cannot produce a fabricated duration: a
    /// missing slot reads as zero and an end below its begin saturates to a zero
    /// span rather than wrapping to ~584 years.
    #[test]
    fn missing_and_inverted_ticks_saturate_to_a_zero_span() {
        let short = GpuFrameTiming::resolved(&[5, 9], ALL, 1.0, FrameIndex::ZERO);
        is_nanos(short.passes()[0].1, 4.0);
        is_nanos(short.passes()[1].1, 0.0);

        let inverted: [u64; PASS_COUNT * 2] = std::array::from_fn(|slot| [100, 1][slot % 2]);
        let timing = GpuFrameTiming::resolved(&inverted, ALL, 1.0, FrameIndex::ZERO);
        is_nanos(timing.total(), 0.0);
    }

    /// The unit a caller actually reads: a 19 ms shadow pass is 0.019 s.
    #[test]
    fn nanoseconds_convert_to_seconds() {
        is_nanos(nanos_to_seconds(0), 0.0);
        assert!((nanos_to_seconds(19_000_000).get() - 0.019).abs() < 1.0e-7);
    }
}
