//! A validated binding of a target + surface + viewport + adapter/device
//! requests, ready for a future live backend to consume.

use crate::host_adapter_request::HostAdapterRequest;
use crate::host_device_request::HostDeviceRequest;
use crate::host_error::HostError;
use crate::host_presentation_target::HostPresentationTarget;
use crate::host_result::HostResult;
use crate::host_surface_descriptor::HostSurfaceDescriptor;
use crate::host_surface_handle::HostSurfaceHandle;

/// A validated presentation request: it binds a [`HostPresentationTarget`]
/// and a [`HostSurfaceHandle`] to a [`HostSurfaceDescriptor`] (the surface
/// shape) plus a [`HostAdapterRequest`] and a [`HostDeviceRequest`].
///
/// This is the artifact a future `axiom-webgpu` live mode will consume to
/// build a real adapter/device/surface — entirely from deterministic
/// host-owned data, with no browser/GPU objects anywhere.
///
/// Construct one only through [`crate::HostApi::presentation_request`], which
/// validates the binding. Its constructor is crate-private.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostPresentationRequest {
    target: HostPresentationTarget,
    surface: HostSurfaceHandle,
    descriptor: HostSurfaceDescriptor,
    adapter: HostAdapterRequest,
    device: HostDeviceRequest,
}

impl HostPresentationRequest {
    /// Validate and bind the request. Crate-private: callers go through
    /// [`crate::HostApi::presentation_request`].
    ///
    /// Failure path (`InvalidPresentationRequest`): a device that requires
    /// presentation while the adapter request does not require a
    /// presentation-capable surface (an inconsistent binding that could never
    /// be satisfied by a real backend). The target and surface handles are
    /// already validated by their own constructors.
    pub(crate) fn new(
        target: HostPresentationTarget,
        surface: HostSurfaceHandle,
        descriptor: HostSurfaceDescriptor,
        adapter: HostAdapterRequest,
        device: HostDeviceRequest,
    ) -> HostResult<Self> {
        // `target` and `surface` are already guaranteed valid by their own
        // constructors (`HostPresentationTarget::new` / `HostSurfaceHandle::new`
        // reject null ids and the fields are private), so the only binding
        // failure that can actually occur is an inconsistent adapter/device
        // pairing.
        let inconsistent =
            device.require_presentation() & !adapter.require_presentation_surface();
        (!inconsistent)
            .then_some(HostPresentationRequest {
                target,
                surface,
                descriptor,
                adapter,
                device,
            })
            .ok_or_else(|| {
                HostError::invalid_presentation_request(
                    "device requires presentation but the adapter request does not \
                     require a presentation-capable surface",
                )
            })
    }

    pub const fn target(&self) -> HostPresentationTarget {
        self.target
    }

    pub const fn surface(&self) -> HostSurfaceHandle {
        self.surface
    }

    pub const fn descriptor(&self) -> &HostSurfaceDescriptor {
        &self.descriptor
    }

    pub const fn adapter(&self) -> HostAdapterRequest {
        self.adapter
    }

    pub const fn device(&self) -> HostDeviceRequest {
        self.device
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_alpha_mode::HostAlphaMode;
    use crate::host_color_format::HostColorFormat;
    use crate::host_device_profile::HostDeviceProfile;
    use crate::host_error_code::HostErrorCode;
    use crate::host_power_preference::HostPowerPreference;
    use crate::host_present_mode::HostPresentMode;
    use crate::host_viewport::HostViewport;
    use axiom_kernel::HandleId;
    use axiom_kernel::Ratio;

    fn descriptor() -> HostSurfaceDescriptor {
        let viewport = HostViewport::new(800, 600, Ratio::new(1.0).unwrap()).unwrap();
        HostSurfaceDescriptor::new(
            viewport,
            HostPresentMode::Fifo,
            HostAlphaMode::Opaque,
            HostColorFormat::Bgra8UnormSrgb,
        )
    }

    fn target() -> HostPresentationTarget {
        HostPresentationTarget::new(HandleId::from_raw(1), "primary").unwrap()
    }

    fn surface() -> HostSurfaceHandle {
        HostSurfaceHandle::new(HandleId::from_raw(2)).unwrap()
    }

    #[test]
    fn valid_request_binds_all_parts() {
        let req = HostPresentationRequest::new(
            target(),
            surface(),
            descriptor(),
            HostAdapterRequest::new(HostPowerPreference::HighPerformance, true),
            HostDeviceRequest::new(true, HostDeviceProfile::Baseline),
        )
        .unwrap();
        assert_eq!(req.target().id().raw(), 1);
        assert_eq!(req.surface().id().raw(), 2);
        assert_eq!(req.descriptor().viewport().logical_width(), 800);
    }

    #[test]
    fn inconsistent_presentation_requirement_is_rejected() {
        let err = HostPresentationRequest::new(
            target(),
            surface(),
            descriptor(),
            // adapter does NOT require a presentation surface ...
            HostAdapterRequest::new(HostPowerPreference::Default, false),
            // ... but the device requires presentation.
            HostDeviceRequest::new(true, HostDeviceProfile::Baseline),
        )
        .unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidPresentationRequest);
    }

    #[test]
    fn identical_requests_compare_equal() {
        let build = || {
            HostPresentationRequest::new(
                target(),
                surface(),
                descriptor(),
                HostAdapterRequest::new(HostPowerPreference::Default, true),
                HostDeviceRequest::new(false, HostDeviceProfile::Baseline),
            )
            .unwrap()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn adapter_and_device_accessors_round_trip() {
        let adapter = HostAdapterRequest::new(HostPowerPreference::LowPower, true);
        let device = HostDeviceRequest::new(false, HostDeviceProfile::Baseline);
        let req = HostPresentationRequest::new(target(), surface(), descriptor(), adapter, device)
            .unwrap();
        assert_eq!(req.adapter(), adapter);
        assert_eq!(req.device(), device);
    }
}
