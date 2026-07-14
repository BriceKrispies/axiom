//! Render the Growth worldgen output to PNGs (no external crates, no browser).
//!
//! Axiom-growth has no renderer yet (presentation is the deferred layer), so
//! this example visualises the real simulation output directly:
//!   - overworld.png : equirectangular biome/elevation map of the whole planet
//!     (sampled via the surface atlas + sampler).
//!   - local.png     : hillshaded heightfield of the streamed game-world chunks
//!     around the play anchor (the "local worldgen").
//!
//! Run: cargo run -p axiom-growth --example growth_render_maps

use axiom_growth::curves;
use axiom_growth::gameworld;
use axiom_growth::model_world::{GameWorldLocalMap, CHUNK_SIZE_CELLS, CHUNK_VERT_SIDE};
use axiom_growth::presets::PlanetPreset;
use axiom_growth::sampler::{self, biome};
use axiom_growth::Growth;
use axiom_math::Vec3;
use std::collections::HashMap;

fn main() {
    let g = Growth::generate("axiom-growth-demo", PlanetPreset::Earthlike, 40_000);
    eprintln!(
        "generated: regions={}, land_fraction(target~{:.2}), world_hash={:#x}",
        g.atlas.region_count(),
        g.genome.implied_land_fraction(),
        g.world_hash
    );

    render_globe(&g, 900, "apps/axiom-growth/examples/growth_overworld.png");
    render_firstperson(&g, "apps/axiom-growth/examples/growth_local.png");
}

// Overworld: a 3D sphere (orthographic projection of the planet), centered on
// the play anchor and diffuse-shaded for roundness — like Growth's debug globe.
fn render_globe(g: &Growth, size: usize, path: &str) {
    // Camera basis: look straight at the play anchor so the globe is centered on
    // where the local terrain lives. forward = anchor; right/up complete a frame.
    let lm = GameWorldLocalMap::anchored(&g.atlas);
    let forward = normalize3(lm.anchor_dir);
    let world_up = [0.0f32, 1.0, 0.0];
    let mut right = cross3(world_up, forward);
    if len3(right) < 1e-4 {
        right = [1.0, 0.0, 0.0];
    }
    let right = normalize3(right);
    let up = normalize3(cross3(forward, right));

    // Light in view space (upper-left, slightly toward camera) for a lit ball.
    let light = normalize3([-0.4, 0.45, 0.8]);
    let r_px = size as f32 * 0.5 - 6.0;
    let cx = size as f32 * 0.5;
    let cy = size as f32 * 0.5;

    let mut rgb = vec![8u8; size * size * 3]; // near-black background
    for py in 0..size {
        for px in 0..size {
            let sx = (px as f32 + 0.5 - cx) / r_px; // [-1,1] across the disc
            let sy = (cy - (py as f32 + 0.5)) / r_px; // y up
            let r2 = sx * sx + sy * sy;
            if r2 > 1.0 {
                continue; // outside the sphere
            }
            let sz = (1.0 - r2).sqrt(); // front hemisphere
                                        // World direction of this surface point (rotate view -> world).
            let dir = Vec3::new(
                right[0] * sx + up[0] * sy + forward[0] * sz,
                right[1] * sx + up[1] * sy + forward[1] * sz,
                right[2] * sx + up[2] * sy + forward[2] * sz,
            );
            let s = sampler::sample_surface(&g.atlas, dir);
            let (cr, cg, cb) = color_for(s.biome.0, s.elevation.get(), s.moisture.get());
            // View-space normal is (sx, sy, sz): diffuse + ambient + limb darkening.
            let ndotl = (sx * light[0] + sy * light[1] + sz * light[2]).max(0.0);
            let shade = (0.30 + 0.85 * ndotl).min(1.15);
            let i = (py * size + px) * 3;
            rgb[i] = (cr as f32 * shade).min(255.0) as u8;
            rgb[i + 1] = (cg as f32 * shade).min(255.0) as u8;
            rgb[i + 2] = (cb as f32 * shade).min(255.0) as u8;
        }
    }
    write_png(path, size, size, &rgb);
    eprintln!("wrote {} ({}x{} sphere)", path, size, size);
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn len3(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn color_for(biome_id: u32, elevation: f32, moisture: f32) -> (u8, u8, u8) {
    if biome_id == biome::OCEAN || elevation < 0.0 {
        // ocean: deeper = darker blue
        let depth = (-elevation).clamp(0.0, 1.0);
        let shade = 1.0 - 0.6 * depth;
        return (
            (30.0 * shade) as u8,
            (70.0 * shade) as u8,
            (150.0 * shade + 40.0) as u8,
        );
    }
    // land: base biome colour, brightened by elevation (snowcaps high up)
    let base = match biome_id {
        x if x == biome::DESERT => (210, 180, 110),
        x if x == biome::RAINFOREST => (30, 120, 40),
        x if x == biome::TUNDRA => (170, 170, 160),
        x if x == biome::TAIGA => (40, 90, 70),
        _ => (90, 140, 70),
    };
    let e = elevation.clamp(0.0, 1.0);
    let snow = (e - 0.6).max(0.0) / 0.4; // blend to white above 0.6
    let dry = 1.0 - 0.25 * moisture;
    let mix = |c: u8| {
        let v = c as f32 * (0.6 + 0.4 * (1.0 - e)) * dry;
        (v * (1.0 - snow) + 250.0 * snow) as u8
    };
    (mix(base.0), mix(base.1), mix(base.2))
}

fn ramp(t: f32) -> (u8, u8, u8) {
    let stops = [
        (0.0, (60, 110, 60)),
        (0.45, (110, 140, 70)),
        (0.7, (130, 110, 80)),
        (0.9, (160, 150, 140)),
        (1.0, (245, 245, 250)),
    ];
    for i in 0..stops.len() - 1 {
        let (t0, c0) = stops[i];
        let (t1, c1) = stops[i + 1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            return (
                lerp(c0.0, c1.0, f),
                lerp(c0.1, c1.1, f),
                lerp(c0.2, c1.2, f),
            );
        }
    }
    stops[stops.len() - 1].1
}

fn lerp(a: u8, b: u8, f: f32) -> u8 {
    curves::lerp(a as f32, b as f32, f) as u8
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

// First-person: "descend" onto the spot and look around, human-sized.
// A heightfield raymarcher (Voxel-Space style) over the SAME chunk worldgen:
// for each screen column it marches a ray out to the horizon, tracking the
// occluding skyline, and paints terrain under a shaded sky. No GPU/browser.
struct Terrain<'a> {
    atlas: &'a axiom_growth::model_planet::PlanetSurfaceAtlas,
    localmap: &'a GameWorldLocalMap,
    seed: u64,
    cache: HashMap<(i32, i32), Vec<f32>>,
}

impl<'a> Terrain<'a> {
    fn new(
        atlas: &'a axiom_growth::model_planet::PlanetSurfaceAtlas,
        localmap: &'a GameWorldLocalMap,
        seed: u64,
    ) -> Self {
        Self {
            atlas,
            localmap,
            seed,
            cache: HashMap::new(),
        }
    }

    /// Continuous terrain height at any world (x,z), by lazily generating the
    /// owning chunk and bilinearly sampling its 17×17 grid. Chunk-edge seams are
    /// shared, so this is continuous across chunk boundaries (GW-E19).
    fn height(&mut self, x: f32, z: f32) -> f32 {
        let cs = CHUNK_SIZE_CELLS as f32;
        let cx = (x / cs).floor() as i32;
        let cz = (z / cs).floor() as i32;
        self.cache.entry((cx, cz)).or_insert_with(|| {
            gameworld::generate_chunk(
                axiom_growth::ids::ChunkCoord::new(cx, cz),
                self.atlas,
                self.localmap,
                self.seed,
            )
            .height_samples
        });
        let h = &self.cache[&(cx, cz)];
        let fx = x - cx as f32 * cs;
        let fz = z - cz as f32 * cs;
        let x0 = (fx.floor() as usize).min(CHUNK_VERT_SIDE - 1);
        let z0 = (fz.floor() as usize).min(CHUNK_VERT_SIDE - 1);
        let x1 = (x0 + 1).min(CHUNK_VERT_SIDE - 1);
        let z1 = (z0 + 1).min(CHUNK_VERT_SIDE - 1);
        let tx = fx - x0 as f32;
        let tz = fz - z0 as f32;
        let s = |lx: usize, lz: usize| h[lz * CHUNK_VERT_SIDE + lx];
        let top = s(x0, z0) + (s(x1, z0) - s(x0, z0)) * tx;
        let bot = s(x0, z1) + (s(x1, z1) - s(x0, z1)) * tx;
        top + (bot - top) * tz
    }
}

fn render_firstperson(g: &Growth, path: &str) {
    let lm = GameWorldLocalMap::anchored(&g.atlas);
    let mut terr = Terrain::new(&g.atlas, &lm, g.seed.value);

    let (cam_x, cam_z) = (8.0f32, 8.0f32);
    let eye = terr.height(cam_x, cam_z) + 1.7; // human eye height (m)

    // local elevation range for the colour ramp (coarse disc around the camera)
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut gx = -250.0;
    while gx <= 250.0 {
        let mut gz = -250.0;
        while gz <= 250.0 {
            let h = terr.height(cam_x + gx, cam_z + gz);
            lo = lo.min(h);
            hi = hi.max(h);
            gz += 12.0;
        }
        gx += 12.0;
    }
    let span = (hi - lo).max(1.0);

    let w = 1000usize;
    let h_img = 560usize;
    let hfov = 75.0f32.to_radians();
    let yaw = 0.7f32; // look heading (radians)
    let horizon = h_img as f32 * 0.46;
    let proj = h_img as f32 * 1.05;
    let maxd = 900.0f32;
    let sun = normalize3([-0.5, 0.7, 0.45]);
    let sky_top = [70u8, 120, 200];
    let sky_horizon = [185u8, 208, 232];

    let mut rgb = vec![0u8; w * h_img * 3];
    for py in 0..h_img {
        let t = (py as f32 / horizon).clamp(0.0, 1.0);
        let c = [
            lerp(sky_top[0], sky_horizon[0], t),
            lerp(sky_top[1], sky_horizon[1], t),
            lerp(sky_top[2], sky_horizon[2], t),
        ];
        for px in 0..w {
            let i = (py * w + px) * 3;
            rgb[i] = c[0];
            rgb[i + 1] = c[1];
            rgb[i + 2] = c[2];
        }
    }

    let mut ybuf = vec![h_img as i32; w]; // skyline occlusion buffer
    for (col, slot) in ybuf.iter_mut().enumerate() {
        let ang = yaw + ((col as f32 + 0.5) / w as f32 - 0.5) * hfov;
        let (dx, dz) = (ang.sin(), ang.cos());
        let mut d = 1.0f32;
        let mut step = 0.5f32;
        while d < maxd {
            let wx = cam_x + dx * d;
            let wz = cam_z + dz * d;
            let hgt = terr.height(wx, wz);
            let sy = (horizon - ((hgt - eye) / d) * proj) as i32;
            if sy < *slot {
                let n = normalize3([
                    terr.height(wx - 2.0, wz) - terr.height(wx + 2.0, wz),
                    8.0,
                    terr.height(wx, wz - 2.0) - terr.height(wx, wz + 2.0),
                ]);
                let lambert = (n[0] * sun[0] + n[1] * sun[1] + n[2] * sun[2]).clamp(0.3, 1.0);
                let t = ((hgt - lo) / span).clamp(0.0, 1.0);
                let (br, bg, bb) = ramp(t);
                let fog = (d / maxd).powf(0.8).clamp(0.0, 1.0);
                let cr = curves::lerp(br as f32 * lambert, sky_horizon[0] as f32, fog);
                let cg = curves::lerp(bg as f32 * lambert, sky_horizon[1] as f32, fog);
                let cb = curves::lerp(bb as f32 * lambert, sky_horizon[2] as f32, fog);
                let top = sy.max(0);
                let bottom = (*slot).min(h_img as i32);
                for py in top..bottom {
                    let i = (py as usize * w + col) * 3;
                    rgb[i] = cr as u8;
                    rgb[i + 1] = cg as u8;
                    rgb[i + 2] = cb as u8;
                }
                *slot = sy;
            }
            d += step;
            step *= 1.012; // distance LOD
        }
    }
    write_png(path, w, h_img, &rgb);
    eprintln!(
        "wrote {} ({}x{} first-person, local relief span {:.1} m)",
        path, w, h_img, span
    );
}

// Minimal PNG encoder (RGB8, zlib "stored" blocks). No external crates.
fn write_png(path: &str, w: usize, h: usize, rgb: &[u8]) {
    let mut raw = Vec::with_capacity(h * (1 + w * 3));
    for y in 0..h {
        raw.push(0u8); // filter: none
        raw.extend_from_slice(&rgb[y * w * 3..(y + 1) * w * 3]);
    }
    let mut png = Vec::new();
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // bit depth 8, colour type 2 (RGB)
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &zlib_stored(&raw));
    write_chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, &png).expect("write png");
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut i = 0;
    while i < data.len() {
        let block = (data.len() - i).min(65535);
        let last = i + block >= data.len();
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&(block as u16).to_le_bytes());
        out.extend_from_slice(&(!(block as u16)).to_le_bytes());
        out.extend_from_slice(&data[i..i + block]);
        i += block;
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
