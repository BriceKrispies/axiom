# fielders.gd — the toy defenders, ported from fielders.ts. Each owns a circular
# patrol region and wanders inside it on a seeded two-frequency drift; when a ball
# is hit, nearby fielders chase the projected landing (clamped to their region) and
# can catch/field a reachable ball. A FielderState is {x, z, chasing}.

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")

static func _hyp(a: float, b: float) -> float:
	return sqrt(a * a + b * b)

static func wander_pos(seed: int, i: int, tick: int) -> Dictionary:
	var spot: Dictionary = HRC.FIELDER_SPOTS[i]
	var f1 := HRMath.mix(HRC.WANDER_FREQ_LO, HRC.WANDER_FREQ_HI, HRMath.hash01(seed, [i, 11]))
	var f2 := HRMath.mix(HRC.WANDER_FREQ_LO, HRC.WANDER_FREQ_HI, HRMath.hash01(seed, [i, 12]))
	var p1 := HRMath.hash01(seed, [i, 13]) * PI * 2.0
	var p2 := HRMath.hash01(seed, [i, 14]) * PI * 2.0
	var p3 := HRMath.hash01(seed, [i, 15]) * PI * 2.0
	var amp: float = spot.radius * HRC.WANDER_AMPLITUDE
	var dx := amp * (sin(tick * f1 + p1) * 0.62 + sin(tick * f2 * 1.7 + p3) * 0.38)
	var dz := amp * (sin(tick * f2 + p2) * 0.62 + sin(tick * f1 * 1.7 + p1) * 0.38)
	var d := _hyp(dx, dz)
	var limit: float = spot.radius * 0.95
	if d > limit:
		dx = (dx / d) * limit
		dz = (dz / d) * limit
	return {"x": spot.x + dx, "z": spot.z + dz}

static func new_fielders(seed: int) -> Array:
	var out: Array = []
	for i in range(HRC.FIELDER_SPOTS.size()):
		var w := wander_pos(seed, i, 0)
		out.append({"chasing": false, "x": w.x, "z": w.z})
	return out

static func project_landing(pos: Vector3, vel: Vector3, gravity_per_tick: float) -> Dictionary:
	var h: float = pos.y - HRC.BALL_RADIUS
	var disc: float = vel.y * vel.y + 2.0 * gravity_per_tick * maxf(0.0, h)
	var t: float = (vel.y + sqrt(disc)) / gravity_per_tick if gravity_per_tick > 0.0 else 0.0
	return {"x": pos.x + vel.x * t, "z": pos.z + vel.z * t}

# Advance every fielder one tick (mutates `fielders`). `landing` is a {x,z} dict, or
# an empty Dictionary when no ball is in play.
static func step_fielders(fielders: Array, seed: int, tick: int, landing: Dictionary) -> void:
	for i in range(fielders.size()):
		var f: Dictionary = fielders[i]
		var spot: Dictionary = HRC.FIELDER_SPOTS[i]
		var tx: float
		var tz: float
		var reachable: bool = not landing.is_empty() and _hyp(landing.x - spot.x, landing.z - spot.z) <= spot.radius * HRC.FIELDER_REACH_MULT
		if reachable:
			var cx: float = landing.x - spot.x
			var cz: float = landing.z - spot.z
			var d := _hyp(cx, cz)
			var limit: float = spot.radius * HRC.FIELDER_CHASE_CLAMP
			if d > limit:
				cx = (cx / d) * limit
				cz = (cz / d) * limit
			tx = spot.x + cx
			tz = spot.z + cz
			f.chasing = true
		else:
			var w := wander_pos(seed, i, tick)
			tx = w.x
			tz = w.z
			f.chasing = false
		var dx: float = tx - f.x
		var dz: float = tz - f.z
		var dd := _hyp(dx, dz)
		var step: float = min(dd, HRC.FIELDER_SPEED)
		if dd > 1e-6:
			f.x += (dx / dd) * step
			f.z += (dz / dd) * step

static func catching_fielder(fielders: Array, ball: Vector3) -> int:
	if ball.y > HRC.CATCH_HEIGHT:
		return -1
	for i in range(fielders.size()):
		var f: Dictionary = fielders[i]
		if _hyp(ball.x - f.x, ball.z - f.z) <= HRC.CATCH_RADIUS:
			return i
	return -1
