//! A mesh description an app adds to an `Assets<Mesh>` collection.

/// A mesh an app registers with the engine.
///
/// The engine provides built-in primitives — the unit cube, a unit plane (a quad,
/// for ground), and a unit UV sphere. A `Mesh` value is a *description*; the
/// engine resolves it into real mesh data (via `axiom-resources`) when the app
/// runs. Scaling is the node `Transform`'s job, so primitives need no size param.
///
/// This is a fieldless enum whose discriminant order (Cube=0, Plane=1, Sphere=2,
/// Cylinder=3) is the index into the resolver's generator table — adding a
/// variant means adding its generator at the matching index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mesh {
    /// The engine's built-in unit cube (±0.5 on each axis).
    Cube,
    /// The engine's built-in unit plane: a 1x1 quad in the XZ plane facing +Y.
    Plane,
    /// The engine's built-in unit UV sphere (radius 0.5).
    Sphere,
    /// The engine's built-in unit cylinder (radius 0.5, height 1).
    Cylinder,
}

impl Mesh {
    /// The built-in unit cube.
    pub const fn cube() -> Self {
        Mesh::Cube
    }

    /// The built-in unit plane (a ground quad).
    pub const fn plane() -> Self {
        Mesh::Plane
    }

    /// The built-in unit UV sphere.
    pub const fn sphere() -> Self {
        Mesh::Sphere
    }

    /// The built-in unit cylinder.
    pub const fn cylinder() -> Self {
        Mesh::Cylinder
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_match_their_primitives() {
        assert_eq!(Mesh::cube(), Mesh::Cube);
        assert_eq!(Mesh::plane(), Mesh::Plane);
        assert_eq!(Mesh::sphere(), Mesh::Sphere);
        assert_eq!(Mesh::cylinder(), Mesh::Cylinder);
    }

    #[test]
    fn discriminant_order_indexes_the_generator_table() {
        // The resolver indexes a 4-entry generator table by `mesh as usize`.
        assert_eq!(Mesh::Cube as usize, 0);
        assert_eq!(Mesh::Plane as usize, 1);
        assert_eq!(Mesh::Sphere as usize, 2);
        assert_eq!(Mesh::Cylinder as usize, 3);
    }
}
