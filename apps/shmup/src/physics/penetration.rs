//! Ported from Claude-of-Duty `src/physics/penetration.js:1-229`.
use crate::physics::bvh::StaticWorld;
use crate::physics::math::HitRecord;
use crate::physics::surfaces::{self, mask};
use crate::rng::Rng;
use crate::world::palette::Surface;

/// Maximum number of material layers a round can penetrate before stopping.
/// `penetration.js:21`.
const MAX_LAYERS: usize = 6;
/// How far past an entry point we look for the exit face, in metres.
/// `penetration.js:23`.
const EXIT_PROBE: f64 = 1.6;
/// Assumed thickness of single-sided geometry, metres.
/// `penetration.js:25`.
const SHEET_THICKNESS: f64 = 0.018;

/// A single impact record — entry or exit.
/// Mirrors the objects pushed into `this.impacts` in the source (`penetration.js:33-43`).
#[derive(Debug, Clone, Copy)]
pub struct Impact {
    pub point: [f64; 3],
    pub normal: [f64; 3],
    pub surface: Surface,
    pub exit: bool,
    pub damage: f64,
    pub distance: f64,
    pub object: i32,
}

impl Default for Impact {
    fn default() -> Self {
        Impact {
            point: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            surface: Surface::Concrete,
            exit: false,
            damage: 0.0,
            distance: 0.0,
            object: -1,
        }
    }
}

/// Result of a thickness probe.
/// Mirrors `this._thick` in the source (`penetration.js:181-186`).
#[derive(Debug, Clone, Copy, Default)]
struct Thickness {
    distance: f64,
    point: [f64; 3],
    normal: [f64; 3],
    backface: bool,
}

/// Multi-layer bullet penetration solver.
///
/// Ported from the `Ballistics` class in `penetration.js:27-229`.
/// The source's `phys` parameter is a `PhysicsSystem` instance; this port
/// accepts an `Rc<StaticWorld>` directly, since the static world is the only
/// collision backend ported so far. Dynamic bodies (actors, ragdolls, rigid
/// bodies) are not yet present in the BVH, so their callbacks are no-ops here.
pub struct Ballistics {
    world: std::rc::Rc<StaticWorld>,
    rng: Option<std::rc::Rc<std::cell::RefCell<Rng>>>,
    impacts: [Impact; MAX_LAYERS * 2 + 2],
    impact_count: usize,
}

impl Ballistics {
    /// Create a new penetration solver for the given static world.
    pub fn new(world: std::rc::Rc<StaticWorld>) -> Self {
        Ballistics {
            world,
            rng: None,
            impacts: [Impact::default(); MAX_LAYERS * 2 + 2],
            impact_count: 0,
        }
    }

    /// Attach an RNG for deflection sampling. The source stores `this.rng`
    /// on the instance and also accepts `o.rng` per-call; this port does both.
    pub fn set_rng(&mut self, rng: std::rc::Rc<std::cell::RefCell<Rng>>) {
        self.rng = Some(rng);
    }

    /// Get the impacts recorded by the last `fire` call.
    /// Valid until the next `fire()`.
    pub fn impacts(&self) -> &[Impact] {
        &self.impacts[..self.impact_count]
    }

    /// Fire a bullet and trace its penetration through up to `MAX_LAYERS` materials.
    ///
    /// Port of `Ballistics.fire` (`penetration.js:58-169`).
    ///
    /// # Parameters
    /// - `origin`: Ray origin [x, y, z]
    /// - `dir`: Ray direction [x, y, z] (will be normalised)
    /// - `max_dist`: Maximum trace distance in metres (default 400)
    /// - `damage`: Base damage at muzzle (default 34)
    /// - `penetration`: Penetration power, 1.0 = 7.62 rifle, 0.35 = pistol, 2.2 = .50/AP (default 1.0)
    /// - `mask`: Collision mask (default `mask::BULLET`)
    /// - `dropoff`: Damage retained at max_dist, 0..1 (default 0.55)
    /// - `rng`: Optional per-call RNG override for deflection
    /// - `emit`: Whether to emit impact events (default true; no-op in this port)
    ///
    /// # Returns
    /// Number of impacts written into `self.impacts`.
    #[allow(clippy::too_many_arguments)]
    pub fn fire(
        &mut self,
        origin: [f64; 3],
        dir: [f64; 3],
        max_dist: f64,
        damage: f64,
        penetration: f64,
        mask: u16,
        dropoff: f64,
        rng: Option<std::rc::Rc<std::cell::RefCell<Rng>>>,
        _emit: bool,
    ) -> usize {
        let (mut ox, mut oy, mut oz) = (origin[0], origin[1], origin[2]);
        let (mut dx, mut dy, mut dz) = (dir[0], dir[1], dir[2]);

        // Normalize direction (source: `dl = Math.hypot(dx, dy, dz) || 1`)
        let dl = (dx * dx + dy * dy + dz * dz).sqrt();
        if dl > 0.0 {
            dx /= dl;
            dy /= dl;
            dz /= dl;
        }

        let mut remaining = max_dist;
        let mut damage = damage;
        let mut power = penetration;
        let start = origin;
        let rng = rng.or_else(|| self.rng.clone());

        self.impact_count = 0;

        for _layer in 0..MAX_LAYERS {
            if remaining <= 0.01 {
                break;
            }

            // Raycast for the next hit
            let hit = self.world.raycast(ox, oy, oz, dx, dy, dz, remaining, mask, -1);
            if !hit.hit {
                break;
            }

            // Distance travelled from muzzle to this hit
            let travelled = ((hit.px - start[0]).powi(2)
                + (hit.py - start[1]).powi(2)
                + (hit.pz - start[2]).powi(2))
                .sqrt();

            // Muzzle-to-target energy loss (quadratic falloff)
            let range01 = (travelled / max_dist).min(1.0);
            let range_mul = 1.0 - (1.0 - dropoff) * range01 * range01;

            let si = hit.surface as usize;
            let props = surfaces::SURFACE_PROPS.get(si).copied().unwrap_or(surfaces::SURFACE_PROPS[0]);

            // Entry impact
            self.push_impact(
                [hit.px, hit.py, hit.pz],
                [hit.nx, hit.ny, hit.nz],
                hit.surface,
                false,
                damage * range_mul,
                travelled,
                hit.object,
            );

            // Dynamic body callbacks — not ported (no rigid bodies / ragdolls in BVH yet)
            // The source does:
            // if (hit.collider && hit.collider.onHit) { hit.collider.onHit(...) }
            // if (hit.body) { hit.body.applyImpulse(...) }
            // if (hit.ragdoll) { hit.ragdoll.applyImpulse(...) }

            // ---- Can we get through? ----
            let budget = props.pen_depth * power;
            if budget <= 1e-4 {
                break;
            }

            // Measure material thickness via backface probe
            let thick = self.measure_thickness(&hit, dx, dy, dz, mask, EXIT_PROBE.min(remaining));
            if thick.distance > budget {
                break; // round stops in the material
            }

            let frac = thick.distance / budget;

            // Exit impact — normal points out of the far face
            let ex_damage = damage * range_mul * (1.0 - props.energy_loss * frac).max(0.05);
            self.push_impact(
                thick.point,
                thick.normal,
                hit.surface,
                true,
                ex_damage,
                travelled + thick.distance,
                hit.object,
            );

            // ---- Degrade and continue ----
            damage *= (1.0 - props.energy_loss * frac).max(0.05);
            power *= (1.0 - frac).max(0.0);
            if power < 0.02 || damage < 1.0 {
                break;
            }

            // Yaw deflection on exit
            if let Some(rng_rc) = &rng {
                if props.deflect > 0.0 {
                    let spread = props.deflect * frac;
                    // Build an orthonormal frame around the current direction
                    let (mut ux, mut uy, uz) = (0.0, 1.0, 0.0);
                    if dy.abs() > 0.9 {
                        ux = 1.0;
                        uy = 0.0;
                    }
                    // r = u x d
                    let mut rx = uy * dz - uz * dy;
                    let mut ry = uz * dx - ux * dz;
                    let mut rz = ux * dy - uy * dx;
                    let rl = (rx * rx + ry * ry + rz * rz).sqrt();
                    if rl > 0.0 {
                        rx /= rl;
                        ry /= rl;
                        rz /= rl;
                    }
                    // s = d x r
                    let sx = dy * rz - dz * ry;
                    let sy = dz * rx - dx * rz;
                    let sz = dx * ry - dy * rx;

                    let mut rng_borrow = rng_rc.borrow_mut();
                    let a = rng_borrow.gauss() * spread;
                    let b = rng_borrow.gauss() * spread;

                    dx += rx * a + sx * b;
                    dy += ry * a + sy * b;
                    dz += rz * a + sz * b;

                    let nl = (dx * dx + dy * dy + dz * dz).sqrt();
                    if nl > 0.0 {
                        dx /= nl;
                        dy /= nl;
                        dz /= nl;
                    }
                }
            }

            // Step past the exit face and keep flying
            const EPS: f64 = 0.004;
            ox = thick.point[0] + dx * EPS;
            oy = thick.point[1] + dy * EPS;
            oz = thick.point[2] + dz * EPS;

            // Decrement by *this segment's* travel, not the total from the muzzle
            remaining -= hit.t + thick.distance + EPS;
        }

        self.impact_count
    }

    /// Measure distance from an entry hit to where the round leaves the material.
    /// Port of `Ballistics._measureThickness` (`penetration.js:175-216`).
    fn measure_thickness(
        &self,
        entry: &HitRecord,
        dx: f64,
        dy: f64,
        dz: f64,
        mask: u16,
        probe: f64,
    ) -> Thickness {
        const EPS: f64 = 0.0015;
        let ox = entry.px + dx * EPS;
        let oy = entry.py + dy * EPS;
        let oz = entry.pz + dz * EPS;

        let h = self.world.raycast(ox, oy, oz, dx, dy, dz, probe, mask, entry.object);

        let same_solid = h.hit && !h.front_face && h.object == entry.object;

        let mut out = Thickness::default();
        if same_solid {
            // Backface hit on the same object — measured thickness
            out.distance = h.t + EPS;
            out.point = [h.px, h.py, h.pz];
            // raycast() reports normals facing the shooter; the exit face's
            // outward normal is the opposite
            out.normal = [-h.nx, -h.ny, -h.nz];
            out.backface = true;
        } else {
            // Single-sided sheet: nominal thickness along the incidence angle
            let cos = (entry.nx * dx + entry.ny * dy + entry.nz * dz).abs();
            let t = SHEET_THICKNESS / cos.max(0.2);
            out.distance = t;
            out.point = [
                entry.px + dx * t,
                entry.py + dy * t,
                entry.pz + dz * t,
            ];
            out.normal = [-entry.nx, -entry.ny, -entry.nz];
            out.backface = false;
        }
        out
    }

    /// Push an impact into the reusable array.
    /// Port of `Ballistics._push` (`penetration.js:218-228`).
    fn push_impact(
        &mut self,
        point: [f64; 3],
        normal: [f64; 3],
        surface: u8,
        exit: bool,
        damage: f64,
        distance: f64,
        object: i32,
    ) {
        if self.impact_count >= self.impacts.len() {
            return;
        }
        let r = &mut self.impacts[self.impact_count];
        self.impact_count += 1;
        r.point = point;
        r.normal = normal;
        r.surface = Surface::from_index(surface);
        r.exit = exit;
        r.damage = damage;
        r.distance = distance;
        r.object = object;
    }
}

#[cfg(test)]
mod tests {

    // Three hand-written cases were removed here. They asserted impact counts
    // (1, 12, 1) that the ORIGINAL `penetration.js` does not produce for the
    // same setups — two of them fired a ray *away* from the geometry, so the
    // real answer is zero. They were expectations about what the code ought to
    // do, never checked against the source. `tests/penetration_port.rs` covers
    // the same scenarios against captures taken from the original instead.

    use super::*;
    use crate::physics::bvh::StaticWorld;
    use crate::physics::surfaces::layer;
    use crate::world::palette::Surface;
    use std::rc::Rc;

    /// Build a simple test world: a floor at y=0 and a wall at x=2.
    fn test_world() -> Rc<StaticWorld> {
        let mut world = StaticWorld::new();
        // Floor: two triangles at y=0, spanning [-10,10] in x and z
        let floor = vec![
            -10.0, 0.0, 10.0, 10.0, 0.0, 10.0, 10.0, 0.0, -10.0,
            -10.0, 0.0, 10.0, 10.0, 0.0, -10.0, -10.0, 0.0, -10.0,
        ];
        world.add_triangles(&floor, 2, Surface::Concrete, layer::STATIC, "floor");
        // Wall: two triangles at x=2, spanning y=[0,3], z=[-10,10], facing -X
        let wall = vec![
            2.0, 0.0, 10.0, 2.0, 3.0, 10.0, 2.0, 3.0, -10.0,
            2.0, 0.0, 10.0, 2.0, 3.0, -10.0, 2.0, 0.0, -10.0,
        ];
        world.add_triangles(&wall, 2, Surface::Metal, layer::STATIC, "wall");
        world.build();
        Rc::new(world)
    }

    
    
    #[test]
    fn bullet_exhausts_six_layer_cap() {
        // Build a world with many thin layers
        let mut world = StaticWorld::new();
        for i in 0..10 {
            let y = i as f64 * 0.5;
            let layer = vec![
                -1.0, y, 1.0, 1.0, y, 1.0, 1.0, y, -1.0,
                -1.0, y, 1.0, 1.0, y, -1.0, -1.0, y, -1.0,
            ];
            world.add_triangles(&layer, 2, Surface::Plaster, layer::STATIC, &format!("layer{}", i));
        }
        world.build();
        let world = Rc::new(world);

        let mut bal = Ballistics::new(world);

        // Plaster pen_depth = 0.7, power = 2.2 (.50 AP) -> budget = 1.54 per layer
        // Single-sided, cos=1 -> thickness = 0.018, easily penetrates
        // Should hit 6-layer cap
        let count = bal.fire(
            [0.0, 10.0, 0.0],
            [0.0, -1.0, 0.0],
            20.0,
            34.0,
            2.2,
            mask::BULLET,
            0.55,
            None,
            false,
        );

        // Max impacts = 2 per layer * 6 layers = 12 (entry + exit each)
        assert_eq!(count, 12);
        let impacts = bal.impacts();
        assert_eq!(impacts.len(), 12);
        // Alternating entry/exit
        for i in 0..12 {
            assert_eq!(impacts[i].exit, i % 2 == 1);
        }
    }

    
    #[test]
    fn damage_bleed_curve_matches_source() {
        // This test will be superseded by the golden capture test
        // but we keep a basic sanity check here
        let world = test_world();
        let mut bal = Ballistics::new(world);

        bal.fire(
            [2.0, 5.0, 0.0],
            [0.0, -1.0, 0.0],
            20.0,
            34.0,
            1.0,
            mask::BULLET,
            0.55,
            None,
            false,
        );

        let impacts = bal.impacts();
        // Entry damage should be base * range_mul (range_mul < 1)
        assert!(impacts[0].damage < 34.0);
        // Exit damage should be lower due to energy loss
        if impacts.len() > 1 && impacts[1].exit {
            assert!(impacts[1].damage < impacts[0].damage);
        }
    }
}