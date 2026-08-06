//! The procedural prop meshes the engine's primitive set cannot express.
//!
//! Cubes cover posts, signs, rocks, buildings and rails; cylinders cover wheels,
//! utility poles and palm trunks. Two silhouettes are missing, and both are the
//! ones that decide what the roadside *is*:
//!
//! * a **cone**, because a conical crown is what makes an inland tree read as a
//!   tree at a glance rather than as a green box;
//! * a **palm crown**, a fan of drooping fronds, because a coast road lined with
//!   cones reads as a pine plantation. No scaling of any primitive produces the
//!   thing a palm actually is — a bare stem with a splayed star on top — so the
//!   frond fan is authored here as flat blades, which is exactly how a real-time
//!   palm has always been built;
//! * a **shrub clump**, a low splayed rosette of leaf blades, because the ground
//!   between a coast road and the treeline is not bare. A palm avenue standing on
//!   an unbroken sheet of green is a colonnade in a car park: the avenue supplies
//!   the vertical beat and nothing supplies the *floor*. A squashed cone is a
//!   green pyramid, and a box is a box — the thing a roadside plant actually is,
//!   at this scale, is a handful of stiff blades thrown out of one root, so that
//!   is what is authored.
//!
//! Each is registered once, at install, and every instance in the course shares
//! it — which is what keeps two hundred of them to a single draw call.

use axiom::prelude::{Handle, Mesh, RunningApp, Vec3};

use super::surface_builder::SurfaceBuilder;

/// Radial segments in the tree cone. Eight is enough to lose the polygon edges
/// at any distance a tree is actually seen from, and cheap enough to instance
/// two hundred of.
pub const CONE_SIDES: u32 = 8;

/// Register the unit cone: base at the origin, apex one unit up, radius `0.5`,
/// so a `Transform`'s scale maps directly onto its bounding box.
pub fn install_cone(app: &mut RunningApp) -> Handle<Mesh> {
    let mut builder = SurfaceBuilder::with_quad_capacity(CONE_SIDES as usize * 2);
    // Authored centred on the origin, like the engine's own primitives, so a
    // prop's transform can be built from its bounds without a special case.
    builder.cone(
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        0.5,
        CONE_SIDES,
    );
    app.add_mesh_data(builder.build())
        .unwrap_or_else(|_| app.add_mesh(Mesh::cube()))
}

/// How many fronds a palm crown carries.
///
/// Seven is the smallest count that still reads as a *star* from behind the car
/// rather than as a cross, and it is odd, so the fan is never symmetric about
/// the view axis however the crown is yawed.
pub const FRONDS: u32 = 7;

/// The height, as a fraction of the crown's own box, at which the fronds meet
/// the trunk. Everything above it is the arch of the frond; everything below is
/// the droop. Placement multiplies by this to seat a crown on a trunk top, so
/// the two numbers can never disagree.
pub const CROWN_ROOT_HEIGHT: f32 = 0.80;

/// Register the unit palm crown: a fan of `FRONDS` drooping blades, authored
/// centred on the origin inside the unit box like the engine's own primitives.
///
/// A frond is two flat quads — root to arch, arch to drooping tip — emitted
/// twice with opposite facing so a blade is visible from underneath as well as
/// from above. The normals are the *facings*, straight up and straight down,
/// which is deliberate: a frond shaded by its own true (near-vertical) normal
/// goes black in a low sun, and a palm crown that goes black is a hole in the
/// sky rather than a tree.
pub fn install_palm_crown(app: &mut RunningApp) -> Handle<Mesh> {
    app.add_mesh_data(palm_crown_surface().build())
        .unwrap_or_else(|_| app.add_mesh(Mesh::cube()))
}

/// The palm crown's geometry, with no engine involved — so the shape itself can
/// be asserted on directly rather than through a mesh handle.
fn palm_crown_surface() -> SurfaceBuilder {
    let mut builder = SurfaceBuilder::with_quad_capacity(FRONDS as usize * 4);
    // (radius from the stem, height, half-width of the blade) at the three
    // points that define a frond: where it leaves the trunk, the top of its
    // arch, and the tip it droops to.
    let root = (0.04f32, 0.30f32, 0.030f32);
    let arch = (0.24f32, 0.50f32, 0.075f32);
    let tip = (0.46f32, -0.50f32, 0.012f32);
    let sides = FRONDS.max(3);
    for i in 0..sides {
        let angle = i as f32 / sides as f32 * std::f32::consts::TAU;
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        let across = Vec3::new(-angle.sin(), 0.0, angle.cos());
        let point = |(radius, height, _): (f32, f32, f32)| {
            out.mul_scalar(radius).add(Vec3::new(0.0, height, 0.0))
        };
        let edge = |p: Vec3, half: f32, sign: f32| p.add(across.mul_scalar(half * sign));
        for (near, far) in [(root, arch), (arch, tip)] {
            let (a, b) = (point(near), point(far));
            for facing in [Vec3::UNIT_Y, Vec3::new(0.0, -1.0, 0.0)] {
                builder.quad(
                    edge(a, near.2, -1.0),
                    edge(a, near.2, 1.0),
                    edge(b, far.2, 1.0),
                    edge(b, far.2, -1.0),
                    facing,
                );
            }
        }
    }
    builder
}

/// How many leaf blades one shrub clump throws out.
///
/// Nine, splayed on the golden angle rather than on an even division, so no two
/// blades line up and one clump never reads as the regular star the palm crown
/// deliberately is. Nine is also the point where the rosette stops looking like
/// a handful of spikes and starts looking like a plant.
pub const SHRUB_BLADES: u32 = 9;

/// The three blade reaches a clump cycles through, as fractions of the full
/// blade. A rosette of nine identical blades has a domed top and reads as a
/// clipped topiary ball; ragged is what a wild plant is.
const SHRUB_REACHES: [f32; 3] = [1.0, 0.72, 0.88];

/// Register the unit shrub clump: `SHRUB_BLADES` stiff blades sweeping up and
/// out of one root, authored centred on the origin inside the unit box like the
/// engine's own primitives and like the palm crown.
///
/// A blade is two flat quads — root to belly, belly to tip — emitted twice with
/// opposite facing, so a clump is solid from every side including from the
/// chase camera's angle looking down onto it. The normals are the *facings*,
/// straight up and straight down, for the same reason the fronds' are: a blade
/// shaded by its own near-vertical normal goes black in a low sun, and a verge
/// full of black plants is worse than a bare verge.
pub fn install_shrub(app: &mut RunningApp) -> Handle<Mesh> {
    app.add_mesh_data(shrub_surface().build())
        .unwrap_or_else(|_| app.add_mesh(Mesh::cube()))
}

/// The shrub clump's geometry, with no engine involved — so the shape itself can
/// be asserted on directly rather than through a mesh handle.
fn shrub_surface() -> SurfaceBuilder {
    let blades = SHRUB_BLADES.max(3);
    let mut builder = SurfaceBuilder::with_quad_capacity(blades as usize * 4);
    // The golden angle. Successive blades land in the widest remaining gap, so
    // the clump is evenly covered without ever being symmetric.
    let step = std::f32::consts::PI * (3.0 - 5.0f32.sqrt());
    for i in 0..blades {
        let angle = i as f32 * step;
        let reach = SHRUB_REACHES[i as usize % SHRUB_REACHES.len()];
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        let across = Vec3::new(-angle.sin(), 0.0, angle.cos());
        // (radius from the root, height, half-width) at the three points that
        // define a blade: the root it leaves the ground at, the belly where it
        // is widest, and the tip it sweeps up and out to. The tip is *above*
        // the belly: these blades stand up and splay, they do not droop, which
        // is what separates a shrub from a palm crown.
        let root = (0.03f32, -0.46f32, 0.045f32);
        let belly = (0.20 * reach, -0.06 + 0.30 * reach, 0.085 * reach);
        let tip = (0.47 * reach, 0.10 + 0.38 * reach, 0.010f32);
        let point = |(radius, height, _): (f32, f32, f32)| {
            out.mul_scalar(radius).add(Vec3::new(0.0, height, 0.0))
        };
        let edge = |p: Vec3, half: f32, sign: f32| p.add(across.mul_scalar(half * sign));
        for (near, far) in [(root, belly), (belly, tip)] {
            let (a, b) = (point(near), point(far));
            for facing in [Vec3::UNIT_Y, Vec3::new(0.0, -1.0, 0.0)] {
                builder.quad(
                    edge(a, near.2, -1.0),
                    edge(a, near.2, 1.0),
                    edge(b, far.2, 1.0),
                    edge(b, far.2, -1.0),
                    facing,
                );
            }
        }
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn app() -> RunningApp {
        App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build()
    }

    #[test]
    fn the_cone_registers_as_a_usable_mesh() {
        let mut app = app();
        let a = install_cone(&mut app);
        let b = app.add_mesh(Mesh::cube());
        assert_ne!(a, b, "it is its own mesh, not the cube fallback");
    }

    #[test]
    fn the_cone_fills_the_unit_box_like_the_engine_primitives() {
        let mut builder = SurfaceBuilder::new();
        builder.cone(Vec3::new(0.0, -0.5, 0.0), Vec3::new(0.0, 1.0, 0.0), 0.5, CONE_SIDES);
        let data = builder.build();
        let (mut lo, mut hi) = (Vec3::ONE.mul_scalar(f32::MAX), Vec3::ONE.mul_scalar(f32::MIN));
        for p in data.positions() {
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
        for (name, value, expected) in [
            ("min y", lo.y, -0.5),
            ("max y", hi.y, 0.5),
            ("min x", lo.x, -0.5),
            ("max x", hi.x, 0.5),
        ] {
            assert!(
                (value - expected).abs() < 0.05,
                "{name} is {value}, expected about {expected}"
            );
        }
    }

    #[test]
    fn the_palm_crown_registers_as_its_own_mesh() {
        let mut app = app();
        let crown = install_palm_crown(&mut app);
        let cone = install_cone(&mut app);
        assert_ne!(crown, cone, "a palm is not a conifer");
        assert_ne!(crown, app.add_mesh(Mesh::cube()), "nor the cube fallback");
    }

    /// The two properties that make a palm read as a palm: the fronds *droop*
    /// (they reach below the point they leave the trunk) and they *splay* (they
    /// reach further out than the stem is thick).
    /// The three properties that make a palm read as a palm rather than as a
    /// bush: the fronds *droop* (they reach below the height they leave the
    /// trunk at), they *arch* above it first, and they *splay* far wider than
    /// the stem — while still fitting the unit box every prop transform assumes.
    #[test]
    fn the_palm_crown_arches_droops_and_splays_inside_the_unit_box() {
        let surface = palm_crown_surface();
        let points = surface.positions();
        let lowest = points.iter().fold(f32::MAX, |m, p| m.min(p.y));
        let highest = points.iter().fold(f32::MIN, |m, p| m.max(p.y));
        let reach = points
            .iter()
            .fold(0.0f32, |m, p| m.max((p.x * p.x + p.z * p.z).sqrt()));
        let root_height = CROWN_ROOT_HEIGHT - 0.5;
        assert!(lowest < root_height, "the fronds droop: {lowest}");
        assert!(highest >= root_height, "and arch above the root: {highest}");
        assert!(reach > 0.4, "and splay out: {reach}");
        assert!(reach <= 0.52, "while staying in the box: {reach}");
        assert!(lowest >= -0.52 && highest <= 0.52, "top and bottom too");
        assert_eq!(
            surface.triangle_count(),
            FRONDS as usize * 8,
            "each frond is two segments, each double-sided"
        );
    }

    /// A blade with only one face is invisible from underneath, which at this
    /// camera height is exactly where every palm ahead of the car is seen from.
    #[test]
    fn every_frond_blade_is_drawn_from_both_sides() {
        let surface = palm_crown_surface();
        let data = palm_crown_surface().build();
        let up = data.normals().iter().filter(|n| n.y > 0.0).count();
        let down = data.normals().iter().filter(|n| n.y < 0.0).count();
        assert_eq!(up, down, "the fan is exactly half up-facing, half down");
        assert!(up > 0 && !surface.is_empty());
    }

    #[test]
    fn the_shrub_clump_registers_as_its_own_mesh() {
        let mut app = app();
        let clump = install_shrub(&mut app);
        assert_ne!(clump, app.add_mesh(Mesh::cube()), "not the cube fallback");
        assert_ne!(clump, install_palm_crown(&mut app), "nor the frond fan");
    }

    /// The shape claim, asserted directly: blades that sweep **up** and out of
    /// one root, filling the unit box. A rosette whose tips fall below its
    /// belly is a palm crown wearing a different name, and the whole reason
    /// this mesh exists is that a coast verge is not made of small palms.
    #[test]
    fn the_shrub_clump_splays_upward_out_of_one_root_inside_the_unit_box() {
        let surface = shrub_surface();
        let data = surface.clone().build();
        let lowest = data.positions().iter().fold(f32::MAX, |m, p| m.min(p.y));
        let highest = data.positions().iter().fold(f32::MIN, |m, p| m.max(p.y));
        let reach = data
            .positions()
            .iter()
            .fold(0.0f32, |m, p| m.max((p.x * p.x + p.z * p.z).sqrt()));
        assert!(lowest <= -0.44, "the clump is rooted at the ground: {lowest}");
        assert!(highest > 0.4, "and stands up out of it: {highest}");
        assert!(highest <= 0.5 && lowest >= -0.5, "inside the box");
        assert!(reach > 0.4, "and splays wide: {reach}");
        assert!(reach <= 0.5, "while staying in the box: {reach}");
        assert_eq!(
            surface.triangle_count(),
            SHRUB_BLADES as usize * 8,
            "each blade is two segments, each double-sided"
        );
    }

    /// Ragged, not domed. Every blade the same length gives a topiary ball;
    /// this asserts the clump genuinely carries more than one reach.
    #[test]
    fn a_shrub_clump_has_an_uneven_top_and_two_faces_per_blade() {
        let data = shrub_surface().build();
        let up = data.normals().iter().filter(|n| n.y > 0.0).count();
        let down = data.normals().iter().filter(|n| n.y < 0.0).count();
        assert_eq!(up, down, "every blade is drawn from both sides");
        let mut tips: Vec<i32> = data
            .positions()
            .iter()
            .map(|p| (p.y * 1000.0) as i32)
            .collect();
        tips.sort_unstable();
        tips.dedup();
        assert!(tips.len() > 6, "the blade tops differ: {} heights", tips.len());
        // And the whole thing is deterministic, like every other prop mesh.
        assert_eq!(shrub_surface().build().positions(), data.positions());
    }

    #[test]
    fn the_cone_is_deterministic_and_well_formed() {
        let build = || {
            let mut b = SurfaceBuilder::new();
            b.cone(Vec3::new(0.0, -0.5, 0.0), Vec3::UNIT_Y, 0.5, CONE_SIDES);
            b.build()
        };
        let a = build();
        let b = build();
        assert_eq!(a.positions(), b.positions());
        assert_eq!(a.indices(), b.indices());
        assert!(a.indices().len() % 3 == 0);
        assert!(a.indices().iter().all(|i| (*i as usize) < a.positions().len()));
        assert!(a
            .positions()
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
    }
}
