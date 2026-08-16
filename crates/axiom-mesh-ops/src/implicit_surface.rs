//! Marching cubes over an **explicitly sampled** scalar field.
//!
//! The caller evaluates whatever field it likes — a signed distance function, a
//! metaball sum, a noise volume, a medical scan, a voxel occupancy grid — onto a
//! `cols x rows x depth` lattice and hands the values over. This operator finds
//! the iso-surface in that lattice and returns it as a mesh. It has no recipe
//! graph, no entropy source, no callback, and no opinion about what the numbers
//! mean; sampled data in, geometry out.
//!
//! **Why sampled data rather than a field callback.** A public `impl Fn`
//! parameter is forbidden across this engine's spine: a callback is an opaque
//! capability that could read a clock or a global, which would make the operator
//! unreplayable. A sampled lattice is a value — hashable, diffable, and
//! reproducible — which is the whole reason this layer exists.
//!
//! **Where the numbers come from.** [`ScalarField::sample`] is the producer that
//! sentence was waiting for: an [`axiom_field::FieldGraph`] is *also* a value —
//! hashable, diffable, canonically serializable — and it has no capability to
//! read a clock, because every external input it may read arrives in the
//! [`axiom_field::EvalContext`] the caller hands it. Evaluating one onto a
//! lattice therefore satisfies this operator's stated requirement exactly, where
//! a callback does not.
//!
//! # Lattice layout
//!
//! Value `(x, y, z)` lives at `values[(z * rows + y) * cols + x]`: `cols` indexes
//! `+X`, `rows` indexes `+Y`, `depth` indexes `+Z`. Node `(x, y, z)` sits at
//! `origin + (x, y, z) * spacing`.
//!
//! # Normals
//!
//! Not face normals. Each cube corner carries the **central-difference gradient
//! of the sampled field**, and an emitted edge vertex interpolates its two
//! corners' gradients by the same parameter that placed it. That is what makes
//! an implicit surface shade smoothly instead of showing the marching-cubes
//! facets: the gradient is the surface's real normal field, sampled where the
//! surface actually is, rather than a per-triangle average of a staircase. The
//! field convention is signed-distance-like — values rise going outward, so the
//! gradient points outward and is the outward normal directly. Where the
//! gradient vanishes (a flat plateau exactly at the iso value) the normal falls
//! back to `+Y`, the layer's deterministic default.

use axiom_field::{EvalContext, FieldGraph};
use axiom_kernel::{Meters, Seconds};
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{weld, Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

use crate::marching_cubes_tables::{MC_CORNER_OFFSET, MC_EDGE_CORNERS, MC_TRI_TABLE};
use crate::DetailBudget;

/// Numeric floor for the edge-interpolation division, so a cube edge whose two
/// corner values are equal still produces a defined (and clamped) parameter.
const EPSILON: f32 = 1.0e-6;

/// How small a fraction of the finest cell spacing two marching-cubes vertices
/// must be apart to survive welding as distinct vertices.
const WELD_FRACTION: f32 = 1.0e-3;

/// The level set an [`implicit_surface_mesh`] extraction follows.
///
/// A one-field newtype so the surface level cannot be confused with a spacing,
/// a radius, or a budget at a call site.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct IsoValue(f32);

impl IsoValue {
    /// Validate an iso level, rejecting `NaN` and infinities with
    /// [`MeshErrorCode::InvalidParameter`].
    pub fn new(value: f32) -> MeshResult<IsoValue> {
        value.is_finite().then_some(IsoValue(value)).ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "an iso value must be finite (no NaN, no Inf)",
            )
        })
    }

    /// The level.
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A sampled scalar field on a regular 3D lattice, stored X-fastest then Y then
/// Z: value `(x, y, z)` is entry `(z * rows + y) * cols + x`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarField {
    values: Vec<f32>,
    cols: u32,
    rows: u32,
    depth: u32,
}

impl ScalarField {
    /// Validate a sampled lattice: every dimension at least 2 and exactly
    /// `cols * rows * depth` values ([`MeshErrorCode::InvalidGridDimensions`]),
    /// every value finite ([`MeshErrorCode::NonFiniteAttribute`]).
    pub fn new(values: Vec<f32>, cols: u32, rows: u32, depth: u32) -> MeshResult<ScalarField> {
        let shaped = (cols >= 2)
            & (rows >= 2)
            & (depth >= 2)
            & (values.len() as u64 == u64::from(cols) * u64::from(rows) * u64::from(depth));
        let finite = values.iter().all(|v| v.is_finite());
        shaped
            .then_some(())
            .ok_or_else(|| {
                MeshError::new(
                    MeshErrorCode::InvalidGridDimensions,
                    "a scalar field needs every dimension >= 2 and exactly cols * rows * depth values",
                )
            })
            .and_then(|()| {
                finite.then_some(()).ok_or_else(|| {
                    MeshError::new(
                        MeshErrorCode::NonFiniteAttribute,
                        "every sampled scalar field value must be finite (no NaN, no Inf)",
                    )
                })
            })
            .map(|()| ScalarField {
                values,
                cols,
                rows,
                depth,
            })
    }

    /// Evaluate `graph` onto a `cols x rows x depth` lattice.
    ///
    /// Lattice node `(x, y, z)` is evaluated with the field's
    /// [`EvalContext::point`] set to `origin + (x, y, z) * spacing`, a zero `uv`,
    /// a `+Y` normal and zero time — a lattice has no surface parameterization
    /// and a baked volume is not animated, so those three inputs are the
    /// [`EvalContext::ORIGIN`] defaults. The result keeps the layout the rest of
    /// this module reads (`+X` fastest, then `+Y`, then `+Z`) and passes through
    /// the same dimension and finiteness validation as [`ScalarField::new`].
    ///
    /// **The field should be signed-distance-like.** This module's normals are
    /// the sampled field's gradient, taken as the *outward* normal directly (see
    /// the module docs), so a graph intended for this consumer must rise going
    /// outward — `length(point) - radius`, not `radius - length(point)`. A field
    /// with the opposite sign extracts the same surface with inverted normals.
    ///
    /// A graph whose value is a vector yields its first lane, which is
    /// [`axiom_field::FieldValue`]'s documented narrowing.
    ///
    /// Fails with [`MeshErrorCode::InvalidGridDimensions`] when the requested
    /// lattice would hold more nodes than the `u32` index arithmetic this module
    /// addresses it with can express, or when the dimensions are otherwise
    /// invalid, and with [`MeshErrorCode::InvalidParameter`] carrying the field
    /// layer's own message when the graph does not evaluate.
    pub fn sample(
        graph: &FieldGraph,
        origin: Vec3,
        spacing: Meters,
        cols: u32,
        rows: u32,
        depth: u32,
    ) -> MeshResult<ScalarField> {
        let count = u64::from(cols) * u64::from(rows) * u64::from(depth);
        (count <= u64::from(u32::MAX))
            .then_some(())
            .ok_or_else(|| {
                MeshError::new(
                    MeshErrorCode::InvalidGridDimensions,
                    "a sampled lattice may hold at most u32::MAX nodes, the width of the index \
                     arithmetic that addresses it",
                )
            })
            .and_then(|()| {
                (0..count as u32)
                    .map(|index| {
                        let step = spacing.get();
                        let point = origin.add(Vec3::new(
                            (index % cols) as f32 * step,
                            ((index / cols) % rows) as f32 * step,
                            (index / (cols * rows)) as f32 * step,
                        ));
                        graph
                            .evaluate(&EvalContext::new(
                                point,
                                Vec2::ZERO,
                                Vec3::UNIT_Y,
                                Seconds::finite_or_zero(0.0),
                            ))
                            .map(|value| value.as_scalar().get())
                    })
                    .collect::<Result<Vec<f32>, _>>()
                    .map_err(|error| {
                        MeshError::new(MeshErrorCode::InvalidParameter, error.message())
                    })
            })
            .and_then(|values| ScalarField::new(values, cols, rows, depth))
    }

    /// The number of samples along `+X`.
    pub const fn cols(&self) -> u32 {
        self.cols
    }

    /// The number of samples along `+Y`.
    pub const fn rows(&self) -> u32 {
        self.rows
    }

    /// The number of samples along `+Z`.
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// The sample at lattice node `(x, y, z)`.
    fn at(&self, x: u32, y: u32, z: u32) -> f32 {
        self.values[(((z * self.rows) + y) * self.cols + x) as usize]
    }
}

/// Where the lattice sits in space, how far apart its samples are, and how much
/// geometry the extraction may produce.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImplicitSurfaceOptions {
    /// World position of lattice node `(0, 0, 0)`.
    pub origin: Vec3,
    /// Distance between adjacent samples on each axis. Every component must be
    /// greater than zero.
    pub spacing: Vec3,
    /// The triangle ceiling. An extraction that would exceed it fails with
    /// [`MeshErrorCode::BudgetExceeded`] instead of allocating.
    pub budget: DetailBudget,
}

impl Default for ImplicitSurfaceOptions {
    /// A unit-spaced lattice at the origin with the default triangle budget.
    fn default() -> Self {
        ImplicitSurfaceOptions {
            origin: Vec3::ZERO,
            spacing: Vec3::ONE,
            budget: DetailBudget::default(),
        }
    }
}

/// The eight corner values of the cell whose low corner is lattice node `base`.
fn corner_values(field: &ScalarField, base: [u32; 3]) -> [f32; 8] {
    core::array::from_fn(|c| {
        let o = MC_CORNER_OFFSET[c];
        field.at(base[0] + o[0], base[1] + o[1], base[2] + o[2])
    })
}

/// The marching-cubes configuration index: bit `c` is set when corner `c` is
/// below the iso value. Accumulated arithmetically, never by a branch chain.
fn configuration(values: &[f32; 8], iso: f32) -> usize {
    (0..8).map(|c| usize::from(values[c] < iso) << c).sum()
}

/// How many triangles configuration `config` emits.
fn triangles_in(config: usize) -> usize {
    MC_TRI_TABLE[config].iter().take_while(|&&e| e >= 0).count() / 3
}

/// The low lattice corner of cell `cell`, in row-major cell order.
fn cell_base(cell: u32, cells: [u32; 3]) -> [u32; 3] {
    [
        cell % cells[0],
        (cell / cells[0]) % cells[1],
        cell / (cells[0] * cells[1]),
    ]
}

/// The central-difference gradient of the field at lattice node `(x, y, z)`,
/// clipped to the lattice at the borders (the divisor shrinks to the real
/// distance spanned, so the estimate stays correct on the boundary).
fn gradient(field: &ScalarField, x: u32, y: u32, z: u32, spacing: Vec3) -> Vec3 {
    let (x0, x1) = (x.saturating_sub(1), (x + 1).min(field.cols - 1));
    let (y0, y1) = (y.saturating_sub(1), (y + 1).min(field.rows - 1));
    let (z0, z1) = (z.saturating_sub(1), (z + 1).min(field.depth - 1));
    Vec3::new(
        (field.at(x1, y, z) - field.at(x0, y, z)) / ((x1 - x0) as f32 * spacing.x),
        (field.at(x, y1, z) - field.at(x, y0, z)) / ((y1 - y0) as f32 * spacing.y),
        (field.at(x, y, z1) - field.at(x, y, z0)) / ((z1 - z0) as f32 * spacing.z),
    )
}

/// The `(position, normal)` pairs one cell contributes, in emission order (each
/// run of three is a triangle). Empty when the cell is wholly inside or outside.
fn cell_vertices(
    field: &ScalarField,
    base: [u32; 3],
    iso: f32,
    options: ImplicitSurfaceOptions,
) -> Vec<(Vec3, Vec3)> {
    let values = corner_values(field, base);
    let spacing = options.spacing;
    let positions: [Vec3; 8] = core::array::from_fn(|c| {
        let o = MC_CORNER_OFFSET[c];
        options.origin.add(Vec3::new(
            (base[0] + o[0]) as f32 * spacing.x,
            (base[1] + o[1]) as f32 * spacing.y,
            (base[2] + o[2]) as f32 * spacing.z,
        ))
    });
    let gradients: [Vec3; 8] = core::array::from_fn(|c| {
        let o = MC_CORNER_OFFSET[c];
        gradient(
            field,
            base[0] + o[0],
            base[1] + o[1],
            base[2] + o[2],
            spacing,
        )
    });
    let row = MC_TRI_TABLE[configuration(&values, iso)];
    let live = row.iter().take_while(|&&e| e >= 0).count();
    // Each triple is emitted **reversed**. The Paul Bourke table, driven by the
    // "bit set when the corner is below the iso value" convention, winds each
    // triangle so its geometric normal points down the field gradient — into the
    // solid. Axiom's convention is the opposite (CCW front face, normal outward),
    // so `k + 2 - 2 * (k % 3)` reverses each run of three edge indices in place.
    // Verified exhaustively against every linear field direction and offset.
    (0..live)
        .map(|k| row[k + 2 - 2 * (k % 3)])
        .map(|e| {
            let [a, b] = MC_EDGE_CORNERS[e as usize];
            let (v0, v1) = (values[a], values[b]);
            let delta = v1 - v0;
            let denominator = delta + (delta.abs() < EPSILON) as i32 as f32 * EPSILON;
            let t = ((iso - v0) / denominator).clamp(0.0, 1.0);
            (
                positions[a].add(positions[b].subtract(positions[a]).mul_scalar(t)),
                gradients[a]
                    .add(gradients[b].subtract(gradients[a]).mul_scalar(t))
                    .normalize()
                    .unwrap_or(Vec3::UNIT_Y),
            )
        })
        .collect()
}

/// Extract the `iso` level set of a sampled scalar field as a mesh.
///
/// The extraction emits an unindexed triangle soup — marching cubes computes an
/// edge vertex once per cell, so every interior vertex appears in two to four
/// cells — and then runs it through [`axiom_mesh::weld`] with a tolerance of
/// `1e-3` of the finest cell spacing. Welding is what turns the soup into a
/// connected surface, and it is safe to do at that tolerance because duplicates
/// of the same edge vertex are computed from the same two corner values and are
/// therefore bit-identical, while genuinely distinct vertices are at least a
/// large fraction of a cell apart. The alternative — a hand-rolled edge-to-index
/// cache inside this operator — would reimplement, less well, the deduplication
/// the `mesh` layer already owns.
///
/// The surface carries positions and gradient normals but no UVs: a volumetric
/// field has no intrinsic surface parameterization, and inventing one (a planar
/// or spherical projection) would be a rendering policy this layer has no
/// business choosing.
///
/// A field that never crosses `iso` is a legal, empty result, not an error. It
/// is returned as a mesh with no triangles; since a [`Mesh`] must carry at least
/// one position, that mesh holds the single point `options.origin`.
///
/// Fails with [`MeshErrorCode::InvalidParameter`] if any spacing component is
/// not greater than zero, and [`MeshErrorCode::BudgetExceeded`] if the
/// extraction would exceed `options.budget`.
pub fn implicit_surface_mesh(
    field: &ScalarField,
    iso: IsoValue,
    options: ImplicitSurfaceOptions,
) -> MeshResult<Mesh> {
    let s = options.spacing;
    ((s.x > 0.0) & (s.y > 0.0) & (s.z > 0.0))
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "implicit surface spacing must be greater than zero on every axis",
            )
        })
        .and_then(|()| extract(field, iso.get(), options))
}

/// Count first, admit against the budget, then emit — so an over-budget request
/// is refused before any geometry is allocated.
fn extract(field: &ScalarField, iso: f32, options: ImplicitSurfaceOptions) -> MeshResult<Mesh> {
    let cells = [field.cols - 1, field.rows - 1, field.depth - 1];
    let cell_count = cells[0] * cells[1] * cells[2];
    let total: usize = (0..cell_count)
        .map(|c| triangles_in(configuration(&corner_values(field, cell_base(c, cells)), iso)))
        .sum();
    options.budget.admit(total).and_then(|()| {
        let (positions, normals): (Vec<Vec3>, Vec<Vec3>) = (0..cell_count)
            .flat_map(|c| cell_vertices(field, cell_base(c, cells), iso, options))
            .unzip();
        assemble(positions, normals, options)
    })
}

/// Build the soup mesh and weld it, or return the documented empty result.
fn assemble(
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    options: ImplicitSurfaceOptions,
) -> MeshResult<Mesh> {
    let crossed = !positions.is_empty();
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    let soup = Mesh::from_streams(MeshStreams {
        normals: crossed
            .then_some(normals)
            .unwrap_or_else(|| vec![Vec3::UNIT_Y]),
        ..MeshStreams::new(
            crossed
                .then_some(positions)
                .unwrap_or_else(|| vec![options.origin]),
            indices,
        )
    });
    let s = options.spacing;
    let tolerance = Meters::finite_or_zero(s.x.min(s.y).min(s.z) * WELD_FRACTION);
    soup.and_then(|mesh| {
        // Nothing to join when the surface never crossed the lattice.
        let joined = crossed.then_some(()).map(|()| weld(&mesh, tolerance));
        joined.unwrap_or(Ok(mesh))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldId, FieldOp};

    /// `length(point)` — the field whose `r` level set is the sphere of radius
    /// `r`. Signed-distance-like in this module's sense: it rises going outward,
    /// so its gradient is the outward normal.
    fn radius_field() -> FieldGraph {
        let (builder, point) = FieldBuilder::new(FieldId::of_name("mesh-ops/test/radius"), 1)
            .push(FieldOp::Point, Vec::new(), Vec::new());
        let (builder, length) = builder.push(FieldOp::Length, Vec::new(), vec![point]);
        builder.build(length)
    }

    /// A graph whose declared output names a node it does not contain (the id
    /// comes from a different builder), so every evaluation of it fails.
    fn dangling_field() -> FieldGraph {
        let (_, node) = FieldBuilder::new(FieldId::of_name("mesh-ops/test/other"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        FieldBuilder::new(FieldId::of_name("mesh-ops/test/dangling"), 1).build(node)
    }

    const SPHERE_RADIUS: f32 = 1.0;
    const SPHERE_STEPS: u32 = 17;
    const SPHERE_SPACING: f32 = 0.25;

    /// A signed-distance field of a unit sphere centred on the origin, sampled
    /// over `[-2, 2]^3` at `SPHERE_SPACING`.
    fn sphere_field() -> ScalarField {
        let n = SPHERE_STEPS;
        let values = (0..n * n * n)
            .map(|k| {
                let p = Vec3::new(
                    (k % n) as f32 * SPHERE_SPACING - 2.0,
                    ((k / n) % n) as f32 * SPHERE_SPACING - 2.0,
                    (k / (n * n)) as f32 * SPHERE_SPACING - 2.0,
                );
                p.length() - SPHERE_RADIUS
            })
            .collect();
        ScalarField::new(values, n, n, n).unwrap()
    }

    fn sphere_options() -> ImplicitSurfaceOptions {
        ImplicitSurfaceOptions {
            origin: Vec3::new(-2.0, -2.0, -2.0),
            spacing: Vec3::new(SPHERE_SPACING, SPHERE_SPACING, SPHERE_SPACING),
            ..ImplicitSurfaceOptions::default()
        }
    }

    fn constant_field(value: f32) -> ScalarField {
        ScalarField::new(vec![value; 27], 3, 3, 3).unwrap()
    }

    #[test]
    fn an_iso_value_carries_a_finite_level() {
        assert_eq!(IsoValue::new(-0.5).unwrap().get(), -0.5);
        assert_eq!(
            IsoValue::new(f32::NAN).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            IsoValue::new(f32::INFINITY).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn sampling_a_field_reproduces_hand_computed_lattice_values() {
        let origin = Vec3::new(-2.0, -2.0, -2.0);
        let spacing = Meters::finite_or_zero(SPHERE_SPACING);
        let n = SPHERE_STEPS;
        let sampled =
            ScalarField::sample(&radius_field(), origin, spacing, n, n, n).unwrap();
        assert_eq!((sampled.cols(), sampled.rows(), sampled.depth()), (n, n, n));

        // The layout is X-fastest, then Y, then Z, and node (x, y, z) sits at
        // origin + (x, y, z) * spacing — checked against the value computed here.
        let node_at = |x: u32, y: u32, z: u32| {
            origin
                .add(Vec3::new(
                    x as f32 * SPHERE_SPACING,
                    y as f32 * SPHERE_SPACING,
                    z as f32 * SPHERE_SPACING,
                ))
                .length()
        };
        assert_eq!(sampled.at(0, 0, 0), node_at(0, 0, 0));
        assert_eq!(sampled.at(0, 0, 0), 12.0_f32.sqrt());
        assert_eq!(sampled.at(8, 8, 8), 0.0); // the lattice centre is the origin
        assert_eq!(sampled.at(16, 8, 8), 2.0); // +X extreme, on the axis
        assert_eq!(sampled.at(1, 2, 3), node_at(1, 2, 3));
        assert_eq!(sampled.at(16, 16, 16), node_at(16, 16, 16));

        // Every node, not just the named ones: the whole lattice is the field.
        let expected: Vec<f32> = (0..n * n * n)
            .map(|k| node_at(k % n, (k / n) % n, k / (n * n)))
            .collect();
        assert_eq!(sampled, ScalarField::new(expected, n, n, n).unwrap());
    }

    #[test]
    fn sampling_is_deterministic_and_reads_a_vector_fields_first_lane() {
        let spacing = Meters::finite_or_zero(0.5);
        let once = ScalarField::sample(&radius_field(), Vec3::ZERO, spacing, 2, 2, 2).unwrap();
        let twice = ScalarField::sample(&radius_field(), Vec3::ZERO, spacing, 2, 2, 2).unwrap();
        assert_eq!(once, twice);

        // A vector-valued field narrows to its first lane — `point.x` here.
        let (builder, point) = FieldBuilder::new(FieldId::of_name("mesh-ops/test/point"), 1)
            .push(FieldOp::Point, Vec::new(), Vec::new());
        let vector = builder.build(point);
        let lanes = ScalarField::sample(&vector, Vec3::ZERO, spacing, 2, 2, 2).unwrap();
        assert_eq!(lanes.at(0, 1, 1), 0.0);
        assert_eq!(lanes.at(1, 0, 1), 0.5);
    }

    #[test]
    fn a_lattice_wider_than_its_own_index_arithmetic_is_refused() {
        // 65536 * 65536 * 2 exceeds u32::MAX nodes: refused before anything is
        // evaluated or allocated.
        assert_eq!(
            ScalarField::sample(
                &radius_field(),
                Vec3::ZERO,
                Meters::finite_or_zero(1.0),
                65_536,
                65_536,
                2,
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::InvalidGridDimensions
        );
        // A too-thin lattice fails the same validation `ScalarField::new` applies.
        assert_eq!(
            ScalarField::sample(
                &radius_field(),
                Vec3::ZERO,
                Meters::finite_or_zero(1.0),
                1,
                2,
                2,
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::InvalidGridDimensions
        );
    }

    #[test]
    fn a_field_that_does_not_evaluate_fails_the_sampling() {
        let error = ScalarField::sample(
            &dangling_field(),
            Vec3::ZERO,
            Meters::finite_or_zero(1.0),
            2,
            2,
            2,
        )
        .unwrap_err();
        assert_eq!(error.code(), MeshErrorCode::InvalidParameter);
        assert!(!error.message().is_empty());
    }

    #[test]
    fn a_sampled_field_drives_the_extraction_it_was_written_for() {
        // The golden: `length(point)` sampled over [-2, 2]^3 at 0.25, extracted
        // at iso = 1, is the unit sphere — the same surface the hand-written
        // `length(point) - 1` lattice extracts at iso = 0.
        let n = SPHERE_STEPS;
        let sampled = ScalarField::sample(
            &radius_field(),
            Vec3::new(-2.0, -2.0, -2.0),
            Meters::finite_or_zero(SPHERE_SPACING),
            n,
            n,
            n,
        )
        .unwrap();
        let mesh = implicit_surface_mesh(&sampled, IsoValue::new(1.0).unwrap(), sphere_options())
            .unwrap();
        assert_eq!(mesh.positions().len(), 270);
        assert_eq!(mesh.indices().len(), 1608);
        assert!(mesh
            .positions()
            .iter()
            .all(|p| (p.length() - SPHERE_RADIUS).abs() < 0.03));
        // Outward normals, the convention a rising-outward field produces.
        assert!(mesh
            .normals()
            .iter()
            .zip(mesh.positions())
            .all(|(n, p)| n.dot(*p) > 0.0));
    }

    #[test]
    fn a_field_reports_its_lattice_shape() {
        let f = ScalarField::new(vec![0.0; 24], 2, 3, 4).unwrap();
        assert_eq!(f.cols(), 2);
        assert_eq!(f.rows(), 3);
        assert_eq!(f.depth(), 4);
    }

    #[test]
    fn a_thin_or_mismatched_lattice_is_rejected() {
        for (n, c, r, d) in [
            (2_usize, 1_u32, 2_u32, 2_u32),
            (2, 2, 1, 2),
            (2, 2, 2, 1),
            (7, 2, 2, 2),
        ] {
            assert_eq!(
                ScalarField::new(vec![0.0; n], c, r, d).unwrap_err().code(),
                MeshErrorCode::InvalidGridDimensions
            );
        }
    }

    #[test]
    fn a_non_finite_sample_is_rejected() {
        let mut values = vec![0.0; 8];
        values[5] = f32::NAN;
        assert_eq!(
            ScalarField::new(values, 2, 2, 2).unwrap_err().code(),
            MeshErrorCode::NonFiniteAttribute
        );
    }

    #[test]
    fn a_non_positive_spacing_is_rejected() {
        let iso = IsoValue::new(0.0).unwrap();
        for spacing in [
            Vec3::new(0.0, 1.0, 1.0),
            Vec3::new(1.0, -1.0, 1.0),
            Vec3::new(1.0, 1.0, 0.0),
        ] {
            let options = ImplicitSurfaceOptions {
                spacing,
                ..ImplicitSurfaceOptions::default()
            };
            assert_eq!(
                implicit_surface_mesh(&constant_field(-1.0), iso, options)
                    .unwrap_err()
                    .code(),
                MeshErrorCode::InvalidParameter
            );
        }
    }

    #[test]
    fn a_sampled_sphere_field_extracts_the_true_sphere() {
        // The correctness proof: every extracted vertex sits on the sphere.
        let mesh = implicit_surface_mesh(
            &sphere_field(),
            IsoValue::new(0.0).unwrap(),
            sphere_options(),
        )
        .unwrap();
        let triangles = mesh.triangle_count();
        assert!(triangles > 100, "only {triangles} triangles");
        let worst = mesh
            .positions()
            .iter()
            .map(|p| (p.length() - SPHERE_RADIUS).abs())
            .fold(0.0_f32, f32::max);
        // A fifth of one cell: the surface really is the sphere, not a blob
        // near it.
        assert!(worst < SPHERE_SPACING * 0.2, "worst radial error {worst}");
    }

    #[test]
    fn the_gradient_normals_on_a_sphere_are_radial_and_unit_length() {
        let mesh = implicit_surface_mesh(
            &sphere_field(),
            IsoValue::new(0.0).unwrap(),
            sphere_options(),
        )
        .unwrap();
        assert!(mesh.has_normals());
        mesh.positions()
            .iter()
            .zip(mesh.normals())
            .for_each(|(p, n)| {
                assert!((n.length() - 1.0).abs() < 0.05, "normal {n:?}");
                let radial = p.normalize().unwrap();
                assert!(
                    n.normalize().unwrap().dot(radial) > 0.9,
                    "normal {n:?} at {p:?} is not radial"
                );
            });
        // No UVs: a volumetric field has no intrinsic parameterization.
        assert!(!mesh.has_uvs());
    }

    #[test]
    fn the_extracted_sphere_winds_counter_clockwise_outward() {
        let mesh = implicit_surface_mesh(
            &sphere_field(),
            IsoValue::new(0.0).unwrap(),
            sphere_options(),
        )
        .unwrap();
        let (p, i) = (mesh.positions(), mesh.indices());
        (0..mesh.triangle_count()).for_each(|t| {
            let (a, b, c) = (
                p[i[t * 3] as usize],
                p[i[t * 3 + 1] as usize],
                p[i[t * 3 + 2] as usize],
            );
            let face = b.subtract(a).cross(c.subtract(a));
            let outward = a.add(b).add(c).mul_scalar(1.0 / 3.0);
            assert!(face.dot(outward) > 0.0, "triangle {t} faces inward");
        });
    }

    #[test]
    fn welding_joins_the_soup_into_a_shared_surface() {
        let mesh = implicit_surface_mesh(
            &sphere_field(),
            IsoValue::new(0.0).unwrap(),
            sphere_options(),
        )
        .unwrap();
        // An unwelded soup has exactly three vertices per triangle; a welded
        // closed surface has far fewer (Euler: V ~ T/2 for a triangulation).
        let (vertices, triangles) = (mesh.vertex_count(), mesh.triangle_count());
        assert!(
            vertices < triangles * 3,
            "{vertices} vertices for {triangles} triangles"
        );
    }

    #[test]
    fn a_raised_iso_level_extracts_a_larger_sphere() {
        // The field is a distance, so iso = 0.5 is the radius-1.5 sphere.
        let mesh = implicit_surface_mesh(
            &sphere_field(),
            IsoValue::new(0.5).unwrap(),
            sphere_options(),
        )
        .unwrap();
        let worst = mesh
            .positions()
            .iter()
            .map(|p| (p.length() - 1.5).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < SPHERE_SPACING * 0.2, "worst radial error {worst}");
    }

    #[test]
    fn a_field_wholly_on_one_side_of_the_iso_level_is_empty_not_an_error() {
        let iso = IsoValue::new(0.0).unwrap();
        let options = ImplicitSurfaceOptions::default();
        for value in [-1.0_f32, 1.0] {
            let mesh = implicit_surface_mesh(&constant_field(value), iso, options).unwrap();
            assert_eq!(mesh.triangle_count(), 0);
            assert_eq!(mesh.positions(), &[options.origin]);
            assert_eq!(mesh.normals(), &[Vec3::UNIT_Y]);
        }
    }

    #[test]
    fn an_over_budget_extraction_is_refused_before_it_allocates() {
        let options = ImplicitSurfaceOptions {
            budget: DetailBudget::new(4).unwrap(),
            ..sphere_options()
        };
        assert_eq!(
            implicit_surface_mesh(&sphere_field(), IsoValue::new(0.0).unwrap(), options)
                .unwrap_err()
                .code(),
            MeshErrorCode::BudgetExceeded
        );
    }

    #[test]
    fn the_default_options_are_a_unit_lattice_at_the_origin() {
        let d = ImplicitSurfaceOptions::default();
        assert_eq!(d.origin, Vec3::ZERO);
        assert_eq!(d.spacing, Vec3::ONE);
        assert_eq!(d.budget, DetailBudget::default());
    }

    #[test]
    fn a_flat_step_field_crosses_on_a_plane() {
        // A field that is exactly the x coordinate: the iso-0 surface is the
        // plane x = 0, and every vertex must land on it with a +X normal. This
        // also exercises the equal-corner-values guard on every cube edge that
        // runs along y or z.
        let values = (0..3 * 3 * 3)
            .map(|k| (k % 3) as f32 - 1.0)
            .collect::<Vec<f32>>();
        let field = ScalarField::new(values, 3, 3, 3).unwrap();
        let mesh = implicit_surface_mesh(
            &field,
            IsoValue::new(0.0).unwrap(),
            ImplicitSurfaceOptions {
                origin: Vec3::new(-1.0, 0.0, 0.0),
                ..ImplicitSurfaceOptions::default()
            },
        )
        .unwrap();
        assert!(mesh.triangle_count() > 0);
        assert!(mesh.positions().iter().all(|p| p.x.abs() < 1.0e-5));
        assert!(mesh
            .normals()
            .iter()
            .all(|n| (n.subtract(Vec3::UNIT_X)).length() < 1.0e-5));
    }
}
