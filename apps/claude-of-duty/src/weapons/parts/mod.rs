//! Ported from Claude-of-Duty `src/weapons/parts.js` — reusable firearm
//! components, split by concern across sibling modules landing from a
//! concurrent port pass against the `geometry` module's fixed API contract
//! (`docs/work-manifests/claude-of-duty-port/03-weapon-geometry-api.md`).

pub mod barrel;
pub mod controls;
pub mod hardware;
pub mod magazine;
pub mod optics;
pub mod receiver;
