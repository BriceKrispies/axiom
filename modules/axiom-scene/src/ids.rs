//! The scene's identity vocabulary — the value-type handle the [`crate::SceneApi`]
//! facade returns and accepts. Kept in an `ids` module so it is published as
//! identity vocabulary (Module Law #8), not counted as a second facade.

pub use crate::scene_node_id::SceneNodeId;
