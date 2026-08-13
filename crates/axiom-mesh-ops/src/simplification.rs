//! Mesh decimation by quadric-error-metric (QEM) edge collapse.
//!
//! This is Garland & Heckbert's algorithm, not a triangle cull. Every vertex
//! accumulates the *fundamental error quadric* of the planes of the triangles
//! touching it — a 4x4 symmetric form whose value at a point is the sum of
//! squared distances to those planes. The cost of collapsing an edge is that
//! summed form evaluated at the position the merged vertex would occupy, so the
//! algorithm removes the edges whose disappearance moves the surface least,
//! wherever they happen to be. Flat regions decimate away; silhouettes and
//! creases survive. Deleting every Nth triangle produces the opposite result and
//! is not what this module does.
//!
//! # Determinism
//!
//! Simplification is a long chain of "pick the cheapest thing", and floating
//! point ties plus unordered iteration are exactly how such a chain becomes
//! irreproducible. Two rules make the output byte-identical on every run:
//!
//! 1. **Costs are quantized.** A cost is keyed as `(cost * 1e6).round() as i64`.
//!    Two collapses whose real costs differ by less than a microunit are
//!    deliberately treated as *equal*, so a last-bit difference in an `f32`
//!    accumulation cannot reorder them.
//! 2. **Ties break on the edge itself.** Candidates live in a
//!    [`BTreeMap`] keyed on `(quantized_cost, min_index, max_index)`. The key is
//!    unique, totally ordered, and derived only from the mesh's own numbering,
//!    so the traversal order is fixed. No hash map is used anywhere in this
//!    module.
//!
//! # Placement and validity
//!
//! The merged vertex goes to the minimizer of the summed quadric, obtained by
//! solving the quadric's 3x3 sub-system. When that system is singular — which is
//! the *normal* case on a flat region, where the planes are all parallel and the
//! minimizer is a whole plane rather than a point — the edge midpoint is used
//! instead.
//!
//! A candidate collapse is rejected, and the next-cheapest tried, when it would
//! turn any surviving triangle by more than 90 degrees (a fold-over), collapse a
//! non-manifold edge, or leave the mesh with no triangles at all. Rejection is a
//! skip, never a failure: the operator returns the best mesh it could reach.
//!
//! # What the surviving vertex keeps
//!
//! Only positions are recomputed. Normals, uvs, tangents, colours, and skin
//! binding are **carried from the surviving (lower-index) endpoint** — an
//! attribute of a real vertex on the real surface, rather than a blend of two
//! vertices that no longer both exist. Callers who need normals re-derived for
//! the decimated silhouette run `axiom_mesh::generate_normals` on the result;
//! this operator does not decide that for them.
//!
//! # Cost
//!
//! Each collapse re-scans the live edge set, so the operator is quadratic in the
//! number of triangles removed. That is a deliberate trade: a mutable priority
//! queue with lazy invalidation is faster and is exactly the kind of hidden
//! retained state whose update order is hard to make reproducible.

use std::collections::{BTreeMap, BTreeSet};

use axiom_kernel::Ratio;
use axiom_math::Vec3;
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

/// Costs below a microunit are treated as equal, so accumulation noise cannot
/// reorder two collapses. See the module documentation.
const COST_QUANTIZATION: f32 = 1.0e6;

/// A 3x3 sub-system with a determinant below this is treated as singular and the
/// collapse falls back to the edge midpoint.
const SINGULAR_DETERMINANT: f32 = 1.0e-12;

/// A post-collapse triangle whose doubled area falls below this has no reliable
/// orientation, so the collapse that would create it is rejected.
const DEGENERATE_AREA: f32 = 1.0e-16;

/// One undirected edge as its `(lower, higher)` endpoint indices.
type EdgeKey = (u32, u32);

/// How much geometry the caller wants left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SimplifyTarget {
    /// Stop once the mesh has at most this many triangles. Must be at least one.
    TriangleCount(u32),
    /// Stop at `round(original * fraction)` triangles, floored at one. The
    /// fraction must lie in `(0, 1]`.
    Fraction(Ratio),
}

/// The triangle-count payload, or `None` for the other variant.
///
/// The Branchless Law admits exactly one pattern form, `matches!`, and a match
/// guard is an ordinary expression — so the pattern test that identifies the
/// variant is also what records its payload. This is the whole of the enum
/// dispatch in this module: everything downstream is `Option` combinators.
fn triangle_count_payload(target: SimplifyTarget) -> Option<u32> {
    let mut found = None;
    let _ = matches!(target, SimplifyTarget::TriangleCount(n) if { found = Some(n); true });
    found
}

/// The fraction payload, or `None` for the other variant. See
/// [`triangle_count_payload`].
fn fraction_payload(target: SimplifyTarget) -> Option<Ratio> {
    let mut found = None;
    let _ = matches!(target, SimplifyTarget::Fraction(r) if { found = Some(r); true });
    found
}

fn invalid_parameter(message: &'static str) -> MeshError {
    MeshError::new(MeshErrorCode::InvalidParameter, message)
}

/// Turn a target into an absolute triangle count, rejecting a target outside its
/// documented domain.
///
/// A fraction is floored at one triangle: a mesh has to describe *some* surface,
/// and `round(4 * 0.01) == 0` is a request the representation cannot honour.
fn resolve_target(target: SimplifyTarget, original: usize) -> MeshResult<usize> {
    let from_count = triangle_count_payload(target).map(|n| {
        (n >= 1)
            .then_some(n as usize)
            .ok_or_else(|| invalid_parameter("a triangle-count target must be at least 1"))
    });
    let from_fraction = fraction_payload(target).map(|r| {
        let f = r.get();
        ((f > 0.0) & (f <= 1.0))
            .then(|| ((original as f32 * f).round() as usize).max(1))
            .ok_or_else(|| invalid_parameter("a fraction target must lie in (0, 1]"))
    });
    from_count.or(from_fraction).unwrap_or(Ok(original))
}

/// The fundamental error quadric, as the ten distinct entries of a symmetric
/// 4x4 matrix in row-major upper-triangular order.
///
/// For plane `(a, b, c, d)` with `a^2 + b^2 + c^2 = 1`, evaluating the form at a
/// point gives the squared distance from that point to the plane; summing the
/// forms of the planes around a vertex gives the squared-distance-to-surface
/// measure the whole algorithm is built on.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Quadric([f32; 10]);

/// The determinant of a 3x3 matrix given in row-major order.
fn determinant3(m: [[f32; 3]; 3]) -> f32 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

impl Quadric {
    /// The zero form: a vertex no triangle touches costs nothing to move.
    const ZERO: Quadric = Quadric([0.0; 10]);

    /// The outer product `p * p^T` of the plane `(normal, offset)`.
    fn from_plane(normal: Vec3, offset: f32) -> Quadric {
        let [a, b, c, d] = [normal.x, normal.y, normal.z, offset];
        Quadric([
            a * a,
            a * b,
            a * c,
            a * d,
            b * b,
            b * c,
            b * d,
            c * c,
            c * d,
            d * d,
        ])
    }

    /// The sum of two forms, which is the quadric of the union of their planes.
    fn plus(self, other: Quadric) -> Quadric {
        Quadric(core::array::from_fn(|i| self.0[i] + other.0[i]))
    }

    /// `v^T Q v` — the summed squared distance from `v` to the accumulated
    /// planes.
    fn error(self, v: Vec3) -> f32 {
        let q = self.0;
        let (x, y, z) = (v.x, v.y, v.z);
        q[0] * x * x
            + 2.0 * q[1] * x * y
            + 2.0 * q[2] * x * z
            + 2.0 * q[3] * x
            + q[4] * y * y
            + 2.0 * q[5] * y * z
            + 2.0 * q[6] * y
            + q[7] * z * z
            + 2.0 * q[8] * z
            + q[9]
    }

    /// The point that minimizes this form, or `fallback` when the form's 3x3
    /// sub-system is singular (parallel planes, a flat neighbourhood) or the
    /// solution is not representable.
    fn optimal(self, fallback: Vec3) -> Vec3 {
        let q = self.0;
        let rows = [[q[0], q[1], q[2]], [q[1], q[4], q[5]], [q[2], q[5], q[7]]];
        let rhs = [-q[3], -q[6], -q[8]];
        let det = determinant3(rows);
        let usable = det.abs() > SINGULAR_DETERMINANT;
        let safe = [1.0, det][usize::from(usable)];
        let solved = Vec3::new(
            determinant3([
                [rhs[0], rows[0][1], rows[0][2]],
                [rhs[1], rows[1][1], rows[1][2]],
                [rhs[2], rows[2][1], rows[2][2]],
            ]) / safe,
            determinant3([
                [rows[0][0], rhs[0], rows[0][2]],
                [rows[1][0], rhs[1], rows[1][2]],
                [rows[2][0], rhs[2], rows[2][2]],
            ]) / safe,
            determinant3([
                [rows[0][0], rows[0][1], rhs[0]],
                [rows[1][0], rows[1][1], rhs[1]],
                [rows[2][0], rows[2][1], rhs[2]],
            ]) / safe,
        );
        let finite = solved.x.is_finite() & solved.y.is_finite() & solved.z.is_finite();
        [fallback, solved][usize::from(usable & finite)]
    }
}

/// The twice-area normal of a triangle, in the layer's counter-clockwise
/// convention.
fn face_normal(a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    b.subtract(a).cross(c.subtract(a))
}

/// The unit-normal plane of a triangle as a quadric, or the zero form when the
/// triangle has no area to define one.
fn triangle_quadric(positions: &[Vec3], triangle: [u32; 3]) -> Quadric {
    let a = positions[triangle[0] as usize];
    let raw = face_normal(a, positions[triangle[1] as usize], positions[triangle[2] as usize]);
    raw.normalize()
        .map(|n| Quadric::from_plane(n, -n.dot(a)))
        .unwrap_or(Quadric::ZERO)
}

/// Sum every incident triangle's plane quadric onto each of its three corners.
fn accumulate_quadrics(positions: &[Vec3], triangles: &[[u32; 3]]) -> Vec<Quadric> {
    triangles.iter().fold(
        vec![Quadric::ZERO; positions.len()],
        |mut quadrics, &triangle| {
            let plane = triangle_quadric(positions, triangle);
            triangle
                .iter()
                .for_each(|&v| quadrics[v as usize] = quadrics[v as usize].plus(plane));
            quadrics
        },
    )
}

/// The mesh mid-decimation: positions and topology change, attributes do not.
#[derive(Debug, Clone, PartialEq)]
struct Working {
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
    quadrics: Vec<Quadric>,
}

/// Whether a triangle still names three distinct vertices.
fn distinct_corners(triangle: &[u32; 3]) -> bool {
    (triangle[0] != triangle[1]) & (triangle[1] != triangle[2]) & (triangle[0] != triangle[2])
}

impl Working {
    /// Read a mesh into decimation state, seeding every vertex's quadric.
    fn from_mesh(mesh: &Mesh) -> Working {
        let positions = mesh.positions().to_vec();
        let triangles: Vec<[u32; 3]> = mesh
            .indices()
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        let quadrics = accumulate_quadrics(&positions, &triangles);
        Working {
            positions,
            triangles,
            quadrics,
        }
    }

    /// The live edge set, deduplicated and ordered.
    fn edges(&self) -> BTreeSet<EdgeKey> {
        self.triangles
            .iter()
            .flat_map(|t| {
                [
                    (t[0].min(t[1]), t[0].max(t[1])),
                    (t[1].min(t[2]), t[1].max(t[2])),
                    (t[0].min(t[2]), t[0].max(t[2])),
                ]
            })
            .collect()
    }

    /// The vertices sharing a triangle with `vertex`.
    fn ring(&self, vertex: u32) -> BTreeSet<u32> {
        self.triangles
            .iter()
            .filter(|t| t.contains(&vertex))
            .flat_map(|t| *t)
            .filter(|&v| v != vertex)
            .collect()
    }
}

/// Quantize a cost into the total order the candidate map is keyed on.
fn quantize(cost: f32) -> i64 {
    (cost * COST_QUANTIZATION).round() as i64
}

/// Every live edge's collapse cost and target position, ordered cheapest-first
/// with ties broken by the edge's own endpoint indices.
fn candidates(state: &Working) -> BTreeMap<(i64, u32, u32), Vec3> {
    state
        .edges()
        .into_iter()
        .map(|(a, b)| {
            let quadric = state.quadrics[a as usize].plus(state.quadrics[b as usize]);
            let midpoint = state.positions[a as usize]
                .add(state.positions[b as usize])
                .mul_scalar(0.5);
            let placement = quadric.optimal(midpoint);
            ((quantize(quadric.error(placement)), a, b), placement)
        })
        .collect()
}

/// Whether a triangle keeps its facing when `edge`'s endpoints move to `to`.
fn preserves_facing(state: &Working, triangle: &[u32; 3], edge: EdgeKey, to: Vec3) -> bool {
    let read = |v: u32| state.positions[v as usize];
    let before = face_normal(read(triangle[0]), read(triangle[1]), read(triangle[2]));
    let moved: [Vec3; 3] = core::array::from_fn(|k| {
        let v = triangle[k];
        [read(v), to][usize::from((v == edge.0) | (v == edge.1))]
    });
    let after = face_normal(moved[0], moved[1], moved[2]);
    (before.dot(after) > 0.0) & (after.length_squared() > DEGENERATE_AREA)
}

/// Whether collapsing `edge` to `to` leaves a mesh worth having.
///
/// Three conditions, all of them reasons to skip and try the next-cheapest edge
/// rather than to fail the operation:
///
/// - **Something survives.** The collapse must not erase the last triangle.
/// - **The edge is manifold.** Its endpoints' one-rings may share exactly as
///   many vertices as there are triangles on the edge, and there must be at most
///   two of those. Collapsing a fin or a fan-of-three welds surface sheets that
///   were never joined and produces geometry no renderer can shade.
/// - **Nothing folds over.** Every triangle that survives but touches the edge
///   must keep its facing to within 90 degrees and keep a usable area.
fn valid_collapse(state: &Working, edge: EdgeKey, to: Vec3) -> bool {
    let (lo, hi) = edge;
    let holds = |t: &&[u32; 3], v: u32| t.contains(&v);
    let removed = state
        .triangles
        .iter()
        .filter(|t| holds(t, lo) & holds(t, hi))
        .count();
    let survives = state.triangles.len() > removed;

    let shared = state.ring(lo).intersection(&state.ring(hi)).count();
    let manifold = (shared == removed) & (removed <= 2);

    let unfolded = state
        .triangles
        .iter()
        .filter(|t| (holds(t, lo) | holds(t, hi)) & !(holds(t, lo) & holds(t, hi)))
        .all(|t| preserves_facing(state, t, edge, to));

    survives & manifold & unfolded
}

/// Merge `edge`'s higher endpoint into its lower one at `to`.
fn apply_collapse(state: &mut Working, edge: EdgeKey, to: Vec3) {
    let (lo, hi) = edge;
    state.positions[lo as usize] = to;
    state.quadrics[lo as usize] = state.quadrics[lo as usize].plus(state.quadrics[hi as usize]);
    state.triangles.iter_mut().for_each(|t| {
        t.iter_mut()
            .for_each(|v| *v = [*v, lo][usize::from(*v == hi)]);
    });
    state.triangles.retain(distinct_corners);
}

/// Perform the cheapest valid collapse, or nothing at all.
///
/// "Nothing at all" covers both terminal cases — the target is already met, and
/// no remaining edge can be collapsed legally — and is what makes the bounded
/// fold below correct: extra iterations are simply idle.
fn collapse_step(state: &mut Working, target: usize) {
    let pending = (state.triangles.len() > target)
        .then(|| candidates(state))
        .unwrap_or_default();
    let chosen = pending
        .iter()
        .find_map(|(&(_, a, b), &to)| valid_collapse(state, (a, b), to).then_some(((a, b), to)));
    chosen
        .into_iter()
        .for_each(|(edge, to)| apply_collapse(state, edge, to));
}

/// Copy the surviving vertices' entries out of an attribute stream, leaving an
/// absent stream absent.
fn gather<T: Copy>(stream: &[T], survivors: &[u32]) -> Vec<T> {
    survivors
        .iter()
        .filter_map(|&v| stream.get(v as usize).copied())
        .collect()
}

/// Renumber the decimated state into a compact, valid mesh: degenerate triangles
/// dropped, orphaned vertices dropped, indices renumbered in ascending order of
/// their original index.
fn compact(mesh: &Mesh, state: &Working) -> MeshResult<Mesh> {
    let live: Vec<[u32; 3]> = state
        .triangles
        .iter()
        .copied()
        .filter(distinct_corners)
        .collect();
    let survivors: Vec<u32> = live
        .iter()
        .flat_map(|t| *t)
        .collect::<BTreeSet<u32>>()
        .into_iter()
        .collect();
    let renumbered: BTreeMap<u32, u32> = survivors
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new as u32))
        .collect();
    Mesh::from_streams(MeshStreams {
        positions: survivors
            .iter()
            .map(|&v| state.positions[v as usize])
            .collect(),
        indices: live
            .iter()
            .flat_map(|t| *t)
            .map(|v| renumbered.get(&v).copied().unwrap_or(0))
            .collect(),
        normals: gather(mesh.normals(), &survivors),
        uvs: gather(mesh.uvs(), &survivors),
        tangents: gather(mesh.tangents(), &survivors),
        colors: gather(mesh.colors(), &survivors),
        joints: gather(mesh.joints(), &survivors),
        weights: gather(mesh.weights(), &survivors),
    })
}

/// Run the bounded collapse loop and compact the result.
///
/// The iteration count is known up front — every accepted collapse removes at
/// least one triangle, so `original - goal` attempts is enough — which is what
/// lets the loop be a `fold` over a range instead of an open-ended `while`.
fn decimate(mesh: &Mesh, goal: usize) -> MeshResult<Mesh> {
    let budget = mesh.triangle_count().saturating_sub(goal);
    let finished = (0..budget).fold(Working::from_mesh(mesh), |mut state, _| {
        collapse_step(&mut state, goal);
        state
    });
    compact(mesh, &finished)
}

/// Decimate a mesh with the quadric error metric.
///
/// Collapses the cheapest valid edge repeatedly until the target is met, where
/// "cheapest" is the summed squared distance from the merged vertex to the
/// planes of every triangle that met at either endpoint. The result is a
/// compact, valid [`Mesh`]: degenerate triangles and orphaned vertices are
/// dropped, and every attribute stream present on the input is present on the
/// output, carried from the surviving endpoints.
///
/// A target at or above the current triangle count returns the mesh unchanged.
/// A [`SimplifyTarget::TriangleCount`] below one, or a
/// [`SimplifyTarget::Fraction`] outside `(0, 1]`, is
/// [`MeshErrorCode::InvalidParameter`].
///
/// The output is byte-identical across runs; see the module documentation for
/// how ties are ordered.
pub fn simplify_quadric(mesh: &Mesh, target: SimplifyTarget) -> MeshResult<Mesh> {
    resolve_target(target, mesh.triangle_count()).and_then(|goal| {
        (goal >= mesh.triangle_count())
            .then(|| Ok(mesh.clone()))
            .unwrap_or_else(|| decimate(mesh, goal))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec4};

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn sheet() -> Mesh {
        Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
                Vec3::new(1.0, 0.0, 1.0),
            ],
            vec![0, 2, 1, 1, 2, 3],
        ))
        .unwrap()
    }

    /// A closed, counter-clockwise-outward octahedron.
    fn octahedron() -> Mesh {
        Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::UNIT_X,
                Vec3::new(-1.0, 0.0, 0.0),
                Vec3::UNIT_Y,
                Vec3::new(0.0, -1.0, 0.0),
                Vec3::UNIT_Z,
                Vec3::new(0.0, 0.0, -1.0),
            ],
            vec![
                0, 2, 4, 4, 2, 1, 1, 2, 5, 5, 2, 0, 4, 3, 0, 1, 3, 4, 5, 3, 1, 0, 3, 5,
            ],
        ))
        .unwrap()
    }

    /// The octahedron refined once by midpoint splitting and re-projected onto
    /// the unit sphere: 32 triangles of a genuinely curved surface, where the
    /// quadrics have full rank and the optimal-placement path is exercised.
    fn sphere() -> Mesh {
        let streams = crate::subdivision::subdivide_midpoint(
            &octahedron(),
            crate::tessellation::Subdivisions::new(1).unwrap(),
        )
        .unwrap()
        .into_streams();
        Mesh::from_streams(MeshStreams {
            positions: streams
                .positions
                .iter()
                .map(|p| p.normalize().unwrap())
                .collect(),
            ..streams
        })
        .unwrap()
    }

    fn extent(mesh: &Mesh) -> (Vec3, Vec3) {
        mesh.positions().iter().fold(
            (
                Vec3::new(f32::MAX, f32::MAX, f32::MAX),
                Vec3::new(f32::MIN, f32::MIN, f32::MIN),
            ),
            |(lo, hi), p| {
                (
                    Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)),
                    Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)),
                )
            },
        )
    }

    #[test]
    fn a_triangle_count_below_one_is_rejected() {
        assert_eq!(
            simplify_quadric(&sheet(), SimplifyTarget::TriangleCount(0))
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_fraction_outside_the_unit_interval_is_rejected() {
        assert_eq!(
            simplify_quadric(&sheet(), SimplifyTarget::Fraction(ratio(0.0)))
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            simplify_quadric(&sheet(), SimplifyTarget::Fraction(ratio(1.5)))
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            simplify_quadric(&sheet(), SimplifyTarget::Fraction(ratio(-0.25)))
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_target_at_or_above_the_current_count_returns_the_mesh_unchanged() {
        let m = sphere();
        assert_eq!(
            simplify_quadric(&m, SimplifyTarget::TriangleCount(m.triangle_count() as u32)).unwrap(),
            m
        );
        assert_eq!(
            simplify_quadric(&m, SimplifyTarget::TriangleCount(10_000)).unwrap(),
            m
        );
        assert_eq!(
            simplify_quadric(&m, SimplifyTarget::Fraction(ratio(1.0))).unwrap(),
            m
        );
    }

    #[test]
    fn a_tiny_fraction_still_asks_for_at_least_one_triangle() {
        assert_eq!(resolve_target(SimplifyTarget::Fraction(ratio(0.001)), 4), Ok(1));
        assert_eq!(resolve_target(SimplifyTarget::Fraction(ratio(0.5)), 32), Ok(16));
        assert_eq!(resolve_target(SimplifyTarget::TriangleCount(7), 32), Ok(7));
    }

    #[test]
    fn a_sphere_decimates_to_a_quarter_of_its_triangles_and_stays_valid() {
        let original = sphere();
        let quarter = (original.triangle_count() / 4) as u32;
        let reduced = simplify_quadric(&original, SimplifyTarget::TriangleCount(quarter)).unwrap();

        assert_eq!(original.triangle_count(), 32);
        assert!(reduced.triangle_count() <= quarter as usize);
        assert!(reduced.triangle_count() >= 1);
        // Still a structurally valid mesh, compacted: no orphaned vertices.
        let referenced: BTreeSet<u32> = reduced.indices().iter().copied().collect();
        assert_eq!(referenced.len(), reduced.vertex_count());
        assert_eq!(
            Mesh::from_streams(reduced.clone().into_streams()).unwrap(),
            reduced
        );

        // The decimated hull stays close to the original's extent: QEM keeps the
        // silhouette rather than shaving whole regions off.
        let (lo0, hi0) = extent(&original);
        let (lo1, hi1) = extent(&reduced);
        let slack = 0.35;
        assert!(lo1.x >= lo0.x - slack && hi1.x <= hi0.x + slack);
        assert!(lo1.y >= lo0.y - slack && hi1.y <= hi0.y + slack);
        assert!(lo1.z >= lo0.z - slack && hi1.z <= hi0.z + slack);
        assert!(hi1.x - lo1.x > (hi0.x - lo0.x) * 0.5);
    }

    #[test]
    fn the_same_input_decimates_to_byte_identical_output() {
        let m = sphere();
        let a = simplify_quadric(&m, SimplifyTarget::Fraction(ratio(0.25))).unwrap();
        let b = simplify_quadric(&m, SimplifyTarget::Fraction(ratio(0.25))).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.clone().into_streams(), b.into_streams());
        assert!(a.triangle_count() < m.triangle_count());
    }

    #[test]
    fn every_attribute_stream_survives_decimation() {
        let base = sphere().into_streams();
        let n = base.positions.len();
        let attributed = Mesh::from_streams(MeshStreams {
            normals: base.positions.iter().map(|p| p.normalize().unwrap()).collect(),
            uvs: (0..n).map(|i| Vec2::new(i as f32 * 0.01, 0.5)).collect(),
            tangents: vec![Vec4::new(1.0, 0.0, 0.0, 1.0); n],
            colors: vec![Vec4::new(0.2, 0.4, 0.6, 1.0); n],
            joints: vec![[0, 1, 2, 3]; n],
            weights: vec![[0.5, 0.5, 0.0, 0.0]; n],
            ..base
        })
        .unwrap();
        let reduced = simplify_quadric(&attributed, SimplifyTarget::TriangleCount(8)).unwrap();
        assert!(reduced.has_normals());
        assert!(reduced.has_uvs());
        assert!(reduced.has_tangents());
        assert!(reduced.has_colors());
        assert!(reduced.is_skinned());
        assert_eq!(reduced.normals().len(), reduced.vertex_count());
        assert_eq!(reduced.uvs().len(), reduced.vertex_count());
        assert_eq!(reduced.weights()[0], [0.5, 0.5, 0.0, 0.0]);
    }

    #[test]
    fn a_flat_sheet_decimates_through_the_singular_midpoint_placement() {
        // Every plane of a flat sheet is the same plane, so the quadric's 3x3
        // sub-system is rank one and the optimal placement does not exist.
        let flat = sheet();
        let reduced = simplify_quadric(&flat, SimplifyTarget::TriangleCount(1)).unwrap();
        assert!(reduced.triangle_count() <= 1);
        assert!(reduced.positions().iter().all(|p| p.y == 0.0));
    }

    #[test]
    fn a_singular_quadric_falls_back_and_a_full_rank_one_solves() {
        // One plane: rank one, no unique minimizer.
        let single = Quadric::from_plane(Vec3::UNIT_Y, 0.0);
        let fallback = Vec3::new(9.0, 9.0, 9.0);
        assert_eq!(single.optimal(fallback), fallback);

        // Three orthogonal planes meeting at (1, 2, 3): a unique minimizer.
        let corner = Quadric::from_plane(Vec3::UNIT_X, -1.0)
            .plus(Quadric::from_plane(Vec3::UNIT_Y, -2.0))
            .plus(Quadric::from_plane(Vec3::UNIT_Z, -3.0));
        let solved = corner.optimal(fallback);
        assert!((solved.x - 1.0).abs() < 1.0e-4);
        assert!((solved.y - 2.0).abs() < 1.0e-4);
        assert!((solved.z - 3.0).abs() < 1.0e-4);
        assert!(corner.error(solved).abs() < 1.0e-4);
        // And the form really does measure squared distance to those planes.
        assert!((corner.error(Vec3::new(2.0, 2.0, 3.0)) - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn a_degenerate_triangle_contributes_no_quadric() {
        let positions = vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_X];
        assert_eq!(triangle_quadric(&positions, [0, 1, 2]), Quadric::ZERO);
    }

    #[test]
    fn a_collapse_that_would_erase_the_last_triangle_is_rejected() {
        let lone = Mesh::from_streams(MeshStreams::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Z],
            vec![0, 1, 2],
        ))
        .unwrap();
        let state = Working::from_mesh(&lone);
        assert!(!valid_collapse(&state, (0, 1), Vec3::ZERO));
    }

    #[test]
    fn a_non_manifold_edge_is_rejected() {
        // Three triangles fanned around edge (0, 1): collapsing it would weld
        // three surface sheets into one.
        let fan = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::ZERO,
                Vec3::UNIT_X,
                Vec3::UNIT_Z,
                Vec3::UNIT_Y,
                Vec3::new(0.0, -1.0, 0.0),
            ],
            vec![0, 1, 2, 0, 1, 3, 0, 1, 4],
        ))
        .unwrap();
        let state = Working::from_mesh(&fan);
        assert!(!valid_collapse(&state, (0, 1), Vec3::new(0.5, 0.0, 0.0)));
        // A boundary edge of the same mesh is manifold and does not fold.
        assert!(valid_collapse(&state, (0, 2), Vec3::new(0.0, 0.0, 0.5)));
    }

    #[test]
    fn a_collapse_that_would_fold_a_neighbour_over_is_rejected() {
        let state = Working::from_mesh(&sheet());
        // Dragging vertex 1 past the far edge turns triangle (1, 2, 3) inside out.
        assert!(!valid_collapse(&state, (0, 1), Vec3::new(0.0, 0.0, 5.0)));
        // A modest placement on the same edge keeps every facing.
        assert!(valid_collapse(&state, (0, 1), Vec3::new(0.5, 0.0, 0.0)));
    }

    #[test]
    fn a_step_past_the_target_leaves_the_state_alone() {
        let mut state = Working::from_mesh(&sheet());
        let before = state.clone();
        collapse_step(&mut state, 2);
        assert_eq!(state, before);
    }

    #[test]
    fn decimation_drops_orphans_and_degenerate_triangles() {
        // An unreferenced vertex and a topologically degenerate triangle both
        // disappear when the result is compacted.
        let messy = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::ZERO,
                Vec3::UNIT_X,
                Vec3::UNIT_Z,
                Vec3::new(1.0, 0.0, 1.0),
                Vec3::new(7.0, 7.0, 7.0),
            ],
            vec![0, 2, 1, 1, 2, 3, 0, 1, 1],
        ))
        .unwrap();
        let reduced = simplify_quadric(&messy, SimplifyTarget::TriangleCount(2)).unwrap();
        assert!(reduced.triangle_count() <= 2);
        assert!(!reduced
            .positions()
            .contains(&Vec3::new(7.0, 7.0, 7.0)));
        let referenced: BTreeSet<u32> = reduced.indices().iter().copied().collect();
        assert_eq!(referenced.len(), reduced.vertex_count());
    }

    #[test]
    fn costs_quantize_to_a_reproducible_total_order() {
        // Sub-microunit differences are deliberately equal, so accumulation
        // noise cannot reorder two collapses.
        assert_eq!(quantize(1.0), 1_000_000);
        assert_eq!(quantize(1.0 + 1.0e-9), quantize(1.0));
        assert!(quantize(2.0) > quantize(1.0));
    }

    #[test]
    fn the_target_variants_report_only_their_own_payload() {
        assert_eq!(
            triangle_count_payload(SimplifyTarget::TriangleCount(12)),
            Some(12)
        );
        assert_eq!(triangle_count_payload(SimplifyTarget::Fraction(ratio(0.5))), None);
        assert_eq!(
            fraction_payload(SimplifyTarget::Fraction(ratio(0.5))),
            Some(ratio(0.5))
        );
        assert_eq!(fraction_payload(SimplifyTarget::TriangleCount(12)), None);
        assert_ne!(
            SimplifyTarget::TriangleCount(1),
            SimplifyTarget::TriangleCount(2)
        );
    }
}
