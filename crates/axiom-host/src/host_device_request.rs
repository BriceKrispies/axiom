//! The engine's abstract request for a future graphics device.

use crate::host_device_profile::HostDeviceProfile;

/// A pure-data request for a future graphics device.
///
/// Intentionally tiny: it carries only whether the device must support
/// presentation and a coarse [`HostDeviceProfile`]. It does **not** mirror
/// the WebGPU device-descriptor / limits / features surface — a future
/// adapter expands the profile into concrete backend limits. Every field
/// combination is valid, so there is no failure path here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostDeviceRequest {
    require_presentation: bool,
    profile: HostDeviceProfile,
}

impl HostDeviceRequest {
    /// Construct a device request.
    pub const fn new(require_presentation: bool, profile: HostDeviceProfile) -> Self {
        HostDeviceRequest {
            require_presentation,
            profile,
        }
    }

    /// Whether the device must be able to drive presentation.
    pub const fn require_presentation(&self) -> bool {
        self.require_presentation
    }

    pub const fn profile(&self) -> HostDeviceProfile {
        self.profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_carries_its_fields() {
        let r = HostDeviceRequest::new(true, HostDeviceProfile::Baseline);
        assert!(r.require_presentation());
        assert_eq!(r.profile(), HostDeviceProfile::Baseline);
    }

    #[test]
    fn require_presentation_reflects_false_input() {
        // Distinguishes `require_presentation -> true`: a request built with
        // `false` must report `false`.
        let r = HostDeviceRequest::new(false, HostDeviceProfile::Baseline);
        assert!(!r.require_presentation());
    }

    #[test]
    fn same_inputs_produce_equal_requests() {
        let a = HostDeviceRequest::new(false, HostDeviceProfile::ExtendedLimits);
        let b = HostDeviceRequest::new(false, HostDeviceProfile::ExtendedLimits);
        assert_eq!(a, b);
    }
}
