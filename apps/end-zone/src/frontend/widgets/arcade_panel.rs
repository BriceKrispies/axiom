//! The arcade panel: a beveled metallic slab with thick borders, a dramatic
//! drop shadow, an optional angled silhouette, and an optional moving light
//! sweep (suppressed under reduced motion by the presenter).

/// One arcade panel.
#[derive(Debug, Clone, PartialEq)]
pub struct ArcadePanel {
    /// Oversized condensed heading riveted to the panel's top edge.
    pub title: Option<String>,
    /// Angled (parallelogram) silhouette.
    pub angled: bool,
    /// Translucent steel (pause overlay) vs solid plate.
    pub translucent: bool,
    /// Chrome light sweep across the plate.
    pub sweep: bool,
}

impl ArcadePanel {
    pub fn plate(title: Option<&str>) -> Self {
        ArcadePanel {
            title: title.map(|t| t.to_string()),
            angled: true,
            translucent: false,
            sweep: true,
        }
    }

    pub fn overlay(title: Option<&str>) -> Self {
        ArcadePanel {
            title: title.map(|t| t.to_string()),
            angled: false,
            translucent: true,
            sweep: false,
        }
    }
}
