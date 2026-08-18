//! Golden captures for `src/physics/{math,surfaces,bvh}.js`, pinned against
//! the original JavaScript.
//!
//! Every `expected` value below was captured by running the original
//! `C:/dev/Claude-of-Duty/src/physics/bvh.js` (`StaticWorld`, unmodified)
//! under Node (v24) over the fixed triangle soup built by [`soup`], and
//! printing the results as JSON. They are golden values, not
//! recomputations — see `apps/claude-of-duty/tests/core_port.rs` for the
//! established methodology this follows.
//!
//! ## The fixed triangle soup
//!
//! Every triangle vertex coordinate below is a small integer, chosen
//! deliberately: they are exactly representable in both `f32` and `f64`, so
//! the `f32` truncation `StaticWorld` applies to its stored geometry (see
//! `src/physics/bvh.rs`'s module doc comment) is a no-op everywhere except
//! the BVH node bounds, which are padded by the non-representable constant
//! `1e-5`. That is why the node-bounds goldens below carry the odd-looking
//! `...9999747378752`/`...0135803223` tails (the `f64` value of the `f32`
//! that `1e-5` actually rounds to) while every other geometric value in this
//! file is an exact integer or half-integer.
//!
//! Triangle order, exactly as built by both the capture script and [`soup`]:
//! - indices `0..32`: a 4x4 floor grid over `x=[0,4]`, `z=[0,4]` at `y=0`.
//!   Cell `(i,j)` (`i` outer, `j` inner, both `0..4`) contributes two
//!   triangles split along the `p00->p11` diagonal: `A=(p00,p10,p11)` then
//!   `B=(p00,p11,p01)`, CCW from above.
//! - indices `32..34`: a vertical wall quad at `x=0`, `y=[0,2]`, `z=[0,4]`,
//!   facing `+X`.
//! - index `34`: one degenerate (zero-area, colinear) triangle at
//!   `(10,0,0),(11,0,0),(12,0,0)`, off to the side. `build()` itself never
//!   drops degenerate triangles (only the unported `bakeMesh` does — see
//!   `src/physics/bvh.rs`'s module doc comment), so this stays in the soup
//!   and exercises the fallback-normal path.

use axiom_claude_of_duty::physics::bvh::{Aabb, StaticWorld};
use axiom_claude_of_duty::physics::surfaces::{layer, mask};
use axiom_claude_of_duty::world::palette::Surface;

const FLOOR_COUNT: usize = 32;
const WALL_COUNT: usize = 2;
const DEGENERATE_COUNT: usize = 1;
const TOTAL: usize = FLOOR_COUNT + WALL_COUNT + DEGENERATE_COUNT;

/// Not bit-guaranteed across libm implementations (`sqrt` is involved), so
/// sweep/overlap results are compared within this absolute tolerance — the
/// same figure `tests/core_port.rs` established for the RNG's Box-Muller
/// draws.
fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "{what}: expected {expected:.17}, got {actual:.17}"
    );
}

fn push_tri(out: &mut Vec<f64>, a: [f64; 3], b: [f64; 3], c: [f64; 3]) {
    out.extend_from_slice(&a);
    out.extend_from_slice(&b);
    out.extend_from_slice(&c);
}

/// Builds the fixed 35-triangle soup described in the module doc comment and
/// returns a built [`StaticWorld`] over it.
fn soup() -> StaticWorld {
    let mut tris = Vec::with_capacity(TOTAL * 9);

    for i in 0..4 {
        for j in 0..4 {
            let (i, j) = (i as f64, j as f64);
            let p00 = [i, 0.0, j];
            let p10 = [i + 1.0, 0.0, j];
            let p01 = [i, 0.0, j + 1.0];
            let p11 = [i + 1.0, 0.0, j + 1.0];
            push_tri(&mut tris, p00, p10, p11);
            push_tri(&mut tris, p00, p11, p01);
        }
    }
    assert_eq!(tris.len(), FLOOR_COUNT * 9);

    push_tri(&mut tris, [0.0, 0.0, 0.0], [0.0, 0.0, 4.0], [0.0, 2.0, 4.0]);
    push_tri(&mut tris, [0.0, 0.0, 0.0], [0.0, 2.0, 4.0], [0.0, 2.0, 0.0]);

    push_tri(&mut tris, [10.0, 0.0, 0.0], [11.0, 0.0, 0.0], [12.0, 0.0, 0.0]);

    assert_eq!(tris.len(), TOTAL * 9);

    let mut world = StaticWorld::new();
    world.add_triangles(&tris, TOTAL, Surface::Concrete, layer::STATIC, "soup");
    world.build();
    world
}

// ---------------------------------------------------------------------------
// build() / _buildNodes() — pure `+ - * /` and comparisons, so exact.
// ---------------------------------------------------------------------------

#[test]
fn build_produces_the_javascript_triangle_and_node_counts() {
    let world = soup();
    assert_eq!(world.tri_count(), TOTAL);
    assert_eq!(world.node_count(), 19);
    assert_eq!(world.max_depth(), 5);
    assert_eq!(
        world.aabb(),
        Aabb {
            minx: 0.0,
            miny: 0.0,
            minz: 0.0,
            maxx: 12.0,
            maxy: 2.0,
            maxz: 4.0,
        }
    );
}

/// Every node's `[minx,miny,minz,maxx,maxy,maxz]` bounds and `[leftFirst,
/// count]` meta, in node order — the whole tree shape, pinned exactly.
#[test]
fn build_produces_the_javascript_node_bounds_and_meta_exactly() {
    let world = soup();
    let expected: [([f64; 6], [i32; 2]); 19] = [
        (
            [-0.000009999999747378752, -0.000009999999747378752, -0.000009999999747378752, 12.000009536743164, 2.0000100135803223, 4.000010013580322],
            [1, 0],
        ),
        (
            [-0.000009999999747378752, -0.000009999999747378752, -0.000009999999747378752, 3.0000100135803223, 2.0000100135803223, 4.000010013580322],
            [7, 0],
        ),
        (
            [2.9999899864196777, -0.000009999999747378752, -0.000009999999747378752, 12.000009536743164, 0.000009999999747378752, 4.000010013580322],
            [3, 0],
        ),
        (
            [2.9999899864196777, -0.000009999999747378752, -0.000009999999747378752, 4.000010013580322, 0.000009999999747378752, 4.000010013580322],
            [5, 0],
        ),
        (
            [9.999990463256836, -0.000009999999747378752, -0.000009999999747378752, 12.000009536743164, 0.000009999999747378752, 0.000009999999747378752],
            [34, 1],
        ),
        (
            [2.9999899864196777, -0.000009999999747378752, -0.000009999999747378752, 4.000010013580322, 0.000009999999747378752, 2.0000100135803223],
            [26, 4],
        ),
        (
            [2.9999899864196777, -0.000009999999747378752, 1.9999899864196777, 4.000010013580322, 0.000009999999747378752, 4.000010013580322],
            [30, 4],
        ),
        (
            [-0.000009999999747378752, -0.000009999999747378752, -0.000009999999747378752, 3.0000100135803223, 2.0000100135803223, 4.000010013580322],
            [13, 0],
        ),
        (
            [-0.000009999999747378752, -0.000009999999747378752, 1.9999899864196777, 3.0000100135803223, 0.000009999999747378752, 4.000010013580322],
            [9, 0],
        ),
        (
            [-0.000009999999747378752, -0.000009999999747378752, 1.9999899864196777, 2.0000100135803223, 0.000009999999747378752, 4.000010013580322],
            [11, 0],
        ),
        (
            [1.9999899864196777, -0.000009999999747378752, 1.9999899864196777, 3.0000100135803223, 0.000009999999747378752, 4.000010013580322],
            [22, 4],
        ),
        (
            [-0.000009999999747378752, -0.000009999999747378752, 1.9999899864196777, 1.0000100135803223, 0.000009999999747378752, 4.000010013580322],
            [14, 4],
        ),
        (
            [0.9999899864196777, -0.000009999999747378752, 1.9999899864196777, 2.0000100135803223, 0.000009999999747378752, 4.000010013580322],
            [18, 4],
        ),
        (
            [-0.000009999999747378752, -0.000009999999747378752, -0.000009999999747378752, 0.000009999999747378752, 2.0000100135803223, 4.000010013580322],
            [0, 2],
        ),
        (
            [-0.000009999999747378752, -0.000009999999747378752, -0.000009999999747378752, 3.0000100135803223, 0.000009999999747378752, 2.0000100135803223],
            [15, 0],
        ),
        (
            [-0.000009999999747378752, -0.000009999999747378752, -0.000009999999747378752, 2.0000100135803223, 0.000009999999747378752, 2.0000100135803223],
            [17, 0],
        ),
        (
            [1.9999899864196777, -0.000009999999747378752, -0.000009999999747378752, 3.0000100135803223, 0.000009999999747378752, 2.0000100135803223],
            [10, 4],
        ),
        (
            [-0.000009999999747378752, -0.000009999999747378752, -0.000009999999747378752, 1.0000100135803223, 0.000009999999747378752, 2.0000100135803223],
            [2, 4],
        ),
        (
            [0.9999899864196777, -0.000009999999747378752, -0.000009999999747378752, 2.0000100135803223, 0.000009999999747378752, 2.0000100135803223],
            [6, 4],
        ),
    ];

    for (i, (bounds, meta)) in expected.into_iter().enumerate() {
        assert_eq!(world.node_bounds(i), bounds, "node {i} bounds");
        assert_eq!(world.node_meta(i), meta, "node {i} meta");
    }
}

#[test]
fn build_falls_back_the_degenerate_triangles_normal_to_plus_y() {
    let world = soup();
    assert_eq!(world.normal_of((TOTAL - 1) as u32), [0.0, 1.0, 0.0]);
}

#[test]
fn build_on_an_empty_world_produces_zero_triangles_and_zero_nodes() {
    let mut world = StaticWorld::new();
    world.build();
    assert_eq!(world.tri_count(), 0);
    assert_eq!(world.node_count(), 0);

    let hit = world.raycast(0.0, 5.0, 0.0, 0.0, -1.0, 0.0, 10.0, mask::ALL, -1);
    assert!(!hit.hit);
    assert!(!world.raycast_any(0.0, 5.0, 0.0, 0.0, -1.0, 0.0, 10.0, mask::ALL));
    assert!(world.query_aabb(-1.0, -1.0, -1.0, 1.0, 1.0, 1.0, mask::ALL).is_empty());
}

// ---------------------------------------------------------------------------
// raycast / raycastAny
// ---------------------------------------------------------------------------

#[test]
fn raycast_straight_down_through_a_floor_cell_interior() {
    let world = soup();
    let hit = world.raycast(2.0, 5.0, 2.0, 0.0, -1.0, 0.0, 10.0, mask::ALL, -1);
    assert!(hit.hit);
    assert_eq!(hit.t, 5.0);
    assert_eq!([hit.px, hit.py, hit.pz], [2.0, 0.0, 2.0]);
    assert_eq!([hit.nx, hit.ny, hit.nz], [0.0, 1.0, 0.0]);
    assert_eq!(hit.tri, 10);
    assert_eq!(hit.surface, Surface::Concrete.index());
    assert_eq!(hit.object, 0);
    assert!(!hit.front_face);
}

/// A ray straight down exactly on the shared diagonal edge of floor cell
/// (0,0) — the "hit exactly on a shared edge" case the recipe calls out.
#[test]
fn raycast_hits_exactly_on_a_shared_triangle_edge() {
    let world = soup();
    let hit = world.raycast(0.5, 5.0, 0.5, 0.0, -1.0, 0.0, 10.0, mask::ALL, -1);
    assert!(hit.hit);
    assert_eq!(hit.t, 5.0);
    assert_eq!([hit.px, hit.py, hit.pz], [0.5, 0.0, 0.5]);
    assert_eq!([hit.nx, hit.ny, hit.nz], [0.0, 1.0, 0.0]);
    assert_eq!(hit.tri, 1);
    assert!(!hit.front_face);
}

/// A ray coplanar with (parallel to) every floor triangle — the Möller–
/// Trumbore determinant is ~0 for all of them, and the ray never reaches the
/// wall (moving away from it), so this must miss entirely.
#[test]
fn raycast_parallel_to_the_floor_plane_misses() {
    let world = soup();
    let hit = world.raycast(2.0, 0.0, 2.0, 1.0, 0.0, 0.0, 10.0, mask::ALL, -1);
    assert!(!hit.hit);
    assert_eq!(hit.t, 0.0);
}

#[test]
fn raycast_hits_the_wall() {
    let world = soup();
    let hit = world.raycast(2.0, 1.0, 2.0, -1.0, 0.0, 0.0, 10.0, mask::ALL, -1);
    assert!(hit.hit);
    assert_eq!(hit.t, 2.0);
    assert_eq!([hit.px, hit.py, hit.pz], [0.0, 1.0, 2.0]);
    assert_eq!([hit.nx, hit.ny, hit.nz], [1.0, 0.0, 0.0]);
    assert_eq!(hit.tri, 33);
    assert!(!hit.front_face);
}

#[test]
fn raycast_any_matches_raycasts_hit_or_miss() {
    let world = soup();
    assert!(world.raycast_any(2.0, 5.0, 2.0, 0.0, -1.0, 0.0, 10.0, mask::ALL));
    assert!(!world.raycast_any(2.0, 0.0, 2.0, 1.0, 0.0, 0.0, 10.0, mask::ALL));
}

// ---------------------------------------------------------------------------
// queryAabb
// ---------------------------------------------------------------------------

#[test]
fn query_aabb_over_the_floor_returns_every_floor_and_wall_triangle() {
    let world = soup();
    let mut candidates = world.query_aabb(-0.5, -0.5, -0.5, 4.5, 0.5, 4.5, mask::ALL);
    candidates.sort_unstable();
    let expected: Vec<u32> = (0..(FLOOR_COUNT + WALL_COUNT) as u32).collect();
    assert_eq!(candidates, expected);
}

// ---------------------------------------------------------------------------
// sweepCapsule
// ---------------------------------------------------------------------------

/// A capsule whose segment already overlaps the floor at the start of the
/// sweep — the recipe's "sweep starting already overlapping" case.
#[test]
fn sweep_capsule_already_overlapping_the_floor_hits_at_t_zero() {
    let world = soup();
    let hit = world.sweep_capsule(2.0, 0.05, 2.0, 2.0, 1.05, 2.0, 0.1, 0.0, -1.0, 0.0, 1.0, mask::ALL);
    assert!(hit.hit);
    assert_eq!(hit.t, 0.0);
    assert_eq!([hit.px, hit.py, hit.pz], [2.0, 0.0, 2.0]);
    assert_eq!([hit.nx, hit.ny, hit.nz], [0.0, 1.0, 0.0]);
    assert_eq!(hit.tri, 21);
    assert_eq!(hit.object, 0);
}

#[test]
fn sweep_capsule_approaching_the_floor_from_above() {
    let world = soup();
    let hit = world.sweep_capsule(2.0, 2.0, 2.0, 2.0, 3.0, 2.0, 0.3, 0.0, -1.0, 0.0, 5.0, mask::ALL);
    assert!(hit.hit);
    assert_close(hit.t, 1.7, "sweep_approach t");
    assert_eq!([hit.px, hit.py, hit.pz], [2.0, 0.0, 2.0]);
    assert_eq!([hit.nx, hit.ny, hit.nz], [0.0, 1.0, 0.0]);
    assert_eq!(hit.tri, 21);
    assert_eq!(hit.object, 0);
}

#[test]
fn sweep_capsule_moving_sideways_within_a_short_max_dist_misses() {
    let world = soup();
    let hit = world.sweep_capsule(2.0, 2.0, 2.0, 2.0, 3.0, 2.0, 0.3, 1.0, 0.0, 0.0, 0.5, mask::ALL);
    assert!(!hit.hit);
    assert_eq!(hit.t, 0.0);
}

// ---------------------------------------------------------------------------
// overlapCapsule
// ---------------------------------------------------------------------------

#[test]
fn overlap_capsule_resting_on_the_floor_collects_six_contacts() {
    let world = soup();
    let contacts = world.overlap_capsule(2.0, 0.05, 2.0, 2.0, 1.0, 2.0, 0.15, mask::ALL, 0.0);
    assert_eq!(contacts.count(), 6);
    let expected_tris = [21, 20, 12, 19, 10, 11];
    assert_eq!(contacts.tri, expected_tris);
    for i in 0..contacts.count() {
        assert_eq!([contacts.nx[i], contacts.ny[i], contacts.nz[i]], [0.0, -1.0, 0.0], "contact {i} normal");
        assert_eq!([contacts.px[i], contacts.py[i], contacts.pz[i]], [2.0, 0.0, 2.0], "contact {i} point");
        assert_eq!(contacts.depth[i], 0.1_f32, "contact {i} depth");
        assert_eq!(contacts.s[i], 0.0_f32, "contact {i} s");
    }
}

#[test]
fn overlap_capsule_well_above_the_floor_collects_nothing() {
    let world = soup();
    let contacts = world.overlap_capsule(2.0, 5.0, 2.0, 2.0, 6.0, 2.0, 0.1, mask::ALL, 0.0);
    assert_eq!(contacts.count(), 0);
}

// ---------------------------------------------------------------------------
// surfaces.js — reuses `crate::world::palette::Surface`; pinned separately
// from the BVH so a change to the enum's declaration order (which the whole
// port depends on matching `SURFACE_NAMES`) fails loudly here.
// ---------------------------------------------------------------------------

#[test]
fn surface_index_matches_the_javascript_surface_names_order() {
    use axiom_claude_of_duty::physics::surfaces::surface_index;

    let expected = [
        ("concrete", Surface::Concrete),
        ("metal", Surface::Metal),
        ("wood", Surface::Wood),
        ("dirt", Surface::Dirt),
        ("sand", Surface::Sand),
        ("glass", Surface::Glass),
        ("water", Surface::Water),
        ("foliage", Surface::Foliage),
        ("fabric", Surface::Fabric),
        ("flesh", Surface::Flesh),
        ("rubber", Surface::Rubber),
        ("plaster", Surface::Plaster),
    ];
    for (i, (name, surface)) in expected.into_iter().enumerate() {
        assert_eq!(surface.index(), i as u8, "{name} index");
        assert_eq!(Surface::from_index(i as u8), surface, "{name} from_index");
        assert_eq!(surface_index(name, Surface::Concrete), surface, "{name} surface_index");
    }
}

#[test]
fn guess_surface_matches_the_javascript_keyword_table() {
    use axiom_claude_of_duty::physics::surfaces::guess_surface;

    // One representative keyword per source GUESS row (surfaces.js:82-95),
    // plus the source's documented fallback for an unmatched name.
    assert_eq!(guess_surface("Brick_Wall_01", Surface::Wood), Surface::Concrete);
    assert_eq!(guess_surface("SteelBarrel", Surface::Concrete), Surface::Metal);
    assert_eq!(guess_surface("WoodenCrate", Surface::Concrete), Surface::Wood);
    assert_eq!(guess_surface("DirtPatch", Surface::Concrete), Surface::Dirt);
    assert_eq!(guess_surface("BeachSand", Surface::Concrete), Surface::Sand);
    assert_eq!(guess_surface("WindowPane", Surface::Concrete), Surface::Glass);
    assert_eq!(guess_surface("PuddleMesh", Surface::Concrete), Surface::Water);
    assert_eq!(guess_surface("BushLeaf", Surface::Concrete), Surface::Foliage);
    assert_eq!(guess_surface("CanvasTarp", Surface::Concrete), Surface::Fabric);
    assert_eq!(guess_surface("EnemyTorso", Surface::Concrete), Surface::Flesh);
    assert_eq!(guess_surface("TireMat", Surface::Concrete), Surface::Rubber);
    assert_eq!(guess_surface("DrywallPartition", Surface::Concrete), Surface::Plaster);
    assert_eq!(guess_surface("UnrecognizedThing", Surface::Wood), Surface::Wood);
    assert_eq!(guess_surface("", Surface::Sand), Surface::Sand);
}

#[test]
fn layer_and_mask_bits_match_the_javascript_constants() {
    assert_eq!(layer::STATIC, 1 << 0);
    assert_eq!(layer::PROP, 1 << 1);
    assert_eq!(layer::DEBRIS, 1 << 2);
    assert_eq!(layer::PLAYER, 1 << 3);
    assert_eq!(layer::ACTOR, 1 << 4);
    assert_eq!(layer::RAGDOLL, 1 << 5);
    assert_eq!(layer::GLASS, 1 << 6);
    assert_eq!(layer::WATER, 1 << 7);
    assert_eq!(layer::CLIP, 1 << 8);
    assert_eq!(layer::SHOOT_ONLY, 1 << 9);
    assert_eq!(layer::TRIGGER, 1 << 10);
    assert_eq!(layer::FOLIAGE, 1 << 11);

    assert_eq!(mask::ALL, 0xffffu16 & !layer::TRIGGER);
    assert_eq!(mask::CHARACTER, layer::STATIC | layer::PROP | layer::CLIP);
    assert_eq!(
        mask::BULLET,
        layer::STATIC | layer::PROP | layer::DEBRIS | layer::ACTOR | layer::RAGDOLL | layer::GLASS | layer::SHOOT_ONLY | layer::FOLIAGE
    );
    assert_eq!(mask::WORLD, layer::STATIC | layer::PROP);
    assert_eq!(mask::SIGHT, layer::STATIC | layer::PROP | layer::DEBRIS);
    assert_eq!(mask::DEBRIS, layer::STATIC | layer::PROP | layer::CLIP);
    assert_eq!(mask::EXPLOSION, layer::STATIC | layer::PROP);
}

#[test]
fn surface_props_match_the_javascript_table_for_concrete_and_glass() {
    let concrete = Surface::Concrete.props();
    assert_eq!(concrete.pen_depth, 0.055);
    assert_eq!(concrete.energy_loss, 0.62);
    assert_eq!(concrete.friction, 0.92);
    assert_eq!(concrete.density, 2400.0);
    assert!(!concrete.shatters);

    let glass = Surface::Glass.props();
    assert_eq!(glass.pen_depth, 0.45);
    assert_eq!(glass.restitution, 0.2);
    assert!(glass.shatters);
}
