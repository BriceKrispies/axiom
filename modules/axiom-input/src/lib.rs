//! # Axiom Input — Engine Module
//!
//! Device-agnostic intent synthesis. A *pointer* here is a single contact —
//! a mouse with its primary button down, a finger, or a pen tip — all reduced
//! to the same neutral shape `(position, is_down)` by the platform edge before
//! it reaches this module. From a frame's worth of those samples plus a virtual
//! on-screen layout, [`TouchControls`] synthesizes input for two schemes: the
//! analog first-person scheme (`update` — a left-thumb **move vector** and a
//! right-thumb **look** drag) and a discrete grid scheme (`swipe` — one
//! directional flick per gesture, for turn-based games). An app uses whichever
//! fits; both share the one facade.
//!
//! ## Why this exists
//! Before this module, every interactive app hand-rolled desktop-only input:
//! `requestPointerLock` + `movementX/Y` + WASD `KeyboardEvent`s, none of which
//! exist on a touchscreen. The gallery faked mobile support with a JS shim that
//! synthesized `KeyboardEvent`s from on-screen buttons. This module makes touch
//! the *primary* input and a mouse just one more pointer source feeding the same
//! synthesis — the mobile-first inversion, done once, in a reusable place.
//!
//! ## What this module is / is not
//! - It **is** a pure, deterministic synthesis core: same surface + same pointer
//!   samples in, same control frame out, fully testable on native.
//! - It is **not** a browser event source. It never references `web_sys` /
//!   PointerEvents / the DOM (Module Law #9). The capture that produces its
//!   pointer samples lives in the platform-facing `windowing` module.
//! - It is **not** a controller/camera system: it produces *intent* (a move
//!   vector, look deltas); the app maps that onto its `FirstPersonInput`.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`TouchControls`]. The control frame
//! it returns is reached only through it.

mod control_frame;
mod touch_controls;

pub use touch_controls::TouchControls;
