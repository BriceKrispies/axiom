//! The boulder, **baked from a mesh recipe** rather than authored as quads.
//!
//! Every other prop mesh in this app is either an engine primitive
//! (`Mesh::cube()`, `Mesh::cylinder()`) or a hand-authored fan of quads
//! ([`super::prop_meshes`]). The boulder was the one prop that was neither
//! honestly: [`super::scenery::PropKind::Rock`] is documented as "a boulder" and
//! was drawn as a **cube**, because a cube was the roundest thing the app had.
//!
//! It is not the roundest thing the *engine* has. `axiom-proc-mesh` is the
//! layer that bakes a [`RecipeGraph`] into neutral geometry, and its `Sphere`
//! source is, in its own words, "the genuinely round primitive the other
//! operators cannot fake". A sphere pushed around by `Displace` is a boulder,
//! and it costs one recipe.
//!
//! # Why the sphere is baked large and then scaled down
//!
//! `Displace` pushes each vertex along its normal by `amount × noise(position)`,
//! and the noise underneath it (`axiom_noise::value_noise`) is lattice noise on
//! the **unit** grid. A unit-box sphere (radius `0.5`) therefore samples inside a
//! single lattice cell in every direction: the "noise" it sees is one smooth
//! lobe, and the result is a sphere leaning slightly to one side rather than a
//! rock.
//!
//! So the recipe generates the sphere at [`NOISE_SPAN`] times its final size,
//! displaces it out there — where it spans several lattice cells and the noise
//! has real structure — and scales the result back into the unit box with a
//! `Transform`. The relief constant below is expressed in *final* units and
//! multiplied up, so the number in the source is the number you see.
//!
//! # What this deliberately does not do
//!
//! * **No `UVProject`.** Rocks are drawn with `palette.stone`, an untextured lit
//!   colour, so a UV node would be a step that changes nothing — a ceremonial
//!   node, and this file has no more business carrying one than a layer has
//!   carrying a ceremonial dependency. The sphere's own lat/long UVs ride along
//!   unused.
//! * **No re-derived normals.** `Displace` moves positions and keeps the input's
//!   normals, so a baked boulder has a lumpy silhouette lit as though it were
//!   still smooth. That is the right trade here: these are two-metre props seen
//!   from a car at racing speed, where the silhouette is the whole read and the
//!   shading gradient is not, and re-deriving normals is not something the
//!   operator vocabulary can express today.
//! * **No hill.** The distant horizon hills (`super::scenery::distant_hills`)
//!   are also `PropKind::Rock`, but they are spawned directly against the cube
//!   handle and are left there on purpose — see [`super::scenery_pool`] and the
//!   note on [`ROCK_RINGS`].

use axiom::prelude::{Handle, Mesh, MeshData, RunningApp};
use axiom_proc_mesh::{MeshBuffer, MeshOp, ProcMeshApi};
use axiom_recipe::{Param, RecipeGraph, RecipeId, Scalar};

/// The recipe's stable identity. Nothing else in this app authors a recipe, so
/// the id space starts here.
const ROCK_RECIPE_ID: u64 = 1;

/// The recipe's version. Bumping it re-keys the entropy stream every `Displace`
/// node draws from, so it changes the boulder's shape — which is exactly what a
/// version is for, and why it is a constant rather than a literal.
const ROCK_RECIPE_VERSION: u32 = 1;

/// The seed the boulder is baked at.
///
/// Fixed rather than taken from the course seed. One mesh is registered per prop
/// kind (that is what keeps two hundred boulders to a single draw call), so a
/// course-seeded bake would still produce exactly *one* boulder shape — it would
/// only make that shape unreviewable. A constant keeps the geometry pinned in
/// the golden resources artifact for a reason a reader can see.
const ROCK_SEED: u64 = 0x5230_434B_0000_0001;

/// How much larger than its final size the sphere is generated and displaced.
///
/// Six, so a unit-box boulder is displaced as a radius-3 sphere spanning six
/// cells of the unit noise lattice in each axis. Below about four the noise
/// degenerates into the single smooth lobe described in the module docs; well
/// above six the lumps become finer than [`ROCK_SEGMENTS`] can resolve and the
/// mesh just gets noisy rather than rocky.
const NOISE_SPAN: f32 = 6.0;

/// Latitude bands in the boulder sphere.
///
/// This and [`ROCK_SEGMENTS`] are the triangle budget, and the budget is set by
/// the *pool*, not by one rock: `PropKind::Rock` carries 240 live instances, and
/// a canyon fills 92% of its slots with them. At 5×8 a boulder is 80 triangles,
/// so a full pool is ~19k — the same order as the whole visible road (~36k), for
/// a kind that was 12 triangles as a cube. That is the trade, and it is worth
/// naming: this spends triangles, the resource the frame has in surplus, and
/// spends none of the resource it has run out of, which is draw calls. The pool
/// still registers one mesh and still issues one draw call per kind.
///
/// It is also why the horizon hills stay cubes. They reuse `PropKind::Rock` but
/// are spawned once, statically, at 24–70 m tall and 190–560 m out, sunk so that
/// only the top ~5% of their box clears the verge. What clears it on a cube is a
/// full-width flat crest — a mesa — and what clears it on a sphere is a narrow
/// cap less than half as wide. Swapping their mesh would shrink the horizon, not
/// improve it; giving them a real dome means changing where they *sit*, which is
/// a change to the look of the course rather than to the shape of a prop.
const ROCK_RINGS: u32 = 6;

/// Longitude divisions in the boulder sphere. See [`ROCK_RINGS`].
const ROCK_SEGMENTS: u32 = 10;

/// How far the surface may be pushed along its normals, in final (unit-box)
/// units.
///
/// This is a **ceiling, not the relief you get.** `Displace` scales by
/// `value_noise(…)`, which is bounded to `[-1, 1]` but — like any coherent
/// gradient noise — only approaches those bounds at a lattice corner it almost
/// never samples. Measured over this sphere the noise spans roughly ±0.35, so
/// the radius actually varies by about a third of the number below. The first
/// value tried here was `0.14`, chosen as "28% of the radius", and it produced a
/// mesh whose radii ran `0.436..0.549` — a sphere with a slight wobble, not a
/// boulder. `0.36` is the value that measures out to a radius range of roughly
/// `0.38..0.62`: no two faces of a boulder read alike, and no vertex is pushed
/// anywhere near through the far side.
const ROCK_RELIEF: f32 = 0.36;

/// The final shrink that fits the displaced sphere back inside the **unit box**.
///
/// Displacement is symmetric about the sphere's radius, so a boulder with enough
/// relief to be a boulder necessarily pushes past the radius it was cut from: at
/// [`ROCK_RELIEF`] the raw mesh reaches half-extents of `0.586 / 0.571 / 0.548`
/// against a box that is `0.5` in each axis. That overflow is not cosmetic. Every
/// rock and every horizon hill is placed by scaling this mesh by
/// `PropKind::half_extents`, and `super::scenery::prop_bounds` culls and seats it
/// on the ground using the *same* box — so a mesh that does not fit its box is a
/// prop that sinks into the verge and pops out of frame early.
///
/// Rather than shrink the relief until the overflow disappears (which is only
/// possible by making the boulder round again), the recipe cuts the sphere
/// smaller by exactly this factor, so the *displaced* result fills the box.
/// `0.85` is measured, not derived — it is `0.5 / 0.586`, rounded down — and
/// [`the_boulder_fills_its_unit_box`] pins it: change the seed, the relief or the
/// subdivision and that test reports the factor the new mesh needs.
const BOX_FIT: f32 = 0.85;

/// A scalar parameter word.
fn scalar(value: f32) -> Param {
    Param::scalar(Scalar::new(value))
}

/// The boulder recipe: a sphere generated large, displaced by noise out there,
/// and scaled back into the unit box.
///
/// Authored in code here. These are a few dozen bytes — `recipe.serialize()` is
/// the whole boulder — and in a fuller pipeline they would be shipped as data
/// rather than as this function.
pub fn rock_recipe() -> RecipeGraph {
    let mut graph = RecipeGraph::new(RecipeId::from_raw(ROCK_RECIPE_ID), ROCK_RECIPE_VERSION);
    let sphere = graph.add(
        MeshOp::Sphere as u16,
        vec![
            scalar(0.5 * NOISE_SPAN),
            Param::int(ROCK_RINGS),
            Param::int(ROCK_SEGMENTS),
        ],
        vec![],
    );
    let displaced = graph.add(
        MeshOp::Displace as u16,
        vec![scalar(ROCK_RELIEF * NOISE_SPAN)],
        vec![sphere],
    );
    let shrink = BOX_FIT / NOISE_SPAN;
    graph.add(
        MeshOp::Transform as u16,
        vec![
            scalar(0.0),
            scalar(0.0),
            scalar(0.0),
            scalar(shrink),
            scalar(shrink),
            scalar(shrink),
        ],
        vec![displaced],
    );
    graph
}

/// Bake the boulder into engine geometry, or `None` if the recipe fails to
/// evaluate.
///
/// The neutral [`MeshBuffer`] and `MeshData` are built on the same `axiom-math`
/// vector types, so this is a plain move of the four streams — the same
/// translation `apps/axiom-proc-player` does, and the reason the layer's output
/// is deliberately engine-free.
pub fn bake_rock() -> Option<MeshData> {
    ProcMeshApi::new()
        .bake(&rock_recipe(), ROCK_SEED)
        .ok()
        .map(|buffer| to_mesh_data(&buffer))
}

/// Move a neutral mesh buffer's streams into the engine's `MeshData`.
fn to_mesh_data(buffer: &MeshBuffer) -> MeshData {
    MeshData::new(
        buffer.positions().to_vec(),
        buffer.normals().to_vec(),
        buffer.uvs().to_vec(),
        buffer.indices().to_vec(),
    )
}

/// Register the boulder mesh, falling back to the cube it replaces if the bake
/// or the upload fails.
///
/// The fallback is the same one [`super::prop_meshes`] uses for the cone, the
/// palm crown and the shrub clump: a prop kind that fails to build must still
/// draw *something*, because a pool with no mesh is a hole in the roadside. It
/// is unreachable for the authored recipe — [`rock_bakes_to_a_lumpy_sphere`]
/// proves the bake succeeds — and exists so a future edit to the recipe cannot
/// silently empty the pool.
pub fn install_rock(app: &mut RunningApp) -> Handle<Mesh> {
    bake_rock()
        .and_then(|data| app.add_mesh_data(data).ok())
        .unwrap_or_else(|| app.add_mesh(Mesh::cube()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::prelude::{App, DefaultPlugins, Vec3, Window};

    fn baked() -> MeshBuffer {
        ProcMeshApi::new()
            .bake(&rock_recipe(), ROCK_SEED)
            .expect("the authored boulder recipe bakes")
    }

    /// The recipe is a legal graph, not merely one that happens to evaluate.
    #[test]
    fn the_rock_recipe_validates_and_round_trips() {
        let recipe = rock_recipe();
        assert!(recipe.validate().is_ok(), "the boulder recipe is a valid DAG");
        let bytes = recipe.serialize();
        let restored = RecipeGraph::deserialize(&bytes).expect("the recipe round-trips");
        assert_eq!(restored, recipe);
        assert!(
            bytes.len() < 256,
            "the whole boulder is a few dozen bytes of recipe, not a mesh: {}",
            bytes.len()
        );
    }

    /// **The point of the file.** The baked mesh has the sphere's topology and a
    /// radius that varies over the surface — which is what separates a boulder
    /// from the sphere it was cut from, and from the cube it replaces.
    #[test]
    fn rock_bakes_to_a_lumpy_sphere() {
        let mesh = baked();
        assert_eq!(
            mesh.vertex_count(),
            ((ROCK_RINGS + 1) * (ROCK_SEGMENTS + 1)) as usize
        );
        assert_eq!(
            mesh.triangle_count(),
            (ROCK_RINGS * ROCK_SEGMENTS * 2) as usize
        );
        let radii: Vec<f32> = mesh
            .positions()
            .iter()
            .map(|p| (p.x * p.x + p.y * p.y + p.z * p.z).sqrt())
            .collect();
        let min = radii.iter().copied().fold(f32::INFINITY, f32::min);
        let max = radii.iter().copied().fold(0.0f32, f32::max);
        assert!(
            max - min > 0.2,
            "a displaced boulder is not a sphere with a wobble — the relief has \
             to be big enough that no two faces read alike: radii {min}..{max}"
        );
    }

    /// **The constant this file is most likely to get wrong.** The boulder must
    /// fill the unit box every prop is placed and culled against — neither
    /// spilling out of it (props sink into the verge and pop early) nor rattling
    /// around inside it (rocks come out visibly smaller than the cube they
    /// replaced). If this fails, the number it prints is the new [`BOX_FIT`].
    #[test]
    fn the_boulder_fills_its_unit_box() {
        let mesh = baked();
        let half = mesh.positions().iter().fold(Vec3::ZERO, |acc, p| {
            Vec3::new(acc.x.max(p.x.abs()), acc.y.max(p.y.abs()), acc.z.max(p.z.abs()))
        });
        let widest = half.x.max(half.y).max(half.z);
        assert!(
            widest <= 0.5,
            "the boulder spills out of its box: {half:?} — rescale BOX_FIT to {}",
            BOX_FIT * 0.5 / widest
        );
        assert!(
            widest > 0.47,
            "the boulder rattles around inside its box: {half:?} — rescale BOX_FIT to {}",
            BOX_FIT * 0.5 / widest
        );
    }

    /// The bake is a pure function of the recipe and the pinned seed — so the
    /// boulder in the golden resources artifact is the boulder every course
    /// draws, run after run.
    #[test]
    fn baking_twice_gives_the_same_boulder() {
        assert_eq!(baked(), baked());
    }

    /// Every rock in the course shares this one mesh, so it must sit centred in
    /// the unit box the placement code scales by `PropKind::half_extents`.
    #[test]
    fn the_boulder_is_centred_on_the_origin() {
        let mesh = baked();
        let sum = mesh
            .positions()
            .iter()
            .fold(Vec3::ZERO, |acc, p| acc.add(*p));
        let centre = sum.mul_scalar(1.0 / mesh.vertex_count() as f32);
        assert!(
            centre.x.abs() < 0.1 && centre.y.abs() < 0.1 && centre.z.abs() < 0.1,
            "the boulder's centroid drifted off the origin: {centre:?}"
        );
    }

    /// The registered handle is a real generated mesh, not the cube fallback.
    #[test]
    fn install_registers_the_baked_mesh_not_the_cube() {
        let mut app = App::new()
            .window(Window::new(320, 200))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build();
        let rock = install_rock(&mut app);
        assert_ne!(rock, app.add_mesh(Mesh::cube()), "not the cube fallback");
    }
}
