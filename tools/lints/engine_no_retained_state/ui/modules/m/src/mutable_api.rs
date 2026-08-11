// Rule 5 (`mutable-engine-api`): a public engine API may not mutate caller-owned
// state in place. Private helpers that use `&mut` to construct their output are
// explicitly legal.
#![allow(dead_code, unused)]

pub struct GameState {
    pub score: u32,
}

pub struct World;

pub struct Effect;

// ---- FLAGGED: public `&mut T` parameter ----

pub fn update(state: &mut GameState) {}

// ---- FLAGGED: public `&mut` nested in a straightforward wrapper ----

pub fn maybe_update(state: Option<&mut GameState>) {}

pub fn update_slice(states: &mut [GameState]) {}

// ---- FLAGGED: public `&mut` return ----

pub fn world_mut(state: &'static mut GameState) -> &'static mut World {
    unreachable!()
}

pub struct Engine {
    world: World,
}

impl Engine {
    // ---- FLAGGED: public `&mut self` ----
    pub fn step(&mut self, input: u32) {}

    // ---- FLAGGED: public `&mut self` returning `&mut T` ----
    pub fn get_mut(&mut self) -> &mut World {
        &mut self.world
    }

    // ---- NOT flagged: shared-reference reader ----
    pub fn world(&self) -> &World {
        &self.world
    }
}

// ---- FLAGGED: an engine-defined public trait that mandates `&mut self` ----

pub trait Stepper {
    fn step(&mut self, input: u32);
}

// ---- NOT flagged: a private helper mutating a local output buffer ----

fn push_effects(out: &mut Vec<Effect>) {
    out.push(Effect);
}

// ---- NOT flagged: local mutation used to construct the returned value ----

pub fn calculate(input: &GameState) -> Vec<Effect> {
    let mut effects = Vec::new();
    push_effects(&mut effects);
    effects
}

fn main() {}
