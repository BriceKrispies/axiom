//! The FX subsystem's physics seam.
//!
//! Not a port of one source file: `fx/*.js` reaches `ctx.peek('physics')`
//! from half a dozen call sites (`decals.js`'s triangle-soup query,
//! `impacts.js`'s spark bounce raycast, `index.js`'s `scorch`/
//! `bloodSpatterBehind`/`onActorDeath`/`onLand` ground probes). This module
//! names that one capability once, in the shape every call site needs,
//! following the precedent [`crate::weapons::ballistics::RaycastWorld`]
//! already established for the same problem in the weapons port: the
//! physics module has not landed a public surface for this yet, so a narrow
//! trait stands in rather than a concrete type that does not exist.
//!
//! [`FxWorld`] extends [`crate::fx::decals::DecalWorld`] (the triangle-soup
//! query decals need) with the raycast and ground-probe shapes `index.js`
//! and `impacts.js` also need, so one implementer — whatever eventually
//! wraps `crate::physics::bvh::StaticWorld` — satisfies the whole seam.

use crate::fx::decals::DecalWorld;
use crate::world::palette::Surface;

/// `HitRecord`'s FX-relevant fields — mirrors `crate::physics::math::
/// HitRecord` (already the shape a `StaticWorld::raycast` returns) rather
/// than reinventing one, so a future implementer forwards it directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxHit {
    pub point: (f64, f64, f64),
    pub normal: (f64, f64, f64),
    pub distance: f64,
    pub surface: Surface,
}

/// The physics capability every FX call site needs. See the module doc.
pub trait FxWorld: DecalWorld {
    /// `phys.raycast(origin, dir, maxDist, mask)`.
    fn raycast(&self, origin: (f64, f64, f64), dir: (f64, f64, f64), max_dist: f64, mask: u16) -> Option<FxHit>;

    /// `phys.groundHeight(x, z, fromY)` — `index.js:673-676, 705-708`. `None`
    /// mirrors the source's `Number.isFinite(gy)` guard.
    fn ground_height(&self, x: f64, z: f64, from_y: f64) -> Option<f64>;
}
