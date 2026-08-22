//! The running app's per-frame **render-look** setters: the clear (background)
//! colour and the hemisphere ambient the frame is lit with. These are the "what
//! the frame looks like" knobs a live reload adjusts without rebuilding the app;
//! they live together here (a child module of `app`, so they reach `RunningApp`'s
//! private render fields) to keep `app.rs` focused on lifecycle + stepping.

use axiom_host::{FrameAmbient, FrameBloom, FrameDepthFog, FrameIndirect, FramePostProcess, FrameSky};

use crate::app::RunningApp;

impl RunningApp {
    /// Set the per-frame clear (background) colour. Used by a live reload to
    /// update the background without rebuilding the running app.
    pub fn set_clear_color(&mut self, color: [f32; 4]) {
        self.clear_color = color;
    }

    /// Set the frame's hemisphere ambient (the sky/ground fill lighting unlit
    /// faces). The authored value flows onto every `FrameOutcome` and is consumed
    /// by both the offscreen capture and the live present arm, so an app can light
    /// its scene to daylight instead of the dim engine default.
    pub fn set_ambient(&mut self, ambient: FrameAmbient) {
        self.ambient = ambient;
    }

    /// The frame's hemisphere ambient (the app's authored sky/ground fill).
    pub const fn ambient(&self) -> FrameAmbient {
        self.ambient
    }

    /// Set the frame's atmospheric depth fog — the colour distance recedes toward
    /// and the normalized-depth range over which it does. The authored value flows
    /// onto every `FrameOutcome` and is consumed by **both** backends (the GPU
    /// shader's fog term and the Canvas 2D fog post-pass read the same numbers), so
    /// a scene's horizon dissolves into its sky the same way whichever backend won
    /// the cascade. An app that authors none is unchanged.
    pub fn set_depth_fog(&mut self, depth_fog: FrameDepthFog) {
        self.depth_fog = Some(depth_fog);
    }

    /// The frame's atmospheric depth fog, or `None` when the app authored none.
    pub const fn depth_fog(&self) -> Option<FrameDepthFog> {
        self.depth_fog
    }

    /// Set the frame's tonemap/colour grade (exposure/white-balance/contrast/
    /// saturation). The authored grade flows onto every `FrameOutcome` and is
    /// consumed by both the offscreen capture and the live present arm, so an app
    /// can present a graded, filmic look instead of a flat, washed-out raster.
    pub fn set_postprocess(&mut self, postprocess: FramePostProcess) {
        self.postprocess = Some(postprocess);
    }

    /// The frame's tonemap/colour grade (the app's authored render-look grade), or
    /// `None` when the app authored none.
    pub const fn postprocess(&self) -> Option<FramePostProcess> {
        self.postprocess
    }

    /// Set the frame's **sky** — a gradient with an optional celestial body in it,
    /// evaluated per pixel behind the scene instead of a flat clear colour.
    ///
    /// This is what gives a frame a light source that is actually *in* it. An app
    /// that authors none keeps its clear colour and is unchanged. Gated by
    /// [`axiom_host::RenderCapability::Sky`], so a backend that cannot evaluate it
    /// reports the drop rather than quietly ignoring it.
    pub fn set_sky(&mut self, sky: FrameSky) {
        self.sky = Some(sky);
    }

    /// The frame's sky, or `None` when the app authored none.
    pub const fn sky(&self) -> Option<FrameSky> {
        self.sky
    }

    /// Set the frame's **bloom** — which pixels spill light into their
    /// neighbours, how far, and how the surplus above white rolls off.
    ///
    /// Without it a material authored to emit more than `1.0` simply clips, so a
    /// lamp reads as a flat white sticker rather than a light. An app that
    /// authors none is unchanged. Gated by
    /// [`axiom_host::RenderCapability::Bloom`], so a backend that cannot afford
    /// the extra render targets reports the drop rather than quietly ignoring it.
    /// Author the frame's **two-band indirect fill** — a cool skylight band and
    /// a warm ground bounce, each with its own normal gate.
    ///
    /// This is what keeps surfaces the key light does not reach from collapsing
    /// toward black. [`Self::set_ambient`]'s hemisphere is a single `mix`
    /// between two colours; it cannot express that a vertical wall sees half the
    /// sky dome, and it carries no warm bounce at all.
    pub fn set_indirect(&mut self, indirect: FrameIndirect) {
        self.indirect = Some(indirect);
    }

    /// The frame's two-band indirect fill, if it authored one.
    pub const fn indirect(&self) -> Option<FrameIndirect> {
        self.indirect
    }

    pub fn set_bloom(&mut self, bloom: FrameBloom) {
        self.bloom = Some(bloom);
    }

    /// The frame's bloom, or `None` when the app authored none.
    pub const fn bloom(&self) -> Option<FrameBloom> {
        self.bloom
    }
}
