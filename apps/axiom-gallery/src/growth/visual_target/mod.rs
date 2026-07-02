//! # Axiom Visual Target 001 — deterministic diorama + visual-convergence comparator
//!
//! Two halves that fit together:
//!
//! **The diorama (the deterministic shot).** A deliberately boring pipeline: a
//! **fixed, versioned scene manifest** ([`scene::Manifest`]) describing a camera, a
//! sun, fog, a terrain patch, ground materials, and vegetation instances → **neutral
//! render data** ([`build::RenderData`]) → one off-screen frame → a PNG → an optional
//! pixel compare against a reference image ([`compare`]). There is **no** procedural
//! world generation, survival, weather, inventory, AI, or gameplay here — the whole
//! diorama is a pure function of one TOML file, reusing the `growth` demo's proven
//! headless render-to-PNG machinery (off-screen GPU + Canvas 2D backends, the
//! `axiom-terrain-mesh` heightfield mesher). None of growth's planet worldgen is
//! involved.
//!
//! **The comparator (the convergence review loop).** On top of the shot sits a
//! disciplined review loop that refuses vague "looks better" judgements. Each
//! candidate is scored on twelve visual [`axes`] (0–5); the [`axes::Scorecard`]
//! reduces to a *lowest-dominated* final score; [`review`] decides — from the
//! champion and candidate scorecards and the one axis this iteration attacked —
//! whether to keep, reject, keep-with-regression, or branch anew; every iteration is
//! appended to the [`ledger`]; and new [`abstraction`]s are forbidden until an axis
//! has resisted three bounded attempts (or is genuinely inexpressible). The
//! [`target`] module wires these over an on-disk target directory.
//!
//! See `VISUAL_TARGET.md` for the format spec, the scoring rubric, the decision
//! rules, the abstraction policy, and the honest determinism boundary (bit-exact at
//! the mesh/data layer and for the Canvas 2D PNG; same-adapter reproducible for the
//! GPU PNG).

pub mod build;
pub mod scatter;
pub mod scene;

// The convergence comparator: pure serde/toml logic over authored scorecards, with
// no render or png dependency, so it always compiles and its every branch is tested
// by `cargo test` regardless of feature flags.
pub mod abstraction;
pub mod axes;
pub mod ledger;
pub mod review;

// The pixel comparator and the target-directory orchestration need the `png` decoder
// + render backends, which only the `visual-target` feature pulls in — so they (and
// only they) are feature-gated.
#[cfg(feature = "visual-target")]
pub mod compare;
#[cfg(feature = "visual-target")]
pub mod target;

pub use build::{all_trees, build, RenderData};
pub use scene::Manifest;
