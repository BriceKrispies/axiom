//! App-local **retro 32-bit pixel-art textures** for the penalty diorama.
//!
//! Low-resolution, hand-authored RGBA8 albedo textures the meshed render path
//! attaches to materials (via `RunningApp::add_texture_data`) so the flat-shaded
//! diorama reads as a textured retro 32-bit scene: a crowd of people instead of flat
//! cards, an AXIOM ad-board, kit fabric, a panelled ball. Each generator is a
//! pure function of constants (deterministic), returns `(width, height, RGBA8)`,
//! and — being app code — is outside the Branchless Law, so it uses ordinary
//! loops and a tiny bitmap font.

/// One authored texture: dimensions + row-major RGBA8 pixels.
struct Canvas {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Canvas {
    fn filled(w: u32, h: u32, rgb: [u8; 3]) -> Self {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        Canvas { w, h, px }
    }

    fn set(&mut self, x: u32, y: u32, rgb: [u8; 3]) {
        let i = ((y * self.w + x) * 4) as usize;
        self.px[i] = rgb[0];
        self.px[i + 1] = rgb[1];
        self.px[i + 2] = rgb[2];
        self.px[i + 3] = 255;
    }

    fn into_texture(self) -> (u32, u32, Vec<u8>) {
        (self.w, self.h, self.px)
    }
}

/// A cheap deterministic hash → `0..=255`, used to scatter crowd colours.
fn hash8(x: u32, y: u32, salt: u32) -> u32 {
    let mut h = x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ salt.wrapping_mul(83_492_791);
    h ^= h >> 13;
    h = h.wrapping_mul(1_274_126_177);
    (h >> 24) & 0xff
}

// --- tiny 5x7 bitmap font (only the glyphs the boards/kit need) --------------

/// The 7 rows of a 5-wide glyph; bit 4 (0x10) is the leftmost column.
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'I' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        _ => [0; 7],
    }
}

/// Blit `text` (upper-case; unknown chars = space) at `(ox, oy)` in `rgb`, scaled
/// by `scale`. 5x7 glyphs, one column of spacing.
fn draw_text(c: &mut Canvas, text: &str, ox: u32, oy: u32, scale: u32, rgb: [u8; 3]) {
    let mut pen = ox;
    for ch in text.chars() {
        let g = glyph(ch);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..5u32 {
                if (bits >> (4 - col)) & 1 == 1 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = pen + col * scale + sx;
                            let py = oy + row as u32 * scale + sy;
                            if px < c.w && py < c.h {
                                c.set(px, py, rgb);
                            }
                        }
                    }
                }
            }
        }
        pen += 6 * scale;
    }
}

// --- the textures ------------------------------------------------------------

/// A packed crowd: a dark terrace scattered with small bright clusters of colour
/// (heads/shirts), so a flat card reads as a stand full of people.
pub fn crowd() -> (u32, u32, Vec<u8>) {
    let (w, h) = (48u32, 48u32);
    let shirts: [[u8; 3]; 6] = [
        [200, 60, 60], [60, 90, 190], [210, 190, 70],
        [220, 220, 220], [90, 170, 90], [180, 110, 70],
    ];
    let mut c = Canvas::filled(w, h, [26, 24, 30]);
    for y in 0..h {
        for x in 0..w {
            let r = hash8(x, y, 1);
            // ~45% of cells are a person; pick a shirt colour, with a darker head
            // dot every other row for a bit of vertical structure.
            if r < 115 {
                let shirt = shirts[(hash8(x, y, 2) as usize) % shirts.len()];
                let head = (y % 2 == 0) && r < 55;
                let rgb = if head { [40, 30, 26] } else { shirt };
                c.set(x, y, rgb);
            }
        }
    }
    c.into_texture()
}

/// A team kit texture: a fabric base with faint vertical shading and a bold
/// number decal centred on the (single) face the UV maps to.
pub fn jersey(base: [u8; 3], number: &str, number_rgb: [u8; 3]) -> (u32, u32, Vec<u8>) {
    let (w, h) = (32u32, 32u32);
    let mut c = Canvas::filled(w, h, base);
    // Subtle fabric shading columns.
    for y in 0..h {
        for x in 0..w {
            if (x / 2) % 2 == 0 {
                let d = |v: u8| (v as u32 * 92 / 100) as u8;
                c.set(x, y, [d(base[0]), d(base[1]), d(base[2])]);
            }
        }
    }
    // Number, roughly centred (two 5-wide glyphs, scale 2 → 22px wide, 14 tall).
    let scale = 2;
    let tw = number.chars().count() as u32 * 6 * scale - scale;
    let ox = (w.saturating_sub(tw)) / 2;
    draw_text(&mut c, number, ox, (h - 7 * scale) / 2, scale, number_rgb);
    c.into_texture()
}

/// A flat kit texture (keeper / shorts) — fabric shading, no number.
pub fn kit(base: [u8; 3]) -> (u32, u32, Vec<u8>) {
    let (w, h) = (24u32, 24u32);
    let mut c = Canvas::filled(w, h, base);
    for y in 0..h {
        for x in 0..w {
            if (x + y) % 3 == 0 {
                let d = |v: u8| (v as u32 * 90 / 100) as u8;
                c.set(x, y, [d(base[0]), d(base[1]), d(base[2])]);
            }
        }
    }
    c.into_texture()
}

/// The AXIOM ad-board: a coloured board with white pixel lettering.
pub fn ad_axiom() -> (u32, u32, Vec<u8>) {
    let (w, h) = (64u32, 20u32);
    let mut c = Canvas::filled(w, h, [176, 40, 46]);
    // Thin dark top/bottom rails.
    for x in 0..w {
        c.set(x, 0, [40, 12, 14]);
        c.set(x, h - 1, [40, 12, 14]);
    }
    draw_text(&mut c, "AXIOM", 5, 6, 1, [245, 245, 245]);
    c.into_texture()
}

/// A generic dark ad-board (SPORT), so the non-AXIOM boards read as signage too.
pub fn ad_generic() -> (u32, u32, Vec<u8>) {
    let (w, h) = (64u32, 20u32);
    let mut c = Canvas::filled(w, h, [46, 78, 190]);
    // Thin dark top/bottom rails, matching the AXIOM board.
    for x in 0..w {
        c.set(x, 0, [14, 22, 60]);
        c.set(x, h - 1, [14, 22, 60]);
    }
    draw_text(&mut c, "SPORTS", 4, 6, 1, [244, 246, 250]);
    c.into_texture()
}

/// The soccer ball: white with black pentagon-ish spots on a lat-long wrap.
pub fn ball() -> (u32, u32, Vec<u8>) {
    let (w, h) = (32u32, 32u32);
    let mut c = Canvas::filled(w, h, [244, 244, 248]);
    // The classic panelled look: a central hex + a ring of pentagons, crisp black.
    let spots: [(i32, i32, i32); 9] = [
        (16, 16, 5),
        (5, 8, 3),
        (27, 8, 3),
        (8, 23, 3),
        (24, 23, 3),
        (16, 3, 3),
        (16, 29, 3),
        (2, 17, 2),
        (30, 17, 2),
    ];
    for y in 0..h {
        for x in 0..w {
            for (sx, sy, r) in spots {
                let dx = x as i32 - sx;
                let dy = y as i32 - sy;
                if dx * dx + dy * dy <= r * r {
                    c.set(x, y, [16, 16, 20]);
                }
            }
        }
    }
    c.into_texture()
}

/// A goal-net texture: white strands on a **transparent** ground so the GPU's
/// alpha-cutout (`albedo.a < 0.5` discards) turns a few flat planes into a real
/// see-through net. A square mesh — strands every `CELL` pixels — mapped once
/// across each net plane (~10×8 cells over the goal mouth).
pub fn net() -> (u32, u32, Vec<u8>) {
    let (w, h) = (64u32, 48u32);
    let cell = 6u32;
    let strand = [226u8, 230, 235];
    // Start fully transparent (alpha 0 in the holes).
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            // A strand where either axis crosses a grid line (2px thick for read).
            let on = x % cell < 1 || y % cell < 1;
            if on {
                let i = ((y * w + x) * 4) as usize;
                px[i] = strand[0];
                px[i + 1] = strand[1];
                px[i + 2] = strand[2];
                px[i + 3] = 255;
            }
        }
    }
    (w, h, px)
}

/// Skin: a warm base with faint dither so heads/hands aren't perfectly flat.
pub fn skin(base: [u8; 3]) -> (u32, u32, Vec<u8>) {
    let (w, h) = (16u32, 16u32);
    let mut c = Canvas::filled(w, h, base);
    for y in 0..h {
        for x in 0..w {
            if (x + y) % 4 == 0 {
                let d = |v: u8| (v as u32 * 94 / 100) as u8;
                c.set(x, y, [d(base[0]), d(base[1]), d(base[2])]);
            }
        }
    }
    c.into_texture()
}
