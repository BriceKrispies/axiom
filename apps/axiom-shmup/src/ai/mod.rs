//! Ported from Claude-of-Duty `src/ai/` — **the whole directory**.
//!
//! | this module      | source |
//! |------------------|--------|
//! | [`system`]       | `index.js` — `AiSystem`: boot, garrison, LOD, tableaus |
//! | [`nav`]          | `nav.js` — walkability grid + A* + string pull + cover |
//! | [`agent`]        | `agent.js` — perception, reaction delay, sound, FSM |
//! | [`squad`]        | `squad.js` — peek tokens, contact sharing, flank/grenade rationing |
//! | [`grounding`]    | `grounding.js` — contact-shadow placement math (not the draw) |
//! | [`soldier`]      | `soldier.js` — the variants and their material requests |
//! | [`parts`]        | `parts.js` — head, torso, webbing, plate, limbs, kit |
//! | [`geo`]          | `geo.js` — `CharacterBuilder`, the skinned buffer set |
//! | [`rig`]          | `rig.js` — the bone table |
//! | [`clips`]        | `clips.js` — the animation clips |
//! | [`animator`]     | `animator.js` — layered pose blending, IK, weapon anchors |
//! | [`textures`]     | `textures.js` + `bake.js` — the procedural material bakes |
//! | [`weapon`]       | `weapon.js` — `buildWeapon(nz, style, rng)`, one geometry builder |
//!
//! ## What is genuinely not here
//!
//! * `preview.js` — a dev-only model previewer with its own page, not part of
//!   the game's frame.
//! * `bake.js`'s worker-thread scheduling (`SOLDIER_SHARDS`, `only`, `bakeMs`).
//!   [`textures`] bakes the same ten tiles from the same seeds, synchronously.
//! * The scene graph. `THREE.Group`, `SkinnedMesh` and the `ai.root` subtree are
//!   render bookkeeping; [`crate::scene::wiring::soldier_draw`] is this port's
//!   equivalent and it lives in the composing tier, not here.
//!
//! ## Where this slice stops, and who carries it the rest of the way
//!
//! Everything above is *behaviour and data*. Two seams remain outside it:
//!
//! * [`crate::scene::wiring::ai`] constructs [`system::AiCore`] against the real
//!   level, physics and camera, and steps it once per frame.
//! * [`crate::scene::wiring::soldier_draw`] turns [`soldier::SoldierBuild`]'s
//!   geometry into engine meshes and [`animator::Animator`]'s bones into a joint
//!   palette. **That file, not this one, is where baked detail is currently
//!   lost** — per-vertex colour (the baked AO and edge wear), the two detail
//!   tiles, `normalScale`/`aoMapIntensity` and the rim term are all baked here
//!   and dropped there. See its module doc for which engine boundary each hits.
//!
//! Each subsystem here that needs something it does not own names the narrowest
//! trait it actually calls, rather than reaching for a whole facade —
//! [`grounding::FootSource`] is one bone-position call
//! (`agent.animator.bonePos`); [`agent::Agent`]'s movement decision
//! ([`agent::Agent::move_step`]) stops short of driving a physics character
//! controller for the same reason. This mirrors the precedent already set by
//! `crate::audio::spatial::WorldProbe` and `crate::player::mantle::WorldProbe`
//! for physics, and `crate::player::movement::CharacterController` for the
//! swept character controller itself.
//!
//! ## Determinism
//!
//! Every `Agent` and every `Squad` takes its own [`crate::rng::Rng`], forked
//! from the AI subsystem's stream exactly once at creation — `ai/index.js:55`
//! (`this.rng = ctx.rng.fork()`), `agent.js:97` (`this.rng = ai.rng.fork()`),
//! and `agent.js:541` (`createSquad()` -> `new Squad(this.rng.fork())`). This
//! port preserves that: [`agent::Agent::new`] and [`squad::Squad::new`] both
//! take an already-forked [`crate::rng::Rng`] rather than forking internally,
//! so a caller assembling a garrison controls the fork order exactly as
//! `ai/index.js`'s `populate()` does — draw order is part of the contract.

pub mod agent;
pub mod animator;
pub mod clips;
pub mod geo;
pub mod grounding;
pub mod nav;
pub mod parts;
pub mod rig;
pub mod soldier;
pub mod squad;
pub mod system;
pub mod textures;
pub mod weapon;
