# Fielder — one toy defender (world XZ position + a chasing flag), plus the seeded
# wander / chase / catch behaviour that drives a whole roster. Each fielder wanders
# inside a circular patrol region on a seeded two-frequency drift; when a ball is
# hit, nearby fielders converge on the projected landing and can catch/field it.
#
# XZ points are Vector2 with x = world X and y = world Z.
extends RefCounted

const Fielder = preload("res://scripts/fielders.gd")
const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")

var x: float
var z: float
var chasing: bool

static func _at(x: float, z: float, chasing: bool) -> Fielder:
	var f := Fielder.new()
	f.x = x
	f.z = z
	f.chasing = chasing
	return f

static func wander_pos(seed: int, i: int, tick: int) -> Vector2:
	var spot: Dictionary = HRC.FIELDER_SPOTS[i]
	var f1 := HRMath.mix(HRC.WANDER_FREQ_LO, HRC.WANDER_FREQ_HI, HRMath.hash01(seed, [i, 11]))
	var f2 := HRMath.mix(HRC.WANDER_FREQ_LO, HRC.WANDER_FREQ_HI, HRMath.hash01(seed, [i, 12]))
	var p1 := HRMath.hash01(seed, [i, 13]) * PI * 2.0
	var p2 := HRMath.hash01(seed, [i, 14]) * PI * 2.0
	var p3 := HRMath.hash01(seed, [i, 15]) * PI * 2.0
	var amp: float = spot.radius * HRC.WANDER_AMPLITUDE
	var dx := amp * (sin(tick * f1 + p1) * 0.62 + sin(tick * f2 * 1.7 + p3) * 0.38)
	var dz := amp * (sin(tick * f2 + p2) * 0.62 + sin(tick * f1 * 1.7 + p1) * 0.38)
	var d := sqrt(dx * dx + dz * dz)
	var limit: float = spot.radius * 0.95
	if d > limit:
		dx = (dx / d) * limit
		dz = (dz / d) * limit
	return Vector2(spot.x + dx, spot.z + dz)

static func new_roster(seed: int) -> Array:
	var out: Array = []
	for i in range(HRC.FIELDER_SPOTS.size()):
		var w := wander_pos(seed, i, 0)
		out.append(_at(w.x, w.y, false))
	return out

# Project where a ball in flight next reaches ground level (closed form).
static func project_landing(pos: Vector3, vel: Vector3, gravity_per_tick: float) -> Vector2:
	var h: float = pos.y - HRC.BALL_RADIUS
	var disc: float = vel.y * vel.y + 2.0 * gravity_per_tick * maxf(0.0, h)
	var t: float = (vel.y + sqrt(disc)) / gravity_per_tick if gravity_per_tick > 0.0 else 0.0
	return Vector2(pos.x + vel.x * t, pos.z + vel.z * t)

# Advance every fielder one tick (mutates the roster). `has_landing` gates whether a
# ball is in play; `landing` is its projected XZ.
static func step(fielders: Array, seed: int, tick: int, has_landing: bool, landing: Vector2) -> void:
	for i in range(fielders.size()):
		var f: Fielder = fielders[i]
		var spot: Dictionary = HRC.FIELDER_SPOTS[i]
		var tx: float
		var tz: float
		var reachable: bool = has_landing and Vector2(landing.x - spot.x, landing.y - spot.z).length() <= spot.radius * HRC.FIELDER_REACH_MULT
		if reachable:
			var c := Vector2(landing.x - spot.x, landing.y - spot.z)
			var limit: float = spot.radius * HRC.FIELDER_CHASE_CLAMP
			if c.length() > limit:
				c = c.normalized() * limit
			tx = spot.x + c.x
			tz = spot.z + c.y
			f.chasing = true
		else:
			var w := wander_pos(seed, i, tick)
			tx = w.x
			tz = w.y
			f.chasing = false
		var delta := Vector2(tx - f.x, tz - f.z)
		var dd := delta.length()
		if dd > 1e-6:
			var step_v := delta.normalized() * minf(dd, HRC.FIELDER_SPEED)
			f.x += step_v.x
			f.z += step_v.y

# The index of a fielder able to catch/field the ball right now, or -1.
static func catching_fielder(fielders: Array, ball: Vector3) -> int:
	if ball.y > HRC.CATCH_HEIGHT:
		return -1
	for i in range(fielders.size()):
		var f: Fielder = fielders[i]
		if Vector2(ball.x - f.x, ball.z - f.z).length() <= HRC.CATCH_RADIUS:
			return i
	return -1
