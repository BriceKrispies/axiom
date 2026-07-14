//! Procedural surface generation for every lab material — nothing is imported.
//! Bases are baked through the engine's `axiom-proc-texture` recipe facade
//! (`Noise` for the grass, `Solid` fills, `Spots` for the soccer rosette and
//! the bowling-ball finger holes — its documented purpose). The op catalog has
//! no line/arc primitive, so the field markings, baseball seams, and football
//! stripes/laces are painted by a minimal app-local deterministic painter over
//! the baked base (the smallest local generator for the missing capability).
//! Everything is a pure function of constants — one bake is byte-identical to
//! the next.

use axiom_proc_texture::{ProcTextureApi, TextureOp};
use axiom_recipe::{Color, Param, RecipeGraph, RecipeId};

/// A baked RGBA8 texture ready for `RunningApp::add_texture_data`.
#[derive(Debug, Clone)]
pub struct BakedTexture {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

fn int(v: u32) -> Param {
    Param::int(v)
}

fn col(r: u8, g: u8, b: u8) -> Param {
    Param::color(Color::rgba(r, g, b, 0xFF))
}

/// Bake a single-node texture recipe deterministically.
fn bake(recipe_id: u64, op: TextureOp, params: Vec<Param>) -> BakedTexture {
    let mut graph = RecipeGraph::new(RecipeId::from_raw(recipe_id), 1);
    graph.add(op as u16, params, vec![]);
    let baked = ProcTextureApi::new()
        .bake(&graph, recipe_id)
        .expect("authored texture recipe bakes");
    BakedTexture {
        width: baked.width(),
        height: baked.height(),
        pixels: baked.into_pixels(),
    }
}

// --- the minimal painter (lines / rects / circle outlines only) -----------------

fn put(tex: &mut BakedTexture, x: i32, y: i32, rgba: [u8; 4]) {
    if x < 0 || y < 0 || x >= tex.width as i32 || y >= tex.height as i32 {
        return;
    }
    let i = ((y as u32 * tex.width + x as u32) * 4) as usize;
    tex.pixels[i..i + 4].copy_from_slice(&rgba);
}

fn fill_rect(tex: &mut BakedTexture, x0: i32, y0: i32, x1: i32, y1: i32, rgba: [u8; 4]) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            put(tex, x, y, rgba);
        }
    }
}

fn rect_outline(tex: &mut BakedTexture, x0: i32, y0: i32, x1: i32, y1: i32, t: i32, rgba: [u8; 4]) {
    fill_rect(tex, x0, y0, x1, y0 + t - 1, rgba);
    fill_rect(tex, x0, y1 - t + 1, x1, y1, rgba);
    fill_rect(tex, x0, y0, x0 + t - 1, y1, rgba);
    fill_rect(tex, x1 - t + 1, y0, x1, y1, rgba);
}

fn circle_outline(tex: &mut BakedTexture, cx: f32, cy: f32, r: f32, t: f32, rgba: [u8; 4]) {
    let (lo, hi) = (r - t * 0.5, r + t * 0.5);
    let (x0, x1) = ((cx - hi).floor() as i32, (cx + hi).ceil() as i32);
    let (y0, y1) = ((cy - hi).floor() as i32, (cy + hi).ceil() as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt();
            if d >= lo && d <= hi {
                put(tex, x, y, rgba);
            }
        }
    }
}

// --- the lab's surfaces ----------------------------------------------------------

const LINE_WHITE: [u8; 4] = [238, 240, 236, 255];
const LINE_FAINT: [u8; 4] = [148, 178, 132, 255];

/// The practice-field surface: value-noise grass with painted markings — the
/// boundary rectangle, the center line + circle, and two faint practice zones.
pub fn field_texture() -> BakedTexture {
    let (w, h) = (256i32, 384i32);
    let mut tex = bake(
        101,
        TextureOp::Noise,
        vec![
            int(w as u32),
            int(h as u32),
            int(20),
            col(0x3A, 0x6E, 0x30),
            col(0x4C, 0x86, 0x3C),
        ],
    );
    // Boundary rectangle.
    rect_outline(&mut tex, 10, 10, w - 11, h - 11, 3, LINE_WHITE);
    // Center line + center circle.
    fill_rect(&mut tex, 10, h / 2 - 1, w - 11, h / 2 + 1, LINE_WHITE);
    circle_outline(
        &mut tex,
        w as f32 / 2.0,
        h as f32 / 2.0,
        34.0,
        3.0,
        LINE_WHITE,
    );
    // Two faint practice zones, one in each half.
    rect_outline(&mut tex, 40, 48, 120, 128, 2, LINE_FAINT);
    rect_outline(&mut tex, w - 121, h - 129, w - 41, h - 49, 2, LINE_FAINT);
    tex
}

/// Soccer ball: white base with the dark panel rosette (the `Spots` op's
/// documented purpose) — a center spot, a mid ring, and wrap-aware edge spots.
pub fn soccer_texture() -> BakedTexture {
    let size = 128u32;
    let mut params = vec![
        int(size),
        int(size),
        col(0xF2, 0xF2, 0xF0),
        col(0x18, 0x18, 0x1C),
    ];
    let mut spots: Vec<(u32, u32, u32)> = vec![(64, 64, 12)];
    for k in 0..5 {
        let a = k as f32 * core::f32::consts::TAU / 5.0;
        spots.push((
            (64.0 + a.cos() * 34.0) as u32,
            (64.0 + a.sin() * 30.0) as u32,
            10,
        ));
    }
    // A row near each pole and the wrap seam (u=0/128 meet on the sphere).
    spots.push((16, 16, 8));
    spots.push((96, 14, 8));
    spots.push((32, 112, 8));
    spots.push((112, 110, 8));
    spots.push((0, 64, 9));
    spots.push((127, 64, 9));
    params.push(int(spots.len() as u32));
    for (cx, cy, r) in spots {
        params.push(int(cx));
        params.push(int(cy));
        params.push(int(r));
    }
    bake(102, TextureOp::Spots, params)
}

/// Bowling ball: dark glossy base with three finger holes (`Spots`).
pub fn bowling_texture() -> BakedTexture {
    let holes = [(56u32, 42u32, 5u32), (72, 42, 5), (64, 57, 6)];
    let mut params = vec![
        int(128),
        int(128),
        col(0x1E, 0x1B, 0x2A),
        col(0x0A, 0x09, 0x10),
        int(holes.len() as u32),
    ];
    for (cx, cy, r) in holes {
        params.push(int(cx));
        params.push(int(cy));
        params.push(int(r));
    }
    bake(103, TextureOp::Spots, params)
}

/// Baseball: white leather with two painted red seam loops (no arc op exists).
pub fn baseball_texture() -> BakedTexture {
    let mut tex = bake(
        104,
        TextureOp::Solid,
        vec![int(128), int(128), col(0xEC, 0xE8, 0xE0)],
    );
    let seam = [0xBC, 0x2C, 0x34, 0xFF];
    circle_outline(&mut tex, 42.0, 64.0, 30.0, 3.0, seam);
    circle_outline(&mut tex, 86.0, 64.0, 30.0, 3.0, seam);
    tex
}

/// Football: brown leather, two white tip stripes, and a painted lace strip.
/// The ball's long axis is the texture's poles (v = 0 / v = 1).
pub fn football_texture() -> BakedTexture {
    let mut tex = bake(
        105,
        TextureOp::Solid,
        vec![int(128), int(128), col(0x76, 0x3A, 0x1C)],
    );
    // Tip stripes (latitude bands near the poles).
    fill_rect(&mut tex, 0, 14, 127, 19, LINE_WHITE);
    fill_rect(&mut tex, 0, 108, 127, 113, LINE_WHITE);
    // Lace spine along one meridian + cross ticks.
    fill_rect(&mut tex, 62, 46, 65, 82, LINE_WHITE);
    for k in 0..6 {
        let y = 48 + k * 6;
        fill_rect(&mut tex, 57, y, 70, y + 1, LINE_WHITE);
    }
    tex
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_well_formed(t: &BakedTexture) {
        assert_eq!(t.pixels.len(), (t.width * t.height * 4) as usize);
        assert!(t.pixels.chunks(4).all(|p| p[3] == 0xFF), "opaque RGBA");
    }

    #[test]
    fn every_surface_bakes_well_formed_and_deterministic() {
        for baker in [
            field_texture,
            soccer_texture,
            bowling_texture,
            baseball_texture,
            football_texture,
        ] {
            let a = baker();
            is_well_formed(&a);
            assert_eq!(a.pixels, baker().pixels, "bakes are byte-identical");
        }
    }

    #[test]
    fn the_field_carries_markings_over_grass() {
        let tex = field_texture();
        let texel = |x: u32, y: u32| {
            let i = ((y * tex.width + x) * 4) as usize;
            [tex.pixels[i], tex.pixels[i + 1], tex.pixels[i + 2]]
        };
        // The boundary line is white; the middle of a quadrant is green.
        assert_eq!(texel(11, 11), [LINE_WHITE[0], LINE_WHITE[1], LINE_WHITE[2]]);
        assert_eq!(
            texel(128, 192),
            [LINE_WHITE[0], LINE_WHITE[1], LINE_WHITE[2]]
        );
        let quad = texel(200, 80);
        assert!(
            quad[1] > quad[0] && quad[1] > quad[2],
            "grass reads green, got {quad:?}"
        );
    }

    #[test]
    fn ball_skins_carry_their_identity_marks() {
        let soccer = soccer_texture();
        let center = ((64 * soccer.width + 64) * 4) as usize;
        assert!(soccer.pixels[center] < 0x40, "soccer center panel is dark");
        let corner = ((100 * soccer.width + 30) * 4) as usize;
        assert!(soccer.pixels[corner] > 0xD0, "soccer base is white");

        let bowling = bowling_texture();
        let hole = ((42 * bowling.width + 56) * 4) as usize;
        let base = ((100 * bowling.width + 64) * 4) as usize;
        assert!(
            bowling.pixels[hole] < bowling.pixels[base] + 20,
            "finger hole is darker"
        );

        let baseball = baseball_texture();
        let seam = ((64 * baseball.width + 12) * 4) as usize;
        assert!(
            baseball.pixels[seam] > 0xA0 && baseball.pixels[seam + 1] < 0x60,
            "red seam"
        );

        let football = football_texture();
        let lace = ((60 * football.width + 63) * 4) as usize;
        assert!(football.pixels[lace] > 0xD0, "white lace on the football");
    }
}
