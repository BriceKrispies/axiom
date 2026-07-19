# SwingOutcome — the authoritative, deterministic hit prediction. evaluate() is
# called ONCE the instant a swing commits and forward-simulates the exact per-tick
# sequence the session runs (Swing.step for the bat, the same pitch integration, the
# real swept contact), then projects the post-contact flight with the REAL BallFlight
# physics so home-run classification can never diverge. Both the real launched ball
# and the home-run cinematic consume this one record.
extends RefCounted

const SwingOutcome = preload("res://scripts/swing_outcome.gd")
const HRC = preload("res://scripts/constants.gd")
const Swing = preload("res://scripts/swing.gd")
const Contact = preload("res://scripts/contact.gd")
const BallFlight = preload("res://scripts/ball.gd")

var contact_occurs: bool
var contact_tick: int
var contact_point: Vector3
var contact_normal: Vector3
var bat_velocity_at_contact: Vector3
var pitch_velocity_at_contact: Vector3
var exit_velocity: Vector3
var exit_speed: float
var spray: float
var contact_quality: float
var launch_direction: Vector3
var launch_angle: float
var projected_apex: Vector3
var projected_landing: Vector3
var projected_distance: float
var is_fair: bool
var is_home_run: bool
var home_run_reason: String

static func _hyp(a: float, b: float) -> float:
	return sqrt(a * a + b * b)

static func _normalize(v: Vector3) -> Vector3:
	var l := v.length()
	return v / l if l > 1e-9 else Vector3.ZERO

static func no_contact() -> SwingOutcome:
	var o := SwingOutcome.new()
	o.contact_occurs = false
	o.contact_tick = -1
	o.contact_point = Vector3.ZERO
	o.contact_normal = Vector3.ZERO
	o.bat_velocity_at_contact = Vector3.ZERO
	o.pitch_velocity_at_contact = Vector3.ZERO
	o.exit_velocity = Vector3.ZERO
	o.exit_speed = 0.0
	o.spray = 0.0
	o.contact_quality = 0.0
	o.launch_direction = Vector3.ZERO
	o.launch_angle = 0.0
	o.projected_apex = Vector3.ZERO
	o.projected_landing = Vector3.ZERO
	o.projected_distance = 0.0
	o.is_fair = false
	o.is_home_run = false
	o.home_run_reason = "no-contact"
	return o

static func _fill_projection(out: SwingOutcome, contact: Contact, tuning: Dictionary) -> void:
	var flight := BallFlight.new_flight(contact.point, contact.exit_vel, contact.exit_speed, contact.loft, contact.spray)
	var apex := contact.point
	var reached_wall_line := false
	var done := false
	for stp in range(tuning.maxPredictionSteps):
		if done:
			break
		for sub in range(tuning.trajectoryPredictionStepTicks):
			if done:
				break
			done = flight.step()
			if flight.pos.y > apex.y:
				apex = flight.pos
			reached_wall_line = reached_wall_line or BallFlight.beyond_wall(flight.pos.x, flight.pos.z)
	var distance: float
	if flight.homer:
		distance = _hyp(flight.pos.x, flight.pos.z)
	elif flight.first_land_dist > 0.0:
		distance = maxf(flight.first_land_dist, _hyp(flight.pos.x, flight.pos.z))
	else:
		distance = _hyp(flight.pos.x, flight.pos.z)
	out.projected_apex = apex
	out.projected_landing = flight.pos
	out.projected_distance = distance
	out.is_fair = not flight.foul
	out.is_home_run = flight.homer
	if flight.homer:
		out.home_run_reason = "clears-wall-fair"
	elif not out.is_fair:
		out.home_run_reason = "not-fair"
	elif reached_wall_line:
		out.home_run_reason = "below-wall-height"
	else:
		out.home_run_reason = "does-not-clear-wall"

static func evaluate(swing_state: Swing, pitch_pos: Vector3, pitch_vel: Vector3, gravity_per_tick: float, batter_x: float, tuning: Dictionary) -> SwingOutcome:
	var swing := swing_state
	var prev_theta := swing_state.theta
	var ball_pos := pitch_pos
	var ball_vel := pitch_vel

	for tick in range(tuning.swingContactSearchMaxTicks):
		var prev_ball := ball_pos
		ball_vel = Vector3(ball_vel.x, ball_vel.y - gravity_per_tick, ball_vel.z)
		ball_pos = ball_pos + ball_vel

		if swing.state == "swing":
			var contact := Swing.swept_contact(prev_theta, swing.theta, swing.omega, batter_x, prev_ball, ball_pos, ball_vel.z)
			if contact != null:
				var o := SwingOutcome.new()
				o.contact_occurs = true
				o.contact_tick = tick
				o.contact_point = contact.point
				o.contact_normal = _normalize(Swing.bat_tangent(swing.theta))
				o.bat_velocity_at_contact = Swing.bat_tangent(swing.theta) * (swing.omega * contact.r * HRC.FIXED_HZ)
				o.pitch_velocity_at_contact = ball_vel
				o.exit_velocity = contact.exit_vel
				o.exit_speed = contact.exit_speed
				o.spray = contact.spray
				o.contact_quality = contact.quality
				o.launch_direction = _normalize(contact.exit_vel)
				o.launch_angle = contact.loft
				_fill_projection(o, contact, tuning)
				return o
		if ball_pos.z <= HRC.CATCHER_Z:
			return no_contact()

		prev_theta = swing.theta
		swing = swing.step(false)
	return no_contact()
