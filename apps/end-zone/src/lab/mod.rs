//! # Animation Lab — view one End Zone player in isolation
//!
//! A standalone diagnostic surface (its own page, `web/lab.html`, and its own
//! wasm entry [`web::end_zone_lab_start`]) that drops a *single* player figure
//! on the field and lets you cycle it through every [`crate::player::AnimState`]
//! — the running/jog/sprint locomotion, the drop-back, and every action / hit
//! / fall override — one at a time.
//!
//! It is not a second engine: it reuses the *exact* animation code the game
//! uses. [`stage::AnimLab`] drives one isolated actor through the same
//! [`crate::presentation::LocomotionAnimator`], the same
//! `crate::player::animation::override_pose`, and the same
//! `crate::player::rig`, so editing `presentation/locomotion/*.rs`,
//! `player/animation.rs`, or `data/locomotion_tuning.rs` changes what the lab
//! shows on the next hot-reload rebuild. That is the whole point: it is the
//! iteration loop for the running animation.
//!
//! The moving clips run the actor on a continuous path with the camera
//! trailing, because the gait advances on *actual* displacement — a stationary
//! treadmill would hide exactly the foot-skate this lab exists to catch.
//!
//! Run it: `cargo run -p axiom-serve -- end-zone`, then open
//! `http://localhost:8080/lab.html` (a source save rebuilds the wasm and
//! reloads the page; the framed clip is restored from the URL hash).

pub mod catalog;
pub mod drive;
pub mod stage;

pub use stage::{AnimLab, LabFrame};

#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::end_zone_lab_start;
