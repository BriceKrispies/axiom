//! Node lifecycle beyond initial authoring — runtime despawn (Category 3 of the
//! game vocabulary, `docs/game-vocabulary.md`). A child module so it reaches
//! `Scene`'s private internals while keeping `scene.rs` within the per-file size
//! budget.

use axiom_kernel::EntityId;

use super::Scene;
use crate::scene_node_id::SceneNodeId;

impl Scene {
    /// Despawn the node marked with `player` index: remove its entity (the ECS
    /// world cleans every component column) along with its player/controller
    /// marks. Returns whether such a node existed — despawning an absent index is
    /// a clean `false`, so it is safe to call every tick.
    pub(crate) fn despawn_player(&mut self, player: u32) -> bool {
        self.world
            .storage()
            .players
            .iter()
            .find_map(|(&entity, &index)| (index == player).then_some(entity))
            .map(|entity| self.despawn_entity(entity))
            .unwrap_or(false)
    }

    /// Despawn `node` by its handle — the Entity-addressed counterpart to
    /// [`Self::despawn_player`]. Removes the entity (every component column) plus
    /// its player/controller marks. Returns whether `node` named a live node;
    /// despawning an absent/already-removed handle is a clean `false`.
    pub(crate) fn despawn_node(&mut self, node: SceneNodeId) -> bool {
        let despawned = self
            .is_node(node)
            .then(|| self.despawn_entity(Self::entity(node)));
        despawned.unwrap_or(false)
    }

    /// Remove `entity` from the world (every component column, via the ECS) and
    /// from the two non-column marker maps the scene keeps alongside (`players`,
    /// `controllers`).
    fn despawn_entity(&mut self, entity: EntityId) -> bool {
        {
            let storage = self.world.storage_mut();
            storage.players.remove(&entity);
            storage.controllers.remove(&entity);
        }
        self.world.despawn(entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_node_id::SceneNodeId;
    use axiom_math::{Transform, Vec3};

    #[test]
    fn despawn_player_removes_the_node_and_is_idempotent() {
        let mut s = Scene::new();
        let node = s.create_node(Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)));
        s.add_player(node, 0).unwrap();
        // The marked node exists and is reachable by its player index.
        assert_eq!(s.player_translation(0), Some(Vec3::new(2.0, 0.0, 0.0)));
        assert_eq!(s.node_count(), 1);

        // Despawning it removes the node and clears its player mark.
        assert!(s.despawn_player(0));
        assert_eq!(s.node_count(), 0);
        assert_eq!(s.player_translation(0), None);
        // The node id no longer resolves as a live node.
        assert!(s
            .world_transform(SceneNodeId::from_raw(node.raw()))
            .is_err());

        // Despawning again (now absent) is a clean `false`.
        assert!(!s.despawn_player(0));
        // An unknown index is also a clean `false`.
        assert!(!s.despawn_player(99));
    }

    #[test]
    fn despawn_node_removes_by_handle_and_is_idempotent() {
        let mut s = Scene::new();
        let node = s.create_node(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(s.node_count(), 1);
        // Despawning by handle removes the node.
        assert!(s.despawn_node(node));
        assert_eq!(s.node_count(), 0);
        // A repeat despawn — and any non-node handle — is a clean `false`.
        assert!(!s.despawn_node(node));
        assert!(!s.despawn_node(SceneNodeId::from_raw(404)));
    }

    #[test]
    fn despawn_player_clears_a_controller_marked_node() {
        // A node that is both a player and a first-person controller: despawn must
        // clear both marker maps (exercises the controllers `remove`).
        let mut s = Scene::new();
        let node = s.create_node(Transform::IDENTITY);
        s.add_player(node, 3).unwrap();
        s.add_controller(node, 3).unwrap();
        assert!(s.despawn_player(3));
        assert_eq!(s.node_count(), 0);
        assert_eq!(s.player_translation(3), None);
    }
}
