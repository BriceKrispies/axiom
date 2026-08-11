// Rule 10 (`stateful-trait-implementation`) and rule 11
// (`drop-side-effect-hole`).
#![allow(dead_code, unused)]

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct Counter {
    remaining: u32,
}

// ---- FLAGGED: an engine-defined Iterator is a resumable state machine ----

impl Iterator for Counter {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        self.remaining = self.remaining.saturating_sub(1);
        Some(self.remaining)
    }
}

impl DoubleEndedIterator for Counter {
    fn next_back(&mut self) -> Option<u32> {
        self.next()
    }
}

// ---- FLAGGED: an engine-defined Future ----

pub struct Pending;

impl Future for Pending {
    type Output = u32;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<u32> {
        Poll::Ready(0)
    }
}

// ---- FLAGGED: a custom Drop ----

pub struct Session {
    pub id: u32,
}

impl Drop for Session {
    fn drop(&mut self) {}
}

// ---- NOT flagged: locally consuming an iterator produced elsewhere ----

pub fn total(values: &[u32]) -> u32 {
    values.iter().copied().sum()
}

// ---- NOT flagged: a plain data type whose std fields drop normally ----

pub struct PlainData {
    pub values: Vec<u32>,
    pub name: String,
}

fn main() {}
