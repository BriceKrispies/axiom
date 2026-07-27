//! `Visible`: whether a node's renderable is drawn this frame.
//!
//! # Why this exists
//!
//! The engine has always carried a `visible` flag on every renderable, and the
//! render pipeline has always dropped invisible renderables at submission — an
//! invisible object costs no projection, no shading, and no draw. What was
//! missing was any way for an **app** to set it. So every app that pools objects
//! (particles, markers, decals, debug gizmos) retired an unused slot by parking
//! it far off-screen at a near-zero scale, and the renderer dutifully projected
//! and shaded all of its triangles before culling them.
//!
//! That is not free. In a pooled scene the retired slots are usually the
//! *majority* of the objects: End Zone parks ~1,100 of them, ~13,700 triangles,
//! and on the software Canvas 2D path — where per-triangle projection is about
//! half of frame time — that measured **~2.2 ms per frame for 502 slots alone**,
//! all of it spent on geometry that is never visible.
//!
//! `Visible(false)` is the supported way to retire a pooled object for a frame.
//! It keeps the entity, its handle, and its components alive — unlike despawn,
//! which churns the scene — and it removes the object from the frame entirely
//! rather than hiding it somewhere the renderer still has to look.
//!
//! ```ignore
//! app.set(slot, Visible(false));   // retired: costs nothing this frame
//! app.set(slot, Visible(true));    // back in the frame
//! ```

use axiom_scene::{SceneApi, SceneNodeId as Entity};

use crate::component::Component;

/// Whether the node's renderable is drawn. Renderables are visible when
/// spawned; write `Visible(false)` to drop one from the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visible(pub bool);

impl Visible {
    /// The flag as a plain `bool`.
    pub const fn get(self) -> bool {
        self.0
    }
}

impl Default for Visible {
    /// Visible — matching a freshly spawned renderable.
    fn default() -> Self {
        Visible(true)
    }
}

impl Component for Visible {
    fn get(scene: &SceneApi, entity: Entity) -> Option<Self> {
        scene.renderable_visible(entity).map(Visible)
    }

    fn set(scene: &mut SceneApi, entity: Entity, value: Self) -> bool {
        scene.set_renderable_visibility(entity, value.0).is_ok()
    }

    fn query(scene: &SceneApi) -> Vec<(Entity, Self)> {
        scene
            .renderable_visibilities()
            .into_iter()
            .map(|(entity, visible)| (entity, Visible(visible)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_round_trips_and_defaults_to_visible() {
        assert!(Visible::default().get());
        assert!(Visible(true).get());
        assert!(!Visible(false).get());
        assert_eq!(Visible(true), Visible::default());
        assert_ne!(Visible(true), Visible(false));
        assert!(format!("{:?}", Visible(false)).contains("Visible"));
        let copied = Visible(false);
        assert_eq!(copied.clone(), copied);
    }
}
