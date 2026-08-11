// Rule 3 (`interior-mutability`), including the alias / deep-nesting cases that
// prove detection is by resolved type identity, not by spelling.
#![allow(dead_code, unused)]

use std::cell::{Cell, RefCell, UnsafeCell};
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Mutex, RwLock};

pub struct World {
    pub tick: u32,
}

// ---- FLAGGED: written directly ----

pub struct DirectCell {
    value: RefCell<u32>,
}

pub struct DirectMutex {
    value: Mutex<World>,
}

pub struct DirectRwLock {
    value: RwLock<World>,
}

pub struct DirectAtomic {
    value: AtomicU32,
}

pub struct DirectAtomicBool {
    value: AtomicBool,
}

pub struct DirectCellNewtype {
    value: Cell<u32>,
}

pub struct DirectUnsafeCell {
    value: UnsafeCell<u32>,
}

// ---- FLAGGED: behind an alias ----

type Hidden = RefCell<World>;

pub struct Aliased {
    value: Hidden,
}

// ---- FLAGGED: deeply nested behind an alias ----
//
// The declared type is `Option<Box<Hidden>>`; the resolved type is
// `Option<Box<RefCell<World>>>`, which is what the walker sees.

pub struct DeeplyNested {
    value: Option<Box<Hidden>>,
}

// ---- FLAGGED: nested inside a tuple / Vec / Result ----

pub struct NestedInCollections {
    value: Vec<(u32, Result<Mutex<World>, ()>)>,
}

// ---- FLAGGED: crossing a function boundary ----

pub fn hand_out_a_cell() -> RefCell<World> {
    RefCell::new(World { tick: 0 })
}

// ---- FLAGGED: a recursive ADT terminates via the visited set ----

pub struct Node {
    next: Option<Box<Node>>,
    slot: Cell<u32>,
}

fn main() {}
