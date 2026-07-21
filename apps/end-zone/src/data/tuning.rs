//! Named tuning data: behavior (steering/contact), camera, and juice numbers.
//! Every knob the systems read lives here as plain data — nothing is buried in
//! system code.

/// Steering + contact tuning shared by the generic player systems. Units:
/// yards, seconds, radians, ticks (60 Hz), normalized strengths `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BehaviorTuning {
    /// Teammates begin to separate inside this range, yd.
    pub separation_radius: f32,
    /// Separation steering weight.
    pub separation_strength: f32,
    /// Arrival slow-down radius, yd.
    pub arrival_radius: f32,
    /// Range at which a blocker latches a defender, yd.
    pub block_engage_range: f32,
    /// How strongly a won block slows the defender, 0..=1.
    pub block_resist: f32,
    /// Range at which a tackle attempt can land, yd.
    pub tackle_range: f32,
    /// Minimum closing speed for a tackle to count, yd/s.
    pub tackle_min_closing_speed: f32,
    /// Relative speed mapped to impact strength 1.0, yd/s.
    pub tackle_full_strength_speed: f32,
    /// Deep-pursuit cushion: how many yards a rallying deep defender stays
    /// goal-side of a perceived pass landing point (over-the-top leverage
    /// instead of camping the catch).
    pub pursuit_cushion: f32,
    /// Impact strength above which the target goes airborne.
    pub airborne_threshold: f32,
    /// Diving-tackle commit window, as a multiple of `tackle_range`: a chaser
    /// leaves their feet when the carrier is beyond standing range but within
    /// `tackle_range * dive_window`.
    pub dive_window: f32,
    /// Minimum closing speed (yd/s) required to commit a dive.
    pub dive_min_closing_speed: f32,
    /// The carrier must be moving at least this fast (yd/s) to be worth diving
    /// at — you don't dive at a stationary target you can just run down.
    pub dive_carrier_min_speed: f32,
    /// Forward launch speed of a dive (yd/s).
    pub dive_launch_forward: f32,
    /// Upward launch speed of a dive (yd/s) — the arc height.
    pub dive_launch_up: f32,
    /// Impact strength recorded for a whiffed dive's own landing (drives the
    /// dust puff when a diver hits the turf without a tackle).
    pub dive_whiff_impact: f32,
    /// Upward launch speed for an airborne knockdown, yd/s.
    pub launch_up_speed: f32,
    /// Ticks a grounded fall lasts before recovery starts.
    pub fall_ticks: u32,
    /// Ticks of the recovery animation/state.
    pub recovery_ticks: u32,
    /// Ticks the snap takes to reach the quarterback.
    pub snap_ticks: u32,
    /// Horizontal pass speed, yd/s (flight time = distance / this).
    pub pass_speed: f32,
    /// Minimum pass flight time, ticks.
    pub min_flight_ticks: u32,
    /// Ticks of quarterback throw wind-up before release.
    pub throw_windup_ticks: u32,
    /// Half-angle of the quarterback's throwing cone, radians. A receiver must
    /// be within this much of the quarterback's facing to be throwable — this
    /// is what makes the stick aim the pass.
    pub throw_cone_half_angle: f32,
    /// How far off straight-downfield a STEERED quarterback may turn, radians.
    /// His facing is clamped to this forward arc, so pushing the stick sideways
    /// strafes him instead of spinning him: he keeps his eyes downfield and can
    /// never end up facing his own end zone. It also bounds how far he can swing
    /// the throwing cone, which is how the stick aims the pass.
    pub qb_aim_max_yaw: f32,
    /// Nearest a receiver may be and still be throwable, yd (a man standing on
    /// top of the quarterback is not a pass).
    pub throw_min_range: f32,
    /// Furthest a receiver may be and still be throwable, yd.
    pub throw_max_range: f32,
    /// Gravity, yd/s² (9.8 m/s² in yards).
    pub gravity: f32,
    /// Boundary clamp margin, yd.
    pub bounds_margin: f32,
}

impl Default for BehaviorTuning {
    fn default() -> Self {
        BehaviorTuning {
            separation_radius: 1.6,
            separation_strength: 6.0,
            arrival_radius: 2.2,
            block_engage_range: 1.4,
            block_resist: 0.8,
            tackle_range: 1.3,
            tackle_min_closing_speed: 2.0,
            tackle_full_strength_speed: 14.0,
            pursuit_cushion: 6.0,
            airborne_threshold: 0.55,
            dive_window: 2.4,
            dive_min_closing_speed: 6.0,
            dive_carrier_min_speed: 4.0,
            dive_launch_forward: 9.5,
            dive_launch_up: 3.2,
            dive_whiff_impact: 0.25,
            launch_up_speed: 4.6,
            fall_ticks: 26,
            recovery_ticks: 40,
            snap_ticks: 7,
            pass_speed: 22.0,
            min_flight_ticks: 24,
            throw_windup_ticks: 12,
            throw_cone_half_angle: 0.95,
            qb_aim_max_yaw: 1.05,
            throw_min_range: 2.0,
            throw_max_range: 34.0,
            gravity: 10.72,
            bounds_margin: 0.6,
        }
    }
}

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
            formation_distance: 17.0,
            formation_height: 9.0,
            catch_blend_ticks: 18,
        }
    }
}

/// Presentation-effect tuning: bounded lifetimes and clamped amplitudes for
/// every juice effect. All effects decay to exactly zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JuiceTuning {
    /// Max simultaneous effects (fixed pool).
    pub max_effects: usize,
    /// Dust burst: particles, life, max radius, max amplitude.
    pub dust_particles: usize,
    pub dust_life_ticks: u32,
    pub dust_radius: f32,
    /// Impact ring life + max radius.
    pub ring_life_ticks: u32,
    pub ring_radius: f32,
    /// Speed streaks: count + life.
    pub streak_count: usize,
    pub streak_life_ticks: u32,
    /// Ball trail: sample count + spacing ticks.
    pub trail_points: usize,
    pub trail_spacing_ticks: u32,
    /// Catch flash life.
    pub flash_life_ticks: u32,
    /// Field wobble: max amplitude (yd) + life.
    pub field_wobble_amplitude: f32,
    pub field_wobble_life_ticks: u32,
    /// Player squash: max pose compression `0..=1` + life.
    pub squash_amplitude: f32,
    pub squash_life_ticks: u32,
    /// Multiplier on flash effects (catch flash, throw pulse) — the flash
    /// accessibility control; `0` spawns no flash effects at all.
    pub flash_scale: f32,
}

impl Default for JuiceTuning {
    fn default() -> Self {
        JuiceTuning {
            max_effects: 16,
            dust_particles: 10,
            dust_life_ticks: 34,
            dust_radius: 1.7,
            ring_life_ticks: 26,
            ring_radius: 2.2,
            streak_count: 6,
            streak_life_ticks: 16,
            trail_points: 14,
            trail_spacing_ticks: 2,
            flash_life_ticks: 18,
            field_wobble_amplitude: 0.16,
            field_wobble_life_ticks: 30,
            squash_amplitude: 0.35,
            squash_life_ticks: 18,
            flash_scale: 1.0,
        }
    }
}
