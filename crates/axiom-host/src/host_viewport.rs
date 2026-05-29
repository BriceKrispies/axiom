//! Host-supplied viewport / surface metadata, validated as pure data.

use axiom_math::MathApi;

use crate::host_error::HostError;
use crate::host_result::HostResult;

/// Viewport / surface metadata supplied by the host.
///
/// This is **data only**. It does not reference a DOM canvas, a window
/// handle, a swapchain, or any browser/OS object — those belong to a future
/// adapter layer that has not been built yet. The host boundary takes the
/// three explicit inputs that drive deterministic engine math (logical size,
/// physical size, scale factor) and validates them.
///
/// Conventions:
/// - `logical_width` / `logical_height` are non-zero `u32`s and represent
///   the device-independent surface in the host's coordinate system.
/// - `physical_width` / `physical_height` are non-zero `u32`s and represent
///   the surface in actual device pixels.
/// - `scale_factor` is a finite positive `f32` (validated through
///   [`MathApi::validate_finite`]) and equals `physical / logical` in each
///   axis up to integer rounding the host has already applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostViewport {
    logical_width: u32,
    logical_height: u32,
    physical_width: u32,
    physical_height: u32,
    scale_factor: f32,
}

impl HostViewport {
    /// Construct from a logical size and a scale factor, deriving the
    /// physical size as `round(logical * scale_factor)`.
    ///
    /// Failure paths:
    /// - zero logical width or height → `InvalidViewportDimensions`,
    /// - non-finite or non-positive scale factor → `InvalidScaleFactor`,
    /// - derived physical width or height of zero → `InvalidViewportDimensions`.
    ///
    /// Math validation routes through [`MathApi::validate_finite`], which is
    /// what makes this constructor a Layer-03 semantic adapter over Layer-02
    /// math (rather than a hand-rolled `is_finite` check).
    pub fn new(
        math: &MathApi,
        logical_width: u32,
        logical_height: u32,
        scale_factor: f32,
    ) -> HostResult<Self> {
        if logical_width == 0 || logical_height == 0 {
            return Err(HostError::invalid_viewport_dimensions(
                "viewport logical width and height must be non-zero",
            ));
        }
        math.validate_finite(scale_factor)
            .map_err(|_| HostError::invalid_scale_factor("viewport scale factor must be finite"))?;
        if scale_factor <= 0.0 {
            return Err(HostError::invalid_scale_factor(
                "viewport scale factor must be positive",
            ));
        }
        let physical_width = ((logical_width as f64) * (scale_factor as f64)).round() as u64;
        let physical_height = ((logical_height as f64) * (scale_factor as f64)).round() as u64;
        if physical_width == 0 || physical_height == 0 {
            return Err(HostError::invalid_viewport_dimensions(
                "derived physical viewport dimensions must be non-zero",
            ));
        }
        if physical_width > u32::MAX as u64 || physical_height > u32::MAX as u64 {
            return Err(HostError::invalid_viewport_dimensions(
                "derived physical viewport dimensions exceed u32::MAX",
            ));
        }
        Ok(HostViewport {
            logical_width,
            logical_height,
            physical_width: physical_width as u32,
            physical_height: physical_height as u32,
            scale_factor,
        })
    }

    /// Construct from an explicit physical size and a scale factor (used by
    /// adapters that already know both the device-pixel surface and the
    /// device-independent logical size).
    pub fn from_physical(
        math: &MathApi,
        physical_width: u32,
        physical_height: u32,
        scale_factor: f32,
    ) -> HostResult<Self> {
        if physical_width == 0 || physical_height == 0 {
            return Err(HostError::invalid_viewport_dimensions(
                "viewport physical width and height must be non-zero",
            ));
        }
        math.validate_finite(scale_factor)
            .map_err(|_| HostError::invalid_scale_factor("viewport scale factor must be finite"))?;
        if scale_factor <= 0.0 {
            return Err(HostError::invalid_scale_factor(
                "viewport scale factor must be positive",
            ));
        }
        let logical_width = ((physical_width as f64) / (scale_factor as f64)).round() as u64;
        let logical_height = ((physical_height as f64) / (scale_factor as f64)).round() as u64;
        if logical_width == 0 || logical_height == 0 {
            return Err(HostError::invalid_viewport_dimensions(
                "derived logical viewport dimensions must be non-zero",
            ));
        }
        if logical_width > u32::MAX as u64 || logical_height > u32::MAX as u64 {
            return Err(HostError::invalid_viewport_dimensions(
                "derived logical viewport dimensions exceed u32::MAX",
            ));
        }
        Ok(HostViewport {
            logical_width: logical_width as u32,
            logical_height: logical_height as u32,
            physical_width,
            physical_height,
            scale_factor,
        })
    }

    pub const fn logical_width(&self) -> u32 {
        self.logical_width
    }

    pub const fn logical_height(&self) -> u32 {
        self.logical_height
    }

    pub const fn physical_width(&self) -> u32 {
        self.physical_width
    }

    pub const fn physical_height(&self) -> u32 {
        self.physical_height
    }

    pub const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// `physical_width / physical_height` as `f32`. Always finite and
    /// positive because the constructors reject zero dimensions.
    pub fn aspect_ratio(&self) -> f32 {
        (self.physical_width as f32) / (self.physical_height as f32)
    }

    /// Logical-to-physical conversion of one axis, deterministic across
    /// architectures because it is rounded `f64` arithmetic.
    pub fn logical_to_physical(&self, value: f32) -> f32 {
        ((value as f64) * (self.scale_factor as f64)) as f32
    }

    /// Physical-to-logical conversion of one axis.
    pub fn physical_to_logical(&self, value: f32) -> f32 {
        ((value as f64) / (self.scale_factor as f64)) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;

    fn math() -> MathApi {
        MathApi::new()
    }

    #[test]
    fn valid_viewport_creation() {
        let v = HostViewport::new(&math(), 800, 600, 2.0).unwrap();
        assert_eq!(v.logical_width(), 800);
        assert_eq!(v.logical_height(), 600);
        assert_eq!(v.physical_width(), 1600);
        assert_eq!(v.physical_height(), 1200);
        assert_eq!(v.scale_factor(), 2.0);
    }

    #[test]
    fn zero_logical_width_fails() {
        let err = HostViewport::new(&math(), 0, 600, 2.0).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn zero_logical_height_fails() {
        let err = HostViewport::new(&math(), 800, 0, 2.0).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn negative_scale_factor_fails() {
        let err = HostViewport::new(&math(), 800, 600, -1.0).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidScaleFactor);
    }

    #[test]
    fn zero_scale_factor_fails() {
        let err = HostViewport::new(&math(), 800, 600, 0.0).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidScaleFactor);
    }

    #[test]
    fn nan_scale_factor_fails() {
        let err = HostViewport::new(&math(), 800, 600, f32::NAN).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidScaleFactor);
    }

    #[test]
    fn infinity_scale_factor_fails() {
        let err = HostViewport::new(&math(), 800, 600, f32::INFINITY).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidScaleFactor);
    }

    #[test]
    fn aspect_ratio_is_stable_across_runs() {
        let v1 = HostViewport::new(&math(), 1600, 900, 1.0).unwrap();
        let v2 = HostViewport::new(&math(), 1600, 900, 1.0).unwrap();
        assert_eq!(v1.aspect_ratio(), v2.aspect_ratio());
        assert!((v1.aspect_ratio() - 16.0 / 9.0).abs() < 1.0e-6);
    }

    #[test]
    fn logical_to_physical_is_deterministic() {
        let v = HostViewport::new(&math(), 100, 100, 2.0).unwrap();
        assert_eq!(v.logical_to_physical(50.0), 100.0);
        // Identical inputs across two calls must match byte-for-byte.
        assert_eq!(v.logical_to_physical(50.0), v.logical_to_physical(50.0));
    }

    #[test]
    fn physical_to_logical_is_inverse_of_logical_to_physical() {
        let v = HostViewport::new(&math(), 100, 100, 2.5).unwrap();
        let recovered = v.physical_to_logical(v.logical_to_physical(40.0));
        assert!((recovered - 40.0).abs() < 1.0e-3);
    }

    #[test]
    fn same_inputs_produce_equal_viewports() {
        let a = HostViewport::new(&math(), 800, 600, 1.5).unwrap();
        let b = HostViewport::new(&math(), 800, 600, 1.5).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn from_physical_round_trips_with_new_for_integer_scale() {
        let math = math();
        let from_logical = HostViewport::new(&math, 800, 600, 2.0).unwrap();
        let from_physical = HostViewport::from_physical(&math, 1600, 1200, 2.0).unwrap();
        assert_eq!(from_logical, from_physical);
    }

    #[test]
    fn from_physical_rejects_zero_dimensions() {
        let err = HostViewport::from_physical(&math(), 0, 100, 1.0).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn from_physical_rejects_non_finite_scale() {
        let err = HostViewport::from_physical(&math(), 100, 100, f32::NAN).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidScaleFactor);
    }
}
