// compile-flags: --test
// This fixture's path contains `modules/`, so it is treated as engine code: a
// naked f32/f64 on the PUBLIC surface here MUST be flagged. Private items,
// private fields, and non-float types must NOT be.
#![allow(dead_code)]

// ---- public surface: FLAGGED ----

// public fn, float parameter
pub fn set_speed(speed: f32) {}

// public fn, float return
pub fn area() -> f64 {
    0.0
}

// public struct with a public float field
pub struct Body {
    pub mass: f32,
}

// public INHERENT-impl methods: a float param and a float return must both be
// flagged, exactly like a free `pub fn`.
pub struct Particle;

impl Particle {
    // public method, float parameter
    pub fn set_speed(&self, speed: f32) {}

    // public method, float return
    pub fn speed(&self) -> f32 {
        0.0
    }
}

// ---- NOT flagged ----

// private fn — out of scope even with a float param.
fn private_speed(x: f32) {}

// public struct, but the float field is private.
pub struct Wrapped {
    mass: f32,
}

// public fn, non-float param.
pub fn ticks(n: u32) {}

// private method on a public type — out of scope.
impl Wrapped {
    fn secret_speed(&self, x: f32) {}
}

// trait-impl method — skipped: the f32 return is the TRAIT's contract, not a
// free choice here, so it must NOT be flagged.
trait Area {
    fn area(&self) -> f32;
}

impl Area for Body {
    fn area(&self) -> f32 {
        0.0
    }
}
