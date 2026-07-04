//! The demo room's recipes — the *how-to-make*, a few hundred bytes that expand
//! into every texture and mesh in the scene. Authored here in code; in a full
//! pipeline these bytes would be emitted by the Workspace editor and shipped
//! alone.

use axiom_proc_mesh::MeshOp;
use axiom_proc_texture::TextureOp;
use axiom_recipe::{Color, Param, RecipeGraph, RecipeId, Scalar};

/// A scalar parameter word.
fn s(v: f32) -> Param {
    Param::scalar(Scalar::new(v))
}

/// An integer parameter word.
fn i(v: u32) -> Param {
    Param::int(v)
}

/// A packed-RGBA color parameter word.
fn col(r: u8, g: u8, b: u8) -> Param {
    Param::color(Color::rgba(r, g, b, 0xFF))
}

/// The full set of recipes the demo room expands from. Each is an independent,
/// self-contained graph; together they are the only bytes a player needs.
#[derive(Debug, Clone)]
pub struct DemoRecipes {
    /// A staggered brick wall texture (bricks softened by a light blur).
    pub brick_texture: RecipeGraph,
    /// A mottled stone floor texture (value noise remapped to floor tones).
    pub floor_texture: RecipeGraph,
    /// A wood-grain crate texture (a warm vertical gradient).
    pub crate_texture: RecipeGraph,
    /// The floor: a subdivided ground plane.
    pub floor_mesh: RecipeGraph,
    /// The back wall: a unit box, scaled thin when placed.
    pub wall_mesh: RecipeGraph,
    /// The crate: a beveled, UV-projected box.
    pub crate_mesh: RecipeGraph,
}

impl DemoRecipes {
    /// Author every demo recipe.
    pub fn build() -> Self {
        Self {
            brick_texture: brick_texture(),
            floor_texture: floor_texture(),
            crate_texture: crate_texture(),
            floor_mesh: floor_mesh(),
            wall_mesh: wall_mesh(),
            crate_mesh: crate_mesh(),
        }
    }

    /// All recipes, in a stable order (for byte accounting / round-tripping).
    pub fn all(&self) -> [&RecipeGraph; 6] {
        [
            &self.brick_texture,
            &self.floor_texture,
            &self.crate_texture,
            &self.floor_mesh,
            &self.wall_mesh,
            &self.crate_mesh,
        ]
    }

    /// The total serialized size of every recipe — the bytes that ship.
    pub fn total_bytes(&self) -> usize {
        self.all().iter().map(|r| r.serialize().len()).sum()
    }

    /// Round-trip every recipe through its serialized bytes, reconstructing the
    /// set purely from bytes. Proves the player depends on nothing but the
    /// recipe data. Panics only if a recipe this module authored is invalid,
    /// which its own tests forbid.
    pub fn round_tripped(&self) -> Self {
        let restored: Vec<RecipeGraph> = self
            .all()
            .into_iter()
            .map(|r| RecipeGraph::deserialize(&r.serialize()).expect("authored recipe round-trips"))
            .collect();
        Self {
            brick_texture: restored[0].clone(),
            floor_texture: restored[1].clone(),
            crate_texture: restored[2].clone(),
            floor_mesh: restored[3].clone(),
            wall_mesh: restored[4].clone(),
            crate_mesh: restored[5].clone(),
        }
    }
}

/// Bricks (128²) softened by a 1px blur.
fn brick_texture() -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
    let b = g.add(
        TextureOp::Bricks as u16,
        vec![i(128), i(128), i(6), i(3), i(4), col(0xB0, 0x50, 0x40), col(0x2E, 0x2E, 0x2E)],
        vec![],
    );
    g.add(TextureOp::Blur as u16, vec![i(1)], vec![b]);
    g
}

/// Value noise (128²) remapped to floor tones.
fn floor_texture() -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(2), 1);
    let n = g.add(
        TextureOp::Noise as u16,
        vec![i(128), i(128), i(8), col(0x00, 0x00, 0x00), col(0xFF, 0xFF, 0xFF)],
        vec![],
    );
    g.add(TextureOp::ColorRamp as u16, vec![col(0x2C, 0x24, 0x1C), col(0x6E, 0x5E, 0x4C)], vec![n]);
    g
}

/// A warm vertical wood gradient (64²).
fn crate_texture() -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(3), 1);
    g.add(
        TextureOp::Gradient as u16,
        vec![i(64), i(64), col(0x8A, 0x5A, 0x2B), col(0x5A, 0x3A, 0x18)],
        vec![],
    );
    g
}

/// An 8×8 ground plane, 10 units across.
fn floor_mesh() -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(10), 1);
    g.add(MeshOp::Grid as u16, vec![i(8), i(8), s(10.0)], vec![]);
    g
}

/// A unit box (scaled into a wall at placement).
fn wall_mesh() -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(11), 1);
    g.add(MeshOp::Cube as u16, vec![s(1.0)], vec![]);
    g
}

/// A beveled, UV-projected, triangulated crate box.
fn crate_mesh() -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(12), 1);
    let c = g.add(MeshOp::Cube as u16, vec![s(1.0)], vec![]);
    let b = g.add(MeshOp::Bevel as u16, vec![s(0.08)], vec![c]);
    let u = g.add(MeshOp::UVProject as u16, vec![s(1.0)], vec![b]);
    g.add(MeshOp::Triangulate as u16, vec![], vec![u]);
    g
}
