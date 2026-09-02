//! Ported from Claude-of-Duty `src/fx/tracers.js:1-91` — the whole file.
//!
//! A tracer is a burning pellet in the base of the round: a short, very
//! bright, velocity-aligned streak that travels. Real muzzle velocity
//! (~900 m/s) crosses a 30 m street in two frames, so the visual speed is
//! clamped into a range that reads on screen while keeping departure and
//! arrival times honest. Three sprites: a hot head, the streak core (HDR,
//! blooms), and a longer, dimmer afterglow behind it.
//!
//! ## The three sprites are data
//!
//! They used to be three near-identical blocks of field assignments — the same
//! twenty lines written out three times with different numbers. `ax shape` reads
//! this file at 0.50 literals per line over a six-word vocabulary, which is the
//! signature of content rather than code, and the three blocks differed in
//! exactly eight values each.
//!
//! They are now [`SPRITES`], a const table, and [`spawn_tracer`] is the driver
//! that supplies what only the call site knows: where the round is, where it is
//! going, how fast, and how warm. Adding a fourth sprite is a row; it used to be
//! twenty more lines of assignment.

use crate::fx::particles::reset_spawn;
use crate::fx::system::FxSystem;

const MIN_SPEED: f64 = 55.0;
const MAX_SPEED: f64 = 340.0;

/// Below this the round has not travelled far enough to be worth drawing.
const MIN_DISTANCE: f64 = 0.35;

/// The source's `speed || 260` — a falsy (zero or NaN) speed falls back before
/// the clamp, not after.
const DEFAULT_SPEED: f64 = 260.0;

/// The muzzle offset: the streak starts a little ahead of the barrel.
const MUZZLE_OFFSET: f64 = 0.25;

/// Shared by all three sprites — they are one physical pellet, so they must
/// travel together.
const DRAG: f64 = 0.02;
const GRAVITY: f64 = -1.2;

/// One sprite of a tracer.
///
/// A colour is `[r, g, b, intensity]`. `warm_tinted` says whether the caller's
/// warmth scales the green and blue channels: the streak and its afterglow take
/// the round's colour temperature, the incandescent head does not — it is hot
/// enough to read as white whatever the propellant is doing.
struct Sprite {
    tile: usize,
    size0: f64,
    size1: f64,
    stretch: f64,
    birth: [f64; 4],
    death: [f64; 4],
    alpha_curve: f64,
    soft: f64,
    warm_tinted: bool,
}

/// The tracer, as three rows. Order is the emission order, which fixes the RNG
/// draw order and therefore every sprite's seed.
const SPRITES: [Sprite; 3] = [
    // The streak core: HDR, blooms.
    Sprite {
        tile: crate::fx::atlas::p::STREAK,
        size0: 0.055,
        size1: 0.04,
        stretch: 0.26,
        birth: [1.0, 0.52, 0.18, 26.0],
        death: [1.0, 0.4, 0.12, 16.0],
        alpha_curve: 0.25,
        soft: 0.1,
        warm_tinted: true,
    },
    // The afterglow: longer, dimmer, trailing.
    Sprite {
        tile: crate::fx::atlas::p::STREAK,
        size0: 0.09,
        size1: 0.07,
        stretch: 0.6,
        birth: [1.0, 0.33, 0.1, 5.5],
        death: [1.0, 0.24, 0.06, 2.5],
        alpha_curve: 0.3,
        soft: 0.14,
        warm_tinted: true,
    },
    // The incandescent head.
    Sprite {
        tile: crate::fx::atlas::p::SPARK,
        size0: 0.05,
        size1: 0.042,
        stretch: 0.0,
        birth: [1.0, 0.85, 0.6, 30.0],
        death: [1.0, 0.6, 0.3, 18.0],
        alpha_curve: 0.2,
        soft: 0.08,
        warm_tinted: false,
    },
];

/// `spawnTracer(fx, from, to, speed, opts)`, `tracers.js:20-91`. `warm`
/// defaults to `1` at the one call site that omits it
/// (`index.js`'s `tracer(from, to, speed)` never passes `opts`).
pub fn spawn_tracer(
    fx: &mut FxSystem,
    from: (f64, f64, f64),
    to: (f64, f64, f64),
    speed: f64,
    warm: f64,
) {
    let (mut dx, mut dy, mut dz) = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    if dist < MIN_DISTANCE {
        return;
    }
    dx /= dist;
    dy /= dist;
    dz /= dist;

    // The fallback is evaluated *before* the clamp, as the source does it.
    let requested = if speed == 0.0 { DEFAULT_SPEED } else { speed };
    let v = requested.max(MIN_SPEED).min(MAX_SPEED);
    let life = dist / v;

    let origin = (
        from.0 + dx * MUZZLE_OFFSET,
        from.1 + dy * MUZZLE_OFFSET,
        from.2 + dz * MUZZLE_OFFSET,
    );

    for sprite in &SPRITES {
        // A sprite that is not warm-tinted takes its colour as authored.
        let tint = if sprite.warm_tinted { warm } else { 1.0 };
        let mut s = reset_spawn();
        s.x = origin.0;
        s.y = origin.1;
        s.z = origin.2;
        s.vx = dx * v;
        s.vy = dy * v;
        s.vz = dz * v;
        s.tile = sprite.tile as f64;
        s.size0 = sprite.size0;
        s.size1 = sprite.size1;
        s.stretch = sprite.stretch;
        s.life = life;
        s.drag = DRAG;
        s.gravity = GRAVITY;
        s.r0 = sprite.birth[0];
        s.g0 = sprite.birth[1] * tint;
        s.b0 = sprite.birth[2] * tint;
        s.i0 = sprite.birth[3];
        s.r1 = sprite.death[0];
        s.g1 = sprite.death[1] * tint;
        s.b1 = sprite.death[2] * tint;
        s.i1 = sprite.death[3];
        s.alpha_curve = sprite.alpha_curve;
        s.soft = sprite.soft;
        s.seed = fx.rng.float();
        fx.emit_add(&s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::system::FxSystem;

    /// The number of fields one particle occupies in the raw buffer.
    const STRIDE: usize = 32;

    #[test]
    fn tracer_shorter_than_the_minimum_distance_spawns_nothing() {
        let mut fx = FxSystem::test_instance(1);
        let before = fx.add.spawned();
        spawn_tracer(&mut fx, (0.0, 0.0, 0.0), (0.1, 0.0, 0.0), 260.0, 1.0);
        assert_eq!(fx.add.spawned(), before);
    }

    #[test]
    fn tracer_spawns_three_additive_particles() {
        let mut fx = FxSystem::test_instance(2);
        let before = fx.add.spawned();
        spawn_tracer(&mut fx, (0.0, 0.0, 0.0), (30.0, 0.0, 0.0), 260.0, 1.0);
        assert_eq!(fx.add.spawned() - before, 3);
    }

    #[test]
    fn tracer_speed_is_clamped_into_the_visual_range() {
        let mut fx = FxSystem::test_instance(3);
        spawn_tracer(&mut fx, (0.0, 0.0, 0.0), (30.0, 0.0, 0.0), 5.0, 1.0);
        let raw = fx.add.raw();
        // vx = dx * v, dx = 1.0 here (slot 0), so vx == the clamped speed.
        assert!((raw[4] as f64 - MIN_SPEED).abs() < 1e-3);
    }

    /// **The conversion's proof.** These are the exact 96 buffer values the
    /// hand-written version emitted for this input, captured before the three
    /// blocks of field assignments became [`SPRITES`]. Every field of every
    /// sprite is in here, including the three `seed` draws — so a change to the
    /// table, the driver, or the emission order shows up as a failure rather
    /// than as a slightly different-looking tracer nobody notices.
    #[test]
    fn the_table_emits_exactly_what_the_hand_written_version_did() {
        let mut fx = FxSystem::test_instance(7);
        spawn_tracer(&mut fx, (1.0, 2.0, 3.0), (31.0, 6.0, 3.0), 260.0, 0.8);
        assert_eq!(fx.add.spawned(), 3);

        #[rustfmt::skip]
        const EXPECTED: [f32; STRIDE * 3] = [
            // the streak core
            1.247807, 2.033041, 3.0, 0.055, 257.71927, 34.362568, 0.0, 0.04,
            0.0, 8.590642, 0.02, -1.2, 0.0, 0.0, 0.26, 1.0,
            1.0, 0.416, 0.144, 26.0, 1.0, 0.32, 0.096, 16.0,
            5.0, 0.1, 1.0, 0.25, 0.0, 1.0, 0.9521306, 0.0,
            // the afterglow
            1.247807, 2.033041, 3.0, 0.09, 257.71927, 34.362568, 0.0, 0.07,
            0.0, 8.590642, 0.02, -1.2, 0.0, 0.0, 0.6, 1.0,
            1.0, 0.264, 0.08, 5.5, 1.0, 0.192, 0.048, 2.5,
            5.0, 0.14, 1.0, 0.3, 0.0, 1.0, 0.39723656, 0.0,
            // the incandescent head, which takes no warm tint
            1.247807, 2.033041, 3.0, 0.05, 257.71927, 34.362568, 0.0, 0.042,
            0.0, 8.590642, 0.02, -1.2, 0.0, 0.0, 0.0, 1.0,
            1.0, 0.85, 0.6, 30.0, 1.0, 0.6, 0.3, 18.0,
            4.0, 0.08, 1.0, 0.2, 0.0, 1.0, 0.63935816, 0.0,
        ];

        let raw = fx.add.raw();
        for (i, want) in EXPECTED.iter().enumerate() {
            assert_eq!(raw[i], *want, "buffer slot {i} (particle {})", i / STRIDE);
        }
    }

    /// The one field the table treats conditionally: warmth tints the streak
    /// and its afterglow, and leaves the incandescent head alone.
    #[test]
    fn warmth_tints_the_streak_but_not_the_head() {
        let mut cold = FxSystem::test_instance(11);
        spawn_tracer(&mut cold, (0.0, 0.0, 0.0), (30.0, 0.0, 0.0), 260.0, 0.5);
        let mut hot = FxSystem::test_instance(11);
        spawn_tracer(&mut hot, (0.0, 0.0, 0.0), (30.0, 0.0, 0.0), 260.0, 1.0);

        // Green-at-birth, per particle. Slot 17 within the stride.
        let green = |raw: &[f32], sprite: usize| raw[sprite * STRIDE + 17];
        assert!(green(cold.add.raw(), 0) < green(hot.add.raw(), 0));
        assert!(green(cold.add.raw(), 1) < green(hot.add.raw(), 1));
        assert_eq!(green(cold.add.raw(), 2), green(hot.add.raw(), 2));
    }

    /// A zero speed falls back before the clamp, not after — the source's
    /// `speed || 260`. A `.max(MIN_SPEED)` applied first would give 55, not 260.
    #[test]
    fn a_zero_speed_falls_back_to_the_default_before_clamping() {
        let mut fx = FxSystem::test_instance(5);
        spawn_tracer(&mut fx, (0.0, 0.0, 0.0), (30.0, 0.0, 0.0), 0.0, 1.0);
        assert!((fx.add.raw()[4] as f64 - DEFAULT_SPEED).abs() < 1e-3);
    }
}
