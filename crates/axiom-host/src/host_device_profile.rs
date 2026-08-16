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
    /// larger shadow atlas, and a **supersampled** render target rather than a
    /// capped one — see [`Self::render_supersample`]).
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

    /// How many render-target samples this tier lays down **per surface pixel
    /// per axis** before the frame is resolved back down on present — the
    /// render-scale path's upward direction, and the engine's only geometric
    /// anti-aliasing.
    ///
    /// Nothing else in the renderer anti-aliases: the scene pass is one sample
    /// per pixel, so every near-vertical edge stair-steps in runs of `1/slope`
    /// pixels. A lane marking receding to a vanishing point is exactly that
    /// case, and no amount of shading, material or camera work can remove it —
    /// it is a sampling-rate defect and only a sampling-rate change fixes it.
    ///
    /// [`Baseline`](Self::Baseline) is `1`: the mobile-first tier never spends
    /// fill rate it was not asked for, so its render target is *bit-for-bit*
    /// what it was before this existed. [`ExtendedLimits`](Self::ExtendedLimits)
    /// is `2` — 4× the fragments, and 4 coverage samples per presented pixel,
    /// because a 2× target resolved through the present filter's linear
    /// minification is an exact 2×2 box average. That is the opt-up bargain the
    /// tier already describes: you pay for headroom, you get the image quality
    /// the headroom buys.
    ///
    /// Branchless: a fieldless enum's discriminant indexes the per-tier table.
    pub const fn render_supersample(self) -> u32 {
        [1, 2][self as usize]
    }

    /// The highest anisotropy this tier will ask a material sampler for.
    ///
    /// Anisotropic filtering costs **taps**: a sampler at 16× fetches up to
    /// sixteen texels per pixel instead of one, and it spends the full budget
    /// exactly where the footprint ratio is most extreme — a road surface
    /// receding to the horizon, which is most of the screen in a driving game.
    /// That is affordable on a desktop GPU and is not affordable on a phone, so
    /// it is a **tier budget**, sitting here beside the shadow atlas and the
    /// supersample rate rather than being inferred somewhere downstream.
    ///
    /// It used to be inferred, and the inference was the bug. The GPU backend
    /// resolved anisotropy from `DownlevelFlags::ANISOTROPIC_FILTERING` — a
    /// *compliance* question — and wgpu answers that flag for the WebGPU backend
    /// with `DownlevelCapabilities::default()` on the stated assumption that
    /// "WebGPU is assumed to be fully compliant". It is never measured, so it is
    /// `true` on the weakest phone as readily as on a workstation. The WebGL2
    /// arm, which genuinely queries `EXT_texture_filter_anisotropic`, could
    /// answer `false` on the very same device — so one browser arm took 16 taps
    /// per road pixel and the other took one, on identical hardware, for a
    /// visually near-identical frame.
    ///
    /// Capability says what a device *may* do. Only the tier says what it should
    /// be asked to *afford*. The backend now takes the smaller of the two.
    ///
    /// Branchless: a fieldless enum's discriminant indexes the per-tier table.
    pub const fn max_anisotropy(self) -> u16 {
        [4, 16][self as usize]
    }

    /// This tier, lowered to what a surface at `scale` device pixels per CSS
    /// pixel can afford — never raised.
    ///
    /// [`Self::render_supersample`] exists to resolve a *CSS-pixel* artifact:
    /// thin near-vertical edges stair-step because the scene is rendered at one
    /// sample per pixel. A high-DPR display has already fixed that. A phone at
    /// `scale = 3` puts three physical pixels behind every CSS pixel, so the
    /// panel is already oversampling by more than the tier was going to buy —
    /// and stacking a 2× supersample on top pays **four times the fill rate** for
    /// antialiasing the display is doing for free.
    ///
    /// Left unchecked the intent inverted completely. An upright phone (a 915 pt
    /// viewport at `scale = 3`) measures 2745 physical pixels on its long edge;
    /// [`ExtendedLimits`](Self::ExtendedLimits) doubles that to 5490 and its own
    /// 4096 cap takes it to 4096. A desktop canvas capped at 1180 CSS px at
    /// `scale = 1` doubles to 2360 and is under the cap. **The phone was
    /// rendering a target 3× the area of the desktop's** — on the tier documented
    /// as the one you opt into when you have the headroom.
    ///
    /// So the opt-up is honoured where it is affordable and withdrawn where the
    /// display has already paid for it. This only ever lowers a tier, so an app
    /// that asked for [`Baseline`](Self::Baseline) is unaffected and no app can
    /// be handed more than it requested.
    ///
    /// Branchless: the two outcomes are a two-element table indexed by the
    /// comparison.
    pub const fn afforded_at_scale(self, scale: axiom_kernel::Ratio) -> HostDeviceProfile {
        // 2.0 is the point at which the display's own oversampling meets what
        // `render_supersample` would add, so anything at or above it is paying
        // twice for the same edge.
        [self, HostDeviceProfile::Baseline][(scale.get() >= 2.0) as usize]
    }

    /// The render-target size for a `physical_width × physical_height` surface
    /// under this tier: the surface scaled by [`Self::render_supersample`], then
    /// clamped so its longest edge is within [`Self::max_render_dimension`],
    /// preserving aspect ratio. A [`Baseline`](Self::Baseline) surface within
    /// the cap therefore renders 1:1, exactly as before; a supersampling tier
    /// renders larger and the present resolve does the downsample.
    ///
    /// Branchless and total: `capped = min(longest * supersample, max)` is the
    /// post-clamp long edge, and each axis is rescaled by `capped / longest` in
    /// widened integer arithmetic. A zero axis is floored to 1 so the result is
    /// always a usable, non-zero target (physical dimensions are validated
    /// non-zero upstream, so this only guards the degenerate case).
    pub fn render_size(self, physical_width: u32, physical_height: u32) -> (u32, u32) {
        let longest = physical_width.max(physical_height).max(1);
        let sampled = longest.saturating_mul(self.render_supersample());
        let capped = sampled.min(self.max_render_dimension());
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
    fn baseline_renders_one_sample_per_pixel_extended_supersamples() {
        assert_eq!(HostDeviceProfile::Baseline.render_supersample(), 1);
        assert_eq!(HostDeviceProfile::ExtendedLimits.render_supersample(), 2);
    }

    #[test]
    fn baseline_asks_for_less_anisotropy_than_the_opt_up_tier() {
        // The mobile tier still gets anisotropic filtering — a road that recedes
        // needs it or the grain washes to flat grey — it just does not get to
        // spend sixteen taps a pixel on it.
        assert_eq!(HostDeviceProfile::Baseline.max_anisotropy(), 4);
        assert_eq!(HostDeviceProfile::ExtendedLimits.max_anisotropy(), 16);
        assert!(
            HostDeviceProfile::Baseline.max_anisotropy()
                < HostDeviceProfile::ExtendedLimits.max_anisotropy(),
            "the mobile-first tier must never ask for more taps than the opt-up"
        );
    }

    /// The whole point of the affordability rule: a dense display has already
    /// bought the antialiasing the supersample was going to add.
    #[test]
    fn a_dense_display_withdraws_the_supersampling_opt_up() {
        let phone = axiom_kernel::Ratio::new(3.0).expect("finite");
        assert_eq!(
            HostDeviceProfile::ExtendedLimits.afforded_at_scale(phone),
            HostDeviceProfile::Baseline,
            "a 3x phone panel already oversamples; a 2x supersample on top is 4x fill for nothing"
        );
        // Exactly at the threshold the display's oversampling equals what the
        // tier would add, so the tier stops paying for it.
        let retina = axiom_kernel::Ratio::new(2.0).expect("finite");
        assert_eq!(
            HostDeviceProfile::ExtendedLimits.afforded_at_scale(retina),
            HostDeviceProfile::Baseline
        );
    }

    #[test]
    fn a_one_to_one_display_keeps_the_tier_it_asked_for() {
        let desktop = axiom_kernel::Ratio::new(1.0).expect("finite");
        assert_eq!(
            HostDeviceProfile::ExtendedLimits.afforded_at_scale(desktop),
            HostDeviceProfile::ExtendedLimits,
            "nothing has oversampled for this display, so the opt-up still earns its cost"
        );
    }

    /// The rule only ever lowers. An app on the mobile tier cannot be silently
    /// promoted by a display that happens to be dense or sparse.
    #[test]
    fn affordability_never_raises_a_tier() {
        [0.5f32, 1.0, 1.5, 2.0, 3.0, 4.0].iter().for_each(|&s| {
            let scale = axiom_kernel::Ratio::new(s).expect("finite");
            assert_eq!(
                HostDeviceProfile::Baseline.afforded_at_scale(scale),
                HostDeviceProfile::Baseline,
                "baseline stayed baseline at scale {s}"
            );
        });
    }

    #[test]
    fn render_size_leaves_a_within_cap_baseline_surface_untouched() {
        // The demo canvases (960×600) are well under every cap, so the engine's
        // default tier renders them 1:1 — no mobile-first change degrades them,
        // and adding supersampling did not move the tier every app is on.
        assert_eq!(
            HostDeviceProfile::Baseline.render_size(960, 600),
            (960, 600)
        );
        // The opt-up tier lays down 2× per axis on the same surface: 4 coverage
        // samples per presented pixel, aspect preserved.
        assert_eq!(
            HostDeviceProfile::ExtendedLimits.render_size(960, 600),
            (1920, 1200)
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
        // ExtendedLimits asks for 2× (6000) and its 4096 cap takes it back down:
        // the supersample is a *request*, the tier cap is still the ceiling, and
        // the aspect survives both.
        assert_eq!(
            HostDeviceProfile::ExtendedLimits.render_size(3000, 1500),
            (4096, 2048)
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
    fn a_supersampled_extreme_surface_saturates_instead_of_wrapping() {
        // The supersample multiply is the one place a surface dimension is
        // scaled UP, so it is the one place a `u32` could wrap. It saturates,
        // and the tier cap then bounds it — a wrapping multiply would produce a
        // tiny (or zero) target and a black frame.
        assert_eq!(
            HostDeviceProfile::ExtendedLimits.render_size(u32::MAX, 1),
            (4096, 1)
        );
    }

    #[test]
    fn render_size_floors_a_degenerate_axis_to_one() {
        // Defensive: a zero axis (never produced by a validated viewport) still
        // yields a usable, non-zero target rather than a zero-sized texture.
        assert_eq!(HostDeviceProfile::Baseline.render_size(0, 0), (1, 1));
    }
}
