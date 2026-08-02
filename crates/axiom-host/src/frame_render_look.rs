//! The **render look**: the app-authored, backend-neutral description of how a
//! frame is lit and graded, bound once rather than authored per draw.
//!
//! Four things travel together everywhere the engine binds a backend — the
//! hemisphere ambient that fills unlit faces, the atmospheric depth fog distance
//! recedes into, the sky behind the scene, and the bloom that decides how bright
//! things spill. They are one concept: *what this world looks like*. They were
//! already travelling as a de-facto tuple through the windowing driver, the GPU
//! backend facade, the live binding and the scene renderer, and every new look
//! knob widened four signatures and about a dozen call sites — most of them in
//! `wasm32`-only code the native test gate never compiles.
//!
//! Naming the bundle is what stops that. A future look parameter is a field
//! here plus the one backend that realizes it, not another positional argument
//! threaded through code no test can reach.
//!
//! Each part is **optional except the ambient**, which always has a value
//! because every backend needs *some* fill light; the engine default hemisphere
//! is that value. An absent part means "the app authored none", and every
//! backend treats that as an exact no-op — so a look that sets nothing renders
//! byte-identically to a frame from before this type existed.
//!
//! Which parts a given backend can actually honour is a separate question,
//! answered by [`crate::BackendCapabilityProfile`]: the look always carries the
//! full intent, and a backend that cannot evaluate the sky or afford the bloom
//! chain **declares the drop** rather than silently ignoring it.

use crate::frame_ambient::FrameAmbient;
use crate::frame_bloom::FrameBloom;
use crate::frame_depth_fog::FrameDepthFog;
use crate::frame_sky::FrameSky;

/// The app-authored render look a backend binds with.
///
/// Built by starting from an ambient and adding the optional parts:
///
/// ```
/// use axiom_host::{FrameAmbient, FrameBloom, FrameRenderLook, FrameSky};
///
/// let look = FrameRenderLook::lit_by(FrameAmbient::default_hemisphere())
///     .with_sky(FrameSky::gradient([0.02, 0.03, 0.06], [0.06, 0.08, 0.13]))
///     .with_bloom(FrameBloom::moonlit());
/// assert!(look.sky().is_some());
/// assert!(look.depth_fog().is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameRenderLook {
    ambient: FrameAmbient,
    depth_fog: Option<FrameDepthFog>,
    sky: Option<FrameSky>,
    bloom: Option<FrameBloom>,
}

impl FrameRenderLook {
    /// A look with `ambient` as its fill light and nothing else authored.
    pub const fn lit_by(ambient: FrameAmbient) -> Self {
        FrameRenderLook {
            ambient,
            depth_fog: None,
            sky: None,
            bloom: None,
        }
    }

    /// This look with a different fill light, every other part untouched — how a
    /// driver holding a look replaces one part without rebuilding the rest.
    pub const fn with_ambient(mut self, ambient: FrameAmbient) -> Self {
        self.ambient = ambient;
        self
    }

    /// This look with atmospheric depth fog — the colour distance recedes toward
    /// and the normalized-depth range over which it does.
    pub const fn with_depth_fog(mut self, depth_fog: FrameDepthFog) -> Self {
        self.depth_fog = Some(depth_fog);
        self
    }

    /// This look with a sky behind the scene, replacing the flat clear colour.
    /// Gated by [`crate::RenderCapability::Sky`].
    pub const fn with_sky(mut self, sky: FrameSky) -> Self {
        self.sky = Some(sky);
        self
    }

    /// This look with bloom — how far a bright pixel spills into its
    /// neighbours. Gated by [`crate::RenderCapability::Bloom`].
    pub const fn with_bloom(mut self, bloom: FrameBloom) -> Self {
        self.bloom = Some(bloom);
        self
    }

    /// The hemisphere ambient filling unlit faces. Always present.
    pub const fn ambient(&self) -> FrameAmbient {
        self.ambient
    }

    /// The atmospheric depth fog, or `None` when the app authored none.
    pub const fn depth_fog(&self) -> Option<FrameDepthFog> {
        self.depth_fog
    }

    /// The sky behind the scene, or `None` when the frame keeps its flat clear
    /// colour.
    pub const fn sky(&self) -> Option<FrameSky> {
        self.sky
    }

    /// The bloom parameters, or `None` when highlights are left to clip.
    pub const fn bloom(&self) -> Option<FrameBloom> {
        self.bloom
    }
}

impl Default for FrameRenderLook {
    /// The engine's default look: the default hemisphere ambient, no fog, no
    /// sky, no bloom — exactly what every backend bound with before the look
    /// had a name, so an app that authors nothing is unchanged.
    fn default() -> Self {
        FrameRenderLook::lit_by(FrameAmbient::default_hemisphere())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Ratio;

    fn fog() -> FrameDepthFog {
        FrameDepthFog::new(
            Ratio::finite_or_zero(0.5),
            Ratio::finite_or_zero(1.0),
            Ratio::finite_or_zero(0.8),
            [0.1, 0.2, 0.3],
        )
    }

    fn sky() -> FrameSky {
        FrameSky::gradient([0.02, 0.03, 0.06], [0.06, 0.08, 0.13])
    }

    /// The default has to reproduce the old hardcode exactly, or every app that
    /// authored no look changes appearance the moment this type lands.
    #[test]
    fn the_default_look_is_the_engine_default_ambient_and_nothing_else() {
        let look = FrameRenderLook::default();
        assert_eq!(look.ambient(), FrameAmbient::default_hemisphere());
        assert!(look.depth_fog().is_none());
        assert!(look.sky().is_none());
        assert!(look.bloom().is_none());
        assert_eq!(
            look,
            FrameRenderLook::lit_by(FrameAmbient::default_hemisphere())
        );
    }

    #[test]
    fn each_part_is_added_independently_and_leaves_the_others_alone() {
        let base = FrameRenderLook::lit_by(FrameAmbient::default_hemisphere());

        let with_fog = base.with_depth_fog(fog());
        assert_eq!(with_fog.depth_fog(), Some(fog()));
        assert!(with_fog.sky().is_none(), "fog did not invent a sky");
        assert!(with_fog.bloom().is_none());

        let with_sky = base.with_sky(sky());
        assert_eq!(with_sky.sky(), Some(sky()));
        assert!(with_sky.depth_fog().is_none());
        assert!(with_sky.bloom().is_none());

        let with_bloom = base.with_bloom(FrameBloom::moonlit());
        assert_eq!(with_bloom.bloom(), Some(FrameBloom::moonlit()));
        assert!(with_bloom.sky().is_none());
        assert!(with_bloom.depth_fog().is_none());

        // The ambient survives every addition.
        [with_fog, with_sky, with_bloom]
            .iter()
            .for_each(|l| assert_eq!(l.ambient(), FrameAmbient::default_hemisphere()));
    }

    /// A driver holding a look replaces one part at a time (its `set_ambient`
    /// must not silently discard an already-authored sky).
    #[test]
    fn replacing_the_ambient_keeps_every_other_part() {
        let night = FrameAmbient::new([0.05, 0.07, 0.12], [0.02, 0.02, 0.03]);
        let look = FrameRenderLook::default()
            .with_depth_fog(fog())
            .with_sky(sky())
            .with_bloom(FrameBloom::moonlit())
            .with_ambient(night);
        assert_eq!(look.ambient(), night);
        assert_eq!(look.depth_fog(), Some(fog()));
        assert_eq!(look.sky(), Some(sky()));
        assert_eq!(look.bloom(), Some(FrameBloom::moonlit()));
    }

    #[test]
    fn a_full_look_carries_all_four_parts_and_compares_by_value() {
        let night = FrameAmbient::new([0.05, 0.07, 0.12], [0.02, 0.02, 0.03]);
        let full = FrameRenderLook::lit_by(night)
            .with_depth_fog(fog())
            .with_sky(sky())
            .with_bloom(FrameBloom::moonlit());
        assert_eq!(full.ambient(), night);
        assert_eq!(full.depth_fog(), Some(fog()));
        assert_eq!(full.sky(), Some(sky()));
        assert_eq!(full.bloom(), Some(FrameBloom::moonlit()));

        assert_eq!(full, full);
        assert_ne!(full, FrameRenderLook::default());
        // A look differing in exactly one part is a different look.
        assert_ne!(
            full,
            FrameRenderLook::lit_by(night)
                .with_depth_fog(fog())
                .with_sky(sky())
                .with_bloom(FrameBloom::highlights())
        );
        assert!(format!("{full:?}").contains("FrameRenderLook"));
    }

    /// Re-authoring a part replaces it rather than accumulating, so a live
    /// reload that re-runs the builder cannot end up with a stale sky.
    #[test]
    fn authoring_a_part_twice_keeps_the_last_one() {
        let look = FrameRenderLook::default()
            .with_bloom(FrameBloom::highlights())
            .with_bloom(FrameBloom::moonlit());
        assert_eq!(look.bloom(), Some(FrameBloom::moonlit()));
    }
}
