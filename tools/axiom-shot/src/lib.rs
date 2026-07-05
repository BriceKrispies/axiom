//! `axiom-shot` library surface: the renderable-slice [`registry`] and the one
//! shared offscreen-[`capture`] routine.
//!
//! Splitting these out of the binary lets the parity tests
//! (`tests/*_parity.rs`) reuse the *exact* capture path the `axiom-shot` binary
//! renders through, instead of each copying the backend glue — folding the
//! previously-triplicated `present_request`/`write_png`/`frame_packet` boilerplate
//! into a single routine.

pub mod capture;
pub mod registry;
