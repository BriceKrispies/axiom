// Rule 7 (`stateful-callback-boundary`) and rule 9 (`generic-behavior-state`).
//
// A closure can capture arbitrary hidden state; a plain function pointer cannot,
// and a data generic carries no behavior at all.
#![allow(dead_code, unused)]

use std::future::Future;

pub struct Input;
pub struct Output;

// ---- FLAGGED: concrete callback types on a public boundary (rule 7) ----

pub fn with_boxed_callback(f: Box<dyn Fn(Input) -> Output>) {}

pub fn with_callback_ref(f: &dyn FnMut(Input)) {}

pub fn with_once_callback(f: Box<dyn FnOnce()>) {}

pub fn return_a_callback() -> Box<dyn Fn(Input) -> Output> {
    unreachable!()
}

// ---- FLAGGED: a public field storing a callback ----

pub struct Hooks {
    pub on_tick: Box<dyn Fn(Input)>,
}

// ---- FLAGGED: behavioral generic parameters (rule 9) ----

pub fn apply<F: Fn(Input) -> Output>(f: F) {}

pub fn apply_mut<F>(f: F)
where
    F: FnMut(Input),
{
}

pub fn apply_impl(f: impl Fn(Input) -> Output) {}

pub fn await_it<F: Future<Output = u32>>(f: F) {}

// ---- NOT flagged: a plain function pointer carries no environment ----

pub fn with_fn_pointer(f: fn(Input) -> Output) {}

pub struct PurePipeline {
    pub transform: fn(Input) -> Output,
}

// ---- NOT flagged: a data generic ----

pub struct StateTable<K, V> {
    pub keys: Vec<K>,
    pub values: Vec<V>,
}

pub fn lookup<K: Clone, V: Clone>(table: &StateTable<K, V>) -> Option<V> {
    table.values.first().cloned()
}

// ---- NOT flagged: an invocation-local closure that never escapes ----

pub fn sum(values: &[u32]) -> u32 {
    values.iter().map(|value| value * 2).sum()
}

fn main() {}
