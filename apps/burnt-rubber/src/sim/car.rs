//! [`CarState`] — the complete deterministic state of the player's car, and the
//! surface classification the controller reads grip from.
//!
//! Velocity is stored **in the chassis frame** (a forward component and a
//! lateral component) rather than as a world vector. That is not a storage
//! detail, it is the model: rotating the chassis converts forward velocity into
//! lateral velocity for free, and "grip" is then simply how fast the lateral
//! component bleeds away. A drift is what you see when the chassis has rotated
//! faster than the grip can re-align the velocity — no tyre forces, no slip
//! angles, no friction circle, and no way for a bad contact to blow the
//! integrator up.

use axiom_math::Vec3;

/// What the car is standing on. Ordered by how much grip it offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The driving surface.
    Tarmac,
    /// The rumble-stripped shoulder: a warning, not a punishment.
    Shoulder,
    /// Dirt. Slow, loose, and where a mistake actually costs you.
    OffRoad,
}

impl Surface {
    /// Whether this surface is off the driving line — the single question the
    /// grip, drag and HUD all ask.
    pub const fn is_off_road(self) -> bool {
        !matches!(self, Surface::Tarmac)
    }

    /// A stable index, so per-surface values can be table-selected.
    pub const fn index(self) -> usize {
        match self {
            Surface::Tarmac => 0,
            Surface::Shoulder => 1,
            Surface::OffRoad => 2,
        }
    }
}

/// The car's deterministic simulation state.
///
/// Everything here is written by the fixed step and read by presentation.
/// Nothing here is written by presentation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarState {
    /// World position of the chassis centre, on (or above) the road surface.
    pub position: Vec3,
    /// Chassis heading (radians), `0` = `+Z`.
    pub yaw: f32,
    /// Yaw rate (rad/s) applied this step — the camera and the body roll read it.
    pub yaw_rate: f32,
    /// Velocity along the chassis nose (m/s). Negative is reverse.
    pub forward_speed: f32,
    /// Velocity across the chassis (m/s). This *is* the drift.
    pub lateral_speed: f32,
    /// Vertical velocity (m/s), non-zero only over crests and on landing.
    pub vertical_speed: f32,
    /// Whether the car is on the surface.
    pub grounded: bool,
    /// Consecutive steps spent airborne.
    pub airborne_steps: u32,
    /// The smoothed steering input actually being applied (`-1..1`).
    pub steer: f32,
    /// Arc length along the course (m) — the progress coordinate.
    pub distance: f32,
    /// Signed offset from the road centre (m), positive to the driver's right.
    pub lateral: f32,
    /// The surface under the car.
    pub surface: Surface,
    /// Whether the car is sliding hard enough to count as drifting.
    pub drifting: bool,
    /// Consecutive steps spent drifting.
    pub drift_steps: u32,
    /// Steps remaining on the current impact state (`0` = no impact).
    pub impact_steps: u32,
    /// World direction of the most recent impact, for the camera kick.
    pub impact_direction: Vec3,
    /// Strength of the most recent impact, `0..1`.
    pub impact_strength: f32,
    /// Yaw rate (rad/s) a collision is still imparting, decaying on its own.
    ///
    /// A collision's rotation is *state*, not a one-shot `yaw +=`, and that is
    /// the whole reason the recovery assist can help with it: a disturbance
    /// applied and forgotten in a single step is a disturbance nothing can damp.
    /// It is added to the steering's yaw rate rather than replacing it, so the
    /// player keeps the wheel throughout. See [`super::contact`].
    pub impact_yaw_rate: f32,
    /// Accumulated wheel rotation (radians), driven by distance travelled.
    pub wheel_spin: f32,
    /// Whether boost is being spent this step.
    pub boosting: bool,
    /// Consecutive steps spent slow and off the road — the auto-reset prompt.
    pub stuck_steps: u32,
    /// Fraction of the car's load thrown onto the outside wheels by the corner
    /// it is currently taking (`0` straight, `1` inside wheels lifting).
    ///
    /// Written by the controller from the chassis geometry, read by presentation
    /// for body roll. It is the simulation's one honest measure of how hard the
    /// car is leaning on its tyres.
    pub load_transfer: f32,
}

impl CarState {
    /// A car parked at `position`, pointing along `yaw`.
    pub fn parked(position: Vec3, yaw: f32) -> CarState {
        CarState {
            position,
            yaw,
            yaw_rate: 0.0,
            forward_speed: 0.0,
            lateral_speed: 0.0,
            vertical_speed: 0.0,
            grounded: true,
            airborne_steps: 0,
            steer: 0.0,
            distance: 0.0,
            lateral: 0.0,
            surface: Surface::Tarmac,
            drifting: false,
            drift_steps: 0,
            impact_steps: 0,
            impact_direction: Vec3::UNIT_Z,
            impact_strength: 0.0,
            impact_yaw_rate: 0.0,
            wheel_spin: 0.0,
            boosting: false,
            stuck_steps: 0,
            load_transfer: 0.0,
        }
    }

    /// The chassis nose direction (horizontal, unit).
    pub fn forward(&self) -> Vec3 {
        let (s, c) = self.yaw.sin_cos();
        Vec3::new(s, 0.0, c)
    }

    /// The chassis right direction (horizontal, unit).
    pub fn right(&self) -> Vec3 {
        let (s, c) = self.yaw.sin_cos();
        Vec3::new(c, 0.0, -s)
    }

    /// The world-space velocity, rebuilt from the chassis-frame components.
    pub fn velocity(&self) -> Vec3 {
        self.forward()
            .mul_scalar(self.forward_speed)
            .add(self.right().mul_scalar(self.lateral_speed))
            .add(Vec3::new(0.0, self.vertical_speed, 0.0))
    }

    /// Ground speed (m/s) — what the speedometer reads, always non-negative.
    pub fn speed(&self) -> f32 {
        (self.forward_speed * self.forward_speed + self.lateral_speed * self.lateral_speed).sqrt()
    }

    /// The direction the car is actually *travelling* (horizontal, unit),
    /// falling back to the nose when stationary. The camera follows this, not
    /// the nose, which is what makes a drift readable.
    pub fn heading_of_travel(&self) -> Vec3 {
        let v = self
            .forward()
            .mul_scalar(self.forward_speed)
            .add(self.right().mul_scalar(self.lateral_speed));
        v.normalize().unwrap_or_else(|_| self.forward())
    }

    /// How sideways the car is, `0..1` — `0` is straight, `1` is fully sideways.
    pub fn slide_ratio(&self) -> f32 {
        let speed = self.speed();
        (self.lateral_speed.abs() / speed.max(1.0e-3)).clamp(0.0, 1.0)
    }

    /// Whether every stored value is finite. The long-run stability tests assert
    /// this after tens of thousands of steps.
    pub fn is_finite(&self) -> bool {
        let vectors = [self.position, self.impact_direction];
        let scalars = [
            self.yaw,
            self.yaw_rate,
            self.forward_speed,
            self.lateral_speed,
            self.vertical_speed,
            self.steer,
            self.distance,
            self.lateral,
            self.impact_strength,
            self.impact_yaw_rate,
            self.wheel_spin,
        ];
        vectors
            .iter()
            .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite())
            && scalars.iter().all(|f| f.is_finite())
    }
}

/// The interpolatable pose presentation reads, captured before and after each
/// fixed step so a 144 Hz browser can render between two 60 Hz truths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarPose {
    pub position: Vec3,
    pub yaw: f32,
    /// Body pitch: the rotation about the chassis **right** axis, where
    /// **positive tips the nose down**. Acceleration and an uphill grade both
    /// produce negative pitch (nose up); braking and a descent produce positive.
    pub pitch: f32,
    /// Body roll: the rotation about the chassis **forward** axis, where
    /// **positive raises the right-hand side**. A right-hand turn and a road
    /// banked to the right both produce positive roll.
    pub roll: f32,
    /// Wheel rotation (radians).
    pub wheel_spin: f32,
    /// Front wheel steering angle (radians).
    pub steer_angle: f32,
}

impl CarPose {
    /// Linearly interpolate between two poses. Angles take the short way round,
    /// so a pose either side of the `±π` wrap does not spin the car.
    pub fn lerp(a: CarPose, b: CarPose, t: f32) -> CarPose {
        let t = t.clamp(0.0, 1.0);
        let angle = |x: f32, y: f32| x + crate::track::shortest_angle(y - x) * t;
        CarPose {
            position: a.position.add(b.position.subtract(a.position).mul_scalar(t)),
            yaw: angle(a.yaw, b.yaw),
            pitch: a.pitch + (b.pitch - a.pitch) * t,
            roll: a.roll + (b.roll - a.roll) * t,
            wheel_spin: angle(a.wheel_spin, b.wheel_spin),
            steer_angle: a.steer_angle + (b.steer_angle - a.steer_angle) * t,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parked_car_is_stationary_and_grounded() {
        let car = CarState::parked(Vec3::new(1.0, 2.0, 3.0), 0.5);
        assert_eq!(car.position, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(car.yaw, 0.5);
        assert_eq!(car.speed(), 0.0);
        assert!(car.grounded);
        assert!(!car.drifting);
        assert!(car.is_finite());
    }

    #[test]
    fn the_chassis_basis_is_right_handed_and_unit() {
        for yaw in [0.0f32, 0.7, 2.5, -1.9] {
            let car = CarState::parked(Vec3::ZERO, yaw);
            let f = car.forward();
            let r = car.right();
            assert!((f.length() - 1.0).abs() < 1.0e-5);
            assert!((r.length() - 1.0).abs() < 1.0e-5);
            assert!(f.dot(r).abs() < 1.0e-5, "forward ⟂ right");
            // `cross(up, forward)` is the right vector — the same handedness the
            // track frames use, so chassis and road agree on which way is right.
            let expected = Vec3::UNIT_Y.cross(f);
            assert!(expected.subtract(r).length() < 1.0e-5);
        }
    }

    #[test]
    fn at_zero_yaw_the_car_faces_positive_z() {
        let car = CarState::parked(Vec3::ZERO, 0.0);
        assert!((car.forward().z - 1.0).abs() < 1.0e-6);
        assert!((car.right().x - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn velocity_rebuilds_from_the_chassis_components() {
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        car.forward_speed = 30.0;
        car.lateral_speed = 4.0;
        car.vertical_speed = -2.0;
        let v = car.velocity();
        assert!((v.z - 30.0).abs() < 1.0e-4);
        assert!((v.x - 4.0).abs() < 1.0e-4);
        assert!((v.y + 2.0).abs() < 1.0e-4);
        // Ground speed ignores the vertical component.
        assert!((car.speed() - (30.0f32 * 30.0 + 16.0).sqrt()).abs() < 1.0e-4);
    }

    #[test]
    fn the_travel_heading_leads_the_nose_during_a_slide() {
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        car.forward_speed = 20.0;
        car.lateral_speed = 20.0;
        let travel = car.heading_of_travel();
        assert!(travel.x > 0.6, "travel is pushed toward the slide");
        assert!(travel.z > 0.6);
        assert!((car.slide_ratio() - 0.7071).abs() < 0.01, "45 degrees sideways");
    }

    #[test]
    fn a_stationary_car_travels_along_its_nose() {
        let car = CarState::parked(Vec3::ZERO, 1.0);
        assert_eq!(car.heading_of_travel(), car.forward());
        assert_eq!(car.slide_ratio(), 0.0);
    }

    #[test]
    fn finiteness_detects_a_poisoned_state() {
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        assert!(car.is_finite());
        car.forward_speed = f32::NAN;
        assert!(!car.is_finite());
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        car.position = Vec3::new(0.0, f32::INFINITY, 0.0);
        assert!(!car.is_finite());
    }

    #[test]
    fn surfaces_classify_and_index_consistently() {
        assert!(!Surface::Tarmac.is_off_road());
        assert!(Surface::Shoulder.is_off_road());
        assert!(Surface::OffRoad.is_off_road());
        let indices: Vec<usize> = [Surface::Tarmac, Surface::Shoulder, Surface::OffRoad]
            .iter()
            .map(|s| s.index())
            .collect();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn pose_interpolation_takes_the_short_way_round() {
        let pi = std::f32::consts::PI;
        let a = CarPose {
            position: Vec3::ZERO,
            yaw: pi - 0.1,
            pitch: 0.0,
            roll: 0.0,
            wheel_spin: 0.0,
            steer_angle: 0.0,
        };
        let b = CarPose {
            position: Vec3::new(10.0, 0.0, 0.0),
            yaw: -pi + 0.1,
            pitch: 0.4,
            roll: -0.2,
            wheel_spin: 6.0,
            steer_angle: 0.3,
        };
        let mid = CarPose::lerp(a, b, 0.5);
        assert!((mid.position.x - 5.0).abs() < 1.0e-5);
        assert!((mid.pitch - 0.2).abs() < 1.0e-5);
        assert!((mid.roll + 0.1).abs() < 1.0e-5);
        assert!((mid.steer_angle - 0.15).abs() < 1.0e-5);
        // Straight lerp would give 0; the short way gives ±π.
        assert!(mid.yaw.abs() > 3.0, "yaw wrapped the short way: {}", mid.yaw);

        assert_eq!(CarPose::lerp(a, b, 0.0), a);
        assert_eq!(CarPose::lerp(a, b, -1.0), a);
        assert_eq!(CarPose::lerp(a, b, 2.0).position, b.position);
    }
}
