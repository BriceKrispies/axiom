//! The drawn line itself: a screen-space polyline, captured as the finger moves.
//!
//! It is deliberately dumb. It decimates (a finger reports far more samples than
//! a shape needs), it can tell you how long it is, it can resample itself evenly,
//! and it can notice it was drawn backwards. It makes no judgement about what the
//! drawing *means* — that is [`super::interpret`]'s job, and keeping the two
//! apart is what lets the meaning be tested without a pointer anywhere near it.

use axiom::prelude::Vec2;

/// A drawn line, in physical surface pixels, with the tick each point landed on.
///
/// The two vectors are kept in lockstep by [`Stroke::push`] and by nothing else,
/// which is what lets every geometric method below stay a plain slice of points.
/// The timing is carried separately because the two are read for different
/// things and must not contaminate each other: **shape** comes from the geometry
/// alone (so a line drawn slowly and the same line drawn fast are the same shot),
/// and **pace** comes from the timing alone.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stroke {
    points: Vec<Vec2>,
    ticks: Vec<u64>,
}

impl Stroke {
    pub fn new() -> Stroke {
        Stroke {
            points: Vec::new(),
            ticks: Vec::new(),
        }
    }

    /// Build from points drawn one per tick — the ordinary tempo.
    pub fn from_points(points: Vec<Vec2>) -> Stroke {
        Stroke::from_timed_points(points, 1)
    }

    /// Build from points spaced `per_point` ticks apart: a hand moving at a
    /// chosen tempo. `0` collapses to one tick per point.
    pub fn from_timed_points(points: Vec<Vec2>, per_point: u64) -> Stroke {
        let step = per_point.max(1);
        let ticks = (0..points.len() as u64).map(|i| i * step).collect();
        Stroke { points, ticks }
    }

    /// The same points, on an explicit tick stamp each — a hand whose tempo
    /// varies through the stroke. Stamps beyond the point count are ignored, and
    /// missing ones fall back to one tick apart.
    pub fn with_ticks(mut self, ticks: Vec<u64>) -> Stroke {
        self.ticks = (0..self.points.len())
            .map(|i| ticks.get(i).copied().unwrap_or(i as u64))
            .collect();
        self
    }

    pub fn points(&self) -> &[Vec2] {
        &self.points
    }

    /// The tick each point landed on.
    pub fn ticks(&self) -> &[u64] {
        &self.ticks
    }

    /// How many ticks the hand was moving for.
    pub fn drawn_ticks(&self) -> u64 {
        self.ticks
            .first()
            .zip(self.ticks.last())
            .map(|(first, last)| last.saturating_sub(*first))
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.ticks.clear();
    }

    /// Add a point, unless it is closer than `spacing` to the last one.
    ///
    /// Decimating on the way in rather than on the way out matters: a finger on a
    /// 120 Hz screen emits hundreds of samples for one swipe, and a shape built
    /// from every one of them is mostly a record of how fast the hand was moving.
    pub fn push(&mut self, point: Vec2, tick: u64, spacing: f32) {
        let far_enough = self
            .points
            .last()
            .map(|last| point.subtract(*last).length() >= spacing.max(0.5))
            .unwrap_or(true);
        far_enough.then(|| {
            self.points.push(point);
            self.ticks.push(tick);
        });
    }

    /// Total drawn length, pixels.
    pub fn length(&self) -> f32 {
        self.points
            .windows(2)
            .map(|w| w[1].subtract(w[0]).length())
            .sum()
    }

    /// The straight-line span from first point to last, pixels.
    pub fn span(&self) -> f32 {
        self.points
            .first()
            .zip(self.points.last())
            .map(|(a, b)| b.subtract(*a).length())
            .unwrap_or(0.0)
    }

    /// The same stroke, guaranteed to run *toward* `goal`.
    ///
    /// People draw a shot from the ball outward, but not always — and a line
    /// drawn back toward yourself is still a clear picture of the shape you
    /// want. Flipping it is free, and refusing it would be the interface being
    /// pedantic about a detail the player is right to ignore.
    pub fn oriented(&self, goal: Vec2) -> Stroke {
        let reversed = self
            .points
            .first()
            .zip(self.points.last())
            .map(|(a, b)| a.subtract(goal).length() < b.subtract(goal).length())
            .unwrap_or(false);
        let mut points = self.points.clone();
        let mut ticks = self.ticks.clone();
        reversed.then(|| {
            points.reverse();
            // Time still runs forwards even when the hand ran backwards: the
            // gaps between points are re-laid in the order the line is now read,
            // so a reversed drawing keeps its tempo instead of inheriting a
            // decreasing clock.
            let gaps: Vec<u64> = ticks.windows(2).map(|w| w[1] - w[0]).rev().collect();
            ticks = gaps
                .iter()
                .scan(0u64, |at, gap| {
                    *at += gap;
                    Some(*at)
                })
                .collect();
            ticks.insert(0, 0);
        });
        Stroke { points, ticks }
    }

    /// `count` points spaced evenly along the drawn length.
    ///
    /// Evenly along the *line*, not along the samples: this is what makes a shape
    /// drawn slowly and the same shape drawn fast interpret identically, which is
    /// the promise that the same swipe always produces the same kick.
    pub fn resampled(&self, count: usize) -> Vec<Vec2> {
        let count = count.max(2);
        let total = self.length();
        let short = (self.points.len() < 2) | (total <= 1.0e-4);
        match short {
            true => vec![*self.points.first().unwrap_or(&Vec2::ZERO); count],
            false => (0..count)
                .map(|i| self.at_length(total * i as f32 / (count - 1) as f32))
                .collect(),
        }
    }

    /// The point `distance` pixels along the drawn line.
    fn at_length(&self, distance: f32) -> Vec2 {
        let mut walked = 0.0f32;
        for pair in self.points.windows(2) {
            let step = pair[1].subtract(pair[0]).length();
            if walked + step >= distance {
                let t = ((distance - walked) / step.max(1.0e-6)).clamp(0.0, 1.0);
                return pair[0].add(pair[1].subtract(pair[0]).mul_scalar(t));
            }
            walked += step;
        }
        *self.points.last().unwrap_or(&Vec2::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(points: &[(f32, f32)]) -> Stroke {
        Stroke::from_points(points.iter().map(|(x, y)| Vec2::new(*x, *y)).collect())
    }

    #[test]
    fn pushing_decimates_a_hand_that_reports_faster_than_it_moves() {
        let mut s = Stroke::new();
        s.push(Vec2::new(0.0, 0.0), 0, 10.0);
        s.push(Vec2::new(2.0, 0.0), 1, 10.0);
        s.push(Vec2::new(3.0, 0.0), 2, 10.0);
        assert_eq!(s.len(), 1, "a jittering finger adds nothing");
        s.push(Vec2::new(20.0, 0.0), 3, 10.0);
        assert_eq!(s.len(), 2);
        assert!(!s.is_empty());
        s.clear();
        assert!(s.is_empty());
        assert_eq!(Stroke::new().length(), 0.0);
        assert_eq!(Stroke::new().span(), 0.0);
        assert_eq!(Stroke::new().drawn_ticks(), 0);
    }

    #[test]
    fn length_measures_the_line_and_span_measures_the_shortcut() {
        // A right-angled path: 3 across then 4 down.
        let s = line(&[(0.0, 0.0), (3.0, 0.0), (3.0, 4.0)]);
        assert!((s.length() - 7.0).abs() < 1.0e-4);
        assert!((s.span() - 5.0).abs() < 1.0e-4);
    }

    #[test]
    fn a_point_carries_the_tick_it_landed_on() {
        let mut s = Stroke::new();
        s.push(Vec2::new(0.0, 0.0), 10, 1.0);
        s.push(Vec2::new(40.0, 0.0), 14, 1.0);
        s.push(Vec2::new(80.0, 0.0), 21, 1.0);
        assert_eq!(s.ticks(), &[10, 14, 21]);
        assert_eq!(s.points().len(), s.ticks().len(), "always in lockstep");
        assert_eq!(s.drawn_ticks(), 11, "measured from the first point, not zero");
        // An explicit tempo, and an explicit stamp list.
        assert_eq!(
            Stroke::from_timed_points(vec![Vec2::ZERO; 4], 3).ticks(),
            &[0, 3, 6, 9]
        );
        assert_eq!(Stroke::from_points(vec![Vec2::ZERO; 3]).ticks(), &[0, 1, 2]);
        // A zero tempo collapses to one tick per point rather than to no time.
        assert_eq!(
            Stroke::from_timed_points(vec![Vec2::ZERO; 3], 0).drawn_ticks(),
            2
        );
        // Short or missing stamp lists fall back rather than panicking.
        let stamped = Stroke::from_points(vec![Vec2::ZERO; 4]).with_ticks(vec![5, 9]);
        assert_eq!(stamped.ticks().len(), 4);
    }

    #[test]
    fn turning_a_line_around_keeps_its_tempo_running_forwards() {
        // Drawn goal-to-ball with the hand slowing down; read ball-to-goal, the
        // gaps must still increase in the order the line is now read.
        let points = vec![
            Vec2::new(0.0, 10.0),
            Vec2::new(0.0, 40.0),
            Vec2::new(0.0, 90.0),
            Vec2::new(0.0, 160.0),
        ];
        let drawn = Stroke::from_points(points).with_ticks(vec![0, 1, 3, 9]);
        let turned = drawn.oriented(Vec2::new(0.0, 0.0));
        assert_eq!(turned.points().first(), Some(&Vec2::new(0.0, 160.0)));
        let ticks = turned.ticks();
        assert!(
            ticks.windows(2).all(|w| w[1] > w[0]),
            "time must still run forwards: {ticks:?}"
        );
        assert_eq!(turned.drawn_ticks(), drawn.drawn_ticks());
    }

    #[test]
    fn a_line_drawn_backwards_is_turned_around() {
        let goal = Vec2::new(0.0, 0.0);
        let toward = line(&[(0.0, 100.0), (0.0, 10.0)]);
        assert_eq!(toward.oriented(goal), toward, "already pointing at the goal");
        let away = line(&[(0.0, 10.0), (0.0, 100.0)]);
        let fixed = away.oriented(goal);
        assert_eq!(fixed.points().first(), Some(&Vec2::new(0.0, 100.0)));
        assert_eq!(fixed.points().last(), Some(&Vec2::new(0.0, 10.0)));
        // An empty stroke has no direction to argue with.
        assert!(Stroke::new().oriented(goal).is_empty());
    }

    #[test]
    fn resampling_is_even_along_the_line_not_along_the_samples() {
        // The same shape, drawn with wildly uneven sample density.
        let sparse = line(&[(0.0, 0.0), (100.0, 0.0)]);
        let dense = line(&[
            (0.0, 0.0),
            (1.0, 0.0),
            (2.0, 0.0),
            (3.0, 0.0),
            (50.0, 0.0),
            (100.0, 0.0),
        ]);
        let a = sparse.resampled(5);
        let b = dense.resampled(5);
        a.iter().zip(b.iter()).for_each(|(p, q)| {
            assert!(p.subtract(*q).length() < 1.0e-3, "{p:?} vs {q:?}");
        });
        assert_eq!(a.len(), 5);
        assert_eq!(a[0], Vec2::ZERO);
        assert!((a[4].x - 100.0).abs() < 1.0e-3);
        assert!((a[2].x - 50.0).abs() < 1.0e-3);
    }

    #[test]
    fn a_degenerate_stroke_still_resamples_to_something_usable() {
        assert_eq!(Stroke::new().resampled(4), vec![Vec2::ZERO; 4]);
        let dot = line(&[(7.0, 9.0)]);
        assert_eq!(dot.resampled(3), vec![Vec2::new(7.0, 9.0); 3]);
        // A count below two is raised rather than producing an empty fit.
        assert_eq!(line(&[(0.0, 0.0), (10.0, 0.0)]).resampled(0).len(), 2);
    }
}
