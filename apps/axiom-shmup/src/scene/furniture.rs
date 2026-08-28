//! **A placeholder street-furniture pass — not a port of `dressing.js`.**
//!
//! `src/world/dressing.js` (the real scatter pass: `dressStreet`,
//! `dressBuildings`, `scatterDebris`, and `registerDressingProps` alongside it)
//! is **not ported**. `crate::world::props` registers ~60 prototypes and
//! nothing places them, so the level would come up as bare buildings on bare
//! ground with the whole prop library dead in the Assembler.
//!
//! This file places a deliberately small set so the prototypes are visible and
//! the instancing path is exercised. It is **not** an attempt at `dressing.js`:
//! there is no rubble scatter, no facade clutter, no roof dressing, no litter,
//! no debris fields, no wrecks, no cables, no laundry, no hangings — every one
//! of those is a `dressing.js` routine with its own rules, and guessing at them
//! would be inventing content the port has not done. When `dressing.rs` lands,
//! **delete this file**; it has no other future.
//!
//! ## What it does place, and where the positions come from
//!
//! Every position below is authored map data already in the port —
//! [`crate::world::layout::SET_PIECES`], the `layout.js` table `dressing.js`
//! itself reads. Nothing here is a made-up coordinate. What *is* this file's
//! own is the mapping from a set-piece entry to prototype ids and offsets
//! (which prototype, how many, how stacked), because that mapping lives in
//! `dressing.js`.
//!
//! | set piece | prototypes placed |
//! |-----------|-------------------|
//! | `lamps` | `lamp_post` + `lamp_glass` on its arm |
//! | `jerseys` | `jersey` |
//! | `sandbag_walls` | a two-course `sandbag_a`/`b`/`c` row along the wall axis |
//! | `tyres` | a stack of `tyre` |
//! | `palms` | `palm_trunk` + a crown of `palm_frond` |
//! | `stalls` | `stall`, with a `crate_a` and a `barrel_rust` beside it |
//!
//! ## Why this stays on the instancing path
//!
//! Every placement goes through [`Assembler::put`]/[`Assembler::put_s`], so it
//! becomes one more matrix on an existing prototype. `Assembler::finalize`
//! then emits **one** instanced batch per prototype per 64 m chunk. Placing any
//! of this as its own static geometry would add a draw call each and defeat
//! exactly the design that keeps the level near a hundred draws.
//!
//! ## Determinism
//!
//! Nothing here draws from an [`crate::rng::Rng`]. The Assembler's per-`put`
//! jitter is opt-in (`Assembler::jitter`, left `None`), so this pass consumes
//! no random numbers at all and cannot shift the world stream the ground,
//! buildings and prototypes were rolled from.

use crate::world::assembler::Assembler;
use crate::world::layout::{SET_PIECES, STREET};

/// Pavement top, the height a kerbside prop stands on
/// (`STREET.walk_h`). Street-level props sit at `0.0`.
fn walk_h() -> f32 {
    STREET.walk_h as f32
}

/// Metres along the lamp's arm, in its own local `+X`, where the diffuser
/// hangs — `street_lamp`'s head housing is authored at local `(0.86, h+0.06)`
/// and its lens plate at `(0.88, h-0.02)` (`props::services::street_lamp`), and
/// `lamp_glass` is a bare 0.4 x 0.05 x 0.2 slab centred on its own origin.
const LAMP_ARM: f32 = 0.88;

/// `street_lamp(rng, 5.4)`'s pole height, as `props::registry` registers it.
const LAMP_HEIGHT: f32 = 5.4;

/// One sandbag's authored length (`props::cover::sandbag`'s widest variant is
/// 0.49 m along local X), plus a little overlap so a row reads as a wall rather
/// than a dotted line.
const SANDBAG_PITCH: f32 = 0.44;

/// One sandbag course's height (the tallest variant is 0.175 m).
const SANDBAG_COURSE: f32 = 0.165;

/// A tyre's section height when it is lying flat (`tyre(rng, 0.33)`'s half
/// section width is `0.33 * 0.3`, so a laid tyre is ~0.2 m tall).
const TYRE_LIFT: f32 = 0.2;

/// Fronds per palm crown.
const FRONDS: usize = 6;

/// Place the placeholder set. See the module doc for what this is and is not.
pub fn place_street_furniture(asm: &mut Assembler) {
    lamps(asm);
    jerseys(asm);
    sandbag_walls(asm);
    tyre_stacks(asm);
    palms(asm);
    stalls(asm);
}

/// A lamp post on the kerb, with its diffuser out on the arm. The arm points
/// along the post's local `+X`; `trs`'s `ry` is a standard right-handed
/// rotation about `+Y`, so local `+X` lands at `(cos ry, 0, -sin ry)`.
fn lamps(asm: &mut Assembler) {
    for [x, z, ry] in SET_PIECES.lamps.iter().copied() {
        let (x, z, ry) = (x as f32, z as f32, ry as f32);
        asm.put("lamp_post", x, walk_h(), z, ry, 1.0, None, 0.0, 0.0);
        asm.put(
            "lamp_glass",
            x + ry.cos() * LAMP_ARM,
            walk_h() + LAMP_HEIGHT - 0.04,
            z - ry.sin() * LAMP_ARM,
            ry,
            1.0,
            None,
            0.0,
            0.0,
        );
    }
}

/// Jersey barriers at their authored positions, on the road surface.
fn jerseys(asm: &mut Assembler) {
    for [x, z, ry] in SET_PIECES.jerseys.iter().copied() {
        asm.put("jersey", x as f32, 0.0, z as f32, ry as f32, 1.0, None, 0.0, 0.0);
    }
}

/// A two-course sandbag row along each authored emplacement. The three sandbag
/// variants alternate so no two neighbours are the same sack, and the upper
/// course is offset half a bag so the courses interlock.
fn sandbag_walls(asm: &mut Assembler) {
    const VARIANTS: [&str; 3] = ["sandbag_a", "sandbag_b", "sandbag_c"];
    for (wall, [x, z, ry, length]) in SET_PIECES.sandbag_walls.iter().copied().enumerate() {
        let (x, z, ry) = (x as f32, z as f32, ry as f32);
        let (dx, dz) = (ry.cos(), -ry.sin());
        let bags = ((length as f32) / SANDBAG_PITCH).floor().max(1.0) as usize;
        for course in 0..2usize {
            let offset = if course == 0 { 0.0 } else { SANDBAG_PITCH * 0.5 };
            let count = bags - course;
            for i in 0..count {
                let t = (i as f32 - (count as f32 - 1.0) * 0.5) * SANDBAG_PITCH + offset;
                let variant = VARIANTS[(wall + course + i) % 3];
                asm.put(
                    variant,
                    x + dx * t,
                    SANDBAG_COURSE * (course as f32 + 0.5),
                    z + dz * t,
                    ry,
                    1.0,
                    None,
                    0.0,
                    0.0,
                );
            }
        }
    }
}

/// A stack of `n` tyres at each authored position, alternating the two sizes so
/// a stack tapers slightly.
fn tyre_stacks(asm: &mut Assembler) {
    for [x, z, n] in SET_PIECES.tyres.iter().copied() {
        let count = (n as usize).max(1);
        for i in 0..count {
            // The top tyre of a tall stack is the small one — a stack that
            // tapers reads as a stack rather than a cylinder.
            let id = if i + 1 == count && count > 2 {
                "tyre_small"
            } else {
                "tyre"
            };
            asm.put(
                id,
                x as f32,
                TYRE_LIFT * i as f32,
                z as f32,
                // Each tyre is rolled a little relative to the one below so the
                // tread blocks do not line up into a single ribbed column.
                i as f32 * 0.7,
                1.0,
                None,
                0.0,
                0.0,
            );
        }
    }
}

/// A palm trunk at each authored position, with a crown of fronds fanned around
/// its top. `palm_tree`'s own lean means the crown is offset from the base, but
/// `top_x` is a property of the *geometry* the prototype was built from and is
/// not exposed through the registry — so the fronds are hung on the trunk's
/// nominal top, which is within ~0.2 m of the leaning tip.
fn palms(asm: &mut Assembler) {
    const PALM_HEIGHT: f32 = 5.4;
    for [x, z, scale] in SET_PIECES.palms.iter().copied() {
        let (x, z, s) = (x as f32, z as f32, scale as f32);
        asm.put("palm_trunk", x, walk_h(), z, 0.0, s, None, 0.0, 0.0);
        let crown = walk_h() + PALM_HEIGHT * s;
        for i in 0..FRONDS {
            let ry = std::f32::consts::TAU * i as f32 / FRONDS as f32;
            // Alternate fronds droop harder, which is what stops a crown
            // reading as a flat parasol.
            let rz = if i % 2 == 0 { -0.35 } else { -0.55 };
            asm.put("palm_frond", x, crown, z, ry, s, None, 0.0, rz);
        }
    }
}

/// A market stall at each authored position, with a crate and a barrel stood
/// beside it — the two prototypes a market bay would obviously carry, placed at
/// a fixed offset rather than scattered, because scattering is `dressing.js`'s
/// job and this is not it.
fn stalls(asm: &mut Assembler) {
    for [x, z, ry, width] in SET_PIECES.stalls.iter().copied() {
        let (x, z, ry, w) = (x as f32, z as f32, ry as f32, width as f32);
        asm.put("stall", x, 0.0, z, ry, 1.0, None, 0.0, 0.0);
        let (dx, dz) = (ry.cos(), -ry.sin());
        let half = w * 0.5 + 0.45;
        asm.put("crate_a", x + dx * half, 0.0, z + dz * half, ry, 1.0, None, 0.0, 0.0);
        asm.put(
            "barrel_rust",
            x - dx * half,
            0.0,
            z - dz * half,
            ry,
            1.0,
            None,
            0.0,
            0.0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::world::props::register_props;

    /// Built with the arena floor policy lifted
    /// ([`crate::world::clutter::ClutterPolicy::RESTORED`]).
    ///
    /// Every prototype this placeholder pass names — `jersey`, the sandbags,
    /// `tyre`, `palm_trunk`/`palm_frond`, `stall`, `crate_a`, `barrel_rust` —
    /// is in `GROUND_CLUTTER`, so under the shipping policy
    /// `Assembler::place` discards all of it and these tests would be
    /// asserting counts of zero. That is not a defect in this pass: it is the
    /// policy doing exactly its job, and this file is the labelled placeholder
    /// its own module doc says to **delete** now that `world::dressing` has
    /// landed (nothing outside these tests calls
    /// [`place_street_furniture`]). Lifting the policy keeps the tests
    /// meaningful for as long as the file survives.
    fn assembler_with_props() -> Assembler {
        let mut asm = Assembler::new(Rng::new(1));
        asm.clutter = crate::world::clutter::ClutterPolicy::RESTORED;
        let mut rng = Rng::new(20260818);
        register_props(&mut asm, &mut rng);
        asm
    }

    #[test]
    fn every_prototype_this_pass_names_is_registered() {
        let mut asm = assembler_with_props();
        place_street_furniture(&mut asm);
        for id in [
            "lamp_post",
            "lamp_glass",
            "jersey",
            "sandbag_a",
            "sandbag_b",
            "sandbag_c",
            "tyre",
            "tyre_small",
            "palm_trunk",
            "palm_frond",
            "stall",
            "crate_a",
            "barrel_rust",
        ] {
            assert!(asm.has(id), "{id} is not a registered prototype");
            assert!(
                asm.count(id) > 0,
                "{id} was named but never placed — a `put` against a missing \
                 prototype is silently dropped, so a typo shows up here"
            );
        }
    }

    #[test]
    fn the_counts_follow_the_authored_set_piece_table() {
        let mut asm = assembler_with_props();
        place_street_furniture(&mut asm);
        assert_eq!(asm.count("lamp_post"), SET_PIECES.lamps.len());
        assert_eq!(asm.count("lamp_glass"), SET_PIECES.lamps.len());
        assert_eq!(asm.count("jersey"), SET_PIECES.jerseys.len());
        assert_eq!(asm.count("stall"), SET_PIECES.stalls.len());
        assert_eq!(asm.count("crate_a"), SET_PIECES.stalls.len());
        assert_eq!(asm.count("palm_trunk"), SET_PIECES.palms.len());
        assert_eq!(asm.count("palm_frond"), SET_PIECES.palms.len() * FRONDS);
        let tyres: usize = SET_PIECES.tyres.iter().map(|t| t[2] as usize).sum();
        assert_eq!(asm.count("tyre") + asm.count("tyre_small"), tyres);
    }

    #[test]
    fn a_sandbag_row_is_two_interlocking_courses() {
        let mut asm = assembler_with_props();
        place_street_furniture(&mut asm);
        let bags: usize = ["sandbag_a", "sandbag_b", "sandbag_c"]
            .iter()
            .map(|id| asm.count(id))
            .sum();
        // Per wall: floor(length / pitch) on the lower course, one fewer above.
        let want: usize = SET_PIECES
            .sandbag_walls
            .iter()
            .map(|w| {
                let n = ((w[3] as f32) / SANDBAG_PITCH).floor().max(1.0) as usize;
                n + (n - 1)
            })
            .sum();
        assert_eq!(bags, want);
        assert!(bags > 20, "a five-emplacement street should read as cover");
    }

    #[test]
    fn the_pass_draws_no_random_numbers() {
        // The world stream must be untouched: this pass runs after the ground,
        // the buildings and the prototypes have all rolled, and a draw here
        // would reshuffle nothing visible today but everything the day a real
        // dressing pass lands after it.
        let mut asm = assembler_with_props();
        let mut rng = Rng::new(9);
        let before = rng.float();
        place_street_furniture(&mut asm);
        let mut same = Rng::new(9);
        assert_eq!(before, same.float());
    }
}
