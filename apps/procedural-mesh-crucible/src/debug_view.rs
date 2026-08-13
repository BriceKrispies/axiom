//! The app-side debug views, and the honest limits of what this renderer can
//! show.
//!
//! **Every debug idea in this file is expressed with the shipped mesh and
//! material vocabulary.** Nothing here reaches into a backend, and nothing here
//! asks `axiom-mesh` or `axiom-mesh-ops` to grow a "debug" concept — a geometry
//! library that knows what a debug view is has stopped being a geometry library.
//!
//! Three views are available, selected by the page's `?view=` query:
//!
//! - `shaded` (default) — the scene's authored colours.
//! - `flat` / `smooth` — every object's normals regenerated with
//!   `generate_flat_normals` / `generate_normals`. This is a real comparison:
//!   the flat pass unwelds to three vertices per triangle and the faceting is
//!   immediately visible on the swept and lathed surfaces.
//! - `normals` — a **normal chart**. Each vertex's UV is replaced by its own
//!   normal's `(x, y)` mapped into `0..1`, and every object is given one shared
//!   app-authored chart texture, so the sampled colour is a direct picture of
//!   the surface normal.
//!
//! The chart is the honest way to visualise normals here: the renderer has no
//! per-vertex colour channel and no shader hook, but it does have per-vertex UVs
//! and app-supplied RGBA textures, and a texture lookup indexed by the normal is
//! exactly a colour-by-normal. Its one limitation is that the two hemispheres of
//! `z` share a chart cell, which the page legend says out loud. What is *not*
//! expressible at all — wireframe, and a per-object UV checker that would need a
//! second UV set — is recorded in `NOTES.md` rather than hacked in.

use axiom_math::Vec2;
use axiom_mesh::{generate_flat_normals, generate_normals, Mesh, MeshResult};

/// Which debug view the page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugView {
    /// The authored materials, with each operator's own normals.
    Shaded,
    /// Every mesh re-normalled flat (unwelded, one normal per triangle).
    Flat,
    /// Every mesh re-normalled smooth (area-weighted per vertex).
    Smooth,
    /// Colour by normal, through a UV chart.
    Normals,
}

impl DebugView {
    /// Every view, in a fixed order.
    pub const ALL: [DebugView; 4] = [
        DebugView::Shaded,
        DebugView::Flat,
        DebugView::Smooth,
        DebugView::Normals,
    ];

    /// The lowercase name the `?view=` query accepts and the legend prints.
    pub fn label(self) -> &'static str {
        ["shaded", "flat", "smooth", "normals"][self as usize]
    }

    /// Parse a `?view=` value; anything unrecognised is [`DebugView::Shaded`].
    pub fn from_label(label: &str) -> DebugView {
        DebugView::ALL
            .into_iter()
            .find(|view| view.label() == label)
            .unwrap_or(DebugView::Shaded)
    }

    /// Whether this view replaces the object's own colour with the chart.
    pub fn uses_chart(self) -> bool {
        self == DebugView::Normals
    }

    /// The mesh as this view wants it drawn.
    pub fn apply(self, mesh: &Mesh) -> MeshResult<Mesh> {
        match self {
            DebugView::Shaded => Ok(mesh.clone()),
            DebugView::Flat => generate_flat_normals(mesh),
            DebugView::Smooth => generate_normals(mesh),
            DebugView::Normals => generate_normals(mesh).and_then(|smooth| chart_uvs(&smooth)),
        }
    }
}

/// How far into the chart the extreme normals are pulled, away from its edge.
///
/// A straight `0.5 + 0.5 * n` maps an axis-aligned normal onto **exactly** `0.0`
/// or `1.0`, which is the texture's repeat seam: a flat `+Y` face samples `v =
/// 1.0`, wraps, and comes back striped in the colour from `v = 0`. Every ground
/// quad, disk, grid and box lid in the scene is exactly that normal, so the
/// artifact was on most of the frame. Landing the extremes at `0.02 .. 0.98`
/// keeps the whole chart addressable and never touches the seam.
const CHART_INSET: f32 = 0.48;

/// Replace every UV with the vertex normal's `(x, y)` mapped into the chart, so
/// a texture lookup paints the surface by its normal.
fn chart_uvs(mesh: &Mesh) -> MeshResult<Mesh> {
    let mut streams = mesh.clone().into_streams();
    streams.uvs = streams
        .normals
        .iter()
        .map(|n| Vec2::new(0.5 + CHART_INSET * n.x, 0.5 + CHART_INSET * n.y))
        .collect();
    Mesh::from_streams(streams)
}

/// The chart texture's edge length in texels.
pub const CHART_SIZE: u32 = 64;

/// The normal chart: an RGBA8 image whose texel at `(u, v)` is the colour a
/// normal pointing that way is painted. Red rises with `+X`, green with `+Y`,
/// and blue falls off toward the rim so a normal facing the camera reads
/// distinctly from one facing away sideways.
pub fn chart_rgba() -> Vec<u8> {
    (0..CHART_SIZE * CHART_SIZE)
        .flat_map(|k| {
            let u = (k % CHART_SIZE) as f32 / (CHART_SIZE - 1) as f32;
            let v = (k / CHART_SIZE) as f32 / (CHART_SIZE - 1) as f32;
            let (nx, ny) = (2.0 * u - 1.0, 2.0 * v - 1.0);
            let rim = (1.0 - (nx * nx + ny * ny)).max(0.0).sqrt();
            [
                channel(0.15 + 0.85 * u),
                channel(0.15 + 0.85 * v),
                channel(0.20 + 0.75 * rim),
                255,
            ]
        })
        .collect()
}

/// A linear `0..1` intensity as an 8-bit texel channel.
fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
