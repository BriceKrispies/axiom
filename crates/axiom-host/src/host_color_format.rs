//! Abstract surface colour format for a host surface.

/// The colour format a future live surface should present in.
///
/// Abstract host-boundary enum. These are the two byte-order/sRGB layouts a
/// future presentation backend will realistically expose for a window
/// surface; a future adapter maps them onto the real backend's format
/// enumeration. The host layer never names a WebGPU/OS texture-format type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostColorFormat {
    /// 8-bit RGBA, sRGB-encoded.
    Rgba8UnormSrgb,
    /// 8-bit BGRA, sRGB-encoded.
    Bgra8UnormSrgb,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(HostColorFormat::Rgba8UnormSrgb, HostColorFormat::Bgra8UnormSrgb);
    }

    #[test]
    fn variants_are_copy_and_equal() {
        let f = HostColorFormat::Rgba8UnormSrgb;
        let g = f;
        assert_eq!(f, g);
    }
}
