//! # Axiom — runtime asset-streaming demo (browser/WASM)
//!
//! A thin host for the [`axiom_assets`] module's [`AssetsApi`] — the
//! deterministic asset-streaming brain. The page **boots fast**: `start()`
//! renders an interactive status to the DOM before any asset is fetched. It then
//! `fetch`es a binary manifest, builds the streaming catalog, and runs a
//! `requestAnimationFrame` loop that drives streaming entirely as data:
//!
//! - each frame it drains the queues of just-completed fetches and feeds them to
//!   [`AssetsApi::advance`], which returns the NEW `(id, locator)` loads to
//!   dispatch now — chosen by priority + dependency order within a concurrency
//!   budget;
//! - for each returned load it `spawn_local`s an async `fetch(locator)` that, on
//!   resolution, pushes the asset id onto the shared completed-OK / completed-FAIL
//!   queue (drained next frame);
//! - it drains [`AssetsApi::take_ready`] and reflects every row's
//!   [`AssetsApi::state_code`] into the DOM, plus a header and a tiny
//!   `window.__assetDemo` progress object for tests.
//!
//! The module owns NO I/O on purpose: the nondeterministic fetch timing enters as
//! explicit data (the completions queued at a frame boundary), so the streaming
//! session is replayable. Decoding the fetched bytes into meshes/textures is
//! intentionally out of scope (that needs `axiom-resources`/scene); the goal is
//! to prove the streaming pipeline end-to-end against real network fetches.
//!
//! All of this is the nondeterministic browser edge, so it is confined to the
//! `wasm32` [`web`] module; native `cargo test` compiles nothing here.
//!
//! ## Two host edges, one streaming brain
//!
//! - [`web`] (`start`, `index.html`): the main-thread variant — `fetch`es run on
//!   the main thread via `spawn_local`. Network transfer is already off-thread, so
//!   downloads are parallel, but any CPU work on the bytes would block the frame.
//! - [`pool`] (`start_pool`, `workers.html`): the **Web Worker pool** variant — a
//!   pool of N background workers each `fetch`+decode one asset off the main
//!   thread (`?workers=N`, default 3), pulling jobs from a shared queue the main
//!   thread fills. This is the "skeleton loads first, assets stream on workers"
//!   architecture; CPU-heavy decode never touches the main thread.
//!
//! Both feed the SAME deterministic [`axiom_assets::AssetsApi`]: completions are
//! drained and applied at a frame boundary, so even though workers finish in
//! arbitrary order, a given completion sequence yields the same schedule. The only
//! difference is *where the work runs* — the brain is untouched.

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
mod pool;
