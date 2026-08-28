//! Ported from Claude-of-Duty `src/world/clutter.js:1-166` — **what the level
//! does not put on the floor**, the arena-shooter policy.
//!
//! This level was dressed like a modern-military campaign map: a burnt-out
//! saloon, drum clusters, tyre piles, pallets, market stalls, and a continuous
//! scatter of bricks, rocks, litter and slab shards over every square metre of
//! ground. That reads as lived-in, and it is exactly wrong for an arena
//! shooter, where the floor is a surface you fight across and the ARCHITECTURE
//! is the cover. Scattered junk in an arena costs readability twice: it hides
//! the silhouette of a player against the ground, and it makes every sightline
//! a negotiation with waist-high debris nobody placed on purpose.
//!
//! So the floor is cleared. Everything that stands on the ground is
//! suppressed; everything attached to the architecture — street lamps, wall
//! signs, roof units, sat dishes, water tanks — stays, because those read as
//! building, not as clutter, and they keep the skyline and the wall surfaces
//! from going flat.
//!
//! ## HOW IT IS DONE, AND WHY THERE
//!
//! The suppression happens in one place — [`Assembler::place`] — rather than
//! by deleting the placement logic in [`crate::world::dressing`]. Three
//! reasons, all the source's (`clutter.js:24-40`):
//!
//! 1. The dressing code still knows how to furnish this level. Turning an id
//!    back on is moving one line out of [`GROUND_CLUTTER`], not an archaeology
//!    exercise in git history.
//! 2. **Every placement decision still runs, so it still draws the same random
//!    numbers in the same order.** The buildings, the layout and the level's
//!    whole architecture are therefore byte-identical to before — only the
//!    props stop being instanced. Deleting the calls instead would reshuffle
//!    the RNG stream and rebuild the level into a different shape.
//! 3. One choke point cannot be half-applied. There is no path that places a
//!    prototype without going through `place()`.
//!
//! The two pieces of the wrecked car that are NOT prototypes — its body slab
//! and the sand drift piled against it — are gated at their own site in
//! [`crate::world::dressing::street_floor`], because raw geometry has no id to
//! suppress. [`ClutterPolicy::is_suppressed`] is public for exactly that case.
//!
//! ## The policy is a value here, not a module global
//!
//! `clutter.js` reads `?clutter=1` once at module load and consults a
//! file-level `const`. This port carries the same decisions as a [`Copy`]
//! value on the [`Assembler`] ([`Assembler::clutter`]), for the reason every
//! other knob in this port is a value: a module-global read of
//! `location.search` is untestable natively and un-overridable from a golden.
//! [`ClutterPolicy::from_environment`] is the module-load read, and it is the
//! only place the query string is touched.
//!
//! [`Assembler`]: crate::world::assembler::Assembler
//! [`Assembler::place`]: crate::world::assembler::Assembler::place
//! [`Assembler::clutter`]: crate::world::assembler::Assembler::clutter

/// Props that stand on the ground. All suppressed (`clutter.js:47-95`).
///
/// Grouped by what they are, so a decision to bring a category back — cover,
/// or vegetation for silhouette — is one edit rather than a scavenger hunt.
/// The grouping and the order are the source's; nothing reads the order, but
/// this list diffs against `clutter.js` by eye and that is worth keeping.
pub const GROUND_CLUTTER: [&str; 53] = [
    // The wrecked saloon and its wheel.
    "wreck",
    "tyre",
    "tyre_small",
    // Drums and containers.
    "barrel_blue",
    "barrel_rust",
    "barrel_wood",
    "bucket",
    "jerry_can",
    "gas_bottle",
    "box_card_a",
    "box_card_b",
    // Crates, pallets and stacked cover. An arena gets its cover from the
    // building shells; a crate on an open floor is the thing this is removing.
    "crate_a",
    "crate_b",
    "crate_c",
    "crate_flat",
    "pallet",
    // Sandbags and concrete barriers — same reasoning as the crates.
    "sandbag_a",
    "sandbag_b",
    "sandbag_c",
    "jersey",
    "block_big",
    "block_small",
    // Rubble and rock.
    "rock_a",
    "rock_b",
    "brick_a",
    "brick_b",
    "slab_shard",
    "rebar",
    "plank_a",
    "plank_b",
    "pock",
    // Litter.
    "litter",
    "can",
    "bottle",
    "glass_shards",
    // Cinder blocks — the single most numerous thing on the floor.
    "cinder",
    // The wheel shed by the wrecked saloon.
    "wheel_flat",
    // Street furniture and market dressing that sits on the floor.
    "stall",
    "table",
    "table_small",
    "chair",
    "shelf",
    "cabinet",
    "mattress",
    "planter",
    "stool",
    "tray",
    "produce",
    // Vegetation. Palms are ground-planted, so they go with the rest; the
    // trunk and frond are separate prototypes and both have to be named.
    "shrub",
    "weeds",
    "palm_trunk",
    "palm_frond",
    // The dust fillet that grounds a prop against the floor. With nothing left
    // to ground it is a stain with no object, which is worse than either.
    "dust_skirt",
];

/// Kept on purpose (`clutter.js:97-105`), listed so the intent is legible and
/// a future edit can see what was decided rather than what was merely
/// forgotten:
///
/// | id | why it stays |
/// |---|---|
/// | `lamp_post`, `lamp_glass` | vertical, and the only thing lighting the street after dusk |
/// | `sign_board`, `sign_hang` | wall-mounted; they break up blank facades |
/// | `ac_unit`, `sat_dish`, `roof_vent`, `water_tank` | roof and wall furniture; they carry the skyline |
pub const KEPT_ON_PURPOSE: [&str; 8] = [
    "lamp_post",
    "lamp_glass",
    "sign_board",
    "sign_hang",
    "ac_unit",
    "sat_dish",
    "roof_vent",
    "water_tank",
];

/// A category of floor dressing (`ARENA_FLOOR`'s keys, `clutter.js:113-136`).
///
/// Named rather than a boolean per call site so the policy reads as a set of
/// decisions, and so turning one back on is one edit in [`ClutterPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Prototypes in [`GROUND_CLUTTER`].
    Props,
    /// The dust rings and swept grit at the foot of a prop
    /// ([`crate::world::dressing::ground_skirt`]). They exist to hide the seam
    /// where an object meets the ground; with the objects gone they are stains
    /// with no object — pale ellipses on an otherwise clean road that read as
    /// decals floating over it.
    Skirts,
    /// Road marks that imply traffic: tyre ruts polished into the dust, the
    /// dust drifted along them, and the scuffs where vehicles swung across the
    /// road.
    ///
    /// Two reasons, and the second is the one that matters. Thematically there
    /// are no vehicles in an arena, so tyre tracks describe something that
    /// does not happen here. Visually they were pale patches lifted ~4 cm off
    /// the road surface — the decal offset every road mark used — which was
    /// invisible under a floor covered in rubble and reads as a hovering disc
    /// on a clean one.
    VehicleMarks,
}

/// `ARENA_FLOOR` plus `RESTORE_CLUTTER` (`clutter.js:107-145`), as one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClutterPolicy {
    /// `RESTORE_CLUTTER` — `?clutter=1` puts every suppressed prop back, for
    /// comparing the two dressings side by side. It overrides every category
    /// below, exactly as the source's `!RESTORE_CLUTTER &&` prefix does.
    pub restore: bool,
    /// `ARENA_FLOOR.props`.
    pub props: bool,
    /// `ARENA_FLOOR.skirts`.
    pub skirts: bool,
    /// `ARENA_FLOOR.vehicleMarks`.
    pub vehicle_marks: bool,
}

impl ClutterPolicy {
    /// The shipping policy: the floor is cleared (`ARENA_FLOOR`'s three
    /// `true`s, `clutter.js:113-136`, with `RESTORE_CLUTTER` false).
    pub const ARENA_FLOOR: ClutterPolicy = ClutterPolicy {
        restore: false,
        props: true,
        skirts: true,
        vehicle_marks: true,
    };

    /// What `?clutter=1` selects: the pre-policy dressing, every prop back.
    ///
    /// The three categories stay `true`; `restore` is what overrides them, so
    /// clearing the switch returns to [`ClutterPolicy::ARENA_FLOOR`] exactly.
    pub const RESTORED: ClutterPolicy = ClutterPolicy {
        restore: true,
        props: true,
        skirts: true,
        vehicle_marks: true,
    };

    /// `isSuppressed(id)` (`clutter.js:140-142`).
    pub fn is_suppressed(self, id: &str) -> bool {
        !self.restore && self.props && GROUND_CLUTTER.contains(&id)
    }

    /// `suppresses(category)` (`clutter.js:145-147`): true when the named
    /// floor-dressing category is switched off.
    pub fn suppresses(self, category: Category) -> bool {
        let on = match category {
            Category::Props => self.props,
            Category::Skirts => self.skirts,
            Category::VehicleMarks => self.vehicle_marks,
        };
        !self.restore && on
    }

    /// `RESTORE_CLUTTER`'s parse (`clutter.js:107-109`), as a pure function of
    /// the query string so it can be tested without a browser.
    ///
    /// Accepts the string with or without its leading `?`. The source uses
    /// `URLSearchParams`, whose `get('clutter')` returns the FIRST occurrence
    /// and decodes `+` as a space; neither matters for the one literal value
    /// this compares against, so the parse is a plain split.
    pub fn from_query(search: &str) -> ClutterPolicy {
        let restored = search
            .trim_start_matches('?')
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .any(|(k, v)| k == "clutter" && v == "1");
        if restored {
            ClutterPolicy::RESTORED
        } else {
            ClutterPolicy::ARENA_FLOOR
        }
    }

    /// The source's module-load read of `location.search`
    /// (`clutter.js:107-109`).
    ///
    /// Read once, where the source reads it once: `place()` runs thousands of
    /// times per build and has no business parsing a query string.
    ///
    /// Off the browser there is no query string and this is
    /// [`ClutterPolicy::ARENA_FLOOR`] — the shipping policy — which is also
    /// what keeps every native golden deterministic. `?clutter=1` is a
    /// side-by-side comparison switch, not a setting; nothing native needs it,
    /// and a test that wants the other dressing passes it explicitly to
    /// [`WorldSystem::init_with_clutter`][crate::world::system::WorldSystem::init_with_clutter].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_environment() -> ClutterPolicy {
        ClutterPolicy::ARENA_FLOOR
    }

    /// See the native arm above.
    ///
    /// `window.location.search` is read through `js_sys::Reflect` rather than
    /// `web_sys::Location`: `Location` is a `web-sys` feature this crate does
    /// not enable, and reaching two properties off `window` is exactly the
    /// job `js-sys` is already a dependency for. A missing `window` (a worker,
    /// a non-browser host) falls back to the shipping policy.
    #[cfg(target_arch = "wasm32")]
    pub fn from_environment() -> ClutterPolicy {
        let search = web_sys::window()
            .and_then(|w| js_sys::Reflect::get(&w, &wasm_bindgen::JsValue::from_str("location")).ok())
            .and_then(|l| js_sys::Reflect::get(&l, &wasm_bindgen::JsValue::from_str("search")).ok())
            .and_then(|s| s.as_string())
            .unwrap_or_default();
        ClutterPolicy::from_query(&search)
    }
}

impl Default for ClutterPolicy {
    fn default() -> Self {
        ClutterPolicy::ARENA_FLOOR
    }
}

/// `auditClutter(knownIds)` (`clutter.js:156-165`): the ids in
/// [`GROUND_CLUTTER`] that no prototype answers to.
///
/// A misspelt id here fails SILENTLY — it simply never matches, and the prop
/// it was meant to remove goes on being placed while the list says otherwise.
/// That is the one failure mode a policy list like this has, so the level
/// checks itself once at build time. Called from
/// [`Assembler::finalize`][crate::world::assembler::Assembler::finalize].
///
/// Returns the unknown ids as well as warning, so a test can assert the list
/// is clean without scraping stderr — which is the only way this audit is
/// worth anything in a port whose build has no console to read.
pub fn audit_clutter(known_ids: &[&str]) -> Vec<&'static str> {
    let unknown: Vec<&'static str> = GROUND_CLUTTER
        .into_iter()
        .filter(|id| !known_ids.contains(id))
        .collect();
    if !unknown.is_empty() {
        eprintln!(
            "[world] {} ids in GROUND_CLUTTER match no prototype and suppress nothing: {}",
            unknown.len(),
            unknown.join(", ")
        );
    }
    unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ground_clutter_list_has_no_duplicates() {
        let mut sorted = GROUND_CLUTTER;
        sorted.sort_unstable();
        let mut deduped = sorted.to_vec();
        deduped.dedup();
        assert_eq!(deduped.len(), GROUND_CLUTTER.len());
    }

    #[test]
    fn nothing_is_both_suppressed_and_kept_on_purpose() {
        for kept in KEPT_ON_PURPOSE {
            assert!(!GROUND_CLUTTER.contains(&kept), "{kept} is on both lists");
        }
    }

    #[test]
    fn the_arena_floor_suppresses_ground_props_and_keeps_architecture() {
        let p = ClutterPolicy::ARENA_FLOOR;
        assert!(p.is_suppressed("cinder"));
        assert!(p.is_suppressed("dust_skirt"));
        assert!(p.is_suppressed("plank_a"));
        assert!(!p.is_suppressed("lamp_post"));
        assert!(!p.is_suppressed("ac_unit"));
        assert!(!p.is_suppressed("sign_board"));
        // An id no prototype answers to is not suppressed either.
        assert!(!p.is_suppressed("not_a_prototype"));
    }

    #[test]
    fn restore_overrides_every_category() {
        let p = ClutterPolicy::RESTORED;
        assert!(!p.is_suppressed("cinder"));
        assert!(!p.suppresses(Category::Props));
        assert!(!p.suppresses(Category::Skirts));
        assert!(!p.suppresses(Category::VehicleMarks));
    }

    #[test]
    fn each_category_is_switched_independently() {
        let p = ClutterPolicy {
            skirts: false,
            ..ClutterPolicy::ARENA_FLOOR
        };
        assert!(p.suppresses(Category::Props));
        assert!(!p.suppresses(Category::Skirts));
        assert!(p.suppresses(Category::VehicleMarks));
    }

    #[test]
    fn clutter_1_is_the_only_query_that_restores() {
        assert_eq!(ClutterPolicy::from_query("?clutter=1"), ClutterPolicy::RESTORED);
        assert_eq!(ClutterPolicy::from_query("clutter=1"), ClutterPolicy::RESTORED);
        assert_eq!(
            ClutterPolicy::from_query("?fidelity=lean&clutter=1"),
            ClutterPolicy::RESTORED
        );
        assert_eq!(ClutterPolicy::from_query("?clutter=0"), ClutterPolicy::ARENA_FLOOR);
        assert_eq!(ClutterPolicy::from_query("?clutter"), ClutterPolicy::ARENA_FLOOR);
        assert_eq!(ClutterPolicy::from_query(""), ClutterPolicy::ARENA_FLOOR);
        assert_eq!(ClutterPolicy::from_query("?other=1"), ClutterPolicy::ARENA_FLOOR);
    }

    #[test]
    fn the_default_and_the_environment_are_the_shipping_policy() {
        assert_eq!(ClutterPolicy::default(), ClutterPolicy::ARENA_FLOOR);
        assert_eq!(ClutterPolicy::from_environment(), ClutterPolicy::ARENA_FLOOR);
    }

    #[test]
    fn audit_names_an_id_no_prototype_answers_to() {
        // Every real id known: nothing to report.
        assert!(audit_clutter(&GROUND_CLUTTER).is_empty());
        // Drop one: the audit names exactly it.
        let short: Vec<&str> = GROUND_CLUTTER.into_iter().filter(|id| *id != "cinder").collect();
        assert_eq!(audit_clutter(&short), vec!["cinder"]);
    }
}
