//! Ported from Claude-of-Duty `src/physics/character.js:1-490` — the whole
//! file.
//!
//! Swept-capsule character controller — collide and slide.
//!
//! The controller is kinematic: `player` (or `ai`) sets a desired displacement
//! each fixed step and we resolve it against the static BVH. Nothing here
//! integrates forces; velocity is owned by the caller and only *clipped* by us,
//! so the movement state machine keeps full authority over feel.
//!
//! Resolution per [`CharacterController::move_by`]:
//!
//!   1. depenetrate  — push out of anything we are already inside
//!   2. lift         — grounded moves raise the capsule by `step_height` first,
//!                     so a stair tread is invisible to the horizontal sweep
//!   3. slide        — up to N swept sweeps, clipping the remaining motion
//!                     against every plane we touch (Quake-style plane stack)
//!   4. drop         — come back down by the lift plus gravity plus the stair
//!                     descent snap, refusing to cling to unwalkable faces
//!   5. ground probe — publish grounded / normal / surface for this frame
//!
//! The sweep is a true continuous test (see
//! [`StaticWorld::sweep_capsule`](crate::physics::bvh::StaticWorld::sweep_capsule)),
//! so there is no tunnelling regardless of speed.
//!
//! ## This is the player module's physics seam, bound
//!
//! `crate::player::movement::CharacterController` and
//! `crate::player::mantle::LedgeCharacter` were written as narrow traits
//! naming exactly the duck-typed calls `movement.js`/`mantle.js` make on
//! `physics.createCharacter()`'s return value. This file *is* that return
//! value, so it implements both at the bottom — the seam closes here and
//! nowhere else.
//!
//! ## Shared world
//!
//! The source holds `this.world` as a plain reference to the one live
//! `StaticWorld`. Every query the controller makes (`sweepCapsule`,
//! `overlapCapsule`) is read-only, and the world is immutable once built, so
//! the port holds an [`Rc<StaticWorld>`] — shared with
//! [`crate::physics::probe::PhysicsWorld`], which serves the *other* half of
//! the same seam (the free-standing `raycast`/`capsuleCast`/`checkCapsule`
//! queries `mantle.js` and `fx` make).

use std::rc::Rc;

use crate::physics::bvh::StaticWorld;
use crate::physics::math::HitRecord;
use crate::physics::surfaces::{mask, SURFACE_PROPS};
use crate::player::mantle::LedgeCharacter;
use crate::player::movement::CharacterController as MovementCharacter;
use crate::world::palette::Surface;

/// `MAX_PLANES`. `character.js:26`.
pub const MAX_PLANES: usize = 5;

/// `SKIN`. `character.js:27`.
pub const SKIN: f64 = 0.008;

/// `constructor(world, opts)`'s option defaults. `character.js:34-41`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterOpts {
    pub radius: f64,
    /// Total capsule height, feet to crown.
    pub height: f64,
    pub step_height: f64,
    pub slope_limit: f64,
    pub snap_distance: f64,
    pub mask: u16,
    pub max_iterations: u32,
}

impl Default for CharacterOpts {
    fn default() -> Self {
        CharacterOpts {
            radius: 0.32,
            height: 1.78,
            step_height: 0.42,
            slope_limit: 50.0 * (std::f64::consts::PI / 180.0),
            snap_distance: 0.32,
            mask: mask::CHARACTER,
            max_iterations: 5,
        }
    }
}

/// `class CharacterController`. `character.js:29-490`.
///
/// The source's `id`/`owner` fields carry no behaviour (they exist so
/// `physics.removeCharacter` and the debug overlay can name a controller); no
/// registry is ported, so they are dropped rather than carried inert.
pub struct Character {
    world: Rc<StaticWorld>,

    pub radius: f64,
    pub height: f64,
    pub step_height: f64,
    pub slope_limit: f64,
    pub snap_distance: f64,
    pub mask: u16,
    pub max_iterations: u32,

    /// Feet position (bottom of the capsule), the authoritative transform.
    pub position: [f64; 3],
    /// Velocity is owned by the caller; `move_by` clips it against contacts.
    pub velocity: [f64; 3],

    pub grounded: bool,
    pub was_grounded: bool,
    pub ground_normal: [f64; 3],
    pub ground_surface: u8,
    pub ground_distance: f64,
    pub ground_object: i32,
    pub on_steep_slope: bool,
    pub touching_ceiling: bool,
    pub touching_wall: bool,
    pub wall_normal: [f64; 3],
    pub last_move_blocked: bool,
    pub stepped_up: f64,
    /// Impact speed along the ground normal on the frame we landed.
    pub landing_speed: f64,
    pub enabled: bool,

    /// `this._hit2` — the record `_sweepMove`/`_sweepDown` write, read back by
    /// `move()`'s cliff-face test (`character.js:167`). The source's other
    /// scratch records carry nothing across a call and are locals here.
    hit2: HitRecord,
}

impl Character {
    /// `new CharacterController(world, opts)`. `character.js:30-71`.
    pub fn new(world: Rc<StaticWorld>, opts: CharacterOpts) -> Self {
        Character {
            world,
            radius: opts.radius,
            height: opts.height,
            step_height: opts.step_height,
            slope_limit: opts.slope_limit,
            snap_distance: opts.snap_distance,
            mask: opts.mask,
            max_iterations: opts.max_iterations,
            position: [0.0, 0.0, 0.0],
            velocity: [0.0, 0.0, 0.0],
            grounded: false,
            was_grounded: false,
            ground_normal: [0.0, 1.0, 0.0],
            ground_surface: 0,
            ground_distance: 0.0,
            ground_object: -1,
            on_steep_slope: false,
            touching_ceiling: false,
            touching_wall: false,
            wall_normal: [0.0, 0.0, 0.0],
            last_move_blocked: false,
            stepped_up: 0.0,
            landing_speed: 0.0,
            enabled: true,
            hit2: HitRecord::default(),
        }
    }

    /// `get cosSlope()`. `character.js:73-75`.
    pub fn cos_slope(&self) -> f64 {
        self.slope_limit.cos()
    }

    /// `get p0y()` — lower sphere centre. `character.js:78-80`.
    pub fn p0y(&self) -> f64 {
        self.position[1] + self.radius
    }

    /// `get p1y()` — upper sphere centre. `character.js:82-84`.
    pub fn p1y(&self) -> f64 {
        self.position[1] + self.height - self.radius
    }

    /// `setPosition(x, y, z)`. `character.js:86-90`.
    pub fn set_position(&mut self, x: f64, y: f64, z: f64) {
        self.position = [x, y, z];
    }

    /// `teleport(x, y, z)`. `character.js:93-100`.
    pub fn teleport(&mut self, x: f64, y: f64, z: f64) {
        self.set_position(x, y, z);
        self.velocity = [0.0, 0.0, 0.0];
        self.grounded = false;
        self.touching_ceiling = false;
        self.touching_wall = false;
        self.depenetrate(8);
        self.probe_ground();
    }

    /// `setHeight(h, force)`. `character.js:106-110`. `false` means standing up
    /// is blocked by a ceiling and the caller stays crouched.
    pub fn set_height(&mut self, h: f64, force: bool) -> bool {
        if h > self.height && !force && !self.can_fit(h) {
            return false;
        }
        self.height = h;
        true
    }

    /// `canFit(h)`. `character.js:113-124`.
    pub fn can_fit(&self, h: f64) -> bool {
        let r = self.radius;
        let p0y = self.position[1] + r;
        let p1y = self.position[1] + h - r;
        if p1y < p0y {
            return true;
        }
        self.world
            .overlap_capsule(
                self.position[0],
                p0y,
                self.position[2],
                self.position[0],
                p1y,
                self.position[2],
                r - 0.01,
                self.mask,
                0.0,
            )
            .count()
            == 0
    }

    /// `move(dx, dy, dz)`. `character.js:137-180`. Returns the distance
    /// actually travelled.
    pub fn move_by(&mut self, dx: f64, dy: f64, dz: f64) -> f64 {
        if !self.enabled {
            return 0.0;
        }
        let st = self.position;
        self.was_grounded = self.grounded;
        self.touching_ceiling = false;
        self.touching_wall = false;
        self.last_move_blocked = false;
        self.stepped_up = 0.0;

        self.depenetrate(4);

        let jumping = dy > 1e-6;
        let use_step_offset = self.was_grounded
            && !jumping
            && self.step_height > 1e-4
            && (dx * dx + dz * dz) > 1e-10;

        if !use_step_offset {
            self.slide(dx, dy, dz);
        } else {
            // 1. lift — a low ceiling shortens the lift automatically
            let lift = self.sweep_move(0.0, self.step_height, 0.0);
            // 2. horizontal
            self.slide(dx, 0.0, dz);
            // 3. drop back down, plus this step's gravity, plus the
            //    stair-descent snap
            let want = lift + (-dy).max(0.0);
            let snap = self.snap_distance;
            let y_before = self.position[1];
            let dropped = self.sweep_down(want + snap, 1.0);
            if dropped < 0.0 {
                // Nothing under us: fall exactly what was asked, no more.
                self.position[1] = y_before - want;
            } else if dropped > want && self.hit2.ny < self.cos_slope() {
                // The only thing within snap range is a cliff face — don't
                // cling to it.
                self.position[1] = y_before - want;
            }
            let gained = self.position[1] - st[1];
            if gained > 1e-4 {
                self.stepped_up = gained;
            }
        }

        self.depenetrate(3);
        self.probe_ground();

        if self.grounded && !self.was_grounded {
            self.landing_speed = -self.velocity[1].min(0.0);
        }

        let d = [
            self.position[0] - st[0],
            self.position[1] - st[1],
            self.position[2] - st[2],
        ];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }

    /// `_slide(dx, dy, dz)`. `character.js:183-285`. Collide-and-slide core;
    /// returns true if any plane stopped us.
    fn slide(&mut self, mut dx: f64, mut dy: f64, mut dz: f64) -> bool {
        let mut planes = [[0.0f64; 3]; MAX_PLANES];
        let mut plane_count = 0usize;
        let mut blocked = false;

        for _iter in 0..self.max_iterations {
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            if dist < 1e-6 {
                break;
            }
            let inv = 1.0 / dist;
            let (ux, uy, uz) = (dx * inv, dy * inv, dz * inv);

            let r = self.radius;
            let hit = self.world.sweep_capsule(
                self.position[0],
                self.p0y(),
                self.position[2],
                self.position[0],
                self.p1y(),
                self.position[2],
                r,
                ux,
                uy,
                uz,
                dist + SKIN,
                self.mask,
            );

            if !hit.hit {
                self.position[0] += dx;
                self.position[1] += dy;
                self.position[2] += dz;
                break;
            }

            blocked = true;
            let advance = (hit.t - SKIN).min(dist).max(0.0);
            self.position[0] += ux * advance;
            self.position[1] += uy * advance;
            self.position[2] += uz * advance;

            // remaining motion
            let rem = dist - advance;
            dx = ux * rem;
            dy = uy * rem;
            dz = uz * rem;

            let (nx, ny, nz) = (hit.nx, hit.ny, hit.nz);
            self.classify_contact(nx, ny, nz, &hit);
            // Note: steep contacts keep their vertical component on purpose.
            // Zeroing it (the usual "don't ramp up cliffs" hack) turns every
            // stair nose into a wall, because the bottom hemisphere always
            // meets a step edge at a shallow angle. Unwalkable surfaces are
            // handled where they should be — probe_ground reports
            // `grounded == false`, so the caller keeps applying gravity and the
            // character slides straight back down.

            if plane_count >= MAX_PLANES {
                break;
            }
            planes[plane_count] = [nx, ny, nz];
            plane_count += 1;

            // Clip against every plane collected so far; if a single-plane
            // projection still violates another plane, slide along the crease.
            let (mut cx, mut cy, mut cz) = (dx, dy, dz);
            let mut resolved = false;
            for i in 0..plane_count {
                if resolved {
                    break;
                }
                let [px, py, pz] = planes[i];
                if dx * px + dy * py + dz * pz >= 0.0 {
                    continue;
                }
                let into = dx * px + dy * py + dz * pz;
                let (tx, ty, tz) = (dx - px * into, dy - py * into, dz - pz * into);
                let mut violates: i32 = -1;
                for j in 0..plane_count {
                    if j == i {
                        continue;
                    }
                    let [qx, qy, qz] = planes[j];
                    if tx * qx + ty * qy + tz * qz < 0.0 {
                        violates = j as i32;
                        break;
                    }
                }
                if violates < 0 {
                    cx = tx;
                    cy = ty;
                    cz = tz;
                    resolved = true;
                } else {
                    // crease: travel along the intersection of the two planes
                    let [qx, qy, qz] = planes[violates as usize];
                    let mut ex = py * qz - pz * qy;
                    let mut ey = pz * qx - px * qz;
                    let mut ez = px * qy - py * qx;
                    let el = (ex * ex + ey * ey + ez * ez).sqrt();
                    if el < 1e-6 {
                        cx = 0.0;
                        cy = 0.0;
                        cz = 0.0;
                        break;
                    }
                    ex /= el;
                    ey /= el;
                    ez /= el;
                    let along = dx * ex + dy * ey + dz * ez;
                    cx = ex * along;
                    cy = ey * along;
                    cz = ez * along;
                    // Reject if the crease direction is blocked by a third plane.
                    let mut bad = false;
                    for j in 0..plane_count {
                        let [rx, ry, rz] = planes[j];
                        if cx * rx + cy * ry + cz * rz < -1e-6 {
                            bad = true;
                            break;
                        }
                    }
                    if bad {
                        cx = 0.0;
                        cy = 0.0;
                        cz = 0.0;
                    }
                    resolved = true;
                }
            }
            dx = cx;
            dy = cy;
            dz = cz;

            // Clip the caller's velocity the same way so accumulated speed
            // doesn't survive a wall impact.
            self.clip_velocity(nx, ny, nz);

            if dx * dx + dy * dy + dz * dz < 1e-12 {
                break;
            }
        }
        self.last_move_blocked = blocked;
        blocked
    }

    /// `_classifyContact(nx, ny, nz, hit)`. `character.js:287-301`.
    fn classify_contact(&mut self, nx: f64, ny: f64, nz: f64, hit: &HitRecord) {
        if ny >= self.cos_slope() {
            self.grounded = true;
            self.ground_normal = [nx, ny, nz];
            self.ground_surface = hit.surface;
            self.ground_object = hit.object;
            self.on_steep_slope = false;
        } else if ny < -0.5 {
            self.touching_ceiling = true;
        } else {
            self.touching_wall = true;
            self.wall_normal = [nx, ny, nz];
            if ny > 0.05 {
                self.on_steep_slope = true;
            }
        }
    }

    /// `_clipVelocity(nx, ny, nz)`. `character.js:303-311`.
    fn clip_velocity(&mut self, nx: f64, ny: f64, nz: f64) {
        let v = self.velocity;
        let into = v[0] * nx + v[1] * ny + v[2] * nz;
        if into < 0.0 {
            self.velocity = [v[0] - nx * into, v[1] - ny * into, v[2] - nz * into];
        }
    }

    /// `_sweepMove(dx, dy, dz)`. `character.js:314-329`. Single swept
    /// translation with no sliding; returns the distance travelled.
    fn sweep_move(&mut self, dx: f64, dy: f64, dz: f64) -> f64 {
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist < 1e-7 {
            return 0.0;
        }
        let inv = 1.0 / dist;
        let (ux, uy, uz) = (dx * inv, dy * inv, dz * inv);
        self.hit2 = self.world.sweep_capsule(
            self.position[0],
            self.p0y(),
            self.position[2],
            self.position[0],
            self.p1y(),
            self.position[2],
            self.radius,
            ux,
            uy,
            uz,
            dist + SKIN,
            self.mask,
        );
        let adv = if self.hit2.hit {
            (self.hit2.t - SKIN).min(dist).max(0.0)
        } else {
            dist
        };
        self.position[0] += ux * adv;
        self.position[1] += uy * adv;
        self.position[2] += uz * adv;
        adv
    }

    /// `_sweepDown(dist, radiusScale)`. `character.js:339-353`. Returns the drop
    /// distance, or `-1` if nothing was hit (the capsule is left where it
    /// started).
    fn sweep_down(&mut self, dist: f64, radius_scale: f64) -> f64 {
        let r = self.radius * radius_scale;
        self.hit2 = self.world.sweep_capsule(
            self.position[0],
            self.position[1] + r,
            self.position[2],
            self.position[0],
            self.position[1] + self.height - r,
            self.position[2],
            r,
            0.0,
            -1.0,
            0.0,
            dist + SKIN,
            self.mask,
        );
        if !self.hit2.hit {
            return -1.0;
        }
        let adv = (self.hit2.t - SKIN).min(dist).max(0.0);
        self.position[1] -= adv;
        adv
    }

    /// `depenetrate(iterations)`. `character.js:356-395`.
    pub fn depenetrate(&mut self, iterations: u32) -> f64 {
        let mut moved = 0.0;
        for _it in 0..iterations {
            let c = self.world.overlap_capsule(
                self.position[0],
                self.p0y(),
                self.position[2],
                self.position[0],
                self.p1y(),
                self.position[2],
                self.radius,
                self.mask,
                0.0,
            );
            let n = c.count();
            if n == 0 {
                break;
            }
            // Accumulate the maximum push along each distinct normal rather
            // than the sum — summing over a tessellated wall ejects the capsule
            // across the map.
            let (mut px, mut py, mut pz) = (0.0f64, 0.0f64, 0.0f64);
            for i in 0..n {
                let d = f64::from(c.depth[i]);
                if d <= 1e-5 {
                    continue;
                }
                let (nx, ny, nz) = (
                    f64::from(c.nx[i]),
                    f64::from(c.ny[i]),
                    f64::from(c.nz[i]),
                );
                let already = px * nx + py * ny + pz * nz;
                let extra = d - already;
                if extra > 0.0 {
                    px += nx * extra;
                    py += ny * extra;
                    pz += nz * extra;
                }
            }
            let l = (px * px + py * py + pz * pz).sqrt();
            if l < 1e-5 {
                break;
            }
            // Damp so a bad contact set can never fling the character.
            let max_push = 0.25;
            let s = if l > max_push { max_push / l } else { 1.0 };
            self.position[0] += px * s;
            self.position[1] += py * s;
            self.position[2] += pz * s;
            moved += l * s;
            if l < 1e-4 {
                break;
            }
        }
        moved
    }

    /// `probeGround()`. `character.js:406-465`. Two traces on purpose — see the
    /// source's comment, reproduced at the call sites below.
    pub fn probe_ground(&mut self) -> bool {
        let probe = 0.06;
        let cos = self.cos_slope();

        // The thin trace (60 % radius) finds the floor while ignoring convex
        // edges — without it a character riding up a stair nose is reported
        // airborne, because the nose is the nearest thing below and its normal
        // is steeper than the slope limit.
        let mut hit = self.world.sweep_capsule(
            self.position[0],
            self.position[1] + self.radius * 0.6,
            self.position[2],
            self.position[0],
            self.position[1] + self.height - self.radius * 0.6,
            self.position[2],
            self.radius * 0.6,
            0.0,
            -1.0,
            0.0,
            probe,
            self.mask,
        );

        let mut found = hit.hit && hit.ny >= cos;
        if !found {
            // The wide trace is the fallback for standing on a narrow beam,
            // where the thin trace would miss entirely.
            hit = self.world.sweep_capsule(
                self.position[0],
                self.p0y(),
                self.position[2],
                self.position[0],
                self.p1y(),
                self.position[2],
                self.radius * 0.98,
                0.0,
                -1.0,
                0.0,
                probe,
                self.mask,
            );
            // A surface with any meaningful upward component supports us even
            // if it is too steep to be "walkable" — that is what a stair nose
            // is.
            found = hit.hit && hit.ny > 0.15;
        }

        if found {
            self.grounded = true;
            self.ground_normal = [hit.nx, hit.ny, hit.nz];
            self.ground_surface = hit.surface;
            self.ground_object = hit.object;
            self.ground_distance = hit.t;
            self.on_steep_slope = hit.ny < cos;
        } else {
            self.grounded = false;
            self.ground_distance = if hit.hit { hit.t } else { f64::INFINITY };
            self.on_steep_slope = hit.hit && hit.ny > 0.05 && hit.ny < cos;
            if hit.hit {
                self.ground_normal = [hit.nx, hit.ny, hit.nz];
                self.ground_surface = hit.surface;
            }
        }

        // Ceiling probe — the movement machine needs this to cancel a jump.
        let ch = self.world.sweep_capsule(
            self.position[0],
            self.p0y(),
            self.position[2],
            self.position[0],
            self.p1y(),
            self.position[2],
            self.radius * 0.98,
            0.0,
            1.0,
            0.0,
            0.06,
            self.mask,
        );
        self.touching_ceiling = ch.hit && ch.ny < -0.4;

        self.grounded
    }

    /// `get groundFriction()`. `character.js:468-470`.
    pub fn ground_friction(&self) -> f64 {
        SURFACE_PROPS
            .get(self.ground_surface as usize)
            .map_or(0.9, |p| p.friction)
    }

    /// `get groundSurfaceName()`. `character.js:472-474`. Typed, not a string —
    /// [`Surface`] is this port's surface vocabulary everywhere.
    pub fn ground_surface(&self) -> Surface {
        Surface::from_index(self.ground_surface)
    }

    /// `checkCapsule(x, y, z, height)`. `character.js:480-489`.
    pub fn check_capsule(&self, x: f64, y: f64, z: f64, height: f64) -> bool {
        self.world
            .overlap_capsule(
                x,
                y + self.radius,
                z,
                x,
                y + height - self.radius,
                z,
                self.radius - 0.005,
                self.mask,
                0.0,
            )
            .count()
            == 0
    }
}

/// The three facts `mantle.js`'s `LedgeProbe` reads off `c`.
impl LedgeCharacter for Character {
    fn position(&self) -> [f64; 3] {
        self.position
    }

    fn radius(&self) -> f64 {
        self.radius
    }

    fn check_capsule(&self, x: f64, y: f64, z: f64, height: f64) -> bool {
        Character::check_capsule(self, x, y, z, height)
    }
}

/// Everything `movement.js` reads and writes on `this.character`. This is the
/// seam `crate::player::movement` was written against, bound to the real swept
/// controller.
impl MovementCharacter for Character {
    fn height(&self) -> f64 {
        self.height
    }

    fn set_height(&mut self, h: f64) {
        // `movement.js` calls `c.setHeight(h)` with no `force`, and ignores the
        // returned "blocked" flag (it has already asked `canFit` itself where
        // it cares) — so the seam's `-> ()` shape is the source's own use.
        Character::set_height(self, h, false);
    }

    fn step_height(&self) -> f64 {
        self.step_height
    }

    fn set_step_height(&mut self, h: f64) {
        self.step_height = h;
    }

    fn grounded(&self) -> bool {
        self.grounded
    }

    fn set_grounded(&mut self, g: bool) {
        self.grounded = g;
    }

    fn velocity(&self) -> [f64; 3] {
        self.velocity
    }

    fn set_velocity(&mut self, v: [f64; 3]) {
        self.velocity = v;
    }

    fn can_fit(&self, height: f64) -> bool {
        Character::can_fit(self, height)
    }

    fn last_move_blocked(&self) -> bool {
        self.last_move_blocked
    }

    fn touching_ceiling(&self) -> bool {
        self.touching_ceiling
    }

    fn ground_normal(&self) -> [f64; 3] {
        self.ground_normal
    }

    fn ground_friction(&self) -> f64 {
        Character::ground_friction(self)
    }

    fn ground_surface(&self) -> Surface {
        Character::ground_surface(self)
    }

    fn landing_speed(&self) -> f64 {
        self.landing_speed
    }

    fn move_by(&mut self, dx: f64, dy: f64, dz: f64) -> f64 {
        Character::move_by(self, dx, dy, dz)
    }

    fn teleport_to(&mut self, x: f64, y: f64, z: f64) {
        self.teleport(x, y, z);
    }

    fn set_position(&mut self, x: f64, y: f64, z: f64) {
        Character::set_position(self, x, y, z);
    }

    fn depenetrate(&mut self, iterations: u32) {
        Character::depenetrate(self, iterations);
    }

    fn probe_ground(&mut self) {
        Character::probe_ground(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::surfaces::layer;

    /// A horizontal quad wound so its geometric normal points **up**. The
    /// winding matters: `overlap_capsule` falls back to the stored face normal
    /// for a deep contact, so a floor wound the other way de-penetrates
    /// downward.
    fn floor_quad(x0: f64, y: f64, z0: f64, x1: f64, z1: f64) -> Vec<f64> {
        vec![
            x0, y, z1, x1, y, z1, x1, y, z0, //
            x0, y, z1, x1, y, z0, x0, y, z0,
        ]
    }

    /// The same quad wound the other way, so its normal points **down** — a
    /// ceiling.
    fn ceiling_quad(x0: f64, y: f64, z0: f64, x1: f64, z1: f64) -> Vec<f64> {
        vec![
            x0, y, z0, x1, y, z0, x1, y, z1, //
            x0, y, z0, x1, y, z1, x0, y, z1,
        ]
    }

    /// A single big floor quad at `y = 0`, plus (optionally) a step of height
    /// `h` occupying `x > 0`.
    fn ground_world(step_height: Option<f64>) -> Rc<StaticWorld> {
        let mut world = StaticWorld::new();
        let quad = floor_quad;
        let floor = quad(-20.0, 0.0, -20.0, 20.0, 20.0);
        world.add_triangles(&floor, 2, Surface::Concrete, layer::STATIC, "floor");
        if let Some(h) = step_height {
            // A raised deck over x in [1, 20], with a vertical riser at x = 1.
            let deck = quad(1.0, h, -20.0, 20.0, 20.0);
            world.add_triangles(&deck, 2, Surface::Concrete, layer::STATIC, "deck");
            // Wound so the riser faces -X, back at an approaching capsule.
            let riser = vec![
                1.0, 0.0, 20.0, 1.0, h, 20.0, 1.0, h, -20.0, //
                1.0, 0.0, 20.0, 1.0, h, -20.0, 1.0, 0.0, -20.0,
            ];
            world.add_triangles(&riser, 2, Surface::Concrete, layer::STATIC, "riser");
        }
        world.build();
        Rc::new(world)
    }

    fn character(world: Rc<StaticWorld>) -> Character {
        Character::new(world, CharacterOpts::default())
    }

    #[test]
    fn a_capsule_dropped_on_the_floor_reports_grounded_with_an_up_normal() {
        let mut c = character(ground_world(None));
        c.teleport(0.0, 0.5, 0.0);
        // Fall for a while.
        for _ in 0..120 {
            c.move_by(0.0, -0.05, 0.0);
        }
        assert!(c.grounded, "the capsule settled on the floor");
        assert!(c.ground_normal[1] > 0.99, "floor normal points up");
        assert!(
            c.position[1].abs() < 0.02,
            "feet rest at the floor, got {}",
            c.position[1]
        );
        assert_eq!(c.ground_surface(), Surface::Concrete);
    }

    #[test]
    fn an_empty_world_never_grounds_and_the_ground_distance_is_infinite() {
        let mut world = StaticWorld::new();
        world.build();
        let mut c = character(Rc::new(world));
        c.teleport(0.0, 5.0, 0.0);
        assert!(!c.grounded);
        assert_eq!(c.ground_distance, f64::INFINITY);
        let travelled = c.move_by(0.0, -1.0, 0.0);
        assert!((travelled - 1.0).abs() < 1e-9, "nothing stopped the fall");
    }

    #[test]
    fn the_step_offset_scheme_walks_up_a_tread_shorter_than_step_height() {
        let mut c = character(ground_world(Some(0.3)));
        c.teleport(-0.6, 0.05, 0.0);
        c.probe_ground();
        assert!(c.grounded);
        // Walk +X into the step over many small fixed steps.
        for _ in 0..240 {
            c.move_by(0.03, -0.001, 0.0);
        }
        assert!(
            c.position[0] > 1.2,
            "the capsule climbed onto the deck, x = {}",
            c.position[0]
        );
        assert!(
            (c.position[1] - 0.3).abs() < 0.05,
            "and is standing on it, y = {}",
            c.position[1]
        );
        assert!(c.stepped_up >= 0.0);
    }

    #[test]
    fn a_wall_taller_than_step_height_blocks_and_clips_the_velocity() {
        let mut c = character(ground_world(Some(2.0)));
        c.teleport(-0.6, 0.05, 0.0);
        c.velocity = [4.0, 0.0, 0.0];
        for _ in 0..240 {
            c.move_by(0.03, -0.001, 0.0);
        }
        assert!(
            c.position[0] < 1.0,
            "the capsule is still on the low side, x = {}",
            c.position[0]
        );
        assert!(c.last_move_blocked, "and the move reported blocked");
        assert!(
            c.velocity[0].abs() < 1e-6,
            "the wall clipped the +X velocity, got {}",
            c.velocity[0]
        );
    }

    #[test]
    fn can_fit_and_check_capsule_agree_about_a_low_ceiling() {
        let mut world = StaticWorld::new();
        world.add_triangles(
            &floor_quad(-8.0, 0.0, -8.0, 8.0, 8.0),
            2,
            Surface::Concrete,
            layer::STATIC,
            "floor",
        );
        world.add_triangles(
            &ceiling_quad(-8.0, 1.2, -8.0, 8.0, 8.0),
            2,
            Surface::Concrete,
            layer::STATIC,
            "ceiling",
        );
        world.build();
        let mut c = character(Rc::new(world));
        // `set_position`, not `teleport`: teleport de-penetrates, and a 1.78 m
        // capsule under a 1.2 m ceiling is penetrating by construction — it
        // would be shoved down through the floor before the test asks its
        // question.
        c.set_position(0.0, 0.0, 0.0);
        assert!(c.can_fit(1.0), "a metre of headroom is available");
        assert!(!c.can_fit(1.78), "standing up is not");
        assert!(!c.check_capsule(0.0, 0.0, 0.0, 1.78));
        assert!(c.check_capsule(0.0, 0.0, 0.0, 1.0));
        // set_height refuses to grow into the ceiling, and force overrides it.
        c.height = 1.0;
        assert!(!c.set_height(1.78, false));
        assert_eq!(c.height, 1.0);
        assert!(c.set_height(1.78, true));
        assert_eq!(c.height, 1.78);
    }

    #[test]
    fn depenetrate_pushes_a_capsule_started_inside_the_floor_back_out() {
        let mut c = character(ground_world(None));
        c.set_position(0.0, -0.2, 0.0);
        let moved = c.depenetrate(8);
        assert!(moved > 0.0, "something pushed");
        assert!(
            c.position[1] > -0.2,
            "and it pushed upward, y = {}",
            c.position[1]
        );
    }

    #[test]
    fn the_ceiling_probe_reports_a_head_contact() {
        let mut world = StaticWorld::new();
        world.add_triangles(
            &ceiling_quad(-8.0, 1.80, -8.0, 8.0, 8.0),
            2,
            Surface::Concrete,
            layer::STATIC,
            "ceiling",
        );
        world.build();
        let mut c = character(Rc::new(world));
        c.set_position(0.0, 0.0, 0.0);
        c.probe_ground();
        assert!(c.touching_ceiling, "the crown is within 6 cm of the slab");
    }

    #[test]
    fn a_disabled_controller_refuses_to_move() {
        let mut c = character(ground_world(None));
        c.teleport(0.0, 1.0, 0.0);
        c.enabled = false;
        assert_eq!(c.move_by(1.0, 0.0, 0.0), 0.0);
        assert_eq!(c.position[0], 0.0);
    }

    #[test]
    fn landing_clips_the_fall_and_the_controllers_own_landing_speed_is_post_clip() {
        let mut c = character(ground_world(None));
        c.teleport(0.0, 1.5, 0.0);
        c.velocity = [0.0, -6.0, 0.0];
        assert!(!c.grounded);
        // One big step straight through to the floor.
        c.move_by(0.0, -3.0, 0.0);
        assert!(c.grounded, "the sweep resolved the whole 3 m in one step");
        assert!(!c.was_grounded, "and it was airborne before it");
        // `landingSpeed = -min(0, velocity.y)` is read AFTER `_slide` has
        // already clipped the velocity into the floor plane, so the
        // controller's own figure is zero on a clean landing. That is the
        // source's behaviour, and exactly why `movement.js`'s `_postMove` maxes
        // it against its own pre-move `prevVy` rather than trusting it — this
        // pins the reason.
        assert_eq!(c.velocity[1], 0.0, "the floor clipped the fall");
        assert_eq!(c.landing_speed, 0.0);
    }

    #[test]
    fn ground_friction_comes_from_the_surface_table() {
        let mut c = character(ground_world(None));
        c.teleport(0.0, 0.2, 0.0);
        c.move_by(0.0, -0.5, 0.0);
        assert_eq!(c.ground_friction(), SURFACE_PROPS[0].friction);
    }

    #[test]
    fn the_seam_impls_forward_to_the_controller() {
        let mut c = character(ground_world(None));
        c.teleport(0.0, 0.2, 0.0);
        // Settle onto the floor: `probe_ground` only reaches 6 cm below the
        // feet, so a capsule teleported 20 cm up is legitimately airborne.
        c.move_by(0.0, -0.5, 0.0);
        let seam: &mut dyn MovementCharacter = &mut c;
        seam.set_velocity([1.0, 0.0, 0.0]);
        assert_eq!(seam.velocity(), [1.0, 0.0, 0.0]);
        seam.set_step_height(0.1);
        assert_eq!(seam.step_height(), 0.1);
        seam.set_grounded(false);
        assert!(!seam.grounded());
        seam.probe_ground();
        assert!(seam.grounded(), "the probe found the floor again");
        seam.set_position(2.0, 0.5, 3.0);
        assert_eq!(LedgeCharacter::position(&c), [2.0, 0.5, 3.0]);
        assert_eq!(LedgeCharacter::radius(&c), 0.32);
    }
}
