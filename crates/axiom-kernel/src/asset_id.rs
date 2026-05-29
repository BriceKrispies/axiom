//! Stable identifier for an asset.

use crate::id_macro::define_id;

define_id! {
    /// A stable, strongly-typed identifier for an asset.
    ///
    /// The kernel knows nothing about asset *content* or loading; this is only
    /// an identity primitive future asset layers may build on.
    AssetId
}
