//! The standard engine plugin bundle.

/// The standard engine capabilities, added to an [`crate::prelude::App`] with
/// `add_plugins(DefaultPlugins)`.
///
/// With `DefaultPlugins` the app drives the full render path each frame; without
/// it the app still steps the deterministic simulation but renders nothing — a
/// headless sim. (As more plugins gain independent behaviour they will be
/// selectable here; today this bundle is the render path.)
#[derive(Debug, Clone, Copy)]
pub struct DefaultPlugins;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_plugins_is_a_zero_sized_marker() {
        assert_eq!(std::mem::size_of::<DefaultPlugins>(), 0);
    }
}
