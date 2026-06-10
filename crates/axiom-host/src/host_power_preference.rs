//! Abstract adapter power preference for a host adapter request.

/// The power/performance trade-off the engine prefers when a future adapter
/// is selected.
///
/// Abstract host-boundary enum: a future adapter maps these onto the real
/// backend's adapter-selection hint. The host layer never names a WebGPU/OS
/// type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostPowerPreference {
    /// No preference; let the backend choose.
    Default,
    /// Prefer a lower-power adapter (e.g. integrated graphics).
    LowPower,
    /// Prefer a higher-performance adapter (e.g. discrete graphics).
    HighPerformance,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(HostPowerPreference::Default, HostPowerPreference::LowPower);
        assert_ne!(
            HostPowerPreference::LowPower,
            HostPowerPreference::HighPerformance
        );
        assert_ne!(
            HostPowerPreference::Default,
            HostPowerPreference::HighPerformance
        );
    }

    #[test]
    fn variants_are_copy_and_equal() {
        let p = HostPowerPreference::HighPerformance;
        let q = p;
        assert_eq!(p, q);
    }
}
