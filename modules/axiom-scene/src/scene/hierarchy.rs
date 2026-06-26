//! Parent/child linkage between scene nodes and the cycle guard that keeps the
//! hierarchy a forest. A child module so it reaches `Scene`'s private internals
//! while keeping `scene.rs` within the per-file size budget, and so neither
//! `impl Scene` block exceeds the engine's impl-block size budget.

use std::collections::BTreeSet;

use axiom_kernel::EntityId;

use super::Scene;
use crate::scene_error::SceneError;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;

impl Scene {
    pub(crate) fn set_parent(
        &mut self,
        child: SceneNodeId,
        parent: SceneNodeId,
    ) -> SceneResult<()> {
        (child != parent)
            .then_some(())
            .ok_or_else(|| {
                SceneError::self_parenting("set_parent: a node cannot be its own parent")
            })
            .and_then(|()| {
                self.is_node(child)
                    .then_some(())
                    .ok_or_else(|| SceneError::missing_node("set_parent: child id not in scene"))
            })
            .and_then(|()| {
                self.is_node(parent)
                    .then_some(())
                    .ok_or_else(|| SceneError::missing_node("set_parent: parent id not in scene"))
            })
            .and_then(|()| {
                (!self.would_introduce_cycle(Self::entity(child), Self::entity(parent)))
                    .then_some(())
                    .ok_or_else(|| {
                        SceneError::hierarchy_cycle(
                            "set_parent: assignment would introduce a cycle",
                        )
                    })
            })
            .map(|()| {
                self.world
                    .storage_mut()
                    .parents
                    .insert(Self::entity(child), Self::entity(parent));
            })
    }

    pub(crate) fn clear_parent(&mut self, child: SceneNodeId) -> SceneResult<()> {
        self.is_node(child)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("clear_parent: child id not in scene"))
            .map(|()| {
                self.world.storage_mut().parents.remove(Self::entity(child));
            })
    }

    /// Walk upward from `new_parent`; the assignment cycles iff we reach
    /// `child` (a direct ancestor loop) or revisit a node (a pre-existing
    /// cycle, only reachable by corrupting the parents column directly).
    fn would_introduce_cycle(&self, child: EntityId, new_parent: EntityId) -> bool {
        let parents = &self.world.storage().parents;
        // Walk the ancestor chain from `new_parent` as a lazy iterator, halting
        // the moment a node repeats (a pre-existing cycle) so the walk is finite.
        // The assignment cycles iff that finite walk visits `child`.
        let mut visited: BTreeSet<EntityId> = BTreeSet::new();
        std::iter::successors(Some(new_parent), |&id| parents.get(id).copied())
            // Each step is a cycle hit iff it reaches `child` or revisits a node;
            // `scan` yields one bool per visited ancestor and `any` short-circuits
            // on the first hit, so the lazy walk never runs past the first repeat.
            .scan((), |(), id| Some((id == child) | !visited.insert(id)))
            .any(|hit| hit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;
    use axiom_math::Transform;

    fn node(s: &mut Scene) -> SceneNodeId {
        s.create_node(Transform::IDENTITY)
    }

    #[test]
    fn set_parent_links_and_parent_of_reports_it() {
        let mut s = Scene::new();
        let p = node(&mut s);
        let c = node(&mut s);
        s.set_parent(c, p).unwrap();
        assert_eq!(s.parent_of(c), Some(p));
        assert_eq!(s.parent_of(p), None);
    }

    #[test]
    fn self_parenting_fails() {
        let mut s = Scene::new();
        let n = node(&mut s);
        assert_eq!(
            s.set_parent(n, n).unwrap_err().code(),
            SceneErrorCode::SelfParenting
        );
    }

    #[test]
    fn set_parent_missing_child_or_parent_fails() {
        let mut s = Scene::new();
        let p = node(&mut s);
        assert_eq!(
            s.set_parent(SceneNodeId::from_raw(99), p)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
        let c = node(&mut s);
        assert_eq!(
            s.set_parent(c, SceneNodeId::from_raw(99))
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn cycle_assignment_is_rejected() {
        // chain: a <- b <- c ; making a a child of c would loop.
        let mut s = Scene::new();
        let a = node(&mut s);
        let b = node(&mut s);
        let c = node(&mut s);
        s.set_parent(b, a).unwrap();
        s.set_parent(c, b).unwrap();
        assert_eq!(
            s.set_parent(a, c).unwrap_err().code(),
            SceneErrorCode::HierarchyCycle
        );
    }

    #[test]
    fn preexisting_cycle_trips_the_visited_guard() {
        // Corrupt the parents column into a 2-cycle (a->b->a), unreachable via
        // the public API, then walk from `a` looking for an unrelated child:
        // the walk revisits `a` and the visited-guard returns true.
        let mut s = Scene::new();
        let a = node(&mut s);
        let b = node(&mut s);
        s.world
            .storage_mut()
            .parents
            .insert(Scene::entity(a), Scene::entity(b));
        s.world
            .storage_mut()
            .parents
            .insert(Scene::entity(b), Scene::entity(a));
        assert!(s.would_introduce_cycle(Scene::entity(SceneNodeId::from_raw(99)), Scene::entity(a)));
    }

    #[test]
    fn clear_parent_present_and_missing() {
        let mut s = Scene::new();
        let p = node(&mut s);
        let c = node(&mut s);
        s.set_parent(c, p).unwrap();
        s.clear_parent(c).unwrap();
        assert_eq!(s.parent_of(c), None);
        // Clearing a root (no parent) still succeeds.
        s.clear_parent(p).unwrap();
        // Missing node fails.
        assert_eq!(
            s.clear_parent(SceneNodeId::from_raw(99))
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
    }
}
