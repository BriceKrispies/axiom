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
    fn constructs() {
        let _ = DefaultPlugins;
        let _ = DefaultPlugins::default();
    }
}
