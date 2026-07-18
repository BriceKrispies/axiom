# pitch.gd — the deterministic seeded pitch sequence, ported from pitch.ts. A pitch
# is a pure function of (seed, pitchIndex): difficulty-ramped profile selection then
# a small seeded jitter around the profile's aim. A PitchSpec is
# {profileId, name, speed, gravity, targetX, targetY, mph}.

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")

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

static func select_pitch(seed: int, pitch_index: int) -> Dictionary:
	var pool := pitch_pool(pitch_index)
	var idx: int = min(pool.size() - 1, int(floor(HRMath.hash01(seed, [pitch_index, 1]) * pool.size())))
	var profile: Dictionary = pool[idx]
	var speed: float = profile.speed * (1.0 + (HRMath.hash01(seed, [pitch_index, 2]) - 0.5) * 2.0 * HRC.JITTER_SPEED)
	return {
		"gravity": profile.gravity,
		"mph": roundi(speed * HRC.MPH_PER_UNIT),
		"name": profile.name,
		"profileId": profile.id,
		"speed": speed,
		"targetX": profile.targetX + (HRMath.hash01(seed, [pitch_index, 3]) - 0.5) * 2.0 * HRC.JITTER_X,
		"targetY": profile.targetY + (HRMath.hash01(seed, [pitch_index, 4]) - 0.5) * 2.0 * HRC.JITTER_Y,
	}

static func pitch_gap_ticks(seed: int, pitch_index: int) -> int:
	return HRC.GAP_TICKS + int(floor(HRMath.hash01(seed, [pitch_index, 5]) * HRC.GAP_JITTER_TICKS))

static func is_strike(x: float, y: float) -> bool:
	return absf(x) <= HRC.STRIKE_ZONE_HALF_X and y >= HRC.STRIKE_ZONE_LOW and y <= HRC.STRIKE_ZONE_HIGH

static func solve_pitch(spec: Dictionary) -> Dictionary:
	var release := HRC.PITCH_RELEASE
	var vz: float = -spec.speed / HRC.FIXED_HZ
	var flight_ticks: float = release.z / (spec.speed / HRC.FIXED_HZ)
	var gravity_per_tick: float = spec.gravity / (HRC.FIXED_HZ * HRC.FIXED_HZ)
	var vx: float = (spec.targetX - release.x) / flight_ticks
	var vy: float = (spec.targetY - release.y + 0.5 * gravity_per_tick * flight_ticks * (flight_ticks + 1.0)) / flight_ticks
	return {"gravityPerTick": gravity_per_tick, "vel": Vector3(vx, vy, vz)}
