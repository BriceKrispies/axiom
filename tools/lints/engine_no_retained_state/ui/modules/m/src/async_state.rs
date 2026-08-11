// Rule 6 (`retained-execution-state`): `async fn`, async blocks, and `Future`
// on a public engine boundary.
#![allow(dead_code, unused)]

use std::future::Future;
use std::pin::Pin;

// ---- FLAGGED: `async fn` ----

pub async fn load() -> u32 {
    1
}

// A private `async fn` is a state machine too.
async fn load_private() -> u32 {
    2
}

// ---- FLAGGED: an async block ----

pub fn make_work() -> impl Future<Output = u32> {
    async { 3 }
}

// ---- FLAGGED: a boxed future on a public boundary ----

pub fn boxed_work() -> Pin<Box<dyn Future<Output = u32>>> {
    Box::pin(async { 4 })
}

// ---- FLAGGED: a public API accepting a future ----

pub fn consume(work: Pin<Box<dyn Future<Output = u32>>>) {}

fn main() {}
