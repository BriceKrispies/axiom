// This fixture's path has an `axiom-kernel` crate component, the other half of
// the lint's scalar floor (the crate that owns the dimensioned-scalar primitives
// themselves, plus serialization and telemetry). A naked f32 on the public
// surface here is the CORRECT type, so the lint must stay SILENT — no `.stderr`
// accompanies this file. (Expected output: empty.)
#![allow(dead_code)]

pub struct Meters(f32);

impl Meters {
    // The quantity primitive's own constructor necessarily takes a raw f32.
    pub fn new(value: f32) -> Self {
        Meters(value)
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

// Serialization primitive — raw f32 by definition.
pub fn write_f32(value: f32) {
    let _ = value;
}

fn main() {}
