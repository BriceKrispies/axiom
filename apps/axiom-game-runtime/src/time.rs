//! Timers + state machines (SPEC-07) and tick-sampled tweens (SPEC-09) composed
//! into the bridge: the deterministic [`axiom_tick::TickApi`] timer wheel + state
//! machines and the [`axiom_tween::TweenApi`] eased-curve table, both pumped once
//! per fixed tick inside the loop, and the `#[wasm_bindgen]` boundary the TS
//! `NativeBridge` timer / machine / tween methods bind.
//!
//! ## What lives here
//! - [`TimeBridge`]: the [`TickApi`] (timers + machines) and [`TweenApi`] the
//!   bridge owns, a `raw-id -> TweenId` handle table (a [`TweenId`] cannot be
//!   rebuilt from the bare `u64` that crossed the JS boundary, so the bridge keeps
//!   the live handles — exactly as `physics.rs` keeps an `entity -> body handle`
//!   table), the fixed step (so a tween's whole-tick duration converts to the
//!   nanosecond clock `TweenApi` advances on), and a per-frame *log* of what each
//!   pumped fixed tick produced. It is a `pub(crate)` field of [`GameBridge`]
//!   (initialized in `GameBridge::new`), so all timer/tween/machine logic stays in
//!   this one file.
//! - An `impl GameBridge` block: the native, fully-testable methods the wasm shell
//!   marshals to.
//! - A `#[wasm_bindgen] impl WasmGame` block (wasm32 only): the camelCase exports
//!   the TS edge forwards verbatim.
//!
//! ## Why a per-tick log (the pump → report split)
//! The TS `GameLoop` drives [`GameBridge::advance`] (which runs the frame's whole
//! fixed ticks) and *then* calls its `TickPump` once per fixed tick to dispatch
//! the author closures. The native schedules are **mutating** reads — `TickApi::due`
//! re-arms a repeating timer and drops a one-shot, `TweenApi::advance` accumulates
//! elapsed time and drops a completed tween — so each must be driven **exactly once
//! per tick**. [`advance`] therefore pumps both inside the fixed-step loop and
//! records, per tick, the fired timer ids and each tween's `(id, value, completed)`
//! sample. The read methods (`timers_due` / `tween_active` / `tween_value` /
//! `tween_completed`) then merely *report* the recorded frame — a pure, idempotent
//! lookup that cannot double-fire. The log is cleared at the head of each
//! `advance`, so it holds only the frame the TS pump is about to consume.
//!
//! ## State machines are native reads, events are pure-TS
//! `TickApi` owns each machine's dense current-state index and entry tick, so
//! `machine_create` / `machine_current` / `machine_transition` /
//! `machine_ticks_in_state` are direct facade calls. The `onEnter`/`onUpdate`/
//! `onExit` event derivation lives in the TS `BridgeStateMachine`, so the native
//! side never drains state events — it answers only "what state, since when".
//!
//! ## Boundary convention (the established scalar/byte/string rule)
//! Every value crosses the wasm boundary as a boundary primitive: a timer / tween
//! / machine id and a tick as a JS `number` (`f64`), a fired-id / active-id /
//! completed-id list as a `number[]` (exactly as `world_query` does), a tween's
//! sampled value as an `f64`. A tween's curve is **not** a structured object at the
//! boundary — the TS edge destructures `TweenCurve` into the scalar
//! `(from, to, duration_ticks, ease_index)` args, the tween analogue of physics'
//! `Vec3` destructuring.
//!
//! ## Per-tick pump
//! [`GameBridge::advance`] runs the fixed-step loop, then [`TimeBridge::pump`]
//! advances the tick schedule and the tweens once per fixed tick that ran. A
//! branchless fold over the step count drives the loop and the report lookups are
//! branchless `find`/`map` chains, matching the engine's Branchless Law for app
//! code.

use axiom_kernel::{Tick, TickDelta};
use axiom_tick::{StateMachineId, TickApi, TimerId};
use axiom_tween::{Ease, TweenApi, TweenId, TweenSpec, TweenValue};

use crate::GameBridge;

/// One live tween's recorded sample for a single pumped tick: its raw id, its
/// eased display value, and whether it reached its end on this tick.
#[derive(Debug, Clone, Copy)]
struct TweenSampleRecord {
    id: u64,
    value: f32,
    completed: bool,
}

/// What one pumped fixed tick produced: the timer ids that fired and every live
/// tween's sample. The read methods report this rather than re-driving the
/// mutating native schedules.
#[derive(Debug, Clone)]
struct TickFrame {
    tick: u64,
    due_timers: Vec<u64>,
    samples: Vec<TweenSampleRecord>,
}

/// The seven ease names in their dense index order, matching the TS `EASES` table
/// and the `axiom_tween::Ease` discriminant order. An out-of-range index falls
/// back to `Linear` (index 0), the same default the TS projection uses for an
/// omitted ease.
fn ease_from_index(index: u32) -> Ease {
    [
        Ease::Linear,
        Ease::QuadIn,
        Ease::QuadOut,
        Ease::QuadInOut,
        Ease::CubicOut,
        Ease::ExpoOut,
        Ease::BackOut,
    ]
    .get(index as usize)
    .copied()
    .unwrap_or(Ease::Linear)
}

/// The timers + state machines (SPEC-07) and tweens (SPEC-09) the bridge owns.
#[derive(Debug)]
pub(crate) struct TimeBridge {
    tick: TickApi,
    tween: TweenApi,
    /// The live tween handles, keyed for cancel-by-raw and pruned on completion (a
    /// `TweenId` cannot be rebuilt from the boundary `u64`, so the bridge keeps it).
    live_tweens: Vec<TweenId>,
    fixed_step_nanos: u64,
    log: Vec<TickFrame>,
}

impl TimeBridge {
    /// A fresh time bridge: no timers, machines, or tweens, and an empty log.
    /// `fixed_step_nanos` keys the tween whole-tick → nanosecond conversion.
    pub(crate) fn new(fixed_step_nanos: u64) -> Self {
        TimeBridge {
            tick: TickApi::new(),
            tween: TweenApi::new(),
            live_tweens: Vec::new(),
            fixed_step_nanos,
            log: Vec::new(),
        }
    }

    /// Schedule a one-shot timer registered at `now`, firing `delay` ticks later
    /// (`timerAfter`). Returns its raw id.
    fn after(&mut self, now: u64, delay: u64) -> u64 {
        self.tick.after(Tick::new(now), TickDelta::new(delay)).raw()
    }

    /// Schedule a repeating timer registered at `now`, firing every `interval`
    /// ticks (`timerEvery`). Returns its raw id.
    fn every(&mut self, now: u64, interval: u64) -> u64 {
        self.tick
            .every(Tick::new(now), TickDelta::new(interval))
            .raw()
    }

    /// Cancel a timer (`timerCancel`); `false` for an unknown or already-fired id.
    fn cancel_timer(&mut self, id: u64) -> bool {
        self.tick.cancel(TimerId::from_raw(id))
    }

    /// Create a state machine of `states` dense states starting in `initial` at
    /// `now` (`machineCreate`). Returns its raw id.
    fn create_machine(&mut self, now: u64, states: u32, initial: u32) -> u64 {
        self.tick
            .create_machine(states, initial, Tick::new(now))
            .raw()
    }

    /// The current dense state index of machine `id` (`machineCurrent`); `0` for an
    /// unknown id (the TS projection only ever reads a live machine).
    fn machine_current(&self, id: u64) -> u32 {
        self.tick.current(StateMachineId::from_raw(id)).unwrap_or(0)
    }

    /// Transition machine `id` to `to` at `now` (`machineTransition`); a no-op for
    /// an unknown id.
    fn transition_machine(&mut self, id: u64, now: u64, to: u32) {
        self.tick
            .transition(StateMachineId::from_raw(id), to, Tick::new(now));
    }

    /// The ticks machine `id` has been in its current state as of `now`
    /// (`machineTicksInState`); `0` for an unknown id.
    fn machine_ticks_in_state(&self, id: u64, now: u64) -> u64 {
        self.tick
            .ticks_in_state(StateMachineId::from_raw(id), Tick::new(now))
            .map(TickDelta::raw)
            .unwrap_or(0)
    }

    /// Add a tween from its scalar curve (`tweenAdd`): the whole-tick `duration`
    /// converts to the nanosecond clock `TweenApi` advances on, `ease_index`
    /// selects the curve. Returns its raw id and records the handle so it can be
    /// cancelled by raw id later. The registration tick is implicit — the next
    /// pumped tick is the tween's first sample.
    fn add_tween(&mut self, from: f64, to: f64, duration_ticks: u64, ease_index: u32) -> u64 {
        let spec = TweenSpec {
            from: TweenValue::new(from as f32),
            to: TweenValue::new(to as f32),
            duration_nanos: duration_ticks.saturating_mul(self.fixed_step_nanos),
            ease: ease_from_index(ease_index),
        };
        let id = self.tween.start(spec);
        self.live_tweens.push(id);
        id.raw()
    }

    /// Cancel a tween so it stops sampling (`tweenCancel`); an unknown id is a
    /// clean no-op. Looks the raw id up in the live-handle table (a `TweenId`
    /// cannot be rebuilt from a bare `u64`).
    fn cancel_tween(&mut self, id: u64) {
        let tween = &mut self.tween;
        self.live_tweens
            .iter()
            .copied()
            .filter(|handle| handle.raw() == id)
            .for_each(|handle| tween.cancel(handle));
        self.live_tweens.retain(|handle| handle.raw() != id);
    }

    /// The recorded frame for `tick`, if the most recent pump produced one.
    fn frame(&self, tick: u64) -> Option<&TickFrame> {
        self.log.iter().find(|frame| frame.tick == tick)
    }

    /// The timer ids that fired on `tick` (`timersDue`); empty when none / no such
    /// frame.
    fn timers_due(&self, tick: u64) -> Vec<u64> {
        self.frame(tick)
            .map(|frame| frame.due_timers.clone())
            .unwrap_or_default()
    }

    /// The tween ids sampled on `tick` (`tweenActive`), in start order; empty when
    /// none.
    fn tween_active(&self, tick: u64) -> Vec<u64> {
        self.frame(tick)
            .map(|frame| frame.samples.iter().map(|sample| sample.id).collect())
            .unwrap_or_default()
    }

    /// The eased value of tween `id` on `tick` (`tweenValue`); `0.0` when the
    /// tween was not sampled on that tick.
    fn tween_value(&self, id: u64, tick: u64) -> f64 {
        self.frame(tick)
            .and_then(|frame| frame.samples.iter().find(|sample| sample.id == id))
            .map(|sample| f64::from(sample.value))
            .unwrap_or(0.0)
    }

    /// The tween ids that reached their end on `tick` (`tweenCompleted`); empty
    /// when none.
    fn tween_completed(&self, tick: u64) -> Vec<u64> {
        self.frame(tick)
            .map(|frame| {
                frame
                    .samples
                    .iter()
                    .filter(|sample| sample.completed)
                    .map(|sample| sample.id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Pump the tick schedule and the tweens once per fixed tick that ran this
    /// frame, beginning at `start`, recording each tick's fired timer ids and tween
    /// samples and pruning completed tweens from the live-handle table. The log is
    /// cleared first so it holds only the frame the TS pump is about to consume.
    /// Branchless: a fold over the step count drives the loop.
    pub(crate) fn pump(&mut self, start: u64, steps: u32, fixed_step_nanos: u64) {
        self.log.clear();
        let tick_api = &mut self.tick;
        let tween_api = &mut self.tween;
        let live = &mut self.live_tweens;
        let log = &mut self.log;
        (0..steps).for_each(|step| {
            let tick = start + u64::from(step);
            let due_timers = tick_api
                .due(Tick::new(tick))
                .into_iter()
                .map(TimerId::raw)
                .collect();
            let samples: Vec<TweenSampleRecord> = tween_api
                .advance(fixed_step_nanos)
                .into_iter()
                .map(|sample| TweenSampleRecord {
                    id: sample.id.raw(),
                    value: sample.value.get(),
                    completed: sample.completed,
                })
                .collect();
            let completed: Vec<u64> = samples
                .iter()
                .filter(|sample| sample.completed)
                .map(|sample| sample.id)
                .collect();
            live.retain(|handle| !completed.contains(&handle.raw()));
            log.push(TickFrame {
                tick,
                due_timers,
                samples,
            });
        });
    }
}

impl GameBridge {
    /// Schedule a one-shot timer registered at `tick`, firing `delay` ticks later
    /// (`timerAfter`, SPEC-07). Returns its raw id.
    pub fn timer_after(&mut self, tick: u64, delay: u64) -> u64 {
        self.time.after(tick, delay)
    }

    /// Schedule a repeating timer registered at `tick`, firing every `interval`
    /// ticks (`timerEvery`, SPEC-07). Returns its raw id.
    pub fn timer_every(&mut self, tick: u64, interval: u64) -> u64 {
        self.time.every(tick, interval)
    }

    /// Cancel a timer (`timerCancel`); `false` for an unknown / already-fired id.
    pub fn timer_cancel(&mut self, id: u64) -> bool {
        self.time.cancel_timer(id)
    }

    /// The timer ids that fired on `tick` this frame (`timersDue`).
    pub fn timers_due(&self, tick: u64) -> Vec<u64> {
        self.time.timers_due(tick)
    }

    /// Create a state machine of `state_count` states starting in `initial`,
    /// entered at `tick` (`machineCreate`, SPEC-07). Returns its raw id.
    pub fn machine_create(&mut self, tick: u64, state_count: u32, initial: u32) -> u64 {
        self.time.create_machine(tick, state_count, initial)
    }

    /// The current dense state index of machine `id` (`machineCurrent`).
    pub fn machine_current(&self, id: u64) -> u32 {
        self.time.machine_current(id)
    }

    /// Transition machine `id` to `to`, recording `tick` as the new entry tick
    /// (`machineTransition`).
    pub fn machine_transition(&mut self, id: u64, tick: u64, to: u32) {
        self.time.transition_machine(id, tick, to);
    }

    /// The ticks machine `id` has been in its current state as of `tick`
    /// (`machineTicksInState`).
    pub fn machine_ticks_in_state(&self, id: u64, tick: u64) -> u64 {
        self.time.machine_ticks_in_state(id, tick)
    }

    /// Add a tween from its scalar curve (`tweenAdd`, SPEC-09). Returns its raw id.
    pub fn tween_add(&mut self, from: f64, to: f64, duration_ticks: u64, ease_index: u32) -> u64 {
        self.time.add_tween(from, to, duration_ticks, ease_index)
    }

    /// Cancel a tween so it stops sampling (`tweenCancel`); a stale id is a no-op.
    pub fn tween_cancel(&mut self, id: u64) {
        self.time.cancel_tween(id);
    }

    /// The tween ids sampled on `tick` this frame (`tweenActive`).
    pub fn tween_active(&self, tick: u64) -> Vec<u64> {
        self.time.tween_active(tick)
    }

    /// The eased value of tween `id` on `tick` (`tweenValue`).
    pub fn tween_value(&self, id: u64, tick: u64) -> f64 {
        self.time.tween_value(id, tick)
    }

    /// The tween ids that completed on `tick` this frame (`tweenCompleted`).
    pub fn tween_completed(&self, tick: u64) -> Vec<u64> {
        self.time.tween_completed(tick)
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// Schedule a one-shot timer registered at `tick`, due `delay` ticks later
        /// (`timerAfter`). The id crosses back as a JS number.
        #[wasm_bindgen(js_name = timerAfter)]
        pub fn timer_after(&mut self, tick: f64, delay: f64) -> f64 {
            self.bridge.timer_after(tick as u64, delay as u64) as f64
        }

        /// Schedule a repeating timer registered at `tick`, firing every
        /// `interval` ticks (`timerEvery`).
        #[wasm_bindgen(js_name = timerEvery)]
        pub fn timer_every(&mut self, tick: f64, interval: f64) -> f64 {
            self.bridge.timer_every(tick as u64, interval as u64) as f64
        }

        /// Cancel a timer (`timerCancel`); the boundary contract is `void`, so the
        /// pending-flag is dropped here.
        #[wasm_bindgen(js_name = timerCancel)]
        pub fn timer_cancel(&mut self, id: f64) {
            self.bridge.timer_cancel(id as u64);
        }

        /// The timer ids due to fire on `tick`, as a JS `number[]` (`timersDue`).
        #[wasm_bindgen(js_name = timersDue)]
        pub fn timers_due(&self, tick: f64) -> Vec<JsValue> {
            self.bridge
                .timers_due(tick as u64)
                .into_iter()
                .map(|id| JsValue::from_f64(id as f64))
                .collect()
        }

        /// Create a machine of `state_count` states starting in `initial`, entered
        /// at `tick` (`machineCreate`).
        #[wasm_bindgen(js_name = machineCreate)]
        pub fn machine_create(&mut self, tick: f64, state_count: f64, initial: f64) -> f64 {
            self.bridge
                .machine_create(tick as u64, state_count as u32, initial as u32) as f64
        }

        /// The current dense state index of machine `id` (`machineCurrent`).
        #[wasm_bindgen(js_name = machineCurrent)]
        pub fn machine_current(&self, id: f64) -> f64 {
            f64::from(self.bridge.machine_current(id as u64))
        }

        /// Move machine `id` to state `to`, recording `tick` as the entry tick
        /// (`machineTransition`).
        #[wasm_bindgen(js_name = machineTransition)]
        pub fn machine_transition(&mut self, id: f64, tick: f64, to: f64) {
            self.bridge
                .machine_transition(id as u64, tick as u64, to as u32);
        }

        /// The ticks machine `id` has been in its current state as of `tick`
        /// (`machineTicksInState`).
        #[wasm_bindgen(js_name = machineTicksInState)]
        pub fn machine_ticks_in_state(&self, id: f64, tick: f64) -> f64 {
            self.bridge.machine_ticks_in_state(id as u64, tick as u64) as f64
        }

        /// Add a tween from its scalar curve (`tweenAdd`): the TS edge destructures
        /// the `TweenCurve` object into these scalar args (the tween analogue of
        /// physics' `Vec3` destructuring). The leading `tick` carries the contract
        /// shape but the registration tick is implicit (the next pumped tick), so
        /// it is ignored. Returns the tween id as a JS number.
        #[wasm_bindgen(js_name = tweenAdd)]
        pub fn tween_add(
            &mut self,
            _tick: f64,
            from: f64,
            to: f64,
            duration_ticks: f64,
            ease_index: f64,
        ) -> f64 {
            self.bridge
                .tween_add(from, to, duration_ticks as u64, ease_index as u32) as f64
        }

        /// Cancel a tween so it stops sampling (`tweenCancel`).
        #[wasm_bindgen(js_name = tweenCancel)]
        pub fn tween_cancel(&mut self, id: f64) {
            self.bridge.tween_cancel(id as u64);
        }

        /// The tween ids to sample on `tick`, as a JS `number[]` (`tweenActive`).
        #[wasm_bindgen(js_name = tweenActive)]
        pub fn tween_active(&self, tick: f64) -> Vec<JsValue> {
            self.bridge
                .tween_active(tick as u64)
                .into_iter()
                .map(|id| JsValue::from_f64(id as f64))
                .collect()
        }

        /// The eased value of tween `id` at `tick` (`tweenValue`).
        #[wasm_bindgen(js_name = tweenValue)]
        pub fn tween_value(&self, id: f64, tick: f64) -> f64 {
            self.bridge.tween_value(id as u64, tick as u64)
        }

        /// The tween ids that reach their end on `tick`, as a JS `number[]`
        /// (`tweenCompleted`).
        #[wasm_bindgen(js_name = tweenCompleted)]
        pub fn tween_completed(&self, tick: f64) -> Vec<JsValue> {
            self.bridge
                .tween_completed(tick as u64)
                .into_iter()
                .map(|id| JsValue::from_f64(id as f64))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{demo_app, GameBridge};

    /// 1 ms fixed step, one tick per `advance` (matches the other slice tests), so
    /// each scripted `advance` is exactly one pumped tick at a known tick index.
    const STEP: u64 = 1_000_000;

    fn bridge() -> GameBridge {
        GameBridge::new(demo_app().build(), 0, STEP, 1)
    }

    /// Deterministic FNV-1a over a byte buffer — the per-tick time/tween fingerprint.
    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    /// The whole observable time/tween boundary for `tick` folded to one hash: the
    /// fired timer ids, the active tween ids and each one's value, and the
    /// completed tween ids.
    fn time_hash(b: &GameBridge, tick: u64, tweens: &[u64]) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        b.timers_due(tick)
            .iter()
            .for_each(|&id| buf.extend_from_slice(&id.to_le_bytes()));
        buf.push(0xFE);
        b.tween_active(tick)
            .iter()
            .for_each(|&id| buf.extend_from_slice(&id.to_le_bytes()));
        buf.push(0xFD);
        tweens
            .iter()
            .for_each(|&id| buf.extend_from_slice(&b.tween_value(id, tick).to_le_bytes()));
        buf.push(0xFC);
        b.tween_completed(tick)
            .iter()
            .for_each(|&id| buf.extend_from_slice(&id.to_le_bytes()));
        fnv1a(&buf)
    }

    /// Drive a scripted timer + tween session and return the per-tick boundary-read
    /// hash sequence: a one-shot `after`, a repeating `every`, a cancelled timer,
    /// and an eased tween, pumped one tick per advance over a fixed window.
    fn scripted_time_hashes() -> Vec<(u64, u64)> {
        let mut b = bridge();
        // The schedule is populated before its due ticks are pumped, exactly as
        // the loop schedules during an author update.
        let _one_shot = b.timer_after(0, 3);
        let _repeating = b.timer_every(0, 4);
        let canceled = b.timer_after(0, 5);
        assert!(b.timer_cancel(canceled));
        // Ease index 4 = cubicOut.
        let tween = b.tween_add(0.0, 100.0, 5, 4);
        (0..10u64)
            .map(|tick| {
                b.advance(STEP);
                assert!(!b.timers_due(tick).contains(&canceled));
                (tick, time_hash(&b, tick, &[tween]))
            })
            .collect()
    }

    #[test]
    fn the_time_boundary_replays_to_a_byte_identical_hash_sequence() {
        let first = scripted_time_hashes();
        assert_eq!(first, scripted_time_hashes());
        // The schedule genuinely evolves (timers fire, the tween ramps), so the
        // fingerprint is not constant.
        let hashes: Vec<u64> = first.iter().map(|&(_, h)| h).collect();
        assert!(hashes.iter().any(|&h| h != hashes[0]));
    }

    #[test]
    fn a_one_shot_timer_fires_once_at_its_deadline_and_every_re_arms() {
        let mut b = bridge();
        let one_shot = b.timer_after(0, 3);
        let repeating = b.timer_every(0, 4);
        let due: Vec<Vec<u64>> = (0..10u64)
            .map(|tick| {
                b.advance(STEP);
                b.timers_due(tick)
            })
            .collect();
        assert_eq!(due[3], vec![one_shot]);
        assert!(due
            .iter()
            .enumerate()
            .all(|(t, ids)| (t == 3) == ids.contains(&one_shot)));
        assert_eq!(due[4], vec![repeating]);
        assert_eq!(due[8], vec![repeating]);
        assert!(due
            .iter()
            .enumerate()
            .all(|(t, ids)| (t == 4 || t == 8) == ids.contains(&repeating)));
    }

    #[test]
    fn a_tween_ramps_to_its_end_value_then_completes_exactly_once() {
        let mut b = bridge();
        // Adding the tween "at tick 0" (after tick 0 pumped) means the next
        // pumped tick (tick 1) is its first sample.
        b.advance(STEP);
        let tween = b.tween_add(0.0, 10.0, 4, 0);
        let samples: Vec<(u64, f64, bool)> = (1..7u64)
            .map(|tick| {
                b.advance(STEP);
                let value = b.tween_value(tween, tick);
                let completed = b.tween_completed(tick).contains(&tween);
                (tick, value, completed)
            })
            .collect();
        // Quarter along the linear 0->10 ramp.
        assert!((samples[0].1 - 2.5).abs() < 1e-3);
        let completed: Vec<&(u64, f64, bool)> = samples.iter().filter(|s| s.2).collect();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].0, 4);
        assert!((completed[0].1 - 10.0).abs() < 1e-3);
    }

    #[test]
    fn a_cancelled_tween_stops_sampling() {
        let mut b = bridge();
        let live = b.tween_add(0.0, 5.0, 10, 0);
        // Added before the first pump, so it is already sampled on tick 0.
        b.advance(STEP);
        b.advance(STEP);
        assert!(b.tween_active(1).contains(&live));
        b.tween_cancel(live);
        b.advance(STEP);
        assert!(!b.tween_active(2).contains(&live));
        assert!((b.tween_value(live, 2) - 0.0).abs() < 1e-9);
        // Cancelling an unknown id must not panic.
        b.tween_cancel(9999);
    }

    #[test]
    fn a_state_machine_tracks_its_current_index_and_dwell_time() {
        let mut b = bridge();
        let m = b.machine_create(0, 3, 0);
        assert_eq!(b.machine_current(m), 0);
        assert_eq!(b.machine_ticks_in_state(m, 5), 5);
        // Transitioning resets the dwell clock.
        b.machine_transition(m, 5, 2);
        assert_eq!(b.machine_current(m), 2);
        assert_eq!(b.machine_ticks_in_state(m, 8), 3);
        assert_eq!(b.machine_current(9999), 0);
        assert_eq!(b.machine_ticks_in_state(9999, 8), 0);
    }

    #[test]
    fn reads_for_an_unpumped_tick_are_empty() {
        let b = bridge();
        assert!(b.timers_due(0).is_empty());
        assert!(b.tween_active(0).is_empty());
        assert!(b.tween_completed(0).is_empty());
        assert!((b.tween_value(0, 0) - 0.0).abs() < 1e-9);
    }
}
