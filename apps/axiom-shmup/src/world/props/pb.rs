//! Ported from Claude-of-Duty `src/world/props.js:59-114` — the `PB` "part
//! accumulator" every prototype builder in this module uses to assemble a
//! handful of chamfered boxes/cylinders/hand-built shapes, each already
//! placed in local space, into one merged geometry.
//!
//! `PB` is deliberately **not** [`crate::world::accum::Accum`]: `Accum` (the
//! Assembler's per-palette-key static batch and per-prototype instance
//! store) transforms-and-appends by matrix at *placement* time; `PB` merges
//! parts that are already baked into their own local space at *authoring*
//! time, with no matrix stored anywhere — exactly `mergeSimple`'s contract.
//! The source keeps these as two different files (`builder.js`'s `Accum`
//! class vs. `props.js`'s local `PB` class) for the same reason.
//!
//! Every `PB::box`/`cyl`/`geo` call in the source returns the pushed
//! geometry (`_push` ends `return g;`), but no call site anywhere in
//! `props.js` ever reads that return value — every call is a bare statement.
//! These methods return `()` rather than plumb an unused value through ~150
//! call sites; see `docs/work-manifests/shmup-port/notes/props.md`.
//!
//! **Every dimension/position/rotation argument here is `f64`, narrowed to
//! `f32` only at the actual `chamfer_box`/`cylinder_geometry`/`trs` call** —
//! the port recipe's "compute in f64, store f32" rule, applied because a
//! violation of it is not theoretical: `chamfer_box`'s UV-axis pick
//! (`kit.rs`'s `add_chamfer_poly`, `ax = if n[0].abs() > n[1].abs() {…}`) is
//! a **discrete** choice that can sit on a knife-edge tie for a thin box —
//! `crate_a`'s slat boxes (`0.016 x s*0.14 x s*0.94`) hit exactly this: with
//! `s: f32` at the call site, `s * 0.14` round-trips through a second,
//! avoidable f32 rounding *before* `chamfer_box` ever sees it, which was
//! measured to flip that tie and pick the wrong UV axis outright (not a
//! small numeric drift — a completely different `uv` pair). Every builder
//! function upstream of [`PB`] (`containers`, `cover`, … ) takes its own
//! size/position parameters as `f64` for the same reason; only `wear`/
//! `grime`/`ao` (plain mask literals, never involved in a geometric
//! decision) stay `f32`, matching [`crate::world::geo::WorldGeo::color`]'s
//! own storage type.

use axiom_math::Mat4;

use crate::world::geo::WorldGeo;
use crate::world::kit::{chamfer_box, cylinder_geometry, merge_simple, trs};

use super::mesh::auto_edge_wear;

/// `PB.box(sx, sy, sz, x=0, y=0, z=0, o={})`'s `o` (`props.js:82-86`).
/// Defaults match the source: `bevel=0.008`, `ry=rx=rz=0`, `wear=1`,
/// `grime=0`, `ao=0`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BoxOpts {
    pub bevel: f64,
    pub ry: f64,
    pub rx: f64,
    pub rz: f64,
    pub wear: f32,
    pub grime: f32,
    pub ao: f32,
}

impl Default for BoxOpts {
    fn default() -> Self {
        BoxOpts { bevel: 0.008, ry: 0.0, rx: 0.0, rz: 0.0, wear: 1.0, grime: 0.0, ao: 0.0 }
    }
}

/// `PB.cyl(r, h, x=0, y=0, z=0, o={})`'s `o` (`props.js:88-100`). Defaults
/// match the source: `taper=1`, `radial=12`, `seg=1`, `open=false`,
/// `margin=min(r,h)*0.12` (modelled as [`None`], resolved at the call site —
/// Rust has no way to default one field off two *other* parameters),
/// `ry=rx=rz=0`, `wear=1`, `grime=0`, `ao=0`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CylOpts {
    pub taper: f64,
    pub radial: u32,
    pub seg: u32,
    pub open: bool,
    pub margin: Option<f64>,
    pub ry: f64,
    pub rx: f64,
    pub rz: f64,
    pub wear: f32,
    pub grime: f32,
    pub ao: f32,
}

impl Default for CylOpts {
    fn default() -> Self {
        CylOpts { taper: 1.0, radial: 12, seg: 1, open: false, margin: None, ry: 0.0, rx: 0.0, rz: 0.0, wear: 1.0, grime: 0.0, ao: 0.0 }
    }
}

/// `PB.geo(g, x=0, y=0, z=0, o={})`'s `o` (`props.js:102-106`). Defaults
/// match the source: `autoWear=true` (only `false` ever suppresses it),
/// `margin=0.02`, `ry=rx=rz=0`, `sx=sy=sz=1`, `wear=1`, `grime=0`, `ao=0`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GeoOpts {
    pub auto_wear: bool,
    pub margin: f64,
    pub ry: f64,
    pub rx: f64,
    pub rz: f64,
    pub sx: f64,
    pub sy: f64,
    pub sz: f64,
    pub wear: f32,
    pub grime: f32,
    pub ao: f32,
}

impl Default for GeoOpts {
    fn default() -> Self {
        GeoOpts { auto_wear: true, margin: 0.02, ry: 0.0, rx: 0.0, rz: 0.0, sx: 1.0, sy: 1.0, sz: 1.0, wear: 1.0, grime: 0.0, ao: 0.0 }
    }
}

/// `mat(x, y, z, ry=0, rx=0, rz=0, sx=1, sy=1, sz=1)` (`props.js:35-41`):
/// translate * rotate(YXZ) * scale, i.e. exactly [`crate::world::kit::trs`]
/// with its arguments reordered to this file's `(ry, rx, rz)` / `(sx, sy,
/// sz)` grouping, narrowing every argument to `f32` right at the call
/// (`trs` — and the `axiom_math::Mat4`/`Vec3` it builds — is `f32`-native;
/// this is the one unavoidable narrowing point for a transform, matching
/// this module's doc). `pub(crate)` (rather than private) because
/// `props::vegetation`'s `shrub`/`weed_tuft`/`palm_frond` call this same
/// helper directly, without going through [`PB`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn mat(x: f64, y: f64, z: f64, ry: f64, rx: f64, rz: f64, sx: f64, sy: f64, sz: f64) -> Mat4 {
    trs(x as f32, y as f32, z as f32, ry as f32, sx as f32, sy as f32, sz as f32, rx as f32, rz as f32)
}

/// `class PB` (`props.js:59-114`): accumulate already-placed parts, then
/// merge them into one geometry for a prototype.
pub(crate) struct PB {
    list: Vec<WorldGeo>,
}

impl PB {
    pub(crate) fn new() -> Self {
        PB { list: Vec::new() }
    }

    /// `_push(g, wear, grime, ao)` (`props.js:65-80`).
    fn push(&mut self, mut g: WorldGeo, wear: f32, grime: f32, ao: f32) {
        if g.color.is_empty() {
            g.fill_masks(0.2, 0.0, 0.0);
        }
        if wear != 1.0 || grime > 0.0 || ao > 0.0 {
            for c in g.color.chunks_exact_mut(3) {
                c[0] = (c[0] * wear).min(1.0);
                c[1] = c[1].max(grime).min(1.0);
                c[2] = c[2].max(ao).min(1.0);
            }
        }
        self.list.push(g);
    }

    /// `box(sx, sy, sz, x=0, y=0, z=0, o={})` (`props.js:82-86`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn box_(&mut self, sx: f64, sy: f64, sz: f64, x: f64, y: f64, z: f64, o: BoxOpts) {
        let mut g = chamfer_box(sx as f32, sy as f32, sz as f32, o.bevel as f32);
        g.apply(&mat(x, y, z, o.ry, o.rx, o.rz, 1.0, 1.0, 1.0));
        self.push(g, o.wear, o.grime, o.ao);
    }

    /// `cyl(r, h, x=0, y=0, z=0, o={})` (`props.js:88-100`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cyl(&mut self, r: f64, h: f64, x: f64, y: f64, z: f64, o: CylOpts) {
        let mut g = cylinder_geometry(o.taper * r, r, h, o.radial, o.seg, o.open);
        let margin = o.margin.unwrap_or_else(|| r.min(h) * 0.12);
        auto_edge_wear(&mut g, margin as f32, 0.9);
        g.apply(&mat(x, y, z, o.ry, o.rx, o.rz, 1.0, 1.0, 1.0));
        self.push(g, o.wear, o.grime, o.ao);
    }

    /// `geo(g, x=0, y=0, z=0, o={})` (`props.js:102-106`).
    pub(crate) fn geo(&mut self, mut g: WorldGeo, x: f64, y: f64, z: f64, o: GeoOpts) {
        if o.auto_wear && g.color.is_empty() {
            auto_edge_wear(&mut g, o.margin as f32, 1.0);
        }
        g.apply(&mat(x, y, z, o.ry, o.rx, o.rz, o.sx, o.sy, o.sz));
        self.push(g, o.wear, o.grime, o.ao);
    }

    /// `build()` (`props.js:108-113`): merge every accumulated part and
    /// clear the list (the source's `for (const p of this.list) p.dispose();
    /// this.list.length = 0;` — ownership drops the parts here instead).
    pub(crate) fn build(&mut self) -> WorldGeo {
        let g = merge_simple(&self.list);
        self.list.clear();
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_and_build_produce_a_chamfer_boxs_worth_of_geometry() {
        let mut p = PB::new();
        p.box_(1.0, 1.0, 1.0, 0.0, 0.0, 0.0, BoxOpts::default());
        let g = p.build();
        assert_eq!(g.tri_count(), 44); // chamfer_box's own fixed 44 triangles.
        assert!(p.build().vert_count() == 0, "build() clears the list");
    }

    #[test]
    fn cyl_applies_auto_edge_wear_when_no_color_present() {
        let mut p = PB::new();
        p.cyl(0.1, 0.5, 0.0, 0.0, 0.0, CylOpts { radial: 8, ..CylOpts::default() });
        let g = p.build();
        // auto_edge_wear always allocates a color column via paint_masks.
        assert_eq!(g.color.len(), g.pos.len());
    }

    #[test]
    fn push_multiplies_wear_and_maxes_grime_and_ao() {
        let mut p = PB::new();
        let mut g = WorldGeo {
            pos: vec![0.0; 3],
            normal: vec![0.0; 3],
            uv: vec![0.0; 2],
            color: vec![0.8, 0.1, 0.05],
            index: Vec::new(),
        };
        g.apply(&Mat4::IDENTITY);
        p.push(g, 0.5, 0.3, 0.2);
        assert_eq!(&p.list[0].color, &[0.4, 0.3, 0.2]);
    }

    #[test]
    fn geo_respects_auto_wear_false() {
        let mut p = PB::new();
        let g = WorldGeo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            uv: Vec::new(),
            color: Vec::new(),
            index: Vec::new(),
        };
        p.geo(g, 0.0, 0.0, 0.0, GeoOpts { auto_wear: false, ..GeoOpts::default() });
        // fill_masks(0.2,0,0) still runs in `_push` since color was empty.
        assert_eq!(p.list[0].color, vec![0.2, 0.0, 0.0, 0.2, 0.0, 0.0, 0.2, 0.0, 0.0]);
    }
}
