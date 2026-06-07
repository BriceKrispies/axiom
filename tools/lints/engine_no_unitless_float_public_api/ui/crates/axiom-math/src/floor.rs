// This fixture's path has an `axiom-math` crate component, which is part of the
// lint's scalar floor (the dimensionless linear-algebra layer). Naked f32 on the
// public surface here is the CORRECT type, so the lint must stay SILENT — no
// `.stderr` accompanies this file. (Expected output: empty.)
#![allow(dead_code)]

pub struct Vector;

impl Vector {
    // A dimensionless constructor and accessor — raw f32 is correct here.
    pub fn new(x: f32, y: f32) -> Self {
        let _ = (x, y);
        Vector
    }

    pub fn length(&self) -> f32 {
        0.0
    }
}

// A free fn and a public field on the floor are also fine.
pub fn dot(a: f32, b: f32) -> f32 {
    a * b
}

pub struct Component {
    pub value: f32,
}

fn main() {}
