//! # Bend It — a penalty kick you draw before you take it
//!
//! A composition-leaf Axiom app (`apps/bend-it`). Every soccer concept lives
//! here; the engine is reached only through its public facades (the `axiom`
//! umbrella's `RunningApp`, plus `FigureApi`, `InputState`, `AgentApi`, and — on
//! wasm32 — `WindowingApi` + `DebugOverlayApi`).
//!
//! ## The mechanic
//!
//! There is one gesture. You **draw the line you want the ball to take**, and
//! when you let go the line disappears and the kicker takes the closest shot it
//! is actually capable of.
//!
//! ```text
//! draw     one freehand line, anywhere on the pitch
//! release  the line goes; the kicker reads it and strikes
//! watch    the ball follows what was read, and the keeper tries to stop it
//! ```
//!
//! Reading a drawing is a **fit, not a parse**. The line is measured against the
//! shot it most resembles and least-squares fitted onto the space of shots a
//! kicker can strike — two Bézier weights per projection. A clean banana gives a
//! banana; a shaky line gives the smooth shot nearest to it; a scribble gives the
//! best single shot that scribble is evidence for. Nothing is rejected, and the
//! finish is clamped into the goal, so every shot is **valid by construction**.
//!
//! The fit is closed form, so the same pixels always produce the same kick.
//!
//! ## One-way flow
//!
//! ```text
//! pointer + keys  →  DeviceFrame  →  InputState              axiom-input
//!   → a drawn line                                           stroke::capture
//!   → ShotIntent — the only thing a drawing may produce       stroke::interpret
//!   → ONE arc-length-uniform world Trajectory                 shot::trajectory
//!   → fixed-step play state machine                           play::session
//!   → keeper read + physical capsule interception             play::keeper
//!   → camera framed from the viewport                         camera
//!   → retained scene submission                               scene::sync
//!   → screen-space overlay view model                         stroke::view
//! ```
//!
//! Each arrow is one-way. The trajectory layer cannot see a pointer; the flight
//! layer cannot see a drawing; and **nothing downstream of `shot::trajectory` is
//! permitted to move the ball off the authored path** — a save is a real capsule
//! contact ([`contact`]), never an edit to the shot.
//!
//! Because the whole player interface is a line of pixels, a machine can play it
//! too: [`agent`] *draws*, and the game reads its line with the same code and the
//! same loss as a thumb's.
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
//! | Drawing, reading it, and the overlay model | [`stroke`] |
//! | An embodied agent that plays it | [`agent`] |
//! | Framing, and screen ↔ world | [`camera`], [`projection`] |
//! | The engine scene | [`scene`] |
//! | The diagnostic view | [`debug`] |
//! | Every shot vs the keeper, measured | [`matrix`] |
//! | The browser | `web` (wasm32 only) |

pub mod agent;
pub mod app;
pub mod camera;
pub mod contact;
pub mod debug;
pub mod figure;
pub mod matrix;
pub mod pitch;
pub mod play;
pub mod projection;
pub mod scene;
pub mod shot;
pub mod stroke;
pub mod tuning;

#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::bend_it_start;

pub use app::{build_bend_it, BendIt};

/// The canvas id the browser page binds the surface to.
pub const CANVAS_ID: &str = "axiom-bend-it-canvas";













