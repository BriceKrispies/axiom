//! A mesh description an app adds to an `Assets<Mesh>` collection.

/// A mesh an app registers with the engine.
///
/// Today the engine provides one built-in primitive — the unit cube. A `Mesh`
/// value is a *description*; the engine resolves it into real mesh data (via
/// `axiom-resources`) when the app runs. Scaling is the node `Transform`'s job,
/// so the cube needs no size parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mesh {
    /// The engine's built-in unit cube (±0.5 on each axis).
    Cube,
}

impl Mesh {
    /// The built-in unit cube.
    pub const fn cube() -> Self {
        Mesh::Cube
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_is_the_cube_primitive() {
        assert_eq!(Mesh::cube(), Mesh::Cube);
    }
}
