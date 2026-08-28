//! **Renderer validation scene** — ported from Claude-of-Duty
//! `src/render/probe.js` (306 lines, the whole file).
//!
//! `RenderProbeScene` exists so the render subsystem can be developed and
//! screenshotted before the world subsystem lands: the renderer adds it on
//! frame 0 if the scene holds no meshes, and deletes it the moment six foreign
//! meshes appear. Its own header says it plainly — *"Nothing here is shipped
//! content."*
//!
//! # Why this is app-tier and not a module
//!
//! An earlier brief in this port listed `probe.js` as a *light probe* and
//! scheduled it into `gpu-backend`. It is neither, and it could not live there
//! if it were: the scene is driven end to end by the app's own xoshiro128\*\*
//! [`Rng`](crate::rng::Rng), and a module may not depend on an app. It is
//! generated content, and content is the app's.
//!
//! # What this produces, and what it leaves to the caller
//!
//! The source builds `THREE.Mesh`es into a `THREE.Group`. This produces the two
//! things that decide what those meshes look like — the procedural
//! [`make_surface`] maps and the [`ProbeScene`] placements — and leaves the
//! scene-graph assembly to the caller, where Axiom's own vocabulary lives. Same
//! split the rest of this port uses.
//!
//! # Determinism: this file is draw order
//!
//! Every value comes off one `Rng`, and the two placement loops consume it
//! differently in a way a natural transcription gets wrong:
//!
//! * **A rejected block draws four values, not five.** `continue` fires
//!   *before* the yaw draw, so rejecting a block leaves the stream one draw
//!   further back than accepting one.
//! * **A rejected crate draws all five.** Its `continue` fires *after* every
//!   draw, so rejection costs the stream nothing.
//!
//! The two rejections are thirty lines apart and behave oppositely. Get either
//! wrong and every subsequent value in the level diverges.

use crate::jsmath;
use crate::rng::Rng;

/// Octaves in [`fbm`] — `o < 5`.
const FBM_OCTAVES: usize = 5;

/// The per-octave frequency step. **2.07, not 2.0**: a lacunarity that is not a
/// power of two keeps successive octaves from lining their grids up, which is
/// what makes plain fBm look blocky.
const FBM_LACUNARITY: f64 = 2.07;

/// Value noise over a permutation table, five octaves — `fbm`
/// (`probe.js:12-36`).
///
/// The bilinear blend is written the source's way,
/// `a + (b - a) * tx + (c - a) * ty + (a - b - c + d) * tx * ty`, and **not** as
/// a pair of `mix`es. The two are algebraically the same and numerically are
/// not; the source's grouping is the specification.
pub fn fbm(x: f64, y: f64, perm: &[u8; 256]) -> f64 {
    // `perm[ ( perm[ i & 255 ] + ( j & 255 ) ) & 255 ] / 255`. The mask matters
    // because `ix` goes NEGATIVE wherever the sampled coordinate does, and in
    // both languages `-1 & 255` is `255` — a wrap, not a clamp.
    let hash = |i: i32, j: i32| -> f64 {
        let first = usize::try_from(i & 255).expect("masked to 0..=255");
        let mixed = i32::from(perm[first]) + (j & 255);
        let second = usize::try_from(mixed & 255).expect("masked to 0..=255");
        f64::from(perm[second]) / 255.0
    };
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);

    let mut amp = 0.5;
    let mut freq = 1.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for _ in 0..FBM_OCTAVES {
        let fx = x * freq;
        let fy = y * freq;
        let fix = fx.floor();
        let fiy = fy.floor();
        let tx = smooth(fx - fix);
        let ty = smooth(fy - fiy);
        // `Math.floor` yields a float and `&` applies ToInt32 to it. The probe's
        // coordinates never approach 2^31, so a saturating cast and a wrapping
        // one agree here; the mask inside `hash` is what bounds the index.
        let ix = fix as i32;
        let iy = fiy as i32;
        let a = hash(ix, iy);
        let b = hash(ix + 1, iy);
        let c = hash(ix, iy + 1);
        let d = hash(ix + 1, iy + 1);
        let v = a + (b - a) * tx + (c - a) * ty + (a - b - c + d) * tx * ty;
        sum += v * amp;
        norm += amp;
        amp *= 0.5;
        freq *= FBM_LACUNARITY;
    }
    sum / norm
}

/// `makeSurface`'s `opts` bag (`probe.js:38`), with the source's `??` defaults
/// resolved into real values at construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceOpts {
    /// Linear base colour.
    pub base: [f64; 3],
    /// `opts.scale ?? 6` — how many noise periods across the tile.
    pub scale: f64,
    /// `opts.rough ?? 0.75`.
    pub rough: f64,
    /// `opts.roughVar ?? 0.3`.
    pub rough_var: f64,
    /// `opts.bump ?? 2.4`.
    pub bump: f64,
    /// `opts.variation ?? 0.22`.
    pub variation: f64,
    /// `opts.cracks` — subtract a ridged crack network from the height.
    pub cracks: bool,
}

impl Default for SurfaceOpts {
    /// Every `??` default from `probe.js:47-93`, in one place.
    fn default() -> Self {
        SurfaceOpts {
            base: [0.5, 0.5, 0.5],
            scale: 6.0,
            rough: 0.75,
            rough_var: 0.3,
            bump: 2.4,
            variation: 0.22,
            cracks: false,
        }
    }
}

/// One baked surface: three RGBA8 maps, `size` square.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeMaps {
    pub size: u32,
    /// Base colour, consumed as sRGB.
    pub albedo: Vec<u8>,
    /// Tangent-space normal.
    pub normal: Vec<u8>,
    /// Occlusion in R, roughness in **G**, metalness in B — one texture read
    /// twice, which is how three's `roughnessMap`/`metalnessMap` pair works.
    pub orm: Vec<u8>,
}

/// `makeSurface( rng, size, opts )` (`probe.js:38-113`).
///
/// **Spends 256 RNG draws before anything else**, filling the permutation
/// table — so two surfaces baked off one stream are different noise, and the
/// order they are baked in is part of the level.
///
/// The height field is a `Float32Array` in the source, so it is `f32` here. The
/// noise filling it is computed in `f64` (JavaScript has no other number) and
/// narrowed on store, and the normal pass differences the **narrowed** values.
/// Carrying the chain in `f64` throughout would produce a different normal map.
pub fn make_surface(rng: &mut Rng, size: u32, opts: SurfaceOpts) -> ProbeMaps {
    let mut perm = [0_u8; 256];
    perm.iter_mut().for_each(|slot| {
        *slot = u8::try_from(rng.int(0, 255)).expect("rng.int(0, 255) is in range");
    });

    let n = size as usize;
    let mut height = vec![0.0_f32; n * n];
    (0..n).for_each(|y| {
        (0..n).for_each(|x| {
            let u = (x as f64 / f64::from(size)) * opts.scale;
            let v = (y as f64 / f64::from(size)) * opts.scale;
            let coarse = fbm(u, v, &perm);
            let fine = fbm(u * 7.3, v * 7.3, &perm);
            let h = coarse * 0.75 + fine * 0.25;
            // A ridged crack network: `abs(fbm - 0.5) * 2` is zero along the
            // noise's own contour lines, and the sixth power of its inverse
            // turns those lines into narrow, deep gouges.
            let cracked = {
                let c = (fbm(u * 1.7 + 5.5, v * 1.7 - 2.2, &perm) - 0.5).abs() * 2.0;
                h - (1.0 - f64::min(1.0, c * 3.2)).powf(6.0) * 0.55
            };
            // The crack term is evaluated either way. That costs three fBm
            // evaluations on a surface that does not want them, and it is what
            // the source does — but note it draws NO rng, so an unconditional
            // evaluation cannot shift the stream.
            height[y * n + x] = [h, cracked][usize::from(opts.cracks)] as f32;
        });
    });

    let mut albedo = vec![0_u8; n * n * 4];
    let mut normal = vec![0_u8; n * n * 4];
    let mut orm = vec![0_u8; n * n * 4];
    (0..n).for_each(|y| {
        (0..n).for_each(|x| {
            let i = y * n + x;
            let h = f64::from(height[i]);

            // Grime pools where the height is low and the tint lifts with it, so
            // a crevice reads both darker and flatter than a ridge.
            let grime = (1.0 - h).powf(2.2);
            let tint = 1.0 - opts.variation * 0.5 + h * opts.variation;
            let rgb = [
                opts.base[0] * tint * (1.0 - grime * 0.35),
                opts.base[1] * tint * (1.0 - grime * 0.38),
                opts.base[2] * tint * (1.0 - grime * 0.42),
            ];
            // `Uint8Array` assignment truncates toward zero after the source's
            // own `Math.min(255, …)`; a saturating cast does the same for the
            // non-negative values this produces.
            (0..3).for_each(|c| albedo[i * 4 + c] = f64::min(255.0, rgb[c] * 255.0) as u8);
            albedo[i * 4 + 3] = 255;

            let rv = opts.rough + (h - 0.5) * opts.rough_var;
            orm[i * 4] = 255;
            orm[i * 4 + 1] = f64::max(0.0, f64::min(255.0, rv * 255.0)) as u8;
            orm[i * 4 + 2] = 0;
            orm[i * 4 + 3] = 255;

            // Central differences, WRAPPED — the map tiles, so the gradient at
            // the last column reads the first.
            let at = |xx: usize, yy: usize| f64::from(height[yy * n + xx]);
            let xp = at((x + 1) % n, y);
            let xm = at((x + n - 1) % n, y);
            let yp = at(x, (y + 1) % n);
            let ym = at(x, (y + n - 1) % n);
            let st = opts.bump * f64::from(size) * 0.004;
            let nx = (xm - xp) * st;
            let ny = (ym - yp) * st;
            // Three-argument `Math.hypot`: V8's max-scaled compensated sum, not
            // the plain root of a sum of squares.
            let len = jsmath::hypot(&[nx, ny, 1.0]);
            normal[i * 4] = (((nx / len) * 0.5 + 0.5) * 255.0) as u8;
            normal[i * 4 + 1] = (((ny / len) * 0.5 + 0.5) * 255.0) as u8;
            // **127, not 127.5.** The x and y lanes encode as
            // `(v * 0.5 + 0.5) * 255`, centred on 127.5; z is written
            // `(1 / len) * 0.5 * 255 + 127` and centres one half-step lower.
            // The asymmetry is the source's. Tidying it would move every z in
            // every normal map this bakes, which is exactly the kind of
            // invisible change a port must not make.
            normal[i * 4 + 2] = ((1.0 / len) * 0.5 * 255.0 + 127.0) as u8;
            normal[i * 4 + 3] = 255;
        });
    });

    ProbeMaps {
        size,
        albedo,
        normal,
        orm,
    }
}

/// XZ positions the blockout must never enclose — the named camera setups in
/// `src/dev/shots.js` (`probe.js:116-128`).
///
/// Without this the random street can drop a block or a crate exactly where a
/// shot camera stands, and the capture comes back as a featureless wall filling
/// the frame — which reads as a broken renderer rather than as a bad seed.
pub const SHOT_KEEPOUT: [[f64; 2]; 7] = [
    [12.0, 18.0],  // hero / night / hud
    [-8.5, 3.2],   // interior
    [3.2, 5.0],    // detail
    [16.0, 22.0],  // sunset
    [6.0, 10.0],   // weapon / ads / muzzle
    [4.0, 12.0],   // combat
    [2.5, 6.0],    // impacts
];

/// True when the axis-aligned footprint (centre plus half extents) clears every
/// shot camera — `footprintClear` (`probe.js:131-140`).
pub fn footprint_clear(px: f64, pz: f64, hx: f64, hz: f64, margin: f64) -> bool {
    let ex = hx + margin;
    let ez = hz + margin;
    SHOT_KEEPOUT
        .iter()
        .all(|k| !(((k[0] - px).abs() < ex) & ((k[1] - pz).abs() < ez)))
}

/// One placed box — a street block or a crate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxPlacement {
    /// Centre, world space. `y` is already half the height, as the source sets it.
    pub position: [f64; 3],
    /// Full extents, not half.
    pub scale: [f64; 3],
    /// Rotation about Y, radians.
    pub yaw: f64,
}

/// One of the four metal spheres — `probe.js:263-277`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpherePlacement {
    pub position: [f64; 3],
    /// `0.06 + i * 0.14`, so the row walks mirror-chrome to near-diffuse and an
    /// SSR or IBL pass can be judged across the whole roughness range at once.
    pub roughness: f64,
}

/// One emissive lamp and its point light — `probe.js:280-296`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LampPlacement {
    pub position: [f64; 3],
}

/// The whole blockout, as placements.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeScene {
    pub blocks: Vec<BoxPlacement>,
    pub crates: Vec<BoxPlacement>,
    pub spheres: Vec<SpherePlacement>,
    pub lamps: Vec<LampPlacement>,
}

/// The ground plane's edge — `PlaneGeometry(120, 120)`.
pub const GROUND_SIZE: f64 = 120.0;

/// The ground's uv repeat. The street blocks use 3.
pub const GROUND_REPEAT: f64 = 18.0;

/// Sphere radius — `SphereGeometry(0.55, 48, 32)`.
pub const SPHERE_RADIUS: f64 = 0.55;

/// `emissiveIntensity` on the lamps. Forty, so there is something genuinely
/// beyond 1.0 in the frame for a bloom threshold to find; a lamp at 1.0 would
/// make bloom look like it worked on any threshold at all.
pub const LAMP_EMISSIVE_INTENSITY: f64 = 40.0;

/// `RenderProbeScene.build()`'s placement pass (`probe.js:209-296`).
///
/// Call **after** the three [`make_surface`] bakes, on the same `Rng`: the
/// source builds concrete, then asphalt, then rust metal, and each spends 256
/// draws before this sees the stream.
pub fn build_scene(rng: &mut Rng) -> ProbeScene {
    let mut blocks = Vec::new();
    for i in 0..14 {
        let side = [1.0_f64, -1.0][usize::from(i % 2 == 0)];
        let w = rng.range(4.0, 8.0);
        let h = rng.range(4.0, 11.0);
        let d = rng.range(5.0, 9.0);
        let z = -22.0 + f64::from(i) * 3.4 + rng.range(-1.0, 1.0);
        let mut x = side * rng.range(9.0, 13.0);
        // Push the block outward until it stops swallowing a shot camera. Four
        // tries, no rng: the street is wide enough that it always resolves, and
        // a fifth try would change nothing but the stream.
        let mut tries = 0;
        while tries < 4 && !footprint_clear(x, z, w / 2.0, d / 2.0, 1.5) {
            x += side * 2.5;
            tries += 1;
        }
        // **The rejection is here, BEFORE the yaw draw.** A rejected block
        // therefore costs four draws where an accepted one costs five. The
        // crate loop below rejects the other way round.
        footprint_clear(x, z, w / 2.0, d / 2.0, 1.5).then(|| {
            let yaw = rng.range(-0.05, 0.05);
            blocks.push(BoxPlacement {
                position: [x, h / 2.0, z],
                scale: [w, h, d],
                yaw,
            });
        });
    }

    let mut crates = Vec::new();
    for _ in 0..22 {
        let s = rng.range(0.4, 1.1);
        let sy = s * rng.range(0.7, 1.2);
        let cx = rng.range(-8.0, 8.0);
        let cz = rng.range(-14.0, 14.0);
        let yaw = rng.range(0.0, std::f64::consts::PI * 2.0);
        // **Every draw has already happened.** A rejected crate costs the stream
        // exactly what an accepted one does — the opposite of the block loop.
        // `0.71` is the source's own half-diagonal for a unit box rotated
        // arbitrarily; it is not `s / 2`.
        footprint_clear(cx, cz, s * 0.71, s * 0.71, 0.9).then(|| {
            crates.push(BoxPlacement {
                position: [cx, sy / 2.0, cz],
                scale: [s, sy, s],
                yaw,
            });
        });
    }

    // Neither the spheres nor the lamps draw from the stream: their positions
    // and roughnesses are a fixed ramp, so the renderer under test always gets
    // the same reflective row to be judged against.
    let spheres = (0..4)
        .map(|i| SpherePlacement {
            position: [-3.0 + f64::from(i) * 2.0, SPHERE_RADIUS, 3.0],
            roughness: 0.06 + f64::from(i) * 0.14,
        })
        .collect();
    let lamps = (0..3)
        .map(|i| LampPlacement {
            position: [-9.5, 4.2, -10.0 + f64::from(i) * 9.0],
        })
        .collect();

    ProbeScene {
        blocks,
        crates,
        spheres,
        lamps,
    }
}
