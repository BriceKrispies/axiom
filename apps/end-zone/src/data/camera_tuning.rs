//! Camera-director tuning: the numbers the camera framework reads.
//!
//! Split out of [`super::tuning`], which owns the behaviour numbers, so each
//! file stays narrowly owned. Pure relocation — the values and their defaults
//! are unchanged, and `data::CameraTuning` still names this type.

/// Camera director tuning — one named struct per the camera framework spec.
/// Distances/heights in yards, times in ticks, angles in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraTuning {
    pub follow_distance: f32,
    pub follow_height: f32,
    /// Velocity look-ahead, seconds of carrier velocity added to the target.
    pub look_ahead: f32,
    pub base_fov_degrees: f32,
    /// Critically-damped spring frequency, Hz.
    pub spring_frequency: f32,
    /// Extra damping ratio (1.0 = critical).
    pub damping_ratio: f32,
    /// Max yaw lag when the carrier turns, radians.
    pub max_yaw_lag: f32,
    /// How wide the pass-flight camera frames around the ball, yd.
    pub flight_framing_radius: f32,
    /// Impact impulse scale (world yards per unit strength).
    pub impact_impulse_scale: f32,
    /// Global multiplier on EVERY camera impulse amplitude + FOV kick — the
    /// screen-shake accessibility control (`0` = no shake, exactly).
    pub shake_scale: f32,
    /// Ticks an impact emphasis lasts before auto-return.
    pub impact_recovery_ticks: u32,
    /// Formation camera: distance behind the offense and height.
    pub formation_distance: f32,
    pub formation_height: f32,
    /// Catch-resolve blend length, ticks.
    pub catch_blend_ticks: u32,
    // --- the run game's chase shot ---
    /// How far behind the running back the eye sits, yd. Close: the whole shot
    /// exists to make the defender in front of him legible.
    pub chase_distance: f32,
    /// How high above him, yd. Above head height so the blocking ahead is not
    /// hidden behind his own shoulders, and no higher, so it still reads as
    /// being *with* him rather than watching from a blimp.
    pub chase_look_ahead: f32,
    pub chase_height: f32,
    /// How much of the runner's airborne height the eye takes on, `0..1`.
    pub chase_height_follow: f32,
    /// How far the shot may bend off straight-downfield toward his heading,
    /// radians. Small on purpose — see [`crate::camera::modes`].
    pub chase_max_yaw_lag: f32,
    /// Extra field of view the chase opens up, degrees: a wider frame catches
    /// the defender arriving from the side, which is the one you get hit by.
    pub chase_fov_widen: f32,
}

impl Default for CameraTuning {
    fn default() -> Self {
        CameraTuning {
            follow_distance: 9.0,
            follow_height: 4.4,
            look_ahead: 0.55,
            base_fov_degrees: 58.0,
            spring_frequency: 2.6,
            damping_ratio: 1.0,
            max_yaw_lag: 0.6,
            flight_framing_radius: 10.0,
            impact_impulse_scale: 0.55,
            shake_scale: 1.0,
            impact_recovery_ticks: 42,
            formation_distance: 7.0,
            formation_height: 2.8,
            catch_blend_ticks: 18,
            chase_distance: 7.2,
            chase_look_ahead: 9.0,
            chase_height: 3.3,
            chase_height_follow: 0.55,
            chase_max_yaw_lag: 0.22,
            chase_fov_widen: 6.0,
        }
    }
}
