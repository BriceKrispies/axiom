//! The level *descriptor* — the neutral data contract produced by procgen and
//! consumed by the game core to build physics bodies and render instances. It is
//! a plain value type: a set of oriented platform boxes, a start and end zone, a
//! coin list, a spawn point, and a kill plane. The generator (`procgen`) fills
//! it; the game core (`mod`) turns it into `axiom-physics` bodies and renderable
//! transforms.

use axiom::prelude::Vec3;
use axiom_math::Quat;

/// The visual/material role of a platform box — drives colour and, for a lattice,
/// whether it is solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    /// The wide spawn/finish pad.
    Plaza,
    /// A standard path tile.
    Path,
    /// A widened path tile.
    PathWide,
    /// A tilted ramp segment (an oriented box — needs the engine's OBB contacts).
    Ramp,
    /// A decorative, non-colliding wireframe block.
    Lattice,
}

impl SurfaceKind {
    /// Whether a tile of this kind participates in collision. Only the lattice is
    /// a pass-through decoration.
    pub fn solid(self) -> bool {
        !matches!(self, SurfaceKind::Lattice)
    }
}

/// A single platform: an oriented box (`rotation` is identity for flat tiles, a
/// pitch for ramps).
#[derive(Clone, Copy, Debug)]
pub struct Platform {
    pub position: Vec3,
    pub half_extents: Vec3,
    pub rotation: Quat,
    pub kind: SurfaceKind,
}

/// A flat circular trigger zone on the course (start pad / finish pad).
#[derive(Clone, Copy, Debug)]
pub struct Zone {
    pub position: Vec3,
    pub radius: f32,
}

/// A collectible coin, hovering at a world position.
#[derive(Clone, Copy, Debug)]
pub struct Coin {
    pub position: Vec3,
}

/// The full authored level: everything the game core needs to instantiate a
/// playable course.
#[derive(Clone, Debug)]
pub struct LevelDescriptor {
    /// Where the marble spawns (its centre).
    pub spawn: Vec3,
    /// The oriented platform boxes making up the course.
    pub platforms: Vec<Platform>,
    /// The start pad: the marble must touch this before the finish counts.
    pub start_zone: Zone,
    /// The finish pad: touching it (after the start) completes the level.
    pub end_zone: Zone,
    /// The coins to collect.
    pub coins: Vec<Coin>,
    /// A fall below this Y is a death.
    pub kill_plane_y: f32,
}
