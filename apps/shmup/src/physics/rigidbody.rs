//! Ported from Claude-of-Duty `src/physics/rigidbody.js:1-650`.
use crate::physics::bvh::StaticWorld;
use crate::physics::math::HitRecord;
use crate::physics::surfaces;
use crate::physics::surfaces::mask;
use std::rc::Rc;

/// `rigidbody.js:205`.
const MAX_CONTACTS: usize = 48;
/// `rigidbody.js:206`.
const SLOP: f64 = 0.0015;
/// `rigidbody.js:208`.
const BAUMGARTE: f64 = 0.4;
/// `rigidbody.js:209`.
const REST_THRESHOLD: f64 = 0.55;
/// `rigidbody.js:210`.
const SLEEP_LINEAR: f64 = 0.035;
/// `rigidbody.js:211`.
const SLEEP_ANGULAR: f64 = 0.22;
/// `rigidbody.js:212`.
const SLEEP_TIME: f64 = 0.45;

/// Rigid body shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Box,
    Sphere,
    Capsule,
}

impl Shape {
    pub fn from_str(s: &str) -> Self {
        match s {
            "sphere" => Shape::Sphere,
            "capsule" => Shape::Capsule,
            _ => Shape::Box,
        }
    }
}

/// A rigid body.
/// Mirrors the `RigidBody` class in `rigidbody.js:26-170`.
#[derive(Debug, Clone)]


pub struct RigidBody {
    pub id: i32,
    pub shape: Shape,
    pub active: bool,

    // Dimensions
    pub hx: f64,
    pub hy: f64,
    pub hz: f64,
    pub radius: f64,
    pub half_height: f64,

    // Mass
    pub mass: f64,
    pub inv_mass: f64,

    // State
    pub position: [f64; 3],
    pub quaternion: [f64; 4], // x, y, z, w
    pub linear_velocity: [f64; 3],
    pub angular_velocity: [f64; 3],
    pub prev_position: [f64; 3],
    pub prev_quaternion: [f64; 4],

    // Material
    pub restitution: f64,
    pub friction: f64,
    pub linear_damping: f64,
    pub angular_damping: f64,
    pub gravity_scale: f64,
    pub surface: u8,
    pub mask: u16,
    pub layer: u16,
    pub ccd: bool,

    // Sleep
    pub sleeping: bool,
    pub sleep_timer: f64,
    pub lifetime: f64,
    pub age: f64,

    // Callbacks (not used in this port)
    _impact_cooldown: f64,

    // Inertia
    pub inv_inertia_local: [f64; 3],
    pub inv_inertia_world: [f64; 9],

    // Probes
    pub probes: Vec<f64>, // [x,y,z,r] * n
    pub probe_count: usize,
    pub probe_radius: f64,
    pub bound_radius: f64,
    pub min_extent: f64,
}

impl RigidBody {
    /// Create a new rigid body. Mirrors `constructor` in `rigidbody.js:27-94`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: i32,
        shape: Shape,
        hx: f64,
        hy: f64,
        hz: f64,
        radius: f64,
        half_height: f64,
        mass: f64,
        position: [f64; 3],
        quaternion: [f64; 4],
        linear_velocity: [f64; 3],
        angular_velocity: [f64; 3],
        restitution: f64,
        friction: f64,
        linear_damping: f64,
        angular_damping: f64,
        gravity_scale: f64,
        surface: u8,
        mask: u16,
        layer: u16,
        ccd: bool,
        lifetime: f64,
    ) -> Self {
        let inv_mass = if mass > 0.0 { 1.0 / mass } else { 0.0 };
        let mut body = RigidBody {
            id,
            shape,
            active: true,
            hx,
            hy,
            hz,
            radius,
            half_height,
            mass,
            inv_mass,
            position,
            quaternion,
            linear_velocity,
            angular_velocity,
            prev_position: position,
            prev_quaternion: quaternion,
            restitution,
            friction,
            linear_damping,
            angular_damping,
            gravity_scale,
            surface,
            mask,
            layer,
            ccd,
            sleeping: false,
            sleep_timer: 0.0,
            lifetime,
            age: 0.0,
            _impact_cooldown: 0.0,
            inv_inertia_local: [0.0; 3],
            inv_inertia_world: [0.0; 9],
            probes: Vec::new(),
            probe_count: 0,
            probe_radius: f64::INFINITY,
            bound_radius: 0.0,
            min_extent: 0.0,
        };
        body.compute_inertia();
        body.build_probes();
        body.bound_radius = hypot3(hx, hy, hz);
        body.min_extent = hx.min(hy).min(hz);
        body
    }

    /// Compute local inertia tensor inverse. Mirrors `_computeInertia` in `rigidbody.js:96-125`.
    fn compute_inertia(&mut self) {
        let m = self.mass;
        if m <= 0.0 {
            self.inv_inertia_local = [0.0, 0.0, 0.0];
            return;
        }
        let (ix, iy, iz) = match self.shape {
            Shape::Sphere => {
                let i = 0.4 * m * self.radius * self.radius;
                (i, i, i)
            }
            Shape::Capsule => {
                let r = self.radius;
                let h = self.half_height * 2.0;
                let mc = m * (h / (h + 1.3333 * r));
                let ms = m - mc;
                let iyy = 0.5 * mc * r * r + 0.8 * ms * r * r;
                let ixx = mc * (0.25 * r * r + h * h / 12.0)
                    + ms * (0.4 * r * r + 0.375 * r * h + 0.25 * h * h);
                (ixx, iyy, ixx)
            }
            Shape::Box => {
                let w = self.hx * 2.0;
                let ht = self.hy * 2.0;
                let d = self.hz * 2.0;
                let k = m / 12.0;
                (k * (ht * ht + d * d), k * (w * w + d * d), k * (w * w + ht * ht))
            }
        };
        self.inv_inertia_local[0] = if ix > 0.0 { 1.0 / ix } else { 0.0 };
        self.inv_inertia_local[1] = if iy > 0.0 { 1.0 / iy } else { 0.0 };
        self.inv_inertia_local[2] = if iz > 0.0 { 1.0 / iz } else { 0.0 };
    }

    /// Build local-space contact probes. Mirrors `buildProbes` in `rigidbody.js:177-201`.
    fn build_probes(&mut self) {
        self.probes.clear();
        match self.shape {
            Shape::Sphere => {
                self.probes.extend_from_slice(&[0.0, 0.0, 0.0, self.radius]);
            }
            Shape::Capsule => {
                let h = self.half_height;
                self.probes.extend_from_slice(&[
                    0.0, -h, 0.0, self.radius,
                    0.0, h, 0.0, self.radius,
                    0.0, 0.0, 0.0, self.radius,
                ]);
            }
            Shape::Box => {
                let hx = self.hx;
                let hy = self.hy;
                let hz = self.hz;
                let r = (0.006_f64).max(hx.min(hy).min(hz) * 0.35);
                for sx in [-1.0, 1.0] {
                    for sy in [-1.0, 1.0] {
                        for sz in [-1.0, 1.0] {
                            self.probes.push(sx * (hx - r));
                            self.probes.push(sy * (hy - r));
                            self.probes.push(sz * (hz - r));
                            self.probes.push(r);
                        }
                    }
                }
            }
        }
        // The source builds probes into a `Float32Array` (`rigidbody.js:179`,
        // `:183`, `:190`), so each coordinate is stored rounded to f32 and every
        // later read — the world-space probe transform, and `probeRadius` below —
        // sees the rounded value. A box half-extent like 0.4 is not exactly
        // representable in f32, so keeping f64 here shifts contact points by ~1e-8.
        self.probes.iter_mut().for_each(|v| *v = *v as f32 as f64);

        self.probe_count = self.probes.len() / 4;
        self.probe_radius = f64::INFINITY;
        for i in 0..self.probe_count {
            let r = self.probes[i * 4 + 3];
            if r < self.probe_radius {
                self.probe_radius = r;
            }
        }
    }

    /// Update world-space inertia tensor. Mirrors `updateInertiaWorld` in `rigidbody.js:127-146`.
    pub fn update_inertia_world(&mut self) {
        // Convert quaternion to rotation matrix (column-major, like THREE.Matrix3)
        let [x, y, z, w] = self.quaternion;
        let xx = x * x; let yy = y * y; let zz = z * z;
        let xy = x * y; let xz = x * z; let yz = y * z;
        let wx = w * x; let wy = w * y; let wz = w * z;

        // Column-major elements (matching THREE.Matrix3):
        // [ e0, e3, e6 ]
        // [ e1, e4, e7 ]
        // [ e2, e5, e8 ]
        let e0 = 1.0 - 2.0 * yy - 2.0 * zz;
        let e1 = 2.0 * xy + 2.0 * wz;
        let e2 = 2.0 * xz - 2.0 * wy;
        let e3 = 2.0 * xy - 2.0 * wz;
        let e4 = 1.0 - 2.0 * xx - 2.0 * zz;
        let e5 = 2.0 * yz + 2.0 * wx;
        let e6 = 2.0 * xz + 2.0 * wy;
        let e7 = 2.0 * yz - 2.0 * wx;
        let e8 = 1.0 - 2.0 * xx - 2.0 * yy;

        let ix = self.inv_inertia_local[0];
        let iy = self.inv_inertia_local[1];
        let iz = self.inv_inertia_local[2];

        // a = R * diag(I)  (column-major: scale columns)
        let a0 = e0 * ix; let a1 = e1 * ix; let a2 = e2 * ix;
        let a3 = e3 * iy; let a4 = e4 * iy; let a5 = e5 * iy;
        let a6 = e6 * iz; let a7 = e7 * iz; let a8 = e8 * iz;

        // invInertiaWorld = a * R^T  (column-major)
        self.inv_inertia_world[0] = a0 * e0 + a3 * e3 + a6 * e6;
        self.inv_inertia_world[1] = a1 * e0 + a4 * e3 + a7 * e6;
        self.inv_inertia_world[2] = a2 * e0 + a5 * e3 + a8 * e6;
        self.inv_inertia_world[3] = a0 * e1 + a3 * e4 + a6 * e7;
        self.inv_inertia_world[4] = a1 * e1 + a4 * e4 + a7 * e7;
        self.inv_inertia_world[5] = a2 * e1 + a5 * e4 + a8 * e7;
        self.inv_inertia_world[6] = a0 * e2 + a3 * e5 + a6 * e8;
        self.inv_inertia_world[7] = a1 * e2 + a4 * e5 + a7 * e8;
        self.inv_inertia_world[8] = a2 * e2 + a5 * e5 + a8 * e8;
    }

    /// Wake the body. Mirrors `wake` in `rigidbody.js:148-151`.
    pub fn wake(&mut self) {
        self.sleeping = false;
        self.sleep_timer = 0.0;
    }

    /// Apply impulse at world point. Mirrors `applyImpulse` in `rigidbody.js:153-169`.
    pub fn apply_impulse(&mut self, ix: f64, iy: f64, iz: f64, px: f64, py: f64, pz: f64) {
        if self.inv_mass == 0.0 {
            return;
        }
        self.wake();
        self.linear_velocity[0] += ix * self.inv_mass;
        self.linear_velocity[1] += iy * self.inv_mass;
        self.linear_velocity[2] += iz * self.inv_mass;
        let rx = px - self.position[0];
        let ry = py - self.position[1];
        let rz = pz - self.position[2];
        let tx = ry * iz - rz * iy;
        let ty = rz * ix - rx * iz;
        let tz = rx * iy - ry * ix;
        let w = &self.inv_inertia_world;
        self.angular_velocity[0] += w[0] * tx + w[3] * ty + w[6] * tz;
        self.angular_velocity[1] += w[1] * tx + w[4] * ty + w[7] * tz;
        self.angular_velocity[2] += w[2] * tx + w[5] * ty + w[8] * tz;
    }
}

/// Quaternion integration. Mirrors `integrateQuaternion` in `rigidbody.js:611-625`.
fn integrate_quaternion(q: &mut [f64; 4], w: &[f64; 3], dt: f64) {
    let wx = w[0] * dt * 0.5;
    let wy = w[1] * dt * 0.5;
    let wz = w[2] * dt * 0.5;
    let x = q[0];
    let y = q[1];
    let z = q[2];
    let s = q[3];
    q[0] = x + (wx * s + wy * z - wz * y);
    q[1] = y + (wy * s + wz * x - wx * z);
    q[2] = z + (wz * s + wx * y - wy * x);
    q[3] = s - (wx * x + wy * y + wz * z);
    let l = hypot4(q[0], q[1], q[2], q[3]);
    if l > 1e-9 {
        let i = 1.0 / l;
        q[0] *= i; q[1] *= i; q[2] *= i; q[3] *= i;
    } else {
        q[0] = 0.0; q[1] = 0.0; q[2] = 0.0; q[3] = 1.0;
    }
}

/// Quaternion-rotate a local vector (x component). Mirrors `rotX` in `rigidbody.js:628-633`.
fn rot_x(q: &[f64; 4], x: f64, y: f64, z: f64) -> f64 {
    let tx = 2.0 * (q[1] * z - q[2] * y);
    let ty = 2.0 * (q[2] * x - q[0] * z);
    let tz = 2.0 * (q[0] * y - q[1] * x);
    x + q[3] * tx + (q[1] * tz - q[2] * ty)
}

/// Quaternion-rotate a local vector (y component). Mirrors `rotY` in `rigidbody.js:634-639`.
fn rot_y(q: &[f64; 4], x: f64, y: f64, z: f64) -> f64 {
    let tx = 2.0 * (q[1] * z - q[2] * y);
    let ty = 2.0 * (q[2] * x - q[0] * z);
    let tz = 2.0 * (q[0] * y - q[1] * x);
    y + q[3] * ty + (q[2] * tx - q[0] * tz)
}

/// Quaternion-rotate a local vector (z component). Mirrors `rotZ` in `rigidbody.js:640-645`.
fn rot_z(q: &[f64; 4], x: f64, y: f64, z: f64) -> f64 {
    let tx = 2.0 * (q[1] * z - q[2] * y);
    let ty = 2.0 * (q[2] * x - q[0] * z);
    let tz = 2.0 * (q[0] * y - q[1] * x);
    z + q[3] * tz + (q[0] * ty - q[1] * tx)
}

/// Apply impulse helper for solver. Mirrors `applyImpulse` function in `rigidbody.js:599-609`.
fn apply_impulse_solver(
    body: &mut RigidBody,
    ix: f64, iy: f64, iz: f64,
    rx: f64, ry: f64, rz: f64,
    iw: &[f64; 9],
) {
    body.linear_velocity[0] += ix * body.inv_mass;
    body.linear_velocity[1] += iy * body.inv_mass;
    body.linear_velocity[2] += iz * body.inv_mass;
    let tx = ry * iz - rz * iy;
    let ty = rz * ix - rx * iz;
    let tz = rx * iy - ry * ix;
    body.angular_velocity[0] += iw[0] * tx + iw[3] * ty + iw[6] * tz;
    body.angular_velocity[1] += iw[1] * tx + iw[4] * ty + iw[7] * tz;
    body.angular_velocity[2] += iw[2] * tx + iw[5] * ty + iw[8] * tz;
}

/// The rigid body world.
/// Mirrors `RigidBodyWorld` class in `rigidbody.js:214-597`.
pub struct RigidBodyWorld {
    world: Rc<StaticWorld>,
    gravity: f64,
    bodies: Vec<RigidBody>,
    max_bodies: usize,
    solver_iterations: usize,
    next_id: i32,

    // Contact scratch
    cn: [f64; MAX_CONTACTS * 3], // normals
    cp: [f64; MAX_CONTACTS * 3], // points
    cd: [f64; MAX_CONTACTS],     // depths
    cf: [f64; MAX_CONTACTS],     // friction coefficients
    ce: [f64; MAX_CONTACTS],     // restitution coefficients
    cs: [u8; MAX_CONTACTS],      // surface indices

    // Hit record for CCD
    hit: HitRecord,

    // Stats
    awake_count: usize,
    contacts_last_step: usize,
}

impl RigidBodyWorld {
    /// Create a new rigid body world.
    pub fn new(world: Rc<StaticWorld>, gravity: f64) -> Self {
        RigidBodyWorld {
            world,
            gravity,
            bodies: Vec::new(),
            max_bodies: 256,
            solver_iterations: 4,
            next_id: 1,
            cn: [0.0; MAX_CONTACTS * 3],
            cp: [0.0; MAX_CONTACTS * 3],
            cd: [0.0; MAX_CONTACTS],
            cf: [0.0; MAX_CONTACTS],
            ce: [0.0; MAX_CONTACTS],
            cs: [0; MAX_CONTACTS],
            hit: HitRecord::default(),
            awake_count: 0,
            contacts_last_step: 0,
        }
    }

    /// Add a body to the world. Mirrors `add` in `rigidbody.js:234-249`.
    pub fn add(&mut self, mut body: RigidBody) -> RigidBody {
        if self.bodies.len() >= self.max_bodies {
            let mut victim = -1;
            let mut best_age = -1.0;
            for (i, b) in self.bodies.iter().enumerate() {
                if b.sleeping && b.age > best_age {
                    best_age = b.age;
                    victim = i as i32;
                }
            }
            if victim < 0 {
                victim = 0;
            }
            self.remove(self.bodies[victim as usize].id);
        }
        body.id = self.next_id;
        self.next_id += 1;
        body.update_inertia_world();
        self.bodies.push(body.clone());
        body
    }

    /// Remove a body by id. Mirrors `remove` in `rigidbody.js:251-256`.
    pub fn remove(&mut self, id: i32) -> Option<RigidBody> {
        let idx = self.bodies.iter().position(|b| b.id == id);
        if let Some(i) = idx {
            let mut body = self.bodies.remove(i);
            body.active = false;
            Some(body)
        } else {
            None
        }
    }

    /// Clear all bodies. Mirrors `clear` in `rigidbody.js:258-260`.
    pub fn clear(&mut self) {
        self.bodies.clear();
    }

    /// Apply radial impulse. Mirrors `applyRadialImpulse` in `rigidbody.js:263-277`.
    pub fn apply_radial_impulse(&mut self, x: f64, y: f64, z: f64, radius: f64, strength: f64) {
        let r2 = radius * radius;
        for body in &mut self.bodies {
            let dx = body.position[0] - x;
            let dy = body.position[1] - y;
            let dz = body.position[2] - z;
            let d2 = dx * dx + dy * dy + dz * dz;
            if d2 > r2 {
                continue;
            }
            let d = d2.sqrt().max(1e-4);
            let falloff = 1.0 - d / radius;
            let j = strength * falloff * falloff * body.mass;
            body.apply_impulse(
                (dx / d) * j,
                (dy / d) * j + j * 0.35,
                (dz / d) * j,
                body.position[0] + dx * 0.05,
                body.position[1] + dy * 0.05,
                body.position[2] + dz * 0.05,
            );
        }
    }

    /// Step the simulation. Mirrors `step` in `rigidbody.js:279-294`.
    pub fn step(&mut self, dt: f64) {
        self.awake_count = 0;
        self.contacts_last_step = 0;
        let mut i = self.bodies.len();
        while i > 0 {
            i -= 1;
            let age = self.bodies[i].age + dt;
            self.bodies[i].age = age;
            if age > self.bodies[i].lifetime {
                self.bodies.remove(i);
                continue;
            }
            if self.bodies[i].sleeping {
                continue;
            }
            self.awake_count += 1;
            self.step_body(i, dt);
        }
    }

    /// Step a single body. Mirrors `_stepBody` in `rigidbody.js:296-383`.
    fn step_body(&mut self, idx: usize, dt: f64) {
        // Copy body out to avoid borrow issues
        let mut body = self.bodies[idx].clone();

        body.prev_position = body.position;
        body.prev_quaternion = body.quaternion;
        if body._impact_cooldown > 0.0 {
            body._impact_cooldown -= dt;
        }

        // --- integrate velocity ---
        body.linear_velocity[1] += self.gravity * body.gravity_scale * dt;
        let ld = (-body.linear_damping * dt).exp();
        let ad = (-body.angular_damping * dt).exp();
        body.linear_velocity[0] *= ld;
        body.linear_velocity[1] *= ld;
        body.linear_velocity[2] *= ld;
        body.angular_velocity[0] *= ad;
        body.angular_velocity[1] *= ad;
        body.angular_velocity[2] *= ad;

        // --- Continuous pass (CCD) ---
        let speed = (body.linear_velocity[0].powi(2) + body.linear_velocity[1].powi(2) + body.linear_velocity[2].powi(2)).sqrt();
        let travel = speed * dt;
        let probe_r = body.probe_radius;
        if body.ccd && travel > probe_r && self.world.tri_count() > 0 {
            let inv = 1.0 / speed;
            let dx = body.linear_velocity[0] * inv;
            let dy = body.linear_velocity[1] * inv;
            let dz = body.linear_velocity[2] * inv;
            let core = probe_r.max(body.min_extent * 0.9);
            let hit = self.world.sweep_capsule(
                body.position[0], body.position[1], body.position[2],
                body.position[0], body.position[1], body.position[2],
                core, dx, dy, dz, travel, body.mask,
            );
            if hit.hit {
                let adv = (hit.t - 0.002).max(0.0);
                body.position[0] += dx * adv;
                body.position[1] += dy * adv;
                body.position[2] += dz * adv;

                let sp = surfaces::SURFACE_PROPS.get(hit.surface as usize).copied().unwrap_or(surfaces::SURFACE_PROPS[0]);
                let e = (body.restitution * sp.restitution).max(0.0).sqrt();
                let mu = (body.friction * sp.friction).max(0.0).sqrt();
                let vn = body.linear_velocity[0] * hit.nx + body.linear_velocity[1] * hit.ny + body.linear_velocity[2] * hit.nz;
                if vn < 0.0 {
                    let tx = body.linear_velocity[0] - hit.nx * vn;
                    let ty = body.linear_velocity[1] - hit.ny * vn;
                    let tz = body.linear_velocity[2] - hit.nz * vn;
                    let keep = (1.0 - mu * 0.5).max(0.0_f64);
                    body.linear_velocity[0] = tx * keep - hit.nx * vn * e;
                    body.linear_velocity[1] = ty * keep - hit.ny * vn * e;
                    body.linear_velocity[2] = tz * keep - hit.nz * vn * e;
                    let spin = (-vn * 2.5).min(30.0);
                    body.angular_velocity[0] += (hit.ny * tz - hit.nz * ty) * spin * 0.05;
                    body.angular_velocity[1] += (hit.nz * tx - hit.nx * tz) * spin * 0.05;
                    body.angular_velocity[2] += (hit.nx * ty - hit.ny * tx) * spin * 0.05;
                }
                // Impact callback not ported
                if body._impact_cooldown <= 0.0 && -vn > 1.0 {
                    body._impact_cooldown = 0.08;
                }
                integrate_quaternion(&mut body.quaternion, &body.angular_velocity, dt);
                body.update_inertia_world();
                self.sleep_check(&mut body, dt);
                self.bodies[idx] = body;
                return;
            }
        }

        // --- Discrete substepping ---
        let max_step_dist = (0.004_f64).max(probe_r * 0.75);
        let sub = if body.ccd { ((travel / max_step_dist).ceil() as usize).max(1) } else { 1 };
        let sub = sub.min(12);
        let h = dt / sub as f64;

        for _s in 0..sub {
            body.position[0] += body.linear_velocity[0] * h;
            body.position[1] += body.linear_velocity[1] * h;
            body.position[2] += body.linear_velocity[2] * h;
            integrate_quaternion(&mut body.quaternion, &body.angular_velocity, h);
            body.update_inertia_world();
            let n = self.collect(&mut body);
            if n > 0 {
                self.contacts_last_step += n;
                self.solve(&mut body, n, h);
            }
        }

        self.sleep_check(&mut body, dt);
        self.bodies[idx] = body;
    }

    /// Sleep check. Mirrors `_sleepCheck` in `rigidbody.js:385-411`.
    fn sleep_check(&mut self, body: &mut RigidBody, dt: f64) {
        let mut lin = body.linear_velocity[0].powi(2) + body.linear_velocity[1].powi(2) + body.linear_velocity[2].powi(2);
        let mut ang = body.angular_velocity[0].powi(2) + body.angular_velocity[1].powi(2) + body.angular_velocity[2].powi(2);
        let ang_limit = SLEEP_ANGULAR / (0.12_f64).max(body.bound_radius);
        if lin < 9.0 * SLEEP_LINEAR * SLEEP_LINEAR && ang < 9.0 * ang_limit * ang_limit {
            body.linear_velocity[0] *= 0.9;
            body.linear_velocity[1] *= 0.9;
            body.linear_velocity[2] *= 0.9;
            body.angular_velocity[0] *= 0.85;
            body.angular_velocity[1] *= 0.85;
            body.angular_velocity[2] *= 0.85;
            lin = body.linear_velocity[0].powi(2) + body.linear_velocity[1].powi(2) + body.linear_velocity[2].powi(2);
            ang = body.angular_velocity[0].powi(2) + body.angular_velocity[1].powi(2) + body.angular_velocity[2].powi(2);
        }
        if lin < SLEEP_LINEAR * SLEEP_LINEAR && ang < ang_limit * ang_limit {
            body.sleep_timer += dt;
            if body.sleep_timer > SLEEP_TIME {
                body.sleeping = true;
                body.linear_velocity = [0.0, 0.0, 0.0];
                body.angular_velocity = [0.0, 0.0, 0.0];
            }
        } else {
            body.sleep_timer = 0.0;
        }
    }

    /// Collect contacts. Mirrors `_collect` in `rigidbody.js:414-486`.
    fn collect(&mut self, body: &RigidBody) -> usize {
        let w = &self.world;
        if w.tri_count() == 0 {
            return 0;
        }
        let r = body.bound_radius + 0.05;
        let candidates = w.query_aabb(
            body.position[0] - r, body.position[1] - r, body.position[2] - r,
            body.position[0] + r, body.position[1] + r, body.position[2] + r,
            body.mask,
        );
        if candidates.is_empty() {
            return 0;
        }

        let probes = &body.probes;
        let q = &body.quaternion;
        let mut count = 0;

        for pi in 0..body.probe_count {
            if count >= MAX_CONTACTS {
                break;
            }
            let lx = probes[pi * 4];
            let ly = probes[pi * 4 + 1];
            let lz = probes[pi * 4 + 2];
            let pr = probes[pi * 4 + 3];

            let wx = rot_x(q, lx, ly, lz) + body.position[0];
            let wy = rot_y(q, lx, ly, lz) + body.position[1];
            let wz = rot_z(q, lx, ly, lz) + body.position[2];

            let mut deepest = 0.0;
            let mut dnx = 0.0;
            let mut dny = 0.0;
            let mut dnz = 0.0;
            let mut dpx = 0.0;
            let mut dpy = 0.0;
            let mut dpz = 0.0;
            let mut dtri = -1i32;

            for &tri in &candidates {
                let tri_u32 = tri as u32;
                let tri_verts = w.triangle_of(tri_u32);
                let (bx, by, bz) = crate::physics::math::closest_pt_point_triangle(
                    wx, wy, wz,
                    tri_verts[0][0], tri_verts[0][1], tri_verts[0][2],
                    tri_verts[1][0], tri_verts[1][1], tri_verts[1][2],
                    tri_verts[2][0], tri_verts[2][1], tri_verts[2][2],
                );
                let ex = wx - bx;
                let ey = wy - by;
                let ez = wz - bz;
                let d2 = ex * ex + ey * ey + ez * ez;
                if d2 >= pr * pr {
                    continue;
                }
                let d = d2.sqrt();
                let (mut nx, mut ny, mut nz);
                if d > 1e-6 {
                    nx = ex / d;
                    ny = ey / d;
                    nz = ez / d;
                    let tri_normal = w.normal_of(tri_u32);
                    let fdot = nx * tri_normal[0] + ny * tri_normal[1] + nz * tri_normal[2];
                    if fdot < 0.02 {
                        nx = tri_normal[0];
                        ny = tri_normal[1];
                        nz = tri_normal[2];
                    }
                } else {
                    let tri_normal = w.normal_of(tri_u32);
                    nx = tri_normal[0];
                    ny = tri_normal[1];
                    nz = tri_normal[2];
                }
                let depth = pr - d;
                if depth > deepest {
                    deepest = depth;
                    dnx = nx; dny = ny; dnz = nz;
                    dpx = bx; dpy = by; dpz = bz;
                    dtri = tri as i32;
                }
            }

            if dtri >= 0 {
                let k = count;
                count += 1;
                // The source's contact scratch is `Float32Array` (`rigidbody.js:223-227`),
                // so every value written here is rounded to f32 and the solver reads
                // the rounded value back. Storing full f64 changes the impulses by
                // ~1e-8 from the first contact onward. `as f32 as f64` reproduces the
                // `Float32Array` store exactly.
                self.cn[k * 3] = dnx as f32 as f64;
                self.cn[k * 3 + 1] = dny as f32 as f64;
                self.cn[k * 3 + 2] = dnz as f32 as f64;
                self.cp[k * 3] = dpx as f32 as f64;
                self.cp[k * 3 + 1] = dpy as f32 as f64;
                self.cp[k * 3 + 2] = dpz as f32 as f64;
                self.cd[k] = deepest as f32 as f64;
                let tri_surface = w.surface_of(dtri as u32);
                let sp = surfaces::SURFACE_PROPS.get(tri_surface.index() as usize).copied().unwrap_or(surfaces::SURFACE_PROPS[0]);
                self.cf[k] = (body.friction * sp.friction).max(0.0).sqrt() as f32 as f64;
                self.ce[k] = (body.restitution * sp.restitution).max(0.0).sqrt() as f32 as f64;
                self.cs[k] = tri_surface.index();
            }
        }
        count
    }

    /// Solve contacts. Mirrors `_solve` in `rigidbody.js:488-592`.
    fn solve(&mut self, body: &mut RigidBody, n: usize, _dt: f64) {
        let iw = body.inv_inertia_world;
        let im = body.inv_mass;
        let mut max_approach = 0.0;
        let mut impact_idx = -1;

        // Pre-pass: find hardest approach for impact reporting
        for k in 0..n {
            let nx = self.cn[k * 3];
            let ny = self.cn[k * 3 + 1];
            let nz = self.cn[k * 3 + 2];
            let rx = self.cp[k * 3] - body.position[0];
            let ry = self.cp[k * 3 + 1] - body.position[1];
            let rz = self.cp[k * 3 + 2] - body.position[2];
            let vx = body.linear_velocity[0] + (body.angular_velocity[1] * rz - body.angular_velocity[2] * ry);
            let vy = body.linear_velocity[1] + (body.angular_velocity[2] * rx - body.angular_velocity[0] * rz);
            let vz = body.linear_velocity[2] + (body.angular_velocity[0] * ry - body.angular_velocity[1] * rx);
            let vn = vx * nx + vy * ny + vz * nz;
            if -vn > max_approach {
                max_approach = -vn;
                impact_idx = k as i32;
            }
        }

        for _iter in 0..self.solver_iterations {
            for k in 0..n {
                let nx = self.cn[k * 3];
                let ny = self.cn[k * 3 + 1];
                let nz = self.cn[k * 3 + 2];
                let rx = self.cp[k * 3] - body.position[0];
                let ry = self.cp[k * 3 + 1] - body.position[1];
                let rz = self.cp[k * 3 + 2] - body.position[2];

                // relative velocity at contact
                let vx = body.linear_velocity[0] + (body.angular_velocity[1] * rz - body.angular_velocity[2] * ry);
                let vy = body.linear_velocity[1] + (body.angular_velocity[2] * rx - body.angular_velocity[0] * rz);
                let vz = body.linear_velocity[2] + (body.angular_velocity[0] * ry - body.angular_velocity[1] * rx);
                let vn = vx * nx + vy * ny + vz * nz;

                // effective mass
                let rnx = ry * nz - rz * ny;
                let rny = rz * nx - rx * nz;
                let rnz = rx * ny - ry * nx;
                let iax = iw[0] * rnx + iw[3] * rny + iw[6] * rnz;
                let iay = iw[1] * rnx + iw[4] * rny + iw[7] * rnz;
                let iaz = iw[2] * rnx + iw[5] * rny + iw[8] * rnz;
                let ang_term = (iay * rz - iaz * ry) * nx + (iaz * rx - iax * rz) * ny + (iax * ry - iay * rx) * nz;
                let kn = im + ang_term;
                if kn <= 1e-9 {
                    continue;
                }

                let e = if max_approach > REST_THRESHOLD { self.ce[k] } else { 0.0 };
                let mut jn = (-(1.0 + e) * vn) / kn;
                if jn < 0.0 { jn = 0.0; }
                if jn > 0.0 {
                    apply_impulse_solver(body, nx * jn, ny * jn, nz * jn, rx, ry, rz, &iw);
                }

                // --- friction ---
                let vx = body.linear_velocity[0] + (body.angular_velocity[1] * rz - body.angular_velocity[2] * ry);
                let vy = body.linear_velocity[1] + (body.angular_velocity[2] * rx - body.angular_velocity[0] * rz);
                let vz = body.linear_velocity[2] + (body.angular_velocity[0] * ry - body.angular_velocity[1] * rx);
                let vnn = vx * nx + vy * ny + vz * nz;
                let mut tx = vx - nx * vnn;
                let mut ty = vy - ny * vnn;
                let mut tz = vz - nz * vnn;
                let tl = hypot3(tx, ty, tz);
                if tl > 1e-6 {
                    tx /= tl; ty /= tl; tz /= tl;
                    let rtx = ry * tz - rz * ty;
                    let rty = rz * tx - rx * tz;
                    let rtz = rx * ty - ry * tx;
                    let jx = iw[0] * rtx + iw[3] * rty + iw[6] * rtz;
                    let jy = iw[1] * rtx + iw[4] * rty + iw[7] * rtz;
                    let jz = iw[2] * rtx + iw[5] * rty + iw[8] * rtz;
                    let ang_t = (jy * rz - jz * ry) * tx + (jz * rx - jx * rz) * ty + (jx * ry - jy * rx) * tz;
                    let kt = im + ang_t;
                    if kt > 1e-9 {
                        let mut jt = -tl / kt;
                        let max_f = self.cf[k] * jn;
                        if jt < -max_f { jt = -max_f; }
                        if jt > max_f { jt = max_f; }
                        apply_impulse_solver(body, tx * jt, ty * jt, tz * jt, rx, ry, rz, &iw);
                    }
                }
            }
        }

        // Positional correction (Baumgarte)
        let mut deepest = 0.0;
        let mut dk: Option<usize> = None;
        for k in 0..n {
            if self.cd[k] > deepest {
                deepest = self.cd[k];
                dk = Some(k);
            }
        }
        if let Some(k) = dk {
            if deepest > SLOP {
                let push = (deepest - SLOP).min(0.08) * BAUMGARTE;
                body.position[0] += self.cn[k * 3] * push;
                body.position[1] += self.cn[k * 3 + 1] * push;
                body.position[2] += self.cn[k * 3 + 2] * push;
            }
        }

        // Impact reporting (callbacks not ported)
        if impact_idx >= 0 && max_approach > 1.0 && body._impact_cooldown <= 0.0 {
            body._impact_cooldown = 0.08;
        }
    }

    /// Get number of awake bodies.
    pub fn awake_count(&self) -> usize {
        self.awake_count
    }

    /// Get all bodies.
    pub fn bodies(&self) -> &[RigidBody] {
        &self.bodies
    }

    /// Get a body by id.
    pub fn get_body(&self, id: i32) -> Option<&RigidBody> {
        self.bodies.iter().find(|b| b.id == id)
    }

    /// Get a mutable body by id.
    pub fn get_body_mut(&mut self, id: i32) -> Option<&mut RigidBody> {
        self.bodies.iter_mut().find(|b| b.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::bvh::StaticWorld;
    use crate::physics::surfaces::layer;
    use crate::world::palette::Surface;
    use std::rc::Rc;

    fn test_world() -> Rc<StaticWorld> {
        let mut world = StaticWorld::new();
        let floor = vec![
            -10.0, 0.0, 10.0, 10.0, 0.0, 10.0, 10.0, 0.0, -10.0,
            -10.0, 0.0, 10.0, 10.0, 0.0, -10.0, -10.0, 0.0, -10.0,
        ];
        world.add_triangles(&floor, 2, Surface::Concrete, layer::STATIC, "floor");
        world.build();
        Rc::new(world)
    }

    #[test]
    fn box_falls_and_rests() {
        let world = test_world();
        let mut rbw = RigidBodyWorld::new(world, -20.6);
        let body = RigidBody::new(
            1, Shape::Box, 0.5, 0.5, 0.5, 0.5, 0.0, 1.0,
            [0.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0], [0.0, 0.0, 0.0],
            0.2, 0.6, 0.16, 0.5, 1.0, 0, mask::DEBRIS, 0, false, f64::INFINITY,
        );
        rbw.add(body);
        for _ in 0..2400 {
            rbw.step(1.0 / 60.0);
        }
        let b = &rbw.bodies()[0];
        assert!(b.position[1] > 0.497 && b.position[1] < 0.6, "y = {}", b.position[1]);
        let lin_vel_sq = b.linear_velocity[0].powi(2) + b.linear_velocity[1].powi(2) + b.linear_velocity[2].powi(2);
        let ang_vel_sq = b.angular_velocity[0].powi(2) + b.angular_velocity[1].powi(2) + b.angular_velocity[2].powi(2);
        assert!(lin_vel_sq < 0.01 && ang_vel_sq < 0.01, "velocities not near zero: lin²={lin_vel_sq}, ang²={ang_vel_sq}");
    }
}

/// `Math.hypot` — max-scaled, not a raw root of the sum of squares.
///
/// The source calls `Math.hypot` in three places (`rigidbody.js:85`, `:547`,
/// `:618`). Transcribing those as `(x*x + y*y + z*z).sqrt()` is a different
/// function: `hypot` divides through by the largest magnitude first, so it
/// rounds differently. That is ~1 ULP in isolation, but `:618` normalises the
/// quaternion every step and the resulting rotation builds the world inertia
/// tensor, so the error reaches both linear and angular velocity through the
/// contact solver and compounds from first contact onward.
fn hypot3(x: f64, y: f64, z: f64) -> f64 {
    let m = x.abs().max(y.abs()).max(z.abs());
    if m == 0.0 {
        return 0.0;
    }
    let (a, b, c) = (x / m, y / m, z / m);
    m * (a * a + b * b + c * c).sqrt()
}

fn hypot4(x: f64, y: f64, z: f64, w: f64) -> f64 {
    let m = x.abs().max(y.abs()).max(z.abs()).max(w.abs());
    if m == 0.0 {
        return 0.0;
    }
    let (a, b, c, d) = (x / m, y / m, z / m, w / m);
    m * (a * a + b * b + c * c + d * d).sqrt()
}
