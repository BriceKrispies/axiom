//! # The shader crucible
//!
//! Ten labelled stations demonstrating Axiom's procedural appearance system in
//! its entirety — and stating, beside the stations they affect, the four things
//! it does not do.
//!
//! ## What this app is for
//!
//! `apps/burnt-rubber` authors a `FieldGraph` and **bakes** it, through
//! `TextureOp::Field`, into an ordinary texture. That was the right call there —
//! asphalt is the largest surface in any frame and making it live per-pixel
//! changes the fill-rate story — but it means the other half of the system had
//! no shipping consumer: **graph → `Surface` → `surface_program` → WGSL**.
//!
//! The crucible authors **live `Surface`s** and takes that path. Every station
//! but one binds an authored graph to a channel of a `Surface`, hands the whole
//! set to `GpuBackendApi::prepare_surfaces` at the startup barrier, and carries
//! the resulting digest onto its draw as a `surface_program`. Station 3 is the
//! deliberate exception: it is the *same graph*, baked, so a viewer can see one
//! graph in two realisations side by side.
//!
//! ## Nothing here is a shading function
//!
//! There is **no WGSL in this app** (`tests/no_wgsl.rs` is a grep test that
//! proves it), and there is no Rust that computes a colour either. `authoring.rs`
//! is one-line spellings of single operators; every pattern, mask, blend,
//! displacement and density in the crucible is a value built out of them. That
//! is the claim the whole system makes — *a new visual effect is a new graph,
//! never a new Rust function* — and this app is what it looks like when it is
//! true.
//!
//! ## The parts
//!
//! | Module | What it owns |
//! |---|---|
//! | [`authoring`] | one-line spellings of the 27 operators |
//! | [`stations`] | the ten stations, and the table that names them |
//! | [`preparation`] | the barrier: the only place a shader is compiled |
//! | [`scene`] | the window, the camera, the light rig, the `RunningApp` |
//! | [`stand`] | the twelve bodies, each carrying its station's surface |
//! | [`label`] | the caption over each body, naming the station it wears |
//! | [`glyphs`] | the 5x7 cell font those captions are welded out of |
//! | [`layout`] | where each station stands |
//! | [`backends`] | station 9: `supported_by` for both real profiles |
//! | [`introspection`] | station 10: `explain` / `digest` / `diff` |
//! | [`limitations`] | the four things this does not do |
//! | [`orbit`] | the interactive camera the page drags the stations around with |
//! | [`redraw`] | when a frame is worth drawing, and why the app idles when it is not |
//! | [`report`] | the one assembled report the page, the console and the README share |
//! | [`diagnostics`] | the frame-performance panel under the canvas, and what it refuses to invent |
//! | [`levers`] | the kill-switches under the panel: what each one removes from a frame |
//! | [`export`] | the whole reading as one JSON object, so a phone's numbers can reach an engineer |
//!
//! ## Determinism
//!
//! Fixed seeds, no wall clock anywhere: station 5's displacement reads
//! `EvalContext::time`, which the frame supplies from the engine's own clock. A
//! tick replayed twice is identical; tick *N* and tick *N + 60* differ exactly
//! where a station is time-varying.

pub mod authoring;
pub mod backends;
pub mod diagnostics;
pub mod export;
pub mod frame;
pub mod glyphs;
pub mod introspection;
pub mod label;
pub mod layout;
pub mod levers;
pub mod limitations;
pub mod orbit;
pub mod preparation;
pub mod redraw;
pub mod report;
pub mod scene;
pub mod stand;
pub mod stations;

#[cfg(target_arch = "wasm32")]
pub mod web;

/// The DOM half of the orbit camera: pointer/wheel gestures measured and handed
/// to [`orbit::OrbitState`]. Compiled only for `wasm32`; the camera policy it
/// drives is browser-free and lives in `src/orbit.rs`.
#[cfg(target_arch = "wasm32")]
mod pointer_input;

pub use layout::{HEIGHT, WIDTH};
pub use orbit::OrbitState;
pub use scene::{crucible_app, crucible_core, shader_crucible_core};

/// **The committed digest of every station surface**, in `stations::all_surfaces`
/// order.
///
/// A surface's digest is the identity a program cache keys on, so a change to one
/// is a change to the identity of a material — never something that should happen
/// by accident. `stations::tests::every_station_digest_is_the_committed_value`
/// fails when one moves; when it does, check that you meant it and then update
/// the number here.
pub const COMMITTED_DIGESTS: [&str; 11] = [
    "19737C182473E77F",
    "660AB93B0FF5FA89",
    "2E1A08596E819F86",
    "9479A9C10E4EF768",
    "05290AF13BBCB027",
    "11512E0237589154",
    "D3E45EF2DAAE46B1",
    "AE543A817123501E",
    "8A1F20455CD496C2",
    "1F782E486116CB01",
    "61734EE4D23007D1",
];

/// Browser entry: author the scene, compile every station's program, and drive
/// the live loop. Called from the page.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn shader_crucible_start() {
    console_error_panic_hook::set_once();
    web::start();
}
