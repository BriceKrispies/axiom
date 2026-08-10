//! Render-facing mesh identity: which uploaded mesh a draw refers to, and how
//! many indices it spans.

/// Render-facing mesh: the **identity** of one mesh the render command list
/// refers to by index, plus the index count a draw over it covers.
///
/// ## Why this carries no geometry
///
/// It used to carry the whole CPU-side mesh — positions, normals, uvs and
/// indices — and that was a category error with a large, measured cost. Mesh
/// geometry is **bind-time resident state**, not frame-packet data: it is
/// uploaded to the backend once when the surface binds (`RunningApp::mesh_set`
/// reads it straight from the registry) and never changes thereafter. A frame
/// packet's job is to say *what to draw this frame*, and the answer is a mesh
/// id, not a copy of the mesh. This is the same line `axiom_host::MaterialTexture`
/// already draws for albedo pixels, for the same reason.
///
/// Carrying the geometry meant every registered mesh's four vertex arrays were
/// cloned into the frame packet, and then cloned again out of it into the render
/// input — **twice per frame, for every mesh in the world, drawn or not**. In a
/// 9 km racing course with ~1,000 registered meshes that measured as roughly a
/// third of the entire browser main thread (~24% in `Vec::clone` plus the
/// allocator traffic it generated), to deliver the two scalars below. Nothing
/// downstream ever read a position, a normal or a uv from here; the per-frame
/// draw-ordering path reads exactly [`Self::id`] and [`Self::index_count`].
///
/// The renderer does not know what an `axiom-resources::MeshData` is — the app
/// translates resource data into this neutral shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderMesh {
    /// An opaque ID the producer assigns; round-tripped to commands
    /// for backend identification.
    id: u64,
    /// How many indices the uploaded mesh holds — the extent of a draw over it.
    index_count: u32,
}

impl RenderMesh {
    pub const fn new(id: u64, index_count: u32) -> Self {
        RenderMesh { id, index_count }
    }

    pub const fn id(&self) -> u64 {
        self.id
    }

    /// How many indices a draw over this mesh covers.
    pub const fn index_count(&self) -> u32 {
        self.index_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let m = RenderMesh::new(7, 3);
        assert_eq!(m.id(), 7);
        assert_eq!(m.index_count(), 3);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderMesh::new(1, 0);
        let b = RenderMesh::new(1, 0);
        let c = RenderMesh::new(2, 0);
        // Same id, different extent, is still a different mesh reference: the
        // index count is what a draw's range is built from, so a packet that
        // disagreed about it would draw the wrong number of triangles.
        let d = RenderMesh::new(1, 3);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }
}
