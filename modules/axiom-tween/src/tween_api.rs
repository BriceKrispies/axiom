//! The one behavioral facade: the tween table and the ease evaluator.

use crate::curve;
use crate::ids::{Ease, TweenId, TweenSample, TweenSpec, TweenValue};

/// One live tween: its handle, its spec, and how much presentation time has
/// elapsed against it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ActiveTween {
    id: TweenId,
    spec: TweenSpec,
    elapsed_nanos: u64,
}

impl ActiveTween {
    /// Normalized time in `[0, 1]`: elapsed over duration, clamped. A zero
    /// duration reads as fully elapsed (the `max(1)` avoids a divide-by-zero and
    /// the clamp pins it to 1).
    fn phase(&self) -> f32 {
        let raw = self.elapsed_nanos as f32 / self.spec.duration_nanos.max(1) as f32;
        raw.clamp(0.0, 1.0)
    }

    /// Whether the tween has reached or passed its duration.
    fn is_complete(&self) -> bool {
        self.elapsed_nanos >= self.spec.duration_nanos
    }

    /// The eased display value at the current phase.
    fn current(&self) -> TweenValue {
        let eased = curve::ease_unit(self.spec.ease, self.phase());
        let from = self.spec.from.get();
        let to = self.spec.to.get();
        TweenValue::new(from + (to - from) * eased)
    }

    /// This tween's sample for the current frame.
    fn sample(&self) -> TweenSample {
        TweenSample {
            id: self.id,
            value: self.current(),
            completed: self.is_complete(),
        }
    }
}

/// The tween module's single facade: a table of live tweens advanced on the
/// presentation clock.
/// Every output is display-only (§17.5) — a sampled value must never be read back
/// into a `sim` API.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TweenApi {
    tweens: Vec<ActiveTween>,
    next_raw: u64,
}

impl TweenApi {
    /// An empty tween table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start `spec`, returning its handle. The tween begins at phase 0 and is
    /// sampled from the next [`advance`](Self::advance).
    pub fn start(&mut self, spec: TweenSpec) -> TweenId {
        let id = TweenId::from_raw(self.next_raw);
        self.next_raw = self.next_raw.saturating_add(1);
        self.tweens.push(ActiveTween {
            id,
            spec,
            elapsed_nanos: 0,
        });
        id
    }

    /// Remove a tween, stopping further samples. Cancelling an unknown id is a
    /// no-op.
    pub fn cancel(&mut self, id: TweenId) {
        self.tweens.retain(|t| t.id != id);
    }

    /// Advance every live tween by `elapsed_nanos` and return a sample per tween
    /// (in start order). A tween that reaches its duration yields one `completed`
    /// sample on this call and is then dropped — so `completed` fires exactly
    /// once.
    pub fn advance(&mut self, elapsed_nanos: u64) -> Vec<TweenSample> {
        self.tweens
            .iter_mut()
            .for_each(|t| t.elapsed_nanos = t.elapsed_nanos.saturating_add(elapsed_nanos));
        let samples = self.tweens.iter().map(ActiveTween::sample).collect();
        self.tweens.retain(|t| !t.is_complete());
        samples
    }

    /// The current display value of `id`, or `None` if it is unknown or already
    /// completed/cancelled.
    pub fn value(&self, id: TweenId) -> Option<TweenValue> {
        self.tweens
            .iter()
            .find(|t| t.id == id)
            .map(ActiveTween::current)
    }

    /// Evaluate an ease `curve` directly at normalized time `t` (the phase as a
    /// [`TweenValue`]). Endpoints are exact; an overshooting curve returns a value
    /// outside `[0, 1]`.
    pub fn ease(curve: Ease, t: TweenValue) -> TweenValue {
        TweenValue::new(curve::ease_unit(curve, t.get()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(v: f32) -> TweenValue {
        TweenValue::new(v)
    }

    fn spec(from: f32, to: f32, duration_nanos: u64, ease: Ease) -> TweenSpec {
        TweenSpec {
            from: val(from),
            to: val(to),
            duration_nanos,
            ease,
        }
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn ease_endpoints_are_exact_for_every_curve() {
        let curves = [
            Ease::Linear,
            Ease::QuadIn,
            Ease::QuadOut,
            Ease::QuadInOut,
            Ease::CubicOut,
            Ease::ExpoOut,
            Ease::BackOut,
        ];
        for c in curves {
            assert_eq!(TweenApi::ease(c, val(0.0)).get(), 0.0, "{c:?} at 0");
            assert_eq!(TweenApi::ease(c, val(1.0)).get(), 1.0, "{c:?} at 1");
        }
    }

    #[test]
    fn ease_midpoints_match_their_curves() {
        let mid = |c| TweenApi::ease(c, val(0.5)).get();
        assert!(approx(mid(Ease::Linear), 0.5));
        assert!(approx(mid(Ease::QuadIn), 0.25));
        assert!(approx(mid(Ease::QuadOut), 0.75));
        assert!(approx(mid(Ease::QuadInOut), 0.5)); // t >= 0.5 takes the second arm
        assert!(approx(mid(Ease::CubicOut), 0.875));
        assert!(approx(mid(Ease::ExpoOut), 0.969_70));
        assert!(TweenApi::ease(Ease::BackOut, val(0.9)).get() > 1.0);
    }

    #[test]
    fn quad_in_out_takes_the_accelerating_arm_below_the_midpoint() {
        assert!(approx(TweenApi::ease(Ease::QuadInOut, val(0.25)).get(), 0.125));
    }

    #[test]
    fn advance_samples_then_completes_once_and_value_tracks_then_clears() {
        let mut api = TweenApi::new();
        let id = api.start(spec(0.0, 10.0, 100, Ease::Linear));
        let half = api.advance(50);
        assert_eq!(half.len(), 1);
        assert!(approx(half[0].value.get(), 5.0));
        assert!(!half[0].completed);
        assert!(approx(api.value(id).expect("still live").get(), 5.0));
        let done = api.advance(60);
        assert_eq!(done.len(), 1);
        assert!(approx(done[0].value.get(), 10.0));
        assert!(done[0].completed);
        assert_eq!(api.value(id), None);
        assert!(api.advance(10).is_empty());
    }

    #[test]
    fn advance_is_chunk_invariant() {
        let run = |chunks: &[u64]| {
            let mut api = TweenApi::new();
            api.start(spec(0.0, 1.0, 1000, Ease::CubicOut));
            chunks
                .iter()
                .filter_map(|&c| api.advance(c).first().map(|s| s.value.get()))
                .last()
                .unwrap_or(f32::NAN)
        };
        assert!(approx(run(&[600]), run(&[100, 100, 100, 100, 100, 100])));
    }

    #[test]
    fn zero_duration_completes_on_the_first_advance() {
        let mut api = TweenApi::new();
        api.start(spec(2.0, 8.0, 0, Ease::Linear));
        let samples = api.advance(1);
        assert_eq!(samples.len(), 1);
        assert!(approx(samples[0].value.get(), 8.0));
        assert!(samples[0].completed);
    }

    #[test]
    fn cancel_removes_a_tween_and_unknown_ids_are_noops() {
        let mut api = TweenApi::new();
        let a = api.start(spec(0.0, 1.0, 100, Ease::Linear));
        let b = api.start(spec(0.0, 1.0, 100, Ease::Linear));
        api.cancel(a);
        assert_eq!(api.value(a), None);
        assert!(api.value(b).is_some());
        api.cancel(a);
        assert!(api.value(b).is_some());
        assert_eq!(api.advance(10).len(), 1);
    }

    #[test]
    fn ids_are_distinct_and_carry_their_raw() {
        let mut api = TweenApi::new();
        let a = api.start(spec(0.0, 1.0, 100, Ease::Linear));
        let b = api.start(spec(0.0, 1.0, 100, Ease::Linear));
        assert_ne!(a, b);
        assert_eq!(a.raw(), 0);
        assert_eq!(b.raw(), 1);
    }
}
