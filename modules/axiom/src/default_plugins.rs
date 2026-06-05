//! The standard engine plugin bundle.

/// The standard engine capabilities, added to an [`crate::prelude::App`] with
/// `add_plugins(DefaultPlugins)`.
///
/// With `DefaultPlugins` the app drives the full render path each frame; without
/// it the app still steps the deterministic simulation but renders nothing — a
/// headless sim. (As more plugins gain independent behaviour they will be
/// selectable here; today this bundle is the render path.)
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultPlugins;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_plugins_is_a_zero_sized_marker() {
        // The bundle carries no data; its render-enabling behaviour is proven in
        // `app.rs` (`add_plugins(DefaultPlugins)` vs. not). Here we pin that it is
        // a true zero-sized marker, and exercise its `Default`.
        assert_eq!(std::mem::size_of::<DefaultPlugins>(), 0);
        let _ = DefaultPlugins::default();
    }
}
