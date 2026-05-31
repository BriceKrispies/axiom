//! Abstract surface alpha-compositing mode for a host surface.

/// How a future live surface composites its alpha channel with whatever is
/// behind it.
///
/// Abstract host-boundary enum: a future adapter maps these onto the real
/// backend's alpha modes. The host layer never names a WebGPU/OS type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostAlphaMode {
    /// Treat the surface as fully opaque; ignore the alpha channel.
    Opaque,
    /// Alpha is pre-multiplied into the colour channels.
    PreMultiplied,
    /// Alpha is kept separate from the colour channels.
    PostMultiplied,
    /// Defer the choice to the host/compositor default.
    Inherit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(HostAlphaMode::Opaque, HostAlphaMode::PreMultiplied);
        assert_ne!(HostAlphaMode::PreMultiplied, HostAlphaMode::PostMultiplied);
        assert_ne!(HostAlphaMode::PostMultiplied, HostAlphaMode::Inherit);
    }

    #[test]
    fn variants_are_copy_and_equal() {
        let a = HostAlphaMode::Opaque;
        let b = a;
        assert_eq!(a, b);
    }
}
