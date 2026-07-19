# Swing — the always-armed bat: an explicit state machine
# (ready -> swing -> follow -> rewind -> ready) plus the swept bat-vs-ball contact
# test. The batter starts wound at full power; one press fires the max-power swing,
# the bat overshoots into follow-through, then re-winds on its own (the cooldown).
# Pure and deterministic. `state` is one of "ready"/"swing"/"follow"/"rewind".
extends RefCounted

const Swing = preload("res://scripts/swing.gd")
const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")
const Contact = preload("res://scripts/contact.gd")

var state: String
var theta: float
var omega: float
var readiness: float
var state_ticks: int

static func create(state: String, theta: float, omega: float, readiness: float, state_ticks: int) -> Swing:
	var s := Swing.new()
	s.state = state
	s.theta = theta
	s.omega = omega
	s.readiness = readiness
	s.state_ticks = state_ticks
	return s

static func ready_swing() -> Swing:
	return create("ready", HRC.THETA_READY, 0.0, 1.0, 0)

static func _rewind_readiness(theta: float) -> float:
	return HRMath.clamp01((HRC.THETA_FOLLOW_END - theta) / (HRC.THETA_FOLLOW_END - HRC.THETA_READY))

func effective_omega() -> float:
	if state == "swing":
		var snap := HRMath.mix(HRC.SNAP_START, 1.0, float(state_ticks + 1) / float(HRC.SNAP_TICKS + 1)) if state_ticks < HRC.SNAP_TICKS else 1.0
		return omega * snap
	if state == "follow":
		return omega
	return 0.0

func step(swing_pressed: bool) -> Swing:
	var t := state_ticks + 1
	match state:
		"ready":
			if swing_pressed:
				return create("swing", theta, HRC.OMEGA_SWING, 0.0, 0)
			return create("ready", theta, omega, readiness, t)
		"swing":
			var w := create("swing", theta, omega, readiness, t - 1).effective_omega()
			var next_theta := theta + w
			if next_theta >= HRC.THETA_FOLLOW_START:
				return create("follow", next_theta, omega, 0.0, 0)
			return create("swing", next_theta, omega, readiness, t)
		"follow":
			var next_omega := omega * HRC.FOLLOW_DRAG
			var next_theta2: float = min(HRC.THETA_FOLLOW_END, theta + next_omega)
			if next_omega < HRC.FOLLOW_MIN_OMEGA or next_theta2 >= HRC.THETA_FOLLOW_END:
				return create("rewind", next_theta2, 0.0, _rewind_readiness(next_theta2), 0)
			return create("follow", next_theta2, next_omega, 0.0, t)
		"rewind":
			var next_theta3 := theta + (HRC.THETA_READY - theta) * HRC.REWIND_RATE
			if absf(next_theta3 - HRC.THETA_READY) < HRC.REWIND_EPSILON:
				return create("ready", HRC.THETA_READY, 0.0, 1.0, 0)
			return create("rewind", next_theta3, 0.0, _rewind_readiness(next_theta3), t)
		_:
			return self

static func bat_dir(theta: float) -> Vector3:
	return Vector3(-sin(theta), 0.0, -cos(theta))

static func bat_tangent(theta: float) -> Vector3:
	return Vector3(-cos(theta), 0.0, sin(theta))

static func bat_plane_y(theta: float) -> float:
	return HRC.BAT_PLANE_Y + clampf(HRC.BAT_UPPERCUT * (theta - HRC.THETA_SWEET), -HRC.BAT_UPPERCUT_CLAMP, HRC.BAT_UPPERCUT_CLAMP)

static func sweet_quality_at(r: float) -> float:
	var d := (r - HRC.SWEET_SPOT_R) / HRC.SWEET_SPOT_WIDTH
	return exp(-d * d)

static func timing_quality_at(theta: float) -> float:
	var d := (theta - HRC.THETA_SWEET) / HRC.TIMING_WIDTH
	return exp(-d * d)

static func resolve_contact(theta: float, omega: float, r: float, dy: float, point: Vector3, pitch_vz: float) -> Contact:
	var vert_miss := HRMath.clamp01((absf(dy) - HRC.VERT_CLEAN_DY) / (HRC.CONTACT_HEIGHT - HRC.VERT_CLEAN_DY))
	var speed_share := HRMath.mix(1.0, HRC.VERT_MISHIT_KEEP, vert_miss)
	var sweet_q := sweet_quality_at(r)
	var timing_q := timing_quality_at(theta)
	var bat_point_speed := omega * r * HRC.FIXED_HZ
	var squareness := HRMath.mix(1.0 - HRC.TIMING_SPEED_SHARE, 1.0, timing_q)
	var exit_speed := (bat_point_speed * HRC.HIT_POWER * (0.5 + 0.5 * sweet_q) + absf(pitch_vz) * HRC.FIXED_HZ * HRC.PITCH_BOUNCE_SHARE) * speed_share * squareness

	var tangent := bat_tangent(theta)
	var spray := atan2(tangent.x, tangent.z)
	var loft := clampf(HRC.LOFT_BASE + dy * HRC.LOFT_GAIN, HRC.LOFT_MIN, HRC.LOFT_MAX)
	var horizontal := (exit_speed * cos(loft)) / HRC.FIXED_HZ

	var c := Contact.new()
	c.r = r
	c.u = HRMath.clamp01((r - HRC.BAT_GRIP_R) / (HRC.BAT_TIP_R - HRC.BAT_GRIP_R))
	c.sweet_q = sweet_q
	c.timing_q = timing_q
	c.vert_q = 1.0 - vert_miss
	c.quality = 0.42 * sweet_q + 0.33 * timing_q + 0.25 * c.vert_q
	c.exit_speed = exit_speed
	c.exit_vel = Vector3(tangent.x * horizontal, (exit_speed * sin(loft)) / HRC.FIXED_HZ, tangent.z * horizontal)
	c.spray = spray
	c.loft = loft
	c.point = point
	return c

# The swept bat-vs-ball test for one tick: both the bat sweep and the ball segment
# are subsampled together so neither can tunnel. Returns the first touching Contact,
# or null.
static func swept_contact(prev_theta: float, theta: float, omega: float, batter_x: float, prev_ball: Vector3, ball: Vector3, pitch_vz: float) -> Contact:
	for k in range(1, HRC.CONTACT_SUBSTEPS + 1):
		var f := float(k) / float(HRC.CONTACT_SUBSTEPS)
		var th := HRMath.mix(prev_theta, theta, f)
		var bx := HRMath.mix(prev_ball.x, ball.x, f)
		var by := HRMath.mix(prev_ball.y, ball.y, f)
		var bz := HRMath.mix(prev_ball.z, ball.z, f)
		var d := bat_dir(th)
		var rel_x := bx - batter_x
		var rel_z := bz - HRC.BATTER_Z
		var r := rel_x * d.x + rel_z * d.z
		if r < HRC.BAT_GRIP_R or r > HRC.BAT_TIP_R:
			continue
		var perp_x := rel_x - r * d.x
		var perp_z := rel_z - r * d.z
		var perp := sqrt(perp_x * perp_x + perp_z * perp_z)
		var dy := by - bat_plane_y(th)
		if perp <= HRC.CONTACT_RADIUS and absf(dy) <= HRC.CONTACT_HEIGHT:
			return resolve_contact(th, omega, r, dy, Vector3(bx, by, bz), pitch_vz)
	return null
