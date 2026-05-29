//! The (deterministic) configuration a [`crate::runtime::Runtime`] starts with.

use axiom_kernel::{FixedStep, KernelApi};

use crate::runtime_error::RuntimeError;
use crate::runtime_error_code::RuntimeErrorCode;
use crate::runtime_result::RuntimeResult;

/// Deterministic runtime configuration.
///
/// The fixed timestep is expressed in integer nanoseconds and validated
/// against the kernel ([`KernelApi::fixed_step`]) at runtime construction —
/// returning a [`FixedStep`] guarantees no zero-step clock can exist.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeConfig {
    fixed_step_nanos: u64,
    max_steps_per_frame: u32,
    fail_on_system_error: bool,
    diagnostics_enabled: bool,
}

impl RuntimeConfig {
    /// A standard 60 Hz config (`16_666_667 ns` ≈ 1/60 s), fail-fast on system
    /// error, diagnostics on, at most one step per frame.
    pub const DEFAULT_FIXED_STEP_NANOS: u64 = 16_666_667;

    /// Build a config with the given fixed step in integer nanoseconds.
    pub const fn new(fixed_step_nanos: u64) -> Self {
        RuntimeConfig {
            fixed_step_nanos,
            max_steps_per_frame: 1,
            fail_on_system_error: true,
            diagnostics_enabled: true,
        }
    }

    /// At most this many simulation steps may be requested per `step` call.
    /// Future host integrations (catch-up loops) will honor this; today the
    /// runtime steps once per call and exposes the value for layers above.
    pub const fn with_max_steps_per_frame(mut self, n: u32) -> Self {
        self.max_steps_per_frame = n;
        self
    }

    /// If `false`, a failing system records the error and continues with the
    /// next system; the runtime stays `Running`. If `true` (default), the
    /// scheduler stops at the failure and the runtime transitions to `Failed`.
    pub const fn with_fail_on_system_error(mut self, fail: bool) -> Self {
        self.fail_on_system_error = fail;
        self
    }

    /// Toggle diagnostics collection. Today this only affects whether the
    /// runtime emits a structured `LogRecord` summarizing each step — step
    /// records themselves are always produced.
    pub const fn with_diagnostics_enabled(mut self, on: bool) -> Self {
        self.diagnostics_enabled = on;
        self
    }

    /// The raw fixed step magnitude, in nanoseconds.
    pub const fn fixed_step_nanos(&self) -> u64 {
        self.fixed_step_nanos
    }

    /// The maximum number of simulation steps that may be requested per frame.
    pub const fn max_steps_per_frame(&self) -> u32 {
        self.max_steps_per_frame
    }

    /// Whether a system error should fail the runtime.
    pub const fn fail_on_system_error(&self) -> bool {
        self.fail_on_system_error
    }

    /// Whether diagnostics emission is enabled.
    pub const fn diagnostics_enabled(&self) -> bool {
        self.diagnostics_enabled
    }

    /// Validate this config against the kernel and return a kernel
    /// [`FixedStep`] suitable for constructing a `SimulationClock`.
    ///
    /// Any kernel rejection is wrapped as
    /// [`RuntimeErrorCode::InvalidConfig`] with the underlying `KernelError`
    /// preserved.
    pub fn validate(&self, kernel: &KernelApi) -> RuntimeResult<FixedStep> {
        kernel.fixed_step(self.fixed_step_nanos).map_err(|e| {
            RuntimeError::with_kernel(
                RuntimeErrorCode::InvalidConfig,
                "fixed step rejected by kernel",
                e,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_60_hz_fail_fast_diag_on() {
        let c = RuntimeConfig::new(RuntimeConfig::DEFAULT_FIXED_STEP_NANOS);
        assert_eq!(c.fixed_step_nanos(), 16_666_667);
        assert_eq!(c.max_steps_per_frame(), 1);
        assert!(c.fail_on_system_error());
        assert!(c.diagnostics_enabled());
    }

    #[test]
    fn builder_setters_are_applied() {
        let c = RuntimeConfig::new(1_000)
            .with_max_steps_per_frame(5)
            .with_fail_on_system_error(false)
            .with_diagnostics_enabled(false);
        assert_eq!(c.max_steps_per_frame(), 5);
        assert!(!c.fail_on_system_error());
        assert!(!c.diagnostics_enabled());
    }

    #[test]
    fn positive_step_validates_against_kernel() {
        let api = KernelApi::new();
        let fs = RuntimeConfig::new(1_000).validate(&api).unwrap();
        assert_eq!(fs.nanos(), 1_000);
    }

    #[test]
    fn zero_step_is_rejected_with_wrapped_kernel_error() {
        let api = KernelApi::new();
        let err = RuntimeConfig::new(0).validate(&api).unwrap_err();
        assert_eq!(err.code(), RuntimeErrorCode::InvalidConfig);
        assert!(
            err.kernel().is_some(),
            "InvalidConfig must preserve the kernel cause"
        );
    }
}
