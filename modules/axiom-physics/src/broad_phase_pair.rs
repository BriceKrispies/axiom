//! Deterministic broad-phase candidate pairing.
//!
//! The broad phase turns the collider set into the unordered collider pairs whose
//! shapes *could* be in contact, so the (more expensive) narrow phase only tests
//! those. The broad phase uses a simple, fully deterministic `O(n²)` scan — no
//! dynamic tree yet (a documented deferral, see `ROADMAP.md`). A pair is a candidate when
//! both colliders and both owning bodies are enabled, the colliders are on
//! **different** bodies, and their geometry could overlap: either both are finite
//! and their world AABBs overlap, or exactly one is an (infinite) plane — a plane
//! cannot be culled by bounds, so every finite collider is always a candidate
//! against every plane. Two planes never pair (no finite shape to contact).
//!
//! Output pairs are sorted by `(a, b)` collider handle, so the candidate list is
//! a deterministic function of world state, independent of scan order.

use axiom_math::Aabb;

use crate::collider_bounds::world_aabb;
use crate::physics_body::PhysicsBody;
use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider::PhysicsCollider;
use crate::physics_collider_handle::PhysicsColliderHandle;

/// A candidate pair of colliders the broad phase hands to the narrow phase. The
/// handles are stored in ascending order (`a < b`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BroadPhasePair {
    a: PhysicsColliderHandle,
    b: PhysicsColliderHandle,
}

impl BroadPhasePair {
    /// Construct a pair, normalizing the two handles into ascending order so a
    /// pair's identity is independent of which collider was discovered first.
    pub(crate) fn new(a: PhysicsColliderHandle, b: PhysicsColliderHandle) -> Self {
        let swap = (a.raw() > b.raw()) as usize;
        BroadPhasePair {
            a: [a, b][swap],
            b: [b, a][swap],
        }
    }

    pub(crate) fn a(&self) -> PhysicsColliderHandle {
        self.a
    }

    pub(crate) fn b(&self) -> PhysicsColliderHandle {
        self.b
    }
}

/// A collider resolved into the data the pair test needs: its handle, its owning
/// body, whether it is live (collider *and* body enabled), and its world AABB
/// (`None` for a plane).
struct Candidate {
    collider: PhysicsColliderHandle,
    body: PhysicsBodyHandle,
    active: bool,
    aabb: Option<Aabb>,
}

/// Resolve every collider against its owning body into a [`Candidate`]. A
/// collider always references a live body (attachment validated it and bodies are
/// never removed), so `find` always matches and every collider yields one
/// candidate; the `filter_map` simply makes the resolution total without a
/// branch.
fn candidates(colliders: &[PhysicsCollider], bodies: &[PhysicsBody]) -> Vec<Candidate> {
    colliders
        .iter()
        .filter_map(|c| {
            bodies
                .iter()
                .find(|b| b.handle() == c.body())
                .map(|b| Candidate {
                    collider: c.handle(),
                    body: c.body(),
                    active: c.enabled() & b.enabled(),
                    aabb: world_aabb(c.shape(), b.transform().translation),
                })
        })
        .collect()
}

/// Whether two resolved colliders form a broad-phase candidate pair.
fn is_candidate_pair(ci: &Candidate, cj: &Candidate) -> bool {
    let both_active = ci.active & cj.active;
    let different_body = ci.body != cj.body;
    let both_finite = ci.aabb.is_some() & cj.aabb.is_some();
    let one_plane = ci.aabb.is_some() ^ cj.aabb.is_some();
    let finite_overlap = ci
        .aabb
        .as_ref()
        .zip(cj.aabb.as_ref())
        .map_or(false, |(a, b)| a.overlaps(b));
    let geometric = (both_finite & finite_overlap) | one_plane;
    both_active & different_body & geometric
}

/// Form the deterministic broad-phase candidate-pair list.
pub(crate) fn detect_pairs(
    colliders: &[PhysicsCollider],
    bodies: &[PhysicsBody],
) -> Vec<BroadPhasePair> {
    let resolved = candidates(colliders, bodies);
    let mut pairs: Vec<BroadPhasePair> = resolved
        .iter()
        .enumerate()
        .flat_map(|(index, ci)| {
            resolved
                .iter()
                .skip(index + 1)
                .filter_map(move |cj| {
                    is_candidate_pair(ci, cj).then(|| BroadPhasePair::new(ci.collider, cj.collider))
                })
        })
        .collect();
    pairs.sort_by_key(|pair| (pair.a().raw(), pair.b().raw()));
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_body_desc::PhysicsBodyDesc;
    use crate::physics_collider_shape::PhysicsColliderShape;
    use crate::physics_material::PhysicsMaterial;
    use axiom_kernel::{Meters, Ratio};
    use axiom_math::{Transform, Vec3};

    fn material() -> PhysicsMaterial {
        PhysicsMaterial::new(
            Ratio::new(0.0).unwrap(),
            Ratio::new(0.0).unwrap(),
            Ratio::new(1.0).unwrap(),
        )
        .unwrap()
    }

    fn body(raw: u64, x: f32) -> PhysicsBody {
        let desc =
            PhysicsBodyDesc::static_body(Transform::from_translation(Vec3::new(x, 0.0, 0.0)))
                .unwrap();
        PhysicsBody::from_desc(PhysicsBodyHandle::from_raw(raw), desc)
    }

    fn sphere_collider(collider_raw: u64, body_raw: u64) -> PhysicsCollider {
        PhysicsCollider::new(
            PhysicsColliderHandle::from_raw(collider_raw),
            PhysicsBodyHandle::from_raw(body_raw),
            PhysicsColliderShape::sphere(Meters::new(1.0).unwrap()).unwrap(),
            material(),
            false,
        )
    }

    fn plane_collider(collider_raw: u64, body_raw: u64) -> PhysicsCollider {
        PhysicsCollider::new(
            PhysicsColliderHandle::from_raw(collider_raw),
            PhysicsBodyHandle::from_raw(body_raw),
            PhysicsColliderShape::plane(Vec3::UNIT_Y, Meters::new(0.0).unwrap()).unwrap(),
            material(),
            false,
        )
    }

    #[test]
    fn pair_exposes_ordered_handles() {
        let pair = BroadPhasePair::new(
            PhysicsColliderHandle::from_raw(1),
            PhysicsColliderHandle::from_raw(2),
        );
        assert_eq!(pair.a(), PhysicsColliderHandle::from_raw(1));
        assert_eq!(pair.b(), PhysicsColliderHandle::from_raw(2));
        assert!(format!("{pair:?}").contains("BroadPhasePair"));
    }

    #[test]
    fn derives_are_exercised() {
        let p = BroadPhasePair::new(
            PhysicsColliderHandle::from_raw(1),
            PhysicsColliderHandle::from_raw(2),
        );
        let copied = p;
        let cloned = p.clone();
        assert_eq!(p, copied);
        assert_eq!(p, cloned);
        // Differ in `b` (same `a`) and in `a` — exercises both comparison arms.
        assert_ne!(
            p,
            BroadPhasePair::new(
                PhysicsColliderHandle::from_raw(1),
                PhysicsColliderHandle::from_raw(3),
            )
        );
        assert_ne!(
            p,
            BroadPhasePair::new(
                PhysicsColliderHandle::from_raw(2),
                PhysicsColliderHandle::from_raw(3),
            )
        );
    }

    #[test]
    fn no_pairs_with_zero_or_one_collider() {
        assert!(detect_pairs(&[], &[]).is_empty());
        let bodies = [body(1, 0.0)];
        let colliders = [sphere_collider(1, 1)];
        assert!(detect_pairs(&colliders, &bodies).is_empty());
    }

    #[test]
    fn overlapping_aabbs_produce_one_stable_pair() {
        // Two unit spheres 1.0 apart on X — AABBs ([-1,1] vs [0,2]) overlap.
        let bodies = [body(1, 0.0), body(2, 1.0)];
        let colliders = [sphere_collider(10, 1), sphere_collider(20, 2)];
        let pairs = detect_pairs(&colliders, &bodies);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].a(), PhysicsColliderHandle::from_raw(10));
        assert_eq!(pairs[0].b(), PhysicsColliderHandle::from_raw(20));
    }

    #[test]
    fn multiple_pairs_are_returned_in_sorted_order() {
        // Three clustered unit spheres — all three pairwise AABBs overlap, so all
        // three pairs are produced and must come back sorted by (a, b) handle.
        let bodies = [body(1, 0.0), body(2, 0.5), body(3, 1.0)];
        let colliders = [
            sphere_collider(30, 3),
            sphere_collider(10, 1),
            sphere_collider(20, 2),
        ];
        let pairs = detect_pairs(&colliders, &bodies);
        let keys: Vec<(u64, u64)> = pairs.iter().map(|p| (p.a().raw(), p.b().raw())).collect();
        assert_eq!(keys, vec![(10, 20), (10, 30), (20, 30)]);
    }

    #[test]
    fn separated_aabbs_produce_no_pair() {
        // Unit spheres 5 apart — AABBs do not overlap.
        let bodies = [body(1, 0.0), body(2, 5.0)];
        let colliders = [sphere_collider(10, 1), sphere_collider(20, 2)];
        assert!(detect_pairs(&colliders, &bodies).is_empty());
    }

    #[test]
    fn a_plane_pairs_with_every_finite_collider_regardless_of_distance() {
        // A far-away sphere still pairs with the (infinite) plane.
        let bodies = [body(1, 0.0), body(2, 1000.0)];
        let colliders = [plane_collider(10, 1), sphere_collider(20, 2)];
        let pairs = detect_pairs(&colliders, &bodies);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].a(), PhysicsColliderHandle::from_raw(10));
        assert_eq!(pairs[0].b(), PhysicsColliderHandle::from_raw(20));
    }

    #[test]
    fn two_planes_never_pair() {
        let bodies = [body(1, 0.0), body(2, 0.0)];
        let colliders = [plane_collider(10, 1), plane_collider(20, 2)];
        assert!(detect_pairs(&colliders, &bodies).is_empty());
    }

    #[test]
    fn disabled_body_is_skipped() {
        let mut sleeping = body(2, 1.0);
        sleeping.set_enabled(false);
        let bodies = [body(1, 0.0), sleeping];
        let colliders = [sphere_collider(10, 1), sphere_collider(20, 2)];
        assert!(detect_pairs(&colliders, &bodies).is_empty());
    }

    #[test]
    fn two_colliders_on_the_same_body_never_pair() {
        let bodies = [body(1, 0.0)];
        let colliders = [sphere_collider(10, 1), sphere_collider(20, 1)];
        assert!(detect_pairs(&colliders, &bodies).is_empty());
    }

    #[test]
    fn pair_order_is_deterministic_regardless_of_insertion_order() {
        let bodies = [body(1, 0.0), body(2, 0.5)];
        let forward = [sphere_collider(10, 1), sphere_collider(20, 2)];
        let reversed = [sphere_collider(20, 2), sphere_collider(10, 1)];
        let a = detect_pairs(&forward, &bodies);
        let b = detect_pairs(&reversed, &bodies);
        assert_eq!(a, b);
        assert_eq!(a[0].a(), PhysicsColliderHandle::from_raw(10));
    }
}
