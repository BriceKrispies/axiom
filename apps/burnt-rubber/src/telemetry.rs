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

/// The commit this binary was built from — the first thing the panel says,
/// because it is what makes everything below it attributable.
///
/// Every other line here is a measurement of *some* build. Without this one you
/// cannot tell which, and in a repo developed across several worktrees served
/// side by side that is a real ambiguity, not a pedantic one: two ports, two
/// bundles, one browser, and no way to tell them apart by looking. A stale
/// `axiom-serve` bundle produces the same confusion from a single port.
///
/// Set by `build.rs`. Reads `<12-hex>`, `<12-hex>+dirty` when the tree carried
/// uncommitted changes, or `unknown` outside a git checkout — never a hash the
/// build did not actually come from.
pub const BUILD: &str = env!("BURNT_RUBBER_BUILD");

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

/// How many frames the window holds — about four seconds at 60 Hz.
///
/// Sixty was too short to say anything about stutter. A hitch that happens once
/// every couple of seconds appears in a one-second window as either nothing at
/// all or as the single worst sample, so the panel could only ever report "there
/// was one bad frame recently", never *how often*. Four seconds is long enough
/// for a rate to mean something and short enough to still respond while driving.
pub const WINDOW: usize = 240;

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
    ///
    /// **This number cannot see stutter, by construction.** A median is precisely
    /// the statistic that discards outliers, and stutter *is* the outliers: a
    /// window of 230 good frames and 10 catastrophic ones has exactly the same
    /// median as a window of 240 good ones, so this reports a confident, steady
    /// 60 through a game that is hitching badly. It answers "is the steady state
    /// fast enough", which is a real question, and NOT "does this feel smooth",
    /// which is the one a player is asking. Read it next to [`Self::low_fps`],
    /// never on its own.
    pub fn fps(&self) -> f32 {
        let median = self.median_ms();
        (median > 0.0).then(|| 1000.0 / median).unwrap_or(0.0)
    }

    /// The frame time at `fraction` through the sorted window (`0.5` is the
    /// median, `0.99` the worst percent). `0` before any frame.
    pub fn percentile_ms(&self, fraction: f32) -> f32 {
        let mut held: Vec<f32> = self.samples[..self.filled].to_vec();
        held.sort_by(f32::total_cmp);
        let last = held.len().saturating_sub(1);
        let at = ((held.len() as f32) * fraction.clamp(0.0, 1.0)) as usize;
        held.get(at.min(last)).copied().unwrap_or(0.0)
    }

    /// The **1% low**: the frame rate implied by the 99th-percentile frame time.
    ///
    /// The number that corresponds to what a player actually feels. A game whose
    /// median says 60 and whose 1% low says 12 is a game that stutters, and the
    /// gap between the two is the size of the problem. Reported alongside the
    /// median precisely so neither can be read alone.
    pub fn low_fps(&self) -> f32 {
        let slow = self.percentile_ms(0.99);
        (slow > 0.0).then(|| 1000.0 / slow).unwrap_or(0.0)
    }

    /// How many frames in the window **dropped** against a `budget_ms` frame
    /// budget, and how many frames the window holds — a *rate* of stutter rather
    /// than a single worst sample.
    ///
    /// "Dropped" is not "over the budget by any amount", and the difference is
    /// the whole usefulness of this number. On a vsync-locked display, frame
    /// deltas **quantise to multiples of the refresh period**: a frame that hits
    /// every scanout measures one period, a frame that misses one measures two,
    /// and nothing lands in between. What does land in between is *jitter* — a
    /// 60 Hz display is not exactly 60.000 Hz and `performance.now()` is coarse,
    /// so a perfect frame reads 16.7–16.8 ms against a 16.667 ms budget.
    ///
    /// Compared with `>` against the bare budget, that made a **flawless** run
    /// report stutter: measured on an unthrottled desktop, 138 of 399 frames
    /// counted as over budget in a window whose worst frame was 16.8 ms. The
    /// panel's warning colour was therefore always on, which is the same as
    /// having no warning colour.
    ///
    /// So the threshold sits at [`DROPPED_FRAME_FACTOR`] of the budget — halfway
    /// between one refresh period and two. Below it the frame hit its scanout;
    /// above it, it certainly missed one.
    pub fn over_budget(&self, budget_ms: f32) -> (usize, usize) {
        let threshold = budget_ms * DROPPED_FRAME_FACTOR;
        let count = self.samples[..self.filled]
            .iter()
            .filter(|ms| **ms > threshold)
            .count();
        (count, self.filled)
    }
}

/// How much longer than the frame budget a frame must take before it counts as
/// having dropped a scanout.
///
/// Halfway between one refresh period and two. See [`FrameTimes::over_budget`]
/// for why anything tighter counts jitter as stutter.
pub const DROPPED_FRAME_FACTOR: f32 = 1.5;

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
/// The candidates are the four systems that actually issue draws. `road_draws`
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

    /// **A flawless run must report zero dropped frames.** This is the case the
    /// bare `> budget` comparison got wrong: a real 60 Hz display delivers
    /// 16.7–16.8 ms against a 16.667 ms budget, so a perfect window counted as
    /// a third stutter and the panel's warning colour was permanently on.
    #[test]
    fn vsync_jitter_is_not_stutter() {
        const BUDGET: f32 = 1000.0 / 60.0;
        let mut t = FrameTimes::new();
        // The real distribution, sampled off an unthrottled desktop: every frame
        // hit its scanout, and every one of them measures a hair over 16.667.
        (0..WINDOW).for_each(|i| t.push([16.7, 16.8, 16.7, 16.75][i % 4]));
        let (over, of) = t.over_budget(BUDGET);
        assert_eq!(of, WINDOW);
        assert_eq!(over, 0, "a flawless 60 Hz run reported {over} dropped frames");
    }

    /// And a frame that genuinely missed a scanout is still counted — the
    /// tolerance must not be so wide that it hides real stutter.
    #[test]
    fn a_missed_scanout_is_still_counted() {
        const BUDGET: f32 = 1000.0 / 60.0;
        let mut t = FrameTimes::new();
        (0..WINDOW - 3).for_each(|_| t.push(16.7));
        // Two frames' worth, and four — one and three missed scanouts.
        t.push(33.4);
        t.push(66.8);
        t.push(50.0);
        let (over, _) = t.over_budget(BUDGET);
        assert_eq!(over, 3, "a dropped scanout went unreported");
        // The threshold sits between the two, so nothing at one period counts
        // and everything at two periods does.
        assert!(BUDGET * DROPPED_FRAME_FACTOR > 16.8);
        assert!(BUDGET * DROPPED_FRAME_FACTOR < 33.3);
    }

    /// A sustained slow run is *not* stutter, and the two readings say so
    /// differently: every frame is dropped, and the median agrees with the 1%
    /// low instead of contradicting it.
    #[test]
    fn a_uniformly_slow_run_reads_slow_on_both_numbers() {
        const BUDGET: f32 = 1000.0 / 60.0;
        let mut t = FrameTimes::new();
        (0..WINDOW).for_each(|_| t.push(50.0));
        assert!((t.fps() - 20.0).abs() < 0.1, "median fps {}", t.fps());
        assert!((t.low_fps() - 20.0).abs() < 0.1, "1% low {}", t.low_fps());
        assert_eq!(t.over_budget(BUDGET), (WINDOW, WINDOW));
    }

    /// The case the two numbers exist to tell apart: a *typical* frame that is
    /// fine and a tail that is not. The median cannot see it; the 1% low can.
    #[test]
    fn stutter_splits_the_median_from_the_one_percent_low() {
        const BUDGET: f32 = 1000.0 / 60.0;
        let mut t = FrameTimes::new();
        (0..WINDOW).for_each(|i| t.push([16.7, 50.0][usize::from(i % 100 == 0)]));
        assert!((t.fps() - 59.9).abs() < 0.5, "median fps {}", t.fps());
        assert!(t.low_fps() < 25.0, "1% low {}", t.low_fps());
        let (over, _) = t.over_budget(BUDGET);
        assert!((1..=4).contains(&over), "{over} dropped");
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
            road_draws: 14,
            total_road_draws: 93,
            road_triangles: 48_320,
            scenery_instances: 136,
            cached_scenery_chunks: 17,
            effect_instances: 28,
            traffic_slots: 9,
            pickup_bodies: 18,
        };
        let top = top_three(&counters);
        assert_eq!(top[0].label, "road");
        assert_eq!(top[0].unit, "tris");
        assert_eq!(top[1].label, "scenery");
        assert_eq!(top[2].label, "effects");
        assert!(top[0].count >= top[1].count && top[1].count >= top[2].count);
    }

    /// The build stamp is only worth showing if it is a real commit. This test
    /// is what stops it degrading into decoration: `build.rs` falling back to
    /// `unknown` inside a checkout would be invisible in the panel — the line
    /// would still render, just meaninglessly — and this is the only place that
    /// notices.
    #[test]
    fn the_build_stamp_names_a_real_commit() {
        assert!(!BUILD.is_empty());
        let hash = BUILD.trim_end_matches("+dirty");
        assert_eq!(hash.len(), 12, "a short commit hash: {BUILD}");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hex, not a fallback: {BUILD}"
        );
        // The repo builds these tests, so this is a checkout and the fallback
        // must not have fired.
        assert_ne!(BUILD, "unknown");
    }
}
