//! `HostSessionConfig`: the validated inbound session identity (seed + params).

use crate::host_session_params::HostSessionParams;

/// The inbound half of the embed seam (SPEC-12 §5): a deterministic `seed` plus
/// opaque session [`HostSessionParams`].
/// The platform arm decodes a URL query / parent `postMessage` payload / JWT
/// claim into this shape; the host boundary only validates and carries it — it
/// never parses a query string or reads a clock. The `seed` is the determinism
/// input (SPEC-12 §6): it must be fixed before tick 0 and is immutable for the
/// session, so two configs built from equal inputs are equal.
#[derive(Debug, Clone, PartialEq)]
pub struct HostSessionConfig {
    seed: u64,
    params: HostSessionParams,
}

impl HostSessionConfig {
    /// Carry a `seed` and already-decoded opaque `params` as a session config.
    pub fn new(seed: u64, params: HostSessionParams) -> Self {
        HostSessionConfig { seed, params }
    }

    /// The deterministic session seed (feeds the sim's `Rng`, SPEC-01).
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// The opaque session parameters, in stable order.
    pub fn params(&self) -> &HostSessionParams {
        &self.params
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_param_value::HostParamValue;

    fn params() -> HostSessionParams {
        HostSessionParams::new().with(String::from("uid"), HostParamValue::Text(String::from("p7")))
    }

    #[test]
    fn config_carries_seed_and_params() {
        let config = HostSessionConfig::new(42, params());
        assert_eq!(config.seed(), 42);
        assert_eq!(config.params(), &params());
    }

    #[test]
    fn equal_inputs_build_equal_configs() {
        assert_eq!(
            HostSessionConfig::new(42, params()),
            HostSessionConfig::new(42, params())
        );
        assert_ne!(
            HostSessionConfig::new(42, params()),
            HostSessionConfig::new(43, params())
        );
    }
}
