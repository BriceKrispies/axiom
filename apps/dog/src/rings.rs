//! The concentric field of rings: how many rings there are, how many dogs walk
//! each one, where each dog starts, which way round it faces, and which entry of
//! the shared colour palette it is painted from.
//!
//! ## The layout is derived, not typed
//!
//! Nothing here is a hand-tuned ring list or dog count. Four measured facts —
//! how long a dog is, how wide it is, how much air is left behind it, and how far
//! the terrain reaches — produce the whole field from the four ring dials in
//! [`crate::config::SceneConfig`]:
//!
//! * a ring knows its radius, and its dog count is its circumference divided by
//!   the room one dog needs ([`SceneConfig::dog_spacing`]), rounded to a whole
//!   animal;
//! * the rings sit a fixed radial pitch apart, and that pitch has a **floor**
//!   set by the dog's width plus the outward bulge a rigid body makes on a
//!   curve — not by its length;
//! * the innermost radius has a floor of one dog's length, below which the body
//!   is longer than the arc it is standing on;
//! * the ring count has two ceilings: the outermost ring must leave half a dog's
//!   length of clear ground before the terrain's rim, and the whole crowd must
//!   fit the instance pool the live backend was bound with.
//!
//! Move any of those dials and the field re-populates itself with the right
//! number of rings, each holding the right number of dogs, because none of those
//! numbers was ever authored.
//!
//! ## The radial pitch: bounded by width, not length
//!
//! A dog is laid **along** its ring, so what separates two rings is its width,
//! plus one correction. A rigid body of length `L` standing on a circle of
//! radius `R` has its centre on the circle and its nose and tail *outside* it, by
//! `sqrt(R² + (L/2)²) − R` — see [`body_bulge`]. Two rings therefore clear each
//! other when
//!
//! ```text
//! ring_spacing  ≥  dog_width  +  body_bulge(inner radius)  +  air
//! ```
//!
//! and [`ring_spacing`] enforces exactly that, whatever the pitch dial says. At
//! the authored dachshund (24.0 long, 3.84 wide) on a 26-unit innermost ring the
//! requirement is `3.84 + 2.64 + 1.0 = 7.48`, and the pitch dial's default is
//! `7.75`: the opening scene is the scene it has always been, and the floor is
//! only felt when the pitch is dragged under it.
//!
//! ## Which way is which
//!
//! Seen from `+Y` looking down at the `XZ` plane — the way the framing camera
//! sees it — a point at `(R·cos θ, R·sin θ)` traversed with **increasing** `θ`
//! goes **clockwise**. (Take screen-right as `+X` and screen-up as `-Z`, the
//! ordinary map orientation: the point's screen angle is `-θ`, so advancing `θ`
//! turns the short way round the clock.) That single fact is what
//! [`Winding::sign`] encodes, and the direction dial is the whole of what picks
//! one — so reversing it reverses the authored parameter direction, which
//! reverses the tangent, which reverses the facing, with no separate "turn the
//! dogs around" step anywhere.
//!
//! The direction is then testable without trusting any of this prose: for a
//! position `p` measured from the ring centre and a heading `h`, the `y`
//! component of `p × h` is `p.z·h.x − p.x·h.z`, which is `−R²` for a clockwise
//! walk and `+R²` for a counter-clockwise one. `tests/rings.rs` asserts exactly
//! that sign on the real posed bones, at both ends of the direction dial.
//!
//! ## Why the colours come from a bounded palette
//!
//! A dog's colour reaches the GPU **only** through its material: the per-instance
//! `colour[4]` in the instance stream is filled from the material the draw names,
//! and draws batch on the `(mesh_id, material_id)` pair. One material per dog
//! would therefore mean `23 bones × dogs` single-instance batches, which throws
//! instancing away entirely.
//!
//! So the field is painted from a fixed [`PALETTE_SIZE`]-entry palette that every
//! dog shares, and the batch count is `23 × PALETTE_SIZE + 1` **whatever the
//! crowd size**.
//!
//! ## Why a coat is the dog's place in the crowd, and nothing cleverer
//!
//! A dog wears palette entry `crowd index % PALETTE_SIZE` — so the hue advances
//! one twentieth of a turn per dog and each ring reads as a rainbow running
//! round it, with no two dogs adjacent *along* a ring ever sharing a coat
//! (consecutive indices cannot be congruent).
//!
//! That is a deliberately dull rule, and the dullness is the point. The ring
//! dials move the crowd size **live**, which the app pays for by spawning
//! [`MAX_DOGS`] pool slots at bind and retiring the unused ones (see
//! `install.rs`) — and a pool slot's coat is fixed at spawn, because `Material`
//! has no runtime mutation and `Renderable` is not a settable component, so an
//! installed instance can never be repainted. The coats a layout may use are
//! therefore exactly the coats the pool already carries: `slot % PALETTE_SIZE`.
//! Any assignment that is not that balanced sequence — the parity-split hue comb
//! this field used to carry, which gave adjacent *rings* disjoint hue sets —
//! cannot be honoured by a fixed pool, and asking for it would mean either
//! respawning the crowd on every ring-dial tick or an engine-tier per-renderable
//! tint. Both are recorded in `NOTES.md`; neither is an app's errand.

use crate::config::SceneConfig;
use crate::rainbow::hue_to_rgb;
use crate::terrain::TERRAIN_HALF_EXTENT;

/// Which way round a ring is walked, seen from `+Y` looking down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Winding {
    /// Anticlockwise from above.
    CounterClockwise,
    /// Clockwise from above.
    Clockwise,
}

impl Winding {
    /// The sign the ring's authored angle advances with. Increasing angle is a
    /// clockwise walk (see the module note), so counter-clockwise is `-1`.
    pub fn sign(self) -> f32 {
        [-1.0, 1.0][self as usize]
    }

    /// The sign of `(position − centre) × heading` a dog on this ring must
    /// produce. This is the *observable* form of the winding: it is what a test
    /// measures on a posed bone, and it is the inverse of [`Self::sign`]
    /// because the cross product of a radius with a clockwise tangent points
    /// down.
    pub fn cross_sign(self) -> f32 {
        -self.sign()
    }
}

/// One ring of dogs: where it sits in the field, and how wide it is.
///
/// Everything else about it — how many dogs it holds, which palette entries they
/// wear — is derived from those two numbers and the configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ring {
    /// Its place in the field, counting outward from the innermost ring.
    pub index: usize,
    /// Its radius about the scene origin, in world units.
    pub radius: f32,
}

impl Ring {
    /// The ring's circumference — the length of the walk, before the terrain's
    /// relief adds its own fraction of a percent.
    pub fn circumference(self) -> f32 {
        core::f32::consts::TAU * self.radius
    }

    /// How many dogs walk this ring: the circumference divided by the room one
    /// dog needs, rounded to the nearest whole animal.
    ///
    /// Rounding rather than flooring is deliberate — the leftover is shared out
    /// between every gap instead of being dropped into one, so the chain stays
    /// evenly spaced either way. The floor of three keeps the arithmetic from
    /// producing a "ring" of one dog chasing itself; the *ceiling* is that a
    /// rounded-up count must never space the chain tighter than the dog is long,
    /// which is what would make the queue overlap itself.
    pub fn count(self, config: &SceneConfig) -> usize {
        let spacing = config.dog_spacing().max(1.0e-3);
        let rounded = (self.circumference() / spacing).round().max(3.0) as usize;
        let fits = (self.circumference() / config.dog_length().max(1.0e-3)).floor() as usize;
        rounded.min(fits.max(1)).max(1)
    }

    /// How far outside its own circle this ring's dogs reach, in world units:
    /// the nose-and-tail bulge of a rigid body standing on a curve.
    pub fn bulge(self, config: &SceneConfig) -> f32 {
        body_bulge(self.radius, config.dog_length())
    }

    /// The radial band this ring's dogs occupy: `(inner, outer)` world radii,
    /// from the inside flank of a dog to the tip of its outward-bulging nose.
    pub fn band(self, config: &SceneConfig) -> (f32, f32) {
        let half_width = config.dog_width() * 0.5;
        (
            self.radius - half_width,
            self.radius + self.bulge(config) + half_width,
        )
    }

}

/// How far outside a circle of `radius` the ends of a rigid `length` body
/// standing on it reach.
///
/// The body's centre is on the circle and its nose is `length / 2` along the
/// tangent, so the nose sits at `sqrt(radius² + (length/2)²)`. This is the
/// correction that makes a tight ring take more radial room than a wide one, and
/// it is what the ring pitch's floor is sized against.
pub fn body_bulge(radius: f32, length: f32) -> f32 {
    let half = length * 0.5;
    (radius * radius + half * half).sqrt() - radius
}

/// The dog's nose-to-tail length in its own authored units, before the
/// presentation scale. `tests/creatures.rs` measures the real assembled bounds
/// against this number, so it cannot drift away from the animal it describes.
pub const DOG_BODY_LENGTH: f32 = 2.40;

/// The dog's flank-to-flank width in its own authored units — measured, like the
/// length, off the assembled bounds. This is the number the ring pitch's floor is
/// built on, because a dog laid along its ring separates two rings by its width
/// and not by its length.
pub const DOG_BODY_WIDTH: f32 = 0.384;

/// The clear air held between one ring's outermost paw and the next ring's
/// innermost flank, in world units. Below this the chains read as touching even
/// though the arithmetic still clears.
pub const RING_AIR: f32 = 1.0;

/// The most rings the field will lay out — the ceiling the ring-count dial
/// declares, restated here because the layout is what has to honour it.
pub const MAX_RINGS: usize = 10;

/// The most dogs the field will ever hold.
///
/// This is not a taste judgement, it is the **instance pool**. The live backend
/// packs every batch into one buffer sized at bind and silently drops whatever
/// will not fit, so the crowd is capped at a number the app has actually
/// budgeted for: `MAX_DOGS × 23 bones + 1 terrain = 3727` instances, inside the
/// 4096-slot buffer `src/live.rs` asks for. Every scene node is spawned up front
/// and retired with `Visible(false)` rather than despawned, so moving a ring dial
/// costs a visibility write per dog and never a scene rebuild.
///
/// It is a whole number of palettes (`9 × 18`), so the pool carries exactly as
/// many of each coat as of every other — which is what makes "a dog wears the
/// coat of its own crowd index" a rule the pool can always honour.
pub const MAX_DOGS: usize = 9 * PALETTE_SIZE;

/// The innermost ring's radius, in world units — the dial, floored.
///
/// **The floor is a floor, not a preference.** A dog is a rigid body whose paws
/// are planted on the ring itself, and the mismatch between the two grows as the
/// curve tightens: at a radius under one dog's length the body is longer than the
/// arc it is standing on and the geometry stops being meaningful. The second term
/// is the same statement from the crowd's side — a ring has to hold three dogs
/// without them overlapping.
pub fn inner_radius(config: &SceneConfig) -> f32 {
    let by_body = config.dog_length();
    let by_crowd = 3.0 * config.dog_spacing() / core::f32::consts::TAU;
    config.raw(crate::config::Dial::InnerRadius).max(by_body.max(by_crowd))
}

/// The radial pitch between neighbouring rings, in world units — the dial,
/// floored by the width of the animal plus its bulge on the tightest curve plus
/// [`RING_AIR`]. See the module note.
pub fn ring_spacing(config: &SceneConfig) -> f32 {
    config
        .raw(crate::config::Dial::RingSpacing)
        .max(min_ring_spacing(config))
}

/// The tightest pitch two rings may sit at without their dogs intersecting.
pub fn min_ring_spacing(config: &SceneConfig) -> f32 {
    config.dog_width() + body_bulge(inner_radius(config), config.dog_length()) + RING_AIR
}

/// The radius of ring `index`.
pub fn ring_radius(config: &SceneConfig, index: usize) -> f32 {
    inner_radius(config) + index as f32 * ring_spacing(config)
}

/// How many concentric rings the field holds — the dial, capped by the ground
/// there is to stand on and by the instance pool there is to draw with.
pub fn ring_count(config: &SceneConfig) -> usize {
    let asked = (config.raw(crate::config::Dial::RingCount) as usize).clamp(1, MAX_RINGS);
    asked.min(rings_on_the_ground(config)).min(rings_in_the_pool(config)).max(1)
}

/// How many rings fit inside the terrain, leaving half a dog's length of clear
/// ground between the outermost dog's furthest reach and the rim.
///
/// The terrain's top surface is a square of half-extent [`TERRAIN_HALF_EXTENT`]
/// and the skirt hangs straight down from that border, so the usable ground is
/// the inscribed disc — the field has to read as standing *on* a plain, not
/// balanced on its lip.
fn rings_on_the_ground(config: &SceneConfig) -> usize {
    (1..=MAX_RINGS)
        .take_while(|count| {
            let outermost = Ring {
                index: count - 1,
                radius: ring_radius(config, count - 1),
            };
            outermost.band(config).1 + config.dog_length() * 0.5 <= TERRAIN_HALF_EXTENT
        })
        .count()
        .max(1)
}

/// How many rings the instance pool can draw. Rings are counted outward and the
/// first one that would push the crowd past [`MAX_DOGS`] ends the field, so what
/// is drawn is always a whole number of complete rings.
fn rings_in_the_pool(config: &SceneConfig) -> usize {
    (1..=MAX_RINGS)
        .scan(0usize, |total, count| {
            *total += Ring {
                index: count - 1,
                radius: ring_radius(config, count - 1),
            }
            .count(config);
            Some((count, *total))
        })
        .take_while(|(_, total)| *total <= MAX_DOGS)
        .count()
        .max(1)
}

/// Every ring, innermost first.
pub fn rings(config: &SceneConfig) -> Vec<Ring> {
    (0..ring_count(config))
        .map(|index| Ring {
            index,
            radius: ring_radius(config, index),
        })
        .collect()
}

/// How many hue materials the whole field shares.
///
/// This is the app's answer to the batching constraint in the module note: the
/// live backend draws one batch per `(mesh, material)` pair, so the palette size
/// — not the crowd size — sets the draw-call count, at no more than
/// `23 bones × PALETTE_SIZE + 1 terrain = 415`. Eighteen is chosen so each of the
/// two interleaved combs is nine hues (40° apart along a ring) and the combs are
/// 20° apart from each other, which is comfortably past the point where two coats
/// read as the same colour.
pub const PALETTE_SIZE: usize = 18;

/// The linear-RGB coat colour of palette entry `index`.
pub fn palette_color(index: usize) -> [f32; 3] {
    hue_to_rgb(index as f32 / PALETTE_SIZE as f32)
}

/// Every palette entry, in order — the material set `install.rs` registers once
/// and every dog in the field draws from.
pub fn palette() -> Vec<[f32; 3]> {
    (0..PALETTE_SIZE).map(palette_color).collect()
}

/// One dog in the crowd: which ring it walks, where in the chain it is, and
/// which palette entry it is painted from.
///
/// It deliberately carries **no geometry and no colour of its own**. Every dog in
/// the field is the same 23 registered bone meshes drawn again at another
/// transform, in one of [`PALETTE_SIZE`] shared coats — this struct is the whole
/// of what makes one dog different from the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingDog {
    /// Which ring this dog walks: an index into [`rings`].
    pub ring: usize,
    /// Its place in the chain, `0..ring.count(config)`.
    pub slot: usize,
    /// Its coat: an index into the shared palette, `0..PALETTE_SIZE`.
    pub palette: usize,
}

impl RingDog {
    /// Its linear-RGB coat colour — a lookup, not a property of the dog.
    pub fn color(self) -> [f32; 3] {
        palette_color(self.palette)
    }
}

/// Every dog in the field, in spawn order: the innermost ring's chain first,
/// then each ring outward.
///
/// A pure function of the configuration — no clock, no randomness, no
/// environment — so the crowd is byte-identical in every process at a given
/// setting. The final `take` is belt-and-braces on the pool bound the ring count
/// already honours.
pub fn ring_dogs(config: &SceneConfig) -> Vec<RingDog> {
    rings(config)
        .into_iter()
        .flat_map(|ring| {
            (0..ring.count(config)).map(move |slot| (ring.index, slot))
        })
        .take(MAX_DOGS)
        .enumerate()
        .map(|(index, (ring, slot))| RingDog {
            ring,
            slot,
            palette: index % PALETTE_SIZE,
        })
        .collect()
}

/// How many dogs the whole field holds.
pub fn dog_total(config: &SceneConfig) -> usize {
    ring_dogs(config).len()
}

/// The room a *disturbed* crowd moves in: how much ground one dog owns, and how
/// far from the origin it may be pushed.
///
/// Both numbers are derived from the layout rather than typed, for the same
/// reason every other number here is — and in this case for one further reason
/// that is load-bearing. A dog knocked off its track is pulled back to it and
/// shoved away from its neighbours at the same time (see `src/herd.rs`), and
/// those two forces only ever come to rest if the *undisturbed* field is already
/// out of contact. If it were not, every dog would be permanently inside someone
/// else's radius, the push would fight the return forever, and the field would
/// hum instead of settling. So the radius is measured off the spacing the layout
/// actually produced, at whatever the dials say.
/// A dog collides as the shape it is: a **capsule** laid along its own ring —
/// a segment of `2 · half_length`, swept by a radius of `half_width`.
///
/// Not a circle. A dachshund is 24 units long and 3.84 wide, and a circle that
/// fits between two rings 7.75 apart has a radius of 3.5 — so a circular dog is
/// a seventh of its own length, and one dragged along a ring slides clean
/// *through* its neighbours, touching only when the two centres nearly coincide.
/// That is not a tuning problem that a bigger radius fixes: a circle big enough
/// to be the animal is far too big to fit between the rings, and would have the
/// whole field permanently in contact.
///
/// The capsule is what makes the crowd feel solid, and it costs one segment
/// distance instead of one point distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrowdSpace {
    /// Half the length of the capsule's spine, along the dog's heading.
    pub half_length: f32,
    /// The capsule's radius — half the dog across the flanks.
    pub half_width: f32,
    /// The furthest from the origin a dog may be pushed or dragged, in world
    /// units — inside the terrain's rim, with a body's length of margin.
    pub bounds: f32,
}

impl CrowdSpace {
    /// The capsule's full length, nose to tail.
    pub fn length(self) -> f32 {
        self.half_length * 2.0 + self.half_width * 2.0
    }
}

/// The share of a gap a body may fill. Under a half in each axis, so two dogs
/// standing where the layout put them are always clear of one another with air
/// to spare — the property the whole disturbance rests on.
const CROWD_SHARE: f32 = 0.45;

/// [`CrowdSpace`] for this configuration: the dog's own body, clipped by
/// whatever room the layout actually left it.
///
/// Two gaps bound it, and each bounds a different axis of the capsule:
///
/// * **across** — the radial pitch between rings, which two roughly-parallel
///   bodies lie either side of. It caps the *width*.
/// * **along** — the shortest chord between neighbours on one ring (`2R·sin(π/n)`
///   — the straight line, which is what a collider meets; the arc they were
///   spaced along is longer than it). Two dogs on the same ring are nose to tail,
///   so this caps the whole *length*, radius included.
///
/// The clip matters. `Ring::count` guarantees each dog an arc at least its own
/// length, but a chord is shorter than its arc — so an unclipped body would
/// overlap its neighbour at rest on a tight ring, and the push would fight the
/// return forever. At the authored dachshund the clip is barely felt: the
/// capsule comes out 23.8 units long against a 24-unit dog.
pub fn crowd_space(config: &SceneConfig) -> CrowdSpace {
    let laid = rings(config);
    let along = laid
        .iter()
        .map(|ring| chord(*ring, ring.count(config)))
        .fold(f32::INFINITY, f32::min);
    let across = [f32::INFINITY, ring_spacing(config)][usize::from(laid.len() > 1)];
    let half_width = (config.dog_width() * 0.5).min(CROWD_SHARE * across);
    CrowdSpace {
        half_width,
        half_length: (config.dog_length() * 0.5 - half_width)
            .min(CROWD_SHARE * along - half_width)
            .max(0.0),
        bounds: TERRAIN_HALF_EXTENT - config.dog_length() * 0.5,
    }
}

/// The straight-line distance between two of `count` dogs spaced evenly around
/// `ring` — the chord under the arc they were laid out along.
fn chord(ring: Ring, count: usize) -> f32 {
    2.0 * ring.radius * (core::f32::consts::PI / count.max(1) as f32).sin()
}

/// The clear ground between the outermost dog's furthest reach and the terrain's
/// rim, in world units. Positive by construction — see [`rings_on_the_ground`].
pub fn outer_clearance(config: &SceneConfig) -> f32 {
    let outer = rings(config);
    let last = outer.last().copied().unwrap_or(Ring {
        index: 0,
        radius: inner_radius(config),
    });
    TERRAIN_HALF_EXTENT - last.band(config).1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Dial;

    #[test]
    fn the_authored_field_is_the_field_this_app_has_always_shown() {
        let config = SceneConfig::defaults();
        assert_eq!(config.dog_length(), 24.0);
        assert!((config.dog_width() - 3.84).abs() < 1.0e-4);
        assert_eq!(config.dog_spacing(), 25.5);
        assert_eq!(ring_count(&config), 8);
        assert_eq!(inner_radius(&config), 26.0);
        assert_eq!(ring_spacing(&config), 7.75);
        assert_eq!(
            rings(&config)
                .iter()
                .map(|ring| ring.count(&config))
                .collect::<Vec<usize>>(),
            vec![6, 8, 10, 12, 14, 16, 18, 20]
        );
        assert_eq!(dog_total(&config), 104);
    }

    #[test]
    fn no_two_rings_intersect_at_any_setting_of_the_ring_dials() {
        // Both ends of every ring dial, and the dog-size dial that scales them
        // all, in every combination — the pitch floor has to hold across the lot.
        for size in [6.0, 10.0, 16.0] {
            for inner in [18.0, 26.0, 60.0] {
                for pitch in [3.0, 7.75, 20.0] {
                    for gap in [0.5, 1.5, 20.0] {
                        let config = SceneConfig::defaults()
                            .with(Dial::DogSize, size)
                            .with(Dial::InnerRadius, inner)
                            .with(Dial::RingSpacing, pitch)
                            .with(Dial::RingCount, MAX_RINGS as f32)
                            .with(Dial::DogGap, gap);
                        let laid = rings(&config);
                        assert!(!laid.is_empty());
                        laid.windows(2).for_each(|pair| {
                            let air = pair[1].band(&config).0 - pair[0].band(&config).1;
                            assert!(
                                air > 0.0,
                                "size {size} inner {inner} pitch {pitch} gap {gap}: \
                                 rings {} and {} overlap by {air}",
                                pair[0].index,
                                pair[1].index
                            );
                        });
                        // ...and the outermost stays on the ground.
                        assert!(
                            outer_clearance(&config) >= 0.0,
                            "size {size} inner {inner} pitch {pitch}: the field leaves the terrain"
                        );
                        // ...and the crowd never outgrows the pool.
                        assert!(dog_total(&config) <= MAX_DOGS);
                    }
                }
            }
        }
    }

    #[test]
    fn no_ring_ever_packs_its_dogs_closer_than_the_dog_is_long() {
        for size in [6.0, 10.0, 16.0] {
            for gap in [0.5, 1.5, 20.0] {
                for inner in [18.0, 26.0, 60.0] {
                    let config = SceneConfig::defaults()
                        .with(Dial::DogSize, size)
                        .with(Dial::DogGap, gap)
                        .with(Dial::InnerRadius, inner)
                        .with(Dial::RingCount, MAX_RINGS as f32);
                    for ring in rings(&config) {
                        let count = ring.count(&config);
                        let arc = ring.circumference() / count as f32;
                        assert!(
                            arc >= config.dog_length(),
                            "size {size} gap {gap} inner {inner}: ring {} gives each of its \
                             {count} dogs {arc} units of arc for a {}-unit body",
                            ring.index,
                            config.dog_length()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_dogs_own_room_is_always_smaller_than_the_gap_the_layout_left_it() {
        // The claim the whole disturbance rests on: at every legal dial setting,
        // two dogs standing where the layout put them are outside each other's
        // radius. `tests/herd.rs` makes the same check against the real posed
        // positions; this one is the arithmetic behind it.
        for size in [6.0, 10.0, 16.0] {
            for inner in [18.0, 26.0, 60.0] {
                for pitch in [3.0, 7.75, 20.0] {
                    for count in [1.0, 4.0, MAX_RINGS as f32] {
                        for gap in [0.5, 1.5, 20.0] {
                            let config = SceneConfig::defaults()
                                .with(Dial::DogSize, size)
                                .with(Dial::InnerRadius, inner)
                                .with(Dial::RingSpacing, pitch)
                                .with(Dial::RingCount, count)
                                .with(Dial::DogGap, gap);
                            let space = crowd_space(&config);
                            let laid = rings(&config);
                            let along = laid
                                .iter()
                                .map(|ring| chord(*ring, ring.count(&config)))
                                .fold(f32::INFINITY, f32::min);
                            // Nose to tail along a ring: the whole capsule fits
                            // inside the chord between two neighbours.
                            assert!(
                                space.length() < along,
                                "size {size} inner {inner} pitch {pitch} rings {count} gap {gap}: \
                                 a {} body does not fit a {along} chord",
                                space.length()
                            );
                            // Flank to flank across rings: two widths fit the
                            // pitch.
                            assert!(
                                space.half_width > 0.0
                                    && (laid.len() < 2
                                        || space.half_width * 2.0 < ring_spacing(&config)),
                                "a {} width does not fit a {} pitch",
                                space.half_width * 2.0,
                                ring_spacing(&config)
                            );
                            // The body is never *bigger* than the animal it
                            // stands for, whatever the field's scale.
                            assert!(space.length() <= config.dog_length() + 1.0e-3);
                            assert!(space.half_width * 2.0 <= config.dog_width() + 1.0e-3);
                            // ...and the ground it may be pushed across holds
                            // every ring the layout laid, so containment never
                            // fights the walk itself.
                            assert!(
                                space.bounds > laid.last().map(|ring| ring.radius).unwrap_or(0.0),
                                "the outermost ring is outside the room the crowd is given"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_palette_is_bounded_balanced_and_no_neighbour_shares_a_coat() {
        for rings_asked in 1..=MAX_RINGS {
            for gap in [0.5, 1.5, 12.0] {
                let config = SceneConfig::defaults()
                    .with(Dial::RingCount, rings_asked as f32)
                    .with(Dial::DogGap, gap);
                let dogs = ring_dogs(&config);
                assert!(dogs.iter().all(|dog| dog.palette < PALETTE_SIZE));
                // Balanced: every coat is worn within one dog of every other,
                // which is exactly what the fixed-coat instance pool carries.
                let worn: Vec<usize> = (0..PALETTE_SIZE)
                    .map(|coat| dogs.iter().filter(|dog| dog.palette == coat).count())
                    .collect();
                let most = worn.iter().copied().max().unwrap_or(0);
                let least = worn.iter().copied().min().unwrap_or(0);
                assert!(most - least <= 1, "{worn:?} is not a balanced palette");
                // No two dogs adjacent along a ring share a coat, including the
                // pair that closes the chain.
                rings(&config).iter().for_each(|ring| {
                    let chain: Vec<usize> = dogs
                        .iter()
                        .filter(|dog| dog.ring == ring.index)
                        .map(|dog| dog.palette)
                        .collect();
                    // A ring reduced to a single dog has no neighbouring pair.
                    (0..chain.len() * usize::from(chain.len() > 1)).for_each(|slot| {
                        assert_ne!(
                            chain[slot],
                            chain[(slot + 1) % chain.len()],
                            "ring {} repeats a coat at slot {slot}",
                            ring.index
                        );
                    });
                });
                assert_eq!(dogs, ring_dogs(&config));
            }
        }
    }

    #[test]
    fn the_direction_dial_is_the_only_thing_that_picks_a_winding() {
        let config = SceneConfig::defaults();
        assert_eq!(config.winding(), Winding::CounterClockwise);
        assert_eq!(
            config.with(Dial::Direction, -1.0).winding(),
            Winding::Clockwise
        );
        assert_eq!(
            Winding::CounterClockwise.cross_sign(),
            -Winding::CounterClockwise.sign()
        );
        assert_eq!(Winding::Clockwise.cross_sign(), -Winding::Clockwise.sign());
    }
}
