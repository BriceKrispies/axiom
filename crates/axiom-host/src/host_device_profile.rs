//! Abstract device capability profile for a host device request.

/// A deterministic, coarse capability profile for a future graphics device.
///
/// This intentionally does **not** mirror the WebGPU limits/features API. It
/// is a tiny abstract hint: a future adapter expands a profile into concrete
/// backend limits. Keeping it coarse means the host boundary stays stable as
/// real backend limit sets churn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostDeviceProfile {
    /// The minimum capability set sufficient to present the rotating-cube
    /// slice (a single pipeline, one mesh, one material).
    Baseline,
    /// A higher capability set for future content that needs larger limits.
    ExtendedLimits,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(
            HostDeviceProfile::Baseline,
            HostDeviceProfile::ExtendedLimits
        );
    }

    #[test]
    fn variants_are_copy_and_equal() {
        let p = HostDeviceProfile::Baseline;
        let q = p;
        assert_eq!(p, q);
    }
}
