# swing.gd — the always-armed bat: the state machine (ready -> swing -> follow ->
# rewind -> ready) plus the swept bat-vs-ball contact resolution. Ported from
# swing.ts. Records are Dictionaries; vectors are Vector3.
#
# A Swing is {state, theta, omega, readiness, stateTicks}.
# A Contact is {r, u, sweetQ, timingQ, vertQ, quality, exitVel, exitSpeed, spray,
#               loft, point}; an empty Dictionary means "no contact".

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")

static func new_swing() -> Dictionary:
	return {"omega": 0.0, "readiness": 1.0, "state": "ready", "stateTicks": 0, "theta": HRC.THETA_READY}

static func _rewind_readiness(theta: float) -> float:
	return HRMath.clamp01((HRC.THETA_FOLLOW_END - theta) / (HRC.THETA_FOLLOW_END - HRC.THETA_READY))

static func effective_omega(s: Dictionary) -> float:
	if s.state == "swing":
		var snap := HRMath.mix(HRC.SNAP_START, 1.0, float(s.stateTicks + 1) / float(HRC.SNAP_TICKS + 1)) if s.stateTicks < HRC.SNAP_TICKS else 1.0
		return s.omega * snap
	if s.state == "follow":
		return s.omega
	return 0.0

static func step_swing(s: Dictionary, swing_pressed: bool) -> Dictionary:
	var t: int = s.stateTicks + 1
	match s.state:
		"ready":
			if swing_pressed:
				return {"omega": HRC.OMEGA_SWING, "readiness": 0.0, "state": "swing", "stateTicks": 0, "theta": s.theta}
			return {"omega": s.omega, "readiness": s.readiness, "state": "ready", "stateTicks": t, "theta": s.theta}
		"swing":
			var w := effective_omega({"omega": s.omega, "readiness": s.readiness, "state": "swing", "stateTicks": t - 1, "theta": s.theta})
			var theta: float = s.theta + w
			if theta >= HRC.THETA_FOLLOW_START:
				return {"omega": s.omega, "readiness": 0.0, "state": "follow", "stateTicks": 0, "theta": theta}
			return {"omega": s.omega, "readiness": s.readiness, "state": "swing", "stateTicks": t, "theta": theta}
		"follow":
			var omega: float = s.omega * HRC.FOLLOW_DRAG
			var theta2: float = min(HRC.THETA_FOLLOW_END, s.theta + omega)
			if omega < HRC.FOLLOW_MIN_OMEGA or theta2 >= HRC.THETA_FOLLOW_END:
				return {"omega": 0.0, "readiness": _rewind_readiness(theta2), "state": "rewind", "stateTicks": 0, "theta": theta2}
			return {"omega": omega, "readiness": 0.0, "state": "follow", "stateTicks": t, "theta": theta2}
		"rewind":
			var theta3: float = s.theta + (HRC.THETA_READY - s.theta) * HRC.REWIND_RATE
			if absf(theta3 - HRC.THETA_READY) < HRC.REWIND_EPSILON:
				return {"omega": 0.0, "readiness": 1.0, "state": "ready", "stateTicks": 0, "theta": HRC.THETA_READY}
			return {"omega": 0.0, "readiness": _rewind_readiness(theta3), "state": "rewind", "stateTicks": t, "theta": theta3}
		_:
			return s

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

static func resolve_contact(theta: float, omega: float, r: float, dy: float, point: Vector3, pitch_vz: float) -> Dictionary:
	var u := HRMath.clamp01((r - HRC.BAT_GRIP_R) / (HRC.BAT_TIP_R - HRC.BAT_GRIP_R))
	var sweet_q := sweet_quality_at(r)
	var timing_q := timing_quality_at(theta)
	var vert_miss := HRMath.clamp01((absf(dy) - HRC.VERT_CLEAN_DY) / (HRC.CONTACT_HEIGHT - HRC.VERT_CLEAN_DY))
	var vert_q := 1.0 - vert_miss
	var speed_share := HRMath.mix(1.0, HRC.VERT_MISHIT_KEEP, vert_miss)

	var bat_point_speed := omega * r * HRC.FIXED_HZ
	var squareness := HRMath.mix(1.0 - HRC.TIMING_SPEED_SHARE, 1.0, timing_q)
	var exit_speed := (bat_point_speed * HRC.HIT_POWER * (0.5 + 0.5 * sweet_q) + absf(pitch_vz) * HRC.FIXED_HZ * HRC.PITCH_BOUNCE_SHARE) * speed_share * squareness

	var tangent := bat_tangent(theta)
	var spray := atan2(tangent.x, tangent.z)
	var loft := clampf(HRC.LOFT_BASE + dy * HRC.LOFT_GAIN, HRC.LOFT_MIN, HRC.LOFT_MAX)
	var horizontal := (exit_speed * cos(loft)) / HRC.FIXED_HZ
	var exit_vel := Vector3(tangent.x * horizontal, (exit_speed * sin(loft)) / HRC.FIXED_HZ, tangent.z * horizontal)

	var quality := 0.42 * sweet_q + 0.33 * timing_q + 0.25 * vert_q
	return {
		"exitSpeed": exit_speed, "exitVel": exit_vel, "loft": loft, "point": point, "quality": quality,
		"r": r, "spray": spray, "sweetQ": sweet_q, "timingQ": timing_q, "u": u, "vertQ": vert_q,
	}

# The swept bat-vs-ball test for one tick — both the bat sweep and the ball segment
# are subsampled together so neither can tunnel. Returns the resolved Contact at the
# first touching substep, or an empty Dictionary.
static func swept_contact(prev_theta: float, theta: float, omega: float, batter_x: float, prev_ball: Vector3, ball: Vector3, pitch_vz: float) -> Dictionary:
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
	return {}
