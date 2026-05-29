//! A declared dependency of one layer on another, by layer index.

/// A declared dependency on another layer, identified by that layer's index.
///
/// Layer indices are ordinals: the kernel is index `0`, and a layer may only
/// depend on strictly lower indices. The dependency carries just the target
/// index; the legality of the import is judged by
/// [`crate::layer_import_rule::LayerImportRule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerDependency {
    layer: u16,
}

impl LayerDependency {
    /// Declare a dependency on the layer at `layer` index.
    pub const fn new(layer: u16) -> Self {
        LayerDependency { layer }
    }

    /// The index of the depended-upon layer.
    pub const fn layer(self) -> u16 {
        self.layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_layer_round_trip() {
        assert_eq!(LayerDependency::new(2).layer(), 2);
    }

    #[test]
    fn equality_detects_duplicates() {
        assert_eq!(LayerDependency::new(1), LayerDependency::new(1));
        assert_ne!(LayerDependency::new(1), LayerDependency::new(2));
    }
}
