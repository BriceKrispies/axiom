//! # Bend It — a penalty kick you draw before you take it
//!
//! A composition-leaf Axiom app (`apps/bend-it`). Every soccer concept lives
//! here; the engine is reached only through its public facades (the `axiom`
//! umbrella's `RunningApp`, plus `FigureApi`, `InputState`, `AgentApi`, and — on
//! wasm32 — `WindowingApi` + `DebugOverlayApi`).
//!
//! ## The mechanic
//!
//! There is no aim-and-swipe and there is no power meter. The player **designs
//! the shape of the shot** and then watches it be taken:
//!
//! ```text
//! AIM      touch inside the goal          → GoalTarget (normalized h, v)
//! BEND     drag the top-down projection   → horizontal BendCurve
//! HEIGHT   drag the side projection       → vertical  BendCurve
//! KICK     commit                         → one world-space Trajectory
//! ```
//!
//! ## One-way flow
//!
//! ```text
//! pointer + keys  →  DeviceFrame  →  InputState              axiom-input
//!   → drag intents                                           editor::drag
//!   → EditorCommand — the only thing gesture code may say     editor
//!   → ShotIntent (a target and two curves)                    shot::intent
//!   → ONE arc-length-uniform world Trajectory                 shot::trajectory
//!   → fixed-step play state machine                           play::session
//!   → keeper read + physical capsule interception             play::keeper
//!   → camera framed from the viewport                         camera
//!   → retained scene submission                               scene::sync
//!   → screen-space overlay view model                         editor::view
//! ```
//!
//! Each arrow is one-way. The trajectory layer cannot see a pointer; the flight
//! layer cannot see a gesture; and **nothing downstream of `shot::trajectory` is
//! permitted to move the ball off the authored path** — a save is a real capsule
//! contact ([`contact`]), never an edit to the shot.
//!
//! Because the seam between "what the player said" and "what the game did" is a
//! five-word command vocabulary rather than a pile of gestures, a machine can
//! play it too: [`agent`] drives the same commands through the same session.
//!
//! ## Where things live
//!
//! | Concern | Module |
//! |---|---|
//! | Every gameplay number | [`tuning`] |
//! | Coordinates, turf, markings, goal, net | [`pitch`] |
//! | The humanoid, its kit, its poses | [`figure`] |
//! | The authored shot and the path it means | [`shot`] |
//! | The attempt, the ball, the keeper, the result | [`play`] |
//! | Touch/mouse → commands, and the overlay model | [`editor`] |
//! | An embodied agent that plays it | [`agent`] |
//! | Framing, and screen ↔ world | [`camera`], [`projection`] |
//! | The engine scene | [`scene`] |
//! | The diagnostic view | [`debug`] |
//! | The browser | `web` (wasm32 only) |

pub mod agent;
pub mod app;
pub mod camera;
pub mod contact;
pub mod debug;
pub mod editor;
pub mod figure;
pub mod pitch;
pub mod play;
pub mod projection;
pub mod scene;
pub mod shot;
pub mod tuning;

#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::bend_it_start;

pub use app::{build_bend_it, BendIt};

/// The canvas id the browser page binds the surface to.
pub const CANVAS_ID: &str = "axiom-bend-it-canvas";
