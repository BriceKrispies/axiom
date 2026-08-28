//! The frame's **camera**: the matrices a backend draws with, and the intrinsics
//! it fits view volumes from.
//!
//! Its own file rather than a section of `frame_packet`, for the reason every
//! other `frame_*` concept in this layer has one. The two types here are one
//! pair: [`FrameCamera`] carries `view`, `projection` and their product — what a
//! backend needs to DRAW — and [`FrameCameraLens`] carries the fov, aspect,
//! planes and world pose those matrices were BUILT FROM, which is what a backend
//! needs to fit a volume to what the camera can see. The second is not derivable
//! from the first without inverting a product, which is the whole reason it is
//! stated rather than recovered.

/// The camera's **intrinsics**: the lens it looks through and the pose it looks
/// from, as first-class frame facts rather than as a product a consumer has to
/// take apart.
///
/// ## Why the frame states these rather than deriving them
///
/// [`FrameCamera`] carries `view`, `projection` and their product. Those three
/// are what a backend needs to *draw*; they are not what a backend needs to
/// *fit a volume to what the camera can see*. Fitting a shadow cascade, a
/// clustered light grid or a screen-space trace to the view frustum needs the
/// frustum's own parameters — the vertical field of view, the aspect, the near
/// and far planes, and where in the world the eye is — and every one of those is
/// destroyed by the multiply that produces `projection`.
///
/// They can be *recovered* by inverting the projection and the view, and that is
/// exactly the shortcut this type exists to remove. The intrinsics are not
/// derived quantities that happen to be convenient: they are the authored facts
/// the projection was *built from*, one layer up, by a scene camera that already
/// holds them as dimensioned values. Publishing the product and asking every
/// consumer to reconstruct the factors is how a frame contract accumulates
/// backends that each guess the fov slightly differently.
///
/// So the producer states them. Everything downstream that needs to reason about
/// the view *volume* rather than the view *transform* reads them directly, and
/// the reasoning stays inside the backend that does it — no cascade matrix, no
/// split plane and no fitted radius has to travel back down the contract.
///
/// `world` is the camera node's world matrix (`inverse(view)` for a rigid view,
/// but stated rather than inverted, for the same reason as above): a volume fit
/// pushes points expressed in view space through it to reach world space.
///
/// Every scalar is a dimensioned kernel quantity, so a caller cannot silently
/// hand degrees to a radians parameter or a normalized depth to a metres one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameCameraLens {
    fovy: axiom_kernel::Radians,
    aspect: axiom_kernel::Ratio,
    near: axiom_kernel::Meters,
    far: axiom_kernel::Meters,
    world: [f32; 16],
}

impl FrameCameraLens {
    /// The lens from the four perspective intrinsics and the camera's
    /// column-major world matrix.
    pub const fn new(
        fovy: axiom_kernel::Radians,
        aspect: axiom_kernel::Ratio,
        near: axiom_kernel::Meters,
        far: axiom_kernel::Meters,
        world: [f32; 16],
    ) -> Self {
        FrameCameraLens {
            fovy,
            aspect,
            near,
            far,
            world,
        }
    }

    /// The vertical field of view.
    pub const fn fovy(&self) -> axiom_kernel::Radians {
        self.fovy
    }

    /// Width over height of the frame the camera is projected into.
    pub const fn aspect(&self) -> axiom_kernel::Ratio {
        self.aspect
    }

    /// The near plane.
    pub const fn near(&self) -> axiom_kernel::Meters {
        self.near
    }

    /// The far plane.
    pub const fn far(&self) -> axiom_kernel::Meters {
        self.far
    }

    /// The camera node's column-major **world** matrix (not the view matrix).
    pub const fn world(&self) -> [f32; 16] {
        self.world
    }
}

/// The frame's camera matrices, all column-major 16-float arrays. `view_proj`
/// is the backend-neutral `projection * view`; a backend applies its own
/// depth-range convention when it consumes the packet.
///
/// Optionally carries the [`FrameCameraLens`] the projection was built from —
/// see that type for why the frame states the intrinsics instead of leaving
/// consumers to invert the matrices back into them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameCamera {
    view: [f32; 16],
    projection: [f32; 16],
    view_proj: [f32; 16],
    /// `None` when the producer genuinely has no intrinsics to state: the
    /// [`FrameCamera::IDENTITY`] stand-in for a frame with no camera at all, and
    /// any producer holding only a pair of matrices. A consumer that needs the
    /// view volume degrades rather than guessing — which is the whole point of
    /// making the absence representable instead of substituting a plausible fov.
    lens: Option<FrameCameraLens>,
}

impl FrameCamera {
    /// A camera from its column-major `view`, `projection`, and precomputed
    /// `view_proj` (`projection * view`) matrices.
    /// The stand-in for a frame that carries no camera: all three matrices
    /// identity.
    ///
    /// Deliberately not a derived `Default`, which would be all **zeros** — a
    /// zero projection is singular, and a consumer that inverts it (screen-space
    /// ambient occlusion does) would get infinities rather than a harmless
    /// no-op. Identity is the value the backends already substituted when they
    /// were handed a bare view-projection, so this preserves that behaviour
    /// under a name instead of at each call site.
    pub const IDENTITY: FrameCamera = {
        const I: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        FrameCamera::new(I, I, I)
    };

    pub const fn new(view: [f32; 16], projection: [f32; 16], view_proj: [f32; 16]) -> Self {
        FrameCamera {
            view,
            projection,
            view_proj,
            lens: None,
        }
    }

    /// State the intrinsics the `projection` was built from. Additive: a camera
    /// built without one carries `None` and every consumer behaves exactly as it
    /// did before the lens existed.
    pub const fn with_lens(mut self, lens: FrameCameraLens) -> Self {
        self.lens = Some(lens);
        self
    }

    /// The intrinsics this camera's projection was built from, when the producer
    /// stated them. See [`FrameCameraLens`].
    pub const fn lens(&self) -> Option<FrameCameraLens> {
        self.lens
    }

    /// The column-major view matrix.
    pub const fn view(&self) -> [f32; 16] {
        self.view
    }

    /// The column-major projection matrix.
    pub const fn projection(&self) -> [f32; 16] {
        self.projection
    }

    /// The column-major `projection * view` matrix.
    pub const fn view_proj(&self) -> [f32; 16] {
        self.view_proj
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A distinguishable 16-float matrix, the same shape `frame_packet`'s tests
    /// use.
    fn mat(seed: f32) -> [f32; 16] {
        core::array::from_fn(|i| seed + i as f32)
    }

    /// The intrinsics survive the packet, are readable back as the dimensioned
    /// quantities they were authored as, and are what distinguishes two cameras
    /// whose matrices happen to agree — which is exactly the case the lane
    /// exists for (two frames can share a projection and be fitted from
    /// different fovs only if one of them is lying).
    #[test]
    fn the_camera_lens_round_trips_and_participates_in_equality() {
        let lens = FrameCameraLens::new(
            axiom_kernel::Radians::finite_or_zero(80_f32.to_radians()),
            axiom_kernel::Ratio::finite_or_zero(16.0 / 9.0),
            axiom_kernel::Meters::finite_or_zero(0.5),
            axiom_kernel::Meters::finite_or_zero(300.0),
            mat(7.0),
        );
        assert_eq!(lens.fovy().get(), 80_f32.to_radians());
        assert_eq!(lens.aspect().get(), 16.0 / 9.0);
        assert_eq!(lens.near().get(), 0.5);
        assert_eq!(lens.far().get(), 300.0);
        assert_eq!(lens.world(), mat(7.0));
        assert!(format!("{lens:?}").contains("FrameCameraLens"));

        let bare = crate::FrameCamera::new(mat(1.0), mat(2.0), mat(3.0));
        let with = bare.with_lens(lens);
        assert_eq!(with.lens(), Some(lens));
        // The matrices are untouched by stating the intrinsics.
        assert_eq!(with.view(), bare.view());
        assert_eq!(with.projection(), bare.projection());
        assert_eq!(with.view_proj(), bare.view_proj());
        // Same matrices, different stated fov => different cameras.
        assert_ne!(with, bare);
        let narrower = bare.with_lens(FrameCameraLens::new(
            axiom_kernel::Radians::finite_or_zero(50_f32.to_radians()),
            axiom_kernel::Ratio::finite_or_zero(16.0 / 9.0),
            axiom_kernel::Meters::finite_or_zero(0.5),
            axiom_kernel::Meters::finite_or_zero(300.0),
            mat(7.0),
        ));
        assert_ne!(with, narrower);
        assert_ne!(lens, narrower.lens().unwrap());
    }

    #[test]
    fn camera_accessors_round_trip() {
        let c = FrameCamera::new(mat(1.0), mat(2.0), mat(3.0));
        assert_eq!(c.view(), mat(1.0));
        assert_eq!(c.projection(), mat(2.0));
        assert_eq!(c.view_proj(), mat(3.0));
        assert_eq!(c, FrameCamera::new(mat(1.0), mat(2.0), mat(3.0)));
        assert_ne!(c, FrameCamera::new(mat(1.0), mat(2.0), mat(9.0)));
        assert!(format!("{c:?}").contains("FrameCamera"));
        // A camera built from matrices alone states no intrinsics, and neither
        // does the no-camera stand-in. That absence is the whole reason the lane
        // is an `Option`: a consumer that needs the view *volume* has to see
        // "not stated" rather than a plausible substitute.
        assert_eq!(c.lens(), None);
        assert_eq!(FrameCamera::IDENTITY.lens(), None);
    }
}
