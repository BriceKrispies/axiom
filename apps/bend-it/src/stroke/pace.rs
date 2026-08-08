//! How *fast* the line was drawn, and what that means for the ball.
//!
//! The shape of a drawing says where the ball goes. Its **tempo** says how hard
//! it was hit. Those are read separately and deliberately: a banana drawn slowly
//! and the same banana drawn fast are the same *shot*, taken at different pace.
//!
//! # Why this is a summary and not a replay
//!
//! The obvious thing — drive the ball from the drawing's velocity sample by
//! sample — produces exactly the artefact it looks like it would: the ball
//! dawdles wherever the hand hesitated and lurches wherever it hurried, in
//! mid-air, with nothing physical to explain it. A struck ball does not do that.
//!
//! So the timing is reduced to **two numbers**, and only two:
//!
//! * `speed` — how quickly the line was drawn overall, which sets the speed the
//!   ball LEAVES at, and through that the torque the hip puts into the swing.
//! * `easing` — whether the hand *sped up* or *slowed down* across the stroke,
//!   which sets how sharply the ball bleeds pace.
//!
//! Everything between those two summaries is discarded. The flight profile that
//! comes out is always the same *kind* of curve — leaves hot, decays smoothly,
//! never accelerates — and the drawing only chooses where in that family it sits.
//! That is the normalisation: the ball's velocity and acceleration follow the
//! hand's, without ever inheriting the hand's stutter.
//!
//! Timing is measured in **fixed simulation ticks**, not wall clock, so it is as
//! deterministic as everything else: the same swipe at the same tempo is the same
//! kick.

use crate::tuning::PaceTuning;

use super::line::Stroke;

/// The tempo of a drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pace {
    /// How fast the line was drawn, `0` (a careful crawl) to `1` (a flick).
    pub speed: f32,
    /// Whether the hand accelerated (`+1`) or decelerated (`-1`) through it.
    pub easing: f32,
}

impl Pace {
    /// An ordinary swipe: mid tempo, even throughout.
    pub const STEADY: Pace = Pace {
        speed: 0.5,
        easing: 0.0,
    };

    /// Read the tempo of a drawing.
    ///
    /// `short_edge` is the viewport's short side in the same pixels the stroke is
    /// measured in, so the reading is the same on a small phone and a large one
    /// — a swipe "across half the screen in a fifth of a second" is one gesture,
    /// not two different ones at two densities.
    pub fn read(stroke: &Stroke, short_edge: f32, tuning: &PaceTuning) -> Pace {
        let ticks = stroke.drawn_ticks();
        let hurried = (ticks == 0) | (stroke.len() < 3);
        match hurried {
            // A line delivered in a single tick has no tempo to read — it was
            // synthesised, or the hand crossed the screen faster than the game
            // samples. Either way the honest answer is "as fast as it goes".
            true => Pace {
                speed: [Pace::STEADY.speed, 1.0][usize::from(ticks == 0 && stroke.len() >= 3)],
                easing: 0.0,
            },
            false => Pace {
                speed: normalise(
                    stroke.length() / ticks as f32,
                    short_edge * tuning.reference,
                ),
                easing: easing_of(stroke),
            },
        }
    }

    /// How sharply the ball bleeds pace, given a base decay.
    ///
    /// A hand that **accelerated** through the line reads as a shot that keeps
    /// running, so it decays less; a hand that **trailed off** reads as one that
    /// dies, so it decays more. The result is clamped strictly above zero, which
    /// is the guarantee that matters: whatever was drawn, the ball's speed only
    /// ever falls. It cannot dawdle and then hurry.
    pub fn decay(&self, base: f32, tuning: &PaceTuning) -> f32 {
        (base * (1.0 - self.easing.clamp(-1.0, 1.0) * tuning.easing_gain))
            .clamp(tuning.min_decay, tuning.max_decay)
    }

    /// A one-line description, for the debug view.
    pub fn describe(&self) -> String {
        format!(
            "{} and {}",
            ["a crawl", "steady", "a flick"][band(self.speed, 0.34, 0.66)],
            ["trailing off", "even", "accelerating"][band(self.easing, -0.12, 0.12)],
        )
    }
}

impl Default for Pace {
    fn default() -> Self {
        Pace::STEADY
    }
}

/// Which of three bands a value falls in.
fn band(value: f32, low: f32, high: f32) -> usize {
    usize::from(value > low) + usize::from(value > high)
}

/// Pixels per tick, against the tempo a normal swipe runs at.
fn normalise(per_tick: f32, reference: f32) -> f32 {
    (per_tick / reference.max(1.0e-4)).clamp(0.0, 1.0)
}

/// How much faster the second half of the line was drawn than the first.
///
/// Split by **arc length**, not by sample count, so it measures the hand's
/// tempo rather than how the decimation happened to fall.
fn easing_of(stroke: &Stroke) -> f32 {
    let (points, ticks) = (stroke.points(), stroke.ticks());
    let total = stroke.length();
    let half = total * 0.5;
    // Walk to the halfway point, accumulating length and elapsed ticks.
    let (first_len, first_ticks) = points
        .windows(2)
        .zip(ticks.windows(2))
        .scan(0.0f32, |walked, (pair, stamps)| {
            let before = *walked;
            *walked += pair[1].subtract(pair[0]).length();
            Some((before, *walked, stamps[1] - stamps[0]))
        })
        .take_while(|(before, _, _)| *before < half)
        .fold((0.0f32, 0u64), |(len, spent), (before, after, gap)| {
            let used = (after.min(half) - before).max(0.0);
            let share = used / (after - before).max(1.0e-6);
            (len + used, spent + (gap as f32 * share) as u64)
        });
    let second_len = (total - first_len).max(0.0);
    let second_ticks = stroke.drawn_ticks().saturating_sub(first_ticks);
    let rate = |len: f32, ticks: u64| len / ticks.max(1) as f32;
    let (a, b) = (rate(first_len, first_ticks), rate(second_len, second_ticks));
    ((b - a) / (a + b).max(1.0e-4)).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;
    use axiom::prelude::Vec2;

    /// A straight line of `count` points, `gap` pixels apart, one point every
    /// `per_tick` ticks.
    fn drawn(count: usize, gap: f32, per_tick: u64) -> Stroke {
        Stroke::from_timed_points(
            (0..count).map(|i| Vec2::new(0.0, i as f32 * gap)).collect(),
            per_tick,
        )
    }

    #[test]
    fn a_faster_hand_reads_as_a_faster_shot() {
        let t = Tuning::DEFAULT.pace;
        let quick = Pace::read(&drawn(20, 30.0, 1), 390.0, &t);
        let slow = Pace::read(&drawn(20, 30.0, 6), 390.0, &t);
        assert!(
            quick.speed > slow.speed + 0.3,
            "quick {:.2} vs slow {:.2}",
            quick.speed,
            slow.speed
        );
        assert!(quick.speed <= 1.0 && slow.speed >= 0.0);
    }

    #[test]
    fn the_reading_is_the_same_gesture_on_any_size_of_screen() {
        // The same swipe on a screen twice as big is twice as many pixels in the
        // same time — and must read as the same shot.
        let t = Tuning::DEFAULT.pace;
        let small = Pace::read(&drawn(20, 20.0, 1), 390.0, &t);
        let large = Pace::read(&drawn(20, 40.0, 1), 780.0, &t);
        assert!((small.speed - large.speed).abs() < 0.02);
    }

    #[test]
    fn a_hand_that_speeds_up_and_one_that_trails_off_read_differently() {
        let t = Tuning::DEFAULT.pace;
        // Same points, but the gaps between ticks shrink (accelerating) or grow.
        let timed = |gaps: &[u64]| {
            let points: Vec<Vec2> = (0..gaps.len() + 1)
                .map(|i| Vec2::new(0.0, i as f32 * 24.0))
                .collect();
            let ticks = gaps
                .iter()
                .scan(0u64, |at, gap| {
                    *at += gap;
                    Some(*at)
                })
                .collect::<Vec<u64>>();
            let mut all = vec![0u64];
            all.extend(ticks);
            Stroke::from_timed_points(points, 1).with_ticks(all)
        };
        let speeding = Pace::read(&timed(&[6, 6, 5, 4, 3, 2, 1, 1]), 390.0, &t);
        let trailing = Pace::read(&timed(&[1, 1, 2, 3, 4, 5, 6, 6]), 390.0, &t);
        assert!(speeding.easing > 0.2, "sped up: {:.2}", speeding.easing);
        assert!(trailing.easing < -0.2, "trailed off: {:.2}", trailing.easing);
        // An even hand is neither.
        let even = Pace::read(&drawn(9, 24.0, 3), 390.0, &t);
        assert!(even.easing.abs() < 0.15, "even: {:.2}", even.easing);
    }

    #[test]
    fn the_ball_never_speeds_up_in_the_air_whatever_was_drawn() {
        // The guarantee the whole module exists for. Across every tempo the
        // reading can produce, the decay stays strictly positive — so the flight
        // profile is always a smooth fall in speed and never a stutter.
        let t = Tuning::DEFAULT;
        let base = t.flight.decel;
        for speed in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            for easing in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
                let pace = Pace { speed, easing };
                let decay = pace.decay(base, &t.pace);
                assert!(
                    decay >= t.pace.min_decay && decay <= t.pace.max_decay,
                    "speed {speed} easing {easing} decayed at {decay}"
                );
                assert!(decay > 0.0, "a positive decay is what keeps it physical");
            }
        }
    }

    #[test]
    fn a_hand_that_accelerated_produces_a_shot_that_keeps_running() {
        let t = Tuning::DEFAULT;
        let base = t.flight.decel;
        let sped_up = Pace {
            speed: 0.5,
            easing: 1.0,
        };
        let trailed = Pace {
            speed: 0.5,
            easing: -1.0,
        };
        assert!(
            sped_up.decay(base, &t.pace) < trailed.decay(base, &t.pace),
            "a shot drawn with an accelerating hand should hold its pace"
        );
    }

    #[test]
    fn a_line_with_no_tempo_to_read_still_answers() {
        let t = Tuning::DEFAULT.pace;
        assert_eq!(Pace::read(&Stroke::new(), 390.0, &t).easing, 0.0);
        // One point, or every point on the same tick: no duration, no reading.
        let instant = Stroke::from_timed_points(
            (0..8).map(|i| Vec2::new(0.0, i as f32 * 20.0)).collect(),
            0,
        );
        let pace = Pace::read(&instant, 390.0, &t);
        assert!(pace.speed.is_finite() && (0.0..=1.0).contains(&pace.speed));
        assert_eq!(Pace::default(), crate::stroke::Pace::STEADY);
        assert!(Pace::STEADY.describe().contains("steady"));
        assert!(Pace {
            speed: 0.9,
            easing: 0.5
        }
        .describe()
        .contains("flick"));
    }
}
