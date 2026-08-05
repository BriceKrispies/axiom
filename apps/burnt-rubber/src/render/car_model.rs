//! The cars: the player's, and the traffic's.
//!
//! Both are built from the engine's primitive vocabulary — boxes for the body,
//! cylinders for the wheels — because that vocabulary is what exists, and a
//! stylised low-poly silhouette read at 300 km/h is worth more than a detailed
//! model read never. What matters at speed is the **silhouette and the lights**:
//! a wedge nose, a raked cabin set back, four wheels that visibly turn and
//! steer, brake lights that come on, and boost exhaust that does not.
//!
//! ## The car is seen from behind, so the rear is where the parts go
//!
//! The chase camera looks at the player's tail for the entire race: the rear
//! three-quarter is the only view of this car anyone ever gets. A single body
//! box therefore spends the whole game presenting one big flat coloured wall,
//! which is exactly what a car does not look like. The parts that break that
//! wall up are all rear parts, and they are the ones a real car reads by:
//!
//! * a **chopped greenhouse** — the cabin is a *slot*, not a cab. A tall box of
//!   glass sitting proud on top of the body is the silhouette of a pickup with
//!   a crew cab, and it is what the car read as before: from the chase camera
//!   you saw two vertical walls of glass down each side of the roof. A sports
//!   car's side glass is a shallow band, so the cabin is barely taller than the
//!   decklid lip and the rear screen is long and shallow instead of short and
//!   steep;
//! * **haunches** — the widest point of the car is the rear arches, not the
//!   cabin, and the body is drawn *narrower* than the wheel track so the tyres
//!   stand proud of it instead of being buried inside a full-width slab;
//! * a **raked backlight** — the long dark sloping rear window over a short
//!   decklid is the single most recognisable line on a fastback;
//! * **tail-light bars** — wide, shallow, and standing proud of the rear panel
//!   rather than sunk flush into it;
//! * a **valance** — a near-black bumper across the bottom of the rear panel,
//!   which both grounds the car and halves the height of the coloured wall.
//!
//! ## The chase camera looks *down* on the car, so the top surfaces are furniture too
//!
//! The camera rides above the roofline, which means the largest continuous area
//! of car on screen is not the tail panel at all — it is the roof, the rear
//! screen and the decklid, seen from above. Those were three flat, unbroken
//! planes. Two things break them up, and both are the things a muscle car is
//! recognised by from directly behind:
//!
//! * **twin centre stripes**, in three segments that follow the surfaces they
//!   cross — over the roof, down the raked backlight, and out to the tail over
//!   the decklid lip. Painted stripes are not a texture here: the engine's
//!   material vocabulary has no decals, so a stripe is a shallow box laid a
//!   centimetre proud of the panel it sits on. That is also why they are
//!   segmented — one straight box cannot follow a roof, a rake and a deck;
//! * a **number plate**, a small pale rectangle mounted on the valance below the
//!   lamps. It is tiny, and it is the single most car-like object on the whole
//!   rear panel: nothing else in a night frame is a bright horizontal rectangle
//!   sitting low and central between two red bars.
//!
//! Every part is a separate entity whose world transform is written each frame.
//! The parts are not parented: the app already knows every part's world pose (it
//! is composing the chassis basis anyway), so writing two dozen transforms is
//! cheaper and far easier to read than maintaining a hierarchy and a set of
//! local transforms that have to be kept consistent with it.

use axiom::prelude::{Entity, Handle, Material, Mesh, RunningApp, Spawn, Transform, Vec3, Visible};
use axiom_math::Quat;

use crate::sim::car::CarPose;

use super::palette::{CarLivery, ScenePalette};

/// The car's overall dimensions (m). Chosen against the road: a 4.5 m car on a
/// 17 m road with the camera 2.0 m up reads as a car, not a toy.
pub const CAR_LENGTH: f32 = 4.5;
/// Overall width (m).
pub const CAR_WIDTH: f32 = 2.0;
/// Wheel radius (m) — matches the controller's spin calculation.
pub const WHEEL_RADIUS: f32 = crate::sim::controller::WHEEL_RADIUS;
/// Wheel width (m).
pub const WHEEL_WIDTH: f32 = 0.30;
/// Half the wheelbase (m).
pub const WHEELBASE_HALF: f32 = 1.42;
/// Half the track width between wheel centres (m).
pub const TRACK_HALF: f32 = 0.86;

/// How far the rear window lies back from horizontal (rad).
///
/// Applied as a *negative* chassis pitch, because positive pitch is nose-down:
/// the front edge of the glass has to rise to the roof and the back edge fall to
/// the decklid, which is the opposite sense.
///
/// Shallow (about 12°), because the rake is not free: the glass has to span from
/// the back of the roof to the decklid, so a steep rake and a low roof cannot
/// both be true. A fastback picks the low roof, and gets a long, nearly-flat
/// rear screen out of it — which is the line the car is recognised by.
pub const BACKLIGHT_RAKE: f32 = 0.21;

/// The top of the roof above the car's floor (m).
///
/// The whole car is this tall, and at less than half its width it is a low car,
/// which is the entire point: raise this and the greenhouse turns back into a
/// cab. The cabin box hangs *below* it, so this is the one number that sets the
/// car's height.
pub const ROOF_HEIGHT: f32 = 0.98;

/// How tall the side glass stands above the shoulder line (m).
///
/// Shallow on purpose. This is the height of the vertical glass wall the chase
/// camera sees down each side of the roof, and it is the single number that
/// decides whether the car reads as a coupe or as a pickup cab.
pub const GREENHOUSE_HEIGHT: f32 = 0.26;

/// How wide the body box is, as a fraction of the car's full width.
///
/// Strictly less than one: the full width belongs to the rear arches, and the
/// gap is what lets the tyres show past the bodywork instead of hiding inside
/// it. A full-width body box is why the old car read as a van.
pub const BODY_WIDTH_FRACTION: f32 = 0.86;

/// How far the centre of each racing stripe sits from the car's centreline (m).
///
/// Set against the cabin, not the car: both stripes plus the gap between them
/// have to sit inside the roof, or the stripe runs off the side of the panel it
/// is painted on.
pub const STRIPE_OFFSET: f32 = 0.30;

/// How wide one stripe is (m).
pub const STRIPE_WIDTH: f32 = 0.22;

/// How far proud of the panel a stripe or the plate stands (m).
///
/// Paint has no thickness, but a coincident face z-fights, so the trim is a
/// shallow box lifted off its panel by the smallest offset that reads as flush
/// from a car's length away.
pub const TRIM_PROUD: f32 = 0.015;

/// The player car's parts.
#[derive(Debug, Clone)]
pub struct PlayerCar {
    body: Entity,
    nose: Entity,
    cabin: Entity,
    backlight: Entity,
    wing: Entity,
    haunches: [Entity; 2],
    valance: Entity,
    /// The twin centre stripes, in surface order: roof pair, backlight pair,
    /// decklid pair.
    stripes: [Entity; 6],
    plate: Entity,
    wheels: [Entity; 4],
    brake_lights: [Entity; 2],
    exhausts: [Entity; 2],
}

impl PlayerCar {
    /// Spawn a car in `livery`.
    ///
    /// The model is the same whoever is driving it; only the materials differ,
    /// which is what lets the ghost be this exact car rendered translucent
    /// rather than a second, drifting copy of the model.
    pub fn install(app: &mut RunningApp, livery: &CarLivery) -> PlayerCar {
        let cube = app.add_mesh(Mesh::cube());
        let cylinder = app.add_mesh(Mesh::cylinder());
        let part = |app: &mut RunningApp, mesh, material| {
            app.spawn(Spawn::new(Transform::IDENTITY, mesh, material))
        };
        PlayerCar {
            body: part(app, cube, livery.body),
            nose: part(app, cube, livery.body),
            cabin: part(app, cube, livery.glass),
            backlight: part(app, cube, livery.glass),
            wing: part(app, cube, livery.body),
            haunches: [part(app, cube, livery.body), part(app, cube, livery.body)],
            // The valance is the tyre material on purpose: it is the darkest
            // thing in the palette, and a near-black bumper is what stops the
            // rear panel reading as one tall coloured slab.
            valance: part(app, cube, livery.tyre),
            stripes: [
                part(app, cube, livery.trim),
                part(app, cube, livery.trim),
                part(app, cube, livery.trim),
                part(app, cube, livery.trim),
                part(app, cube, livery.trim),
                part(app, cube, livery.trim),
            ],
            plate: part(app, cube, livery.trim),
            wheels: [
                part(app, cylinder, livery.tyre),
                part(app, cylinder, livery.tyre),
                part(app, cylinder, livery.tyre),
                part(app, cylinder, livery.tyre),
            ],
            brake_lights: [
                part(app, cube, livery.brake_light),
                part(app, cube, livery.brake_light),
            ],
            exhausts: [
                part(app, cube, livery.exhaust),
                part(app, cube, livery.exhaust),
            ],
        }
    }

    /// Pose every part for this frame.
    ///
    /// `braking` and `boost` are presentation intensities in `0..1`, not
    /// simulation state: the brake lights fade up and the exhaust plume grows,
    /// and neither feeds anything back into the car.
    pub fn pose(&self, app: &mut RunningApp, pose: &CarPose, braking: f32, boost: f32) {
        let basis = ChassisBasis::of(pose);
        let rotation = basis.rotation();
        // Posing a car shows it. The parts that are always on say so explicitly
        // rather than relying on never having been hidden — the ghost is hidden
        // whenever there is no ghost run, and this is what brings it back.
        self.always_on()
            .into_iter()
            .for_each(|entity| {
                app.set(entity, Visible(true));
            });

        // Body: the main mass, sitting low — and narrower than the wheel track,
        // so the tyres and the arches, not the flanks, are the widest thing.
        app.set(
            self.body,
            Transform::new(
                basis.at(Vec3::new(0.0, 0.46, -0.15)),
                rotation,
                Vec3::new(CAR_WIDTH * BODY_WIDTH_FRACTION, 0.52, CAR_LENGTH * 0.78),
            ),
        );
        // Nose: narrower and lower still, so the silhouette is a wedge.
        app.set(
            self.nose,
            Transform::new(
                basis.at(Vec3::new(0.0, 0.34, 1.90)),
                rotation,
                Vec3::new(CAR_WIDTH * 0.72, 0.34, 1.20),
            ),
        );
        // Cabin: a narrow, *chopped* greenhouse — long fore-and-aft and barely
        // taller than the decklid lip, sitting straight on the body's shoulder
        // line at 0.72. A cabin as wide as the body is a van roof; a cabin as
        // tall as it is wide is a pickup cab.
        app.set(
            self.cabin,
            Transform::new(
                basis.at(Vec3::new(0.0, ROOF_HEIGHT - GREENHOUSE_HEIGHT * 0.5, 0.24)),
                rotation,
                Vec3::new(CAR_WIDTH * 0.64, GREENHOUSE_HEIGHT, 1.55),
            ),
        );
        // Backlight: the long raked rear window running from the back of the
        // roof down to the decklid. Pitched in the *chassis* frame, so it keeps
        // its rake through pitch and roll instead of standing up under load.
        // Its ends are not free: the top edge meets the back of the roof
        // (z = -0.535, y = ROOF_HEIGHT) and the bottom edge lands on the decklid
        // ahead of the wing (z = -1.60, y = 0.75), and the rake, length and
        // centre below are what that span works out to.
        let glass_centre = Vec3::new(0.0, 0.87, -1.07);
        let glass_rotation = rotation.multiply(Quat::from_euler_xyz(-BACKLIGHT_RAKE, 0.0, 0.0));
        let glass_thickness = 0.08;
        let glass_length = 1.09;
        app.set(
            self.backlight,
            Transform::new(
                basis.at(glass_centre),
                glass_rotation,
                Vec3::new(CAR_WIDTH * 0.62, glass_thickness, glass_length),
            ),
        );
        // Rear haunches: the arches over the back wheels, reaching the car's
        // full width. These are the shoulders the whole rear silhouette hangs
        // off, and the reason the tail reads wider than the roof.
        for (index, entity) in self.haunches.iter().enumerate() {
            let side = [-1.0, 1.0][index];
            app.set(
                *entity,
                Transform::new(
                    basis.at(Vec3::new(side * (CAR_WIDTH * 0.5 - 0.28), 0.56, -1.24)),
                    rotation,
                    Vec3::new(0.56, 0.46, 1.52),
                ),
            );
        }
        // Decklid lip, sitting on the tail rather than floating behind it.
        app.set(
            self.wing,
            Transform::new(
                basis.at(Vec3::new(0.0, 0.76, -1.74)),
                rotation,
                Vec3::new(CAR_WIDTH * 0.84, 0.09, 0.36),
            ),
        );
        // Valance: the dark bumper across the bottom of the rear panel.
        app.set(
            self.valance,
            Transform::new(
                basis.at(Vec3::new(0.0, 0.28, -1.88)),
                rotation,
                Vec3::new(CAR_WIDTH * 0.88, 0.24, 0.30),
            ),
        );

        // Twin centre stripes, in three segments that each lie on the panel they
        // are painted on. The roof and decklid segments are square to the
        // chassis; the middle one carries the backlight's own rake, and is
        // lifted along the *raked* normal rather than straight up, or it would
        // sink through the low end of the glass and float off the high end.
        let stripe_thickness = 0.02;
        let (rake_sin, rake_cos) = BACKLIGHT_RAKE.sin_cos();
        let glass_lift = glass_thickness * 0.5 + stripe_thickness * 0.5 + TRIM_PROUD;
        // (height, along-track centre, orientation, length) per surface.
        let segments: [(f32, f32, Quat, f32); 3] = [
            // Roof: the full length of the cabin, sitting on its top face.
            (
                ROOF_HEIGHT + stripe_thickness * 0.5 + TRIM_PROUD,
                0.24,
                rotation,
                1.55,
            ),
            // Backlight: the same box as the glass, thinner and lifted clear.
            (
                glass_centre.y + glass_lift * rake_cos,
                glass_centre.z - glass_lift * rake_sin,
                glass_rotation,
                glass_length,
            ),
            // Decklid: out over the lip to the back edge of the car.
            (
                0.805 + stripe_thickness * 0.5 + TRIM_PROUD,
                -1.74,
                rotation,
                0.36,
            ),
        ];
        for (index, entity) in self.stripes.iter().enumerate() {
            let (height, along, orientation, length) = segments[index / 2];
            let side = [-1.0, 1.0][index % 2];
            app.set(
                *entity,
                Transform::new(
                    basis.at(Vec3::new(side * STRIPE_OFFSET, height, along)),
                    orientation,
                    Vec3::new(STRIPE_WIDTH, stripe_thickness, length),
                ),
            );
        }

        // Number plate: low and central *on* the valance — fully inside the
        // bumper's height so it reads as bolted to it rather than floating in
        // the air behind the tail, and standing proud of its back face.
        app.set(
            self.plate,
            Transform::new(
                basis.at(Vec3::new(0.0, 0.30, -2.03 - TRIM_PROUD)),
                rotation,
                Vec3::new(0.40, 0.16, 0.05),
            ),
        );

        // Wheels: front pair steers, all four spin with distance travelled.
        for (index, entity) in self.wheels.iter().enumerate() {
            let front = index < 2;
            let side = if index % 2 == 0 { -1.0 } else { 1.0 };
            let steer = if front { pose.steer_angle } else { 0.0 };
            let centre = basis.at(Vec3::new(
                side * TRACK_HALF,
                WHEEL_RADIUS,
                if front { WHEELBASE_HALF } else { -WHEELBASE_HALF },
            ));
            app.set(
                *entity,
                Transform::new(
                    centre,
                    basis.wheel_rotation(steer, pose.wheel_spin),
                    Vec3::new(WHEEL_RADIUS * 2.0, WHEEL_WIDTH, WHEEL_RADIUS * 2.0),
                ),
            );
        }

        // Tail lights: a pair of wide, shallow bars reaching out to the arches
        // with a gap between them, standing *proud* of the rear panel — the old
        // pair sat flush in the bodywork 0.015 m deep and read as a scratch.
        // They are always present, and only grow *taller* when braking, which
        // is the cue that survives being three car-lengths away.
        let light_height = 0.13 + 0.17 * braking.clamp(0.0, 1.0);
        for (index, entity) in self.brake_lights.iter().enumerate() {
            let side = [-1.0, 1.0][index];
            app.set(
                *entity,
                Transform::new(
                    basis.at(Vec3::new(side * 0.50, 0.58, -1.95)),
                    rotation,
                    Vec3::new(0.72, light_height, 0.12),
                ),
            );
            app.set(*entity, Visible(true));
        }

        // Boost exhaust: a plume that only exists while boosting.
        let plume = boost.clamp(0.0, 1.0);
        for (index, entity) in self.exhausts.iter().enumerate() {
            let side = [-1.0, 1.0][index];
            let length = 0.35 + 2.4 * plume;
            app.set(
                *entity,
                Transform::new(
                    // Tucked down under the valance, where an exhaust exits.
                    basis.at(Vec3::new(side * 0.38, 0.30, -2.20 - length * 0.5)),
                    rotation,
                    Vec3::new(0.30 + 0.16 * plume, 0.26 + 0.14 * plume, length),
                ),
            );
            app.set(*entity, Visible(plume > 0.02));
        }
    }

    /// Hide every part — the car is not in this frame at all.
    pub fn hide(&self, app: &mut RunningApp) {
        self.entities()
            .into_iter()
            .for_each(|entity| {
                app.set(entity, Visible(false));
            });
    }

    /// The parts that are visible whenever the car is, i.e. everything except
    /// the two conditional sets [`Self::pose`] drives itself (the brake lamps
    /// and the boost plume).
    fn always_on(&self) -> Vec<Entity> {
        let mut all = vec![
            self.body,
            self.nose,
            self.cabin,
            self.backlight,
            self.wing,
            self.valance,
            self.plate,
        ];
        all.extend_from_slice(&self.stripes);
        all.extend_from_slice(&self.haunches);
        all.extend_from_slice(&self.wheels);
        all
    }

    /// Every entity, for diagnostics and teardown.
    pub fn entities(&self) -> Vec<Entity> {
        let mut all = vec![
            self.body,
            self.nose,
            self.cabin,
            self.backlight,
            self.wing,
            self.valance,
            self.plate,
        ];
        all.extend_from_slice(&self.stripes);
        all.extend_from_slice(&self.haunches);
        all.extend_from_slice(&self.wheels);
        all.extend_from_slice(&self.brake_lights);
        all.extend_from_slice(&self.exhausts);
        all
    }
}

/// The chassis frame a car's parts are placed in.
///
/// Local coordinates are `+X` right, `+Y` up, `+Z` forward (the nose), matching
/// the simulation's own convention, so a part's authored offset reads the way it
/// sounds.
#[derive(Debug, Clone, Copy)]
pub struct ChassisBasis {
    origin: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    yaw: f32,
    pitch: f32,
    roll: f32,
}

impl ChassisBasis {
    /// The basis for a pose.
    ///
    /// All three axes come from the *same* rotation the parts are drawn with, so
    /// the basis is orthonormal by construction. Tilting the flat axes
    /// separately by pitch and roll — which reads more simply — is wrong: two
    /// independent tilts do not stay perpendicular, and the car's parts then
    /// shear apart under combined pitch and roll.
    pub fn of(pose: &CarPose) -> ChassisBasis {
        let rotation = Quat::from_euler_xyz(pose.pitch, pose.yaw, pose.roll);
        ChassisBasis {
            origin: pose.position,
            right: rotation.rotate(Vec3::UNIT_X),
            up: rotation.rotate(Vec3::UNIT_Y),
            forward: rotation.rotate(Vec3::UNIT_Z),
            yaw: pose.yaw,
            pitch: pose.pitch,
            roll: pose.roll,
        }
    }

    /// A local offset in world space.
    pub fn at(&self, local: Vec3) -> Vec3 {
        self.origin
            .add(self.right.mul_scalar(local.x))
            .add(self.up.mul_scalar(local.y))
            .add(self.forward.mul_scalar(local.z))
    }

    /// The chassis rotation, for a part that is square to the body.
    pub fn rotation(&self) -> Quat {
        Quat::from_euler_xyz(self.pitch, self.yaw, self.roll)
    }

    /// The rotation for a wheel: the chassis, plus its steering angle, plus its
    /// roll about its own axle.
    ///
    /// The engine's cylinder is a unit cylinder along `+Y`, so a wheel needs a
    /// quarter turn about `+Z` to lie on its side before anything else applies.
    pub fn wheel_rotation(&self, steer: f32, spin: f32) -> Quat {
        let lay_flat = Quat::from_euler_xyz(0.0, 0.0, std::f32::consts::FRAC_PI_2);
        let rolling = Quat::from_euler_xyz(0.0, spin, 0.0);
        // Minus, for the same reason the controller negates its yaw rate: a
        // right-hand steering input is a decreasing yaw, so adding the steering
        // angle here would point the front wheels out of the turn.
        let steered = Quat::from_euler_xyz(self.pitch, self.yaw - steer, self.roll);
        steered.multiply(lay_flat).multiply(rolling)
    }

    /// The chassis axes, for effects that need to emit from the car.
    pub const fn axes(&self) -> (Vec3, Vec3, Vec3) {
        (self.right, self.up, self.forward)
    }
}

/// One traffic car's parts: a body, a cabin and a tail-light bar.
#[derive(Debug, Clone, Copy)]
pub struct TrafficCarParts {
    body: Entity,
    cabin: Entity,
    lights: Entity,
}

/// The traffic pool's visuals.
#[derive(Debug, Clone)]
pub struct TrafficVisuals {
    cars: Vec<TrafficCarParts>,
}

impl TrafficVisuals {
    /// Spawn `count` traffic cars, all retired.
    pub fn install(app: &mut RunningApp, palette: &ScenePalette, count: usize) -> TrafficVisuals {
        let cube = app.add_mesh(Mesh::cube());
        let cars = (0..count)
            .map(|index| {
                let variant = palette.traffic[index % palette.traffic.len()];
                TrafficCarParts {
                    body: retired(app, cube, variant),
                    cabin: retired(app, cube, palette.car_glass),
                    lights: retired(app, cube, palette.traffic_light),
                }
            })
            .collect();
        TrafficVisuals { cars }
    }

    /// How many cars the pool holds.
    pub fn len(&self) -> usize {
        self.cars.len()
    }

    /// Whether the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.cars.is_empty()
    }

    /// Pose pool entry `index` at `position` facing `yaw`, or retire it.
    pub fn pose(&self, app: &mut RunningApp, index: usize, placement: Option<(Vec3, f32, Vec3)>) {
        let Some(parts) = self.cars.get(index) else {
            return;
        };
        let Some((position, yaw, up)) = placement else {
            for entity in [parts.body, parts.cabin, parts.lights] {
                app.set(entity, Visible(false));
            }
            return;
        };
        let (sy, cy) = yaw.sin_cos();
        let forward = Vec3::new(sy, 0.0, cy);
        let right = Vec3::new(cy, 0.0, -sy);
        let rotation = Quat::from_euler_xyz(0.0, yaw, 0.0);
        let at = |local: Vec3| {
            position
                .add(right.mul_scalar(local.x))
                .add(up.mul_scalar(local.y))
                .add(forward.mul_scalar(local.z))
        };
        app.set(
            parts.body,
            Transform::new(
                at(Vec3::new(0.0, 0.75, 0.0)),
                rotation,
                Vec3::new(2.05, 1.05, 4.4),
            ),
        );
        app.set(
            parts.cabin,
            Transform::new(
                at(Vec3::new(0.0, 1.42, -0.20)),
                rotation,
                Vec3::new(1.78, 0.72, 2.20),
            ),
        );
        app.set(
            parts.lights,
            Transform::new(
                at(Vec3::new(0.0, 0.90, -2.16)),
                rotation,
                Vec3::new(1.70, 0.16, 0.10),
            ),
        );
        for entity in [parts.body, parts.cabin, parts.lights] {
            app.set(entity, Visible(true));
        }
    }
}

/// Spawn a part parked and invisible.
fn retired(app: &mut RunningApp, mesh: Handle<Mesh>, material: Handle<Material>) -> Entity {
    let entity = app.spawn(Spawn::new(Transform::IDENTITY, mesh, material));
    app.set(entity, Visible(false));
    entity
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn app() -> RunningApp {
        App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build()
    }

    fn pose_at(yaw: f32, pitch: f32, roll: f32) -> CarPose {
        CarPose {
            position: Vec3::new(10.0, 2.0, -30.0),
            yaw,
            pitch,
            roll,
            wheel_spin: 1.2,
            steer_angle: 0.0,
        }
    }

    #[test]
    fn the_chassis_basis_is_orthonormal_and_right_handed() {
        for (yaw, pitch, roll) in [(0.0, 0.0, 0.0), (1.3, 0.05, -0.08), (-2.7, -0.06, 0.1)] {
            let basis = ChassisBasis::of(&pose_at(yaw, pitch, roll));
            let (r, u, f) = basis.axes();
            for v in [r, u, f] {
                assert!((v.length() - 1.0).abs() < 1.0e-4, "unit: {v:?}");
            }
            assert!(r.dot(f).abs() < 1.0e-3);
            assert!(r.dot(u).abs() < 1.0e-3);
            assert!(u.y > 0.9, "the car is never upside down");
        }
    }

    #[test]
    fn a_local_offset_lands_where_it_should() {
        let basis = ChassisBasis::of(&pose_at(0.0, 0.0, 0.0));
        // At zero yaw the chassis is world-aligned: +Z is the nose.
        let nose = basis.at(Vec3::new(0.0, 0.0, 2.0));
        assert!((nose.z - (-30.0 + 2.0)).abs() < 1.0e-4);
        let right = basis.at(Vec3::new(1.0, 0.0, 0.0));
        assert!((right.x - 11.0).abs() < 1.0e-4);
        assert_eq!(basis.at(Vec3::ZERO), Vec3::new(10.0, 2.0, -30.0));
    }

    #[test]
    fn pitching_the_nose_down_lowers_the_front_of_the_car() {
        let level = ChassisBasis::of(&pose_at(0.0, 0.0, 0.0));
        let nose_down = ChassisBasis::of(&pose_at(0.0, 0.15, 0.0));
        let front = Vec3::new(0.0, 0.0, 2.0);
        assert!(
            nose_down.at(front).y < level.at(front).y,
            "positive pitch is nose-down, as documented"
        );
    }

    #[test]
    fn rolling_raises_the_right_hand_side() {
        let level = ChassisBasis::of(&pose_at(0.0, 0.0, 0.0));
        let rolled = ChassisBasis::of(&pose_at(0.0, 0.0, 0.2));
        let right = Vec3::new(1.0, 0.0, 0.0);
        assert!(
            rolled.at(right).y > level.at(right).y,
            "positive roll raises the right side, as documented"
        );
    }

    #[test]
    fn the_player_car_poses_every_part_somewhere_sensible() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.7, 0.02, -0.03);
        car.pose(&mut app, &pose, 0.0, 0.0);

        for entity in car.entities() {
            let t = app.get::<Transform>(entity).expect("posed");
            assert!(
                t.translation.distance(pose.position) < CAR_LENGTH,
                "a part ended up {} m from the car",
                t.translation.distance(pose.position)
            );
            assert!(t.scale.x > 0.0 && t.scale.y > 0.0 && t.scale.z > 0.0);
        }
    }

    #[test]
    fn the_wheels_sit_at_the_corners_and_on_the_ground() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.0, 0.0, 0.0);
        car.pose(&mut app, &pose, 0.0, 0.0);

        let centres: Vec<Vec3> = car
            .wheels
            .iter()
            .map(|e| app.get::<Transform>(*e).expect("posed").translation)
            .collect();
        for c in &centres {
            assert!(
                (c.y - (pose.position.y + WHEEL_RADIUS)).abs() < 1.0e-3,
                "a wheel is not on the ground"
            );
            assert!((c.x - pose.position.x).abs() > TRACK_HALF * 0.9, "wheels are outboard");
        }
        // Two in front, two behind.
        let front = centres.iter().filter(|c| c.z > pose.position.z).count();
        assert_eq!(front, 2, "two front wheels");
    }

    /// The rear silhouette, pinned: the tail is the widest part of the car, the
    /// roof is the narrowest, and the tyres are not buried inside the bodywork.
    /// Widening the body box back to the full car width is exactly the edit that
    /// turns this car back into a van, so it fails here.
    #[test]
    fn the_tail_is_the_widest_part_of_the_car_and_the_tyres_show_past_the_body() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.0, 0.0, 0.0);
        car.pose(&mut app, &pose, 0.0, 0.0);

        let body = app.get::<Transform>(car.body).unwrap();
        let cabin = app.get::<Transform>(car.cabin).unwrap();
        let haunch = app.get::<Transform>(car.haunches[1]).unwrap();
        let wheel = app.get::<Transform>(car.wheels[3]).unwrap();

        let body_half = body.scale.x * 0.5;
        let haunch_edge = (haunch.translation.x - pose.position.x) + haunch.scale.x * 0.5;
        let tyre_edge = (wheel.translation.x - pose.position.x) + WHEEL_WIDTH * 0.5;

        assert!(cabin.scale.x < body.scale.x, "the roof is narrower than the flanks");
        assert!(body_half < CAR_WIDTH * 0.5, "the body is inboard of the full width");
        assert!(
            tyre_edge > body_half + 0.1,
            "the rear tyre is buried in the bodywork: {tyre_edge} vs {body_half}"
        );
        assert!(
            (haunch_edge - CAR_WIDTH * 0.5).abs() < 0.05,
            "the arch defines the car's full width: {haunch_edge}"
        );
    }

    /// The greenhouse is a chopped slot, not a cab.
    ///
    /// This is the edit that turns the car back into a pickup: raise the roof,
    /// or make the cabin box tall instead of long, and the chase camera sees two
    /// vertical walls of glass down the sides of the roof. Both the height of
    /// that wall and the height of the whole car are pinned here.
    #[test]
    fn the_greenhouse_is_a_chopped_slot_rather_than_a_cab() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.0, 0.0, 0.0);
        car.pose(&mut app, &pose, 0.0, 0.0);

        let cabin = app.get::<Transform>(car.cabin).unwrap();
        let floor = pose.position.y;
        let roof = cabin.translation.y + cabin.scale.y * 0.5 - floor;

        assert!((roof - ROOF_HEIGHT).abs() < 1.0e-4, "the roof is where it says: {roof}");
        assert!(
            roof < CAR_WIDTH * 0.5,
            "the car is lower than half its width, or it is not a sports car: {roof}"
        );
        assert!(
            cabin.scale.y < cabin.scale.z * 0.25,
            "the greenhouse is long and shallow, not a box: {} tall, {} long",
            cabin.scale.y,
            cabin.scale.z
        );
        // And it sits *on* the body's shoulder rather than floating above it.
        let body = app.get::<Transform>(car.body).unwrap();
        let shoulder = body.translation.y + body.scale.y * 0.5;
        assert!(
            (cabin.translation.y - cabin.scale.y * 0.5 - shoulder).abs() < 0.02,
            "the cabin sits on the shoulder line"
        );
    }

    /// The rear screen spans exactly the gap it has to: the back of the roof
    /// down to the decklid. A steeper rake with this roof height would leave the
    /// glass hanging in the air short of the tail.
    #[test]
    fn the_rear_screen_reaches_from_the_roof_to_the_decklid() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.0, 0.0, 0.0);
        car.pose(&mut app, &pose, 0.0, 0.0);

        let cabin = app.get::<Transform>(car.cabin).unwrap();
        let glass = app.get::<Transform>(car.backlight).unwrap();
        let body = app.get::<Transform>(car.body).unwrap();
        let wing = app.get::<Transform>(car.wing).unwrap();

        let along = glass.rotation.rotate(Vec3::UNIT_Z).mul_scalar(glass.scale.z * 0.5);
        let top = glass.translation.add(along);
        let bottom = glass.translation.add(along.mul_scalar(-1.0));

        assert!(
            (top.y - (cabin.translation.y + cabin.scale.y * 0.5)).abs() < 0.02,
            "the glass meets the roof: {} vs {}",
            top.y,
            cabin.translation.y + cabin.scale.y * 0.5
        );
        assert!(
            (top.z - (cabin.translation.z - cabin.scale.z * 0.5)).abs() < 0.02,
            "and it meets it at the *back* of the roof: {top:?}"
        );
        let deck = body.translation.y + body.scale.y * 0.5;
        assert!(
            bottom.y > deck - 0.02 && bottom.y < wing.translation.y + wing.scale.y * 0.5 + 0.02,
            "the glass lands on the decklid, not above or through it: {}",
            bottom.y
        );
        assert!(
            bottom.z > wing.translation.z,
            "and ahead of the decklid lip: {} vs {}",
            bottom.z,
            wing.translation.z
        );
    }

    /// The rear window is glass laid back over the decklid, not a flat roof
    /// panel: its long axis points up and forward in the chassis frame.
    #[test]
    fn the_backlight_is_raked_back_over_the_decklid() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        car.pose(&mut app, &pose_at(0.0, 0.0, 0.0), 0.0, 0.0);

        let glass = app.get::<Transform>(car.backlight).unwrap();
        let along = glass.rotation.rotate(Vec3::UNIT_Z);
        assert!(
            (along.y - BACKLIGHT_RAKE.sin()).abs() < 0.02,
            "the glass leans back by the authored rake: {along:?}"
        );
        assert!(along.z > 0.8, "and it is still mostly fore-and-aft");
    }

    /// The tail lights sit on the outside of the rear panel. Sunk flush — as
    /// they were — they are invisible from the one camera that ever sees them.
    #[test]
    fn the_tail_lights_stand_proud_of_the_rear_panel() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        car.pose(&mut app, &pose_at(0.0, 0.0, 0.0), 0.0, 0.0);

        let body = app.get::<Transform>(car.body).unwrap();
        let light = app.get::<Transform>(car.brake_lights[0]).unwrap();
        let valance = app.get::<Transform>(car.valance).unwrap();
        // At zero yaw the chassis is world-aligned, so -Z is the tail.
        let panel = body.translation.z - body.scale.z * 0.5;
        let lens = light.translation.z - light.scale.z * 0.5;
        assert!(panel - lens > 0.05, "the lens is flush with the panel: {panel} vs {lens}");
        assert!(light.scale.x > 0.6, "and it is a bar, not a stud");
        assert!(
            valance.translation.y < light.translation.y,
            "the dark valance is below the lights, where a bumper goes"
        );
    }

    /// The stripes are a pair, they run the length of the car's centre, and each
    /// segment sits *on* the panel it is painted on rather than inside it.
    #[test]
    fn the_twin_stripes_lie_on_the_roof_the_glass_and_the_decklid() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.0, 0.0, 0.0);
        car.pose(&mut app, &pose, 0.0, 0.0);

        let cabin = app.get::<Transform>(car.cabin).unwrap();
        let glass = app.get::<Transform>(car.backlight).unwrap();
        let stripes: Vec<Transform> = car
            .stripes
            .iter()
            .map(|e| app.get::<Transform>(*e).expect("posed"))
            .collect();

        // A pair per surface, symmetric about the centreline and inside the roof.
        for pair in stripes.chunks(2) {
            let left = pair[0].translation.x - pose.position.x;
            let right = pair[1].translation.x - pose.position.x;
            assert!((left + right).abs() < 1.0e-4, "the pair straddles the centre");
            assert!(right > 0.0 && left < 0.0);
            assert!(
                right + STRIPE_WIDTH * 0.5 < cabin.scale.x * 0.5,
                "a stripe hangs off the side of the roof: {right}"
            );
            assert!(
                left + STRIPE_WIDTH * 0.5 < 0.0,
                "the two stripes have run into each other"
            );
        }
        // Roof pair: resting on the cabin's top face.
        let roof = cabin.translation.y + cabin.scale.y * 0.5;
        assert!(
            stripes[0].translation.y - stripes[0].scale.y * 0.5 > roof
                && stripes[0].translation.y - stripes[0].scale.y * 0.5 < roof + 0.05,
            "the roof stripe floats or sinks: {}",
            stripes[0].translation.y
        );
        // Backlight pair: carrying the glass's rake, and clear of its outer face.
        assert_eq!(stripes[2].rotation, glass.rotation, "the stripe follows the rake");
        // "Proud of the glass" is a distance along the glass's OWN normal, not the
        // distance between the two centres: the pair straddles the centreline by
        // ±STRIPE_OFFSET (asserted above), so the raw separation is dominated by that
        // lateral span — 0.30 across against 0.065 of lift — and measuring it would
        // fail a correctly-placed stripe.
        let normal = glass.rotation.rotate(Vec3::UNIT_Y);
        let lift = stripes[2].translation.subtract(glass.translation).dot(normal);
        assert!(
            lift > glass.scale.y * 0.5 && lift < glass.scale.y * 0.5 + 0.05,
            "the glass stripe is inside the glass, or floating: {lift}"
        );
        // And every segment is a long thin band, not a patch.
        for s in &stripes {
            assert!(s.scale.z > s.scale.x, "a stripe runs fore-and-aft: {s:?}");
            assert!(s.scale.y < 0.05, "and it is paint, not a spoiler");
        }
    }

    /// The plate is bolted to the bumper: inside its height, proud of its face.
    #[test]
    fn the_number_plate_sits_on_the_valance_below_the_lamps() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        car.pose(&mut app, &pose_at(0.0, 0.0, 0.0), 0.0, 0.0);

        let plate = app.get::<Transform>(car.plate).unwrap();
        let valance = app.get::<Transform>(car.valance).unwrap();
        let light = app.get::<Transform>(car.brake_lights[0]).unwrap();

        assert!(
            plate.translation.y + plate.scale.y * 0.5
                <= valance.translation.y + valance.scale.y * 0.5 + 1.0e-4,
            "the plate hangs off the top of the bumper"
        );
        assert!(
            plate.translation.y + plate.scale.y * 0.5 < light.translation.y,
            "and it is below the lamps, where a plate goes"
        );
        assert!(plate.scale.x < light.scale.x, "narrower than a lamp bar");
        assert!(plate.scale.x > plate.scale.y * 2.0, "and it is a landscape plate");
        // At zero yaw the tail is -Z: the plate's back face is behind the bumper's.
        assert!(
            plate.translation.z - plate.scale.z * 0.5
                < valance.translation.z - valance.scale.z * 0.5,
            "the plate is sunk into the bumper instead of standing on it"
        );
    }

    #[test]
    fn braking_grows_the_brake_lights_and_boosting_shows_the_exhaust() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.0, 0.0, 0.0);

        car.pose(&mut app, &pose, 0.0, 0.0);
        let off = app.get::<Transform>(car.brake_lights[0]).unwrap().scale.y;
        assert_eq!(app.get::<Visible>(car.exhausts[0]), Some(Visible(false)));

        car.pose(&mut app, &pose, 1.0, 1.0);
        let on = app.get::<Transform>(car.brake_lights[0]).unwrap().scale.y;
        assert!(on > off, "the brake lights light up: {off} -> {on}");
        assert_eq!(app.get::<Visible>(car.exhausts[0]), Some(Visible(true)));
        let plume = app.get::<Transform>(car.exhausts[0]).unwrap().scale.z;
        assert!(plume > 2.0, "and the plume is unmistakable: {plume}");

        // Out-of-range intensities are clamped rather than exploding.
        car.pose(&mut app, &pose, 9.0, 9.0);
        let clamped = app.get::<Transform>(car.exhausts[0]).unwrap().scale.z;
        assert!((clamped - plume).abs() < 1.0e-4);
    }

    #[test]
    fn steering_turns_only_the_front_wheels() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let straight = CarPose { steer_angle: 0.0, ..pose_at(0.0, 0.0, 0.0) };
        car.pose(&mut app, &straight, 0.0, 0.0);
        let rear_before = app.get::<Transform>(car.wheels[2]).unwrap().rotation;
        let front_before = app.get::<Transform>(car.wheels[0]).unwrap().rotation;

        let turned = CarPose { steer_angle: 0.5, ..straight };
        car.pose(&mut app, &turned, 0.0, 0.0);
        assert_eq!(
            app.get::<Transform>(car.wheels[2]).unwrap().rotation,
            rear_before,
            "the rear wheels do not steer"
        );
        assert_ne!(
            app.get::<Transform>(car.wheels[0]).unwrap().rotation,
            front_before,
            "the front wheels do"
        );
    }

    #[test]
    fn the_wheel_rotation_lays_the_cylinder_on_its_side() {
        let basis = ChassisBasis::of(&pose_at(0.0, 0.0, 0.0));
        // The engine's cylinder runs along +Y; a wheel's axle must end up
        // pointing sideways (along the car's X) instead.
        let axle = basis.wheel_rotation(0.0, 0.0).rotate(Vec3::UNIT_Y);
        assert!(axle.x.abs() > 0.99, "the axle points sideways: {axle:?}");
        assert!(axle.y.abs() < 0.05);
    }

    #[test]
    fn traffic_cars_pose_and_retire() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let traffic = TrafficVisuals::install(&mut app, &palette, 6);
        assert_eq!(traffic.len(), 6);
        assert!(!traffic.is_empty());

        traffic.pose(&mut app, 0, Some((Vec3::new(3.0, 1.0, 9.0), 0.4, Vec3::UNIT_Y)));
        assert_eq!(app.get::<Visible>(traffic.cars[0].body), Some(Visible(true)));
        let body = app.get::<Transform>(traffic.cars[0].body).unwrap();
        assert!(body.translation.distance(Vec3::new(3.0, 1.0, 9.0)) < 2.0);

        traffic.pose(&mut app, 0, None);
        assert_eq!(app.get::<Visible>(traffic.cars[0].body), Some(Visible(false)));
        assert_eq!(app.get::<Visible>(traffic.cars[0].lights), Some(Visible(false)));

        // An out-of-range index is a no-op rather than a panic.
        traffic.pose(&mut app, 99, None);
    }

    #[test]
    fn the_car_is_a_believable_size_against_the_road() {
        // A 2 m car on a road at least 12 m wide leaves room for four lanes and
        // keeps the car from reading as a toy.
        assert!(CAR_WIDTH < crate::tuning::CourseTuning::DEFAULT.min_half_width);
        assert!(CAR_LENGTH > CAR_WIDTH * 1.8, "it is a car, not a cube");
        assert!(WHEEL_RADIUS * 2.0 < CAR_WIDTH * 0.5);
    }
}
