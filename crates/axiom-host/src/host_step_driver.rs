//! The deterministic adapter that drives `Runtime::step` from host frames.

use axiom_runtime::Runtime;

use crate::host_boundary_config::HostBoundaryConfig;
use crate::host_error::HostError;
use crate::host_frame_input::HostFrameInput;
use crate::host_frame_report::HostFrameReport;
use crate::host_lifecycle_signal::HostLifecycleSignal;
use crate::host_lifecycle_state::HostLifecycleState;
use crate::host_result::HostResult;
use crate::host_step_plan::HostStepPlan;

/// The host-boundary driver. Owns the deterministic accumulator and the
/// running lifecycle projection; borrows a [`Runtime`] each call to actually
/// step the engine.
///
/// The driver is **the only place** in Layer 03 that calls
/// [`Runtime::step`]. It never sleeps, never spawns, never schedules a
/// `requestAnimationFrame`, and never reads a clock — every timing value is
/// supplied by the host as data on the [`HostFrameInput`]. This is what makes
/// the driver a Layer-03 semantic adapter over Layer-01 runtime stepping.
#[derive(Debug, Clone)]
pub struct HostStepDriver {
    config: HostBoundaryConfig,
    lifecycle: HostLifecycleState,
    accumulator_nanos: u64,
    last_sequence: Option<u64>,
}

impl HostStepDriver {
    /// Build a driver around a validated boundary config.
    pub fn new(config: HostBoundaryConfig) -> Self {
        HostStepDriver {
            config,
            lifecycle: HostLifecycleState::initial(),
            accumulator_nanos: 0,
            last_sequence: None,
        }
    }

    /// Apply one externally-supplied lifecycle signal to the driver's
    /// projection. The runtime is not touched.
    pub fn apply_lifecycle_signal(&mut self, signal: HostLifecycleSignal) {
        self.lifecycle = self.lifecycle.apply(signal);
    }

    /// The current lifecycle state.
    pub const fn lifecycle(&self) -> HostLifecycleState {
        self.lifecycle
    }

    /// The current accumulator carryover in integer nanoseconds.
    pub const fn accumulator_nanos(&self) -> u64 {
        self.accumulator_nanos
    }

    /// The most recent host frame sequence the driver accepted, or `None`
    /// if no frame has been driven yet.
    pub const fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    /// The current boundary config.
    pub const fn config(&self) -> &HostBoundaryConfig {
        &self.config
    }

    /// Drive one host frame: validate the input, build a step plan, and
    /// invoke `Runtime::step` exactly as many times as the plan asks for.
    ///
    /// - Out-of-order frames are rejected with `InvalidFrameSequence`.
    /// - A `Runtime::step` error is wrapped as `RuntimeStepFailed` with the
    ///   runtime cause preserved; partial step records collected before the
    ///   failure are still returned in the error path's drop, but the
    ///   driver returns `Err` so callers can react explicitly.
    pub fn drive(
        &mut self,
        runtime: &mut Runtime,
        input: HostFrameInput,
    ) -> HostResult<HostFrameReport> {
        if let Some(last) = self.last_sequence {
            if input.sequence() <= last {
                return Err(HostError::invalid_frame_sequence(
                    "host frame sequence must strictly increase",
                ));
            }
        }

        let plan = HostStepPlan::build(
            &input,
            &self.config,
            &self.lifecycle,
            self.accumulator_nanos,
        );

        let mut step_records = Vec::with_capacity(plan.steps() as usize);
        for _ in 0..plan.steps() {
            let record = runtime.step().map_err(|e| {
                HostError::runtime_step_failed("Runtime::step rejected by runtime", e)
            })?;
            step_records.push(record);
        }

        // Commit accumulator only after a successful drive. For a lifecycle
        // skip the plan's `retained_nanos` already reflects policy.
        self.accumulator_nanos = plan.retained_nanos();
        self.last_sequence = Some(input.sequence());

        let steps_executed = step_records.len() as u32;
        Ok(HostFrameReport::new(
            input.sequence(),
            plan,
            steps_executed,
            step_records,
            *input.viewport(),
            self.lifecycle,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;
    use crate::host_lifecycle_signal::HostLifecycleSignal;
    use crate::host_viewport::HostViewport;
    use axiom_kernel::Ratio;
    use axiom_runtime::RuntimeConfig;

    const STEP_NANOS: u64 = 1_000;

    fn vp() -> HostViewport {
        HostViewport::new(800, 600, Ratio::new(1.0).unwrap()).unwrap()
    }

    fn cfg() -> HostBoundaryConfig {
        HostBoundaryConfig::new(STEP_NANOS, 5).unwrap()
    }

    fn started_driver_and_runtime() -> (HostStepDriver, Runtime) {
        let mut driver = HostStepDriver::new(cfg());
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);
        let mut runtime = Runtime::new(RuntimeConfig::new(STEP_NANOS)).unwrap();
        runtime.initialize().unwrap();
        runtime.start().unwrap();
        (driver, runtime)
    }

    #[test]
    fn driver_performs_exact_planned_step_count() {
        let (mut driver, mut runtime) = started_driver_and_runtime();
        let report = driver
            .drive(&mut runtime, HostFrameInput::new(1, 3 * STEP_NANOS, vp()))
            .unwrap();
        assert_eq!(report.steps_executed(), 3);
        assert_eq!(report.plan().steps(), 3);
        assert_eq!(report.step_records().len(), 3);
    }

    #[test]
    fn driver_returns_ordered_step_records() {
        let (mut driver, mut runtime) = started_driver_and_runtime();
        let report = driver
            .drive(&mut runtime, HostFrameInput::new(1, 3 * STEP_NANOS, vp()))
            .unwrap();
        let ticks: Vec<u64> = report
            .step_records()
            .iter()
            .map(|r| r.step().tick().raw())
            .collect();
        assert_eq!(ticks, vec![1, 2, 3]);
    }

    #[test]
    fn driver_rejects_out_of_order_frame_sequence() {
        let (mut driver, mut runtime) = started_driver_and_runtime();
        driver
            .drive(&mut runtime, HostFrameInput::new(5, STEP_NANOS, vp()))
            .unwrap();
        let err = driver
            .drive(&mut runtime, HostFrameInput::new(4, STEP_NANOS, vp()))
            .unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidFrameSequence);
    }

    #[test]
    fn driver_rejects_equal_frame_sequence() {
        let (mut driver, mut runtime) = started_driver_and_runtime();
        driver
            .drive(&mut runtime, HostFrameInput::new(5, STEP_NANOS, vp()))
            .unwrap();
        let err = driver
            .drive(&mut runtime, HostFrameInput::new(5, STEP_NANOS, vp()))
            .unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidFrameSequence);
    }

    #[test]
    fn driver_preserves_accumulator_between_frames() {
        let (mut driver, mut runtime) = started_driver_and_runtime();
        // Frame 1: half a step elapsed → 0 runtime steps, half a step retained.
        let r1 = driver
            .drive(&mut runtime, HostFrameInput::new(1, STEP_NANOS / 2, vp()))
            .unwrap();
        assert_eq!(r1.steps_executed(), 0);
        assert_eq!(driver.accumulator_nanos(), STEP_NANOS / 2);
        // Frame 2: another half-step elapsed → accumulator now 1 step → 1 runtime step.
        let r2 = driver
            .drive(&mut runtime, HostFrameInput::new(2, STEP_NANOS / 2, vp()))
            .unwrap();
        assert_eq!(r2.steps_executed(), 1);
        assert_eq!(driver.accumulator_nanos(), 0);
    }

    #[test]
    fn driver_clamps_catch_up_to_max_steps() {
        let (mut driver, mut runtime) = started_driver_and_runtime();
        let report = driver
            .drive(&mut runtime, HostFrameInput::new(1, 100 * STEP_NANOS, vp()))
            .unwrap();
        assert_eq!(
            report.steps_executed(),
            5,
            "clamped to max_steps_per_frame=5"
        );
        assert_eq!(driver.accumulator_nanos(), 95 * STEP_NANOS);
    }

    #[test]
    fn driver_is_value_for_value_deterministic_across_identical_input_sequences() {
        let make = || {
            let (mut driver, mut runtime) = started_driver_and_runtime();
            let mut last_tick = 0u64;
            for (sequence, elapsed) in
                (1u64..).zip([STEP_NANOS / 2, STEP_NANOS / 2, 3 * STEP_NANOS, STEP_NANOS])
            {
                let r = driver
                    .drive(&mut runtime, HostFrameInput::new(sequence, elapsed, vp()))
                    .unwrap();
                if let Some(last) = r.step_records().last() {
                    last_tick = last.step().tick().raw();
                }
            }
            (driver.accumulator_nanos(), last_tick)
        };
        assert_eq!(make(), make());
    }

    #[test]
    fn driver_does_not_step_while_hidden_when_policy_forbids() {
        let mut driver = HostStepDriver::new(cfg());
        // Lifecycle: started then hidden → !visible, default policy
        // (step_while_hidden=false) blocks stepping.
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);
        driver.apply_lifecycle_signal(HostLifecycleSignal::Hidden);
        let mut runtime = Runtime::new(RuntimeConfig::new(STEP_NANOS)).unwrap();
        runtime.initialize().unwrap();
        runtime.start().unwrap();
        let report = driver
            .drive(&mut runtime, HostFrameInput::new(1, STEP_NANOS, vp()))
            .unwrap();
        assert!(report.is_skipped());
        assert_eq!(report.steps_executed(), 0);
    }

    #[test]
    fn driver_does_not_step_when_suspended() {
        let mut driver = HostStepDriver::new(cfg().with_step_while_hidden(true));
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);
        driver.apply_lifecycle_signal(HostLifecycleSignal::Suspended);
        let mut runtime = Runtime::new(RuntimeConfig::new(STEP_NANOS)).unwrap();
        runtime.initialize().unwrap();
        runtime.start().unwrap();
        let report = driver
            .drive(&mut runtime, HostFrameInput::new(1, STEP_NANOS, vp()))
            .unwrap();
        assert!(report.is_skipped());
        assert_eq!(report.steps_executed(), 0);
    }

    #[test]
    fn driver_propagates_runtime_step_failures() {
        // A runtime that has never been `start()`ed rejects `step()`; we
        // make the driver visible so the plan asks for one runtime step.
        let mut driver = HostStepDriver::new(cfg());
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);
        let mut runtime = Runtime::new(RuntimeConfig::new(STEP_NANOS)).unwrap();
        // Deliberately NOT calling initialize/start.
        let err = driver
            .drive(&mut runtime, HostFrameInput::new(1, STEP_NANOS, vp()))
            .unwrap_err();
        assert_eq!(err.code(), HostErrorCode::RuntimeStepFailed);
        assert!(err.runtime().is_some(), "runtime cause must be preserved");
    }

    #[test]
    fn last_sequence_reflects_the_most_recent_driven_frame() {
        // Distinguishes `last_sequence -> None`: after driving a frame the
        // driver reports the real accepted sequence, not None.
        let (mut driver, mut runtime) = started_driver_and_runtime();
        assert_eq!(driver.last_sequence(), None);
        driver
            .drive(&mut runtime, HostFrameInput::new(42, STEP_NANOS, vp()))
            .unwrap();
        assert_eq!(driver.last_sequence(), Some(42));
        driver
            .drive(&mut runtime, HostFrameInput::new(43, STEP_NANOS, vp()))
            .unwrap();
        assert_eq!(driver.last_sequence(), Some(43));
    }

    #[test]
    fn accessors_round_trip_initial_state() {
        let driver = HostStepDriver::new(cfg());
        assert_eq!(driver.accumulator_nanos(), 0);
        assert_eq!(driver.last_sequence(), None);
        assert_eq!(driver.lifecycle(), HostLifecycleState::initial());
        assert_eq!(driver.config(), &cfg());
    }
}
