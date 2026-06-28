//! Data-driven semantic **world tags** for the growth agent — the "nouns" an
//! agent resolves a high-level command against ("walk to the *mountaintop*, look
//! at the *ground*"). Tags are inert [`axiom_introspect::WorldTag`] values fed to
//! an `IntrospectApi`; the directive runner resolves a target name through it.
//!
//! Tags come from **two sources**, exactly as a general engine should support:
//! - **runtime-registered** ([`runtime_tags`]) from data the sim already
//!   computes (the procedural vista's peak / spawn shelf), so a *generated* world
//!   still has nouns; and
//! - **TOML-authored** ([`toml_tags`]) from a package data file, the static-world
//!   path that graduates to the full `.axpkg` pipeline later.
//!
//! Native + `agent` feature only (it imports the `agent`-gated deps).

use axiom_introspect::WorldTag;
use serde::Deserialize;

use crate::ground::GroundSim;

/// A summit / high point.
pub const KIND_SUMMIT: u16 = 1;
/// A ground / low reference point.
pub const KIND_GROUND: u16 = 2;
/// The spawn / start point.
pub const KIND_SPAWN: u16 = 3;
/// An unclassified point of interest.
pub const KIND_OTHER: u16 = 0;

/// A world-unit `f32` as fixed-point micro-units (the engine's tag/observation
/// coordinate convention — millionths of a world unit).
fn micro(value: f32) -> i64 {
    (f64::from(value) * 1_000_000.0) as i64
}

/// Map an authored kind name to its coarse code (the app owns the vocabulary).
fn kind_code(name: &str) -> u16 {
    match name {
        "summit" | "mountaintop" | "peak" => KIND_SUMMIT,
        "ground" => KIND_GROUND,
        "spawn" => KIND_SPAWN,
        _ => KIND_OTHER,
    }
}

/// The tags the sim derives from its **own generated vista** — the procedural
/// source of nouns: the `mountaintop` (the peak) and the `ground` (the spawn
/// shelf far below). Tag ids `1..` are reserved for these.
pub fn runtime_tags(sim: &GroundSim) -> Vec<WorldTag> {
    let (peak_x, peak_z) = sim.peak_xz();
    let (spawn_x, spawn_z) = sim.spawn_xz();
    vec![
        WorldTag::new(
            1,
            "mountaintop".to_string(),
            KIND_SUMMIT,
            micro(peak_x),
            micro(sim.peak_height_m()),
            micro(peak_z),
        ),
        WorldTag::new(
            2,
            "ground".to_string(),
            KIND_GROUND,
            micro(spawn_x),
            micro(sim.shelf_height_m()),
            micro(spawn_z),
        ),
    ]
}

/// One authored tag entry in `package/world/tags.toml`.
#[derive(Debug, Deserialize)]
struct TagEntry {
    name: String,
    #[serde(default)]
    kind: String,
    /// A symbolic anchor resolved against the live sim: `"spawn"` or `"summit"`.
    #[serde(default)]
    at: Option<String>,
    #[serde(default)]
    x: Option<f32>,
    #[serde(default)]
    z: Option<f32>,
    /// Absolute height (metres); sampled from the terrain when omitted.
    #[serde(default)]
    y: Option<f32>,
}

/// The `[[tag]]` table of an authored tag file.
#[derive(Debug, Deserialize)]
struct TagFile {
    #[serde(default)]
    tag: Vec<TagEntry>,
}

/// Parse authored TOML tags, resolving each against the live sim: a symbolic
/// `at = "spawn"|"summit"` or an explicit `x`/`z`, with the height sampled off
/// the terrain when `y` is omitted. Authored tag ids start at `100` so they never
/// collide with the runtime tags.
pub fn toml_tags(sim: &GroundSim, toml_str: &str) -> Vec<WorldTag> {
    let file: TagFile = toml::from_str(toml_str).expect("growth tags.toml parses");
    file.tag
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let (x, z) = match entry.at.as_deref() {
                Some("spawn") => sim.spawn_xz(),
                Some("summit") | Some("mountaintop") => sim.peak_xz(),
                _ => (entry.x.unwrap_or(0.0), entry.z.unwrap_or(0.0)),
            };
            let y = entry.y.unwrap_or_else(|| sim.ground_abs_at(x, z));
            WorldTag::new(
                100 + index as u32,
                entry.name,
                kind_code(&entry.kind),
                micro(x),
                micro(y),
                micro(z),
            )
        })
        .collect()
}
