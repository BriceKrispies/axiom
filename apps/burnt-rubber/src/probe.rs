//! A deterministic control surface for diagnosing the **live browser** frame.
//!
//! ## Why this exists
//!
//! Some rendering defects only exist in motion. Sampling artifacts are the
//! clearest case: a moiré is visible in a still, but *crawl* — the pattern
//! swimming as the camera advances — is by definition a relationship between
//! consecutive frames, and no single capture can show it. The off-screen
//! `axiom-shot` slices render one frame of one posed moment, which makes them
//! excellent regression evidence and useless for this.
//!
//! Driving the real page by hand does not work either. Synthetic key events give
//! no control over *where* the car is or *how fast* it is going, the run drifts
//! off the racing line, traffic interferes, and two runs are never comparable. A
//! screenshot taken while the game is free-running is a frame at an unknown
//! moment, so a pair of them differ by an unknown amount of travel — which is
//! precisely the quantity a crawl measurement has to hold fixed.
//!
//! This module makes the live session **steppable**: park the car at a known
//! point at a known speed, freeze the clock, and advance an exact number of fixed
//! simulation steps at a time. Every screenshot is then a known game state, and
//! two consecutive screenshots differ by exactly one frame of travel — the same
//! difference the player's eye integrates into "the road is crawling". The wall
//! clock is out of the loop, so capture latency cannot smear the measurement.
//!
//! ## Shape
//!
//! The probe owns only *intent*, never the app. [`ProbeControls`] is a small
//! plain value that the browser frame closure reads once per frame and acts on,
//! and writes a state snapshot back into for JavaScript to read. That keeps the
//! live state private to `web.rs` where it belongs, keeps this module free of any
//! app internals, and — because the whole thing is plain data — leaves it
//! testable natively, without a browser.
//!
//! It is diagnosis scaffolding, not a game feature: nothing in the shipping play
//! path reads it, and with no probe command issued every field is inert and the
//! frame closure behaves exactly as it did before.

use core::cell::RefCell;

/// What the probe wants the next frame to do, plus what the last frame reported.
///
/// Every field is inert at its default, so a session where JavaScript never
/// touches the probe runs the normal free-running game.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ProbeControls {
    /// Freeze the simulation clock. While set, the game advances only by
    /// [`Self::request_steps`] and renders a stable frame in between.
    paused: bool,
    /// Fixed steps still owed to a [`Self::request_steps`] call.
    pending_steps: u32,
    /// A pending "put the car here, at this speed" request, in `(metres along
    /// the course, metres per second)`.
    placement: Option<(f32, f32)>,
    /// Drive with the deterministic script autopilot instead of player input, so
    /// the car holds the racing line without a human and two runs from the same
    /// placement are identical.
    autopilot: bool,
    /// The last frame's reported course distance (m).
    distance: f32,
    /// The last frame's reported speed (m/s).
    speed: f32,
}

impl ProbeControls {
    /// Whether the simulation clock is frozen.
    pub const fn paused(&self) -> bool {
        self.paused
    }

    /// Whether the deterministic autopilot is driving.
    pub const fn autopilot(&self) -> bool {
        self.autopilot
    }

    /// Freeze or resume the simulation clock. Resuming drops any steps still
    /// owed — they were a request about frozen time, and honouring them after
    /// the clock restarts would double-advance the car.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.pending_steps = if paused { self.pending_steps } else { 0 };
    }

    /// Drive with the deterministic autopilot, or hand control back.
    pub fn set_autopilot(&mut self, autopilot: bool) {
        self.autopilot = autopilot;
    }

    /// Ask for the car to be placed `distance` metres along the course, moving
    /// at `speed` m/s. Applied by the next frame; a second request before that
    /// frame replaces the first rather than queueing.
    pub fn request_placement(&mut self, distance: f32, speed: f32) {
        self.placement = Some((distance, speed));
    }

    /// Take the pending placement, if any. The frame closure calls this, so a
    /// placement is applied exactly once.
    pub fn take_placement(&mut self) -> Option<(f32, f32)> {
        self.placement.take()
    }

    /// Ask for `steps` more fixed simulation steps while frozen. Requests
    /// accumulate, so two calls of 1 owe two steps.
    pub fn request_steps(&mut self, steps: u32) {
        self.pending_steps = self.pending_steps.saturating_add(steps);
    }

    /// Take the steps owed, zeroing the debt. Returns `0` when the clock is
    /// running, because then the frame advances by elapsed time instead and
    /// spending a step here as well would advance twice.
    pub fn take_steps(&mut self) -> u32 {
        let owed = if self.paused { self.pending_steps } else { 0 };
        self.pending_steps -= owed;
        owed
    }

    /// Record what the frame observed, for [`Self::state_json`] to report.
    pub fn report(&mut self, distance: f32, speed: f32) {
        self.distance = distance;
        self.speed = speed;
    }

    /// The probe's state as a JSON object, for the driving script to read back
    /// and assert on. Hand-formatted rather than pulling in a serialiser: this
    /// is four numbers and two flags on a diagnosis path.
    pub fn state_json(&self) -> String {
        format!(
            "{{\"distance\":{:.3},\"speed\":{:.3},\"paused\":{},\"autopilot\":{},\"pendingSteps\":{}}}",
            self.distance, self.speed, self.paused, self.autopilot, self.pending_steps
        )
    }
}

thread_local! {
    /// The live session's probe. A thread local because the browser arm is
    /// single-threaded and the frame closure and the `wasm_bindgen` entry points
    /// are different call stacks that must reach the same value.
    static PROBE: RefCell<ProbeControls> = const { RefCell::new(ProbeControls {
        paused: false,
        pending_steps: 0,
        placement: None,
        autopilot: false,
        distance: 0.0,
        speed: 0.0,
    }) };
}

/// Read-modify-write the session probe.
pub fn with_probe<R>(f: impl FnOnce(&mut ProbeControls) -> R) -> R {
    PROBE.with(|p| f(&mut p.borrow_mut()))
}

/// Park the car `distance` metres along the course at `speed` m/s.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn burnt_rubber_probe_place(distance: f32, speed: f32) {
    with_probe(|p| p.request_placement(distance, speed));
}

/// Freeze (or resume) the simulation clock.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn burnt_rubber_probe_pause(paused: bool) {
    with_probe(|p| p.set_paused(paused));
}

/// Advance exactly `steps` fixed simulation steps while frozen.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn burnt_rubber_probe_step(steps: u32) {
    with_probe(|p| p.request_steps(steps));
}

/// Drive with the deterministic autopilot instead of player input.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn burnt_rubber_probe_autopilot(on: bool) {
    with_probe(|p| p.set_autopilot(on));
}

/// The probe's current state, as JSON.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn burnt_rubber_probe_state() -> String {
    with_probe(|p| p.state_json())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_probe_is_completely_inert() {
        let mut p = ProbeControls::default();
        assert!(!p.paused());
        assert!(!p.autopilot());
        assert_eq!(p.take_placement(), None);
        assert_eq!(p.take_steps(), 0, "a running clock owes no manual steps");
    }

    #[test]
    fn a_placement_is_applied_exactly_once() {
        let mut p = ProbeControls::default();
        p.request_placement(1_200.0, 86.0);
        assert_eq!(p.take_placement(), Some((1_200.0, 86.0)));
        assert_eq!(p.take_placement(), None, "a placement must not repeat");
        // A second request before the frame reads it replaces the first.
        p.request_placement(10.0, 1.0);
        p.request_placement(20.0, 2.0);
        assert_eq!(p.take_placement(), Some((20.0, 2.0)));
    }

    /// The core of the whole measurement: while frozen, the car advances by
    /// exactly the steps asked for and by nothing else.
    #[test]
    fn steps_are_owed_only_while_frozen_and_are_spent_exactly_once() {
        let mut p = ProbeControls::default();
        p.set_paused(true);
        p.request_steps(1);
        p.request_steps(2);
        assert_eq!(p.take_steps(), 3, "requests accumulate");
        assert_eq!(p.take_steps(), 0, "and are not re-spent");
    }

    /// While the clock runs, the frame advances by elapsed time. Spending owed
    /// steps as well would advance the car twice in one frame and quietly
    /// corrupt any measurement taken from it.
    #[test]
    fn a_running_clock_never_spends_manual_steps() {
        let mut p = ProbeControls::default();
        p.request_steps(5);
        assert_eq!(p.take_steps(), 0);
        // Freezing then makes the same debt spendable.
        p.set_paused(true);
        assert_eq!(p.take_steps(), 5);
    }

    #[test]
    fn resuming_the_clock_drops_steps_that_were_never_spent() {
        let mut p = ProbeControls::default();
        p.set_paused(true);
        p.request_steps(4);
        p.set_paused(false);
        p.set_paused(true);
        assert_eq!(
            p.take_steps(),
            0,
            "steps owed to frozen time must not survive the clock restarting"
        );
    }

    #[test]
    fn the_state_json_reports_what_the_frame_observed() {
        let mut p = ProbeControls::default();
        p.set_paused(true);
        p.set_autopilot(true);
        p.request_steps(2);
        p.report(1_234.5, 86.25);
        let json = p.state_json();
        assert!(json.contains("\"distance\":1234.500"), "{json}");
        assert!(json.contains("\"speed\":86.250"), "{json}");
        assert!(json.contains("\"paused\":true"), "{json}");
        assert!(json.contains("\"autopilot\":true"), "{json}");
        assert!(json.contains("\"pendingSteps\":2"), "{json}");
    }

    #[test]
    fn the_thread_local_probe_round_trips() {
        with_probe(|p| p.set_autopilot(true));
        assert!(with_probe(|p| p.autopilot()));
        with_probe(|p| p.set_autopilot(false));
        assert!(!with_probe(|p| p.autopilot()));
    }
}
