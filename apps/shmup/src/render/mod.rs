//! **Render-subsystem support that is content, not engine.**
//!
//! Today: the renderer validation blockout (`probe`), which is generated
//! content driven by the app's own `Rng` and therefore cannot live in a
//! module — a module may not depend on an app.

pub mod probe;
pub mod system;
