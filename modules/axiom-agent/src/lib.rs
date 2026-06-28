//! # Axiom Agent — Engine Module
//!
//! A deterministic, headless **embodied-agent substrate**. It models one neutral
//! loop and nothing else:
//!
//! ```text
//! observe -> decide -> emit player-equivalent intents -> record decision report
//! ```
//!
//! ## What this module is
//! - An *isolated* engine module depending only on the approved layers
//!   [`axiom_kernel`] (the `Tick` identity and the result/error model) and
//!   [`axiom_runtime`] (the deterministic `RuntimeStep` that drives one decision).
//! - The single owner of the neutral agent **contracts**: a bounded
//!   [`Observation`](crate::AgentApi), a memory-bounded agent store, a set of
//!   player-equivalent [`ActionIntent`](crate::AgentApi)s, two deterministic
//!   brains (scripted + replay), and the per-step [`DecisionReport`](crate::AgentApi).
//!
//! ## What this module is not
//! It is **not** an AI framework. There is no neural network, no machine
//! learning, no LLM, no pathfinding, no navmesh, no behavior tree, no utility-AI,
//! and no planner. It is **not** enemy AI and carries no player or gameplay
//! rules. It does not render, does not touch a scene/physics/asset/input device,
//! and does not drive a game loop. It reads no wall-clock and uses no randomness.
//!
//! Apps translate their own scene/sim/render/game state into an `Observation`,
//! and translate the emitted `ActionIntent`s back into concrete input. Machine
//! vision is represented here only as the declared
//! [`ObservationChannel`](crate::AgentApi) `screen_sample` — a label a future
//! app/tool may fill, never an implementation that lives in this module.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** thing: the [`AgentApi`] facade. Every
//! contract type (`AgentId`, `AgentProfile`, `AgentMemory`, `Observation`,
//! `ObservationBuilder`, `ObservationChannel`, `ActionIntent`, `ActionQueue`,
//! `ScriptedBrain`, `ReplayBrain`, `DecisionReport`) is sealed behind a private
//! module and is reachable only *through* the facade — constructed by it and
//! handed back as opaque values.

mod action_intent;
mod action_queue;
mod agent_api;
mod agent_brain;
mod agent_id;
mod agent_memory;
mod agent_profile;
mod agent_runtime;
mod decision_report;
mod observation;
mod observation_builder;
mod observation_channel;
mod replay_brain;
mod scripted_brain;

pub use agent_api::AgentApi;
