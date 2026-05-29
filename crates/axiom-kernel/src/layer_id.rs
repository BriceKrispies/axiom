//! Stable identifier for a layer.

use crate::id_macro::define_id;

define_id! {
    /// A stable, strongly-typed identifier for a layer.
    ///
    /// This is the identity primitive only. The *ordering* contract between
    /// layers (who may import whom) lives in
    /// [`crate::layer_manifest::LayerManifest`] and uses an explicit layer
    /// index, not this id.
    LayerId
}
