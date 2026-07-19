# Cinematic — the home-run cinematic's own state machine. The session calls
# enter_phase() at each event boundary and step() every real tick the cinematic is
# live; presentation values (letterbox, zoom, camera blend, time scale) ease
# continuously across phase boundaries. `phase` is one of
# "none"/"anticipation"/"contact"/"ballFollow"/"landing"/"celebration".
extends RefCounted

const Cinematic = preload("res://scripts/cinematic.gd")
const HRMath = preload("res://scripts/math_util.gd")

const LETTERBOX_UP := ["anticipation", "contact"]
const CAMERA_OWNED := ["anticipation", "contact", "ballFollow", "landing"]

var phase: String
var phase_ticks: int
var elapsed_ticks: int
var letterbox: float
var time_scale: float
var zoom: float
var cam_blend: float
var impact_particles: int

static func new_cinematic() -> Cinematic:
	var c := Cinematic.new()
	c.phase = "none"
	c.phase_ticks = 0
	c.elapsed_ticks = 0
	c.letterbox = 0.0
	c.time_scale = 1.0
	c.zoom = 0.0
	c.cam_blend = 0.0
	c.impact_particles = 0
	return c

func _copy() -> Cinematic:
	var c := Cinematic.new()
	c.phase = phase
	c.phase_ticks = phase_ticks
	c.elapsed_ticks = elapsed_ticks
	c.letterbox = letterbox
	c.time_scale = time_scale
	c.zoom = zoom
	c.cam_blend = cam_blend
	c.impact_particles = impact_particles
	return c

func enter_phase(next_phase: String) -> Cinematic:
	var c := _copy()
	c.phase = next_phase
	c.phase_ticks = 0
	return c

static func _approach(current: float, target: float, duration_ticks: float) -> float:
	var st: float = 1.0 / duration_ticks if duration_ticks > 0.0 else 1.0
	return min(target, current + st) if target > current else max(target, current - st)

func _time_scale_target(next_phase_ticks: int, tuning: Dictionary) -> float:
	match phase:
		"anticipation":
			return tuning.contactSlowMotionScale
		"ballFollow":
			return 1.0 if next_phase_ticks > tuning.contactSlowMotionDurationTicks else tuning.postContactSlowMotionScale
		"contact":
			return tuning.contactSlowMotionScale
		_:
			return 1.0

func step(tuning: Dictionary) -> Cinematic:
	if phase == "none":
		return self
	var next := _copy()
	next.phase_ticks = phase_ticks + 1
	next.elapsed_ticks = elapsed_ticks + 1

	var letterbox_target := 1.0 if phase in LETTERBOX_UP else 0.0
	next.letterbox = HRMath.clamp01(_approach(letterbox, letterbox_target, tuning.letterboxEntranceDurationTicks if letterbox_target == 1.0 else tuning.letterboxExitDurationTicks))
	next.zoom = HRMath.clamp01(_approach(zoom, letterbox_target, tuning.cinematicCameraBlendDurationTicks))

	var cam_blend_target := 1.0 if phase in CAMERA_OWNED else 0.0
	next.cam_blend = HRMath.clamp01(_approach(cam_blend, cam_blend_target, tuning.cinematicCameraBlendDurationTicks))

	var target := _time_scale_target(next.phase_ticks, tuning)
	var ramp: float = tuning.timeScaleRecoveryDurationTicks if target > time_scale else tuning.cinematicCameraBlendDurationTicks
	next.time_scale = clampf(_approach(time_scale, target, ramp), tuning.contactSlowMotionScale, 1.0)

	next.impact_particles = min(tuning.impactParticleMaxCount, next.phase_ticks * 2) if phase == "contact" else max(0, impact_particles - 1)
	return next
