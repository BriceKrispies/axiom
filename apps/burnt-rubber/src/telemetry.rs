//! The in-game telemetry readout: is this frame slow, and what is in it.
//!
//! This exists because the profiling that matters cannot be done on the
//! developer's machine. A desktop renders this game with so much headroom that
//! vsync flattens every difference — measured, every section of the course
//! reports an identical 16.7 ms — while the phone it is actually played on has
//! no such slack. The only instrument that can answer "why is the tunnel slow"
//! is one that runs *on the device seeing it*.
//!
//! ## What it can and cannot say
//!
//! It reports **frame time**, which is measured, and **what the frame is made
//! of**, which is counted. It does **not** report per-system GPU milliseconds,
//! because nothing in this browser can measure that: WebGL2 has no timestamp
//! queries at all, and the WebGPU ones are not reliably available. A panel
//! showing "road: 4.2 ms" would be a number nobody computed, and a made-up
//! number is worse than no number — it would be trusted exactly as far as a real
//! one and would send the next investigation somewhere arbitrary.
//!
//! So the panel pairs a real frame time with real counts, and the reader draws
//! the conclusion: if the frame time rises where a count rises, that count is
//! the suspect. That is precisely how the tunnel question gets answered.

/// A rolling window of frame times, in milliseconds.
///
/// Median rather than mean: a single 200 ms hitch (a tab restore, a GC pause)
/// drags a mean far enough to hide the steady state, and the steady state is the
/// question. The worst is reported alongside precisely so a hitch is still
/// visible rather than smoothed away.
#[derive(Debug, Clone)]
pub struct FrameTimes {
    samples: Vec<f32>,
    next: usize,
    filled: usize,
}

/// How many frames the window holds — about a second at 60 Hz, so the readout
/// settles fast enough to be watched while driving.
pub const WINDOW: usize = 60;

impl FrameTimes {
    /// An empty window.
    pub fn new() -> FrameTimes {
        FrameTimes {
            samples: vec![0.0; WINDOW],
            next: 0,
            filled: 0,
        }
    }

    /// Record one frame's wall time.
    pub fn push(&mut self, ms: f32) {
        self.samples[self.next] = ms.max(0.0);
        self.next = (self.next + 1) % WINDOW;
        self.filled = (self.filled + 1).min(WINDOW);
    }

    /// The window's median frame time, or `0` before any frame.
    pub fn median_ms(&self) -> f32 {
        let mut held: Vec<f32> = self.samples[..self.filled].to_vec();
        held.sort_by(f32::total_cmp);
        held.get(held.len() / 2).copied().unwrap_or(0.0)
    }

    /// The window's worst frame, or `0` before any frame.
    pub fn worst_ms(&self) -> f32 {
        self.samples[..self.filled]
            .iter()
            .copied()
            .fold(0.0, f32::max)
    }

    /// Frames per second implied by the median. `0` before any frame.
    pub fn fps(&self) -> f32 {
        let median = self.median_ms();
        (median > 0.0).then(|| 1000.0 / median).unwrap_or(0.0)
    }
}

impl Default for FrameTimes {
    fn default() -> Self {
        FrameTimes::new()
    }
}

/// A triple-tap detector on one screen element.
///
/// Separate from the browser so the rule is testable: three taps, each within
/// [`TRIPLE_TAP_GAP_MS`] of the last, toggles. A slower third tap starts a new
/// count rather than completing an old one, which is what stops an accidental
/// double-tap plus an unrelated later tap from opening the panel mid-race.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TripleTap {
    taps: u32,
    last_ms: f64,
}

/// The most time allowed between taps of one triple (ms).
pub const TRIPLE_TAP_GAP_MS: f64 = 600.0;

impl TripleTap {
    /// A detector that has seen nothing.
    pub const fn new() -> TripleTap {
        TripleTap {
            taps: 0,
            last_ms: 0.0,
        }
    }

    /// Register a tap at `now_ms`. `true` when it completed a triple.
    pub fn tap(&mut self, now_ms: f64) -> bool {
        let continues = self.taps > 0 && (now_ms - self.last_ms) <= TRIPLE_TAP_GAP_MS;
        self.taps = [1, self.taps + 1][usize::from(continues)];
        self.last_ms = now_ms;
        let complete = self.taps >= 3;
        self.taps = [self.taps, 0][usize::from(complete)];
        complete
    }
}

/// One counted contributor to the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contributor {
    /// What it is, for the panel.
    pub label: &'static str,
    /// How many of it the frame drew.
    pub count: usize,
    /// What `count` counts — "tris", "props", "cars". Shown, so a reader is
    /// never left guessing whether a number is triangles or objects.
    pub unit: &'static str,
}

/// The three biggest contributors to this frame, largest first.
///
/// The candidates are the four systems that actually issue draws. `active_chunks`
/// is deliberately not among them: a chunk is a subdivision of the road, not a
/// separate consumer, and listing it meant the road was counted twice and pushed
/// a real system off the bottom of a three-line panel.
///
/// Ranked by count within each unit rather than across units, which is why the
/// unit is carried and printed: 48,000 triangles and 9 cars are not comparable
/// quantities, and pretending otherwise by summing them into one "cost" would be
/// the same invention as a fabricated millisecond.
pub fn top_three(counters: &crate::render::SceneCounters) -> [Contributor; 3] {
    let mut all = [
        Contributor {
            label: "road",
            count: counters.road_triangles,
            unit: "tris",
        },
        Contributor {
            label: "scenery",
            count: counters.scenery_instances,
            unit: "props",
        },
        Contributor {
            label: "effects",
            count: counters.effect_instances,
            unit: "fx",
        },
        Contributor {
            label: "traffic",
            count: counters.traffic_slots,
            unit: "cars",
        },
    ];
    all.sort_by(|a, b| b.count.cmp(&a.count));
    [all[0], all[1], all[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_ignores_a_single_hitch_and_the_worst_reports_it() {
        let mut t = FrameTimes::new();
        (0..40).for_each(|_| t.push(16.7));
        t.push(220.0);
        assert!((t.median_ms() - 16.7).abs() < 0.01, "{}", t.median_ms());
        assert!((t.worst_ms() - 220.0).abs() < 0.01);
        assert!((t.fps() - 59.88).abs() < 0.5, "{}", t.fps());
    }

    #[test]
    fn an_empty_window_reports_zero_rather_than_dividing_by_it() {
        let t = FrameTimes::new();
        assert_eq!(t.median_ms(), 0.0);
        assert_eq!(t.worst_ms(), 0.0);
        assert_eq!(t.fps(), 0.0);
    }

    #[test]
    fn the_window_rolls_rather_than_growing() {
        let mut t = FrameTimes::new();
        (0..WINDOW * 3).for_each(|_| t.push(10.0));
        t.push(99.0);
        assert_eq!(t.samples.len(), WINDOW);
        assert!((t.median_ms() - 10.0).abs() < 0.01);
    }

    #[test]
    fn three_quick_taps_toggle_and_the_count_then_restarts() {
        let mut tap = TripleTap::new();
        assert!(!tap.tap(0.0));
        assert!(!tap.tap(200.0));
        assert!(tap.tap(400.0), "the third quick tap toggles");
        // And the next tap begins a fresh triple rather than toggling again.
        assert!(!tap.tap(500.0));
    }

    #[test]
    fn a_slow_third_tap_starts_over_instead_of_completing() {
        let mut tap = TripleTap::new();
        assert!(!tap.tap(0.0));
        assert!(!tap.tap(200.0));
        assert!(
            !tap.tap(200.0 + TRIPLE_TAP_GAP_MS + 1.0),
            "too late to be part of that triple"
        );
        // It counted as the first of a new one, so two more complete it.
        assert!(!tap.tap(1000.0));
        assert!(tap.tap(1200.0));
    }

    #[test]
    fn the_panel_ranks_the_biggest_three_and_keeps_their_units() {
        let counters = crate::render::SceneCounters {
            active_chunks: 14,
            total_chunks: 93,
            road_triangles: 48_320,
            scenery_instances: 136,
            cached_scenery_chunks: 17,
            effect_instances: 28,
            traffic_slots: 9,
        };
        let top = top_three(&counters);
        assert_eq!(top[0].label, "road");
        assert_eq!(top[0].unit, "tris");
        assert_eq!(top[1].label, "scenery");
        assert_eq!(top[2].label, "effects");
        assert!(top[0].count >= top[1].count && top[1].count >= top[2].count);
    }
}
