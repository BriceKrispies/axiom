//! Host-supplied viewport / surface metadata, validated as pure data.

use axiom_kernel::Ratio;

use crate::host_error::HostError;
use crate::host_orientation::Orientation;
use crate::host_result::HostResult;
use crate::host_safe_area_insets::HostSafeAreaInsets;
use crate::pixels::Pixels;

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
/// - `scale_factor` is a positive [`Ratio`] (the kernel quantity type already
///   guarantees finiteness at construction) and equals `physical / logical` in
///   each axis up to integer rounding the host has already applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostViewport {
    logical_width: u32,
    logical_height: u32,
    physical_width: u32,
    physical_height: u32,
    scale_factor: Ratio,
    safe_area_insets: HostSafeAreaInsets,
}

impl HostViewport {
    /// Construct from a logical size and a scale factor, deriving the
    /// physical size as `round(logical * scale_factor)`.
    ///
    /// Failure paths:
    /// - zero logical width or height → `InvalidViewportDimensions`,
    /// - non-positive scale factor → `InvalidScaleFactor`,
    /// - derived physical width or height of zero → `InvalidViewportDimensions`.
    ///
    /// Finiteness of the scale factor is guaranteed by the kernel [`Ratio`]
    /// type at its boundary — a non-finite scale can no longer even be
    /// constructed — so this constructor only enforces positivity and the
    /// dimension invariants.
    pub fn new(logical_width: u32, logical_height: u32, scale_factor: Ratio) -> HostResult<Self> {
        // `&`/`|` over the pure comparisons reproduces the original `||`/`&&`
        // truth tables without short-circuit control flow; every operand is a
        // total comparison with no side effect, so eager evaluation is exact.
        ((logical_width != 0) & (logical_height != 0))
            .then_some(())
            .ok_or_else(|| {
                HostError::invalid_viewport_dimensions(
                    "viewport logical width and height must be non-zero",
                )
            })
            .and_then(|()| {
                (scale_factor.get() > 0.0).then_some(()).ok_or_else(|| {
                    HostError::invalid_scale_factor("viewport scale factor must be positive")
                })
            })
            .and_then(|()| {
                let physical_width =
                    ((logical_width as f64) * (scale_factor.get() as f64)).round() as u64;
                let physical_height =
                    ((logical_height as f64) * (scale_factor.get() as f64)).round() as u64;
                ((physical_width != 0) & (physical_height != 0))
                    .then_some(())
                    .ok_or_else(|| {
                        HostError::invalid_viewport_dimensions(
                            "derived physical viewport dimensions must be non-zero",
                        )
                    })
                    .and_then(|()| {
                        ((physical_width <= u32::MAX as u64) & (physical_height <= u32::MAX as u64))
                            .then_some(())
                            .ok_or_else(|| {
                                HostError::invalid_viewport_dimensions(
                                    "derived physical viewport dimensions exceed u32::MAX",
                                )
                            })
                    })
                    .map(|()| HostViewport {
                        logical_width,
                        logical_height,
                        physical_width: physical_width as u32,
                        physical_height: physical_height as u32,
                        scale_factor,
                        safe_area_insets: HostSafeAreaInsets::none(),
                    })
            })
    }

    /// Construct from an explicit physical size and a scale factor (used by
    /// adapters that already know both the device-pixel surface and the
    /// device-independent logical size).
    pub fn from_physical(
        physical_width: u32,
        physical_height: u32,
        scale_factor: Ratio,
    ) -> HostResult<Self> {
        // Same branchless shape as `new`: `&`/`|` over total comparisons
        // reproduces the original `||` guards without short-circuit flow.
        ((physical_width != 0) & (physical_height != 0))
            .then_some(())
            .ok_or_else(|| {
                HostError::invalid_viewport_dimensions(
                    "viewport physical width and height must be non-zero",
                )
            })
            .and_then(|()| {
                (scale_factor.get() > 0.0).then_some(()).ok_or_else(|| {
                    HostError::invalid_scale_factor("viewport scale factor must be positive")
                })
            })
            .and_then(|()| {
                let logical_width =
                    ((physical_width as f64) / (scale_factor.get() as f64)).round() as u64;
                let logical_height =
                    ((physical_height as f64) / (scale_factor.get() as f64)).round() as u64;
                ((logical_width != 0) & (logical_height != 0))
                    .then_some(())
                    .ok_or_else(|| {
                        HostError::invalid_viewport_dimensions(
                            "derived logical viewport dimensions must be non-zero",
                        )
                    })
                    .and_then(|()| {
                        ((logical_width <= u32::MAX as u64) & (logical_height <= u32::MAX as u64))
                            .then_some(())
                            .ok_or_else(|| {
                                HostError::invalid_viewport_dimensions(
                                    "derived logical viewport dimensions exceed u32::MAX",
                                )
                            })
                    })
                    .map(|()| HostViewport {
                        logical_width: logical_width as u32,
                        logical_height: logical_height as u32,
                        physical_width,
                        physical_height,
                        scale_factor,
                        safe_area_insets: HostSafeAreaInsets::none(),
                    })
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

    pub const fn scale_factor(&self) -> Ratio {
        self.scale_factor
    }

    /// Attach host-supplied safe-area insets, replacing the default
    /// [`HostSafeAreaInsets::none`] this viewport was constructed with. Mirrors
    /// [`crate::HostFrameInput::with_presentation_nanos`]: the base constructors
    /// stay small, and the optional cutout fact is layered on by the adapter
    /// that actually has it.
    pub const fn with_safe_area_insets(mut self, insets: HostSafeAreaInsets) -> Self {
        self.safe_area_insets = insets;
        self
    }

    /// The host-supplied safe-area insets in effect for this surface. Defaults
    /// to [`HostSafeAreaInsets::none`] when the host supplied no cutout data.
    pub const fn safe_area_insets(&self) -> HostSafeAreaInsets {
        self.safe_area_insets
    }

    /// The surface orientation, derived from the physical pixel extents. A
    /// pure function of the dimensions the engine renders into, so it can never
    /// disagree with them (see [`Orientation`]).
    pub fn orientation(&self) -> Orientation {
        Orientation::from_extents(self.physical_width, self.physical_height)
    }

    /// `physical_width / physical_height` as a [`Ratio`]. Both dimensions are
    /// validated non-zero in the constructors, so the quotient is provably
    /// finite and the `Ratio::new` invariant cannot be violated here.
    pub fn aspect_ratio(&self) -> Ratio {
        Ratio::new((self.physical_width as f32) / (self.physical_height as f32))
            .expect("non-zero viewport dimensions guarantee a finite aspect ratio")
    }

    /// Logical-to-physical conversion of one axis, deterministic across
    /// architectures because it is rounded `f64` arithmetic.
    pub fn logical_to_physical(&self, value: Pixels) -> Pixels {
        let result = ((value.get() as f64) * (self.scale_factor.get() as f64)) as f32;
        Pixels::new(result).expect("a finite pixel value scaled by a finite scale factor is finite")
    }

    /// Physical-to-logical conversion of one axis. `self.scale_factor` is a
    /// positive, non-zero [`Ratio`] by the viewport's construction invariant,
    /// so dividing a finite pixel value by it stays finite.
    pub fn physical_to_logical(&self, value: Pixels) -> Pixels {
        let result = ((value.get() as f64) / (self.scale_factor.get() as f64)) as f32;
        Pixels::new(result)
            .expect("a finite pixel value divided by a finite positive scale factor is finite")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;

    fn ratio(value: f32) -> Ratio {
        Ratio::new(value).unwrap()
    }

    #[test]
    fn valid_viewport_creation() {
        let v = HostViewport::new(800, 600, ratio(2.0)).unwrap();
        assert_eq!(v.logical_width(), 800);
        assert_eq!(v.logical_height(), 600);
        assert_eq!(v.physical_width(), 1600);
        assert_eq!(v.physical_height(), 1200);
        assert_eq!(v.scale_factor(), ratio(2.0));
    }

    #[test]
    fn zero_logical_width_fails() {
        let err = HostViewport::new(0, 600, ratio(2.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn zero_logical_height_fails() {
        let err = HostViewport::new(800, 0, ratio(2.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn negative_scale_factor_fails() {
        let err = HostViewport::new(800, 600, ratio(-1.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidScaleFactor);
    }

    #[test]
    fn zero_scale_factor_fails() {
        let err = HostViewport::new(800, 600, ratio(0.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidScaleFactor);
    }

    #[test]
    fn aspect_ratio_is_stable_across_runs() {
        let v1 = HostViewport::new(1600, 900, ratio(1.0)).unwrap();
        let v2 = HostViewport::new(1600, 900, ratio(1.0)).unwrap();
        assert_eq!(v1.aspect_ratio(), v2.aspect_ratio());
        assert!((v1.aspect_ratio().get() - 16.0 / 9.0).abs() < 1.0e-6);
    }

    #[test]
    fn logical_to_physical_is_deterministic() {
        let v = HostViewport::new(100, 100, ratio(2.0)).unwrap();
        assert_eq!(
            v.logical_to_physical(Pixels::new(50.0).unwrap()),
            Pixels::new(100.0).unwrap()
        );
        // Identical inputs across two calls must match byte-for-byte.
        assert_eq!(
            v.logical_to_physical(Pixels::new(50.0).unwrap()),
            v.logical_to_physical(Pixels::new(50.0).unwrap())
        );
    }

    #[test]
    fn physical_to_logical_is_inverse_of_logical_to_physical() {
        let v = HostViewport::new(100, 100, ratio(2.5)).unwrap();
        let recovered = v.physical_to_logical(v.logical_to_physical(Pixels::new(40.0).unwrap()));
        assert!((recovered.get() - 40.0).abs() < 1.0e-3);
    }

    #[test]
    fn same_inputs_produce_equal_viewports() {
        let a = HostViewport::new(800, 600, ratio(1.5)).unwrap();
        let b = HostViewport::new(800, 600, ratio(1.5)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn from_physical_round_trips_with_new_for_integer_scale() {
        let from_logical = HostViewport::new(800, 600, ratio(2.0)).unwrap();
        let from_physical = HostViewport::from_physical(1600, 1200, ratio(2.0)).unwrap();
        assert_eq!(from_logical, from_physical);
    }

    #[test]
    fn from_physical_rejects_zero_dimensions() {
        let err = HostViewport::from_physical(0, 100, ratio(1.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn default_safe_area_insets_are_none() {
        let v = HostViewport::new(800, 600, ratio(1.0)).unwrap();
        assert_eq!(v.safe_area_insets(), HostSafeAreaInsets::none());
    }

    #[test]
    fn with_safe_area_insets_attaches_them() {
        let insets = HostSafeAreaInsets::new(
            Pixels::new(44.0).unwrap(),
            Pixels::new(0.0).unwrap(),
            Pixels::new(34.0).unwrap(),
            Pixels::new(0.0).unwrap(),
        )
        .unwrap();
        let v = HostViewport::new(390, 844, ratio(3.0))
            .unwrap()
            .with_safe_area_insets(insets);
        assert_eq!(v.safe_area_insets(), insets);
        // The other viewport facts survive the builder untouched.
        assert_eq!(v.logical_width(), 390);
        assert_eq!(v.logical_height(), 844);
    }

    #[test]
    fn orientation_tracks_physical_extents() {
        let landscape = HostViewport::new(1600, 900, ratio(1.0)).unwrap();
        assert_eq!(landscape.orientation(), Orientation::Landscape);
        let portrait = HostViewport::new(390, 844, ratio(1.0)).unwrap();
        assert_eq!(portrait.orientation(), Orientation::Portrait);
        let square = HostViewport::new(512, 512, ratio(1.0)).unwrap();
        assert_eq!(square.orientation(), Orientation::Square);
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use crate::host_error_code::HostErrorCode;

    fn ratio(value: f32) -> Ratio {
        Ratio::new(value).unwrap()
    }

    #[test]
    fn new_derived_physical_width_zero_fails() {
        // logical * tiny scale rounds to 0 physical width.
        let err = HostViewport::new(1, 1, ratio(1.0e-4)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn new_derived_physical_height_zero_fails() {
        // Width survives but height rounds to zero exercises the `||` rhs.
        let err = HostViewport::new(100_000, 1, ratio(1.0e-4)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn new_derived_physical_exceeds_u32_max_fails() {
        let err = HostViewport::new(u32::MAX, 1, ratio(4.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn new_derived_physical_height_exceeds_u32_max_fails() {
        // Width fits but height overflows exercises the `||` rhs.
        let err = HostViewport::new(1, u32::MAX, ratio(4.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn from_physical_zero_height_fails() {
        // Width survives, height zero exercises the `||` rhs.
        let err = HostViewport::from_physical(100, 0, ratio(1.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn from_physical_negative_scale_fails() {
        let err = HostViewport::from_physical(100, 100, ratio(-1.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidScaleFactor);
    }

    #[test]
    fn from_physical_zero_scale_fails() {
        let err = HostViewport::from_physical(100, 100, ratio(0.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidScaleFactor);
    }

    #[test]
    fn from_physical_derived_logical_zero_fails() {
        // physical / huge scale rounds logical to zero.
        let err = HostViewport::from_physical(1, 1, ratio(1.0e6)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn from_physical_derived_logical_height_zero_fails() {
        // Logical width survives, height rounds to zero exercises the `||` rhs.
        let err = HostViewport::from_physical(100_000, 1, ratio(1.0e3)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn from_physical_derived_logical_exceeds_u32_max_fails() {
        let err = HostViewport::from_physical(u32::MAX, 1, ratio(0.25)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn from_physical_derived_logical_height_exceeds_u32_max_fails() {
        // Logical width fits, height overflows exercises the `||` rhs.
        let err = HostViewport::from_physical(1, u32::MAX, ratio(0.25)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn from_physical_zero_width_reports_the_physical_zero_error_not_the_derived_one() {
        // Distinguishes `||` -> `&&` in the physical-zero guard. With `||`,
        // `physical_width == 0` short-circuits to the *physical* dimension
        // error message. With `&&`, the guard never fires for a single zero
        // and the code falls through to the *derived logical* error message.
        // Both share the `InvalidViewportDimensions` code, so the message is
        // the only thing that distinguishes the two operators.
        let err = HostViewport::from_physical(0, 100, ratio(1.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
        assert_eq!(
            err.message(),
            "viewport physical width and height must be non-zero"
        );
    }

    #[test]
    fn from_physical_derived_logical_width_exactly_u32_max_is_accepted() {
        // Boundary for the width comparison `logical_width > u32::MAX`: a scale
        // of 1.0 makes derived logical width exactly u32::MAX, which is in
        // range. The `>=` mutant would wrongly reject this exact value.
        let v = HostViewport::from_physical(u32::MAX, 8, ratio(1.0)).unwrap();
        assert_eq!(v.logical_width(), u32::MAX);
        assert_eq!(v.physical_width(), u32::MAX);
    }

    #[test]
    fn from_physical_derived_logical_height_exactly_u32_max_is_accepted() {
        // Boundary for the height comparison `logical_height > u32::MAX`: scale
        // 1.0 makes derived logical height exactly u32::MAX, in range. The
        // `>=` mutant would wrongly reject this exact value.
        let v = HostViewport::from_physical(8, u32::MAX, ratio(1.0)).unwrap();
        assert_eq!(v.logical_height(), u32::MAX);
        assert_eq!(v.physical_height(), u32::MAX);
    }
}
