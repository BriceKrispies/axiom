# game.gd — the Main scene host. It owns the fixed-step loop (_physics_process at the
# project's 60 Hz physics tick), reads input through the InputMap, advances the
# HomeRunSession, and drives the presentation: the static MultiMesh stadium (built
# once), the persistent actor views (batter / machine / ball / ten fielders), the
# camera, the two directional lights, the HUD, and the audio cues. Gameplay events
# are re-emitted as the `feedback` signal (the HUD's message + the audio connect to
# it), the idiomatic decoupling from the polled per-frame state.
extends Node3D

const HRC = preload("res://scripts/constants.gd")
const HomeRunSession = preload("res://scripts/session.gd")
const HRMaterials = preload("res://scripts/materials.gd")
const Stadium = preload("res://scripts/stadium.gd")
const SunState = preload("res://scripts/sun.gd")
const BatterView = preload("res://scripts/batter_view.gd")
const MachineView = preload("res://scripts/machine_view.gd")
const BallView = preload("res://scripts/ball_view.gd")
const FielderView = preload("res://scripts/fielder_view.gd")
const AudioScript = preload("res://scripts/audio.gd")
const HudScript = preload("res://scripts/hud.gd")
const Canvas2DRenderer = preload("res://scripts/canvas2d_renderer.gd")

signal feedback(kind: String, text: String, big: bool)

@onready var _camera: Camera3D = $Camera
@onready var _sun_light: DirectionalLight3D = $Sun
@onready var _fill_light: DirectionalLight3D = $Fill
@onready var _static_root: Node3D = $Field/Static
@onready var _actors_root: Node3D = $Field/Actors
@onready var _hud: HudScript = $HUD/Root

var _session: HomeRunSession
var _meshes: Dictionary
var _materials: Dictionary
var _batter: BatterView
var _machine: MachineView
var _ball: BallView
var _fielders: Array[FielderView] = []
var _audio: AudioScript
var _canvas: Canvas2DRenderer
var _canvas_mode := false
var _touch_left := false
var _touch_right := false
var _touch_swing := false

# Screenshot affordance: run with `-- shot <frame> [out.png] [seed] [swingAt]`.
var _shot_at := -1
var _shot_out := "user://shot.png"
var _shot_swing_at := -1
var _shot_seed := -1
var _frame := 0

func _ready() -> void:
	randomize()
	_parse_shot_args()
	_canvas_mode = OS.get_cmdline_user_args().has("canvas2d")
	# On the web, also honor ?backend=canvas2d in the URL (Axiom's convention).
	if OS.has_feature("web"):
		var q = JavaScriptBridge.eval("location.search || ''", true)
		if typeof(q) == TYPE_STRING and "canvas2d" in q:
			_canvas_mode = true
	_register_input()
	_build_meshes()
	_materials = HRMaterials.build()

	Stadium.build(_static_root, _meshes, _materials)

	_batter = BatterView.new()
	_batter.build(_meshes, _materials)
	_actors_root.add_child(_batter)
	_machine = MachineView.new()
	_machine.build(_meshes, _materials)
	_actors_root.add_child(_machine)
	_ball = BallView.new()
	_ball.build(_meshes, _materials)
	_actors_root.add_child(_ball)
	for i in range(HRC.FIELDER_SPOTS.size()):
		var fv := FielderView.new()
		fv.build(_meshes, _materials)
		_actors_root.add_child(fv)
		_fielders.append(fv)

	_audio = AudioScript.new()
	add_child(_audio)

	# The software "3D attempt" backend: a Node2D on a CanvasLayer below the HUD,
	# reading the same 3D nodes and rasterizing them to 2D (toggle with B).
	var canvas_layer := CanvasLayer.new()
	canvas_layer.layer = 0
	add_child(canvas_layer)
	_canvas = Canvas2DRenderer.new()
	_canvas.setup(_camera, $Field/Static, $Field/Actors)
	canvas_layer.add_child(_canvas)

	_camera.near = HRC.CAMERA_NEAR
	_camera.far = HRC.CAMERA_FAR
	_fill_light.shadow_enabled = false
	_fill_light.light_color = Color(0.72, 0.8, 1.0)
	_fill_light.light_energy = 0.65
	_orient_light(_fill_light, Vector3(0.45, -0.5, -0.4))
	_sun_light.shadow_enabled = false

	feedback.connect(_hud.on_feedback)
	_build_touch_ui()

	_session = HomeRunSession.new(_shot_seed if _shot_seed >= 0 else randi())

# On-screen controls for touch/web (hidden on desktop): move left/right + a big
# SWING button (which also starts the round and restarts once it's over).
func _build_touch_ui() -> void:
	if not (DisplayServer.is_touchscreen_available() or OS.has_feature("web")):
		return
	var layer := CanvasLayer.new()
	layer.layer = 2
	add_child(layer)
	var left := _touch_button("<", 0.0, 24, 40, 104, 104)
	left.button_down.connect(func() -> void: _touch_left = true)
	left.button_up.connect(func() -> void: _touch_left = false)
	layer.add_child(left)
	var right := _touch_button(">", 0.0, 140, 40, 104, 104)
	right.button_down.connect(func() -> void: _touch_right = true)
	right.button_up.connect(func() -> void: _touch_right = false)
	layer.add_child(right)
	var swing := _touch_button("SWING", 1.0, -236, 48, 212, 128)
	swing.button_down.connect(func() -> void: _touch_swing = true)
	layer.add_child(swing)

func _touch_button(label: String, anchor_x: float, left_off: float, bottom_off: float, w: float, h: float) -> Button:
	var b := Button.new()
	b.text = label
	b.focus_mode = Control.FOCUS_NONE
	b.anchor_left = anchor_x
	b.anchor_right = anchor_x
	b.anchor_top = 1.0
	b.anchor_bottom = 1.0
	b.offset_left = left_off
	b.offset_right = left_off + w
	b.offset_top = -bottom_off - h
	b.offset_bottom = -bottom_off
	b.modulate = Color(1, 1, 1, 0.7)
	b.add_theme_font_size_override("font_size", 30)
	return b

func _build_meshes() -> void:
	var box := BoxMesh.new()
	box.size = Vector3.ONE
	var sphere := SphereMesh.new()
	sphere.radius = 0.5
	sphere.height = 1.0
	sphere.radial_segments = 18
	sphere.rings = 9
	var cyl := CylinderMesh.new()
	cyl.top_radius = 0.5
	cyl.bottom_radius = 0.5
	cyl.height = 1.0
	cyl.radial_segments = 20
	_meshes = {"box": box, "sphere": sphere, "cylinder": cyl}

func _register_input() -> void:
	_add_action("move_left", [KEY_A, KEY_LEFT])
	_add_action("move_right", [KEY_D, KEY_RIGHT])
	_add_action("swing", [KEY_SPACE])
	_add_action("restart", [KEY_ENTER])
	_add_action("toggle_backend", [KEY_B])

func _add_action(action: String, keys: Array) -> void:
	if InputMap.has_action(action):
		return
	InputMap.add_action(action)
	for k in keys:
		var e := InputEventKey.new()
		e.physical_keycode = k
		InputMap.action_add_event(action, e)

func _orient_light(light: DirectionalLight3D, direction: Vector3) -> void:
	var dir := direction.normalized()
	var up := Vector3.UP if absf(dir.dot(Vector3.UP)) < 0.99 else Vector3.FORWARD
	light.look_at_from_position(Vector3.ZERO, dir, up)

# ── fixed-step loop (60 Hz physics tick) ──
func _physics_process(_delta: float) -> void:
	_frame += 1
	if Input.is_action_just_pressed("toggle_backend"):
		_canvas_mode = not _canvas_mode
	var swing_edge := Input.is_action_just_pressed("swing") or _touch_swing
	_touch_swing = false
	var restart_edge := Input.is_action_just_pressed("restart")
	var axis := Input.get_axis("move_left", "move_right") + (1.0 if _touch_right else 0.0) - (1.0 if _touch_left else 0.0)
	var move_x := -clampf(axis, -1.0, 1.0)
	if _shot_at >= 0:
		swing_edge = _frame == _shot_swing_at
		restart_edge = _frame == 2

	var prev_state := _session.current_swing().state
	_session.advance(move_x, swing_edge, swing_edge or restart_edge)
	for e in _session.tick_events():
		feedback.emit(e.kind, e.text, e.big)
		_audio.play(e.kind, e.big)
	if _session.current_swing().state == "ready" and prev_state != "ready":
		_audio.ready_click()

	_render()

	if _shot_at >= 0 and _frame >= _shot_at:
		_capture_and_quit()

func _render() -> void:
	var view := _session.view()
	var sun := SunState.compute(SunState.START_MS + float(Time.get_ticks_msec()))

	_camera.fov = rad_to_deg(view.camera_fov_y)
	if view.camera_pos.distance_to(view.camera_target) > 1e-4:
		_camera.look_at_from_position(view.camera_pos, view.camera_target, Vector3.UP)
	else:
		_camera.position = view.camera_pos

	_sun_light.light_color = sun.color
	_sun_light.light_energy = sun.energy
	_orient_light(_sun_light, sun.direction)

	_batter.pose(view, sun)
	_machine.pose(view, sun)
	_ball.pose(view, sun)
	var celebration := 1.8 if view.cinematic_phase == "celebration" else 1.0
	for i in range(_fielders.size()):
		_fielders[i].pose(view.fielders[i], i, view.tick, celebration, sun)

	_hud.update(_session, view)

	# Backend switch: real 3D (Camera3D renders) vs the software canvas rasterizer.
	_camera.cull_mask = 0 if _canvas_mode else 0xFFFFF
	_canvas.enabled = _canvas_mode
	if _canvas_mode:
		_canvas.set_frame(sun.direction, sun.energy)
	_canvas.queue_redraw()

# ── screenshot affordance ──
func _parse_shot_args() -> void:
	var args := OS.get_cmdline_user_args()
	var i := args.find("shot")
	if i < 0:
		return
	_shot_at = int(args[i + 1]) if args.size() > i + 1 else 120
	if args.size() > i + 2:
		_shot_out = args[i + 2]
	_shot_seed = int(args[i + 3]) if args.size() > i + 3 else 1
	_shot_swing_at = int(args[i + 4]) if args.size() > i + 4 else -1

func _capture_and_quit() -> void:
	_shot_at = -1
	await RenderingServer.frame_post_draw
	get_viewport().get_texture().get_image().save_png(_shot_out)
	get_tree().quit()
