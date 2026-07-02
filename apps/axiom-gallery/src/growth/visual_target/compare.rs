//! Reference-image comparison for the visual-target runner.
//!
//! Decodes a reference PNG, compares it to freshly-rendered RGBA8 pixels, and
//! reports pixel-difference statistics with a pass/fail verdict. The deterministic
//! **Canvas 2D** backend is compared byte-exact (tolerance 0); the **GPU** backend —
//! which is only bit-reproducible on the same adapter — is compared within a
//! tolerance, the same posture as the repo's existing GPU↔Canvas parity test.
//!
//! Only compiled behind the `visual-target` feature (it needs the `png` decoder).

/// Pixel-difference statistics between a produced frame and a reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffReport {
    pub width: u32,
    pub height: u32,
    /// Mean absolute per-channel difference (0..255).
    pub mean_diff: f32,
    /// Largest absolute per-channel difference (0..255).
    pub max_diff: u8,
    /// Fraction of pixels where any channel differs by more than the threshold.
    pub changed_fraction: f32,
}

/// The pass/fail tolerance for a comparison.
#[derive(Debug, Clone, Copy)]
pub struct Tolerance {
    /// Max allowed mean per-channel difference.
    pub mean: f32,
    /// Max allowed single-channel difference.
    pub max: u8,
    /// Per-pixel channel threshold used for `changed_fraction`.
    pub per_pixel: u8,
}

impl Tolerance {
    /// Byte-exact (deterministic Canvas 2D backend).
    pub const EXACT: Tolerance = Tolerance { mean: 0.0, max: 0, per_pixel: 0 };

    /// A lenient default for the GPU backend (same-adapter reproducible, but not
    /// bit-identical across drivers).
    pub const GPU_DEFAULT: Tolerance = Tolerance { mean: 2.0, max: 40, per_pixel: 16 };

    /// Whether `report` passes this tolerance.
    pub fn passes(&self, report: &DiffReport) -> bool {
        report.mean_diff <= self.mean && report.max_diff <= self.max
    }
}

/// Compare two equally-sized RGBA8 buffers. Returns an error if the dimensions or
/// buffer lengths disagree.
pub fn compare_rgba(
    produced: &[u8],
    reference: &[u8],
    width: u32,
    height: u32,
    per_pixel: u8,
) -> Result<DiffReport, String> {
    let expected = width as usize * height as usize * 4;
    (produced.len() == expected && reference.len() == expected)
        .then_some(())
        .ok_or_else(|| {
            format!(
                "buffer size mismatch: produced {}, reference {}, expected {expected} ({width}x{height})",
                produced.len(),
                reference.len()
            )
        })?;

    let mut sum: u64 = 0;
    let mut max_diff: u8 = 0;
    let mut changed: u64 = 0;
    for (p, r) in produced.chunks_exact(4).zip(reference.chunks_exact(4)) {
        let mut pixel_max = 0u8;
        for (pc, rc) in p.iter().zip(r) {
            let d = pc.abs_diff(*rc);
            sum += d as u64;
            pixel_max = pixel_max.max(d);
        }
        max_diff = max_diff.max(pixel_max);
        changed += u64::from(pixel_max > per_pixel);
    }

    let pixels = (width as u64) * (height as u64);
    Ok(DiffReport {
        width,
        height,
        mean_diff: sum as f32 / expected as f32,
        max_diff,
        changed_fraction: changed as f32 / pixels.max(1) as f32,
    })
}

/// Decode an RGBA8 PNG into `(rgba, width, height)`. Rejects non-RGBA8 PNGs (the
/// runner only ever blesses RGBA8 references).
pub fn decode_rgba_png(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().map_err(|e| format!("PNG header: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("PNG decode: {e}"))?;
    (info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight)
        .then_some(())
        .ok_or_else(|| {
            format!("reference must be RGBA8 PNG (got {:?} {:?})", info.color_type, info.bit_depth)
        })?;
    buf.truncate(info.buffer_size());
    Ok((buf, info.width, info.height))
}

/// A red-on-black heatmap of the per-pixel maximum channel difference, as RGBA8
/// (for `--diff` output so a human can see *where* two renders disagree).
pub fn diff_heatmap(produced: &[u8], reference: &[u8]) -> Vec<u8> {
    produced
        .chunks_exact(4)
        .zip(reference.chunks_exact(4))
        .flat_map(|(p, r)| {
            let d = p.iter().zip(r).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
            [d, 0, 0, 255]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_buffers_report_zero() {
        let a = vec![10u8, 20, 30, 255, 40, 50, 60, 255];
        let report = compare_rgba(&a, &a, 2, 1, 0).unwrap();
        assert_eq!(report.mean_diff, 0.0);
        assert_eq!(report.max_diff, 0);
        assert_eq!(report.changed_fraction, 0.0);
        assert!(Tolerance::EXACT.passes(&report));
    }

    #[test]
    fn shifted_buffer_reports_nonzero_and_fails_exact() {
        let a = vec![10u8, 20, 30, 255, 40, 50, 60, 255];
        let mut b = a.clone();
        b[0] = 200; // one channel differs by 190.
        let report = compare_rgba(&a, &b, 2, 1, 16).unwrap();
        assert!(report.mean_diff > 0.0);
        assert_eq!(report.max_diff, 190);
        assert_eq!(report.changed_fraction, 0.5); // one of two pixels changed.
        assert!(!Tolerance::EXACT.passes(&report));
    }

    #[test]
    fn gpu_tolerance_absorbs_small_noise() {
        // Realistic driver noise is *sparse* — a few channels off by a few LSBs —
        // not a uniform bias on every pixel (which would be a systematic shift a
        // lenient tolerance should still catch). Perturb three channels: mean stays
        // well under GPU_DEFAULT.mean (2.0) while max stays under GPU_DEFAULT.max.
        let a = vec![100u8; 16];
        let mut b = a.clone();
        b[0] = b[0].saturating_add(5);
        b[5] = b[5].saturating_add(4);
        b[10] = b[10].saturating_add(3);
        let report = compare_rgba(&a, &b, 2, 2, 16).unwrap();
        assert!((report.mean_diff - 12.0 / 16.0).abs() < 1e-6); // 0.75, under 2.0
        assert!(Tolerance::GPU_DEFAULT.passes(&report));
        assert!(!Tolerance::EXACT.passes(&report)); // any non-zero diff fails EXACT
    }

    #[test]
    fn dimension_mismatch_errors() {
        let a = vec![0u8; 8];
        let b = vec![0u8; 4];
        assert!(compare_rgba(&a, &b, 2, 1, 0).is_err());
    }

    #[test]
    fn png_round_trips_through_decode() {
        // Encode a 2x1 RGBA8 image to an in-memory PNG, then decode it back.
        let pixels = vec![11u8, 22, 33, 255, 44, 55, 66, 255];
        let mut png_bytes: Vec<u8> = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&pixels).unwrap();
        }
        let (decoded, w, h) = decode_rgba_png(&png_bytes).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn heatmap_marks_the_changed_pixel() {
        let a = vec![0u8, 0, 0, 255, 0, 0, 0, 255];
        let mut b = a.clone();
        b[4] = 90;
        let map = diff_heatmap(&a, &b);
        assert_eq!(&map[0..4], &[0, 0, 0, 255]); // unchanged pixel.
        assert_eq!(&map[4..8], &[90, 0, 0, 255]); // changed pixel, red = diff.
    }
}
