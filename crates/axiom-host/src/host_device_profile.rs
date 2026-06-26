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
    ///
    /// This is the **mobile-first default** every caller picks today: its
    /// per-tier render parameters target the constrained device (a smaller
    /// shadow atlas, a capped render resolution). Content that genuinely needs
    /// more must opt up to [`HostDeviceProfile::ExtendedLimits`] — you opt *out*
    /// of the mobile budget, never silently into a desktop one.
    Baseline,
    /// A higher capability set for content that has the headroom for it (a
    /// larger shadow atlas, an uncapped render resolution).
    ExtendedLimits,
}

impl HostDeviceProfile {
    /// The shadow-map edge length, in texels, this tier renders the shadow
    /// depth pre-pass into. The pre-pass cost (and the 4-bytes-per-texel atlas
    /// memory) scales with the square of this, so the mobile-first
    /// [`Baseline`](Self::Baseline) tier halves it — `1024²` is a quarter the
    /// fragments and a quarter the VRAM of `2048²`, for a barely-perceptible
    /// change in soft-shadow quality at demo scale.
    ///
    /// Branchless: a fieldless enum's discriminant indexes the per-tier table.
    pub const fn shadow_map_size(self) -> u32 {
        [1024, 2048][self as usize]
    }

    /// The longest render-target edge, in device pixels, this tier will render
    /// the 3D scene at before presenting. A surface whose longest side exceeds
    /// this is rendered smaller (aspect-preserved) and upscaled on present —
    /// the single biggest GPU saving on a high-DPR phone, where the physical
    /// surface can be 3× the CSS size. The [`Baseline`](Self::Baseline) cap is
    /// high enough that ordinary desktop-sized surfaces are rendered 1:1 and
    /// only genuinely large (retina / mobile) surfaces are capped.
    pub const fn max_render_dimension(self) -> u32 {
        [1600, 4096][self as usize]
    }

    /// The render-target size for a `physical_width × physical_height` surface
    /// under this tier: the surface size itself when its longest edge is within
    /// [`Self::max_render_dimension`], otherwise scaled down to fit while
    /// preserving aspect ratio.
    ///
    /// Branchless and total: `capped = min(longest, max)` is the post-clamp
    /// long edge, and each axis is rescaled by `capped / longest` in widened
    /// integer arithmetic. A zero axis is floored to 1 so the result is always
    /// a usable, non-zero target (physical dimensions are validated non-zero
    /// upstream, so this only guards the degenerate case).
    pub fn render_size(self, physical_width: u32, physical_height: u32) -> (u32, u32) {
        let longest = physical_width.max(physical_height).max(1);
        let capped = longest.min(self.max_render_dimension());
        let scale =
            |axis: u32| (((axis as u64) * (capped as u64)) / (longest as u64)).max(1) as u32;
        (scale(physical_width), scale(physical_height))
    }
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

    #[test]
    fn baseline_uses_the_smaller_shadow_atlas() {
        assert_eq!(HostDeviceProfile::Baseline.shadow_map_size(), 1024);
        assert_eq!(HostDeviceProfile::ExtendedLimits.shadow_map_size(), 2048);
    }

    #[test]
    fn baseline_caps_the_render_dimension_lower() {
        assert_eq!(HostDeviceProfile::Baseline.max_render_dimension(), 1600);
        assert_eq!(
            HostDeviceProfile::ExtendedLimits.max_render_dimension(),
            4096
        );
    }

    #[test]
    fn render_size_leaves_a_within_cap_surface_untouched() {
        // The demo canvases (960×600) are well under every cap, so the engine's
        // default tier renders them 1:1 — no mobile-first change degrades them.
        assert_eq!(
            HostDeviceProfile::Baseline.render_size(960, 600),
            (960, 600)
        );
        assert_eq!(
            HostDeviceProfile::ExtendedLimits.render_size(960, 600),
            (960, 600)
        );
    }

    #[test]
    fn render_size_caps_a_large_landscape_surface_preserving_aspect() {
        // A 3000×1500 (2:1) surface on Baseline: longest 3000 > 1600, so it is
        // scaled to a 1600 long edge, 800 short edge — aspect preserved.
        assert_eq!(
            HostDeviceProfile::Baseline.render_size(3000, 1500),
            (1600, 800)
        );
        // ExtendedLimits' 4096 cap leaves the same surface untouched.
        assert_eq!(
            HostDeviceProfile::ExtendedLimits.render_size(3000, 1500),
            (3000, 1500)
        );
    }

    #[test]
    fn render_size_caps_a_tall_high_dpr_phone_surface() {
        // A 1170×2532 (≈ iPhone at DPR 3) surface on Baseline: longest 2532 >
        // 1600 → portrait long edge becomes 1600, width 1170*1600/2532 = 739.
        assert_eq!(
            HostDeviceProfile::Baseline.render_size(1170, 2532),
            (739, 1600)
        );
    }

    #[test]
    fn render_size_at_exactly_the_cap_is_unchanged() {
        // Boundary: longest edge == cap. `min(longest, cap)` keeps it, so the
        // surface renders 1:1. A `>`-vs-`>=` mutant would wrongly rescale here.
        assert_eq!(
            HostDeviceProfile::Baseline.render_size(1600, 900),
            (1600, 900)
        );
    }

    #[test]
    fn render_size_floors_a_degenerate_axis_to_one() {
        // Defensive: a zero axis (never produced by a validated viewport) still
        // yields a usable, non-zero target rather than a zero-sized texture.
        assert_eq!(HostDeviceProfile::Baseline.render_size(0, 0), (1, 1));
    }
}
