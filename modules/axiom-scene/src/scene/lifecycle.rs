//! Node lifecycle beyond initial authoring — runtime despawn (Category 3 of the
//! game vocabulary, `docs/game-vocabulary.md`). A child module so it reaches
//! `Scene`'s private internals while keeping `scene.rs` within the per-file size
//! budget.

use std::collections::BTreeSet;

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

    /// Despawn `node` and its entire subtree, returning whether `node` named a
    /// live node — the cascade the contract's `World.despawn` requires so that
    /// removing a parent takes its attached parts with it. Descendants are
    /// collected from the child→parent column via a bounded ancestor walk (a
    /// corrupted cyclic column still terminates) and removed before the node.
    pub(crate) fn despawn_subtree(&mut self, node: SceneNodeId) -> bool {
        let target = Self::entity(node);
        let descendants: Vec<EntityId> = {
            let parents = &self.world.storage().parents;
            parents
                .iter()
                .filter_map(|(child, _parent)| {
                    std::iter::successors(parents.get(child).copied(), |&id| {
                        parents.get(id).copied()
                    })
                    .scan(BTreeSet::new(), |seen, id| seen.insert(id).then_some(id))
                    .any(|id| id == target)
                    .then_some(child)
                })
                .collect()
        };
        descendants.into_iter().for_each(|descendant| {
            self.despawn_node(SceneNodeId::from_raw(descendant.raw()));
        });
        self.despawn_node(node)
    }

    /// Remove `entity` from the world (every component column, via the ECS) and
    /// from the two non-column marker maps the scene keeps alongside (`players`,
    /// `controllers`).
    fn despawn_entity(&mut self, entity: EntityId) -> bool {
        {
            let storage = self.world.storage_mut();
            storage.players.remove(&entity);
            storage.controllers.remove(&entity);
            // Drop the entity's app-defined dynamic components too, so a reused
            // id (the registry recycles slots) never inherits stale components.
            storage.dynamic.remove_entity(entity);
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
        assert_eq!(s.player_translation(0), Some(Vec3::new(2.0, 0.0, 0.0)));
        assert_eq!(s.node_count(), 1);

        assert!(s.despawn_player(0));
        assert_eq!(s.node_count(), 0);
        assert_eq!(s.player_translation(0), None);
        assert!(s
            .world_transform(SceneNodeId::from_raw(node.raw()))
            .is_err());

        assert!(!s.despawn_player(0));
        assert!(!s.despawn_player(99));
    }

    #[test]
    fn despawn_node_removes_by_handle_and_is_idempotent() {
        let mut s = Scene::new();
        let node = s.create_node(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(s.node_count(), 1);
        assert!(s.despawn_node(node));
        assert_eq!(s.node_count(), 0);
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

    #[test]
    fn despawn_subtree_removes_node_and_all_descendants() {
        let mut s = Scene::new();
        let root = s.create_node(Transform::IDENTITY);
        let child = s.create_node(Transform::IDENTITY);
        let grandchild = s.create_node(Transform::IDENTITY);
        let bystander = s.create_node(Transform::IDENTITY);
        s.set_parent(child, root).unwrap();
        s.set_parent(grandchild, child).unwrap();
        assert_eq!(s.node_count(), 4);

        assert!(s.despawn_subtree(root));
        assert_eq!(s.node_count(), 1);
        assert!(s
            .world_transform(SceneNodeId::from_raw(child.raw()))
            .is_err());
        assert!(s
            .world_transform(SceneNodeId::from_raw(grandchild.raw()))
            .is_err());
        assert!(s.world_transform(bystander).is_ok());

        assert!(!s.despawn_subtree(SceneNodeId::from_raw(999)));
        assert!(s.despawn_subtree(bystander));
        assert_eq!(s.node_count(), 0);
    }
}
