//! Adaptive render resolution: how much of the device tier's render target a
//! frame actually renders, and the closed loop that picks it from measured frame
//! time.
//!
//! ## Why this exists
//!
//! [`crate::HostDeviceProfile`] resolves a render-target size once, from a tier
//! the app authors at startup. That is a *static* answer to a question whose
//! answer is not static: the same tier lands on a flagship phone with headroom to
//! spare and on a budget one that cannot hold 60 Hz, and the app has no way to
//! tell them apart. Authoring for the weaker device throws away image quality
//! everywhere; authoring for the stronger one drops frames on everything else.
//! A racing game's thin, receding geometry — lane markings, posts, rails — wants
//! supersampling badly enough that both answers are wrong.
//!
//! So the tier keeps deciding what the frame would *like* to render at, and this
//! decides what it can *afford* to. Fill cost is very nearly linear in pixels, so
//! resolution is the one quality dial that trades smoothly and immediately
//! against frame time — unlike dropping a light or a shadow, which is a visible
//! cliff.
//!
//! ## The loop has no clock
//!
//! [`RenderScaleController`] is handed each frame's measured duration and returns
//! a scale. It reads no clock, allocates nothing, and is a pure function of the
//! durations it has been shown — so a replay that feeds the same sequence gets
//! the same scales, and the whole policy is testable natively without a browser
//! or a GPU. Real time enters where it always does in this engine: at the one
//! platform edge that measures it.

use axiom_kernel::Ratio;

/// The scales the controller may select, coarsest first.
///
/// A ladder rather than a continuous value, for two reasons. Changing the render
/// scale reallocates the scene colour target, its depth buffer and the whole
/// bloom chain, which is far too expensive to do on a frame-by-frame gradient.
/// And a continuously-drifting resolution is *visible* — the frame softens and
/// sharpens as it hunts — where a small number of stops that are held for a while
/// reads as a stable image.
///
/// The rungs are spaced by roughly equal steps in **pixel count** (the thing that
/// costs), not in linear scale: each step down is about 25-30% fewer fragments,
/// which is enough to matter and small enough not to be a jolt.
const LADDER: [f32; 5] = [0.50, 0.62, 0.75, 0.87, 1.0];

/// How far over budget a frame must run to count as too slow, in percent.
const DROP_ABOVE_PCT: u64 = 108;

/// How far *under* budget a frame must run to count as comfortable, in percent.
///
/// **This number is not a taste setting, it is a stability condition**, and the
/// first version of this file got it wrong. Climbing a rung multiplies the
/// fragment count by the square of the rung ratio, and a fill-bound frame's cost
/// moves with it. So a frame that was just barely "comfortable" at rung *n*
/// becomes `RAISE_BELOW_PCT × ratio²` of budget at rung *n+1* — and if that lands
/// above [`DROP_ABOVE_PCT`], the loop immediately drops back and has built itself
/// a limit cycle.
///
/// At 78% it did exactly that on the two lowest rungs (0.50→0.62 is 1.54× the
/// pixels, so 0.78 × 1.54 = **1.20× budget**, well past the 1.08 drop line), which
/// is the worst possible place for it: those are the rungs a *struggling* device
/// settles on, so the phones this feature exists to help were the ones it put into
/// a climb-drop cycle roughly every 98 frames — and each transition reallocates
/// the render target and the whole bloom chain. That is a stutter the loop
/// manufactures on a device that would otherwise have been merely slow.
///
/// The bound is `DROP_ABOVE_PCT / max(ratio²)` = 108 / 1.54 = **70%**. This sits
/// under it with margin, and `the_ladder_cannot_build_a_limit_cycle` pins the
/// relationship against the ladder rather than against these numbers, so a future
/// re-spacing of the rungs cannot silently reintroduce the cycle.
const RAISE_BELOW_PCT: u64 = 62;

/// How many frames after any rung change before another may be considered.
///
/// Note the consequence for a cold start, which [`RenderScaleController::holding_floor`]
/// exists to answer: crossing the ladder costs roughly
/// `(LADDER.len() - 1) x (DROP_RUN + CHANGE_COOLDOWN)` frames, so a controller
/// that starts at full scale cannot defend a floor during its own descent.
///
/// A scale change is not free: it reallocates the scene colour target, its depth
/// buffer and the bloom chain, which on a mobile GPU is tens of milliseconds —
/// a visible hitch. The dead band above makes an oscillation impossible in
/// steady state; this bounds how often even *legitimate* changes can happen, so
/// a device wandering across a threshold cannot hitch its way through a race.
/// Ten seconds at 60 Hz.
const CHANGE_COOLDOWN: u32 = 600;

/// How many consecutive over-budget frames it takes to drop a rung.
///
/// Short: a frame that is too slow is a problem the player is feeling right now,
/// and dropping resolution is the cheapest way to stop feeling it.
const DROP_RUN: u32 = 8;

/// The slowest budget the controller will ever settle for: a 60 Hz frame.
///
/// The budget tracks the *display*, not a constant — see
/// [`RenderScaleController::observe`] — but it is clamped here so a device that
/// never once reaches its refresh cannot talk the loop into accepting whatever it
/// happens to be doing. Without this floor the budget would follow a struggling
/// device downward and the controller would stop pushing at 30 fps, having
/// declared 30 fps the target.
const SLOWEST_BUDGET_NANOS: u64 = 16_666_667;

/// The presentation intervals the loop knows how to chase, **fastest first**:
/// 240, 144, 120, 90 and 60 Hz.
///
/// A table of real refresh rates rather than the raw fastest frame seen, because
/// the raw minimum is not a robust estimator: `performance.now` jitter routinely
/// pairs a short interval with a long one, so a single 4 ms reading on a 120 Hz
/// panel would retarget the whole loop to 240 Hz and it would then spend
/// resolution chasing a rate the display cannot present.
const CANDIDATE_PERIODS_NANOS: [u64; 5] =
    [4_166_667, 6_944_444, 8_333_333, 11_111_111, 16_666_667];

/// The fastest budget the loop will chase — the first candidate.
const FASTEST_BUDGET_NANOS: u64 = CANDIDATE_PERIODS_NANOS[0];

/// How much over a candidate's period a frame may run and still count as having
/// met it, in percent. Presentation intervals are never exact.
const REFRESH_TOLERANCE_PCT: u64 = 110;

/// What share of a window must meet a candidate for it to be believed, as a
/// divisor: a quarter. A display genuinely presenting at 120 Hz meets 120 Hz on
/// nearly every frame; one that met it a handful of times by luck did not.
const REFRESH_QUORUM_DIVISOR: u32 = 4;

/// How many frames the refresh estimate is accumulated over. Long enough that
/// jitter cannot carry a quorum, short enough to follow a real change (a display
/// switching rate, a device leaving a thermal cap).
const REFRESH_WINDOW: u32 = 240;

/// How many consecutive comfortable frames it takes to climb one.
///
/// Deliberately an order of magnitude longer than [`DROP_RUN`]. Climbing is
/// speculative — it re-raises the cost that was just relieved — so it should only
/// happen once the headroom has clearly persisted, and it must never race the
/// drop. Together with the gap between the two thresholds below, this is what
/// stops the loop oscillating between two rungs forever.
const RAISE_RUN: u32 = 90;

/// One selected render scale: the fraction of the device tier's render size a
/// frame is rendered at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderScale(Ratio);

impl RenderScale {
    /// The full tier resolution — what a frame renders at before any adaptation.
    pub const FULL: RenderScale = RenderScale(Ratio::finite_or_zero(1.0));

    /// This scale as a ratio.
    pub const fn ratio(self) -> Ratio {
        self.0
    }

    /// Apply this scale to a render-target size, preserving aspect and never
    /// producing a zero axis (a zero-sized attachment is not a valid target).
    pub fn apply(self, width: u32, height: u32) -> (u32, u32) {
        let scale = self.0.get();
        let axis = |v: u32| (((v as f32) * scale) as u32).max(1);
        (axis(width), axis(height))
    }
}

/// The closed loop: measured frame durations in, a render scale out.
///
/// Branchless — the run counters advance by multiplication and the rung moves by
/// a clamped integer step, so there is no control flow to make one device's path
/// through this differ from another's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderScaleController {
    rung: u32,
    over_run: u32,
    under_run: u32,
    /// The presentation interval being defended. Held rather than recovered from
    /// the thresholds: dividing back out of an integer-scaled threshold loses a
    /// nanosecond, and a budget that reads one off its own input is a small lie
    /// in exactly the place a reader goes to check the loop's target.
    budget_nanos: u64,
    drop_above_nanos: u64,
    raise_below_nanos: u64,
    /// Per candidate refresh rate, how many frames of this window met it.
    met: [u32; CANDIDATE_PERIODS_NANOS.len()],
    /// Frames left before the refresh estimate is recomputed.
    window_left: u32,
    /// Frames left before another rung change may be considered. A change costs
    /// a render-target reallocation, so they are rate-limited on top of the
    /// dead band — see [`CHANGE_COOLDOWN`].
    cooldown_left: u32,
}

impl RenderScaleController {
    /// A controller targeting `frame_budget_nanos` per frame, starting at full
    /// tier resolution.
    ///
    /// The two thresholds straddle the budget with a deliberate gap: a frame is
    /// "too slow" only past 1.08x the budget and "comfortable" only below 0.78x.
    /// Anything between is left alone. Without that dead band the loop would drop
    /// a rung, find itself just under budget, climb it back, and repeat forever —
    /// the resolution visibly breathing at the exact frame rate it was asked to
    /// hold steady.
    pub fn new(frame_budget_nanos: u64) -> RenderScaleController {
        let mut c = RenderScaleController {
            rung: (LADDER.len() - 1) as u32,
            over_run: 0,
            under_run: 0,
            budget_nanos: 0,
            drop_above_nanos: 0,
            raise_below_nanos: 0,
            met: [0; CANDIDATE_PERIODS_NANOS.len()],
            window_left: REFRESH_WINDOW,
            cooldown_left: 0,
        };
        c.retarget(frame_budget_nanos);
        c
    }

    /// A controller that defends whatever refresh rate the display turns out to
    /// offer, starting from the 60 Hz assumption until it has evidence.
    ///
    /// This is the constructor an app should use. The alternative — handing it
    /// the simulation's fixed step — bakes in the assumption that the display
    /// refreshes at the tick rate, which on a 120 Hz phone means the loop cheerfully
    /// holds 16 ms frames and calls it a success while the panel is asking for 8.
    /// A fixed simulation step and a display refresh are two different clocks and
    /// only one of them is a frame budget.
    pub fn for_display() -> RenderScaleController {
        RenderScaleController::new(SLOWEST_BUDGET_NANOS)
    }

    /// A controller that starts at the **coarsest** rung and climbs only once the
    /// device has proved it has headroom.
    ///
    /// [`Self::new`] and [`Self::for_display`] start at full scale and descend,
    /// which is right when the goal is "look as good as this device allows": a
    /// capable machine never pays for a probe it did not need, and a slow one
    /// gives up a little quality after a moment.
    ///
    /// It is the wrong shape when the goal is a FLOOR the frame rate may never go
    /// under, because the descent is not free. Each step waits [`DROP_RUN`]
    /// consecutive slow frames and then [`CHANGE_COOLDOWN`] before the next may be
    /// considered, so crossing the whole ladder takes on the order of
    /// `4 x (DROP_RUN + CHANGE_COOLDOWN)` frames. Measured on a fill-bound app
    /// that needs the bottom rung, that was **about a minute of play spent under
    /// the target** — and no budget value can fix it, because the cost is in the
    /// starting position rather than in the threshold.
    ///
    /// This inverts the risk. The first frame is already at the rung a struggling
    /// device would have taken a minute to reach, and quality is recovered
    /// upward on evidence: [`RAISE_RUN`] comfortable frames per step, which is an
    /// order of magnitude longer than a drop precisely because climbing is
    /// speculative. A device with headroom still reaches full scale; it simply
    /// arrives from below.
    ///
    /// Use this when a minimum frame rate is a requirement rather than a
    /// preference. Use [`Self::for_display`] otherwise — starting coarse on a
    /// capable device spends image quality it never needed to spend.
    pub fn holding_floor(frame_budget_nanos: u64) -> RenderScaleController {
        let mut c = RenderScaleController::new(frame_budget_nanos);
        c.rung = 0;
        c
    }

    /// Point the thresholds at a new budget.
    fn retarget(&mut self, budget_nanos: u64) {
        let budget = budget_nanos
            .max(FASTEST_BUDGET_NANOS)
            .min(SLOWEST_BUDGET_NANOS);
        self.budget_nanos = budget;
        self.drop_above_nanos = budget.saturating_mul(DROP_ABOVE_PCT) / 100;
        self.raise_below_nanos = budget.saturating_mul(RAISE_BELOW_PCT) / 100;
    }

    /// The frame budget currently being defended, in nanoseconds.
    pub const fn budget_nanos(&self) -> u64 {
        self.budget_nanos
    }

    /// The scale currently selected.
    pub fn scale(&self) -> RenderScale {
        RenderScale(Ratio::finite_or_zero(
            LADDER[(self.rung as usize).min(LADDER.len() - 1)],
        ))
    }

    /// Fold one frame's measured duration in and return the scale to render the
    /// next frame at. Equal to [`Self::scale`] on every frame that does not move
    /// a rung, which is nearly all of them.
    pub fn observe(&mut self, frame_nanos: u64) -> RenderScale {
        // Tally which presentation intervals this frame met, and once a window is
        // full, believe the fastest one a quorum of frames actually reached. A
        // device that can present at 120 Hz proves it by delivering 8 ms frames
        // over and over; one that never does is never asked to. The clamp in
        // `retarget` keeps a struggling device from redefining its own target
        // downward, so this can only ever ask for MORE than 60 Hz, never less.
        self.met
            .iter_mut()
            .zip(CANDIDATE_PERIODS_NANOS)
            .for_each(|(hits, period)| {
                *hits += u32::from(frame_nanos <= period * REFRESH_TOLERANCE_PCT / 100);
            });
        self.window_left = self.window_left.saturating_sub(1);
        // Zero-or-one retarget, over the Option iterator — no branch.
        (self.window_left == 0).then_some(()).into_iter().for_each(|()| {
            let quorum = REFRESH_WINDOW / REFRESH_QUORUM_DIVISOR;
            let believed = self
                .met
                .iter()
                .position(|hits| *hits >= quorum)
                .map_or(SLOWEST_BUDGET_NANOS, |i| CANDIDATE_PERIODS_NANOS[i]);
            self.retarget(believed);
            self.met = [0; CANDIDATE_PERIODS_NANOS.len()];
            self.window_left = REFRESH_WINDOW;
        });

        let over = u32::from(frame_nanos > self.drop_above_nanos);
        let under = u32::from(frame_nanos < self.raise_below_nanos);
        // Each run counts consecutive frames on its own side of the dead band:
        // multiplying by the flag both advances it and resets it to zero the
        // moment the condition lapses.
        self.over_run = (self.over_run + 1) * over;
        self.under_run = (self.under_run + 1) * under;

        // A change is only *considered* once the previous one has settled. The
        // cooldown gates the decision rather than the runs, so a device that is
        // genuinely too slow keeps accumulating evidence while it waits and acts
        // the moment it is allowed to.
        self.cooldown_left = self.cooldown_left.saturating_sub(1);
        let ready = u32::from(self.cooldown_left == 0);
        let drop = u32::from(self.over_run >= DROP_RUN) * ready;
        let raise = u32::from(self.under_run >= RAISE_RUN) * ready;
        // At most one rung per frame, clamped at both ends of the ladder. The two
        // can never both fire: `drop_above > raise_below`, so a single duration
        // cannot satisfy both conditions, and a run cannot be full on both sides.
        let down = drop.min(self.rung);
        let up = raise.min((LADDER.len() as u32 - 1).saturating_sub(self.rung));
        self.rung = self.rung + up - down;

        // Spending a run consumes it, so the next move needs a fresh one — a rung
        // change has to be given time to show up in the measurements before the
        // loop is allowed to act again.
        self.over_run *= 1 - drop;
        self.under_run *= 1 - raise;
        // Any actual movement re-arms the cooldown (table pick, no branch).
        let moved = drop | raise;
        self.cooldown_left = [self.cooldown_left, CHANGE_COOLDOWN][moved as usize];
        self.scale()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 60 Hz, in nanoseconds — the budget every case below is written against.
    const BUDGET: u64 = 16_666_667;

    fn controller() -> RenderScaleController {
        RenderScaleController::new(BUDGET)
    }

    /// Frames to feed for one rung change to be *considered* and then settle:
    /// the run itself plus the post-change cooldown.
    const PER_CHANGE: u32 = DROP_RUN + RAISE_RUN + CHANGE_COOLDOWN + 1;

    fn feed(c: &mut RenderScaleController, nanos: u64, frames: u32) {
        (0..frames).for_each(|_| {
            c.observe(nanos);
        });
    }

    #[test]
    fn a_fresh_controller_renders_at_full_tier_resolution() {
        let c = controller();
        assert_eq!(c.scale(), RenderScale::FULL);
        assert_eq!(c.scale().ratio().get(), 1.0);
    }

    #[test]
    fn a_scale_applies_to_both_axes_and_never_produces_a_zero_target() {
        let half = RenderScale(Ratio::finite_or_zero(0.5));
        assert_eq!(half.apply(1889, 4096), (944, 2048));
        // A degenerate target still yields a usable attachment.
        let tiny = RenderScale(Ratio::finite_or_zero(0.01));
        assert_eq!(tiny.apply(1, 1), (1, 1));
        assert_eq!(RenderScale::FULL.apply(800, 600), (800, 600));
    }

    #[test]
    fn a_sustained_slow_frame_drops_exactly_one_rung_per_run() {
        let mut c = controller();
        let slow = BUDGET * 2;
        // The run has to complete before anything moves.
        (0..DROP_RUN - 1).for_each(|_| {
            assert_eq!(c.observe(slow), RenderScale::FULL);
        });
        let dropped = c.observe(slow);
        assert!(dropped.ratio().get() < 1.0, "one rung down");
        assert_eq!(dropped.ratio().get(), LADDER[3]);
        // And the run is consumed: the very next slow frame does not drop again.
        assert_eq!(c.observe(slow).ratio().get(), LADDER[3]);
    }

    #[test]
    fn sustained_slowness_walks_down_to_the_floor_and_stops_there() {
        let mut c = controller();
        let slow = BUDGET * 4;
        feed(&mut c, slow, PER_CHANGE * LADDER.len() as u32);
        assert_eq!(c.scale().ratio().get(), LADDER[0], "pinned at the floor");
        // The floor holds — the rung cannot underflow.
        feed(&mut c, slow, PER_CHANGE * 2);
        assert_eq!(c.scale().ratio().get(), LADDER[0]);
    }

    #[test]
    fn holding_floor_starts_at_the_coarsest_rung_so_frame_one_is_already_safe() {
        let c = RenderScaleController::holding_floor(BUDGET);
        assert_eq!(
            c.scale().ratio().get(),
            LADDER[0],
            "a floor-holding controller must not spend its descent under the target"
        );
        // The distinction that matters: the optimistic constructors start at the
        // top, which is what costs a slow device its first seconds of play.
        assert_eq!(RenderScaleController::new(BUDGET).scale(), RenderScale::FULL);
        assert_eq!(RenderScaleController::for_display().scale(), RenderScale::FULL);
    }

    #[test]
    fn holding_floor_still_climbs_when_the_device_proves_it_has_headroom() {
        let mut c = RenderScaleController::holding_floor(BUDGET);
        assert_eq!(c.scale().ratio().get(), LADDER[0]);
        // `FASTEST_BUDGET_NANOS / 4`, not `BUDGET / 4`, for the reason
        // `sustained_headroom_climbs_to_the_ceiling_and_stops_there` already
        // records: frames merely fast against the CURRENT budget make the loop
        // retarget itself to a higher refresh, after which they are no longer
        // comfortable and the climb stalls part-way. Written with `BUDGET / 4`
        // first, this test stopped at rung 0.62 for exactly that reason.
        feed(&mut c, FASTEST_BUDGET_NANOS / 4, PER_CHANGE * LADDER.len() as u32);
        assert_eq!(
            c.scale(),
            RenderScale::FULL,
            "a capable device still reaches full scale; it just arrives from below"
        );
    }

    #[test]
    fn holding_floor_and_new_agree_on_everything_except_where_they_start() {
        // The pessimistic start must not smuggle in a different budget or a
        // different set of thresholds — only a different opening rung.
        let floor = RenderScaleController::holding_floor(BUDGET);
        let optimistic = RenderScaleController::new(BUDGET);
        assert_eq!(floor.budget_nanos(), optimistic.budget_nanos());
        // Fed identical slow frames, the pessimistic one is already where the
        // optimistic one is heading.
        let mut a = RenderScaleController::holding_floor(BUDGET);
        let mut b = RenderScaleController::new(BUDGET);
        feed(&mut a, BUDGET * 4, PER_CHANGE * LADDER.len() as u32);
        feed(&mut b, BUDGET * 4, PER_CHANGE * LADDER.len() as u32);
        assert_eq!(a.scale(), b.scale(), "they converge on the same rung");
    }

    #[test]
    fn a_frame_inside_the_dead_band_moves_nothing_in_either_direction() {
        let mut c = controller();
        // Just over the raise threshold and just under the drop one: neither run
        // ever advances, so the ladder never moves however long it goes on.
        let comfortable = BUDGET;
        (0..RAISE_RUN * 3).for_each(|_| {
            assert_eq!(c.observe(comfortable), RenderScale::FULL);
        });
    }

    #[test]
    fn headroom_climbs_back_but_only_after_a_much_longer_run() {
        let mut c = controller();
        feed(&mut c, BUDGET * 2, DROP_RUN);
        let dropped = c.scale().ratio().get();
        assert!(dropped < 1.0);

        // Comfortably inside even the fastest budget the loop will chase, so the
        // climb is not blocked by the refresh retargeting itself.
        let fast = FASTEST_BUDGET_NANOS / 4;
        // A drop-length run of fast frames is nowhere near enough to climb.
        feed(&mut c, fast, DROP_RUN * 2);
        assert_eq!(c.scale().ratio().get(), dropped, "climbing is not symmetric");

        // Nor is a full raise run on its own — the cooldown after the drop has
        // to expire first, which is the whole point of rate-limiting a change
        // that costs a render-target reallocation.
        feed(&mut c, fast, RAISE_RUN);
        assert_eq!(c.scale().ratio().get(), dropped, "the cooldown still holds");

        feed(&mut c, fast, PER_CHANGE);
        assert!(c.scale().ratio().get() > dropped);
    }

    #[test]
    fn sustained_headroom_climbs_to_the_ceiling_and_stops_there() {
        let mut c = controller();
        feed(&mut c, BUDGET * 4, PER_CHANGE * LADDER.len() as u32);
        assert_eq!(c.scale().ratio().get(), LADDER[0]);
        // Comfortably inside even the fastest budget the loop will ever chase, so
        // the climb survives the loop retargeting itself to a higher refresh.
        let very_fast = FASTEST_BUDGET_NANOS / 4;
        feed(&mut c, very_fast, PER_CHANGE * LADDER.len() as u32);
        assert_eq!(c.scale(), RenderScale::FULL, "pinned at the ceiling");
        // The ceiling holds — the rung cannot overflow past the ladder.
        feed(&mut c, very_fast, PER_CHANGE * 2);
        assert_eq!(c.scale(), RenderScale::FULL);
    }

    /// **The 120 Hz contract.** A display that keeps presenting every 8.3 ms is
    /// believed, and the loop then defends 120 fps rather than the 60 it started
    /// assuming. Without this the controller holds 16 ms frames on a 120 Hz panel
    /// and calls that a success.
    #[test]
    fn a_display_that_presents_at_120_retargets_the_budget_to_120() {
        let mut c = RenderScaleController::for_display();
        assert_eq!(c.budget_nanos(), SLOWEST_BUDGET_NANOS, "starts at 60 Hz");
        let at_120 = 8_333_333;
        (0..REFRESH_WINDOW).for_each(|_| {
            c.observe(at_120);
        });
        assert_eq!(
            c.budget_nanos(),
            8_333_333,
            "the loop now defends the rate the display proved it can present"
        );
    }

    /// The robustness the candidate table exists for: timer jitter pairs a short
    /// interval with a long one, and a raw fastest-seen estimator would take the
    /// short one as gospel and start chasing a rate the panel cannot present.
    #[test]
    fn a_handful_of_freakishly_fast_frames_does_not_retarget_the_loop() {
        let mut c = RenderScaleController::for_display();
        let at_60 = 16_666_667;
        (0..REFRESH_WINDOW).for_each(|i| {
            // One frame in twenty reads absurdly short — far below any real
            // refresh — and the rest are honest 60 Hz frames.
            c.observe([at_60, 1_000_000][usize::from(i % 20 == 0)]);
        });
        assert_eq!(
            c.budget_nanos(),
            SLOWEST_BUDGET_NANOS,
            "a 5% minority cannot carry the quorum"
        );
    }

    /// The floor that stops a struggling device redefining success. A phone stuck
    /// at 20 fps must keep being pushed toward 60, not have 20 declared the target.
    #[test]
    fn a_device_that_never_reaches_60_still_has_60_as_its_target() {
        let mut c = RenderScaleController::for_display();
        feed(&mut c, 50_000_000, PER_CHANGE * LADDER.len() as u32);
        assert_eq!(c.budget_nanos(), SLOWEST_BUDGET_NANOS);
        assert_eq!(c.scale().ratio().get(), LADDER[0], "and it is still pushing");
    }

    /// The oscillation guard, stated as a property: a frame time that sits
    /// *between* the two thresholds is stable, so a device that lands there after
    /// a drop stays put instead of climbing back into the drop.
    #[test]
    fn a_run_broken_before_it_completes_starts_over_rather_than_accumulating() {
        let mut c = controller();
        let slow = BUDGET * 2;
        (0..DROP_RUN - 1).for_each(|_| {
            c.observe(slow);
        });
        // One comfortable frame breaks the run.
        c.observe(BUDGET);
        (0..DROP_RUN - 1).for_each(|_| {
            c.observe(slow);
        });
        assert_eq!(
            c.scale(),
            RenderScale::FULL,
            "two partial runs must not add up to a drop"
        );
    }

    /// **The invariant the first version of this file violated**, and the reason
    /// a phone reported a steady 60 fps median with 80 ms worst frames and stuttered
    /// badly: the loop was manufacturing the stutter.
    ///
    /// Climbing a rung multiplies the fragment count by the square of the rung
    /// ratio, so a fill-bound frame that was just barely comfortable at rung *n*
    /// costs `RAISE_BELOW_PCT × ratio²` at rung *n+1*. If that exceeds
    /// [`DROP_ABOVE_PCT`] the loop drops straight back, climbs again, and cycles —
    /// reallocating the render target and the whole bloom chain every time.
    ///
    /// Pinned against the LADDER rather than against the constants, so re-spacing
    /// the rungs closer together (which is the tempting way to make the scale
    /// changes less visible) cannot silently reintroduce the cycle.
    #[test]
    fn the_ladder_cannot_build_a_limit_cycle() {
        LADDER.windows(2).for_each(|pair| {
            let (from, to) = (pair[0], pair[1]);
            let ratio = (to / from) * (to / from);
            let after_climbing = (RAISE_BELOW_PCT as f32) * ratio;
            // Every value the message needs is bound here rather than passed as a
            // trailing argument: a trailing argument is evaluated only when the
            // assertion fails, which leaves an uncovered region on the path this
            // test is supposed to take.
            assert!(
                after_climbing < DROP_ABOVE_PCT as f32,
                "climbing {from:.2} -> {to:.2} multiplies the fragments by \
                 {ratio:.2}, so a frame at the {RAISE_BELOW_PCT}% raise line lands \
                 at {after_climbing:.0}% of budget — past the {DROP_ABOVE_PCT}% \
                 drop line. The loop would drop straight back and cycle, \
                 reallocating the render target each time."
            );
        });
    }

    /// The same property end-to-end: a fill-bound device is simulated (frame cost
    /// moves with the pixel count) and the loop must come to rest, not oscillate.
    #[test]
    fn a_fill_bound_device_settles_instead_of_oscillating() {
        let mut c = controller();
        // Cost at full scale: 1.9x budget. The device can afford some rung.
        let cost_at_full = (BUDGET as f32) * 1.9;
        let cost = |scale: f32| (cost_at_full * scale * scale) as u64;

        let mut changes = 0;
        let mut last = c.scale().ratio().get();
        (0..PER_CHANGE * 12).for_each(|_| {
            let now = c.observe(cost(last)).ratio().get();
            changes += u32::from(now != last);
            last = now;
        });
        // It must have moved (it could not hold full scale) and then stopped.
        assert!(changes > 0, "the loop never adapted at all");
        assert!(
            changes < LADDER.len() as u32,
            "the loop changed rung {changes} times over the run — it is oscillating,              and every change reallocates the render target"
        );

        // ...and from here it is stable: no further change, however long it runs.
        let settled = last;
        (0..PER_CHANGE * 6).for_each(|_| {
            assert_eq!(
                c.observe(cost(settled)).ratio().get(),
                settled,
                "a settled fill-bound device must never move again"
            );
        });
    }

    #[test]
    fn the_thresholds_straddle_the_budget_with_a_real_dead_band() {
        let c = controller();
        assert!(c.drop_above_nanos > BUDGET, "slow means slower than budget");
        assert!(c.raise_below_nanos < BUDGET, "comfortable means faster");
        assert!(
            c.raise_below_nanos < c.drop_above_nanos,
            "no duration may be both"
        );
    }

    /// A nonsense budget is clamped into the band of real refresh rates rather
    /// than producing a degenerate threshold (a zero budget would otherwise make
    /// every frame "too slow" forever, including on hardware with headroom).
    #[test]
    fn an_out_of_band_budget_is_clamped_to_a_real_refresh_rate() {
        assert_eq!(
            RenderScaleController::new(0).budget_nanos(),
            FASTEST_BUDGET_NANOS
        );
        assert_eq!(
            RenderScaleController::new(u64::MAX).budget_nanos(),
            SLOWEST_BUDGET_NANOS
        );
    }
}
