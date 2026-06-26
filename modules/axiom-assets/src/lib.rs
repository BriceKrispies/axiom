//! # Axiom Assets — Engine Module
//!
//! The deterministic runtime **asset-streaming brain**: it parses an
//! Axiom-native binary manifest and drives a load-state machine + scheduler so
//! the engine can boot fast and stream game content in the background.
//!
//! ## What this module is
//! - The owner of the asset **manifest** (the versioned list of assets + their
//!   dependency edges, encoded with the kernel's deterministic binary codec).
//! - A **load-state machine** per asset (`unrequested → in-flight → ready /
//!   failed`) advanced purely by per-frame **completions**.
//! - A **scheduler** that, within a concurrency budget, picks the next loads to
//!   dispatch in dependency order and by priority.
//!
//! ## What this module is not
//! - **Not** an I/O layer. It performs no `fetch`, no network, no disk, no
//!   threads, and touches no browser API. The app performs the (async, parallel)
//!   fetches the scheduler asks for and feeds the results back as completions.
//! - **Not** a decoder. It deals in opaque asset ids + locators, never mesh or
//!   texture bytes; decoding ready bytes into `axiom-resources` is the app's job.
//!
//! ## Why this shape
//! Streaming is nondeterministic only in *when* bytes arrive. By modelling the
//! state machine deterministically and taking the set of completions as
//! **explicit input each tick**, the same completion sequence always yields the
//! same schedule — so a streaming session is recordable and replayable, exactly
//! like every other Axiom input that crosses a boundary as data.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`AssetsApi`].

mod asset_catalog;
mod asset_entry;
mod asset_state;
mod assets_api;
mod manifest;

pub use assets_api::AssetsApi;
