# HomeRunSession — the framework-free heart of the game. It owns one explicit
# mutable state and advances it exactly one deterministic tick per advance(), folding
# the typed sim classes (Swing, PitchSpec, BallFlight, Fielder, SwingOutcome,
# Cinematic) into the round state machine
# (ready -> windup -> pitch -> flight -> result -> ... -> over). It touches no engine
# API; view() hands the presentation a read-only SceneView. All variation derives
# from the constructor seed via the deterministic hash.
extends RefCounted

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")
const HRCine = preload("res://scripts/cinematic_constants.gd")
const Swing = preload("res://scripts/swing.gd")
const PitchSpec = preload("res://scripts/pitch.gd")
const BallFlight = preload("res://scripts/ball.gd")
const Fielder = preload("res://scripts/fielders.gd")
const SwingOutcome = preload("res://scripts/swing_outcome.gd")
const Cinematic = preload("res://scripts/cinematic.gd")
const CameraPose = preload("res://scripts/cinematic_camera.gd")
const SceneView = preload("res://scripts/scene_view.gd")
const Feedback = preload("res://scripts/feedback.gd")
const PitchResult = preload("res://scripts/pitch_result.gd")

const TRAIL_MAX := 14
const HIDDEN_BALL := Vector3(0, -100, 0)

const OUTCOME_TEXT := {
	"ball": "BALL", "clean": "CLEAN HIT", "foul": "FOUL", "grounder": "GROUNDER",
	"homer": "HOME RUN!", "miss": "MISS", "popup": "POP UP", "weak": "WEAK HIT",
}

var _seed: int
var _phase := "ready"
var _tick := 0
var _phase_ticks := 0
var _pitch_index := 0
var _results: Array = []          # PitchResult

var _score := 0
var _homers := 0
var _streak := 0
var _best_dist := 0

var _batter_x := HRC.BATTER_START_X
var _swing: Swing
var _swung_this_pitch := false

var _spec: PitchSpec = null
var _gap := 0
var _ball_pos := HIDDEN_BALL
var _ball_vel := Vector3.ZERO
var _pitch_gravity := 0.0
var _ball_live := false
var _has_plate_cross := false
var _plate_cross := Vector2.ZERO   # x = plate-cross X, y = plate-cross height

var _flight: BallFlight = null
var _trail: Array = []             # Vector3
var _fielders: Array               # Fielder

var _hit_stop := 0
var _muzzle_flash := 0.0
var _punch_ticks := 0
var _shake_ticks := 0
var _shake_total := 1
var _shake_mag := 0.0
var _follow_blend := 0.0
var _impact_flash := 0.0
var _result_duration := HRC.RESULT_TICKS

var _events: Array = []            # Feedback (rolling, capped)
var _tick_events: Array = []       # Feedback (this tick only)
var _last_mph := 0
var _last_pitch_name := ""

var _swing_outcome: SwingOutcome = null
var _swing_commit_sim_tick := 0
var _sim_tick := 0
var _sim_accum := 1.0
var _cinematic: Cinematic
var _cinematic_cam_pos := Vector3.ZERO
var _cinematic_cam_target := Vector3.ZERO
var _ground_camera_zoom := 0.0

func _init(seed: int = 1) -> void:
	_seed = seed
	_swing = Swing.ready_swing()
	_cinematic = Cinematic.new_cinematic()
	_fielders = Fielder.new_roster(_seed)

static func _hyp(a: float, b: float) -> float:
	return sqrt(a * a + b * b)

# ── advance one fixed tick ──
func advance(move_x: float, swing_pressed: bool, start_pressed: bool) -> void:
	_tick += 1
	_tick_events = []

	if _cinematic.phase != "none":
		_cinematic = _cinematic.step(HRCine.TUNING)
		_update_cinematic_camera()

	if _hit_stop > 0:
		_hit_stop -= 1
		return
	_phase_ticks += 1

	_sim_accum += _cinematic.time_scale
	if _sim_accum < 1.0:
		return
	_sim_accum -= 1.0
	_sim_tick += 1
	_decay_feel()

	if (_phase == "ready" or _phase == "windup" or _phase == "pitch") and _swing.state != "swing":
		_batter_x = clampf(_batter_x + move_x * HRC.BATTER_STEP_SPEED, HRC.BATTER_MIN_X, HRC.BATTER_MAX_X)

	var prev_swing_state := _swing.state
	if _phase != "over":
		_swing = _swing.step(swing_pressed)
		if _swing.state == "swing" and (_phase == "pitch" or _phase == "windup"):
			_swung_this_pitch = true
	if prev_swing_state == "ready" and _swing.state == "swing":
		_commit_swing()

	match _phase:
		"ready":
			Fielder.step(_fielders, _seed, _sim_tick, false, Vector2.ZERO)
			if start_pressed or swing_pressed:
				_begin_pitch()
		"windup":
			Fielder.step(_fielders, _seed, _sim_tick, false, Vector2.ZERO)
			if _phase_ticks >= _gap + HRC.WINDUP_TICKS:
				_release_pitch()
		"pitch":
			Fielder.step(_fielders, _seed, _sim_tick, false, Vector2.ZERO)
			_step_pitch()
		"flight":
			_step_flight()
		"result":
			Fielder.step(_fielders, _seed, _sim_tick, false, Vector2.ZERO)
			if _cinematic.phase == "landing" and _cinematic.phase_ticks >= HRCine.TUNING.landingCameraDurationTicks:
				_cinematic = _cinematic.enter_phase("celebration")
			if _phase_ticks >= _result_duration:
				_next_pitch_or_over()
		"over":
			Fielder.step(_fielders, _seed, _sim_tick, false, Vector2.ZERO)
			if start_pressed:
				reset()

func _commit_swing() -> void:
	_swing_outcome = SwingOutcome.evaluate(_swing, _ball_pos, _ball_vel, _pitch_gravity, _batter_x, HRCine.TUNING)
	_swing_commit_sim_tick = _sim_tick
	if _swing_outcome.is_home_run:
		_cinematic = Cinematic.new_cinematic().enter_phase("anticipation")
		_emit(Feedback.make("cinematicAnticipation", "", false))

func reset() -> void:
	_phase = "ready"
	_phase_ticks = 0
	_pitch_index = 0
	_results = []
	_score = 0
	_homers = 0
	_streak = 0
	_best_dist = 0
	_batter_x = HRC.BATTER_START_X
	_swing = Swing.ready_swing()
	_swung_this_pitch = false
	_spec = null
	_gap = 0
	_ball_pos = HIDDEN_BALL
	_ball_vel = Vector3.ZERO
	_pitch_gravity = 0.0
	_ball_live = false
	_has_plate_cross = false
	_plate_cross = Vector2.ZERO
	_flight = null
	_trail = []
	_fielders = Fielder.new_roster(_seed)
	_hit_stop = 0
	_muzzle_flash = 0.0
	_punch_ticks = 0
	_shake_ticks = 0
	_shake_mag = 0.0
	_follow_blend = 0.0
	_impact_flash = 0.0
	_events = []
	_last_mph = 0
	_last_pitch_name = ""
	_swing_outcome = null
	_swing_commit_sim_tick = 0
	_sim_tick = 0
	_sim_accum = 1.0
	_cinematic = Cinematic.new_cinematic()
	_cinematic_cam_pos = Vector3.ZERO
	_cinematic_cam_target = Vector3.ZERO
	_ground_camera_zoom = 0.0

# ── pitch lifecycle ──
func _begin_pitch() -> void:
	_phase = "windup"
	_phase_ticks = 0
	_swung_this_pitch = false
	_spec = PitchSpec.select_pitch(_seed, _pitch_index)
	_gap = PitchSpec.pitch_gap_ticks(_seed, _pitch_index)
	_ball_pos = HIDDEN_BALL
	_ball_live = false
	_has_plate_cross = false
	_trail = []
	_swing_outcome = null
	_cinematic = Cinematic.new_cinematic()
	_sim_accum = 1.0
	_ground_camera_zoom = 0.0
	_emit(Feedback.make("windup", "", false))

func _release_pitch() -> void:
	_ball_pos = HRC.PITCH_RELEASE
	_ball_vel = _spec.solve_velocity()
	_pitch_gravity = _spec.gravity_per_tick()
	_ball_live = true
	_last_mph = _spec.mph
	_last_pitch_name = _spec.name
	_muzzle_flash = HRC.FLASH_TICKS
	_punch_ticks = HRC.CAMERA_PUNCH_TICKS
	_phase = "pitch"
	_phase_ticks = 0
	_emit(Feedback.make("release", "%d MPH" % _spec.mph, false))

func _step_pitch() -> void:
	var prev_ball := _ball_pos
	_ball_vel = Vector3(_ball_vel.x, _ball_vel.y - _pitch_gravity, _ball_vel.z)
	_ball_pos = _ball_pos + _ball_vel

	if not _has_plate_cross and prev_ball.z > 0.0 and _ball_pos.z <= 0.0:
		var f := prev_ball.z / (prev_ball.z - _ball_pos.z)
		_plate_cross = Vector2(prev_ball.x + (_ball_pos.x - prev_ball.x) * f, prev_ball.y + (_ball_pos.y - prev_ball.y) * f)
		_has_plate_cross = true

	if _swing.state == "swing" and _swing_outcome != null and _swing_outcome.contact_occurs and _sim_tick - _swing_commit_sim_tick == _swing_outcome.contact_tick:
		_begin_flight(_swing_outcome)
		return

	if _ball_pos.z <= HRC.CATCHER_Z:
		_ball_live = false
		_ball_pos = HIDDEN_BALL
		var took := not _swung_this_pitch
		var was_ball := took and (not _has_plate_cross or not PitchSpec.is_strike(_plate_cross.x, _plate_cross.y))
		if was_ball:
			_resolve("BALL", "ball", 0.0, false)
			return
		_resolve("STRIKE" if took else "MISS", "miss", 0.0, false)

func _begin_flight(outcome: SwingOutcome) -> void:
	_flight = BallFlight.new_flight(outcome.contact_point, outcome.exit_velocity, outcome.exit_speed, outcome.launch_angle, outcome.spray)
	_ball_pos = outcome.contact_point
	_ball_live = true
	_phase = "flight"
	_phase_ticks = 0
	_trail = []
	var q := outcome.contact_quality
	_emit(Feedback.make("contact", "", q > 0.8 or outcome.is_home_run))
	_impact_flash = roundi(6.0 + 10.0 * q)
	if outcome.is_home_run:
		_hit_stop = HRCine.TUNING.impactHoldDurationTicks
		_shake(HRCine.TUNING.cameraShakeStrength, HRCine.TUNING.cameraShakeDurationTicks)
		_cinematic = _cinematic.enter_phase("contact")
	elif q >= HRC.HIT_STOP_QUALITY:
		_hit_stop = HRC.HIT_STOP_BASE_TICKS + roundi(HRC.HIT_STOP_MAX_EXTRA * HRMath.clamp01((q - HRC.HIT_STOP_QUALITY) / (1.0 - HRC.HIT_STOP_QUALITY)))
		_shake(HRC.SHAKE_CONTACT * (0.5 + q), HRC.SHAKE_TICKS)

func _step_flight() -> void:
	var b := _flight
	var was_homer := b.homer
	var landing := Fielder.project_landing(b.pos, b.vel, HRC.GRAVITY / (HRC.FIXED_HZ * HRC.FIXED_HZ))
	Fielder.step(_fielders, _seed, _sim_tick, not b.foul, landing)

	var done := b.step()
	_ball_pos = b.pos
	_trail.append(b.pos)
	if _trail.size() > TRAIL_MAX:
		_trail.pop_front()

	if not was_homer and b.homer:
		_shake(HRC.SHAKE_HOMER, HRC.SHAKE_TICKS_HOMER)

	var long_hit := not b.foul and b.exit_speed > 20.0 and b.pos.z > 12.0
	_follow_blend = HRMath.clamp01(_follow_blend + (HRC.CAMERA_FOLLOW_RATE if long_hit else -HRC.CAMERA_FOLLOW_RATE))
	if _follow_blend > HRC.CAMERA_FOLLOW_MAX:
		_follow_blend = HRC.CAMERA_FOLLOW_MAX

	if _cinematic.phase == "contact" and _cinematic.phase_ticks >= HRCine.TUNING.contactSlowMotionDurationTicks:
		_cinematic = _cinematic.enter_phase("ballFollow")
		_emit(Feedback.make("crowdErupt", "", false))

	if not b.homer:
		var who := Fielder.catching_fielder(_fielders, b.pos)
		if who >= 0:
			var caught_air := b.bounces == 0 and not b.foul
			_resolve("CAUGHT!" if caught_air else "FIELDED", b.classify_caught(), _hyp(b.pos.x, b.pos.z), true)
			return

	if done:
		var outcome := b.classify_flight()
		var dist: float
		if outcome == "homer":
			dist = _hyp(b.pos.x, b.pos.z)
		elif b.first_land_dist > 0.0:
			dist = maxf(b.first_land_dist, _hyp(b.pos.x, b.pos.z))
		else:
			dist = _hyp(b.pos.x, b.pos.z)
		if _cinematic.phase != "none":
			_cinematic = _cinematic.enter_phase("landing")
		_resolve(OUTCOME_TEXT[outcome], outcome, dist, false)

func _resolve(text: String, outcome: String, distance: float, caught: bool) -> void:
	var dist := roundi(distance)
	_streak = _streak + 1 if outcome == "homer" else 0
	var points := BallFlight.score_for(outcome, float(dist), _streak)
	_score += points
	if outcome == "homer":
		_homers += 1
	if outcome != "miss" and outcome != "ball" and outcome != "foul" and not caught:
		_best_dist = max(_best_dist, dist)
	_results.append(PitchResult.make(outcome, points, dist, _last_mph, caught))

	var suffix := " +%d" % points if points > 0 else ""
	var streak_tag := " x%d" % min(_streak, HRC.STREAK_MULT_CAP) if outcome == "homer" and _streak > 1 else ""
	_emit(Feedback.make(outcome, "%s%s%s" % [text, suffix, streak_tag], outcome == "homer"))

	_flight = null
	_result_duration = HRC.HOMER_RESULT_TICKS if outcome == "homer" else HRC.RESULT_TICKS
	_phase = "result"
	_phase_ticks = 0

func _next_pitch_or_over() -> void:
	_pitch_index += 1
	_ball_pos = HIDDEN_BALL
	_ball_live = false
	_trail = []
	if _pitch_index >= HRC.PITCHES_PER_ROUND:
		_phase = "over"
		_phase_ticks = 0
		return
	_begin_pitch()

# ── feel + camera ──
func _shake(mag: float, ticks: int) -> void:
	_shake_mag = maxf(_shake_mag, mag)
	_shake_ticks = max(_shake_ticks, ticks)
	_shake_total = max(_shake_ticks, 1)

func _decay_feel() -> void:
	_muzzle_flash = maxf(0.0, _muzzle_flash - 1.0)
	_punch_ticks = max(0, _punch_ticks - 1)
	_impact_flash = maxf(0.0, _impact_flash - 1.0)
	if _shake_ticks > 0:
		_shake_ticks -= 1
		if _shake_ticks == 0:
			_shake_mag = 0.0
	if _phase != "flight":
		_follow_blend = HRMath.clamp01(_follow_blend - HRC.CAMERA_FOLLOW_RATE)

func _shake_offset() -> Vector3:
	if _shake_ticks <= 0:
		return Vector3.ZERO
	var m := _shake_mag * (float(_shake_ticks) / float(_shake_total))
	return Vector3(sin(_sim_tick * 2.9) * m, cos(_sim_tick * 2.3) * m * 0.6, 0.0)

func _windup_progress() -> float:
	if _phase != "windup" or _phase_ticks < _gap:
		return 0.0
	var w := HRMath.clamp01(float(_phase_ticks - _gap) / float(HRC.WINDUP_TICKS))
	return w * w * (3.0 - 2.0 * w)

func _update_cinematic_camera() -> void:
	var tuning := HRCine.TUNING
	if _cinematic.phase == "landing" or _cinematic.phase == "celebration":
		return
	if _cinematic.phase == "ballFollow":
		if BallFlight.beyond_wall(_ball_pos.x, _ball_pos.z):
			return
		var pose := CameraPose.ground_tracking_pose(_batter_x, HRC.BATTER_Z, _ball_pos, tuning)
		_cinematic_cam_pos = pose.position
		_cinematic_cam_target = pose.target
		var zoom_target := CameraPose.ground_tracking_zoom_target(_ball_vel, tuning)
		var zoom_rate: float = 1.0 / tuning.cinematicCameraBlendDurationTicks if tuning.cinematicCameraBlendDurationTicks > 0 else 1.0
		if zoom_target > _ground_camera_zoom:
			_ground_camera_zoom = min(zoom_target, _ground_camera_zoom + zoom_rate)
		else:
			_ground_camera_zoom = max(zoom_target, _ground_camera_zoom - zoom_rate)
		return
	var pose2 := CameraPose.contact_pose(_batter_x, HRC.BATTER_Z, tuning)
	_cinematic_cam_pos = pose2.position
	_cinematic_cam_target = pose2.target
	_ground_camera_zoom = 0.0

# ── read-only snapshot ──
func view() -> SceneView:
	var windup := _windup_progress()
	var dolly := windup * HRC.CAMERA_WINDUP_DOLLY + (float(_punch_ticks) / float(HRC.CAMERA_PUNCH_TICKS)) * HRC.CAMERA_RELEASE_PUNCH
	var shake := _shake_offset()
	var gameplay_cam_pos := HRC.CAMERA_POS + Vector3(0, 0, dolly) + shake
	var follow_target := HRC.CAMERA_TARGET.lerp(_ball_pos, _follow_blend) if _follow_blend > 0.0 and _ball_live else HRC.CAMERA_TARGET
	var gameplay_cam_target := follow_target + shake * 0.5

	var cam_blend := 0.0 if _cinematic.phase == "none" else _cinematic.cam_blend
	var v := SceneView.new()
	v.phase = _phase
	v.tick = _sim_tick
	v.batter_x = _batter_x
	v.swing = _swing
	v.ball = _ball_pos
	v.ball_visible = _ball_live
	v.ball_in_play = _flight != null
	v.trail = _trail
	v.windup = windup
	v.muzzle_flash = HRMath.clamp01(_muzzle_flash / HRC.FLASH_TICKS)
	v.fielders = _fielders
	v.camera_pos = gameplay_cam_pos.lerp(_cinematic_cam_pos, cam_blend) if cam_blend > 0.0 else gameplay_cam_pos
	v.camera_target = gameplay_cam_target.lerp(_cinematic_cam_target, cam_blend) if cam_blend > 0.0 else gameplay_cam_target
	v.camera_fov_y = CameraPose.cinematic_fov_y(HRMath.clamp01(_cinematic.zoom + _ground_camera_zoom), HRCine.TUNING)
	v.impact_flash = HRMath.clamp01(_impact_flash / 12.0)
	v.hit_stop = _hit_stop > 0
	v.cinematic_phase = _cinematic.phase
	v.letterbox_progress = _cinematic.letterbox
	v.hud_visible = _cinematic.letterbox < 0.5
	return v

func _emit(event: Feedback) -> void:
	_events.append(event)
	_tick_events.append(event)
	if _events.size() > 8:
		_events.pop_front()

func tick_events() -> Array:
	return _tick_events

# HUD accessors.
func phase() -> String: return _phase
func score() -> int: return _score
func homers() -> int: return _homers
func streak() -> int: return _streak
func streak_multiplier() -> int: return clampi(_streak, 1, HRC.STREAK_MULT_CAP)
func best_distance() -> int: return _best_dist
func pitch_number() -> int: return min(_pitch_index + 1, HRC.PITCHES_PER_ROUND)
func last_mph() -> int: return _last_mph
func last_pitch_name() -> String: return _last_pitch_name
func current_swing() -> Swing: return _swing
func results() -> Array: return _results
