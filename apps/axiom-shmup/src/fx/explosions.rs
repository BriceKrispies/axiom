//! Ported from Claude-of-Duty `src/fx/explosions.js:1-209` — the whole file.
//!
//! Explosions, ordered the way a real detonation is ordered, because the
//! order is the whole effect: a white-hot core gone in 60 ms plus a real
//! light flash, a fireball expanding on a decelerating curve, a shockwave
//! ring that outruns it, a debris cone and ground dust ring thrown radially,
//! and a smoke column that keeps rising long after the fire has gone out.

use crate::fx::atlas::p;
use crate::fx::particles::reset_spawn;
use crate::fx::system::FxSystem;
use crate::fx::util::{cone, disc_on};

const TWO_PI: f64 = std::f64::consts::PI * 2.0;

/// `explode(fx, o)`'s parameters, `explosions.js:20-27` (the `o` object).
pub struct ExplosionOpts {
    pub position: (f64, f64, f64),
    pub radius: f64,
    /// `o.up ?? {x:0,y:1,z:0}`.
    pub up: (f64, f64, f64),
}

impl Default for ExplosionOpts {
    fn default() -> Self {
        ExplosionOpts {
            position: (0.0, 0.0, 0.0),
            radius: 5.0,
            up: (0.0, 1.0, 0.0),
        }
    }
}

/// `explode(fx, o)`, `explosions.js:20-209`.
pub fn explode(fx: &mut FxSystem, o: &ExplosionOpts) {
    let q = fx.pscale;
    let r = o.radius.max(0.6);
    let up = o.up;
    let (px, py, pz) = o.position;

    // ---- core flash ---------------------------------------------------------
    let mut s = reset_spawn();
    s.x = px;
    s.y = py;
    s.z = pz;
    s.tile = p::FLASH_CORE as f64;
    s.size0 = r * 0.35;
    s.size1 = r * 1.5;
    s.size_curve = 0.3;
    s.life = 0.085;
    s.drag = 5.0;
    s.r0 = 1.0;
    s.g0 = 0.95;
    s.b0 = 0.85;
    s.i0 = 85.0;
    s.r1 = 1.0;
    s.g1 = 0.42;
    s.b1 = 0.1;
    s.i1 = 0.0;
    s.alpha_curve = 0.5;
    s.soft = 0.5;
    s.seed = fx.rng.float();
    fx.emit_add(&s);

    // ---- fireball -------------------------------------------------------------
    let n_fire = (12.0 * q).round() as i32 + 5;
    for _ in 0..n_fire {
        let (vx, vy, vz) = cone(&mut fx.rng, up.0, up.1, up.2, 1.5, 0.6);
        let sp = fx.rng.range(1.5, 5.5) * (r / 4.0);
        let (dx, dy, dz) = disc_on(&mut fx.rng, up.0, up.1, up.2, r * 0.16);
        let mut s = reset_spawn();
        s.x = px + dx;
        s.y = py + dy + r * 0.05;
        s.z = pz + dz;
        s.vx = vx * sp;
        s.vy = vy * sp + 1.4;
        s.vz = vz * sp;
        s.tile = p::FIRE as f64;
        s.size0 = r * fx.rng.range(0.18, 0.34);
        s.size1 = r * fx.rng.range(0.7, 1.15);
        s.size_curve = 0.34;
        s.life = fx.rng.range(0.3, 0.62);
        s.delay = fx.rng.range(0.0, 0.06);
        s.drag = fx.rng.range(2.6, 4.2);
        s.gravity = 2.2;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 2.2;
        s.r0 = 1.0;
        s.g0 = fx.rng.range(0.7, 0.92);
        s.b0 = fx.rng.range(0.4, 0.62);
        s.i0 = fx.rng.range(7.0, 17.0);
        s.r1 = 1.0;
        s.g1 = 0.22;
        s.b1 = 0.04;
        s.i1 = 0.3;
        s.alpha_curve = 0.55;
        s.soft = 0.6;
        s.turb = r * 0.05;
        s.turb_freq = 2.4;
        s.seed = fx.rng.float();
        fx.emit_add(&s);
    }

    // dark hot smoke boiling off the fireball immediately
    let n_boil = (9.0 * q).round() as i32 + 4;
    for i in 0..n_boil {
        let (vx, vy, vz) = cone(&mut fx.rng, up.0, up.1, up.2, 1.4, 0.7);
        let sp = fx.rng.range(1.2, 4.0) * (r / 4.0);
        let mut s = reset_spawn();
        s.x = px + vx * r * 0.12;
        s.y = py + r * 0.08;
        s.z = pz + vz * r * 0.12;
        s.vx = vx * sp;
        s.vy = vy * sp + 1.0;
        s.vz = vz * sp;
        s.tile = if i % 2 == 1 { p::SMOKE_A } else { p::SMOKE_B } as f64;
        s.size0 = r * fx.rng.range(0.2, 0.34);
        s.size1 = r * fx.rng.range(0.8, 1.35);
        s.size_curve = 0.5;
        s.life = fx.rng.range(1.1, 2.2);
        s.delay = fx.rng.range(0.03, 0.16);
        s.drag = 1.9;
        s.gravity = 0.9;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 0.9;
        s.r0 = 0.1;
        s.g0 = 0.095;
        s.b0 = 0.09;
        s.r1 = 0.19;
        s.g1 = 0.185;
        s.b1 = 0.18;
        s.alpha = fx.rng.range(0.55, 0.85);
        s.alpha_curve = 1.5;
        s.soft = 0.7;
        s.turb = r * 0.06;
        s.turb_freq = 1.1;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }

    // ---- shockwave --------------------------------------------------------------
    fx.haze_ring(px, py, pz, r * 0.25, 9.0, 0.34, 2.2);
    let mut s = reset_spawn();
    s.x = px;
    s.y = py;
    s.z = pz;
    s.tile = p::RING as f64;
    s.size0 = r * 0.35;
    s.size1 = r * 2.4;
    s.size_curve = 0.42;
    s.life = 0.2;
    s.drag = 6.0;
    s.r0 = 1.0;
    s.g0 = 0.9;
    s.b0 = 0.78;
    s.i0 = 3.2;
    s.r1 = 1.0;
    s.g1 = 0.6;
    s.b1 = 0.3;
    s.i1 = 0.0;
    s.alpha_curve = 1.1;
    s.soft = 1.2;
    s.seed = fx.rng.float();
    fx.emit_add(&s);

    // ---- ground dust ring ---------------------------------------------------
    let n_ring = (11.0 * q).round() as i32 + 5;
    for i in 0..n_ring {
        let a = (f64::from(i) / f64::from(n_ring)) * TWO_PI + fx.rng.range(-0.2, 0.2);
        let dx = a.cos();
        let dz = a.sin();
        let sp = fx.rng.range(4.0, 9.0) * (r / 4.0);
        let mut s = reset_spawn();
        s.x = px + dx * r * 0.2;
        s.y = py - r * 0.05;
        s.z = pz + dz * r * 0.2;
        s.vx = dx * sp;
        s.vy = fx.rng.range(0.3, 1.4);
        s.vz = dz * sp;
        s.tile = if i % 3 == 0 { p::SMOKE_B } else { p::DUST } as f64;
        s.size0 = r * fx.rng.range(0.12, 0.22);
        s.size1 = r * fx.rng.range(0.55, 0.95);
        s.size_curve = 0.45;
        s.life = fx.rng.range(0.9, 1.8);
        s.drag = fx.rng.range(2.4, 3.6);
        s.gravity = -0.5;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 1.0;
        s.r0 = 0.42;
        s.g0 = 0.36;
        s.b0 = 0.29;
        s.r1 = 0.34;
        s.g1 = 0.3;
        s.b1 = 0.25;
        s.alpha = fx.rng.range(0.4, 0.7);
        s.alpha_curve = 1.5;
        s.soft = 0.5;
        s.turb = 0.1;
        s.turb_freq = 1.4;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }

    // ---- debris cone ----------------------------------------------------------
    let n_deb = (26.0 * q).round() as i32 + 10;
    for _ in 0..n_deb {
        let (vx, vy, vz) = cone(&mut fx.rng, up.0, up.1, up.2, 1.35, 0.8);
        let sp = fx.rng.range(6.0, 20.0) * (0.6 + r / 8.0);
        let mut s = reset_spawn();
        s.x = px;
        s.y = py + 0.05;
        s.z = pz;
        s.vx = vx * sp;
        s.vy = vy * sp;
        s.vz = vz * sp;
        s.tile = if fx.rng.float() < 0.3 { p::SPLINTER } else { p::CHIP } as f64;
        s.size0 = fx.rng.range(0.01, 0.05);
        s.size1 = s.size0;
        s.life = fx.rng.range(0.7, 1.8);
        s.drag = 0.4;
        s.gravity = -19.0;
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 26.0;
        s.r0 = 0.24;
        s.g0 = 0.21;
        s.b0 = 0.18;
        s.r1 = 0.2;
        s.g1 = 0.18;
        s.b1 = 0.16;
        s.alpha_curve = 0.3;
        s.soft = 0.06;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);
    }
    // embers riding the debris
    let n_ember = (14.0 * q).round() as i32 + 5;
    for _ in 0..n_ember {
        let (vx, vy, vz) = cone(&mut fx.rng, up.0, up.1, up.2, 1.4, 0.9);
        let sp = fx.rng.range(4.0, 14.0);
        let mut s = reset_spawn();
        s.x = px;
        s.y = py + 0.05;
        s.z = pz;
        s.vx = vx * sp;
        s.vy = vy * sp;
        s.vz = vz * sp;
        s.tile = p::STREAK as f64;
        s.size0 = fx.rng.range(0.012, 0.03);
        s.size1 = s.size0 * 0.4;
        s.stretch = 1.1;
        s.life = fx.rng.range(0.5, 1.4);
        s.drag = 1.1;
        s.gravity = -13.0;
        s.r0 = 1.0;
        s.g0 = 0.6;
        s.b0 = 0.22;
        s.i0 = fx.rng.range(8.0, 20.0);
        s.r1 = 1.0;
        s.g1 = 0.18;
        s.b1 = 0.03;
        s.i1 = 0.2;
        s.flags = 1.0;
        s.alpha_curve = 0.6;
        s.soft = 0.06;
        s.seed = fx.rng.float();
        fx.emit_add(&s);
    }

    // ---- lingering smoke column -----------------------------------------------
    fx.add_smoke_column(
        px,
        py + r * 0.1,
        pz,
        crate::fx::system::SmokeColumnOpts {
            radius: r * 0.35,
            duration: 1.5,
            rate: 9.0,
            rise: 1.6,
            dark: 0.12,
            life: 3.4,
            growth: 3.2,
        },
    );

    // ---- light + ground scorch -------------------------------------------------
    fx.lights.flash(px, py + r * 0.15, pz, 1.0, 0.72, 0.4, 420.0 * (r / 4.0), 0.45, 8.0, r * 8.0, 4.0);
    fx.scorch(px, py, pz, r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::system::FxSystem;

    #[test]
    fn explode_spawns_additive_and_lit_particles_and_a_light() {
        let mut fx = FxSystem::test_instance(1);
        let before_add = fx.add.spawned();
        let before_lit = fx.lit.spawned();
        explode(&mut fx, &ExplosionOpts::default());
        assert!(fx.add.spawned() > before_add);
        assert!(fx.lit.spawned() > before_lit);
        assert!(fx.lights.slots.iter().any(|s| s.intensity > 0.0 || s.priority > 0.0));
    }

    #[test]
    fn radius_is_floored_at_the_minimum() {
        let mut fx = FxSystem::test_instance(2);
        let opts = ExplosionOpts {
            radius: 0.0,
            ..ExplosionOpts::default()
        };
        // must not panic and must still spawn a core flash sized off `r.max(0.6)`.
        explode(&mut fx, &opts);
        assert!(fx.add.spawned() > 0);
    }
}
