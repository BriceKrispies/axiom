//! Ported from Claude-of-Duty `src/world/dressing.js:46-149` and
//! `:176-215` — the shared spatial queries every dressing pass is filtered
//! through (`inBuilding`, `isOpen`, `groundY`, `nearestWall`, `camClear`),
//! the contact fillet they all drop (`groundSkirt`), and the fixed-seed
//! jitter stream the whole set-dressing pass runs on (`jitterRig`).

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::{Assembler, Jitter};
use crate::world::kit::{ll, patch_geometry};
use crate::world::clutter::Category;
use crate::world::layout::{road_y, ALLEYS, BUILDINGS, STREET};

// --------------------------------------------------------------- occupancy --
/// `inBuilding(x, z, m = 0.3)` (`dressing.js:49-61`): true inside (or within
/// `m` of) any building footprint.
pub fn in_building(x: f64, z: f64, m: f64) -> bool {
    BUILDINGS
        .iter()
        .any(|b| x > b.x - b.w / 2.0 - m && x < b.x + b.w / 2.0 + m && z > b.z - b.d / 2.0 - m && z < b.z + b.d / 2.0 + m)
}

/// `isOpen(x, z, m = 0.3)` (`dressing.js:64-72`): true on the street, a
/// pavement or an alley — i.e. somewhere props can sit.
pub fn is_open(x: f64, z: f64, m: f64) -> bool {
    if in_building(x, z, m) {
        return false;
    }
    if x.abs() < STREET.kerb - 0.1 && z > STREET.z_min && z < STREET.z_max {
        return true;
    }
    ALLEYS.iter().any(|a| x > a.x0 + m && x < a.x1 - m && z > a.z0 + m && z < a.z1 - m)
}

/// `groundY(x, z)` (`dressing.js:75-81`): ground height for a prop —
/// pavement slabs sit a kerb above the road, and the road itself is
/// cambered.
pub fn ground_y(x: f64, z: f64) -> f64 {
    if x.abs() < STREET.half_width {
        // `roadY(x, 0.004)` (`dressing.js:81`). The road is cambered; props
        // placed at y=0 sink into the crown by 5 cm. Value-identical to the
        // formula this replaced — it is the same numbers, from the one
        // definition (`crate::world::layout::road_y`).
        return road_y(x, 0.004);
    }
    if x.abs() < STREET.kerb && z > STREET.z_min && z < STREET.z_max {
        return STREET.walk_h;
    }
    0.03
}

/// `Math.sign` — three-valued, unlike [`f64::signum`]. `world/dressing.js` calls it
/// directly; the transcription lives once in [`crate::jsmath`], which is
/// pinned bit-for-bit against V8 (including `-0` and `NaN`, which the local
/// copy this replaced flattened to `+0`).
pub(crate) use crate::jsmath::sign as js_sign;

/// `nearestWall(x, z)`'s return shape (`dressing.js:148`).
///
/// **`nx`/`nz` are dead.** `nearestWall`'s only caller
/// (`scatterDebris`'s alley pass) reads `near.d` and nothing else. The
/// outward normal is computed anyway, exactly as the source does — the port
/// recipe's "dead computation in the source is still part of the source".
#[derive(Debug, Clone, Copy)]
pub struct NearestWall {
    pub d: f64,
    pub nx: f64,
    pub nz: f64,
}

/// `nearestWall(x, z)` (`dressing.js:126-149`): distance to the nearest
/// building wall, and the outward normal, in level space.
pub fn nearest_wall(x: f64, z: f64) -> NearestWall {
    let mut best = 1e9;
    let mut nx = 0.0;
    let mut nz = 0.0;
    for b in BUILDINGS {
        let dx = (x - b.x).abs() - b.w / 2.0;
        let dz = (z - b.z).abs() - b.d / 2.0;
        let d = dx.max(dz);
        if d < best {
            best = d;
            if dx > dz {
                nx = js_sign(x - b.x);
                nz = 0.0;
            } else {
                nx = 0.0;
                nz = js_sign(z - b.z);
            }
        }
    }
    NearestWall { d: best, nx, nz }
}

// ---------------------------------------------------------- shot clearance --
/// `SHOT_CLEAR` (`dressing.js:186-194`): where the named shot cameras stand,
/// in LEVEL space. A silhouette breaker dropped on top of a camera turns a
/// hero capture into a close-up of an oil drum, so every mid-ground mass is
/// tested against these.
pub const SHOT_CLEAR: [[f64; 2]; 7] = [
    [0.0, 20.0],    // hero / night / hud
    [1.1, 25.6],    // sunset
    [-3.3, 10.6],   // combat
    [-0.55, 10.0],  // weapon / ads / muzzle
    [-1.25, 4.8],   // impacts
    [-0.11, 4.3],   // detail
    [-8.86, 6.8],   // interior
];

/// `camClear(x, z, r = 1.6)` (`dressing.js:197-204`): true when a prop of
/// radius `r` at `(x, z)` leaves every shot camera clear.
///
/// This is a **squared-distance** test written out as `dx*dx + dz*dz`, not
/// `Math.hypot` — transcribed as written (the port recipe's `Math.hypot`
/// trap runs the other way too: substituting `hypot` here would round
/// differently).
pub fn cam_clear(x: f64, z: f64, r: f64) -> bool {
    !SHOT_CLEAR.iter().any(|c| {
        let dx = x - c[0];
        let dz = z - c[1];
        dx * dx + dz * dz < (r + 1.5) * (r + 1.5)
    })
}

/// `camClear`'s `r` default (`dressing.js:197`).
pub const CAM_CLEAR_DEFAULT_R: f64 = 1.6;

// -------------------------------------------------------------- the jitter --
/// `jitterRig()` (`dressing.js:172-174`): the set-dressing placement jitter
/// — +/-12 deg of yaw, +/-8% of scale, and whatever tilt each prototype
/// declared as loose.
///
/// It runs on its **own fixed-seed stream** (`0x9e3779b1` — note the final
/// `b1`, not the `b9` of `Rng`'s default seed). Drawing the jitter from the
/// placement rng would shift every subsequent position in the level, which
/// walks props into the shot cameras' keepout zones and re-rolls the whole
/// layout on any edit.
pub fn jitter_rig() -> Jitter {
    Jitter { rng: Rng::new(0x9e37_79b1), yaw: 0.209, scale: 0.08 }
}

// ------------------------------------------------------------ ground skirt --
/// `groundSkirt(A, rng, x, y, z, radius, opts = {})`'s `opts`
/// (`dressing.js:92`). Defaults: `key="dirt"`, `grime=0.85`, `ao=0.55`,
/// `pebbles` absent (drawn as `rng.int(4, 8)` — see [`ground_skirt`]).
#[derive(Debug, Clone, Copy)]
pub struct SkirtOpts<'a> {
    pub key: &'a str,
    pub grime: f64,
    pub ao: f64,
    /// `opts.pebbles`. `None` means "not supplied", which is **not** the same
    /// as any number: the source's `opts.pebbles ?? rng.int(4, 8)`
    /// short-circuits, so supplying a count skips that draw entirely.
    pub pebbles: Option<i32>,
}

impl Default for SkirtOpts<'_> {
    fn default() -> Self {
        SkirtOpts { key: "dirt", grime: 0.85, ao: 0.55, pebbles: None }
    }
}

/// The pebble vocabulary (`dressing.js:113`). `rock_b` appears twice: it is
/// a weighted pick, not a typo.
const PEBBLES: [&str; 6] = ["rock_b", "rock_b", "brick_b", "cinder", "rock_a", "litter"];

/// `groundSkirt(A, rng, x, y, z, radius, opts = {})` (`dressing.js:92-124`):
/// a dirt/rubble skirt at the base of a heavy prop.
///
/// Nothing in the real world meets the ground on a clean line: there is a
/// dust halo where it was dragged into place, grit swept up against it, and
/// a few pebbles that got kicked out.
///
/// **Draw order is load-bearing.** JavaScript evaluates call arguments
/// left-to-right, so the `LL(...)` calls below consume `rng.range(0, 0.005)`
/// → `rng.float()` → `rng.range(0.7, 1.0)` in that order, interleaved with
/// the `patchGeometry` draws exactly as sequenced here.
///
/// ## Muted by the arena floor policy
///
/// With the props gone these are stains with no object
/// ([`Category::Skirts`]). **Muted rather than skipped** so every random draw
/// below still happens and nothing downstream of it moves — see
/// [`crate::world::clutter`]. `dressing.js:96-104` splits this into a
/// `groundSkirt` gate and a `_groundSkirt` body for the same reason; here the
/// gate is the first two lines and the body is the rest.
pub fn ground_skirt(asm: &mut Assembler, rng: &mut Rng, x: f64, y: f64, z: f64, radius: f64, opts: SkirtOpts) {
    if asm.clutter.suppresses(Category::Skirts) {
        return asm.muted(|a| ground_skirt_body(a, rng, x, y, z, radius, opts));
    }
    ground_skirt_body(asm, rng, x, y, z, radius, opts);
}

/// `_groundSkirt(A, rng, x, y, z, radius, opts)` (`dressing.js:106-140`).
fn ground_skirt_body(asm: &mut Assembler, rng: &mut Rng, x: f64, y: f64, z: f64, radius: f64, opts: SkirtOpts) {
    let r = radius * rng.range(1.15, 1.55);
    let g = patch_geometry(rng, r, 11, 0.5, 0.0);
    let y0 = y + 0.011 + rng.range(0.0, 0.005);
    let ry0 = rng.float() * 6.28;
    let sz0 = rng.range(0.7, 1.0);
    let m = ll(&Mat4::IDENTITY, x as f32, y0 as f32, z as f32, ry0 as f32, 1.0, 1.0, sz0 as f32, 0.0, 0.0);
    asm.add_once(
        opts.key,
        &g,
        Some(&m),
        Some(AccumAddOpts { masks: Some([0.08, opts.grime as f32, opts.ao as f32]), paint: None }),
    );

    // a second, tighter and darker ring right at the contact line
    let r2 = radius * rng.range(0.75, 1.0);
    let g2 = patch_geometry(rng, r2, 9, 0.35, 0.0);
    let y1 = y + 0.018 + rng.range(0.0, 0.004);
    let ry1 = rng.float() * 6.28;
    let m2 = ll(&Mat4::IDENTITY, x as f32, y1 as f32, z as f32, ry1 as f32, 1.0, 1.0, 0.85, 0.0, 0.0);
    asm.add_once("dirt", &g2, Some(&m2), Some(AccumAddOpts { masks: Some([0.05, 1.0, 0.8]), paint: None }));

    let n = match opts.pebbles {
        Some(v) => v,
        None => rng.int(4, 8),
    };
    for _ in 0..n {
        let a = rng.float() * 6.28;
        let rr = radius * rng.range(0.75, 1.5);
        let px = x + a.cos() * rr;
        let pz = z + a.sin() * rr;
        if !is_open(px, pz, 0.05) {
            continue;
        }
        let id = *rng.pick(&PEBBLES);
        let py = ground_y(px, pz) + 0.012;
        let pry = rng.float() * 6.28;
        let ps = rng.range(0.45, 0.95);
        let grime = rng.range(1.1, 1.5);
        let prx = rng.range(-0.3, 0.3);
        let prz = rng.range(-0.3, 0.3);
        asm.put(id, px as f32, py as f32, pz as f32, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), prx as f32, prz as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_sign_is_three_valued_unlike_signum() {
        assert_eq!(js_sign(0.0), 0.0);
        assert_eq!(js_sign(-0.0), 0.0);
        assert_eq!(js_sign(2.0), 1.0);
        assert_eq!(js_sign(-2.0), -1.0);
        // The trap this exists to avoid.
        assert_eq!(0.0f64.signum(), 1.0);
    }

    #[test]
    fn ground_y_is_the_road_camber_over_the_asphalt() {
        // Crown of the road.
        assert!((ground_y(0.0, 0.0) - 0.059).abs() < 1e-12);
        // Pavement.
        assert_eq!(ground_y(5.5, 0.0), STREET.walk_h);
        // Off the map entirely.
        assert_eq!(ground_y(40.0, 0.0), 0.03);
    }

    #[test]
    fn cam_clear_rejects_a_prop_on_a_shot_camera() {
        assert!(!cam_clear(0.0, 20.0, CAM_CLEAR_DEFAULT_R));
        assert!(cam_clear(0.0, -20.0, CAM_CLEAR_DEFAULT_R));
    }

    #[test]
    fn jitter_rig_uses_the_pinned_seed_not_the_rng_default() {
        let j = jitter_rig();
        assert_eq!(j.rng.state(), Rng::new(0x9e37_79b1).state());
        assert_ne!(j.rng.state(), Rng::default().state());
    }

    #[test]
    fn supplying_a_pebble_count_skips_the_rng_int_draw() {
        let mut asm_a = Assembler::new(Rng::new(1));
        let mut a = Rng::new(5);
        ground_skirt(&mut asm_a, &mut a, 0.0, 0.0, 0.0, 0.4, SkirtOpts { pebbles: Some(0), ..SkirtOpts::default() });

        let mut b = Rng::new(5);
        // The same leading draws, minus the `rng.int(4, 8)` the `??` skips.
        b.range(1.15, 1.55);
        for _ in 0..11 {
            b.float();
        }
        b.range(0.0, 0.005);
        b.float();
        b.range(0.7, 1.0);
        b.range(0.75, 1.0);
        for _ in 0..9 {
            b.float();
        }
        b.range(0.0, 0.004);
        b.float();
        assert_eq!(a.state(), b.state());
    }
}
