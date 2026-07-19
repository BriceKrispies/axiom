# HUD — the on-screen overlay: the bottom stat bar, the centre outcome message, the
# ready/over prompts, the swing-cooldown meter, and the cinematic letterbox bars. It
# polls the session for continuous state (update) and receives one-shot outcome text
# via a signal (on_feedback).
extends Control

const HRCine = preload("res://scripts/cinematic_constants.gd")
const SceneView = preload("res://scripts/scene_view.gd")
const HomeRunSession = preload("res://scripts/session.gd")

var _score: Label
var _pitch: Label
var _hr: Label
var _streak: Label
var _speed: Label
var _best: Label
var _message: Label
var _ready_lbl: Label
var _over: Label
var _meter_bg: ColorRect
var _meter_fill: ColorRect
var _lb_top: ColorRect
var _lb_bottom: ColorRect

func _label(size: int, color: Color) -> Label:
	var l := Label.new()
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", color)
	return l

func _ready() -> void:
	set_anchors_preset(Control.PRESET_FULL_RECT)
	mouse_filter = Control.MOUSE_FILTER_IGNORE

	_lb_top = ColorRect.new()
	_lb_top.color = Color.BLACK
	_lb_top.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(_lb_top)
	_lb_bottom = ColorRect.new()
	_lb_bottom.color = Color.BLACK
	_lb_bottom.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(_lb_bottom)

	var bar := HBoxContainer.new()
	bar.add_theme_constant_override("separation", 26)
	bar.set_anchors_preset(Control.PRESET_CENTER_BOTTOM)
	bar.position = Vector2(0, -46)
	bar.grow_horizontal = Control.GROW_DIRECTION_BOTH
	add_child(bar)
	var stat := Color(0.86, 0.9, 1.0)
	_score = _label(20, Color(1, 0.85, 0.2))
	_pitch = _label(20, stat)
	_hr = _label(20, stat)
	_streak = _label(20, Color(1, 0.7, 0.3))
	_speed = _label(18, stat)
	_best = _label(18, stat)
	for l: Label in [_score, _pitch, _hr, _streak, _speed, _best]:
		bar.add_child(l)

	_message = _label(64, Color(1, 0.95, 0.55))
	_message.set_anchors_preset(Control.PRESET_CENTER)
	_message.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_message.grow_vertical = Control.GROW_DIRECTION_BOTH
	_message.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	add_child(_message)

	_ready_lbl = _label(34, Color(0.9, 0.95, 1))
	_ready_lbl.text = "PRESS SPACE TO BAT"
	_ready_lbl.set_anchors_preset(Control.PRESET_CENTER)
	_ready_lbl.position = Vector2(-160, 120)
	add_child(_ready_lbl)

	_over = _label(40, Color(1, 0.85, 0.3))
	_over.set_anchors_preset(Control.PRESET_CENTER)
	_over.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_over.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	add_child(_over)

	_meter_bg = ColorRect.new()
	_meter_bg.color = Color(0.1, 0.13, 0.2, 0.7)
	_meter_bg.set_anchors_preset(Control.PRESET_CENTER_BOTTOM)
	_meter_bg.position = Vector2(-90, -84)
	_meter_bg.size = Vector2(180, 10)
	_meter_bg.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(_meter_bg)
	_meter_fill = ColorRect.new()
	_meter_fill.color = Color(1, 0.8, 0.25)
	_meter_fill.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_meter_bg.add_child(_meter_fill)

func on_feedback(kind: String, text: String, _big: bool) -> void:
	if text.length() > 0 and kind != "release":
		_message.text = text
		_message.modulate.a = 1.0

func update(session: HomeRunSession, view: SceneView) -> void:
	_score.text = "SCORE %d" % session.score()
	_pitch.text = "PITCH %d/%d" % [session.pitch_number(), 10]
	_hr.text = "HR %d" % session.homers()
	_streak.text = "%dx" % session.streak_multiplier()
	_speed.text = ("%d MPH %s" % [session.last_mph(), session.last_pitch_name()]) if session.last_mph() > 0 else "-"
	_best.text = ("BEST %dm" % session.best_distance()) if session.best_distance() > 0 else "BEST -"

	var phase := view.phase
	var ready_now := session.current_swing().state == "ready"
	var live := phase != "ready" and phase != "over"
	_meter_bg.visible = live and not ready_now
	_meter_fill.size = Vector2(180.0 * session.current_swing().readiness, 10)
	_ready_lbl.visible = phase == "ready"

	var over := phase == "over"
	_over.visible = over
	if over:
		var best := "%dm" % session.best_distance() if session.best_distance() > 0 else "-"
		_over.text = "ROUND OVER\nSCORE %d   HR %d   BEST %s" % [session.score(), session.homers(), best]

	if _message.modulate.a > 0.0:
		_message.modulate.a = maxf(0.0, _message.modulate.a - 0.02)

	var bar_frac: float = view.letterbox_progress * HRCine.TUNING.letterboxScreenFraction
	var vp := get_viewport_rect().size
	var bar_h := vp.y * bar_frac
	_lb_top.position = Vector2.ZERO
	_lb_top.size = Vector2(vp.x, bar_h)
	_lb_bottom.position = Vector2(0, vp.y - bar_h)
	_lb_bottom.size = Vector2(vp.x, bar_h)
	modulate.a = 1.0 if view.hud_visible else 0.25
