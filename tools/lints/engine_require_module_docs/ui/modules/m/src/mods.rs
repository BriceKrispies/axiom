// compile-flags: --test
// This fixture's path contains `modules/`, so it is treated as engine code.
// `pub mod` without a doc comment MUST be flagged.
// `pub mod` with a doc comment must NOT be flagged.
// Private `mod` (no `pub`) must NOT be flagged.
#![allow(dead_code)]

// ---- engine code: FLAGGED (pub mod, no doc) ----

pub mod undocumented {}

// ---- engine code: NOT flagged (pub mod, has outer doc comment) ----

/// Scene graph and transform hierarchy. Depends only on kernel and math.
pub mod documented {}

// ---- engine code: NOT flagged (pub mod, has inner doc comment) ----

pub mod documented_inner {
    //! Inner-documented module — equally valid.
}

// ---- engine code: NOT flagged (private mod, no pub) ----

mod private_one {}
