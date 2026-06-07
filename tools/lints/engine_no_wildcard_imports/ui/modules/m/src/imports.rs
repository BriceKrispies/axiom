// compile-flags: --test
// This fixture's path contains `modules/`, so it is treated as engine code.
// A glob `use foo::*` here MUST be flagged.
// A specific `use foo::{A, B}` must NOT fire.
// Test code must NOT fire.
#![allow(unused_imports)]

// ---- engine code: FLAGGED ----

use std::collections::*;

// ---- engine code: NOT flagged ----

// Specific named import — not a glob, must never fire.
use std::collections::BTreeMap;

// ---- test code in an engine file: NOT flagged ----

#[cfg(test)]
mod tests {
    // A glob inside a test module is exempt.
    use std::collections::*;
}
