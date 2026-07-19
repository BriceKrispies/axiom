# PitchSpec — one selected, jittered pitch, plus the deterministic seeded selection
# that produces it. A pitch is a pure function of (seed, pitch_index): a difficulty-
# ramped profile choice then a small seeded jitter, so the same seed reproduces the
# same ten pitches. Profiles are read from Const.PITCH_PROFILES (config data).
extends RefCounted

const PitchSpec = preload("res://scripts/pitch.gd")
const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")

var profile_id: String
var name: String
var speed: float      # u/s toward the plate (after jitter)
var gravity: float
var target_x: float
var target_y: float
var mph: int

static func pitch_pool(pitch_index: int) -> Array:
	if pitch_index < HRC.EASY_ONLY_BEFORE:
		return HRC.PITCH_PROFILES.filter(func(p): return p.tier == "easy")
	if pitch_index < HRC.HARD_ALLOWED_FROM:
		return HRC.PITCH_PROFILES.filter(func(p): return p.tier != "hard")
	var weighted: Array = []
	for p in HRC.PITCH_PROFILES:
		var copies: int = HRC.HARD_LATE_WEIGHT if p.tier == "hard" else 1
		for k in range(copies):
			weighted.append(p)
	return weighted

static func select_pitch(seed: int, pitch_index: int) -> PitchSpec:
	var pool := pitch_pool(pitch_index)
	var idx: int = min(pool.size() - 1, int(floor(HRMath.hash01(seed, [pitch_index, 1]) * pool.size())))
	var profile: Dictionary = pool[idx]
	var speed: float = profile.speed * (1.0 + (HRMath.hash01(seed, [pitch_index, 2]) - 0.5) * 2.0 * HRC.JITTER_SPEED)
	var spec := PitchSpec.new()
	spec.profile_id = profile.id
	spec.name = profile.name
	spec.speed = speed
	spec.gravity = profile.gravity
	spec.target_x = profile.target_x + (HRMath.hash01(seed, [pitch_index, 3]) - 0.5) * 2.0 * HRC.JITTER_X
	spec.target_y = profile.target_y + (HRMath.hash01(seed, [pitch_index, 4]) - 0.5) * 2.0 * HRC.JITTER_Y
	spec.mph = roundi(speed * HRC.MPH_PER_UNIT)
	return spec

static func pitch_gap_ticks(seed: int, pitch_index: int) -> int:
	return HRC.GAP_TICKS + int(floor(HRMath.hash01(seed, [pitch_index, 5]) * HRC.GAP_JITTER_TICKS))

static func is_strike(x: float, y: float) -> bool:
	return absf(x) <= HRC.STRIKE_ZONE_HALF_X and y >= HRC.STRIKE_ZONE_LOW and y <= HRC.STRIKE_ZONE_HIGH

func gravity_per_tick() -> float:
	return gravity / (HRC.FIXED_HZ * HRC.FIXED_HZ)

# Release velocity (per TICK) that carries the ball from PITCH_RELEASE to the
# plate-crossing target under this pitch's own gravity (closed form).
func solve_velocity() -> Vector3:
	var release := HRC.PITCH_RELEASE
	var vz := -speed / HRC.FIXED_HZ
	var flight_ticks: float = release.z / (speed / HRC.FIXED_HZ)
	var g := gravity_per_tick()
	var vx := (target_x - release.x) / flight_ticks
	var vy := (target_y - release.y + 0.5 * g * flight_ticks * (flight_ticks + 1.0)) / flight_ticks
	return Vector3(vx, vy, vz)
