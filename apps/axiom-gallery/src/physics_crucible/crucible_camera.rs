//! Deterministic camera placement for the crucible.
//!
//! Pure functions returning `(eye, target)` pairs the renderer turns into a
//! `looking_at` transform. The overview frames the whole 3×2 room; a per-station
//! view frames one cell for a focused screenshot. No state, no time, no randomness.

use axiom::prelude::Vec3;

use crate::physics_crucible::crucible_station::CrucibleStation;

/// Eye + look-target framing the entire room from above and behind `+Z`. Pulled
/// in close enough that the individual bodies read as shapes (not specks) while
/// the full 3×2 station grid stays in frame.
pub fn overview() -> (Vec3, Vec3) {
    (Vec3::new(0.0, 16.0, 27.0), Vec3::new(0.0, 2.0, 0.0))
}

/// A slowly orbiting eye (and fixed look-target) framing the whole room, for the
/// live browser demo. `step` is the simulation step, which drives the orbit angle
/// — a full revolution every ~1050 steps — so the camera circles the room while
/// the physics plays out beneath it.
pub fn orbit(step: u64) -> (Vec3, Vec3) {
    let target = Vec3::new(0.0, 2.0, 0.0);
    let angle = step as f32 * 0.006;
    let radius = 28.0;
    let eye = Vec3::new(
        target.x + angle.sin() * radius,
        16.0,
        target.z + angle.cos() * radius,
    );
    (eye, target)
}

/// Eye + look-target framing a single station's cell.
pub fn station_view(station: CrucibleStation) -> (Vec3, Vec3) {
    let origin = station.origin();
    let eye = origin.add(Vec3::new(0.0, 6.0, 12.0));
    let target = origin.add(Vec3::new(0.0, 1.0, 0.0));
    (eye, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_is_above_and_behind_the_origin() {
        let (eye, target) = overview();
        assert!(eye.y > target.y);
        assert!(eye.z > target.z);
    }

    #[test]
    fn orbit_circles_the_room_at_a_fixed_height() {
        let (e0, t0) = orbit(0);
        let (e1, _t1) = orbit(200);
        assert_eq!(e0.y, e1.y, "the orbit holds a fixed height");
        assert_ne!((e0.x, e0.z), (e1.x, e1.z), "the eye moves around the room");
        assert_eq!(t0, Vec3::new(0.0, 2.0, 0.0));
    }

    #[test]
    fn station_view_is_offset_from_the_cell_origin() {
        let s = CrucibleStation::StressBay;
        let (eye, target) = station_view(s);
        assert_eq!(target, s.origin().add(Vec3::new(0.0, 1.0, 0.0)));
        assert!(eye.z > target.z);
    }
}
