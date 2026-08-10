//! The cars: the player's, and the traffic's.
//!
//! Both are built from the engine's primitive vocabulary — boxes for the body,
//! cylinders for the wheels — because that vocabulary is what exists, and a
//! stylised low-poly silhouette read at 300 km/h is worth more than a detailed
//! model read never. What matters at speed is the **silhouette and the lights**:
//! a wedge nose, a raked cabin set back, four wheels that visibly turn and
//! steer, brake lights that come on, and boost exhaust that does not.
//!
//! ## The proportion is a long bonnet, and that is the first thing you read
//!
//! Before any panel or lamp, a car is a *plan proportion*: where the cabin sits
//! along the length. A cab-forward car with the windscreen over the front axle
//! is a hatchback; a car with the cabin set well aft behind a long bonnet is a
//! muscle fastback, and it is a fastback from any angle, including the one angle
//! this game ever uses. This car was cab-forward — a 1.55 m cabin sitting almost
//! amidships with a short stub of nose beyond it — and from the chase camera it
//! read as short and tall rather than long and low, because the only cue the
//! camera has for length is *how much car there is ahead of the roof*.
//!
//! So the car is built in three volumes along its length, not two:
//!
//! * the **tub** — flanks, floor and decklid — which now stops at the **cowl**,
//!   the shut line at the base of the windscreen, instead of running most of the
//!   way to the nose;
//! * the **bonnet** — a long, slightly narrower and slightly lower panel from the
//!   cowl forward, which is the surface the whole long-nose read lives on;
//! * the **prow** — narrower and lower again, the last half-metre.
//!
//! Bonnet and prow are what a chamfered wedge nose is when the vocabulary is
//! boxes: each tier steps in *and* down, so the silhouette narrows toward the
//! vanishing point instead of ending in one flat slab the width of the body. The
//! flat roof shortens to pay for it — which is also what a fastback's roof does,
//! since its roofline is mostly backlight — and the roof's *rear* edge does not
//! move, so the rear screen, the sails and the whole tail are untouched.
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
//! * a **raked backlight** — the dark sloping rear window over the decklid is
//!   the single most recognisable line on a fastback. It is a *window*, which
//!   means it is an **aperture in bodywork**, not a pane laid across the whole
//!   top of the car. That distinction is the difference between a car and a
//!   glasshouse, and from the chase camera — which looks down on the largest
//!   continuous area of car there is — it is the difference you see first. The
//!   glass is therefore framed on three sides by painted panels: a **roof skin**
//!   above it (the top slice of the cabin, painted, with the side glass showing
//!   as a shallow band underneath), **sail panels** down either side of it, and
//!   the **decklid** below it. The glass itself is drawn narrow enough, and
//!   stops far enough forward, to leave room for all three;
//! * **tail-light clusters** — a *cluster*, not a patch. One glowing block per
//!   side is what the chase camera used to see, and under braking it inflated
//!   into a square orange eye, which is the one shape a car's tail never is.
//!   Each side is now a dark bezel of fixed size with two thin lens tubes
//!   inside it, all standing proud of the rear panel rather than sunk flush
//!   into it. The bezel holds the silhouette still; only the lit tubes swell
//!   when the brakes come on;
//! * a **centre badge** — a small disc between the two clusters. It is the one
//!   piece of furniture that was missing from the middle of the tail, and the
//!   gap it fills is exactly the gap the lamps leave: a rear panel with two
//!   lamps and nothing between them reads as a pair of eyes on a wall, and a
//!   rear panel with a roundel between them reads as the back of a car;
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
//! * **twin centre stripes**, in four segments that follow the surfaces they
//!   cross — down the bonnet, over the roof skin, across the decklid, and out to
//!   the tail over the decklid lip. Painted stripes are not a texture here: the engine's
//!   material vocabulary has no decals, so a stripe is a shallow box laid a
//!   centimetre proud of the panel it sits on. That is also why they are
//!   segmented — one straight box cannot follow a roof and a deck at two
//!   heights. They do **not** cross the backlight: paint does not run over
//!   glass, and a stripe drawn on the rear screen fills the one dark shape the
//!   whole rear silhouette is read by;
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

/// Where the outer face of the tail panel sits along the car (m).
///
/// The back of the car, and the one end of it that is pinned: the chase camera
/// is framed on the tail, so lengthening the car has to happen at the nose.
pub const BODY_TAIL_Z: f32 = -1.905;

/// Where the cowl sits along the car (m) — the base of the windscreen, and the
/// shut line between the tub and the bonnet.
///
/// This is the number the whole proportion turns on. Everything aft of it is
/// cabin and deck; everything ahead of it is bonnet. Push it forward and the car
/// becomes cab-forward — a hatchback with a spoiler.
pub const COWL_Z: f32 = 0.53;

/// Where the bonnet ends and the prow begins (m).
pub const PROW_Z: f32 = 2.00;

/// Where the very front of the car is (m).
pub const NOSE_Z: f32 = 2.66;

/// How high the bonnet's top surface stands above the car's floor (m).
///
/// A shade under the tub's shoulder at 0.72, so the cowl reads as a shut line
/// and the front tyres crown just proud of the bonnet — which is what a wheel
/// arch looks like on a car whose fenders stand above its bonnet.
pub const BONNET_HEIGHT: f32 = 0.68;

/// How high the prow's top surface stands above the car's floor (m).
pub const PROW_HEIGHT: f32 = 0.52;

/// How wide the bonnet is, as a fraction of the car's full width.
pub const BONNET_WIDTH_FRACTION: f32 = 0.80;

/// How wide the prow is, as a fraction of the car's full width.
///
/// Narrower than the bonnet, which is narrower than the tub: three tiers is what
/// a chamfered wedge nose is when the vocabulary is boxes.
pub const PROW_WIDTH_FRACTION: f32 = 0.65;

/// How far the rear window lies back from horizontal (rad).
///
/// Applied as a *negative* chassis pitch, because positive pitch is nose-down:
/// the front edge of the glass has to rise to the roof and the back edge fall to
/// the decklid, which is the opposite sense.
///
/// About 18°. The rake is not free — the glass has to span from the back of the
/// roof to the decklid, so rake, length and roof height are three names for one
/// number. The screen used to be raked as shallowly as it could be and run as
/// far back as it could reach, which spent the entire top of the car on glass
/// and left no decklid at all: from behind, the car was a pane of navy from
/// flank to flank with the tail panel bolted under it. A slightly steeper rake
/// buys the same drop in two-thirds of the length, and the two-thirds it gives
/// back is the painted deck the stripes and the lip live on.
pub const BACKLIGHT_RAKE: f32 = 0.31;

/// How wide the rear screen is, as a fraction of the car's full width.
///
/// Strictly narrower than the cabin: the glass is an aperture, and the margin is
/// the sail panel down each side of it. A screen as wide as the bodywork is not
/// a window, it is a roof made of glass.
pub const BACKLIGHT_WIDTH_FRACTION: f32 = 0.44;

/// How thick the painted roof skin over the cabin is (m).
///
/// The cabin box is the glazing; this is the panel that caps it. Without it the
/// roof is glass, and the single largest area of car the chase camera ever sees
/// is a dark slab instead of paint.
pub const ROOF_SKIN: f32 = 0.06;

/// The top of the roof above the car's floor (m).
///
/// The whole car is this tall, and at less than half its width it is a low car,
/// which is the entire point: raise this and the greenhouse turns back into a
/// cab. The cabin box hangs *below* it, so this is the one number that sets the
/// car's height.
pub const ROOF_HEIGHT: f32 = 0.98;

/// How tall the glazed cabin box stands above the shoulder line (m).
///
/// Shallow on purpose. This is the height of the glass wall the chase camera
/// sees down each side of the roof, and it is the single number that decides
/// whether the car reads as a coupe or as a pickup cab. The painted
/// [`ROOF_SKIN`] takes the top slice of it, so what is left showing is a band.
pub const GREENHOUSE_HEIGHT: f32 = 0.20;

/// Where the back of the cabin sits along the car (m).
///
/// Pinned: the rear screen hangs off the back of the roof, so this is the one
/// end of the cabin that cannot move without moving the whole tail with it.
pub const CABIN_REAR_Z: f32 = -0.535;

/// How long the cabin is, fore and aft (m).
///
/// Short, because a fastback's roofline is mostly backlight and because every
/// centimetre the cabin does not spend is a centimetre of bonnet. Its back edge
/// is pinned at [`CABIN_REAR_Z`], so shortening it moves the *windscreen* aft —
/// which is exactly the edit that turns a cab-forward car into a long-nosed one.
pub const CABIN_LENGTH: f32 = 1.05;

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

/// How wide one tail-lamp cluster is (m).
///
/// Two of these plus the badge between them span nearly the whole tail, which
/// is what makes the rear panel read as lamps-with-a-car-around-them rather
/// than a wall with two patches on it.
pub const LAMP_WIDTH: f32 = 0.68;

/// How tall the dark bezel around one tail-lamp cluster is (m).
///
/// Fixed, on purpose: this is the lamp's *silhouette*, and a silhouette that
/// changes size when the driver brakes is a lamp that looks like it is
/// inflating. The lit tubes inside it are what change.
pub const LAMP_BEZEL_HEIGHT: f32 = 0.22;

/// How tall one lens tube is with the brakes off (m).
pub const LAMP_TUBE_HEIGHT: f32 = 0.05;

/// How far each lens tube sits above/below the cluster's centreline (m).
///
/// Set so that both tubes, at their braking height, still clear each other and
/// still sit inside the bezel — the dark line between them is the detail that
/// makes it a cluster instead of one thick bar.
pub const LAMP_TUBE_OFFSET: f32 = 0.052;

/// How tall the tail-lamp centre sits above the car's floor (m).
pub const LAMP_HEIGHT: f32 = 0.58;

/// How far each tail-lamp cluster sits from the centreline (m).
pub const LAMP_OFFSET: f32 = 0.50;

/// How far back the rear-panel furniture — lamps, badge — sits (m).
pub const TAIL_PANEL_Z: f32 = -1.95;

/// The diameter of the centre badge (m).
///
/// Sized to the gap the two lamp clusters leave between them, with a margin:
/// a badge that touches the lenses is a bar, not a roundel.
pub const BADGE_DIAMETER: f32 = 0.17;

/// How far proud of the panel a stripe or the plate stands (m).
///
/// Paint has no thickness, but a coincident face z-fights, so the trim is a
/// shallow box lifted off its panel by the smallest offset that reads as flush
/// from a car's length away.
pub const TRIM_PROUD: f32 = 0.015;

/// The player car's parts.
#[derive(Debug, Clone)]
pub struct PlayerCar {
    /// The tub: flanks, floor and decklid, from the tail forward to the cowl.
    body: Entity,
    /// The long panel from the cowl forward — the surface the long-nose read
    /// lives on, and the second tier of the wedge.
    bonnet: Entity,
    /// The last half-metre: narrower and lower again, the third tier.
    prow: Entity,
    cabin: Entity,
    /// The painted panel capping the cabin — the roof, as opposed to the glass.
    roof: Entity,
    backlight: Entity,
    /// The painted pillars down either side of the backlight, left then right.
    sails: [Entity; 2],
    wing: Entity,
    haunches: [Entity; 2],
    valance: Entity,
    /// The twin centre stripes, front to back: bonnet pair, roof pair, decklid
    /// pair, lip pair.
    stripes: [Entity; 8],
    plate: Entity,
    /// The round centre badge, between the two lamp clusters.
    badge: Entity,
    wheels: [Entity; 4],
    /// The dark surround of each tail-lamp cluster, left then right.
    lamp_bezels: [Entity; 2],
    /// The lit lens tubes inside the bezels, in cluster order: left upper, left
    /// lower, right upper, right lower.
    brake_lights: [Entity; 4],
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
            bonnet: part(app, cube, livery.body),
            prow: part(app, cube, livery.body),
            cabin: part(app, cube, livery.glass),
            // The roof skin and the sail panels are paint, not glass: they are
            // the frame the rear screen is an aperture in.
            roof: part(app, cube, livery.body),
            backlight: part(app, cube, livery.glass),
            sails: [part(app, cube, livery.body), part(app, cube, livery.body)],
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
                part(app, cube, livery.trim),
                part(app, cube, livery.trim),
            ],
            plate: part(app, cube, livery.trim),
            // The badge is the cylinder mesh stood on its end so its flat face
            // points at the chase camera — the one round thing on a car built
            // entirely out of boxes, which is exactly why it reads.
            badge: part(app, cylinder, livery.trim),
            wheels: [
                part(app, cylinder, livery.tyre),
                part(app, cylinder, livery.tyre),
                part(app, cylinder, livery.tyre),
                part(app, cylinder, livery.tyre),
            ],
            // The bezels take the tyre material for the same reason the valance
            // does: it is the darkest thing in the palette, and a dark surround
            // is what makes a lit lens look lit.
            lamp_bezels: [part(app, cube, livery.tyre), part(app, cube, livery.tyre)],
            brake_lights: [
                part(app, cube, livery.brake_light),
                part(app, cube, livery.brake_light),
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

        // Tub: the main mass, sitting low — narrower than the wheel track, so the
        // tyres and the arches, not the flanks, are the widest thing — and
        // stopping at the *cowl* rather than running on toward the nose. The tub
        // is cabin and deck; the bonnet ahead of it is its own volume, and that
        // separation is what buys the car a long nose.
        app.set(
            self.body,
            Transform::new(
                basis.at(Vec3::new(0.0, 0.46, (COWL_Z + BODY_TAIL_Z) * 0.5)),
                rotation,
                Vec3::new(
                    CAR_WIDTH * BODY_WIDTH_FRACTION,
                    0.52,
                    COWL_Z - BODY_TAIL_Z,
                ),
            ),
        );
        // Bonnet: the long panel from the cowl to the prow, a step narrower and a
        // step lower than the tub. It shares the tub's floor so nothing hangs in
        // the air, and its top face is what the front stripes are painted on.
        app.set(
            self.bonnet,
            Transform::new(
                basis.at(Vec3::new(
                    0.0,
                    (BONNET_HEIGHT + 0.20) * 0.5,
                    (PROW_Z + COWL_Z) * 0.5,
                )),
                rotation,
                Vec3::new(
                    CAR_WIDTH * BONNET_WIDTH_FRACTION,
                    BONNET_HEIGHT - 0.20,
                    PROW_Z - COWL_Z,
                ),
            ),
        );
        // Prow: narrower and lower again — the third tier of the wedge, and the
        // one that makes the car taper toward the vanishing point instead of
        // ending in a slab the width of its own flanks.
        app.set(
            self.prow,
            Transform::new(
                basis.at(Vec3::new(
                    0.0,
                    (PROW_HEIGHT + 0.22) * 0.5,
                    (NOSE_Z + PROW_Z) * 0.5,
                )),
                rotation,
                Vec3::new(
                    CAR_WIDTH * PROW_WIDTH_FRACTION,
                    PROW_HEIGHT - 0.22,
                    NOSE_Z - PROW_Z,
                ),
            ),
        );
        // Cabin: a narrow, *chopped* greenhouse — barely taller than the decklid
        // lip, sitting straight on the body's shoulder line at 0.72, and set well
        // aft: its back edge is pinned at CABIN_REAR_Z and it is only
        // CABIN_LENGTH long, so its windscreen lands near the cowl and the whole
        // metre and a half in front of that is bonnet. A cabin as wide as the
        // body is a van roof; a cabin as tall as it is wide is a pickup cab; a
        // cabin that runs on toward the front axle is a hatchback. This box is
        // the *glazing*; the painted skin below caps it, so what shows down each
        // flank is a band.
        let cabin_centre_z = CABIN_REAR_Z + CABIN_LENGTH * 0.5;
        app.set(
            self.cabin,
            Transform::new(
                basis.at(Vec3::new(
                    0.0,
                    ROOF_HEIGHT - ROOF_SKIN - GREENHOUSE_HEIGHT * 0.5,
                    cabin_centre_z,
                )),
                rotation,
                Vec3::new(CAR_WIDTH * 0.64, GREENHOUSE_HEIGHT, CABIN_LENGTH),
            ),
        );
        // Roof skin: the painted panel over the cabin, a shade wider and longer
        // than the glass it caps so its edge reads as a drip rail rather than
        // z-fighting the glazing. Its top face *is* ROOF_HEIGHT.
        app.set(
            self.roof,
            Transform::new(
                basis.at(Vec3::new(0.0, ROOF_HEIGHT - ROOF_SKIN * 0.5, cabin_centre_z)),
                rotation,
                Vec3::new(CAR_WIDTH * 0.66, ROOF_SKIN, CABIN_LENGTH + 0.02),
            ),
        );
        // Backlight: the raked rear window running from the back of the roof
        // down to the decklid. Pitched in the *chassis* frame, so it keeps its
        // rake through pitch and roll instead of standing up under load. Its
        // ends are not free: the top edge meets the back of the roof skin
        // (z = -0.535, y = ROOF_HEIGHT) and the bottom edge lands on the deck
        // at z = -1.24, well ahead of the lip — the deck aft of it is what the
        // stripes cross and what stops the top of the car being all glass.
        let glass_centre = Vec3::new(0.0, 0.867, -0.887);
        let glass_rotation = rotation.multiply(Quat::from_euler_xyz(-BACKLIGHT_RAKE, 0.0, 0.0));
        let glass_thickness = 0.08;
        let glass_length = 0.74;
        let glass_half_width = CAR_WIDTH * BACKLIGHT_WIDTH_FRACTION * 0.5;
        app.set(
            self.backlight,
            Transform::new(
                basis.at(glass_centre),
                glass_rotation,
                Vec3::new(glass_half_width * 2.0, glass_thickness, glass_length),
            ),
        );
        // Sail panels: the painted pillars the screen is set between. They carry
        // the glass's own rake and sit on its centreline, thicker than it, so
        // the glass is recessed between two raised edges — which is what turns a
        // dark quad into a window instead of a hole in the roof.
        for (index, entity) in self.sails.iter().enumerate() {
            let side = [-1.0, 1.0][index];
            app.set(
                *entity,
                Transform::new(
                    basis.at(Vec3::new(
                        side * (glass_half_width + 0.11),
                        glass_centre.y,
                        glass_centre.z,
                    )),
                    glass_rotation,
                    Vec3::new(0.22, glass_thickness + 0.04, glass_length),
                ),
            );
        }
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

        // Twin centre stripes, in four segments that each lie on the painted
        // panel they belong to, running front to back. All four are square to the
        // chassis, because all four surfaces are: the stripes stop at the foot of
        // the windscreen and of the roof and pick up again beyond, exactly as
        // paint does. The bonnet pair is not decoration — a metre and a half of
        // blank paint ahead of the roof reads as a gap in the car, and the
        // stripes running away up it are what makes the length legible from the
        // one camera that ever sees this car.
        let stripe_thickness = 0.02;
        // (height, along-track centre, orientation, length) per surface.
        let segments: [(f32, f32, Quat, f32); 4] = [
            // Bonnet: from the cowl forward to just short of the prow step.
            (
                BONNET_HEIGHT + stripe_thickness * 0.5 + TRIM_PROUD,
                (PROW_Z + COWL_Z) * 0.5,
                rotation,
                PROW_Z - COWL_Z - 0.06,
            ),
            // Roof skin: the full length of the cabin, sitting on its top face.
            (
                ROOF_HEIGHT + stripe_thickness * 0.5 + TRIM_PROUD,
                cabin_centre_z,
                rotation,
                CABIN_LENGTH,
            ),
            // Decklid: the flat deck between the foot of the backlight
            // (z = -1.24) and the front of the lip (z = -1.56), on the body's
            // own top face at 0.72.
            (
                0.72 + stripe_thickness * 0.5 + TRIM_PROUD,
                -1.40,
                rotation,
                0.32,
            ),
            // Decklid lip: out over it to the back edge of the car.
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

        // Tail lamps: two clusters reaching out towards the arches, each a dark
        // bezel with a pair of thin lens tubes inside it, all standing *proud*
        // of the rear panel — the old lamps sat flush in the bodywork and read
        // as a scratch. The bezel never changes size: braking swells only the
        // lit tubes, so the car's tail keeps the same shape whether or not the
        // driver is on the brakes, and the cue is the light growing rather than
        // the lamp growing.
        for (index, entity) in self.lamp_bezels.iter().enumerate() {
            let side = [-1.0, 1.0][index];
            app.set(
                *entity,
                Transform::new(
                    basis.at(Vec3::new(side * LAMP_OFFSET, LAMP_HEIGHT, TAIL_PANEL_Z)),
                    rotation,
                    Vec3::new(LAMP_WIDTH + 0.06, LAMP_BEZEL_HEIGHT, 0.12),
                ),
            );
        }
        let lens_height = LAMP_TUBE_HEIGHT + 0.035 * braking.clamp(0.0, 1.0);
        for (index, entity) in self.brake_lights.iter().enumerate() {
            let side = [-1.0, 1.0][index / 2];
            let tube = [1.0, -1.0][index % 2];
            app.set(
                *entity,
                Transform::new(
                    basis.at(Vec3::new(
                        side * LAMP_OFFSET,
                        LAMP_HEIGHT + tube * LAMP_TUBE_OFFSET,
                        TAIL_PANEL_Z - 0.04,
                    )),
                    rotation,
                    Vec3::new(LAMP_WIDTH, lens_height, 0.08),
                ),
            );
            app.set(*entity, Visible(true));
        }

        // Centre badge: a disc filling the gap between the clusters, at the
        // lamps' own height. The engine's cylinder runs along +Y, so it takes a
        // quarter turn about the chassis X to lay its flat face against the
        // tail — the same trick the wheels use, a quarter turn the other way.
        app.set(
            self.badge,
            Transform::new(
                basis.at(Vec3::new(0.0, LAMP_HEIGHT, TAIL_PANEL_Z - 0.05)),
                rotation.multiply(Quat::from_euler_xyz(std::f32::consts::FRAC_PI_2, 0.0, 0.0)),
                Vec3::new(BADGE_DIAMETER, 0.06, BADGE_DIAMETER),
            ),
        );

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
            self.bonnet,
            self.prow,
            self.cabin,
            self.roof,
            self.backlight,
            self.wing,
            self.valance,
            self.plate,
            self.badge,
        ];
        all.extend_from_slice(&self.sails);
        all.extend_from_slice(&self.stripes);
        all.extend_from_slice(&self.haunches);
        all.extend_from_slice(&self.wheels);
        all.extend_from_slice(&self.lamp_bezels);
        all
    }

    /// Every entity, for diagnostics and teardown.
    pub fn entities(&self) -> Vec<Entity> {
        let mut all = vec![
            self.body,
            self.bonnet,
            self.prow,
            self.cabin,
            self.roof,
            self.backlight,
            self.wing,
            self.valance,
            self.plate,
            self.badge,
        ];
        all.extend_from_slice(&self.sails);
        all.extend_from_slice(&self.stripes);
        all.extend_from_slice(&self.haunches);
        all.extend_from_slice(&self.wheels);
        all.extend_from_slice(&self.lamp_bezels);
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

    /// The car is a long-bonnet fastback: there is more car ahead of the
    /// windscreen than behind the rear screen, and the nose tapers in plan and in
    /// elevation over three tiers rather than ending in one slab.
    ///
    /// Pushing the cabin forward, or collapsing the bonnet and the prow back into
    /// a single stub, is exactly the edit that turns this car back into the
    /// stubby cab-forward hatchback it was — so it fails here.
    #[test]
    fn the_bonnet_is_longer_than_the_deck_and_the_nose_tapers_over_three_tiers() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.0, 0.0, 0.0);
        car.pose(&mut app, &pose, 0.0, 0.0);

        let body = app.get::<Transform>(car.body).unwrap();
        let bonnet = app.get::<Transform>(car.bonnet).unwrap();
        let prow = app.get::<Transform>(car.prow).unwrap();
        let cabin = app.get::<Transform>(car.cabin).unwrap();
        // At zero yaw the chassis is world-aligned, so +Z is the nose.
        let along = |t: &Transform| {
            (
                t.translation.z - pose.position.z - t.scale.z * 0.5,
                t.translation.z - pose.position.z + t.scale.z * 0.5,
            )
        };
        let (cabin_back, cabin_front) = along(&cabin);
        let (bonnet_back, bonnet_front) = along(&bonnet);
        let (prow_back, prow_front) = along(&prow);
        let (tail, cowl) = along(&body);

        // The tub stops at the cowl, and the bonnet takes over from there.
        assert!(
            (cowl - COWL_Z).abs() < 1.0e-4 && (tail - BODY_TAIL_Z).abs() < 1.0e-4,
            "the tub does not span tail to cowl: {tail}..{cowl}"
        );
        assert!(
            bonnet_back <= cowl + 1.0e-3 && prow_back <= bonnet_front + 1.0e-3,
            "the three volumes leave a gap: {tail}..{cowl}..{bonnet_front}..{prow_front}"
        );
        // The cabin is set aft of the cowl, not forward over the front axle.
        assert!(
            cabin_front < COWL_Z + 1.0e-3 && cabin_front < WHEELBASE_HALF,
            "the windscreen sits ahead of the cowl: {cabin_front}"
        );
        // And the long-nose read itself: more bonnet than deck, by a margin.
        let bonnet_run = prow_front - cabin_front;
        let deck_run = cabin_back - tail;
        assert!(
            bonnet_run > deck_run * 1.4,
            "the car is cab-forward: {bonnet_run} m of bonnet against {deck_run} m of deck"
        );

        // The wedge: each tier steps in *and* down toward the nose.
        assert!(
            body.scale.x > bonnet.scale.x && bonnet.scale.x > prow.scale.x,
            "the nose does not taper in plan: {} {} {}",
            body.scale.x,
            bonnet.scale.x,
            prow.scale.x
        );
        let top = |t: &Transform| t.translation.y - pose.position.y + t.scale.y * 0.5;
        assert!(
            top(&body) > top(&bonnet) && top(&bonnet) > top(&prow),
            "the nose does not drop toward the front: {} {} {}",
            top(&body),
            top(&bonnet),
            top(&prow)
        );
        // Nothing hangs in the air: the front tiers share the tub's floor.
        let floor = |t: &Transform| t.translation.y - pose.position.y - t.scale.y * 0.5;
        assert!(
            floor(&bonnet) <= floor(&body) + 0.05 && floor(&prow) <= floor(&body) + 0.05,
            "a front volume floats above the floor"
        );
        // The front tyres still crown at or above the bonnet, as fenders do.
        assert!(
            WHEEL_RADIUS * 2.0 >= top(&bonnet) - 0.05,
            "the bonnet has swallowed the front wheels"
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
        let skin = app.get::<Transform>(car.roof).unwrap();
        let floor = pose.position.y;
        let roof = skin.translation.y + skin.scale.y * 0.5 - floor;

        // The roof is paint, and it caps the glazing rather than floating over
        // it: swapping the skin back out for glass is the edit that turns the
        // whole top of the car into a navy slab, so it fails here.
        assert!(skin.scale.x > cabin.scale.x, "the skin caps the glazing it sits on");
        assert!(
            (skin.translation.y - skin.scale.y * 0.5 - (cabin.translation.y + cabin.scale.y * 0.5))
                .abs()
                < 1.0e-4,
            "the roof skin floats off the cabin instead of capping it"
        );
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

        let skin = app.get::<Transform>(car.roof).unwrap();
        let glass = app.get::<Transform>(car.backlight).unwrap();
        let body = app.get::<Transform>(car.body).unwrap();
        let wing = app.get::<Transform>(car.wing).unwrap();

        let along = glass.rotation.rotate(Vec3::UNIT_Z).mul_scalar(glass.scale.z * 0.5);
        let top = glass.translation.add(along);
        let bottom = glass.translation.add(along.mul_scalar(-1.0));

        assert!(
            (top.y - (skin.translation.y + skin.scale.y * 0.5)).abs() < 0.02,
            "the glass meets the roof: {} vs {}",
            top.y,
            skin.translation.y + skin.scale.y * 0.5
        );
        assert!(
            (top.z - (skin.translation.z - skin.scale.z * 0.5)).abs() < 0.02,
            "and it meets it at the *back* of the roof: {top:?}"
        );
        let deck = body.translation.y + body.scale.y * 0.5;
        assert!(
            bottom.y > deck - 0.02 && bottom.y < wing.translation.y + wing.scale.y * 0.5 + 0.02,
            "the glass lands on the decklid, not above or through it: {}",
            bottom.y
        );
        // And it stops with real deck left behind it. A screen that runs back to
        // the lip spends the whole top of the car on glass, which is exactly the
        // silhouette this model exists not to have.
        assert!(
            bottom.z > wing.translation.z + wing.scale.z * 0.5 + 0.25,
            "no decklid is left aft of the glass: {} vs a lip at {}",
            bottom.z,
            wing.translation.z + wing.scale.z * 0.5
        );
    }

    /// The rear screen is an *aperture*: narrower than the roof it hangs off,
    /// with a painted pillar down each side of it, recessed between them.
    ///
    /// Widening the glass back out to the bodywork is the edit that turns the
    /// car's whole top surface into one navy pane, so it fails here.
    #[test]
    fn the_backlight_is_a_glazed_aperture_framed_by_sail_panels() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.0, 0.0, 0.0);
        car.pose(&mut app, &pose, 0.0, 0.0);

        let skin = app.get::<Transform>(car.roof).unwrap();
        let glass = app.get::<Transform>(car.backlight).unwrap();
        let left = app.get::<Transform>(car.sails[0]).unwrap();
        let right = app.get::<Transform>(car.sails[1]).unwrap();

        assert!(
            glass.scale.x < skin.scale.x * 0.75,
            "the screen is as wide as the roof: {} vs {}",
            glass.scale.x,
            skin.scale.x
        );
        for (sail, side) in [(&left, -1.0_f32), (&right, 1.0_f32)] {
            let inner = (sail.translation.x - pose.position.x).abs() - sail.scale.x * 0.5;
            assert!(
                (sail.translation.x - pose.position.x).signum() == side,
                "the sails are on the wrong sides"
            );
            assert!(
                (inner - glass.scale.x * 0.5).abs() < 0.02,
                "a sail does not meet the edge of the glass: {inner} vs {}",
                glass.scale.x * 0.5
            );
            assert_eq!(sail.rotation, glass.rotation, "a sail must carry the glass's rake");
            assert!(
                sail.scale.y > glass.scale.y,
                "the glass stands proud of its own frame instead of being recessed in it"
            );
            // Outboard of the glass, but inboard of the flanks: the shoulder
            // still shows past them.
            assert!(
                (sail.translation.x - pose.position.x).abs() + sail.scale.x * 0.5
                    < CAR_WIDTH * 0.5,
                "a sail panel hangs off the side of the car"
            );
        }
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

    /// A tail lamp is a *cluster*: a dark bezel of fixed size holding two thin
    /// lens tubes that clear each other, with the badge in the gap between the
    /// two clusters. Collapsing it back to one glowing block per side — which
    /// is what it was — is exactly the edit that turns the tail back into two
    /// square orange eyes on a black wall, so it fails here.
    #[test]
    fn each_tail_lamp_is_a_bezel_holding_two_lens_tubes_with_a_badge_between_them() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.0, 0.0, 0.0);

        for braking in [0.0, 1.0] {
            car.pose(&mut app, &pose, braking, 0.0);
            let bezel = app.get::<Transform>(car.lamp_bezels[0]).unwrap();
            let upper = app.get::<Transform>(car.brake_lights[0]).unwrap();
            let lower = app.get::<Transform>(car.brake_lights[1]).unwrap();

            assert!(
                (bezel.scale.y - LAMP_BEZEL_HEIGHT).abs() < 1.0e-4,
                "the bezel changes size with the brakes: {}",
                bezel.scale.y
            );
            for tube in [&upper, &lower] {
                assert!(tube.scale.x > tube.scale.y * 4.0, "a lens is a tube, not a patch");
                let inside = (tube.translation.y - bezel.translation.y).abs() + tube.scale.y * 0.5;
                assert!(
                    inside < bezel.scale.y * 0.5,
                    "a lens hangs out of its bezel: {inside} vs {}",
                    bezel.scale.y * 0.5
                );
                // And it stands proud of the bezel it sits in.
                assert!(
                    tube.translation.z - tube.scale.z * 0.5
                        < bezel.translation.z - bezel.scale.z * 0.5,
                    "the lens is sunk into the bezel"
                );
            }
            assert!(
                upper.translation.y - upper.scale.y * 0.5 > lower.translation.y + lower.scale.y * 0.5,
                "the two lenses have merged into one bar at braking {braking}"
            );
        }

        // Badge: round, centred, and filling the gap without touching a lens.
        let badge = app.get::<Transform>(car.badge).unwrap();
        let lamp = app.get::<Transform>(car.brake_lights[0]).unwrap();
        assert!((badge.translation.x - pose.position.x).abs() < 1.0e-4, "on the centreline");
        assert!((badge.scale.x - badge.scale.z).abs() < 1.0e-4, "it is a disc");
        assert!(badge.scale.y < badge.scale.x * 0.5, "and a thin one");
        let lamp_inner = (lamp.translation.x - pose.position.x).abs() - lamp.scale.x * 0.5;
        assert!(
            badge.scale.x * 0.5 < lamp_inner,
            "the badge runs into the lenses: {} vs {lamp_inner}",
            badge.scale.x * 0.5
        );
        assert!(
            badge.scale.x > lamp_inner * 0.8,
            "the badge rattles around in the gap instead of filling it"
        );
        // Its flat face points at the chase camera, not at the sky.
        let axis = badge.rotation.rotate(Vec3::UNIT_Y);
        assert!(axis.z.abs() > 0.99, "the badge is laid face-up: {axis:?}");
    }

    /// The stripes are a pair, they run the length of the car's centre, and each
    /// segment sits *on* the painted panel it belongs to rather than inside it —
    /// and none of them is painted on the glass.
    #[test]
    fn the_twin_stripes_lie_on_the_bonnet_the_roof_the_decklid_and_the_lip() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let car = PlayerCar::install(&mut app, &palette.player_livery());
        let pose = pose_at(0.0, 0.0, 0.0);
        car.pose(&mut app, &pose, 0.0, 0.0);

        let cabin = app.get::<Transform>(car.cabin).unwrap();
        let skin = app.get::<Transform>(car.roof).unwrap();
        let glass = app.get::<Transform>(car.backlight).unwrap();
        let body = app.get::<Transform>(car.body).unwrap();
        let wing = app.get::<Transform>(car.wing).unwrap();
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
        // Bonnet pair: on the bonnet's top face, and lying wholly on it — a
        // stripe that overruns the cowl is painted on thin air.
        let bonnet = app.get::<Transform>(car.bonnet).unwrap();
        let bonnet_top = bonnet.translation.y + bonnet.scale.y * 0.5;
        assert!(
            stripes[0].translation.y - stripes[0].scale.y * 0.5 > bonnet_top
                && stripes[0].translation.y - stripes[0].scale.y * 0.5 < bonnet_top + 0.05,
            "the bonnet stripe floats or sinks: {}",
            stripes[0].translation.y
        );
        assert!(
            stripes[0].translation.z - stripes[0].scale.z * 0.5
                >= bonnet.translation.z - bonnet.scale.z * 0.5 - 1.0e-3
                && stripes[0].translation.z + stripes[0].scale.z * 0.5
                    <= bonnet.translation.z + bonnet.scale.z * 0.5 + 1.0e-3,
            "the bonnet stripe runs off the end of the bonnet: {:?}",
            stripes[0].translation
        );
        // Roof pair: resting on the painted skin's top face, not on the glazing.
        let roof = skin.translation.y + skin.scale.y * 0.5;
        assert!(
            stripes[2].translation.y - stripes[2].scale.y * 0.5 > roof
                && stripes[2].translation.y - stripes[2].scale.y * 0.5 < roof + 0.05,
            "the roof stripe floats or sinks: {}",
            stripes[2].translation.y
        );
        // Decklid pair: on the body's own top face, in the gap the backlight
        // leaves between its foot and the lip.
        let deck = body.translation.y + body.scale.y * 0.5;
        assert!(
            stripes[4].translation.y - stripes[4].scale.y * 0.5 > deck
                && stripes[4].translation.y - stripes[4].scale.y * 0.5 < deck + 0.05,
            "the deck stripe floats or sinks: {}",
            stripes[4].translation.y
        );
        let glass_foot = glass
            .translation
            .subtract(glass.rotation.rotate(Vec3::UNIT_Z).mul_scalar(glass.scale.z * 0.5));
        assert!(
            stripes[4].translation.z + stripes[4].scale.z * 0.5 <= glass_foot.z + 1.0e-3
                && stripes[4].translation.z - stripes[4].scale.z * 0.5
                    >= wing.translation.z + wing.scale.z * 0.5 - 1.0e-3,
            "the deck stripe runs under the glass or over the lip: {:?}",
            stripes[4].translation
        );
        // No segment is painted on the glass, and none carries its rake.
        let square = ChassisBasis::of(&pose).rotation();
        for s in &stripes {
            assert_eq!(s.rotation, square, "a stripe follows a flat painted panel");
            assert_ne!(s.rotation, glass.rotation, "a stripe is painted on the rear screen");
        }
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
