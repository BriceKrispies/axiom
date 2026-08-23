//! `RenderSystem` — the render subsystem, `render/index.js:125`.
//!
//! **Where you would look for it, under the id the graph already names.**
//! Three subsystems declare `"render"` in their `deps()` — `player`, `ui` and
//! `materials` — and until now nothing answered to it. `Registry::resolve`
//! failed the moment any of them was registered, which is why
//! `scene::wiring::physics_player` records that the registry "cannot admit these
//! two" and the port grew a second composition root instead. One missing file
//! held the real one shut.
//!
//! ## What it is here, and what it is not
//!
//! The source's `RenderSystem` owns a `THREE.WebGLRenderer`: the device, the
//! passes, the targets, the light rig. Axiom owns all of that — a port that
//! re-implemented it would be building a renderer beside the engine rather than
//! on it. So this owns the half that is genuinely the *game's*: **the frame's
//! render look, and the pass registry its dependents ask about.**
//!
//! That is not a thin shim. The look — ambient, depth fog, sky, the two-band
//! indirect fill, the tone map — is currently set from five places in
//! `scene::app`, and every capability added to the app has had to remember all
//! five. One owner is the point.
//!
//! ## The fork it deliberately does not take
//!
//! `render/index.js:135` is `this.rng = ctx.rng.fork()` — the source's render
//! subsystem draws from the root stream, first, before every other slot. **This
//! one does not**, and that is a divergence recorded rather than hidden: this
//! port's root sequence begins at `world`, so `render`, `physics` and `player`
//! never take the forks the source gives them and the world is already not the
//! source's world.
//!
//! Taking the fork here would move the level. It is the right eventual fix and
//! it is a deliberate change with a golden to update
//! (`scene::game::tests::the_root_stream_is_consumed_in_the_registrys_order`),
//! not a side effect of moving a composition root. Adding random content to
//! this system without also updating that golden is the mistake it exists to
//! catch.

use std::any::Any;

use axiom_kernel::Seconds;

use crate::engine::Ctx;
use crate::error::CoreError;
use crate::registry::{Phase, Subsystem};

/// The frame's render look, as the game authors it.
///
/// Every field is `Option` for the same reason the engine's own setters are: a
/// look nobody authored must render as it did before the field existed, and
/// `None` says that where a zero would be a claim.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct RenderLook {
    pub ambient: Option<axiom::prelude::FrameAmbient>,
    pub depth_fog: Option<axiom::prelude::FrameDepthFog>,
    pub sky: Option<axiom::prelude::FrameSky>,
    pub indirect: Option<axiom::prelude::FrameIndirect>,
    pub tonemap: Option<axiom::prelude::FrameTonemap>,
    pub clear_color: Option<[f32; 4]>,
}

/// The render subsystem.
pub struct RenderSystem {
    look: RenderLook,
    /// `render.registerPass(name)` — the source's pass registry. `player` asks
    /// whether this system exists before installing its low-health pass
    /// (`player/system.rs`, `ctx.peek("render")`), and that question only has
    /// an answer once something can be registered.
    passes: Vec<&'static str>,
    viewport: (u32, u32),
}

impl Default for RenderSystem {
    fn default() -> Self {
        RenderSystem::new()
    }
}

impl RenderSystem {
    /// A render subsystem with nothing authored yet.
    pub const fn new() -> Self {
        RenderSystem {
            look: RenderLook {
                ambient: None,
                depth_fog: None,
                sky: None,
                indirect: None,
                tonemap: None,
                clear_color: None,
            },
            passes: Vec::new(),
            viewport: (0, 0),
        }
    }

    /// The frame's look, for whoever binds it to the engine.
    pub const fn look(&self) -> RenderLook {
        self.look
    }

    /// Author the frame's look. Called by whoever resolves the sky.
    pub const fn set_look(&mut self, look: RenderLook) {
        self.look = look;
    }

    /// `render.registerPass(name)`.
    pub fn register_pass(&mut self, name: &'static str) {
        self.passes.contains(&name).then_some(()).map_or_else(
            || self.passes.push(name),
            |()| (),
        );
    }

    /// The passes registered so far, in registration order.
    pub fn passes(&self) -> &[&'static str] {
        &self.passes
    }

    /// The surface size the frame is being drawn at.
    pub const fn viewport(&self) -> (u32, u32) {
        self.viewport
    }
}

impl Subsystem for RenderSystem {
    fn id(&self) -> &'static str {
        "render"
    }

    /// `static deps = []` (`render/index.js:126`) — render is a root of the
    /// graph, which is what lets three other systems depend on it.
    fn deps(&self) -> &'static [&'static str] {
        &[]
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Resize]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn init(&mut self, _ctx: &Ctx<'_>) -> Result<(), CoreError> {
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32, _ctx: &Ctx<'_>) {
        self.viewport = (width, height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The id and the empty `deps` are the whole reason this file exists.**
    /// Three subsystems name `"render"`; a typo here puts the registry back
    /// where it was, failing to resolve with a message about an unregistered
    /// subsystem.
    #[test]
    fn it_answers_to_the_id_three_other_systems_depend_on() {
        let render = RenderSystem::new();
        assert_eq!(render.id(), "render");
        assert_eq!(render.deps(), &[] as &[&str]);
    }

    #[test]
    fn a_pass_registers_once_however_often_it_is_asked() {
        let mut render = RenderSystem::new();
        render.register_pass("low-health");
        render.register_pass("low-health");
        assert_eq!(render.passes(), &["low-health"]);
    }

    #[test]
    fn resize_records_the_surface_the_frame_is_drawn_at() {
        let mut render = RenderSystem::new();
        assert_eq!(render.viewport(), (0, 0));
        render.viewport = (1280, 720);
        assert_eq!(render.viewport(), (1280, 720));
    }

    /// An unauthored look is every-field-`None`, which is what lets a frame that
    /// authors none render as it did before this system existed.
    #[test]
    fn an_unauthored_look_claims_nothing() {
        let look = RenderSystem::new().look();
        assert_eq!(look, RenderLook::default());
        assert!(look.ambient.is_none() && look.tonemap.is_none());
    }
}
