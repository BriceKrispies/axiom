//! The engine's abstract request for a future graphics adapter.

use crate::host_power_preference::HostPowerPreference;

/// A pure-data request for a future graphics adapter.
///
/// Describes *what the engine wants* from adapter selection — a power
/// preference and whether the adapter must be able to drive a presentation
/// surface — without naming any WebGPU/OS adapter object. Every field
/// combination is valid, so there is no failure path here; a future adapter
/// consumes this to drive real adapter selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostAdapterRequest {
    power_preference: HostPowerPreference,
    require_presentation_surface: bool,
}

impl HostAdapterRequest {
    /// Construct an adapter request.
    pub const fn new(
        power_preference: HostPowerPreference,
        require_presentation_surface: bool,
    ) -> Self {
        HostAdapterRequest {
            power_preference,
            require_presentation_surface,
        }
    }

    pub const fn power_preference(&self) -> HostPowerPreference {
        self.power_preference
    }

    /// Whether the selected adapter must be able to present to a surface.
    pub const fn require_presentation_surface(&self) -> bool {
        self.require_presentation_surface
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_carries_its_fields() {
        let r = HostAdapterRequest::new(HostPowerPreference::HighPerformance, true);
        assert_eq!(r.power_preference(), HostPowerPreference::HighPerformance);
        assert!(r.require_presentation_surface());
    }

    #[test]
    fn same_inputs_produce_equal_requests() {
        let a = HostAdapterRequest::new(HostPowerPreference::LowPower, false);
        let b = HostAdapterRequest::new(HostPowerPreference::LowPower, false);
        assert_eq!(a, b);
    }
}
