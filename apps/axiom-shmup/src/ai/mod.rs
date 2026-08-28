//! AI — navigation, perception and squad behaviour.
//!
//! Ported from Claude-of-Duty `src/ai/`, this slice only:
//!
//! | this module     | source        |
//! |------------------|----------------|
//! | [`nav`]          | `nav.js` — walkability grid + A* + string pull + cover |
//! | [`agent`]        | `agent.js` — perception, reaction delay, sound, FSM (not the body) |
//! | [`squad`]        | `squad.js` — peek tokens, contact sharing, flank/grenade rationing |
//! | [`grounding`]    | `grounding.js` — contact-shadow placement math (not the draw) |
//!
//! ## What is deliberately not in this slice
//!
//! `src/ai/soldier.js`, `parts.js`, `rig.js`, `animator.js`, `clips.js`,
//! `geo.js`, `textures.js`, `weapon.js` — the character *rendering and
//! animation* half: skeleton, skinned mesh, layered pose blending, IK,
//! per-bone hitbox sync, muzzle/tracer/shell events, and the carried weapon
//! model. A later slice ports those.
//!
//! `src/ai/index.js` (`AiSystem`) is also not ported here: it is the
//! orchestration tier — booting navigation, prewarming character shaders,
//! spawning and garrisoning the level, the frame-wide events wiring
//! (`weapon:fire`/`bullet:impact`/`explosion`/`player:footstep`), the
//! per-frame A* budget rationing (`requestPath`, `pathsPerFrame`), the LOD
//! relevance sweep, and the staged capture tableaus. It is the natural home
//! for gluing [`nav`], [`agent`] and [`squad`] together once the deferred
//! body/animation slice lands and there is a real character to drive; wiring
//! it prematurely against a bodyless `Agent` would mean inventing behaviour
//! this port has no source to check against.
//!
//! Every subsystem here that needs the unported body work names the
//! narrowest trait it actually calls, rather than waiting on the whole
//! slice — [`grounding::FootSource`] is one bone-position call
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
