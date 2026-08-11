// Rule 8 (`opaque-behavior-state`): `dyn Trait` across a public engine boundary.
// Ordinary static dispatch through a concrete type is untouched.
#![allow(dead_code, unused)]

pub trait Renderer {
    fn draw(&self) -> u32;
}

pub struct SoftwareRenderer;

impl Renderer for SoftwareRenderer {
    fn draw(&self) -> u32 {
        0
    }
}

// ---- FLAGGED: `dyn Trait` parameter ----

pub fn render(renderer: &dyn Renderer) -> u32 {
    renderer.draw()
}

// ---- FLAGGED: `dyn Trait` return ----

pub fn pick_renderer() -> Box<dyn Renderer> {
    Box::new(SoftwareRenderer)
}

// ---- FLAGGED: publicly exposed `dyn Trait` field ----

pub struct Pipeline {
    pub renderer: Box<dyn Renderer>,
}

// ---- FLAGGED: publicly exposed `dyn Trait` alias ----

pub type AnyRenderer = Box<dyn Renderer>;

// ---- NOT flagged: static dispatch through a concrete type ----

pub fn render_software(renderer: &SoftwareRenderer) -> u32 {
    renderer.draw()
}

// ---- NOT flagged: a trait declaration by itself is not a state hole ----

pub trait Describe {
    fn describe(&self) -> u32;
}

fn main() {}
