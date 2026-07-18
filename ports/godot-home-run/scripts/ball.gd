# ball.gd — ball flight for a ball in play (post-contact), the toy stadium boundary
# rules (foul wedge, wall line, home-run volume), outcome classification, and the
# scoring table. Ported from ball.ts. A BallFlight is a Dictionary mutated in place:
# {pos, vel, bounces, firstLandDist, homer, foul, exitSpeed, loft, spray, ticks}.
class_name HRBall

const SQRT1_2 := 0.7071067811865476

static func new_flight(pos: Vector3, vel: Vector3, exit_speed: float, loft: float, spray: float) -> Dictionary:
	return {
		"bounces": 0, "exitSpeed": exit_speed, "firstLandDist": 0.0,
		"foul": absf(spray) > HRC.FOUL_ANGLE, "homer": false, "loft": loft,
		"pos": pos, "spray": spray, "ticks": 0, "vel": vel,
	}

static func is_fair(x: float, z: float) -> bool:
	return z >= 0.0 and absf(x) <= z

static func beyond_wall(x: float, z: float) -> bool:
	return absf(x) + z >= HRC.WALL_LINE

static func _hyp(a: float, b: float) -> float:
	return sqrt(a * a + b * b)

# Advance the flight one tick (mutates b). Returns true once the ball rests/dies.
static func step_flight(b: Dictionary) -> bool:
	b.ticks += 1
	var g: float = HRC.GRAVITY / (HRC.FIXED_HZ * HRC.FIXED_HZ)
	b.vel = Vector3(b.vel.x, b.vel.y - g, b.vel.z)
	var next := Vector3(b.pos.x + b.vel.x, b.pos.y + b.vel.y, b.pos.z + b.vel.z)

	if not b.homer and not beyond_wall(b.pos.x, b.pos.z) and beyond_wall(next.x, next.z):
		if b.foul:
			return true
		if next.y >= HRC.WALL_HEIGHT:
			b.homer = true
		else:
			var sx := 1.0 if b.pos.x >= 0.0 else -1.0
			var inv := SQRT1_2
			var nx := -sx * inv
			var nz := -inv
			var vn: float = b.vel.x * nx + b.vel.z * nz
			b.vel = Vector3(
				(b.vel.x - 2.0 * vn * nx) * HRC.WALL_RESTITUTION,
				b.vel.y * HRC.WALL_RESTITUTION,
				(b.vel.z - 2.0 * vn * nz) * HRC.WALL_RESTITUTION,
			)
			next = Vector3(b.pos.x + b.vel.x, b.pos.y + b.vel.y, b.pos.z + b.vel.z)

	if next.y <= HRC.BALL_RADIUS and b.vel.y < 0.0:
		if b.firstLandDist == 0.0:
			b.firstLandDist = _hyp(next.x, next.z)
			if not is_fair(next.x, next.z) and not b.homer:
				b.foul = true
		b.bounces += 1
		next = Vector3(next.x, HRC.BALL_RADIUS, next.z)
		b.vel = Vector3(b.vel.x * HRC.BOUNCE_FRICTION, -b.vel.y * HRC.BOUNCE_RESTITUTION, b.vel.z * HRC.BOUNCE_FRICTION)
		if b.bounces > 3 or absf(b.vel.y) * HRC.FIXED_HZ < 1.2:
			b.vel = Vector3(b.vel.x, 0.0, b.vel.z)

	if next.y <= HRC.BALL_RADIUS + 1e-6 and b.vel.y == 0.0:
		b.vel = Vector3(b.vel.x * HRC.ROLL_DECAY, 0.0, b.vel.z * HRC.ROLL_DECAY)
		var speed := _hyp(b.vel.x, b.vel.z) * HRC.FIXED_HZ
		if speed < HRC.REST_SPEED:
			b.pos = next
			return true

	if next.z <= HRC.CATCHER_Z:
		b.pos = next
		return true
	b.pos = next
	return b.ticks >= HRC.FLIGHT_TIMEOUT_TICKS

static func classify_flight(b: Dictionary) -> String:
	if b.homer:
		return "homer"
	if b.foul:
		return "foul"
	if b.exitSpeed < HRC.WEAK_EXIT_SPEED:
		return "weak"
	if b.loft < HRC.GROUNDER_LOFT:
		return "grounder"
	var dist: float = b.firstLandDist if b.firstLandDist > 0.0 else _hyp(b.pos.x, b.pos.z)
	if b.loft > HRC.POPUP_LOFT and dist < HRC.POPUP_MAX_DIST:
		return "popup"
	return "clean"

static func classify_caught(b: Dictionary) -> String:
	if b.foul:
		return "foul"
	if b.bounces == 0:
		return "popup" if b.loft > HRC.POPUP_LOFT else "weak"
	return "grounder" if b.loft < HRC.GROUNDER_LOFT else "weak"

static func score_for(outcome: String, distance: float, homer_streak: int) -> int:
	var base: int = HRC.SCORE_TABLE[outcome]
	if outcome == "clean":
		return base + roundi(distance * HRC.CLEAN_DIST_BONUS)
	if outcome == "homer":
		var mult: int = clampi(homer_streak, 1, HRC.STREAK_MULT_CAP)
		return (base + roundi(distance * HRC.HOMER_DIST_BONUS)) * mult
	return base
