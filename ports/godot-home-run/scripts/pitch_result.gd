# PitchResult — the per-pitch log entry recorded when a pitch resolves.
extends RefCounted

const PitchResult = preload("res://scripts/pitch_result.gd")

var outcome: String
var points: int
var distance: int
var mph: int
var caught: bool

static func make(outcome: String, points: int, distance: int, mph: int, caught: bool) -> PitchResult:
	var r := PitchResult.new()
	r.outcome = outcome
	r.points = points
	r.distance = distance
	r.mph = mph
	r.caught = caught
	return r
