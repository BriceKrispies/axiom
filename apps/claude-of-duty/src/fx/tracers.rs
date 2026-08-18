//! Ported from Claude-of-Duty `src/fx/tracers.js:1-91` — the whole file.
//!
//! A tracer is a burning pellet in the base of the round: a short, very
//! bright, velocity-aligned streak that travels. Real muzzle velocity
//! (~900 m/s) crosses a 30 m street in two frames, so the visual speed is
//! clamped into a range that reads on screen while keeping departure and
//! arrival times honest. Three sprites: a hot head, the streak core (HDR,
//! blooms), and a longer, dimmer afterglow behind it.

use crate::fx::particles::reset_spawn;
use crate::fx::system::FxSystem;

const MIN_SPEED: f64 = 55.0;
const MAX_SPEED: f64 = 340.0;

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
    let mut dx = to.0 - from.0;
    let mut dy = to.1 - from.1;
    let mut dz = to.2 - from.2;
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    if dist < 0.35 {
        return;
    }
    dx /= dist;
    dy /= dist;
    dz /= dist;
    let v = speed.max(MIN_SPEED).min(MAX_SPEED);
    // `speed || 260` — the source falls back to 260 for a falsy (zero/NaN)
    // speed, evaluated *before* the min/max clamp above. `Rng`-free and
    // exact, so ported as the equivalent `if speed == 0.0`.
    let v = if speed == 0.0 { 260.0_f64.max(MIN_SPEED).min(MAX_SPEED) } else { v };
    let life = dist / v;
    let ox = from.0 + dx * 0.25;
    let oy = from.1 + dy * 0.25;
    let oz = from.2 + dz * 0.25;

    // core streak
    let mut s = reset_spawn();
    s.x = ox;
    s.y = oy;
    s.z = oz;
    s.vx = dx * v;
    s.vy = dy * v;
    s.vz = dz * v;
    s.tile = crate::fx::atlas::p::STREAK as f64;
    s.size0 = 0.055;
    s.size1 = 0.04;
    s.stretch = 0.26;
    s.life = life;
    s.drag = 0.02;
    s.gravity = -1.2;
    s.r0 = 1.0;
    s.g0 = 0.52 * warm;
    s.b0 = 0.18 * warm;
    s.i0 = 26.0;
    s.r1 = 1.0;
    s.g1 = 0.4 * warm;
    s.b1 = 0.12 * warm;
    s.i1 = 16.0;
    s.alpha_curve = 0.25;
    s.soft = 0.1;
    s.seed = fx.rng.float();
    fx.emit_add(&s);

    // afterglow
    let mut s = reset_spawn();
    s.x = ox;
    s.y = oy;
    s.z = oz;
    s.vx = dx * v;
    s.vy = dy * v;
    s.vz = dz * v;
    s.tile = crate::fx::atlas::p::STREAK as f64;
    s.size0 = 0.09;
    s.size1 = 0.07;
    s.stretch = 0.6;
    s.life = life;
    s.drag = 0.02;
    s.gravity = -1.2;
    s.r0 = 1.0;
    s.g0 = 0.33 * warm;
    s.b0 = 0.1 * warm;
    s.i0 = 5.5;
    s.r1 = 1.0;
    s.g1 = 0.24 * warm;
    s.b1 = 0.06 * warm;
    s.i1 = 2.5;
    s.alpha_curve = 0.3;
    s.soft = 0.14;
    s.seed = fx.rng.float();
    fx.emit_add(&s);

    // incandescent head
    let mut s = reset_spawn();
    s.x = ox;
    s.y = oy;
    s.z = oz;
    s.vx = dx * v;
    s.vy = dy * v;
    s.vz = dz * v;
    s.tile = crate::fx::atlas::p::SPARK as f64;
    s.size0 = 0.05;
    s.size1 = 0.042;
    s.life = life;
    s.drag = 0.02;
    s.gravity = -1.2;
    s.r0 = 1.0;
    s.g0 = 0.85;
    s.b0 = 0.6;
    s.i0 = 30.0;
    s.r1 = 1.0;
    s.g1 = 0.6;
    s.b1 = 0.3;
    s.i1 = 18.0;
    s.alpha_curve = 0.2;
    s.soft = 0.08;
    s.seed = fx.rng.float();
    fx.emit_add(&s);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::system::FxSystem;

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
}
