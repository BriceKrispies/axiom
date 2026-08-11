// The recursive type inspector, at its two descent depths.
//
// `const` / `static` / alias types are walked *transitively* — through the
// fields of crate-local ADTs — because the whole composed value is the subject
// and there is no inner declaration to blame. Fields and signatures are walked
// *structurally*, so a prohibited type is reported once, at the declaration that
// introduces it, and not again at every type that transitively contains it.
#![allow(dead_code, unused)]

use std::cell::{Cell, RefCell};
use std::thread::LocalKey;

// ---- FLAGGED once, at the field that introduces the `RefCell` ----

pub struct Inner {
    slot: RefCell<u32>,
}

// ---- NOT flagged: `Outer` merely contains `Inner`, which is already blamed ----

pub struct Outer {
    inner: Inner,
}

// ---- NOT flagged: passing the composed type around blames nothing new ----

pub fn read(outer: &Outer) -> u32 {
    0
}

// ---- FLAGGED: a `const` is walked transitively, so the `RefCell` two structs
// down is found here too. A `const` holding interior mutability is the classic
// "every use is a fresh copy" trap, and there is no inner `const` to blame. ----

pub const EMPTY: Option<Outer> = None;

// ---- FLAGGED: transitive descent terminates on a recursive ADT ----
//
// `Chain` -> `Option<Box<Chain>>` -> `Chain` revisits an already-visited
// interned type and stops; the walk then continues to `slot` and reports.

pub struct Chain {
    next: Option<Box<Chain>>,
    slot: Cell<u32>,
}

pub const NO_CHAIN: Option<Chain> = None;

// ---- FLAGGED: `LocalKey` by resolved type identity, with no `thread_local!`
// macro anywhere in sight ----

pub struct HandWrittenKey {
    key: &'static LocalKey<u32>,
}

fn main() {}
