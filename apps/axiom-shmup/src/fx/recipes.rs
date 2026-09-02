//! The impact bursts, loaded from their `.burst` assets.
//!
//! Not a port of anything, and — as of this file — **not code either**. Every
//! recipe is a text file in `recipes/`, read by [`crate::fx::burst_text`] into
//! the [`Burst`] values [`crate::fx::burst`] executes. What is left here is the
//! list of files and nothing else.
//!
//! Ten of the twelve impact surfaces went through three forms to get here.
//! First hand-written Rust functions; then Rust *data*, which was byte-exact
//! and about twice the size of the code it replaced; and now text, which is the
//! same data with the syntax removed. The middle step was not wasted — it is
//! what proved the recipes were values rather than behaviour, and a value can
//! be moved out of a language. It just should not have stayed there.
//!
//! Each asset was checked against the builder that used to produce it, by
//! structural equality on the whole `Burst` — same instructions, same
//! registers, same field table, same companion — before the builders were
//! deleted. The frozen fingerprint ledger is the standing oracle now.
//!
//! # Why `include_str!` and not a file read
//!
//! This app is WebAssembly-first, where there is no filesystem to read from and
//! an asset fetch is a network round trip the impact system cannot wait for.
//! Compiling the text in keeps the recipes editable without touching Rust —
//! which is the whole point — while a burst still costs nothing at the moment
//! it fires. A runtime loader belongs with the rest of the asset pipeline, not
//! here.

use std::sync::LazyLock;

use crate::fx::burst::Burst;

/// Parse one recipe asset.
///
/// A failure here is a malformed file that was compiled in, so it cannot be
/// handled at runtime and should not pretend to be: the message names the line,
/// and `every_recipe_parses` catches it long before a player would.
fn load(name: &str, src: &str) -> Vec<Burst> {
    crate::fx::burst_text::parse(src).unwrap_or_else(|e| panic!("recipe `{name}`: {e}"))
}

/// Foliage: shredded leaf matter, no hole. `impacts.js:844-864`.
pub static FOLIAGE: LazyLock<Vec<Burst>> =
    LazyLock::new(|| load("foliage", include_str!("recipes/foliage.burst")));

/// Wood: splinters and a brown, resinous puff. `impacts.js:546-594`.
pub static WOOD: LazyLock<Vec<Burst>> =
    LazyLock::new(|| load("wood", include_str!("recipes/wood.burst")));

/// Flesh: a dark aerosol cone and heavy droplets. `impacts.js:793-841`.
pub static FLESH: LazyLock<Vec<Burst>> =
    LazyLock::new(|| load("flesh", include_str!("recipes/flesh.burst")));

/// Wet earth: a plume and heavy clods. `impacts.js:597-659`, the dirt row.
pub static GROUND_DIRT: LazyLock<Vec<Burst>> =
    LazyLock::new(|| load("dirt", include_str!("recipes/dirt.burst")));

/// Dry sand — the same burst, paler, with finer clods and more drag on them.
pub static GROUND_SAND: LazyLock<Vec<Burst>> =
    LazyLock::new(|| load("sand", include_str!("recipes/sand.burst")));

/// Water: a column, droplets, a hanging mist. `impacts.js:727-790`.
pub static WATER: LazyLock<Vec<Burst>> =
    LazyLock::new(|| load("water", include_str!("recipes/water.burst")));

/// Woven cloth — pale, dusty fibres. `impacts.js:867-916`, the fabric row.
pub static FABRIC: LazyLock<Vec<Burst>> =
    LazyLock::new(|| load("fabric", include_str!("recipes/fabric.burst")));

/// Rubber — the same burst, nearly black.
pub static RUBBER: LazyLock<Vec<Burst>> =
    LazyLock::new(|| load("rubber", include_str!("recipes/rubber.burst")));

/// Glass: glinting shards and a fine aerosol. `impacts.js:662-724`.
pub static GLASS: LazyLock<Vec<Burst>> =
    LazyLock::new(|| load("glass", include_str!("recipes/glass.burst")));

/// Plaster / drywall: banded white powder, crumbs, ejecta. `impacts.js:332-410`.
pub static PLASTER: LazyLock<Vec<Burst>> =
    LazyLock::new(|| load("plaster", include_str!("recipes/plaster.burst")));

/// An explosion's fire — core flash, fireball, boiling smoke.
/// `explosions.js:35-140`.
pub static EXPLOSION_FIRE: LazyLock<Vec<Burst>> = LazyLock::new(|| {
    load(
        "explosion_fire",
        include_str!("recipes/explosion_fire.burst"),
    )
});

/// An explosion's blast — shockwave, ground ring, debris, embers.
/// `explosions.js:140-262`. A separate recipe because the haze ring fires
/// between the two and draws from the same stream.
pub static EXPLOSION_BLAST: LazyLock<Vec<Burst>> = LazyLock::new(|| {
    load(
        "explosion_blast",
        include_str!("recipes/explosion_blast.burst"),
    )
});

/// Every recipe, so a test can sweep them.
pub fn all() -> Vec<(&'static str, &'static Burst)> {
    [
        ("foliage", &*FOLIAGE),
        ("wood", &*WOOD),
        ("flesh", &*FLESH),
        ("dirt", &*GROUND_DIRT),
        ("sand", &*GROUND_SAND),
        ("water", &*WATER),
        ("fabric", &*FABRIC),
        ("rubber", &*RUBBER),
        ("glass", &*GLASS),
        ("plaster", &*PLASTER),
        ("explosion_fire", &*EXPLOSION_FIRE),
        ("explosion_blast", &*EXPLOSION_BLAST),
    ]
    .into_iter()
    .flat_map(|(name, bursts)| bursts.iter().map(move |b| (name, b)))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::burst::Src;

    /// Every asset parses, and none of them is empty.
    ///
    /// `load` panics on a malformed file, so this failing is the parse error
    /// itself — which is the point: a recipe that does not read is a compile-time
    /// mistake wearing a runtime costume, and this is where it gets caught.
    #[test]
    fn every_recipe_parses_into_at_least_one_burst() {
        // Twelve recipes, twenty-eight bursts: foliage is one, water and
        // plaster are three each, the explosion's two halves are three and
        // four, and the rest are two.
        assert_eq!(all().len(), 28, "the recipe set changed size");
    }

    #[test]
    fn every_recipe_reads_only_registers_written_before_it() {
        all().iter().for_each(|(name, burst)| {
            assert!(burst.operands_resolve(), "{name}");
        });
    }

    /// Every recipe's program is shorter than the table it fills.
    ///
    /// This is the format's central claim, so it is asserted rather than
    /// described. A burst writes twenty-odd fields and computes about half of
    /// them; when constants were instructions the program was the longer half,
    /// which is what made the data form bigger than the code it replaced.
    #[test]
    fn a_recipe_computes_less_than_it_states() {
        all().iter().for_each(|(name, burst)| {
            let computed = burst
                .fields
                .iter()
                .filter(|(_, src)| matches!(src, Src::Reg(_)))
                .count();
            assert!(
                computed < burst.fields.len(),
                "{name}: every field is computed, so nothing is a constant"
            );
        });
    }

    /// Foliage is the smallest recipe and the one every change to the parser is
    /// most likely to break first, so its shape is pinned by number.
    #[test]
    fn the_foliage_program_writes_twenty_two_registers() {
        assert_eq!(FOLIAGE[0].register_count(), 22);
    }
}
