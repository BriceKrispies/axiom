//! The shape of a flight: what the player drew, kept.
//!
//! # Why this replaced a two-parameter curve
//!
//! The shot used to be four numbers. A drawing was **fitted** onto them — a
//! least-squares solve for the two Bézier weights per projection nearest to what
//! the hand did — and the kicker played the result. It was compact, it could not
//! represent an illegal path, and it was the single biggest thing wrong with the
//! game.
//!
//! Because there was a translator in the loop. The player's model is *"I drew
//! that line"*; the game's model was *"you drew evidence for a shot"*. Every
//! wobble, every late flick, every bit of character in a hand-drawn curve was
//! averaged away into the nearest smooth thing, and what came back was never
//! quite the shot anyone meant. That gap is where a player's sense of having
//! *done* it leaks out — and a game about drawing a shot has nothing else to
//! sell.
//!
//! So the drawing is kept. A [`ShotPath`] is the flight's offset from the
//! straight line ball→target, **sampled** rather than parameterised: a fixed
//! number of `(across, up)` pairs in metres, evenly spaced along the shot. Draw a
//! wobble and the ball wobbles. Draw a late dip and the ball dips late, not at
//! the nearest place a cubic could put a dip.
//!
//! Two guarantees survive the change, and they are the two that matter:
//!
//! * **both ends are pinned to zero**, so the flight starts on the ball and
//!   finishes on the authored point — by construction, not by correction;
//! * every sample is **bounded**, so no drawing can ask for a shape a kicker
//!   could not strike.
//!
//! Everything else — where the curve peaks, how sharply it breaks, whether it
//! moves twice — is now the player's business rather than the model's.
//!
//! [`BendCurve`] did not die with the fit. It survives as a **generator**: the
//! shot matrix has to sweep a parameter space, the agent has to author a shot
//! without a screen, and a test has to be able to say "a shot that breaks late".
//! Those all want a compact description that produces a path. They just no longer
//! sit between the player and the ball.

use super::curve::BendCurve;

/// How many offsets a flight's shape is stored as.
///
/// The drawn line is decimated to about this many points anyway, so this is
/// roughly "keep what the hand gave us" — fine enough that a real flick survives,
/// coarse enough that a fingertip's jitter does not become geometry.
pub const SHAPE_SAMPLES: usize = 24;

/// The offsets a flight takes from its own straight line, in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotPath {
    across: [f32; SHAPE_SAMPLES],
    up: [f32; SHAPE_SAMPLES],
}

/// The shot progress one sample sits at.
fn at_index(i: usize) -> f32 {
    i as f32 / (SHAPE_SAMPLES - 1) as f32
}

/// Zero at both ends, whatever the samples said.
fn pinned(mut values: [f32; SHAPE_SAMPLES]) -> [f32; SHAPE_SAMPLES] {
    values[0] = 0.0;
    values[SHAPE_SAMPLES - 1] = 0.0;
    values
}

impl ShotPath {
    /// A shot with no shape on it at all: straight from the ball to the point.
    pub const STRAIGHT: ShotPath = ShotPath {
        across: [0.0; SHAPE_SAMPLES],
        up: [0.0; SHAPE_SAMPLES],
    };

    /// Build a shape by asking for the offsets at each sample's own progress.
    pub fn sampled(shape: impl Fn(f32) -> (f32, f32)) -> ShotPath {
        let mut across = [0.0f32; SHAPE_SAMPLES];
        let mut up = [0.0f32; SHAPE_SAMPLES];
        (0..SHAPE_SAMPLES).for_each(|i| {
            let (a, u) = shape(at_index(i));
            across[i] = a;
            up[i] = u;
        });
        ShotPath {
            across: pinned(across),
            up: pinned(up),
        }
    }

    /// The shape a pair of Bézier offsets describes — the authoring path for
    /// everything that does not have a hand: the matrix, the agent, the tests.
    pub fn from_curves(bend: BendCurve, loft: BendCurve) -> ShotPath {
        ShotPath::sampled(|u| (bend.offset(u), loft.offset(u)))
    }

    /// The offset at shot progress `u`, interpolated between samples.
    pub fn at(&self, u: f32) -> (f32, f32) {
        let scaled = u.clamp(0.0, 1.0) * (SHAPE_SAMPLES - 1) as f32;
        let i = (scaled.floor() as usize).min(SHAPE_SAMPLES - 1);
        let j = (i + 1).min(SHAPE_SAMPLES - 1);
        let t = scaled - i as f32;
        (
            self.across[i] + (self.across[j] - self.across[i]) * t,
            self.up[i] + (self.up[j] - self.up[i]) * t,
        )
    }

    /// Every sample, as `(progress, across, up)`.
    pub fn samples(&self) -> impl Iterator<Item = (f32, f32, f32)> + '_ {
        (0..SHAPE_SAMPLES).map(|i| (at_index(i), self.across[i], self.up[i]))
    }

    /// The largest offset each way, keeping its sign — how far the shot bends and
    /// how high it lifts, which is what the kicker's body and the shot's pace both
    /// read off the shape.
    pub fn reach(&self) -> (f32, f32) {
        (peak(&self.across), peak(&self.up))
    }

    /// Where along the shot each offset reaches its extreme — *where* the curve
    /// breaks and *where* the arc peaks, as shot progress.
    ///
    /// Read out of the samples rather than stored, which is the point: with a
    /// two-weight curve this was a property of the model, and now it is simply a
    /// question you can ask of the line somebody drew.
    pub fn peak_at(&self) -> (f32, f32) {
        let where_of = |values: &[f32; SHAPE_SAMPLES]| {
            values
                .iter()
                .enumerate()
                .fold((0.0f32, 0.0f32), |best, (i, v)| {
                    [best, (at_index(i), *v)][usize::from(v.abs() > best.1.abs())]
                })
                .0
        };
        (where_of(&self.across), where_of(&self.up))
    }

    /// Every sample brought inside what a kicker can strike.
    pub fn bounded(&self, bend: (f32, f32), loft: (f32, f32)) -> ShotPath {
        let mut out = *self;
        (0..SHAPE_SAMPLES).for_each(|i| {
            out.across[i] = self.across[i].clamp(bend.0, bend.1);
            out.up[i] = self.up[i].clamp(loft.0, loft.1);
        });
        out
    }

    /// The height offsets lifted so the flight never goes through the turf.
    pub fn floored(&self, floor: impl Fn(f32) -> f32) -> ShotPath {
        let mut out = *self;
        (0..SHAPE_SAMPLES).for_each(|i| {
            out.up[i] = self.up[i].max(-floor(at_index(i)));
        });
        ShotPath {
            up: pinned(out.up),
            ..out
        }
    }

    /// The same shape drawn the other way round the goal — the mirror the
    /// symmetry sweep compares a shot against.
    pub fn mirrored(&self) -> ShotPath {
        let mut out = *self;
        (0..SHAPE_SAMPLES).for_each(|i| out.across[i] = -self.across[i]);
        out
    }

    /// A three-tap smoothing of the samples.
    ///
    /// This is **not** the fit coming back. A fit replaced the drawing with the
    /// nearest member of a small family and threw the rest away; this removes the
    /// tremor of a fingertip on glass and keeps everything else. A hand shake is
    /// not a shot shape, and at 35 m/s a two-centimetre kink is a visible twitch
    /// in the flight.
    pub fn smoothed(&self) -> ShotPath {
        let tap = |v: &[f32; SHAPE_SAMPLES]| {
            let mut out = *v;
            (1..SHAPE_SAMPLES - 1)
                .for_each(|i| out[i] = 0.25 * v[i - 1] + 0.5 * v[i] + 0.25 * v[i + 1]);
            out
        };
        ShotPath {
            across: tap(&self.across),
            up: tap(&self.up),
        }
    }
}

impl Default for ShotPath {
    fn default() -> Self {
        ShotPath::STRAIGHT
    }
}

/// The value furthest from zero, sign kept.
fn peak(values: &[f32; SHAPE_SAMPLES]) -> f32 {
    values
        .iter()
        .copied()
        .fold(0.0f32, |best, v| [best, v][usize::from(v.abs() > best.abs())])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arc(size: f32) -> ShotPath {
        ShotPath::from_curves(BendCurve::STRAIGHT, BendCurve::through(0.5, size, 0.14))
    }

    #[test]
    fn a_shape_starts_and_finishes_on_nothing() {
        // The guarantee the whole trajectory rests on: whatever was drawn, the
        // flight leaves the ball and arrives at the authored point.
        [
            ShotPath::STRAIGHT,
            arc(2.0),
            ShotPath::sampled(|u| (9.0 * u, 4.0 - u)),
        ]
        .into_iter()
        .for_each(|path| {
            assert_eq!(path.at(0.0), (0.0, 0.0));
            assert_eq!(path.at(1.0), (0.0, 0.0));
        });
    }

    #[test]
    fn a_drawn_shape_is_kept_rather_than_fitted() {
        // The point of the module. A wobble a cubic could not represent survives
        // into the flight, sample for sample.
        let wobble = |u: f32| ((u * 24.0).sin() * 0.8, 0.0);
        let path = ShotPath::sampled(wobble);
        path.samples().skip(1).take(SHAPE_SAMPLES - 2).for_each(|(u, across, _)| {
            assert!(
                (across - wobble(u).0).abs() < 1.0e-5,
                "the drawing was altered at u={u}"
            );
        });
        // ... and it genuinely changes direction more than twice, which is more
        // than the old two-weight curve could do at all.
        let turns = path
            .samples()
            .map(|(_, a, _)| a)
            .collect::<Vec<_>>()
            .windows(3)
            .filter(|w| (w[1] - w[0]).signum() != (w[2] - w[1]).signum())
            .count();
        assert!(turns >= 3, "only {turns} changes of direction");
    }

    #[test]
    fn the_peak_is_read_out_of_the_line_rather_than_stored() {
        let early = ShotPath::from_curves(
            BendCurve::through(0.25, 2.0, 0.14),
            BendCurve::through(0.75, 1.0, 0.14),
        );
        let (bend_at, loft_at) = early.peak_at();
        // Against the curve's OWN peak rather than the argument that shaped it:
        // a two-weight cubic puts its extreme near where it was asked to, not
        // exactly there, and the question here is whether the samples find it.
        let want_bend = BendCurve::through(0.25, 2.0, 0.14).peak().0;
        let want_loft = BendCurve::through(0.75, 1.0, 0.14).peak().0;
        let a_sample = 1.0 / (SHAPE_SAMPLES - 1) as f32;
        assert!(
            (bend_at - want_bend).abs() <= a_sample,
            "the bend broke at {bend_at}, the curve breaks at {want_bend}"
        );
        assert!(
            (loft_at - want_loft).abs() <= a_sample,
            "the arc peaked at {loft_at}, the curve peaks at {want_loft}"
        );
        // The order is the thing that matters to a player: this one bends early
        // and lifts late.
        assert!(bend_at < loft_at);
    }

    #[test]
    fn the_reach_is_the_biggest_offset_either_way() {
        let (bend, lift) = arc(2.5).reach();
        assert_eq!(bend, 0.0, "a straight shot does not bend");
        assert!((lift - 2.5).abs() < 0.05, "peaked at {lift}");
        // Sign is kept, because which way it bends decides which side the kicker
        // strikes from.
        let (left, _) = ShotPath::from_curves(
            BendCurve::through(0.5, -3.0, 0.14),
            BendCurve::STRAIGHT,
        )
        .reach();
        assert!(left < -2.5, "bent to {left}");
        assert_eq!(ShotPath::STRAIGHT.reach(), (0.0, 0.0));
    }

    #[test]
    fn bounding_brings_every_sample_inside_what_a_kicker_can_strike() {
        let wild = ShotPath::sampled(|u| (40.0 * u - 20.0, 30.0 * u));
        let tamed = wild.bounded((-2.0, 2.0), (-1.5, 4.0));
        tamed.samples().for_each(|(u, across, up)| {
            assert!((-2.0..=2.0).contains(&across), "across {across} at {u}");
            assert!((-1.5..=4.0).contains(&up), "up {up} at {u}");
        });
    }

    #[test]
    fn flooring_lifts_a_shape_out_of_the_turf_without_moving_its_ends() {
        // A shot aimed low, dipped hard: the dip is lifted to the floor and the
        // ends stay exactly where they were.
        let dipped = ShotPath::sampled(|_| (0.0, -3.0));
        let safe = dipped.floored(|u| 0.4 + u);
        safe.samples().for_each(|(u, _, up)| {
            assert!(up >= -(0.4 + u) - 1.0e-5, "still {up} below the turf at {u}");
        });
        assert_eq!(safe.at(0.0), (0.0, 0.0));
        assert_eq!(safe.at(1.0), (0.0, 0.0));
    }

    #[test]
    fn a_mirrored_shape_bends_the_other_way_and_lifts_the_same() {
        let path = ShotPath::from_curves(
            BendCurve::through(0.3, 1.8, 0.14),
            BendCurve::through(0.7, 1.1, 0.14),
        );
        let other = path.mirrored();
        path.samples().zip(other.samples()).for_each(|(a, b)| {
            assert!((a.1 + b.1).abs() < 1.0e-6, "the bend did not mirror");
            assert!((a.2 - b.2).abs() < 1.0e-6, "the lift should not have moved");
        });
        assert_eq!(other.mirrored(), path);
    }

    #[test]
    fn smoothing_takes_the_tremor_out_and_leaves_the_shot_in() {
        let shaky = ShotPath::sampled(|u| {
            (
                u * 2.0 - 1.0 + [(-0.06f32), 0.06][usize::from((u * 40.0) as usize % 2 == 0)],
                0.0,
            )
        });
        let clean = shaky.smoothed();
        // The tremor is gone...
        let jitter = |p: &ShotPath| {
            p.samples()
                .map(|(_, a, _)| a)
                .collect::<Vec<_>>()
                .windows(3)
                .map(|w| (w[0] - 2.0 * w[1] + w[2]).abs())
                .fold(0.0f32, f32::max)
        };
        assert!(jitter(&clean) < jitter(&shaky) * 0.6, "it is still shaking");
        // ... and the shot is still the shot: the peak barely moved.
        assert!((clean.reach().0 - shaky.reach().0).abs() < 0.2);
        // The ends are still pinned.
        assert_eq!(clean.at(0.0), (0.0, 0.0));
        assert_eq!(clean.at(1.0), (0.0, 0.0));
    }

    #[test]
    fn a_shape_reads_the_same_everywhere_between_its_samples() {
        let path = arc(2.0);
        // Interpolation is monotonic between samples and lands exactly on them.
        path.samples().for_each(|(u, across, up)| {
            let (a, b) = path.at(u);
            assert!((a - across).abs() < 1.0e-5 && (b - up).abs() < 1.0e-5);
        });
        // Out of range clamps rather than extrapolating into nonsense.
        assert_eq!(path.at(-1.0), path.at(0.0));
        assert_eq!(path.at(2.0), path.at(1.0));
        assert_eq!(ShotPath::default(), ShotPath::STRAIGHT);
    }
}
