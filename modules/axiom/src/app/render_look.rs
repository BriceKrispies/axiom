//! The running app's per-frame **render-look** setters: the clear (background)
//! colour and the hemisphere ambient the frame is lit with. These are the "what
//! the frame looks like" knobs a live reload adjusts without rebuilding the app;
//! they live together here (a child module of `app`, so they reach `RunningApp`'s
//! private render fields) to keep `app.rs` focused on lifecycle + stepping.

use axiom_host::FrameAmbient;

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
}
