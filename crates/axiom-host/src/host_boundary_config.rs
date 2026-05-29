//! Deterministic configuration for the Layer-03 host boundary.

use axiom_kernel::KernelApi;

use crate::host_error::HostError;
use crate::host_result::HostResult;

/// Deterministic configuration for the host boundary's step planner.
///
/// Owns the fixed simulation step, the maximum number of catch-up steps a
/// single host frame may drive, and the lifecycle / accumulator policy. The
/// fixed step is validated against the kernel's [`KernelApi::fixed_step`] at
/// [`HostBoundaryConfig::validate`] time so a zero or otherwise invalid step
/// is rejected before the driver ever runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostBoundaryConfig {
    fixed_step_nanos: u64,
    max_steps_per_frame: u32,
    step_while_hidden: bool,
    retain_accumulator: bool,
}

impl HostBoundaryConfig {
    /// Construct with the two required values: the fixed simulation step in
    /// integer nanoseconds, and the maximum number of catch-up runtime steps
    /// a single host frame may schedule. Rejects `max_steps_per_frame == 0`
    /// up front; the fixed step is validated through [`Self::validate`]
    /// against a kernel facade.
    pub const fn new(
        fixed_step_nanos: u64,
        max_steps_per_frame: u32,
    ) -> HostResult<HostBoundaryConfig> {
        if max_steps_per_frame == 0 {
            return Err(HostError::invalid_boundary_config(
                "host boundary max_steps_per_frame must be non-zero",
            ));
        }
        Ok(HostBoundaryConfig {
            fixed_step_nanos,
            max_steps_per_frame,
            step_while_hidden: false,
            retain_accumulator: true,
        })
    }

    /// If `true`, the driver continues to step the runtime while the host
    /// state reports `!visible`. Default: `false` (block stepping when
    /// hidden, which is the energy-conscious choice for a future browser
    /// adapter).
    pub const fn with_step_while_hidden(mut self, step: bool) -> Self {
        self.step_while_hidden = step;
        self
    }

    /// If `true` (default), unspent accumulated nanoseconds carry forward to
    /// the next host frame. If `false`, the accumulator is reset to zero
    /// after planning each frame — useful for hosts that want to discard
    /// stalls instead of compensating for them.
    pub const fn with_retain_accumulator(mut self, retain: bool) -> Self {
        self.retain_accumulator = retain;
        self
    }

    pub const fn fixed_step_nanos(&self) -> u64 {
        self.fixed_step_nanos
    }

    pub const fn max_steps_per_frame(&self) -> u32 {
        self.max_steps_per_frame
    }

    pub const fn step_while_hidden(&self) -> bool {
        self.step_while_hidden
    }

    pub const fn retain_accumulator(&self) -> bool {
        self.retain_accumulator
    }

    /// Validate the fixed step against the kernel. Any kernel rejection is
    /// wrapped as `InvalidBoundaryConfig` so the host layer's error model is
    /// the single source of truth at the boundary.
    pub fn validate(&self, kernel: &KernelApi) -> HostResult<()> {
        kernel.fixed_step(self.fixed_step_nanos).map_err(|_| {
            HostError::invalid_boundary_config(
                "host boundary fixed_step_nanos was rejected by the kernel",
            )
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;

    #[test]
    fn valid_config_creation() {
        let c = HostBoundaryConfig::new(16_666_667, 5).unwrap();
        assert_eq!(c.fixed_step_nanos(), 16_666_667);
        assert_eq!(c.max_steps_per_frame(), 5);
        assert!(!c.step_while_hidden());
        assert!(c.retain_accumulator());
    }

    #[test]
    fn zero_max_steps_fails() {
        let err = HostBoundaryConfig::new(16_666_667, 0).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidBoundaryConfig);
    }

    #[test]
    fn invalid_fixed_step_is_rejected_by_validate() {
        let c = HostBoundaryConfig::new(0, 1).unwrap();
        let err = c.validate(&KernelApi::new()).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidBoundaryConfig);
    }

    #[test]
    fn valid_fixed_step_passes_validate() {
        let c = HostBoundaryConfig::new(1_000, 1).unwrap();
        assert!(c.validate(&KernelApi::new()).is_ok());
    }

    #[test]
    fn step_while_hidden_policy_is_set_via_builder() {
        let c = HostBoundaryConfig::new(1_000, 1)
            .unwrap()
            .with_step_while_hidden(true);
        assert!(c.step_while_hidden());
    }

    #[test]
    fn retain_accumulator_policy_is_set_via_builder() {
        let c = HostBoundaryConfig::new(1_000, 1)
            .unwrap()
            .with_retain_accumulator(false);
        assert!(!c.retain_accumulator());
    }

    #[test]
    fn same_inputs_produce_equal_configs() {
        let a = HostBoundaryConfig::new(1_000, 3)
            .unwrap()
            .with_step_while_hidden(true)
            .with_retain_accumulator(false);
        let b = HostBoundaryConfig::new(1_000, 3)
            .unwrap()
            .with_step_while_hidden(true)
            .with_retain_accumulator(false);
        assert_eq!(a, b);
    }
}
