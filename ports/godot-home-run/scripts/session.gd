# session.gd — HomeRunSession, the framework-free heart of the game, ported from
# session.ts. It owns one explicit mutable state, advances it exactly one deterministic
# tick per advance(intent), and folds the pure modules (swing/pitch/fielders/ball) into
# the round state machine (ready -> windup -> pitch -> flight -> result -> ... -> over).
#
# "undefined" values from the original are represented as empty Dictionaries ({}) and
# tested with is_empty(). Records are Dictionaries; vectors are Vector3.
extends RefCounted

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")
const HRSwing = preload("res://scripts/swing.gd")
const HRPitch = preload("res://scripts/pitch.gd")
const HRFielders = preload("res://scripts/fielders.gd")
const HRBall = preload("res://scripts/ball.gd")
const HRSwingOutcome = preload("res://scripts/swing_outcome.gd")
const HRCine = preload("res://scripts/cinematic_constants.gd")
const HRCinematic = preload("res://scripts/cinematic.gd")
const HRCineCam = preload("res://scripts/cinematic_camera.gd")

const TRAIL_MAX := 14
const HIDDEN_BALL := Vector3(0, -100, 0)

const OUTCOME_TEXT := {
	"ball": "BALL", "clean": "CLEAN HIT", "foul": "FOUL", "grounder": "GROUNDER",
	"homer": "HOME RUN!", "miss": "MISS", "popup": "POP UP", "weak": "WEAK HIT",
}

var _seed: int

# Round + clock.
var _phase := "ready"
var _tick := 0
var _phase_ticks := 0
var _pitch_index := 0
var _results: Array = []

# Score.
var _score := 0
var _homers := 0
var _streak := 0
var _best_dist := 0

# Batter + bat.
var _batter_x := HRC.BATTER_START_X
var _swing := HRSwing.new_swing()
var _swung_this_pitch := false

# The live pitch (machine -> plate).
var _spec: Dictionary = {}
var _gap := 0
var _ball_pos := HIDDEN_BALL
var _ball_vel := Vector3.ZERO
var _pitch_gravity := 0.0
var _ball_live := false
var _plate_cross: Dictionary = {}

# The ball in play (post-contact).
var _flight: Dictionary = {}
var _trail: Array = []

# Fielders.
var _fielders: Array

# Feel + camera animation state.
var _hit_stop := 0
var _muzzle_flash := 0.0
var _punch_ticks := 0
var _shake_ticks := 0
var _shake_total := 1
var _shake_mag := 0.0
var _follow_blend := 0.0
var _impact_flash := 0.0
var _result_duration := HRC.RESULT_TICKS

var _events: Array = []
var _tick_events: Array = []
var _last_mph := 0
var _last_pitch_name := ""

# Home-run cinematic.
var _swing_outcome: Dictionary = {}
var _swing_commit_sim_tick := 0
var _sim_tick := 0
var _sim_accum := 1.0
var _cinematic := HRCinematic.new_cinematic()
var _cinematic_cam_pos := Vector3.ZERO
var _cinematic_cam_target := Vector3.ZERO
var _ground_camera_zoom := 0.0

func _init(seed: int = 1) -> void:
	_seed = seed
	_fielders = HRFielders.new_fielders(_seed)

static func _hyp(a: float, b: float) -> float:
	return sqrt(a * a + b * b)

# ── advance one fixed tick ──
func advance(intent: Dictionary) -> void:
	_tick += 1
	_tick_events = []

	if _cinematic.phase != "none":
		_cinematic = HRCinematic.step_cinematic(_cinematic, HRCine.TUNING)
		_update_cinematic_camera()

	if _hit_stop > 0:
		_hit_stop -= 1
		return
	_phase_ticks += 1

	_sim_accum += _cinematic.timeScale
	if _sim_accum < 1.0:
		return
	_sim_accum -= 1.0
	_sim_tick += 1
	_decay_feel()

	if (_phase == "ready" or _phase == "windup" or _phase == "pitch") and _swing.state != "swing":
		_batter_x = clampf(_batter_x + intent.moveX * HRC.BATTER_STEP_SPEED, HRC.BATTER_MIN_X, HRC.BATTER_MAX_X)

	var prev_swing_state: String = _swing.state
	if _phase != "over":
		_swing = HRSwing.step_swing(_swing, intent.swing)
		if _swing.state == "swing" and (_phase == "pitch" or _phase == "windup"):
			_swung_this_pitch = true
	if prev_swing_state == "ready" and _swing.state == "swing":
		_commit_swing()

	match _phase:
		"ready":
			HRFielders.step_fielders(_fielders, _seed, _sim_tick, {})
			if intent.start or intent.swing:
				_begin_pitch()
		"windup":
			HRFielders.step_fielders(_fielders, _seed, _sim_tick, {})
			if _phase_ticks >= _gap + HRC.WINDUP_TICKS:
				_release_pitch()
		"pitch":
			HRFielders.step_fielders(_fielders, _seed, _sim_tick, {})
			_step_pitch()
		"flight":
			_step_flight()
		"result":
			HRFielders.step_fielders(_fielders, _seed, _sim_tick, {})
			if _cinematic.phase == "landing" and _cinematic.phaseTicks >= HRCine.TUNING.landingCameraDurationTicks:
				_cinematic = HRCinematic.enter_cinematic_phase(_cinematic, "celebration")
			if _phase_ticks >= _result_duration:
				_next_pitch_or_over()
		"over":
			HRFielders.step_fielders(_fielders, _seed, _sim_tick, {})
			if intent.start:
				reset()

func _commit_swing() -> void:
	var pitch_state := {"gravityPerTick": _pitch_gravity, "pos": _ball_pos, "vel": _ball_vel}
	var batter_state := {"x": _batter_x, "z": HRC.BATTER_Z}
	_swing_outcome = HRSwingOutcome.evaluate_swing_outcome(_swing, pitch_state, batter_state, HRCine.TUNING)
	_swing_commit_sim_tick = _sim_tick
	if _swing_outcome.isHomeRun:
		_cinematic = HRCinematic.enter_cinematic_phase(HRCinematic.new_cinematic(), "anticipation")
		_emit({"big": false, "kind": "cinematicAnticipation", "text": ""})

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
	_swing = HRSwing.new_swing()
	_swung_this_pitch = false
	_spec = {}
	_gap = 0
	_ball_pos = HIDDEN_BALL
	_ball_vel = Vector3.ZERO
	_pitch_gravity = 0.0
	_ball_live = false
	_plate_cross = {}
	_flight = {}
	_trail = []
	_fielders = HRFielders.new_fielders(_seed)
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
	_swing_outcome = {}
	_swing_commit_sim_tick = 0
	_sim_tick = 0
	_sim_accum = 1.0
	_cinematic = HRCinematic.new_cinematic()
	_cinematic_cam_pos = Vector3.ZERO
	_cinematic_cam_target = Vector3.ZERO
	_ground_camera_zoom = 0.0

# ── pitch lifecycle ──
func _begin_pitch() -> void:
	_phase = "windup"
	_phase_ticks = 0
	_swung_this_pitch = false
	_spec = HRPitch.select_pitch(_seed, _pitch_index)
	_gap = HRPitch.pitch_gap_ticks(_seed, _pitch_index)
	_ball_pos = HIDDEN_BALL
	_ball_live = false
	_plate_cross = {}
	_trail = []
	_swing_outcome = {}
	_cinematic = HRCinematic.new_cinematic()
	_sim_accum = 1.0
	_ground_camera_zoom = 0.0
	_emit({"big": false, "kind": "windup", "text": ""})

func _release_pitch() -> void:
	var spec := _spec
	var solved := HRPitch.solve_pitch(spec)
	_ball_pos = HRC.PITCH_RELEASE
	_ball_vel = solved.vel
	_pitch_gravity = solved.gravityPerTick
	_ball_live = true
	_last_mph = spec.mph
	_last_pitch_name = spec.name
	_muzzle_flash = HRC.FLASH_TICKS
	_punch_ticks = HRC.CAMERA_PUNCH_TICKS
	_phase = "pitch"
	_phase_ticks = 0
	_emit({"big": false, "kind": "release", "text": "%d MPH" % spec.mph})

func _step_pitch() -> void:
	var prev_ball := _ball_pos
	_ball_vel = Vector3(_ball_vel.x, _ball_vel.y - _pitch_gravity, _ball_vel.z)
	_ball_pos = _ball_pos + _ball_vel

	if _plate_cross.is_empty() and prev_ball.z > 0.0 and _ball_pos.z <= 0.0:
		var f := prev_ball.z / (prev_ball.z - _ball_pos.z)
		_plate_cross = {
			"x": prev_ball.x + (_ball_pos.x - prev_ball.x) * f,
			"y": prev_ball.y + (_ball_pos.y - prev_ball.y) * f,
		}

	var outcome := _swing_outcome
	if _swing.state == "swing" and not outcome.is_empty() and outcome.contactOccurs and _sim_tick - _swing_commit_sim_tick == outcome.contactTick:
		_begin_flight(outcome)
		return

	if _ball_pos.z <= HRC.CATCHER_Z:
		_ball_live = false
		_ball_pos = HIDDEN_BALL
		var cross := _plate_cross
		var took := not _swung_this_pitch
		var was_ball := took and (cross.is_empty() or not HRPitch.is_strike(cross.x, cross.y))
		if was_ball:
			_resolve("BALL", "ball", 0.0, false)
			return
		_resolve("STRIKE" if took else "MISS", "miss", 0.0, false)

func _begin_flight(outcome: Dictionary) -> void:
	_flight = HRBall.new_flight(outcome.contactPoint, outcome.exitVelocity, outcome.exitSpeed, outcome.launchAngle, outcome.spray)
	_ball_pos = outcome.contactPoint
	_ball_live = true
	_phase = "flight"
	_phase_ticks = 0
	_trail = []
	var quality: float = outcome.contactQuality
	_emit({"big": quality > 0.8 or outcome.isHomeRun, "kind": "contact", "text": ""})
	_impact_flash = roundi(6.0 + 10.0 * quality)
	if outcome.isHomeRun:
		_hit_stop = HRCine.TUNING.impactHoldDurationTicks
		_shake(HRCine.TUNING.cameraShakeStrength, HRCine.TUNING.cameraShakeDurationTicks)
		_cinematic = HRCinematic.enter_cinematic_phase(_cinematic, "contact")
	elif quality >= HRC.HIT_STOP_QUALITY:
		_hit_stop = HRC.HIT_STOP_BASE_TICKS + roundi(HRC.HIT_STOP_MAX_EXTRA * HRMath.clamp01((quality - HRC.HIT_STOP_QUALITY) / (1.0 - HRC.HIT_STOP_QUALITY)))
		_shake(HRC.SHAKE_CONTACT * (0.5 + quality), HRC.SHAKE_TICKS)

func _step_flight() -> void:
	var b := _flight
	var was_homer: bool = b.homer
	var landing := HRFielders.project_landing(b.pos, b.vel, HRC.GRAVITY / (HRC.FIXED_HZ * HRC.FIXED_HZ))
	HRFielders.step_fielders(_fielders, _seed, _sim_tick, {} if b.foul else landing)

	var done := HRBall.step_flight(b)
	_ball_pos = b.pos
	_trail.append(b.pos)
	if _trail.size() > TRAIL_MAX:
		_trail.pop_front()

	if not was_homer and b.homer:
		_shake(HRC.SHAKE_HOMER, HRC.SHAKE_TICKS_HOMER)

	var long_hit: bool = not b.foul and b.exitSpeed > 20.0 and b.pos.z > 12.0
	_follow_blend = HRMath.clamp01(_follow_blend + (HRC.CAMERA_FOLLOW_RATE if long_hit else -HRC.CAMERA_FOLLOW_RATE))
	if _follow_blend > HRC.CAMERA_FOLLOW_MAX:
		_follow_blend = HRC.CAMERA_FOLLOW_MAX

	if _cinematic.phase == "contact" and _cinematic.phaseTicks >= HRCine.TUNING.contactSlowMotionDurationTicks:
		_cinematic = HRCinematic.enter_cinematic_phase(_cinematic, "ballFollow")
		_emit({"big": false, "kind": "crowdErupt", "text": ""})

	if not b.homer:
		var who := HRFielders.catching_fielder(_fielders, b.pos)
		if who >= 0:
			var outcome := HRBall.classify_caught(b)
			var dist := _hyp(b.pos.x, b.pos.z)
			var caught_air: bool = b.bounces == 0 and not b.foul
			_resolve("CAUGHT!" if caught_air else "FIELDED", outcome, dist, true)
			return

	if done:
		var outcome2 := HRBall.classify_flight(b)
		var dist2: float
		if outcome2 == "homer":
			dist2 = _hyp(b.pos.x, b.pos.z)
		elif b.firstLandDist > 0.0:
			dist2 = maxf(b.firstLandDist, _hyp(b.pos.x, b.pos.z))
		else:
			dist2 = _hyp(b.pos.x, b.pos.z)
		if _cinematic.phase != "none":
			_cinematic = HRCinematic.enter_cinematic_phase(_cinematic, "landing")
		_resolve(OUTCOME_TEXT[outcome2], outcome2, dist2, false)

func _resolve(text: String, outcome: String, distance: float, caught: bool) -> void:
	var dist := roundi(distance)
	_streak = _streak + 1 if outcome == "homer" else 0
	var points := HRBall.score_for(outcome, float(dist), _streak)
	_score += points
	if outcome == "homer":
		_homers += 1
	if outcome != "miss" and outcome != "ball" and outcome != "foul" and not caught:
		_best_dist = max(_best_dist, dist)
	_results.append({"caught": caught, "distance": dist, "mph": _last_mph, "outcome": outcome, "points": points})

	var suffix := " +%d" % points if points > 0 else ""
	var streak_tag := " x%d" % min(_streak, HRC.STREAK_MULT_CAP) if outcome == "homer" and _streak > 1 else ""
	_emit({"big": outcome == "homer", "kind": outcome, "text": "%s%s%s" % [text, suffix, streak_tag]})

	_flight = {}
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
	var decay := float(_shake_ticks) / float(_shake_total)
	var m := _shake_mag * decay
	return Vector3(sin(_sim_tick * 2.9) * m, cos(_sim_tick * 2.3) * m * 0.6, 0.0)

func _windup_progress() -> float:
	if _phase != "windup" or _phase_ticks < _gap:
		return 0.0
	var w := HRMath.clamp01(float(_phase_ticks - _gap) / float(HRC.WINDUP_TICKS))
	return w * w * (3.0 - 2.0 * w)

func _update_cinematic_camera() -> void:
	var tuning := HRCine.TUNING
	var batter := {"x": _batter_x, "z": HRC.BATTER_Z}

	if _cinematic.phase == "landing" or _cinematic.phase == "celebration":
		return

	if _cinematic.phase == "ballFollow":
		if HRBall.beyond_wall(_ball_pos.x, _ball_pos.z):
			return
		var pose := HRCineCam.ground_tracking_camera_pose(batter, _ball_pos, tuning)
		_cinematic_cam_pos = pose.position
		_cinematic_cam_target = pose.target
		var zoom_target := HRCineCam.ground_tracking_zoom_target(_ball_vel, tuning)
		var zoom_rate: float = 1.0 / tuning.cinematicCameraBlendDurationTicks if tuning.cinematicCameraBlendDurationTicks > 0 else 1.0
		if zoom_target > _ground_camera_zoom:
			_ground_camera_zoom = min(zoom_target, _ground_camera_zoom + zoom_rate)
		else:
			_ground_camera_zoom = max(zoom_target, _ground_camera_zoom - zoom_rate)
		return

	var pose2 := HRCineCam.contact_camera_pose(batter, tuning)
	_cinematic_cam_pos = pose2.position
	_cinematic_cam_target = pose2.target
	_ground_camera_zoom = 0.0

# ── read-only snapshots ──
func view() -> Dictionary:
	var windup := _windup_progress()
	var dolly := windup * HRC.CAMERA_WINDUP_DOLLY + (float(_punch_ticks) / float(HRC.CAMERA_PUNCH_TICKS)) * HRC.CAMERA_RELEASE_PUNCH
	var shake := _shake_offset()
	var gameplay_cam_pos := HRC.CAMERA_POS + Vector3(0, 0, dolly) + shake
	var follow_target := HRC.CAMERA_TARGET.lerp(_ball_pos, _follow_blend) if _follow_blend > 0.0 and _ball_live else HRC.CAMERA_TARGET
	var gameplay_cam_target := follow_target + shake * 0.5

	var cam_blend: float = 0.0 if _cinematic.phase == "none" else _cinematic.camBlend
	var camera_pos := gameplay_cam_pos.lerp(_cinematic_cam_pos, cam_blend) if cam_blend > 0.0 else gameplay_cam_pos
	var camera_target := gameplay_cam_target.lerp(_cinematic_cam_target, cam_blend) if cam_blend > 0.0 else gameplay_cam_target

	var fielders_view: Array = []
	for f in _fielders:
		fielders_view.append({"chasing": f.chasing, "x": f.x, "z": f.z})

	return {
		"ball": _ball_pos,
		"ballInPlay": not _flight.is_empty(),
		"ballVisible": _ball_live,
		"batterX": _batter_x,
		"cameraFovY": HRCineCam.cinematic_fov_y(HRMath.clamp01(_cinematic.zoom + _ground_camera_zoom), HRCine.TUNING),
		"cameraPos": camera_pos,
		"cameraTarget": camera_target,
		"cinematicPhase": _cinematic.phase,
		"fielders": fielders_view,
		"hitStop": _hit_stop > 0,
		"hudVisible": _cinematic.letterbox < 0.5,
		"impactFlash": HRMath.clamp01(_impact_flash / 12.0),
		"letterboxProgress": _cinematic.letterbox,
		"muzzleFlash": HRMath.clamp01(_muzzle_flash / HRC.FLASH_TICKS),
		"phase": _phase,
		"swing": _swing,
		"tick": _sim_tick,
		"trail": _trail,
		"windup": windup,
	}

func _emit(event: Dictionary) -> void:
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
func swing_state() -> Dictionary: return _swing
func results() -> Array: return _results
