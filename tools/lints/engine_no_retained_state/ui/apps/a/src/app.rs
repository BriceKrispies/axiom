// The path contains `apps/`, not `crates/` or `modules/`, so this is a
// composition leaf outside the engine spine. Apps are allowed to own the current
// explicit state snapshot and imperative host/platform resources, so NOTHING
// here may be flagged — there is no `.stderr` beside this file.
#![allow(dead_code, unused)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

pub struct World {
    pub tick: u32,
}

static FRAME_COUNT: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static SCRATCH: RefCell<u32> = const { RefCell::new(0) };
}

pub struct Host {
    world: Rc<RefCell<World>>,
    shared: Arc<Mutex<World>>,
    hooks: Box<dyn Fn(u32)>,
    raw: *mut u32,
}

impl Host {
    pub fn step(&mut self, input: u32) {}

    pub fn world_mut(&mut self) -> &mut World {
        unreachable!()
    }
}

impl Drop for Host {
    fn drop(&mut self) {}
}

pub async fn boot() -> u32 {
    unsafe { 0 }
}

fn main() {}
