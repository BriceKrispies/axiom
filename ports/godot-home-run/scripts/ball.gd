# BallFlight — a ball in play (post-contact): flight integration, the toy stadium's
# boundary rules (foul wedge, wall line, home-run volume), outcome classification,
# and the scoring table. `step()` advances one tick in place and returns true once
# the ball rests or dies.
extends RefCounted

const BallFlight = preload("res://scripts/ball.gd")
const HRC = preload("res://scripts/constants.gd")

const SQRT1_2 := 0.7071067811865476

var pos: Vector3
var vel: Vector3
var bounces: int
var first_land_dist: float   # horizontal distance of the FIRST ground touch (0 until it lands)
var homer: bool
var foul: bool
var exit_speed: float         # launch params frozen at contact
var loft: float
var spray: float
var ticks: int

static func new_flight(pos: Vector3, vel: Vector3, exit_speed: float, loft: float, spray: float) -> BallFlight:
	var b := BallFlight.new()
	b.pos = pos
	b.vel = vel
	b.bounces = 0
	b.first_land_dist = 0.0
	b.homer = false
	b.foul = absf(spray) > HRC.FOUL_ANGLE
	b.exit_speed = exit_speed
	b.loft = loft
	b.spray = spray
	b.ticks = 0
	return b

static func is_fair(x: float, z: float) -> bool:
	return z >= 0.0 and absf(x) <= z

static func beyond_wall(x: float, z: float) -> bool:
	return absf(x) + z >= HRC.WALL_LINE

static func _hyp(a: float, b: float) -> float:
	return sqrt(a * a + b * b)

func step() -> bool:
	ticks += 1
	var g: float = HRC.GRAVITY / (HRC.FIXED_HZ * HRC.FIXED_HZ)
	vel = Vector3(vel.x, vel.y - g, vel.z)
	var next := Vector3(pos.x + vel.x, pos.y + vel.y, pos.z + vel.z)

	if not homer and not beyond_wall(pos.x, pos.z) and beyond_wall(next.x, next.z):
		if foul:
			return true
		if next.y >= HRC.WALL_HEIGHT:
			homer = true
		else:
			var sx := 1.0 if pos.x >= 0.0 else -1.0
			var nx := -sx * SQRT1_2
			var nz := -SQRT1_2
			var vn: float = vel.x * nx + vel.z * nz
			vel = Vector3(
				(vel.x - 2.0 * vn * nx) * HRC.WALL_RESTITUTION,
				vel.y * HRC.WALL_RESTITUTION,
				(vel.z - 2.0 * vn * nz) * HRC.WALL_RESTITUTION,
			)
			next = Vector3(pos.x + vel.x, pos.y + vel.y, pos.z + vel.z)

	if next.y <= HRC.BALL_RADIUS and vel.y < 0.0:
		if first_land_dist == 0.0:
			first_land_dist = _hyp(next.x, next.z)
			if not is_fair(next.x, next.z) and not homer:
				foul = true
		bounces += 1
		next = Vector3(next.x, HRC.BALL_RADIUS, next.z)
		vel = Vector3(vel.x * HRC.BOUNCE_FRICTION, -vel.y * HRC.BOUNCE_RESTITUTION, vel.z * HRC.BOUNCE_FRICTION)
		if bounces > 3 or absf(vel.y) * HRC.FIXED_HZ < 1.2:
			vel = Vector3(vel.x, 0.0, vel.z)

	if next.y <= HRC.BALL_RADIUS + 1e-6 and vel.y == 0.0:
		vel = Vector3(vel.x * HRC.ROLL_DECAY, 0.0, vel.z * HRC.ROLL_DECAY)
		if _hyp(vel.x, vel.z) * HRC.FIXED_HZ < HRC.REST_SPEED:
			pos = next
			return true

	if next.z <= HRC.CATCHER_Z:
		pos = next
		return true
	pos = next
	return ticks >= HRC.FLIGHT_TIMEOUT_TICKS

func classify_flight() -> String:
	if homer:
		return "homer"
	if foul:
		return "foul"
	if exit_speed < HRC.WEAK_EXIT_SPEED:
		return "weak"
	if loft < HRC.GROUNDER_LOFT:
		return "grounder"
	var dist: float = first_land_dist if first_land_dist > 0.0 else _hyp(pos.x, pos.z)
	if loft > HRC.POPUP_LOFT and dist < HRC.POPUP_MAX_DIST:
		return "popup"
	return "clean"

func classify_caught() -> String:
	if foul:
		return "foul"
	if bounces == 0:
		return "popup" if loft > HRC.POPUP_LOFT else "weak"
	return "grounder" if loft < HRC.GROUNDER_LOFT else "weak"

static func score_for(outcome: String, distance: float, homer_streak: int) -> int:
	var base: int = HRC.SCORE_TABLE[outcome]
	if outcome == "clean":
		return base + roundi(distance * HRC.CLEAN_DIST_BONUS)
	if outcome == "homer":
		return (base + roundi(distance * HRC.HOMER_DIST_BONUS)) * clampi(homer_streak, 1, HRC.STREAK_MULT_CAP)
	return base
