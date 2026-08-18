//! Ported from Claude-of-Duty `src/world/kit.js:74-110` — `facadeWall`, a
//! facade panel with real openings.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::noise::fbm3;

use super::{solid_slabs, wall_panel, WallHole, WallPanelOpts, WallTop};

/// `facadeWall(A, pm, spec)`'s `spec` (`kit.js:75`). Rust has no default
/// arguments; the source's defaults are named here: `bevel = 0.022`,
/// `top = Flat { jag: 0.0 }`, `warp = 0.018`.
pub struct FacadeSpec<'a> {
    pub w: f32,
    pub h: f32,
    pub t: f32,
    pub key: &'a str,
    pub openings: &'a [WallHole],
    pub rng: Option<&'a mut Rng>,
    pub bevel: f32,
    pub top: WallTop,
    pub warp: f32,
    /// `spec.paint` (`kit.js:102`): an optional per-vertex paint callback
    /// forwarded straight into [`AccumAddOpts::paint`].
    pub paint: Option<&'a mut dyn FnMut(f32, f32, f32, f32, f32, f32, &mut [f32; 3])>,
}

/// `facadeWall(A, pm, spec)` (`kit.js:74-110`): builds the wall panel
/// (`wallPanel`, always with `curveSegments: 7` — a hardcoded literal in the
/// source distinct from `wallPanel`'s own default of `6`), bows the face by
/// a few millimetres of `fbm3` ("nothing perfectly flat", `kit.js:84`) where
/// the surface normal is close to `+-Z`, adds the panel into the `key`
/// batch, and authors collision from [`solid_slabs`] — the solid rectangles
/// left after the openings are cut, never derived from the (possibly warped,
/// possibly bevelled) visual mesh. Returns `spec.openings` back to the
/// caller, exactly as the source returns its `openings` array so callers can
/// hang props (AC units, laundry, awnings) off the same list.
pub fn facade_wall<'a>(asm: &mut Assembler, pm: &Mat4, spec: FacadeSpec<'a>) -> &'a [WallHole] {
    let mut g = wall_panel(
        spec.w,
        spec.h,
        spec.t,
        spec.openings,
        WallPanelOpts { bevel: spec.bevel, top: spec.top, curve_segments: 7 },
        spec.rng,
    );

    if spec.warp > 0.0 {
        for i in 0..g.vert_count() {
            let nz = g.normal[i * 3 + 2];
            if nz.abs() < 0.5 {
                continue;
            }
            let x = g.pos[i * 3];
            let y = g.pos[i * 3 + 1];
            let d = (fbm3(f64::from(x) * 0.5 + 3.7, f64::from(y) * 0.42 + 1.3, 0.5, 2) - 0.5) * f64::from(spec.warp) * 2.0;
            g.pos[i * 3 + 2] += d as f32;
        }
        g.compute_vertex_normals();
    }

    let opts = spec.paint.map(|paint| AccumAddOpts { masks: None, paint: Some(paint) });
    asm.add_once(spec.key, &g, Some(pm), opts);

    let surface = asm.surface_of(spec.key);
    for s in solid_slabs(spec.w, spec.h, spec.openings) {
        asm.slab_box(surface, pm, s.x, s.y, s.w, s.h, spec.t);
    }

    spec.openings
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Mat4;

    #[test]
    fn facade_wall_with_no_openings_produces_one_full_slab_of_collision() {
        let mut asm = Assembler::new(Rng::new(1));
        let openings: [WallHole; 0] = [];
        facade_wall(&mut asm, &Mat4::IDENTITY, FacadeSpec {
            w: 4.0,
            h: 3.0,
            t: 0.3,
            key: "plaster_cream",
            openings: &openings,
            rng: None,
            bevel: 0.022,
            top: WallTop::Flat { jag: 0.0 },
            warp: 0.0,
            paint: None,
        });
        let result = asm.finalize();
        assert_eq!(result.statics.len(), 1);
        assert_eq!(result.collision.len(), 1);
        assert!((result.stats.collide_tris as f32 - 12.0).abs() < 1e-6);
    }

    #[test]
    fn facade_wall_returns_the_same_openings_slice_back() {
        let mut asm = Assembler::new(Rng::new(1));
        let hole = WallHole { x: 0.0, y: 1.5, w: 0.6, h: 0.8, arch: 0.0, ragged: 0.0 };
        let openings = [hole];
        let returned = facade_wall(&mut asm, &Mat4::IDENTITY, FacadeSpec {
            w: 4.0,
            h: 3.0,
            t: 0.3,
            key: "plaster_cream",
            openings: &openings,
            rng: None,
            bevel: 0.022,
            top: WallTop::Flat { jag: 0.0 },
            warp: 0.018,
            paint: None,
        });
        assert_eq!(returned, &openings);
    }

    #[test]
    fn facade_wall_a_hole_leaves_a_real_gap_in_collision() {
        let mut asm = Assembler::new(Rng::new(1));
        let hole = WallHole { x: 0.0, y: 1.5, w: 1.0, h: 1.0, arch: 0.0, ragged: 0.0 };
        let openings = [hole];
        facade_wall(&mut asm, &Mat4::IDENTITY, FacadeSpec {
            w: 4.0,
            h: 3.0,
            t: 0.3,
            key: "plaster_cream",
            openings: &openings,
            rng: None,
            bevel: 0.0,
            top: WallTop::Flat { jag: 0.0 },
            warp: 0.0,
            paint: None,
        });
        let result = asm.finalize();
        // 4 collision slabs (left, right, above, below the hole) x 12 tris/box.
        assert_eq!(result.collision[0].geo.tri_count(), 4 * 12);
    }
}
