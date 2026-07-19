# SunState — the wall-clock sun for a given time: a directional-light colour/
# direction/energy plus the ground-shadow projection (direction + stretch) the actor
# shadow ellipses use. compute() is a pure function of elapsed milliseconds.
extends RefCounted

const SunState = preload("res://scripts/sun.gd")
const HRMath = preload("res://scripts/math_util.gd")

const LAP_MS := 40 * 60 * 1000
const NOON_MS := (40 * 60 * 1000) / 2
const START_MS := (40 * 60 * 1000) * 0.3
const ELEV_LOW := 0.14
const ELEV_HIGH := 0.42
const GROUND := 0.28
const GLARE_MAX := 1.5
const STRETCH_MAX := 1.5

var color: Color
var direction: Vector3
var energy: float
var dx: float          # unit XZ direction the projected shadows run (away from the sun)
var dz: float
var stretch: float     # shadow length per unit of caster height (capped)

static func compute(time_ms: float) -> SunState:
	var azimuth := (fmod(time_ms, LAP_MS) / LAP_MS) * PI * 2.0
	var height := 0.5 - 0.5 * cos(azimuth)
	var elev := HRMath.mix(ELEV_LOW, ELEV_HIGH, height)
	var sun_x := cos(elev) * sin(azimuth)
	var sun_y := sin(elev)
	var sun_z := cos(elev) * cos(azimuth)
	var horiz := sqrt(sun_x * sun_x + sun_z * sun_z)
	var glow := sqrt(height)
	var s := SunState.new()
	s.color = Color(1.0, HRMath.mix(0.62, 0.82, glow), HRMath.mix(0.34, 0.6, glow))
	s.direction = Vector3(-sun_x, -sun_y, -sun_z)
	s.energy = minf(GROUND / sin(elev), GLARE_MAX)
	s.dx = -sun_x / horiz
	s.dz = -sun_z / horiz
	s.stretch = minf(horiz / sun_y, STRETCH_MAX)
	return s
