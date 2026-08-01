//! The one procedural prop mesh the engine's primitive set cannot express.
//!
//! Cubes cover posts, signs, rocks, buildings and rails; cylinders cover wheels
//! and utility poles. The only silhouette missing is a **cone**, and it is the
//! one that matters most for the roadside, because a conical crown is what makes
//! a tree read as a tree at a glance rather than as a green box.
//!
//! It is registered once, at install, and every tree in the course shares it —
//! which is what keeps two hundred trees to a single draw call.

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
