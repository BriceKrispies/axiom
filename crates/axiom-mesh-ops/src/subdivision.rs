//! Recursive triangle refinement: 1-to-4 midpoint splitting, and Loop
//! subdivision.
//!
//! Both operators share one topological step — every triangle becomes four by
//! introducing a vertex on each edge — and differ entirely in *where the
//! vertices go*:
//!
//! - [`subdivide_midpoint`] is **interpolating**. A new vertex sits exactly
//!   half-way along its edge and no original vertex ever moves, so the result
//!   has the same silhouette as the input with four times the triangles. It is
//!   the right tool when the input already *is* the surface (a displaced
//!   heightfield, a sphere about to be re-projected onto its radius) and the
//!   only thing missing is resolution.
//! - [`subdivide_loop`] is **approximating**. It applies Charles Loop's masks,
//!   so both the new (odd) and the original (even) vertices move toward the
//!   limit surface: a closed mesh shrinks slightly and every crease is rounded.
//!   It is the right tool when the input is a control cage.
//!
//! Contrasting them is the cheapest way to see they are genuinely different
//! algorithms — the tests assert exactly that.
//!
//! ## Edge vertices are shared, not duplicated
//!
//! Each edge produces **one** vertex, deduplicated between the two triangles
//! that share it through a [`BTreeMap`] keyed on the sorted endpoint pair.
//! `BTreeMap`, never a hash map: the insertion order of the new vertices is part
//! of the output mesh, and hash iteration order is not reproducible. One
//! subdivision of one triangle therefore adds three vertices, not nine, and the
//! refined mesh stays welded.
//!
//! ## What happens to each attribute
//!
//! | stream | midpoint | Loop |
//! |---|---|---|
//! | positions | linear midpoint | odd/even position masks |
//! | uvs, colors | linear midpoint | the same masks, applied to the values |
//! | normals | linear midpoint, re-normalized | **regenerated** from the refined positions |
//! | tangents | midpoint of `xyz` re-normalized; `w` carried from the first (lower-index) endpoint | the masks on `xyz`, re-normalized; `w` carried from the first endpoint / the original vertex |
//! | joints + weights | per slot, the endpoint with the greater weight, re-normalized | odd: the same rule; even: unchanged |
//!
//! Normals are *regenerated* after Loop rather than smoothed with the position
//! mask because the mask is a rule about where a **point** goes, not about how a
//! surface turns: applying it to a normal field produces a normal that no longer
//! agrees with the geometry it labels. Area-weighted regeneration from the
//! refined positions is both cheaper to justify and visibly better. Midpoint
//! subdivision does not move any surface, so its normals may safely be
//! interpolated in place.
//!
//! An absent stream (empty, per the [`MeshStreams`] contract) stays absent: the
//! refinement of nothing is nothing.

use std::collections::{BTreeMap, BTreeSet};

use axiom_math::{Vec2, Vec3, Vec4};
use axiom_mesh::{generate_normals, Mesh, MeshResult, MeshStreams};

use crate::tessellation::Subdivisions;

/// One undirected edge, as its `(lower, higher)` endpoint indices.
type EdgeKey = (u32, u32);

/// The canonical key of the edge between `a` and `b`.
fn edge_key(a: u32, b: u32) -> EdgeKey {
    (a.min(b), a.max(b))
}

/// The three edges of a triangle, in winding order.
fn triangle_edges(triangle: &[u32]) -> [EdgeKey; 3] {
    [
        edge_key(triangle[0], triangle[1]),
        edge_key(triangle[1], triangle[2]),
        edge_key(triangle[2], triangle[0]),
    ]
}

/// An attribute value that can be combined by weight.
///
/// Every floating-point attribute stream shares one blending implementation
/// through this trait, so the odd/even masks are written once and applied
/// identically to positions, uvs, tangents, and colours. The integer skin
/// joints deliberately do **not** implement it — averaging a bone index is
/// meaningless.
trait Blend: Copy {
    /// The additive identity, used to seed a weighted sum.
    fn zero() -> Self;
    /// This value scaled by `k`.
    fn scale(self, k: f32) -> Self;
    /// The component-wise sum.
    fn plus(self, other: Self) -> Self;
}

impl Blend for Vec2 {
    fn zero() -> Self {
        Vec2::ZERO
    }
    fn scale(self, k: f32) -> Self {
        self.mul_scalar(k)
    }
    fn plus(self, other: Self) -> Self {
        self.add(other)
    }
}

impl Blend for Vec3 {
    fn zero() -> Self {
        Vec3::ZERO
    }
    fn scale(self, k: f32) -> Self {
        self.mul_scalar(k)
    }
    fn plus(self, other: Self) -> Self {
        self.add(other)
    }
}

impl Blend for Vec4 {
    fn zero() -> Self {
        Vec4::ZERO
    }
    fn scale(self, k: f32) -> Self {
        self.mul_scalar(k)
    }
    fn plus(self, other: Self) -> Self {
        self.add(other)
    }
}

/// The weighted sum of `terms`.
fn combine<T: Blend>(terms: &[(T, f32)]) -> T {
    terms
        .iter()
        .fold(T::zero(), |acc, &(value, weight)| acc.plus(value.scale(weight)))
}

/// The plain half-way blend of two attribute values.
fn midway<T: Blend>(a: T, b: T) -> T {
    combine(&[(a, 0.5), (b, 0.5)])
}

/// A midpoint normal, restored to unit length.
///
/// Two exactly opposite normals cancel and have no meaningful average; the first
/// endpoint's normal is the documented deterministic fallback, which keeps the
/// stream finite (the mesh contract's requirement) instead of failing the whole
/// refinement over one folded fin.
fn midway_normal(a: Vec3, b: Vec3) -> Vec3 {
    midway(a, b).normalize().unwrap_or(a)
}

/// The `xyz` part of `blended`, re-normalized, carrying `source`'s handedness.
///
/// Handedness is a discrete `±1` flag, not a quantity: averaging it would
/// produce `0` on a mirror seam and silently destroy the bitangent basis. The
/// **first (lower-index) endpoint's** `w` is carried instead — deterministic,
/// and correct everywhere except across a seam, where the two endpoints
/// disagreed to begin with.
fn rebuild_tangent(blended: Vec4, source: Vec4) -> Vec4 {
    let fallback = Vec3::new(source.x, source.y, source.z);
    let direction = Vec3::new(blended.x, blended.y, blended.z)
        .normalize()
        .unwrap_or(fallback);
    Vec4::new(direction.x, direction.y, direction.z, source.w)
}

/// The midpoint tangent of an edge.
fn midway_tangent(a: Vec4, b: Vec4) -> Vec4 {
    rebuild_tangent(midway(a, b), a)
}

/// The skin binding of a vertex introduced between two skinned endpoints.
///
/// **Rule:** slot by slot, take whichever endpoint binds that slot more
/// strongly, then re-normalize the four kept weights. Taking the stronger
/// influence keeps the new vertex bound to the bones that actually drive its
/// neighbourhood (a genuine average would need to *merge* two four-bone sets
/// into four slots, which cannot be done without dropping influences anyway),
/// and it is fully deterministic. Re-normalizing is what keeps the refined mesh
/// valid: the row must still sum to one.
///
/// The divisor cannot be zero — each endpoint row sums to one with non-negative
/// entries, so the slot-wise maxima sum to at least one.
fn dominant_skin(ja: [u16; 4], wa: [f32; 4], jb: [u16; 4], wb: [f32; 4]) -> ([u16; 4], [f32; 4]) {
    let picked: [(u16, f32); 4] = core::array::from_fn(|k| {
        let take_b = usize::from(wb[k] > wa[k]);
        ([ja[k], jb[k]][take_b], [wa[k], wb[k]][take_b])
    });
    let total: f32 = picked.iter().map(|&(_, w)| w).sum();
    (
        core::array::from_fn(|k| picked[k].0),
        core::array::from_fn(|k| picked[k].1 / total),
    )
}

/// Every distinct edge of a triangle list, in first-encounter order.
struct EdgeTable {
    /// The edges, in the order their vertices are appended to the mesh.
    order: Vec<EdgeKey>,
    /// Each edge's position in `order`.
    index: BTreeMap<EdgeKey, u32>,
}

impl EdgeTable {
    /// The refined-mesh vertex index of `edge`, given the original vertex count.
    fn vertex_of(&self, edge: EdgeKey, base: u32) -> u32 {
        base + self.index.get(&edge).copied().unwrap_or(0)
    }
}

/// Collect the deduplicated edge set of a triangle list.
///
/// The first-encounter order is a pure function of the index buffer, so the
/// refined vertex numbering is reproducible. An edge is new exactly when the
/// value `or_insert` left behind is the index we offered, because every
/// previously stored value is smaller than the current length.
fn build_edge_table(indices: &[u32]) -> EdgeTable {
    indices
        .chunks_exact(3)
        .flat_map(triangle_edges)
        .fold(
            EdgeTable {
                order: Vec::new(),
                index: BTreeMap::new(),
            },
            |mut table, edge| {
                let next = table.order.len() as u32;
                let stored = *table.index.entry(edge).or_insert(next);
                (stored == next).then(|| table.order.push(edge));
                table
            },
        )
}

/// Split every triangle into four, corner triangles first, centre triangle last.
///
/// Winding is preserved: each corner triangle keeps the parent's orientation,
/// and the centre triangle `(m01, m12, m20)` is the parent's orientation too.
fn split_triangles(indices: &[u32], table: &EdgeTable, base: u32) -> Vec<u32> {
    indices
        .chunks_exact(3)
        .flat_map(|t| {
            let m = triangle_edges(t).map(|e| table.vertex_of(e, base));
            [
                t[0], m[0], m[2], //
                m[0], t[1], m[1], //
                m[2], m[1], t[2], //
                m[0], m[1], m[2],
            ]
        })
        .collect()
}

/// Append one blended value per edge to a stream, leaving an absent stream absent.
fn extend_stream<T: Copy>(stream: &[T], edges: &[EdgeKey], blend: fn(T, T) -> T) -> Vec<T> {
    stream
        .iter()
        .copied()
        .chain(edges.iter().filter_map(|&(a, b)| {
            stream
                .get(a as usize)
                .zip(stream.get(b as usize))
                .map(|(&x, &y)| blend(x, y))
        }))
        .collect()
}

/// Append one skin binding per edge, leaving absent skin streams absent.
fn extend_skin(
    joints: &[[u16; 4]],
    weights: &[[f32; 4]],
    edges: &[EdgeKey],
) -> (Vec<[u16; 4]>, Vec<[f32; 4]>) {
    let added: Vec<([u16; 4], [f32; 4])> = edges
        .iter()
        .filter_map(|&(a, b)| {
            joints
                .get(a as usize)
                .zip(joints.get(b as usize))
                .zip(weights.get(a as usize).zip(weights.get(b as usize)))
                .map(|((&ja, &jb), (&wa, &wb))| dominant_skin(ja, wa, jb, wb))
        })
        .collect();
    (
        joints.iter().copied().chain(added.iter().map(|&(j, _)| j)).collect(),
        weights.iter().copied().chain(added.iter().map(|&(_, w)| w)).collect(),
    )
}

/// One level of 1-to-4 midpoint refinement.
fn refine_midpoint(mesh: &Mesh) -> MeshResult<Mesh> {
    let table = build_edge_table(mesh.indices());
    let base = mesh.vertex_count() as u32;
    let edges = table.order.as_slice();
    let (joints, weights) = extend_skin(mesh.joints(), mesh.weights(), edges);
    Mesh::from_streams(MeshStreams {
        positions: extend_stream(mesh.positions(), edges, midway),
        indices: split_triangles(mesh.indices(), &table, base),
        normals: extend_stream(mesh.normals(), edges, midway_normal),
        uvs: extend_stream(mesh.uvs(), edges, midway),
        tangents: extend_stream(mesh.tangents(), edges, midway_tangent),
        colors: extend_stream(mesh.colors(), edges, midway),
        joints,
        weights,
    })
}

/// The adjacency Loop's masks are defined over.
struct LoopTopology {
    /// For each edge, the opposite corner of every triangle sharing it. A
    /// boundary edge has exactly one; an interior edge has two.
    opposites: BTreeMap<EdgeKey, Vec<u32>>,
    /// For each vertex, the vertices it shares an edge with.
    neighbours: BTreeMap<u32, BTreeSet<u32>>,
    /// For each vertex that touches a boundary edge, its boundary neighbours
    /// only. A vertex absent from this map is interior.
    boundary_neighbours: BTreeMap<u32, BTreeSet<u32>>,
}

/// Map each edge to the opposite corners of the triangles sharing it.
fn build_opposites(indices: &[u32]) -> BTreeMap<EdgeKey, Vec<u32>> {
    indices
        .chunks_exact(3)
        .flat_map(|t| {
            [
                (edge_key(t[0], t[1]), t[2]),
                (edge_key(t[1], t[2]), t[0]),
                (edge_key(t[2], t[0]), t[1]),
            ]
        })
        .fold(BTreeMap::new(), |mut map, (edge, opposite)| {
            map.entry(edge).or_default().push(opposite);
            map
        })
}

/// Accumulate an undirected adjacency map from a stream of directed pairs.
fn accumulate_adjacency<I: Iterator<Item = (u32, u32)>>(pairs: I) -> BTreeMap<u32, BTreeSet<u32>> {
    pairs.fold(BTreeMap::new(), |mut map, (from, to)| {
        map.entry(from).or_default().insert(to);
        map
    })
}

/// Both directions of every edge, so each vertex learns its full one-ring.
fn build_neighbours(edges: &[EdgeKey]) -> BTreeMap<u32, BTreeSet<u32>> {
    accumulate_adjacency(edges.iter().flat_map(|&(a, b)| [(a, b), (b, a)]))
}

/// The one-ring restricted to boundary edges, for the vertices that have one.
fn build_boundary_neighbours(
    opposites: &BTreeMap<EdgeKey, Vec<u32>>,
) -> BTreeMap<u32, BTreeSet<u32>> {
    accumulate_adjacency(
        opposites
            .iter()
            .filter(|&(_, faces)| faces.len() < 2)
            .flat_map(|(&(a, b), _)| [(a, b), (b, a)]),
    )
}

/// Build the full adjacency Loop needs, deterministically.
fn build_topology(indices: &[u32], edges: &[EdgeKey]) -> LoopTopology {
    let opposites = build_opposites(indices);
    let boundary_neighbours = build_boundary_neighbours(&opposites);
    LoopTopology {
        opposites,
        neighbours: build_neighbours(edges),
        boundary_neighbours,
    }
}

/// Warren's smoothing coefficient for a vertex of valence `n`.
///
/// `3/16` at valence three, `3/(8n)` above it. Below valence three the general
/// formula is used as well: those vertices cannot occur on a closed manifold
/// interior, and reaching them through the general formula keeps the function
/// total. The valence is floored at one so an unreferenced vertex — which the
/// mesh contract permits — yields a finite coefficient instead of dividing by
/// zero; its neighbour sum is empty, so it stays exactly where it was.
fn warren_beta(valence: usize) -> f32 {
    let n = valence.max(1) as f32;
    [3.0 / (8.0 * n), 3.0 / 16.0][usize::from(valence == 3)]
}

/// The Loop **odd** (edge) value.
///
/// Interior: `3/8*(a + b) + 1/8*(c + d)`, where `c` and `d` are the opposite
/// corners of the two triangles sharing the edge. Boundary (one adjacent
/// triangle): `1/2*(a + b)`. A non-manifold edge with more than two adjacent
/// triangles uses the first two opposite corners in triangle order — a
/// deterministic reading of geometry that has no correct answer.
fn odd_value<T: Blend>(stream: &[T], edge: EdgeKey, topology: &LoopTopology) -> T {
    let (a, b) = edge;
    let faces = topology.opposites.get(&edge);
    let interior = usize::from(faces.map_or(0, Vec::len) >= 2);
    let c = faces.and_then(|f| f.first().copied()).unwrap_or(a);
    let d = faces.and_then(|f| f.get(1).copied()).unwrap_or(b);
    let end = [0.5, 0.375][interior];
    let opposite = [0.0, 0.125][interior];
    combine(&[
        (stream[a as usize], end),
        (stream[b as usize], end),
        (stream[c as usize], opposite),
        (stream[d as usize], opposite),
    ])
}

/// The Loop **even** (original vertex) value.
///
/// Interior: `(1 - n*beta)*v + beta*sum(neighbours)` with Warren's `beta`.
/// Boundary: `1/8*prev + 3/4*v + 1/8*next`, where `prev`/`next` are the vertex's
/// two boundary neighbours — taken as the lowest and highest index in the
/// boundary one-ring, which is the pair for a manifold boundary and a
/// deterministic choice for anything worse. A dangling boundary vertex with a
/// single neighbour reads it as both, degrading to `1/4*prev + 3/4*v`.
fn even_value<T: Blend>(stream: &[T], vertex: u32, topology: &LoopTopology) -> T {
    let ring = topology.neighbours.get(&vertex);
    let valence = ring.map_or(0, BTreeSet::len);
    let beta = warren_beta(valence);
    let ring_sum = ring.map_or(T::zero(), |set| {
        set.iter()
            .fold(T::zero(), |acc, &u| acc.plus(stream[u as usize]))
    });
    let interior = combine(&[
        (stream[vertex as usize], 1.0 - valence as f32 * beta),
        (ring_sum, beta),
    ]);

    let boundary_ring = topology.boundary_neighbours.get(&vertex);
    let previous = boundary_ring
        .and_then(|set| set.iter().next().copied())
        .unwrap_or(vertex);
    let next = boundary_ring
        .and_then(|set| set.iter().next_back().copied())
        .unwrap_or(vertex);
    let boundary = combine(&[
        (stream[previous as usize], 0.125),
        (stream[vertex as usize], 0.75),
        (stream[next as usize], 0.125),
    ]);

    [interior, boundary][usize::from(boundary_ring.is_some())]
}

/// Apply the even mask to every original value and the odd mask to every edge,
/// leaving an absent stream absent.
fn loop_stream<T: Blend>(stream: &[T], edges: &[EdgeKey], topology: &LoopTopology) -> Vec<T> {
    let present = &edges[..edges.len() * usize::from(!stream.is_empty())];
    (0..stream.len() as u32)
        .map(|v| even_value(stream, v, topology))
        .chain(present.iter().map(|&e| odd_value(stream, e, topology)))
        .collect()
}

/// The tangent whose handedness each refined vertex inherits: an original vertex
/// keeps its own, an edge vertex takes its first (lower-index) endpoint's.
fn tangent_sources(tangents: &[Vec4], edges: &[EdgeKey]) -> Vec<Vec4> {
    let present = &edges[..edges.len() * usize::from(!tangents.is_empty())];
    tangents
        .iter()
        .copied()
        .chain(present.iter().map(|&(a, _)| tangents[a as usize]))
        .collect()
}

/// Mask the tangent directions, then restore unit length and handedness.
fn loop_tangents(tangents: &[Vec4], edges: &[EdgeKey], topology: &LoopTopology) -> Vec<Vec4> {
    loop_stream(tangents, edges, topology)
        .into_iter()
        .zip(tangent_sources(tangents, edges))
        .map(|(blended, source)| rebuild_tangent(blended, source))
        .collect()
}

/// Regenerate normals only when the mesh carried them, so an unlit mesh does not
/// acquire a normal stream by being refined.
fn restore_normals(mesh: Mesh, wanted: bool) -> MeshResult<Mesh> {
    wanted
        .then(|| generate_normals(&mesh))
        .unwrap_or_else(|| Ok(mesh))
}

/// One level of Loop refinement.
fn refine_loop(mesh: &Mesh) -> MeshResult<Mesh> {
    let table = build_edge_table(mesh.indices());
    let edges = table.order.as_slice();
    let topology = build_topology(mesh.indices(), edges);
    let base = mesh.vertex_count() as u32;
    let (joints, weights) = extend_skin(mesh.joints(), mesh.weights(), edges);
    Mesh::from_streams(MeshStreams {
        positions: loop_stream(mesh.positions(), edges, &topology),
        indices: split_triangles(mesh.indices(), &table, base),
        uvs: loop_stream(mesh.uvs(), edges, &topology),
        tangents: loop_tangents(mesh.tangents(), edges, &topology),
        colors: loop_stream(mesh.colors(), edges, &topology),
        joints,
        weights,
        ..MeshStreams::default()
    })
    .and_then(|refined| restore_normals(refined, mesh.has_normals()))
}

/// Split every triangle into four, placing each new vertex at the exact midpoint
/// of its edge.
///
/// **Interpolating**: no original vertex moves, so the mesh's silhouette,
/// bounds, and creases are untouched — only its resolution changes. The
/// triangle count is multiplied by exactly four per level, and one vertex is
/// added per distinct edge (three for a lone triangle, not nine: edge vertices
/// are shared).
///
/// `levels = 0` returns the mesh unchanged.
pub fn subdivide_midpoint(mesh: &Mesh, levels: Subdivisions) -> MeshResult<Mesh> {
    (0..levels.get()).try_fold(mesh.clone(), |current, _| refine_midpoint(&current))
}

/// Refine with Charles Loop's approximating subdivision scheme.
///
/// **Approximating**: the original (even) vertices are repositioned toward the
/// limit surface along with the new (odd) ones, so a closed mesh contracts
/// slightly and creases round off. Boundary edges and boundary vertices use the
/// cubic-B-spline boundary masks, which keeps an open mesh's border curve
/// independent of the interior — an open edge stays an open edge, it does not
/// get pulled inward by the surface behind it.
///
/// The topology step is identical to [`subdivide_midpoint`]: four triangles per
/// triangle, one shared vertex per edge, per level. Normals are regenerated from
/// the refined positions rather than smoothed; see the module documentation.
///
/// `levels = 0` returns the mesh unchanged.
pub fn subdivide_loop(mesh: &Mesh, levels: Subdivisions) -> MeshResult<Mesh> {
    (0..levels.get()).try_fold(mesh.clone(), |current, _| refine_loop(&current))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_mesh::MeshErrorCode;

    fn levels(n: u32) -> Subdivisions {
        Subdivisions::new(n).unwrap()
    }

    fn triangle() -> Mesh {
        Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 2.0),
            ],
            vec![0, 1, 2],
        ))
        .unwrap()
    }

    /// A closed octahedron: every edge has two adjacent faces, every vertex has
    /// valence four, and every triangle is wound counter-clockwise from outside.
    fn octahedron() -> Mesh {
        let positions = vec![
            Vec3::UNIT_X,
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::UNIT_Y,
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::UNIT_Z,
            Vec3::new(0.0, 0.0, -1.0),
        ];
        let indices = vec![
            0, 2, 4, 4, 2, 1, 1, 2, 5, 5, 2, 0, 4, 3, 0, 1, 3, 4, 5, 3, 1, 0, 3, 5,
        ];
        Mesh::from_streams(MeshStreams::new(positions, indices)).unwrap()
    }

    /// Two triangles sharing edge (1,2): interior vertices exist, and so do
    /// boundary ones.
    fn quad_pair() -> Mesh {
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

    /// Every optional stream populated, on the two-triangle sheet.
    fn fully_attributed() -> Mesh {
        let base = quad_pair().into_streams();
        Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::UNIT_Y; 4],
            uvs: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0),
            ],
            tangents: vec![Vec4::new(1.0, 0.0, 0.0, 1.0); 4],
            colors: vec![
                Vec4::new(1.0, 0.0, 0.0, 1.0),
                Vec4::new(0.0, 1.0, 0.0, 1.0),
                Vec4::new(0.0, 0.0, 1.0, 1.0),
                Vec4::new(1.0, 1.0, 1.0, 1.0),
            ],
            joints: vec![[0, 1, 2, 3]; 4],
            weights: vec![
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.5, 0.5, 0.0, 0.0],
                [0.25, 0.25, 0.25, 0.25],
            ],
            ..base
        })
        .unwrap()
    }

    fn max_radius(mesh: &Mesh) -> f32 {
        mesh.positions()
            .iter()
            .map(|p| p.length())
            .fold(0.0f32, f32::max)
    }

    #[test]
    fn midpoint_level_zero_returns_the_input_unchanged() {
        let m = fully_attributed();
        assert_eq!(subdivide_midpoint(&m, levels(0)).unwrap(), m);
        assert_eq!(subdivide_loop(&m, levels(0)).unwrap(), m);
    }

    #[test]
    fn one_midpoint_level_shares_edge_vertices_between_neighbours() {
        // A lone triangle has three edges, so three new vertices — not nine.
        let refined = subdivide_midpoint(&triangle(), levels(1)).unwrap();
        assert_eq!(refined.triangle_count(), 4);
        assert_eq!(refined.vertex_count(), 6);

        // The two-triangle sheet has five edges (four border + one shared), so
        // the shared edge contributes exactly one vertex to both triangles.
        let sheet = subdivide_midpoint(&quad_pair(), levels(1)).unwrap();
        assert_eq!(sheet.triangle_count(), 8);
        assert_eq!(sheet.vertex_count(), 4 + 5);
    }

    #[test]
    fn midpoint_levels_multiply_the_triangle_count_by_four() {
        assert_eq!(
            subdivide_midpoint(&triangle(), levels(2))
                .unwrap()
                .triangle_count(),
            16
        );
        assert_eq!(
            subdivide_midpoint(&triangle(), levels(3))
                .unwrap()
                .triangle_count(),
            64
        );
    }

    #[test]
    fn midpoint_places_new_vertices_exactly_half_way() {
        let refined = subdivide_midpoint(&triangle(), levels(1)).unwrap();
        // Edge order is first-encounter: (0,1), (1,2), (0,2).
        assert_eq!(refined.positions()[3], Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(refined.positions()[4], Vec3::new(1.0, 0.0, 1.0));
        assert_eq!(refined.positions()[5], Vec3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn midpoint_interpolates_uvs_at_the_new_vertices() {
        let refined = subdivide_midpoint(&fully_attributed(), levels(1)).unwrap();
        // Edges in first-encounter order: (0,2), (1,2), (0,1), (2,3), (1,3),
        // so the new vertices are 4..=8 in that order.
        assert_eq!(refined.uvs()[4], Vec2::new(0.0, 0.5)); // between (0,0) and (0,1)
        assert_eq!(refined.uvs()[5], Vec2::new(0.5, 0.5)); // between (1,0) and (0,1)
        assert_eq!(refined.uvs()[6], Vec2::new(0.5, 0.0)); // between (0,0) and (1,0)
        assert_eq!(refined.uvs()[8], Vec2::new(1.0, 0.5)); // between (1,0) and (1,1)
    }

    #[test]
    fn midpoint_interpolates_every_other_present_stream() {
        let refined = subdivide_midpoint(&fully_attributed(), levels(1)).unwrap();
        assert_eq!(refined.vertex_count(), 9);
        // Normals were all +Y, so every midpoint normal is +Y.
        assert!(refined.normals().iter().all(|n| *n == Vec3::UNIT_Y));
        // Vertex 6 is the edge (0,1) midpoint. Colours blend linearly: red and
        // green meet at half of each.
        assert_eq!(refined.colors()[6], Vec4::new(0.5, 0.5, 0.0, 1.0));
        // Tangent direction is re-normalized and handedness is carried.
        assert_eq!(refined.tangents()[6], Vec4::new(1.0, 0.0, 0.0, 1.0));
        // Skin: slot-wise stronger endpoint, re-normalized. Between
        // [1,0,0,0] and [0,1,0,0] that is [1,1,0,0] / 2.
        assert_eq!(refined.weights()[6], [0.5, 0.5, 0.0, 0.0]);
        assert_eq!(refined.joints()[6], [0, 1, 2, 3]);
        let sums: Vec<f32> = refined.weights().iter().map(|w| w.iter().sum()).collect();
        assert!(sums.iter().all(|s| (s - 1.0).abs() < 1.0e-5));
    }

    #[test]
    fn midpoint_normal_blending_falls_back_when_two_normals_cancel() {
        let m = Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::UNIT_Y, Vec3::new(0.0, -1.0, 0.0), Vec3::UNIT_Y],
            ..triangle().into_streams()
        })
        .unwrap();
        let refined = subdivide_midpoint(&m, levels(1)).unwrap();
        // Edge (0,1) averages +Y with -Y: no direction, so the first endpoint wins.
        assert_eq!(refined.normals()[3], Vec3::UNIT_Y);
        assert!(refined.normals().iter().all(|n| n.length().is_finite()));
    }

    #[test]
    fn a_zero_length_tangent_falls_back_to_its_source_direction() {
        let m = Mesh::from_streams(MeshStreams {
            tangents: vec![
                Vec4::new(1.0, 0.0, 0.0, -1.0),
                Vec4::new(-1.0, 0.0, 0.0, 1.0),
                Vec4::new(1.0, 0.0, 0.0, -1.0),
            ],
            ..triangle().into_streams()
        })
        .unwrap();
        let refined = subdivide_midpoint(&m, levels(1)).unwrap();
        // Edge (0,1) cancels; the first endpoint's direction and handedness stand.
        assert_eq!(refined.tangents()[3], Vec4::new(1.0, 0.0, 0.0, -1.0));
    }

    #[test]
    fn midpoint_never_moves_an_original_vertex_but_loop_does() {
        let original = octahedron();
        let midpoint = subdivide_midpoint(&original, levels(1)).unwrap();
        assert_eq!(
            &midpoint.positions()[..original.vertex_count()],
            original.positions()
        );

        let looped = subdivide_loop(&original, levels(1)).unwrap();
        assert_ne!(
            &looped.positions()[..original.vertex_count()],
            original.positions()
        );

        // The two schemes agree on topology and disagree on geometry — the
        // proof they are different algorithms, not one wearing two names.
        assert_eq!(looped.triangle_count(), midpoint.triangle_count());
        assert_eq!(looped.vertex_count(), midpoint.vertex_count());
        assert_ne!(looped.positions(), midpoint.positions());
    }

    #[test]
    fn loop_contracts_a_closed_mesh_toward_its_limit_surface() {
        let original = octahedron();
        let before = max_radius(&original);
        let once = subdivide_loop(&original, levels(1)).unwrap();
        let twice = subdivide_loop(&original, levels(2)).unwrap();
        assert!(max_radius(&once) < before);
        assert!(max_radius(&twice) < max_radius(&once));
        // Midpoint, being interpolating, keeps the extreme radius exactly.
        assert_eq!(
            max_radius(&subdivide_midpoint(&original, levels(1)).unwrap()),
            before
        );
    }

    #[test]
    fn loop_keeps_a_closed_mesh_closed() {
        let refined = subdivide_loop(&octahedron(), levels(2)).unwrap();
        let opposites = build_opposites(refined.indices());
        assert!(!opposites.is_empty());
        assert!(opposites.values().all(|faces| faces.len() == 2));
    }

    #[test]
    fn loop_uses_the_boundary_rules_on_an_open_mesh_without_producing_nan() {
        let refined = subdivide_loop(&triangle(), levels(1)).unwrap();
        assert_eq!(refined.triangle_count(), 4);
        assert!(refined
            .positions()
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));

        // Every edge of a lone triangle is a boundary edge, so every odd vertex
        // is the plain edge midpoint.
        assert_eq!(refined.positions()[3], Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(refined.positions()[4], Vec3::new(1.0, 0.0, 1.0));
        assert_eq!(refined.positions()[5], Vec3::new(0.0, 0.0, 1.0));

        // Every vertex is a boundary vertex: 1/8*prev + 3/4*v + 1/8*next.
        // For vertex 0 the boundary neighbours are 1 and 2.
        let expected = Vec3::new(2.0, 0.0, 0.0)
            .mul_scalar(0.125)
            .add(Vec3::new(0.0, 0.0, 2.0).mul_scalar(0.125));
        assert_eq!(refined.positions()[0], expected);
    }

    #[test]
    fn loop_applies_the_interior_vertex_mask_at_valence_three_and_above() {
        // A vertex of valence 3 uses Warren's 3/16; the general 3/(8n) form
        // would give 1/8 and move the vertex further.
        assert_eq!(warren_beta(3), 3.0 / 16.0);
        assert_eq!(warren_beta(4), 3.0 / 32.0);
        assert_eq!(warren_beta(6), 3.0 / 48.0);
        // An unreferenced vertex is floored to valence one, so beta is finite.
        assert!(warren_beta(0).is_finite());
        assert_eq!(warren_beta(0), 3.0 / 8.0);
    }

    #[test]
    fn loop_leaves_an_unreferenced_vertex_where_it_was() {
        // A vertex no triangle mentions has no ring and no boundary ring: the
        // interior mask reduces to the identity, which is the only defensible
        // answer for a point with no surface around it.
        let m = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::ZERO,
                Vec3::UNIT_X,
                Vec3::UNIT_Z,
                Vec3::new(9.0, 9.0, 9.0),
            ],
            vec![0, 1, 2],
        ))
        .unwrap();
        let refined = subdivide_loop(&m, levels(1)).unwrap();
        assert_eq!(refined.positions()[3], Vec3::new(9.0, 9.0, 9.0));
    }

    #[test]
    fn loop_carries_every_attribute_stream_through_the_masks() {
        let refined = subdivide_loop(&fully_attributed(), levels(1)).unwrap();
        assert_eq!(refined.vertex_count(), 9);
        assert!(refined.has_normals());
        assert!(refined.has_uvs());
        assert!(refined.has_tangents());
        assert!(refined.has_colors());
        assert!(refined.is_skinned());

        // The sheet is flat and +Y-wound, so regenerated normals are all +Y.
        assert!(refined.normals().iter().all(|n| *n == Vec3::UNIT_Y));
        // Tangents stay unit-length with their handedness intact.
        assert!(refined.tangents().iter().all(|t| {
            let len = Vec3::new(t.x, t.y, t.z).length();
            (len - 1.0).abs() < 1.0e-5 && t.w == 1.0
        }));
        // Weights still normalize, so the mesh validates as skinned.
        assert!(refined
            .weights()
            .iter()
            .all(|w| (w.iter().sum::<f32>() - 1.0).abs() < 1.0e-5));
        // The even (original) uvs move too — that is what "approximating" means.
        // Vertex 0 is on the boundary, so 1/8*uv1 + 3/4*uv0 + 1/8*uv2.
        assert_eq!(refined.uvs()[0], Vec2::new(0.125, 0.125));
        assert_ne!(refined.uvs()[0], fully_attributed().uvs()[0]);
        // Vertex 5 is the odd vertex of the interior edge (1,2):
        // 3/8*(uv1 + uv2) + 1/8*(uv0 + uv3).
        assert_eq!(refined.uvs()[5], Vec2::new(0.5, 0.5));
    }

    #[test]
    fn loop_leaves_an_absent_normal_stream_absent() {
        let refined = subdivide_loop(&octahedron(), levels(1)).unwrap();
        assert!(!refined.has_normals());
        assert!(!refined.has_uvs());
        assert!(!refined.has_tangents());
        assert!(!refined.is_skinned());
    }

    #[test]
    fn both_schemes_are_reproducible() {
        let m = fully_attributed();
        assert_eq!(
            subdivide_midpoint(&m, levels(2)).unwrap(),
            subdivide_midpoint(&m, levels(2)).unwrap()
        );
        assert_eq!(
            subdivide_loop(&m, levels(2)).unwrap(),
            subdivide_loop(&m, levels(2)).unwrap()
        );
    }

    #[test]
    fn a_mesh_with_no_triangles_refines_to_itself() {
        // No edges means no new vertices and no new triangles; the operator
        // must still produce a valid mesh rather than an out-of-range index.
        let m = Mesh::from_streams(MeshStreams::new(vec![Vec3::ZERO], Vec::new())).unwrap();
        assert_eq!(subdivide_midpoint(&m, levels(2)).unwrap(), m);
        assert_eq!(subdivide_loop(&m, levels(1)).unwrap(), m);
    }

    #[test]
    fn a_non_manifold_edge_reads_its_first_two_faces() {
        // Three triangles fanned around edge (0,1). The edge is non-manifold;
        // Loop must still produce a finite, reproducible result.
        let m = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::ZERO,
                Vec3::UNIT_X,
                Vec3::UNIT_Z,
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, -1.0, 0.0),
            ],
            vec![0, 1, 2, 0, 1, 3, 0, 1, 4],
        ))
        .unwrap();
        let once = subdivide_loop(&m, levels(1)).unwrap();
        assert_eq!(once.triangle_count(), 12);
        assert!(once.positions().iter().all(|p| p.x.is_finite()));
        assert_eq!(subdivide_loop(&m, levels(1)).unwrap(), once);
    }

    #[test]
    fn refinement_reports_a_mesh_error_when_the_result_cannot_validate() {
        // A closed tetrahedron whose vertices all sit near the top of the f32
        // range. Loop's interior mask sums the whole one-ring before scaling it,
        // and three coordinates of 3e38 overflow to infinity — which the Mesh
        // contract rejects. The operator surfaces that rather than handing back
        // an invalid mesh.
        let far = 3.0e38_f32;
        let blown = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::new(far, 0.0, 0.0),
                Vec3::new(far, 1.0, 0.0),
                Vec3::new(far, 0.0, 1.0),
                Vec3::new(far, 1.0, 1.0),
            ],
            vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        ))
        .unwrap();
        assert_eq!(
            subdivide_loop(&blown, levels(1)).unwrap_err().code(),
            MeshErrorCode::NonFinitePosition
        );
    }
}
