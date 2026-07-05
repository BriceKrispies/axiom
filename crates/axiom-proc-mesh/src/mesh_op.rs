//! The mesh operator codes, as an authoring-friendly enum.

/// The eleven mesh operators. The discriminant **is** the operator code stored in
/// a recipe node and indexes the dispatch table, so this order is the dispatch
/// order and must not be reshuffled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MeshOp {
    /// Axis-aligned box. Params: `[size]`.
    Cube = 0,
    /// Capped cylinder about +Y. Params: `[radius, height, segments]`.
    Cylinder = 1,
    /// Flat XZ plane. Params: `[cols, rows, size]`.
    Grid = 2,
    /// Translate + component-scale. Params: `[tx, ty, tz, sx, sy, sz]`.
    Transform = 3,
    /// Parallel-shell thicken along +Y. Params: `[distance]`.
    Extrude = 4,
    /// Pull vertices toward the centroid. Params: `[amount]`.
    Bevel = 5,
    /// Rotate about Z by `angle × x`. Params: `[angle]`.
    Bend = 6,
    /// Push vertices along their normals by noise. Params: `[amount]`.
    Displace = 7,
    /// Planar XZ UV projection. Params: `[scale]`.
    UVProject = 8,
    /// Re-wrap as a validated triangle list. No params.
    Triangulate = 9,
    /// UV sphere about the origin. Params: `[radius, rings, segments]`.
    Sphere = 10,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_their_dispatch_indices() {
        assert_eq!(MeshOp::Cube as u16, 0);
        assert_eq!(MeshOp::Triangulate as u16, 9);
        assert_eq!(MeshOp::Sphere as u16, 10);
    }
}
