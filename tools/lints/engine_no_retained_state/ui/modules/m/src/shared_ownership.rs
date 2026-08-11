// Rule 4 (`shared-state-ownership`): `Rc`, `Arc`, and both `Weak`s, including
// through an alias. `Box<T>` and `Vec<T>` are plain owned data and must NOT fire.
#![allow(dead_code, unused)]

use std::rc::{Rc, Weak as RcWeak};
use std::sync::{Arc, Weak as ArcWeak};

pub struct World {
    pub tick: u32,
}

// ---- FLAGGED ----

pub struct DirectArc {
    value: Arc<World>,
}

pub struct DirectRc {
    value: Rc<World>,
}

pub struct DirectRcWeak {
    value: RcWeak<World>,
}

pub struct DirectArcWeak {
    value: ArcWeak<World>,
}

type Shared<T> = Arc<T>;

pub struct Aliased {
    value: Shared<World>,
}

pub fn hand_out_shared() -> Arc<World> {
    Arc::new(World { tick: 0 })
}

// ---- NOT flagged: plain owned data ----

pub struct OwnedOnly {
    boxed: Box<World>,
    many: Vec<World>,
    text: String,
    fixed: [u32; 4],
}

fn main() {}
