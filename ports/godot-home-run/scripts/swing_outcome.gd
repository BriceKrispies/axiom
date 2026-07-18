# swing_outcome.gd — the authoritative, deterministic hit-outcome model, ported from
# swing-outcome.ts. evaluate_swing_outcome is called ONCE the instant a swing commits
# and forward-simulates the exact per-tick sequence the session runs (stepSwing for the
# bat, the same pitch integration, the real swept contact), then projects the post-
# contact flight with the REAL ball physics so home-run classification can never diverge.
# Returns a fully-populated SwingOutcome Dictionary.

const HRC = preload("res://scripts/constants.gd")
const HRBall = preload("res://scripts/ball.gd")
const HRSwing = preload("res://scripts/swing.gd")

static func _normalize(v: Vector3) -> Vector3:
	var len := v.length()
	return v / len if len > 1e-9 else Vector3.ZERO

static func _hyp(a: float, b: float) -> float:
	return sqrt(a * a + b * b)

static func no_contact() -> Dictionary:
	return {
		"batVelocityAtContact": Vector3.ZERO, "contactNormal": Vector3.ZERO, "contactOccurs": false,
		"contactPoint": Vector3.ZERO, "contactQuality": 0.0, "contactTick": -1, "exitSpeed": 0.0,
		"exitVelocity": Vector3.ZERO, "homeRunReason": "no-contact", "isFair": false, "isHomeRun": false,
		"launchAngle": 0.0, "launchDirection": Vector3.ZERO, "pitchVelocityAtContact": Vector3.ZERO,
		"projectedApex": Vector3.ZERO, "projectedDistance": 0.0, "projectedLanding": Vector3.ZERO, "spray": 0.0,
	}

static func _project_flight(contact: Dictionary, tuning: Dictionary) -> Dictionary:
	var flight := HRBall.new_flight(contact.point, contact.exitVel, contact.exitSpeed, contact.loft, contact.spray)
	var apex: Vector3 = contact.point
	var reached_wall_line := false
	var done := false
	for step in range(tuning.maxPredictionSteps):
		if done:
			break
		for sub in range(tuning.trajectoryPredictionStepTicks):
			if done:
				break
			done = HRBall.step_flight(flight)
			if flight.pos.y > apex.y:
				apex = flight.pos
			reached_wall_line = reached_wall_line or HRBall.beyond_wall(flight.pos.x, flight.pos.z)
	var distance: float
	if flight.homer:
		distance = _hyp(flight.pos.x, flight.pos.z)
	elif flight.firstLandDist > 0.0:
		distance = maxf(flight.firstLandDist, _hyp(flight.pos.x, flight.pos.z))
	else:
		distance = _hyp(flight.pos.x, flight.pos.z)
	var is_fair: bool = not flight.foul
	var home_run_reason: String
	if flight.homer:
		home_run_reason = "clears-wall-fair"
	elif not is_fair:
		home_run_reason = "not-fair"
	elif reached_wall_line:
		home_run_reason = "below-wall-height"
	else:
		home_run_reason = "does-not-clear-wall"
	return {"apex": apex, "distance": distance, "homeRunReason": home_run_reason, "isFair": is_fair, "isHomeRun": flight.homer, "landing": flight.pos}

static func evaluate_swing_outcome(swing_state: Dictionary, pitch_state: Dictionary, batter_state: Dictionary, tuning: Dictionary) -> Dictionary:
	var swing := swing_state
	var prev_theta: float = swing_state.theta
	var ball_pos: Vector3 = pitch_state.pos
	var ball_vel: Vector3 = pitch_state.vel

	for tick in range(tuning.swingContactSearchMaxTicks):
		var prev_ball := ball_pos
		ball_vel = Vector3(ball_vel.x, ball_vel.y - pitch_state.gravityPerTick, ball_vel.z)
		ball_pos = ball_pos + ball_vel

		if swing.state == "swing":
			var contact := HRSwing.swept_contact(prev_theta, swing.theta, swing.omega, batter_state.x, prev_ball, ball_pos, ball_vel.z)
			if not contact.is_empty():
				var projected := _project_flight(contact, tuning)
				return {
					"batVelocityAtContact": HRSwing.bat_tangent(swing.theta) * (swing.omega * contact.r * HRC.FIXED_HZ),
					"contactNormal": _normalize(HRSwing.bat_tangent(swing.theta)),
					"contactOccurs": true,
					"contactPoint": contact.point,
					"contactQuality": contact.quality,
					"contactTick": tick,
					"exitSpeed": contact.exitSpeed,
					"exitVelocity": contact.exitVel,
					"homeRunReason": projected.homeRunReason,
					"isFair": projected.isFair,
					"isHomeRun": projected.isHomeRun,
					"launchAngle": contact.loft,
					"launchDirection": _normalize(contact.exitVel),
					"pitchVelocityAtContact": ball_vel,
					"projectedApex": projected.apex,
					"projectedDistance": projected.distance,
					"projectedLanding": projected.landing,
					"spray": contact.spray,
				}
		if ball_pos.z <= HRC.CATCHER_Z:
			return no_contact()

		prev_theta = swing.theta
		swing = HRSwing.step_swing(swing, false)
	return no_contact()
