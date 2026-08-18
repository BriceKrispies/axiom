//! Ported from Claude-of-Duty `src/fx/shells.js:1-245`.
//!
//! Ejected shell casings: a fixed-capacity ring of slots, each either a real
//! rigid body (when physics is up) or, absent that, a crude ballistic
//! arc-plus-tumble fallback so brass still behaves before that subsystem
//! boots.
//!
//! ## What is ported
//!
//! [`ShellSystem::spawn`] and [`ShellSystem::update`]'s **fallback**
//! integration path (`shells.js:230-238`, the `else` branch taken when
//! `slot.body` is absent) — real `+ - * /` arithmetic plus one quaternion
//! composition per step, deterministic and independent of any physics
//! binding. This is the actually-testable half of the source: the rigid-body
//! path hands the casing to `physics.addRigidBody({...})` and reads its pose
//! back every frame, which needs a real rigid-body simulation this port does
//! not have (out of scope for this slice — see `crate::weapons::ballistics`'
//! `RaycastWorld` for the established seam pattern this would extend). The
//! lathed brass-case profile (`caseProfile`, `shells.js:26-40`) and the
//! `THREE.InstancedMesh`/material/texture construction are GPU presentation
//! and are not ported here either.
//!
//! Rotation uses `axiom_math::Quat` (already a dependency of this crate for
//! the weapon geometry port) rather than a hand-rolled quaternion: its
//! [`axiom_math::Quat::from_euler_xyz`] composes `Rz · Ry · Rx` — apply X,
//! then Y, then Z — matching `THREE.Quaternion.setFromEuler` under its
//! default `'XYZ'` order, which is what `shells.js:141, 220` uses.

use axiom_math::Quat;

use crate::rng::Rng;

/// `CAPACITY`, `shells.js:20`.
pub const CAPACITY: usize = 14;
/// `LIFETIME`, `shells.js:21`.
pub const LIFETIME: f64 = 9.0;
/// `FADE`, `shells.js:22`.
pub const FADE: f64 = 0.7;
/// `CASE_LEN` — the modelled 5.56x45 case length in metres, the scale=1
/// reference. `shells.js:24`.
pub const CASE_LEN: f64 = 0.045;

/// One casing slot, `shells.js:87-101`. Only the fallback-integration fields
/// are kept (`pos`/`vel`/`quat`/`spin`/`scale`/`baseScale`/`alive`/`age`);
/// `body`/`proxy` (the rigid-body handle and its `THREE.Object3D` mirror)
/// have no meaning without a physics binding.
#[derive(Debug, Clone, Copy)]
pub struct ShellSlot {
    pub alive: bool,
    pub age: f64,
    pub pos: (f64, f64, f64),
    pub vel: (f64, f64, f64),
    pub quat: Quat,
    /// Angular velocity, radians/second per axis.
    pub spin: (f64, f64, f64),
    pub scale: f64,
    pub base_scale: f64,
}

impl Default for ShellSlot {
    fn default() -> Self {
        ShellSlot {
            alive: false,
            age: 0.0,
            pos: (0.0, 0.0, 0.0),
            vel: (0.0, 0.0, 0.0),
            quat: Quat::IDENTITY,
            spin: (0.0, 0.0, 0.0),
            scale: 1.0,
            base_scale: 1.0,
        }
    }
}

/// `spawn(position, velocity, opts)`'s optional overrides, `shells.js:
/// 103-107` (the `opts` object).
#[derive(Debug, Clone, Copy, Default)]
pub struct ShellSpawnOpts {
    /// `opts.caseLen` — falls back to [`CASE_LEN`] when `<= 0`.
    pub case_len: f64,
    /// `opts.spin` — a magnitude; falls back to a rolled spin when `<= 0`.
    pub spin: f64,
}

/// `class ShellSystem`'s fallback-relevant state, `shells.js:47-101`.
pub struct ShellSystem {
    pub slots: [ShellSlot; CAPACITY],
    cursor: usize,
    /// `buildBrassTextures(fx.rng.fork(), 128)`, `shells.js:58`. The bytes
    /// have no consumer in this port (no GPU upload yet — see the module
    /// doc), but the **draw** matters: it is one `rng.fork()` off the
    /// parent stream, and skipping it would desync every RNG draw after
    /// `ShellSystem::new` from the source's call order. Kept as a public
    /// field so a test can assert the bake itself is deterministic, the same
    /// way the source retains `this.textures`.
    pub brass: crate::fx::atlas::BrassTextures,
}

impl ShellSystem {
    /// `constructor`, `shells.js:47-101` — the ring of slots, minus the
    /// geometry/material/mesh construction (GPU presentation). `rng` is the
    /// **parent** FX stream; this forks it exactly once
    /// (`fx.rng.fork()`, `shells.js:58`), matching the source's draw order.
    pub fn new(rng: &mut Rng) -> Self {
        let brass = crate::fx::atlas::bake_brass_textures(&mut rng.fork(), 128);
        ShellSystem {
            slots: [ShellSlot::default(); CAPACITY],
            cursor: 0,
            brass,
        }
    }

    /// `spawn(position, velocity, opts)`, `shells.js:103-155` — minus the
    /// `physics?.addRigidBody` branch (`shells.js:145-155`; a slot spawned
    /// through this port always takes the fallback integration path in
    /// [`ShellSystem::update`]).
    pub fn spawn(
        &mut self,
        rng: &mut Rng,
        position: (f64, f64, f64),
        velocity: Option<(f64, f64, f64)>,
        opts: ShellSpawnOpts,
    ) -> usize {
        let idx = self.cursor;
        self.cursor = (self.cursor + 1) % CAPACITY;
        let slot = &mut self.slots[idx];
        // `if (slot.alive) this._release(slot);` — the only release work
        // without a physics binding is clearing `alive`, done below anyway.

        slot.alive = true;
        slot.age = 0.0;
        let case_len = if opts.case_len > 0.0 { opts.case_len } else { CASE_LEN };
        slot.base_scale = case_len / CASE_LEN;
        slot.scale = slot.base_scale;
        slot.pos = position;
        let v = velocity.unwrap_or((2.4, 1.6, 0.0));
        slot.vel = v;
        if opts.spin > 0.0 {
            slot.spin = (
                rng.signed() * opts.spin,
                rng.signed() * opts.spin * 0.7,
                rng.signed() * opts.spin,
            );
        } else {
            slot.spin = (rng.range(-38.0, 38.0), rng.range(-26.0, 26.0), rng.range(-38.0, 38.0));
        }
        let e = (rng.float() * 6.28, rng.float() * 6.28, rng.float() * 6.28);
        slot.quat = Quat::from_euler_xyz(e.0 as f32, e.1 as f32, e.2 as f32);
        idx
    }

    /// `update(dt, now)`'s fallback branch, `shells.js:207-238` — the
    /// per-slot ballistic-arc-plus-tumble integration when no rigid body
    /// backs the slot, plus the shared age/fade bookkeeping
    /// (`shells.js:209-215, 224-227`) that applies either way.
    pub fn update(&mut self, dt: f64, gravity: f64) {
        for slot in &mut self.slots {
            if !slot.alive {
                continue;
            }
            slot.age += dt;
            if slot.age > LIFETIME {
                slot.alive = false;
                continue;
            }
            slot.vel.1 += gravity * dt;
            slot.pos = (
                slot.pos.0 + slot.vel.0 * dt,
                slot.pos.1 + slot.vel.1 * dt,
                slot.pos.2 + slot.vel.2 * dt,
            );
            let step = Quat::from_euler_xyz(
                (slot.spin.0 * dt) as f32,
                (slot.spin.1 * dt) as f32,
                (slot.spin.2 * dt) as f32,
            );
            slot.quat = slot.quat.multiply(step);

            let fade_at = LIFETIME - FADE;
            slot.scale = slot.base_scale
                * if slot.age > fade_at {
                    (1.0 - (slot.age - fade_at) / FADE).max(0.0)
                } else {
                    1.0
                };
        }
    }

    /// Count of currently-alive slots — the source's `count` local in
    /// `update()`, used to decide `mesh.visible` (`shells.js:216-223`); kept
    /// here as a plain query since there is no mesh.
    pub fn alive_count(&self) -> usize {
        self.slots.iter().filter(|s| s.alive).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_places_the_ring_cursor_deterministically() {
        let mut rng = Rng::new(1);
        let mut sys = ShellSystem::new(&mut rng);
        let a = sys.spawn(&mut rng, (0.0, 0.0, 0.0), None, ShellSpawnOpts::default());
        let b = sys.spawn(&mut rng, (1.0, 0.0, 0.0), None, ShellSpawnOpts::default());
        assert_eq!(a, 0);
        assert_eq!(b, 1);
        assert!(sys.slots[0].alive);
        assert!(sys.slots[1].alive);
    }

    #[test]
    fn case_len_scales_relative_to_the_5_56_reference() {
        let mut rng = Rng::new(2);
        let mut sys = ShellSystem::new(&mut rng);
        let opts = ShellSpawnOpts {
            case_len: CASE_LEN * 0.42,
            spin: 0.0,
        };
        let idx = sys.spawn(&mut rng, (0.0, 0.0, 0.0), None, opts);
        assert!((sys.slots[idx].base_scale - 0.42).abs() < 1e-9);
    }

    #[test]
    fn ring_wraps_after_capacity_spawns() {
        let mut rng = Rng::new(3);
        let mut sys = ShellSystem::new(&mut rng);
        let mut last = 0;
        for _ in 0..CAPACITY {
            last = sys.spawn(&mut rng, (0.0, 0.0, 0.0), None, ShellSpawnOpts::default());
        }
        assert_eq!(last, CAPACITY - 1);
        let wrapped = sys.spawn(&mut rng, (0.0, 0.0, 0.0), None, ShellSpawnOpts::default());
        assert_eq!(wrapped, 0);
    }

    #[test]
    fn update_applies_gravity_and_expires_past_lifetime() {
        let mut rng = Rng::new(4);
        let mut sys = ShellSystem::new(&mut rng);
        sys.spawn(&mut rng, (0.0, 1.0, 0.0), Some((0.0, 0.0, 0.0)), ShellSpawnOpts::default());
        for _ in 0..10 {
            sys.update(1.0 / 60.0, -19.62);
        }
        assert!(sys.slots[0].vel.1 < 0.0);
        sys.update(LIFETIME + 1.0, -19.62);
        assert!(!sys.slots[0].alive);
    }

    #[test]
    fn scale_fades_out_near_end_of_life() {
        let mut rng = Rng::new(5);
        let mut sys = ShellSystem::new(&mut rng);
        sys.spawn(&mut rng, (0.0, 1.0, 0.0), Some((0.0, 0.0, 0.0)), ShellSpawnOpts::default());
        sys.update(LIFETIME - FADE * 0.5, -19.62);
        assert!(sys.slots[0].alive);
        assert!(sys.slots[0].scale < 1.0);
        assert!(sys.slots[0].scale > 0.0);
    }

    #[test]
    fn alive_count_matches_live_slots() {
        let mut rng = Rng::new(6);
        let mut sys = ShellSystem::new(&mut rng);
        sys.spawn(&mut rng, (0.0, 0.0, 0.0), None, ShellSpawnOpts::default());
        sys.spawn(&mut rng, (0.0, 0.0, 0.0), None, ShellSpawnOpts::default());
        assert_eq!(sys.alive_count(), 2);
    }

    #[test]
    fn brass_bake_is_deterministic() {
        let mut rng_a = Rng::new(7);
        let mut rng_b = Rng::new(7);
        let sys_a = ShellSystem::new(&mut rng_a);
        let sys_b = ShellSystem::new(&mut rng_b);
        assert_eq!(sys_a.brass.normal, sys_b.brass.normal);
        assert_eq!(sys_a.brass.orm, sys_b.brass.orm);
    }
}
