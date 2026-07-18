# cinematic.gd — the home-run cinematic's own state machine, ported from cinematic.ts.
# session.gd calls enter_cinematic_phase at each event boundary and step_cinematic
# every real tick the cinematic is live; presentation values (letterbox, zoom, camera
# blend, time scale) ease continuously across phase boundaries. A CinematicState is
# {camBlend, elapsedTicks, impactParticles, letterbox, phase, phaseTicks, timeScale, zoom}.

const HRMath = preload("res://scripts/math_util.gd")

const LETTERBOX_UP := ["anticipation", "contact"]
const CAMERA_OWNED := ["anticipation", "contact", "ballFollow", "landing"]

static func new_cinematic() -> Dictionary:
	return {"camBlend": 0.0, "elapsedTicks": 0, "impactParticles": 0, "letterbox": 0.0, "phase": "none", "phaseTicks": 0, "timeScale": 1.0, "zoom": 0.0}

static func enter_cinematic_phase(state: Dictionary, phase: String) -> Dictionary:
	var n := state.duplicate()
	n.phase = phase
	n.phaseTicks = 0
	return n

static func _approach(current: float, target: float, duration_ticks: float) -> float:
	var step: float = 1.0 / duration_ticks if duration_ticks > 0.0 else 1.0
	return min(target, current + step) if target > current else max(target, current - step)

static func _time_scale_target(phase: String, phase_ticks: int, tuning: Dictionary) -> float:
	match phase:
		"anticipation":
			return tuning.contactSlowMotionScale
		"ballFollow":
			return 1.0 if phase_ticks > tuning.contactSlowMotionDurationTicks else tuning.postContactSlowMotionScale
		"contact":
			return tuning.contactSlowMotionScale
		_:
			return 1.0

static func step_cinematic(state: Dictionary, tuning: Dictionary) -> Dictionary:
	if state.phase == "none":
		return state
	var phase_ticks: int = state.phaseTicks + 1
	var elapsed_ticks: int = state.elapsedTicks + 1

	var letterbox_target := 1.0 if state.phase in LETTERBOX_UP else 0.0
	var letterbox := _approach(state.letterbox, letterbox_target, tuning.letterboxEntranceDurationTicks if letterbox_target == 1.0 else tuning.letterboxExitDurationTicks)

	var zoom := _approach(state.zoom, letterbox_target, tuning.cinematicCameraBlendDurationTicks)

	var cam_blend_target := 1.0 if state.phase in CAMERA_OWNED else 0.0
	var cam_blend := _approach(state.camBlend, cam_blend_target, tuning.cinematicCameraBlendDurationTicks)

	var target := _time_scale_target(state.phase, phase_ticks, tuning)
	var ramp_duration: float = tuning.timeScaleRecoveryDurationTicks if target > state.timeScale else tuning.cinematicCameraBlendDurationTicks
	var time_scale := clampf(_approach(state.timeScale, target, ramp_duration), tuning.contactSlowMotionScale, 1.0)

	var impact_particles: int = min(tuning.impactParticleMaxCount, phase_ticks * 2) if state.phase == "contact" else max(0, state.impactParticles - 1)

	return {"camBlend": HRMath.clamp01(cam_blend), "elapsedTicks": elapsed_ticks, "impactParticles": impact_particles, "letterbox": HRMath.clamp01(letterbox), "phase": state.phase, "phaseTicks": phase_ticks, "timeScale": time_scale, "zoom": HRMath.clamp01(zoom)}
