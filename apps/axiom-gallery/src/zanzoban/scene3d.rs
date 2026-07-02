//! Turning a [`RenderModel`] into engine render instances — the 3D view.
//!
//! Instead of hand-drawing the board on a 2D canvas, Zanzoban now renders through
//! the Axiom engine's instanced-cube renderer (the same GPU/Canvas2D backends
//! retro FPS uses). Each cell, actor, and crate becomes one cube instance: a
//! model-view-projection matrix plus a colour. The instance layout matches the
//! engine's lit-mesh batch format (`axiom-windowing`'s `run_web_multi`, the same
//! retro FPS feeds): `[mvp(16), world(16), colour(4)]` per instance, column-major,
//! exactly how the engine composes it (`view_projection.multiply(world)` for the
//! mvp, the model matrix for `world` — used by the lighting/shadow pass).
//!
//! The live browser path sources its camera from a real engine `App`
//! (`camera_view_proj`, in `web.rs`) — a steep near-top-down view for the editor
//! and an angled perspective diorama for playtest. The `view_projection` here is
//! a hand-rolled fallback used only by the native tests.

use axiom_math::{Mat4, Quat, Transform, Vec3};

use crate::zanzoban::actor_state::ActorKind;
use crate::zanzoban::render_model::{RenderActor, RenderModel, RenderTile};

/// The background clear colour (linear RGBA).
pub const CLEAR_COLOR: [f32; 4] = [0.055, 0.062, 0.078, 1.0];

/// Floats per instance: MVP (4×4) + world (4×4) + an RGBA colour — the engine's
/// lit-mesh batch layout (`run_web_multi`).
const FLOATS_PER_INSTANCE: usize = 36;

/// A cube's colour and vertical extent for a tile, as `(height, [r,g,b,a])`.
/// Heights read the board as a shallow diorama: floor is a thin slab, walls and
/// closed doors are tall blocks, buttons/switches sit low, open doors/wells sink.
fn tile_box(tile: RenderTile) -> (f32, [f32; 4]) {
    match tile {
        RenderTile::Floor => (0.14, [0.11, 0.12, 0.15, 1.0]),
        RenderTile::Wall => (1.0, [0.36, 0.39, 0.45, 1.0]),
        RenderTile::Entrance => (0.20, [0.18, 0.42, 0.25, 1.0]),
        RenderTile::Exit => (0.20, [0.72, 0.56, 0.18, 1.0]),
        RenderTile::Button { pressed: true } => (0.22, [0.64, 0.34, 0.34, 1.0]),
        RenderTile::Button { pressed: false } => (0.40, [0.64, 0.23, 0.23, 1.0]),
        RenderTile::Door { open: true } => (0.06, [0.10, 0.11, 0.14, 1.0]),
        RenderTile::Door { open: false } => (1.0, [0.48, 0.35, 0.21, 1.0]),
        RenderTile::Well => (0.08, [0.12, 0.31, 0.34, 1.0]),
        RenderTile::Switch { latched: true } => (0.22, [0.48, 0.34, 0.66, 1.0]),
        RenderTile::Switch { latched: false } => (0.40, [0.32, 0.24, 0.52, 1.0]),
        RenderTile::Hazard => (0.14, [0.54, 0.18, 0.18, 1.0]),
    }
}

/// A crate's box: a chunky woody block.
fn crate_box() -> (f32, [f32; 4]) {
    (0.7, [0.54, 0.42, 0.25, 1.0])
}

/// An actor's box: a solid player block or a translucent ghost.
fn actor_box(actor: &RenderActor) -> (f32, [f32; 4]) {
    match actor.kind {
        ActorKind::Player => (0.72, [0.25, 0.50, 0.88, 1.0]),
        ActorKind::Ghost => (0.66, [0.33, 0.78, 0.84, actor.alpha]),
    }
}

/// The tile side of a cube instance (a small inset so cells read as separated).
const TILE_SIZE: f32 = 0.92;
/// The actor/crate side (smaller, so they sit within a cell).
const ACTOR_SIZE: f32 = 0.62;

/// A hand-rolled camera view-projection for the board — the native-test fallback.
/// The live browser path instead uses the engine's own `camera_view_proj` (see
/// `web.rs`), whose clip-space convention the three backends expect.
pub fn view_projection(width: u32, height: u32, aspect: f32, perspective: bool) -> Mat4 {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    let center = Vec3::new(w * 0.5, 0.0, h * 0.5);
    let up = Vec3::new(0.0, 1.0, 0.0);
    if perspective {
        // Angled diorama: above and to the south, looking at the board centre.
        let span = w.max(h);
        let eye = Vec3::new(w * 0.5, span * 0.95, h * 0.5 + span * 0.85);
        let view = Mat4::look_at(eye, center, up).unwrap_or(Mat4::IDENTITY);
        let proj = Mat4::perspective(0.85, aspect, 0.1, span * 6.0 + 50.0)
            .unwrap_or(Mat4::IDENTITY);
        proj.multiply(view)
    } else {
        // Straight-down orthographic: cells map linearly to the screen. The board
        // is framed with a small margin, keeping cells square for the aspect.
        let margin = 0.6;
        let half_x = w * 0.5 + margin;
        let half_z = h * 0.5 + margin;
        // Fit both extents while holding square cells for the surface aspect.
        let oy = half_z.max(half_x / aspect);
        let ox = oy * aspect;
        let eye = Vec3::new(w * 0.5, 20.0, h * 0.5);
        // Straight down: use -Z as "up" so row 0 renders at the top.
        let view = Mat4::look_at(eye, center, Vec3::new(0.0, 0.0, -1.0)).unwrap_or(Mat4::IDENTITY);
        let proj = Mat4::orthographic(-ox, ox, -oy, oy, 0.1, 100.0).unwrap_or(Mat4::IDENTITY);
        proj.multiply(view)
    }
}

/// One cube instance's 36 floats: `mvp(16)`, `world(16)` (both column-major),
/// then `colour(4)`.
fn push_instance(out: &mut Vec<f32>, view_proj: Mat4, cx: f32, cz: f32, size: f32, height: f32, color: [f32; 4]) {
    // Model: a cube of `size × height × size`, its base on the floor (y = 0).
    let model = Transform::new(
        Vec3::new(cx, height * 0.5, cz),
        Quat::IDENTITY,
        Vec3::new(size, height, size),
    )
    .to_matrix();
    out.extend_from_slice(&view_proj.multiply(model).as_cols_array());
    out.extend_from_slice(&model.as_cols_array());
    out.extend_from_slice(&color);
}

/// Build the per-frame instance buffer for `model` using an explicit camera
/// view-projection (the engine's own `camera_view_proj`, so the MVP convention
/// matches all three backends). One cube per cell, crate, and actor. Returns
/// `(clear_color, instances, count)` for `WindowingApi::run_web`.
pub fn build_instances(model: &RenderModel, vp: Mat4) -> ([f32; 4], Vec<f32>, u32) {
    let cell_count = model.cells.len();
    let mut instances =
        Vec::with_capacity((cell_count + model.crates.len() + model.actors.len()) * FLOATS_PER_INSTANCE);

    // Cells (row-major), then crates, then actors (ghosts under the player).
    model.cells.iter().for_each(|cell| {
        let (height, color) = tile_box(cell.tile);
        let cx = cell.coord.x as f32 + 0.5;
        let cz = cell.coord.y as f32 + 0.5;
        push_instance(&mut instances, vp, cx, cz, TILE_SIZE, height, color);
    });
    model.crates.iter().for_each(|c| {
        let (height, color) = crate_box();
        push_instance(&mut instances, vp, c.x as f32 + 0.5, c.y as f32 + 0.5, ACTOR_SIZE, height, color);
    });
    model.actors.iter().for_each(|actor| {
        let (height, color) = actor_box(actor);
        let cx = actor.coord.x as f32 + 0.5;
        let cz = actor.coord.y as f32 + 0.5;
        push_instance(&mut instances, vp, cx, cz, ACTOR_SIZE, height, color);
    });

    let count = (instances.len() / FLOATS_PER_INSTANCE) as u32;
    (CLEAR_COLOR, instances, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zanzoban::level_codec;
    use crate::zanzoban::playtest_model::PlaytestSession;
    use crate::zanzoban::LEVEL_001_TOML;

    #[test]
    fn emits_one_instance_per_cell_plus_actors() {
        let level = level_codec::from_toml(LEVEL_001_TOML).expect("parses");
        let session = PlaytestSession::new(level);
        let model = session.render_model();
        let vp = view_projection(model.width, model.height, 960.0 / 600.0, true);
        let (_clear, instances, count) = build_instances(&model, vp);
        // One cube per cell + the live player (one actor, no ghosts yet).
        let expected = model.cells.len() as u32 + model.actors.len() as u32;
        assert_eq!(count, expected);
        assert_eq!(instances.len(), count as usize * FLOATS_PER_INSTANCE);
        // Every MVP float is finite (the camera + model composed cleanly).
        assert!(instances.iter().all(|f| f.is_finite()));
    }

    #[test]
    fn both_cameras_produce_finite_matrices() {
        let level = level_codec::from_toml(LEVEL_001_TOML).expect("parses");
        let model = PlaytestSession::new(level).render_model();
        for perspective in [true, false] {
            let vp = view_projection(model.width, model.height, 800.0 / 600.0, perspective);
            let (_c, inst, n) = build_instances(&model, vp);
            assert!(n > 0);
            assert!(inst.iter().all(|f| f.is_finite()));
        }
    }
}
