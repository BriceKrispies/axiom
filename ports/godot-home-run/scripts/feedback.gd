# Feedback — one buffered gameplay event, drained each tick by the host for HUD text
# and audio cues. `kind` is an outcome ("homer", "clean", ...) or a cue
# ("windup", "release", "contact", "caught", "cinematicAnticipation", "crowdErupt").
extends RefCounted

const Feedback = preload("res://scripts/feedback.gd")

var kind: String
var text: String
var big: bool

static func make(kind: String, text: String, big: bool) -> Feedback:
	var f := Feedback.new()
	f.kind = kind
	f.text = text
	f.big = big
	return f
