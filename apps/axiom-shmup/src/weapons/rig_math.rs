//! The viewmodel rig's `f64` vector/quaternion/matrix kit — **now the engine's**.
//!
//! This file used to hold its own `V3`, `Q` and `M4`, transcribed line-for-line
//! from `three@0.180`. Its own module doc gave two reasons for not reusing
//! `axiom_math`, and both were correct at the time:
//!
//! > `axiom_math::Quat::from_euler_xyz` composes `qz*qy*qx` where THREE's
//! > `'XYZ'` order composes `qx*qy*qz` — a different rotation for the same three
//! > angles — and `axiom_math` are `f32` throughout, while the rig integrates
//! > every frame in `f64`.
//!
//! Both are now answered rather than worked around. [`axiom_math::DQuat`] is
//! transcribed from the same THREE source this file was (it *is* this file's
//! code, moved), and the whole `D`-prefixed family is `f64`. So the kit is an
//! engine capability living in an engine layer, and what remains here is the
//! three names the rig calls them by.
//!
//! The aliases stay because `V3`/`Q`/`M4` is what `viewmodel.js` and `hands.js`
//! call them and what every call site in this app reads. Renaming ~600 call
//! sites to prove a point would be churn, not progress.

pub use axiom_math::{DMat4 as M4, DQuat as Q, DVec3 as V3};
