//! Contact-sheet composition: lay the captured frames out in a grid, scale each
//! to fit its tile without distortion (letterboxed), and draw a small readable
//! label band under each tile — all into one RGBA8 buffer.
//!
//! Text uses a built-in compact 5x7 uppercase bitmap font blitted directly into
//! the buffer; there are **no external font assets**. Labels are uppercased so the
//! font needs only `A-Z 0-9` and a few punctuation glyphs.

use crate::capture_plan::contact_grid;
use crate::replay_driver::CapturedFrame;

/// Background of the sheet and letterbox bars.
const SHEET_BG: [u8; 4] = [18, 20, 26, 255];
/// Background of a tile's label band.
const LABEL_BG: [u8; 4] = [10, 12, 16, 255];
/// Label text colour.
const TEXT: [u8; 4] = [232, 236, 242, 255];

const PAD: u32 = 10;
/// Largest tile content width; larger captures are downscaled to keep the sheet
/// a reviewable size.
const MAX_TILE_W: u32 = 480;
/// Font pixel scale (each 5x7 glyph becomes 10x14).
const FONT_SCALE: u32 = 2;
const GLYPH_W: u32 = 5;
const GLYPH_H: u32 = 7;
/// Horizontal advance per character (glyph + 1px spacing, scaled).
const ADVANCE: u32 = (GLYPH_W + 1) * FONT_SCALE;
const LINE_H: u32 = GLYPH_H * FONT_SCALE;
const TEXT_MARGIN: u32 = 6;
const LINE_GAP: u32 = 4;
/// Two label lines plus padding.
const LABEL_H: u32 = TEXT_MARGIN * 2 + LINE_H * 2 + LINE_GAP;

/// A simple RGBA8 image the sheet is composed into.
pub struct Sheet {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Compose `frames` into one contact sheet. `app`/`scenario`/`backend` fill the
/// per-tile label along with each frame's tick and optional marker.
pub fn compose(
    frames: &[CapturedFrame],
    columns: u32,
    app: &str,
    scenario: &str,
    backend: &str,
) -> Sheet {
    let (rows, cols) = contact_grid(frames.len(), columns);
    // Uniform tile content box: fit the largest captured frame into MAX_TILE_W.
    let (max_w, max_h) = frames
        .iter()
        .fold((1, 1), |(w, h), f| (w.max(f.width), h.max(f.height)));
    let (tile_w, tile_h) = fit_inside(max_w, max_h, MAX_TILE_W, MAX_TILE_W);
    let cell_h = tile_h + LABEL_H;

    let width = cols * tile_w + (cols + 1) * PAD;
    let height = rows * cell_h + (rows + 1) * PAD;
    let (width, height) = (width.max(1), height.max(1));
    let mut sheet = Sheet {
        rgba: fill(width, height, SHEET_BG),
        width,
        height,
    };

    frames.iter().enumerate().for_each(|(i, frame)| {
        let (r, c) = (i as u32 / cols, i as u32 % cols);
        let x = PAD + c * (tile_w + PAD);
        let y = PAD + r * (cell_h + PAD);
        blit_scaled(&mut sheet, x, y, tile_w, tile_h, frame);
        let line1 = format!("{app}  {scenario}");
        let marker = frame
            .point
            .marker
            .as_deref()
            .map(|m| format!("  {m}"))
            .unwrap_or_default();
        let line2 = format!("{backend}  TICK {}{marker}", frame.point.tick);
        draw_label(&mut sheet, x, y + tile_h, tile_w, &line1, &line2);
    });

    sheet
}

/// Fit `(sw, sh)` inside `(bw, bh)` preserving aspect (never upscaling past the box).
fn fit_inside(sw: u32, sh: u32, bw: u32, bh: u32) -> (u32, u32) {
    let scale = (bw as f64 / sw.max(1) as f64)
        .min(bh as f64 / sh.max(1) as f64)
        .min(1.0);
    (
        ((sw as f64 * scale).round() as u32).max(1),
        ((sh as f64 * scale).round() as u32).max(1),
    )
}

/// A solid-filled RGBA buffer.
fn fill(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
    color
        .iter()
        .copied()
        .cycle()
        .take((w * h * 4) as usize)
        .collect()
}

/// Write one pixel (bounds-checked).
fn put(sheet: &mut Sheet, x: u32, y: u32, color: [u8; 4]) {
    (x < sheet.width && y < sheet.height).then(|| {
        let off = ((y * sheet.width + x) * 4) as usize;
        sheet.rgba[off..off + 4].copy_from_slice(&color);
    });
}

/// Fill a rectangle (clipped to the sheet).
fn rect(sheet: &mut Sheet, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    (0..h).for_each(|dy| (0..w).for_each(|dx| put(sheet, x + dx, y + dy, color)));
}

/// Nearest-neighbour blit of `frame`, scaled to fit `(tile_w, tile_h)` and
/// centred (letterboxed) into the tile at `(x, y)`.
fn blit_scaled(sheet: &mut Sheet, x: u32, y: u32, tile_w: u32, tile_h: u32, frame: &CapturedFrame) {
    let (fw, fh) = fit_inside(frame.width, frame.height, tile_w, tile_h);
    let ox = x + (tile_w - fw) / 2;
    let oy = y + (tile_h - fh) / 2;
    (0..fh).for_each(|py| {
        (0..fw).for_each(|px| {
            let sx = (px * frame.width / fw).min(frame.width - 1);
            let sy = (py * frame.height / fh).min(frame.height - 1);
            let so = ((sy * frame.width + sx) * 4) as usize;
            let color = [
                frame.rgba[so],
                frame.rgba[so + 1],
                frame.rgba[so + 2],
                frame.rgba[so + 3],
            ];
            put(sheet, ox + px, oy + py, color);
        });
    });
}

/// Draw a tile's two-line label band under the image.
fn draw_label(sheet: &mut Sheet, x: u32, y: u32, tile_w: u32, line1: &str, line2: &str) {
    rect(sheet, x, y, tile_w, LABEL_H, LABEL_BG);
    let max_chars = ((tile_w.saturating_sub(TEXT_MARGIN * 2)) / ADVANCE) as usize;
    draw_text(sheet, x + TEXT_MARGIN, y + TEXT_MARGIN, line1, max_chars);
    draw_text(
        sheet,
        x + TEXT_MARGIN,
        y + TEXT_MARGIN + LINE_H + LINE_GAP,
        line2,
        max_chars,
    );
}

/// Draw an uppercased string (truncated to `max_chars`) at `(x, y)`.
fn draw_text(sheet: &mut Sheet, x: u32, y: u32, text: &str, max_chars: usize) {
    text.to_uppercase()
        .chars()
        .take(max_chars)
        .enumerate()
        .for_each(|(i, ch)| draw_glyph(sheet, x + i as u32 * ADVANCE, y, ch));
}

/// Blit one scaled glyph.
fn draw_glyph(sheet: &mut Sheet, x: u32, y: u32, ch: char) {
    let rows = glyph(ch);
    (0..GLYPH_H).for_each(|gy| {
        let bits = rows[gy as usize];
        (0..GLYPH_W).for_each(|gx| {
            let on = bits & (1 << (GLYPH_W - 1 - gx)) != 0;
            on.then(|| {
                rect(
                    sheet,
                    x + gx * FONT_SCALE,
                    y + gy * FONT_SCALE,
                    FONT_SCALE,
                    FONT_SCALE,
                    TEXT,
                );
            });
        });
    });
}

/// The 5x7 bitmap for a character (low 5 bits per row, MSB = leftmost column).
/// Unmapped characters render blank.
fn glyph(ch: char) -> [u8; 7] {
    match ch {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0E],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0E, 0x11, 0x10, 0x0E, 0x01, 0x11, 0x0E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x08],
        '|' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        _ => [0x00; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_plan::CapturePoint;

    fn solid_frame(w: u32, h: u32, tick: u64, marker: Option<&str>) -> CapturedFrame {
        CapturedFrame {
            rgba: vec![120; (w * h * 4) as usize],
            width: w,
            height: h,
            point: CapturePoint {
                tick,
                marker: marker.map(str::to_string),
            },
        }
    }

    #[test]
    fn compose_lays_out_the_expected_grid_dimensions() {
        // 2 frames of 96x60 at 4 columns -> 1 row, 2 cols.
        let frames = vec![
            solid_frame(96, 60, 0, None),
            solid_frame(96, 60, 10, Some("m")),
        ];
        let sheet = compose(
            &frames,
            4,
            "soccer_penalty",
            "default_penalty_kick",
            "canvas2d",
        );
        let (tile_w, tile_h) = fit_inside(96, 60, MAX_TILE_W, MAX_TILE_W);
        let expected_w = 2 * tile_w + 3 * PAD;
        let expected_h = (tile_h + LABEL_H) + 2 * PAD;
        assert_eq!(sheet.width, expected_w);
        assert_eq!(sheet.height, expected_h);
        assert_eq!(sheet.rgba.len(), (expected_w * expected_h * 4) as usize);
    }

    #[test]
    fn compose_draws_frame_pixels_into_the_first_tile() {
        let frames = vec![solid_frame(60, 60, 5, None)];
        let sheet = compose(&frames, 4, "a", "b", "gpu");
        // The tile centre should carry the frame's fill colour (120), not the bg.
        let (cx, cy) = (PAD + 30, PAD + 30);
        let off = ((cy * sheet.width + cx) * 4) as usize;
        assert_eq!(sheet.rgba[off], 120);
    }

    #[test]
    fn glyph_maps_known_and_unknown_characters() {
        assert_ne!(glyph('A'), [0u8; 7]);
        assert_ne!(glyph('7'), [0u8; 7]);
        assert_eq!(glyph(' '), [0u8; 7]);
        assert_eq!(glyph('~'), [0u8; 7]);
    }

    #[test]
    fn fit_inside_never_upscales() {
        assert_eq!(fit_inside(100, 100, 480, 480), (100, 100));
        assert_eq!(fit_inside(960, 600, 480, 480), (480, 300));
    }
}
