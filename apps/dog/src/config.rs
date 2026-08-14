//! [`Dial`] and [`SceneConfig`]: the scene's live parameters, as **one value**.
//!
//! Everything the page's slider panel can move lives here, and it lives here as
//! data rather than as a mutable global. A [`SceneConfig`] is a plain `Copy`
//! value carrying one raw number per [`Dial`]; every consumer — the ring layout,
//! the gait, the walk, the install pass — takes it by reference and reads
//! *derived* accessors off it. There is no cell, no static, and no hidden state:
//! hand the same config and the same tick to the animation twice and you get the
//! same pose, which is exactly the property the determinism tests rest on.
//!
//! ## Raw dials in, derived quantities out
//!
//! A dial's raw value is what the user asked for. It is clamped to the dial's own
//! declared range the moment it is written ([`SceneConfig::with`]), and it is
//! clamped **again**, against the rest of the configuration, by the accessor that
//! reads it. That second clamp is where the scene is kept legal:
//!
//! * the crouch cannot exceed the height the shoulder actually stands at, or the
//!   body would sink through its own paws;
//! * the paw lift cannot exceed a fraction of the leg that has to swing it;
//! * the **stride** cannot exceed the swing room the leg has left after the body
//!   is stood up — which is a function of the leg length, the dog's size, the
//!   crouch, the terrain relief the feet may follow and the curve correction the
//!   innermost ring imposes. `tests/dials.rs` and `tests/locomotion.rs` measure
//!   the real posed limbs at both ends of every dial, so a ceiling that is too
//!   generous fails there rather than on the page.
//!
//! The field's own clamps (a ring pitch that cannot be tighter than the dogs are
//! wide, a ring count that cannot walk off the terrain or past the instance pool)
//! live next to the layout they constrain, in [`crate::rings`].
//!
//! ## Why the dial table is a table
//!
//! The panel on the page is **generated** from [`Dial::ALL`] — label, range, step
//! and default all come from here — so a slider cannot exist without a dial
//! behind it and a dial cannot exist without a slider in front of it. The same
//! table encodes the query string, which is what lets a resize-triggered reload
//! (and the one non-live dial, `detail`) put the scene back exactly as it was.

use crate::creature_dog::{front_hip_drop, front_leg_reach};
use crate::creature_pose::{Gait, DOG_GAIT};
use crate::rings::{inner_radius, Winding};
use crate::variant::SceneVariant;

/// One live parameter of the scene — one slider on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dial {
    /// World units a dog covers per engine tick.
    Speed,
    /// Which way round the rings are walked: `-1` clockwise, `+1` anticlockwise.
    Direction,
    /// One full step, in world units.
    Stride,
    /// Peak height of the swinging paw's arc, in world units.
    Lift,
    /// The fraction of a step a paw spends on the ground.
    Duty,
    /// How far the body is carried below its standing height, in world units.
    Crouch,
    /// Peak vertical bob, in world units.
    Bob,
    /// Steady forward pitch, in radians.
    Lean,
    /// The leg's length as a multiple of the authored dachshund leg.
    LegLength,
    /// The uniform world scale a dog is presented at.
    DogSize,
    /// The innermost ring's radius, in world units.
    InnerRadius,
    /// The radial pitch between neighbouring rings, in world units.
    RingSpacing,
    /// How many concentric rings the field holds.
    RingCount,
    /// The clear air between one dog's tail and the next dog's nose, which is
    /// what decides how many dogs a ring holds.
    DogGap,
    /// Tessellation density. The **one** dial that is not live: mesh geometry is
    /// uploaded once at bind, so changing it reloads the page (see `NOTES.md`).
    Detail,
}

/// How many dials there are.
pub const DIAL_COUNT: usize = 15;

/// One dial's metadata: everything the page needs to draw a slider for it, and
/// everything the query string needs to round-trip it.
#[derive(Debug, Clone, Copy)]
pub struct DialSpec {
    /// The short stable identifier: the DOM `data-dial` attribute and the query
    /// parameter name.
    pub key: &'static str,
    /// The human label printed beside the slider.
    pub label: &'static str,
    /// The lowest value the slider offers.
    pub min: f32,
    /// The highest value the slider offers.
    pub max: f32,
    /// The slider's granularity.
    pub step: f32,
    /// The value the opening scene is built at.
    pub default: f32,
    /// How many decimals the numeric read-out prints.
    pub decimals: usize,
    /// Whether moving it re-poses the running scene (`true`) or needs a reload
    /// (`false` — geometry, which is uploaded once at bind).
    pub live: bool,
}

/// Every dial, in panel order.
const DIALS: [DialSpec; DIAL_COUNT] = [
    DialSpec { key: "speed", label: "walk speed", min: 0.0, max: 0.60, step: 0.01, default: 0.21, decimals: 2, live: true },
    DialSpec { key: "dir", label: "direction", min: -1.0, max: 1.0, step: 2.0, default: 1.0, decimals: 0, live: true },
    DialSpec { key: "stride", label: "stride", min: 1.5, max: 9.0, step: 0.1, default: 5.2, decimals: 1, live: true },
    DialSpec { key: "lift", label: "paw lift", min: 0.0, max: 1.5, step: 0.05, default: 0.45, decimals: 2, live: true },
    DialSpec { key: "duty", label: "stance duty", min: 0.30, max: 0.90, step: 0.01, default: 0.52, decimals: 2, live: true },
    DialSpec { key: "crouch", label: "crouch", min: 0.0, max: 2.5, step: 0.05, default: 0.40, decimals: 2, live: true },
    DialSpec { key: "bob", label: "body bob", min: 0.0, max: 0.50, step: 0.01, default: 0.09, decimals: 2, live: true },
    DialSpec { key: "lean", label: "lean", min: -0.30, max: 0.30, step: 0.01, default: -0.04, decimals: 2, live: true },
    // The floor is measured, not chosen. Below ~0.70 the hind chain cannot absorb
    // the terrain's roll across a wheelbase that does not shrink with the leg,
    // however far the stride is wound back — `tests/locomotion.rs` walks the
    // whole field at both ends of this dial and holds it to the same reach bar
    // the authored dog meets.
    DialSpec { key: "leg", label: "leg length", min: 0.70, max: 1.80, step: 0.05, default: 1.0, decimals: 2, live: true },
    DialSpec { key: "size", label: "dog size", min: 6.0, max: 16.0, step: 0.5, default: 10.0, decimals: 1, live: true },
    DialSpec { key: "inner", label: "inner radius", min: 18.0, max: 60.0, step: 0.25, default: 26.0, decimals: 2, live: true },
    DialSpec { key: "pitch", label: "ring spacing", min: 3.0, max: 20.0, step: 0.25, default: 7.75, decimals: 2, live: true },
    DialSpec { key: "rings", label: "ring count", min: 1.0, max: 10.0, step: 1.0, default: 8.0, decimals: 0, live: true },
    DialSpec { key: "gap", label: "nose-to-tail gap", min: 0.5, max: 20.0, step: 0.5, default: 1.5, decimals: 1, live: true },
    DialSpec { key: "detail", label: "detail (reloads)", min: 0.0, max: 2.0, step: 1.0, default: 1.0, decimals: 0, live: false },
];

impl Dial {
    /// Every dial, in panel order.
    pub const ALL: [Dial; DIAL_COUNT] = [
        Dial::Speed,
        Dial::Direction,
        Dial::Stride,
        Dial::Lift,
        Dial::Duty,
        Dial::Crouch,
        Dial::Bob,
        Dial::Lean,
        Dial::LegLength,
        Dial::DogSize,
        Dial::InnerRadius,
        Dial::RingSpacing,
        Dial::RingCount,
        Dial::DogGap,
        Dial::Detail,
    ];

    /// This dial's metadata.
    pub fn spec(self) -> DialSpec {
        DIALS[self as usize]
    }

    /// The dial whose [`DialSpec::key`] is `key`.
    pub fn from_key(key: &str) -> Option<Dial> {
        Dial::ALL.into_iter().find(|dial| dial.spec().key == key)
    }
}

/// Every live parameter of the scene, as one value.
///
/// Constructed from [`SceneConfig::defaults`] and moved a dial at a time. The
/// opening scene is `defaults()`, so the page a visitor lands on is the scene
/// this app has always presented.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneConfig {
    raw: [f32; DIAL_COUNT],
}

impl SceneConfig {
    /// The authored scene: every dial at its default.
    pub fn defaults() -> SceneConfig {
        let mut raw = [0.0; DIAL_COUNT];
        Dial::ALL
            .into_iter()
            .for_each(|dial| raw[dial as usize] = dial.spec().default);
        SceneConfig { raw }
    }

    /// What the user asked for on `dial` — the number the slider shows.
    pub fn raw(&self, dial: Dial) -> f32 {
        self.raw[dial as usize]
    }

    /// This config with `dial` moved to `value`, snapped to the dial's step and
    /// clamped to its declared range. A non-finite value leaves the dial alone.
    pub fn with(mut self, dial: Dial, value: f32) -> SceneConfig {
        let spec = dial.spec();
        // Snapped from the dial's own floor, so the detents are the ones the
        // page's `<input type=range min step>` offers, and then rounded to the
        // dial's printed precision — otherwise `0.4` snaps to `0.39999998` and
        // the read-out, the query string and the equality checks all disagree
        // with each other about the same slider.
        let detents = ((value - spec.min) / spec.step).round();
        let snapped = spec.min + detents * spec.step;
        let scale = 10.0_f32.powi(spec.decimals as i32);
        let taken = ((snapped * scale).round() / scale).clamp(spec.min, spec.max);
        self.raw[dial as usize] = [self.raw[dial as usize], taken][usize::from(taken.is_finite())];
        self
    }

    /// Move `dial` in place. See [`SceneConfig::with`].
    pub fn set(&mut self, dial: Dial, value: f32) {
        *self = self.with(dial, value);
    }

    /// Read a whole config out of a `key=value&…` query string. Anything absent,
    /// unrecognised or unparsable keeps its default.
    pub fn from_query(query: &str) -> SceneConfig {
        query
            .trim_start_matches('?')
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .fold(SceneConfig::defaults(), |config, (key, value)| {
                match (Dial::from_key(key), value.parse::<f32>()) {
                    (Some(dial), Ok(number)) => config.with(dial, number),
                    _ => config,
                }
            })
    }

    /// This config as a query string — only the dials that have been moved, so a
    /// scene at its defaults has a clean URL. Round-trips through
    /// [`SceneConfig::from_query`].
    pub fn to_query(&self) -> String {
        Dial::ALL
            .into_iter()
            .filter(|dial| self.raw(*dial) != dial.spec().default)
            .map(|dial| {
                let spec = dial.spec();
                format!("{}={:.*}", spec.key, spec.decimals, self.raw(dial))
            })
            .collect::<Vec<String>>()
            .join("&")
    }

    /// Whether two configs differ in any dial that re-poses the running scene.
    /// The one non-live dial (`detail`) is excluded: it is answered by a reload,
    /// not by a re-pose.
    pub fn live_differs(&self, other: &SceneConfig) -> bool {
        Dial::ALL
            .into_iter()
            .filter(|dial| dial.spec().live)
            .any(|dial| self.raw(dial) != other.raw(dial))
    }

    // ---- derived: the whole animal -----------------------------------------

    /// The tessellation density the geometry is built at.
    pub fn variant(&self) -> SceneVariant {
        SceneVariant::from_index(self.raw(Dial::Detail) as usize)
    }

    /// World units a dog covers per engine tick.
    pub fn travel_per_tick(&self) -> f32 {
        self.raw(Dial::Speed)
    }

    /// Which way round the rings are walked.
    pub fn winding(&self) -> Winding {
        [Winding::Clockwise, Winding::CounterClockwise][usize::from(self.raw(Dial::Direction) > 0.0)]
    }

    /// The uniform world scale a dog is presented at.
    pub fn dog_scale(&self) -> f32 {
        self.raw(Dial::DogSize)
    }

    /// The leg's length as a multiple of the authored dachshund leg, floored so
    /// the leg is never shorter than [`MIN_LEG_SPAN`] in **world** units.
    ///
    /// This is the one place two dials genuinely constrain each other. The leg
    /// dial and the size dial both shorten a leg, and multiplied together they
    /// reach a length no stride reduction can rescue: the ground's roll across a
    /// wheelbase is a property of the heightfield, not of the animal, so past a
    /// point the leg simply cannot span it and the paw comes off its plant. The
    /// floor is measured (`tests/locomotion.rs` holds every corner of the dial
    /// space to the authored dog's own reach bar), and it derives rather than
    /// forbids: a 6-unit dog is given legs at their authored proportion instead
    /// of being refused.
    pub fn leg_scale(&self) -> f32 {
        let spec = Dial::LegLength.spec();
        let floor = (MIN_LEG_SPAN / self.dog_scale()).clamp(spec.min, spec.max);
        self.raw(Dial::LegLength).max(floor)
    }

    /// The world scale a **leg bone** is drawn along its own length at — the
    /// product of the presentation scale and the leg dial, and the same number
    /// the inverse-kinematic solver is handed. Keeping them one expression is
    /// what stops a limb that *looks* longer from solving at its old length.
    pub fn leg_span(&self) -> f32 {
        self.dog_scale() * self.leg_scale()
    }

    /// The dog's world-space nose-to-tail length.
    pub fn dog_length(&self) -> f32 {
        crate::rings::DOG_BODY_LENGTH * self.dog_scale()
    }

    /// The dog's world-space flank-to-flank width.
    pub fn dog_width(&self) -> f32 {
        crate::rings::DOG_BODY_WIDTH * self.dog_scale()
    }

    /// The clear air between one dog's tail and the next dog's nose.
    pub fn dog_gap(&self) -> f32 {
        self.raw(Dial::DogGap)
    }

    /// The arc one dog occupies on its ring: its own length plus the gap behind
    /// it. This is what decides how many dogs a ring holds.
    pub fn dog_spacing(&self) -> f32 {
        self.dog_length() + self.dog_gap()
    }

    /// The fore-aft span between the front and hind contacts, in world units.
    pub fn wheelbase(&self) -> f32 {
        crate::creature_dog::wheelbase_local() * self.dog_scale()
    }

    // ---- derived: the gait, every dial clamped against the leg --------------

    /// The whole trot, with every dial resolved against the leg that has to walk
    /// it. This is the one value the pose pass reads.
    pub fn gait(&self) -> Gait {
        Gait {
            scale: self.dog_scale(),
            leg_scale: self.leg_scale(),
            stride: self.stride(),
            duty: self.raw(Dial::Duty),
            // **The lead is half the duty, always.** A foot plants `lead · stride`
            // ahead of its hip and is then left behind for the whole stance, so
            // it ends `(duty − lead) · stride` behind it: only at `duty / 2` is
            // that excursion symmetric, and anywhere else one end of it is the
            // longer and decides the reach. The authored gait already sits there
            // (`0.26` against a `0.52` duty), so deriving it changes nothing at
            // the defaults and stops the duty dial from quietly dragging the
            // stance backwards out of the leg's reach.
            lead: self.raw(Dial::Duty) * 0.5,
            lift: self.lift(),
            crouch: self.crouch(),
            bob: self.bob(),
            relief: self.relief(),
            lean: self.raw(Dial::Lean),
            pitch_swing: DOG_GAIT.pitch_swing,
            terrain_pitch: DOG_GAIT.terrain_pitch,
            flex: DOG_GAIT.flex,
        }
    }

    /// One full step, in world units — the requested stride or the longest one
    /// the leg can pay for, whichever is shorter.
    pub fn stride(&self) -> f32 {
        self.raw(Dial::Stride).min(self.stride_ceiling())
    }

    /// How far the body is carried below its standing height. Capped well inside
    /// the shoulder height, so the barrel can never be driven through its own
    /// paws.
    pub fn crouch(&self) -> f32 {
        self.raw(Dial::Crouch)
            .min(CROUCH_OF_DROP * front_hip_drop() * self.leg_span())
    }

    /// Peak height of the swinging paw's arc. Capped against the leg that lifts
    /// it, so a short leg cannot be asked to raise a paw past its own knee.
    pub fn lift(&self) -> f32 {
        self.raw(Dial::Lift)
            .min(LIFT_OF_REACH * front_leg_reach() * self.leg_span())
    }

    /// Peak vertical bob. Capped against the shoulder height for the same reason
    /// the crouch is.
    pub fn bob(&self) -> f32 {
        self.raw(Dial::Bob)
            .min(BOB_OF_DROP * front_hip_drop() * self.leg_span())
    }

    /// How far above or below its own body's line a paw may be set down. Not a
    /// dial: it is a property of the leg, so it scales with the leg and with the
    /// animal.
    pub fn relief(&self) -> f32 {
        DOG_GAIT.relief * self.leg_span() / DOG_GAIT.scale
    }

    /// The longest stride the leg has room for, in world units.
    ///
    /// Stated as a **ratio against the authored dog**, not as an absolute
    /// formula: the authored gait is measured (`tests/locomotion.rs` walks every
    /// limb of every dog for more than a lap and reports the worst reach as 86%
    /// of the leg), so scaling the authored stride by how the swing room moved
    /// carries that measurement to every other configuration instead of
    /// re-deriving it from scratch and hoping. At the defaults the ratio is
    /// exactly one, so the opening scene is never clamped.
    fn stride_ceiling(&self) -> f32 {
        let authored = SceneConfig::defaults();
        let room = (self.swing_room() / authored.swing_room()).max(0.0);
        // A foot is planted for `duty · stride` of travel, so the excursion it
        // makes about its own hip is `duty · stride / 2` — the stride the leg can
        // pay for is inversely proportional to the duty. At the authored duty
        // this term is exactly one.
        let dwell = authored.raw(Dial::Duty) / self.raw(Dial::Duty).max(0.05);
        // **The terrain does not shrink with the dog.** A leg shorter than the
        // authored one — because the leg dial shortened it, or because the whole
        // animal did — still has to absorb the ground's roll across its own
        // wheelbase, and that roll is a property of the heightfield rather than
        // of the animal standing on it. So a leg at a fraction of the authored
        // span takes a proportionally shorter step, over and above what the swing
        // room alone predicts. It is exactly one at the authored size, so the
        // opening scene is untouched; `tests/locomotion.rs` walks the whole dial
        // space and this is the factor that keeps the short corners inside the
        // same reach bar the authored dog meets.
        let ground = (self.leg_span() / DOG_GAIT.scale).min(1.0);
        DOG_GAIT.stride * room * dwell * ground
    }

    /// The horizontal room a leg has left once the body is stood up on it, the
    /// terrain it may follow is allowed for, and the tightest ring's curve
    /// correction is paid — in world units.
    fn swing_room(&self) -> f32 {
        let span = self.leg_span();
        let reach = front_leg_reach() * span;
        let drop = (front_hip_drop() * span - self.crouch()).max(0.05 * reach);
        let arc = ((REACH_BUDGET * reach).powi(2) - drop * drop)
            .max(0.0)
            .sqrt();
        (arc - self.curve_offset() - self.relief()).max(0.0)
    }

    /// How far outside the circle its own paw is planted on a shoulder sits, on
    /// the tightest ring in the field — the correction a rigid body standing on
    /// a curve costs the leg that has to absorb it.
    fn curve_offset(&self) -> f32 {
        let radius = inner_radius(self);
        let half = self.wheelbase() * 0.5;
        (radius * radius + half * half).sqrt() - radius
    }
}

/// The fraction of its own length a leg is allowed to be extended to. The
/// authored gait measures 86% at its worst over a lap; the reach test bars 97%,
/// and this sits between them so a re-scaled configuration keeps the same margin
/// the authored one has.
const REACH_BUDGET: f32 = 0.93;

/// How much of the shoulder height the crouch may eat.
const CROUCH_OF_DROP: f32 = 0.60;

/// How much of the shoulder height the bob may swing through.
const BOB_OF_DROP: f32 = 0.15;

/// How much of its own leg a paw may be lifted by.
const LIFT_OF_REACH: f32 = 0.40;

/// The shortest leg, in **world** units, that can still walk this terrain — the
/// floor [`SceneConfig::leg_scale`] holds the leg and size dials to together.
const MIN_LEG_SPAN: f32 = 6.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_dial_has_a_key_a_range_and_a_default_inside_it() {
        let mut keys: Vec<&str> = Dial::ALL.into_iter().map(|d| d.spec().key).collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "two dials share a key");
        for dial in Dial::ALL {
            let spec = dial.spec();
            assert!(spec.min < spec.max, "{} has an empty range", spec.key);
            assert!(spec.step > 0.0, "{} has no step", spec.key);
            assert!(
                (spec.min..=spec.max).contains(&spec.default),
                "{}'s default {} is outside {}..{}",
                spec.key,
                spec.default,
                spec.min,
                spec.max
            );
            assert_eq!(Dial::from_key(spec.key), Some(dial));
        }
        assert_eq!(Dial::from_key("nonsense"), None);
    }

    #[test]
    fn a_dial_clamps_to_its_range_and_snaps_to_its_step() {
        let config = SceneConfig::defaults();
        for dial in Dial::ALL {
            let spec = dial.spec();
            assert_eq!(config.with(dial, spec.min - 1000.0).raw(dial), spec.min);
            assert_eq!(config.with(dial, spec.max + 1000.0).raw(dial), spec.max);
            // A non-finite write is refused rather than poisoning the scene.
            assert_eq!(config.with(dial, f32::NAN).raw(dial), spec.default);
        }
        // The step really snaps: a value between two detents lands on one.
        let snapped = config.with(Dial::RingCount, 4.4).raw(Dial::RingCount);
        assert_eq!(snapped, 4.0);
    }

    #[test]
    fn the_query_string_round_trips_every_moved_dial() {
        assert_eq!(SceneConfig::defaults().to_query(), "");
        let moved = SceneConfig::defaults()
            .with(Dial::Speed, 0.4)
            .with(Dial::RingCount, 3.0)
            .with(Dial::Direction, -1.0);
        let query = moved.to_query();
        assert_eq!(query, "speed=0.40&dir=-1&rings=3", "{query}");
        assert_eq!(SceneConfig::from_query(&query), moved);
        assert_eq!(SceneConfig::from_query(&format!("?{query}")), moved);
        // Junk is ignored, not fatal.
        assert_eq!(
            SceneConfig::from_query("nope=1&speed=oops&rings="),
            SceneConfig::defaults()
        );
    }

    #[test]
    fn the_defaults_are_never_clamped_by_their_own_ceilings() {
        let config = SceneConfig::defaults();
        assert_eq!(config.stride(), DOG_GAIT.stride);
        assert_eq!(config.crouch(), DOG_GAIT.crouch);
        assert_eq!(config.lift(), DOG_GAIT.lift);
        assert_eq!(config.bob(), DOG_GAIT.bob);
        assert!((config.relief() - DOG_GAIT.relief).abs() < 1.0e-6);
        assert_eq!(config.dog_scale(), DOG_GAIT.scale);
        assert_eq!(config.winding(), Winding::CounterClockwise);
        assert_eq!(config.variant(), SceneVariant::Base);
    }

    #[test]
    fn only_a_live_dial_counts_as_a_live_difference() {
        let base = SceneConfig::defaults();
        assert!(!base.live_differs(&base));
        assert!(base.live_differs(&base.with(Dial::Speed, 0.5)));
        assert!(!base.live_differs(&base.with(Dial::Detail, 2.0)));
    }
}
