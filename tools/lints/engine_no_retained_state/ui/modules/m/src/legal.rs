// The explicitly legal half of the state law. This file is engine spine code
// (`modules/.../src`) and MUST produce ZERO findings — there is no `.stderr`
// beside it.
//
// «Explicit state data is legal. Hidden retained state in engine behavior is
// illegal.»
#![allow(dead_code, unused)]

use std::collections::BTreeMap;

// ---- constants ----

const MAX_PLAYERS: usize = 64;
const FIXED_STEP_NS: u64 = 16_666_667;

// ---- plain state data ----

#[derive(Clone)]
pub struct BallState {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone)]
pub struct BaseballState {
    pub score: u32,
    pub inning: u8,
    pub ball: BallState,
}

pub struct Input {
    pub swing: bool,
}

pub struct Effect;

pub struct SimulationState {
    pub tick: u64,
}

pub struct SimulationInput {
    pub delta_ns: u64,
}

// ---- owned collections are explicit data, not retained state ----

pub struct Snapshot {
    pub players: Vec<BaseballState>,
    pub by_name: BTreeMap<String, u32>,
    pub boxed: Box<BallState>,
    pub fixed: [u32; MAX_PLAYERS],
    pub pair: (u32, u8),
    pub label: String,
}

// ---- immutable input, explicit output ----

pub fn step(state: &BaseballState, input: &Input) -> BaseballState {
    let mut next = state.clone();
    next.score = next.score.saturating_add(u32::from(input.swing));
    next
}

pub fn advance(state: &SimulationState, input: &SimulationInput) -> SimulationState {
    SimulationState {
        tick: state.tick + input.delta_ns / FIXED_STEP_NS,
    }
}

// ---- local mutation used to construct the returned value ----

pub fn build_effects(input: &Input) -> Vec<Effect> {
    let mut effects = Vec::new();
    effects.push(Effect);
    collect_more(&mut effects);
    effects
}

// ---- a private helper taking `&mut Vec<Effect>` for output construction ----

fn collect_more(out: &mut Vec<Effect>) {
    out.push(Effect);
}

// ---- plain function pointers ----

pub struct Rules {
    pub score_for: fn(&BaseballState) -> u32,
}

pub fn apply_rule(state: &BaseballState, rule: fn(&BaseballState) -> u32) -> u32 {
    rule(state)
}

// ---- deterministic data generics ----

pub struct StateTable<K, V> {
    pub keys: Vec<K>,
    pub values: Vec<V>,
}

pub fn first_value<K, V: Clone>(table: &StateTable<K, V>) -> Option<V> {
    table.values.first().cloned()
}

// ---- static dispatch through a concrete type ----

pub struct Scorer;

impl Scorer {
    pub fn new() -> Self {
        Scorer
    }

    pub fn score(&self, state: &BaseballState) -> u32 {
        state.score
    }
}

// ---- consuming an iterator from a dependency is fine ----

pub fn total(states: &[BaseballState]) -> u32 {
    states.iter().map(|state| state.score).sum()
}

fn main() {}
