# game.gd — the Godot host: the platform edge that owns the loop, the GPU scene,
# input, and audio, driving the pure port. It mirrors the original harness.ts +
# game.ts wiring: a 60 Hz fixed-step accumulator folds input into an Intent and
# advances the HomeRunSession; each frame the session's view() is fed to
# HRView.scene_of and RECONCILED into MeshInstance3D nodes (spawn/re-pose/despawn),
# the camera + two directional lights are re-posed, the DOM-style HUD is redrawn,
# and this tick's feedback events are synthesized into WebAudio-style tones.
extends Node3D

const HRC = preload("res://scripts/constants.gd")
const HRView = preload("res://scripts/view.gd")
const HRCine = preload("res://scripts/cinematic_constants.gd")
const HRMaterials = preload("res://scripts/materials.gd")
const HomeRunSession = preload("res://scripts/session.gd")

const TICK := 1.0 / 60.0
const MAX_TICKS_PER_FRAME := 6

var _session: HomeRunSession
var _materials: Dictionary
var _box_mesh: BoxMesh
var _sphere_mesh: SphereMesh
var _cyl_mesh: CylinderMesh
var _field: Node3D
var _nodes: Dictionary = {}
var _camera: Camera3D
var _sun_light: DirectionalLight3D
var _fill_light: DirectionalLight3D
var _accum := 0.0

# Screenshot affordance: run with `-- shot <frame> [out.png] [seed] [swingAt]` to
# capture one deterministic frame and quit (used to compare against the reference).
var _shot_at := -1
var _shot_out := "user://shot.png"
var _shot_swing_at := -1
var _shot_seed := -1
var _frame_count := 0

# input edge tracking
var _prev_space := false
var _prev_enter := false
var _prev_swing_state := "ready"

# audio pool
var _audio_players: Array[AudioStreamPlayer] = []
var _audio_next := 0

# HUD nodes
var _hud: Control
var _lbl_score: Label
var _lbl_pitch: Label
var _lbl_hr: Label
var _lbl_streak: Label
var _lbl_speed: Label
var _lbl_best: Label
var _lbl_message: Label
var _lbl_ready: Label
var _lbl_over: Label
var _meter_bg: ColorRect
var _meter_fill: ColorRect
var _letterbox_top: ColorRect
var _letterbox_bottom: ColorRect

func _ready() -> void:
	randomize()
	_parse_shot_args()
	_materials = HRMaterials.build()
	_build_meshes()
	_build_environment()
	_build_camera_and_lights()
	_field = Node3D.new()
	_field.name = "Field"
	add_child(_field)
	_build_audio_pool()
	_build_hud()
	_session = HomeRunSession.new(_shot_seed if _shot_seed >= 0 else randi())

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

func _build_meshes() -> void:
	_box_mesh = BoxMesh.new()
	_box_mesh.size = Vector3(1, 1, 1)
	_sphere_mesh = SphereMesh.new()
	_sphere_mesh.radius = 0.5
	_sphere_mesh.height = 1.0
	_sphere_mesh.radial_segments = 18
	_sphere_mesh.rings = 9
	_cyl_mesh = CylinderMesh.new()
	_cyl_mesh.top_radius = 0.5
	_cyl_mesh.bottom_radius = 0.5
	_cyl_mesh.height = 1.0
	_cyl_mesh.radial_segments = 20

func _mesh_for(kind: String) -> Mesh:
	match kind:
		"box":
			return _box_mesh
		"sphere":
			return _sphere_mesh
		_:
			return _cyl_mesh

func _build_environment() -> void:
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.62, 0.72, 0.95)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(0.48, 0.56, 0.74)
	env.ambient_light_energy = 0.3
	env.tonemap_mode = Environment.TONE_MAPPER_LINEAR
	# The original's flat Lambert reads darker than Godot's gamma-brightened midtones;
	# a small brightness/contrast trim brings the toy palette back toward the dusk mood.
	env.adjustment_enabled = true
	env.adjustment_brightness = 0.86
	env.adjustment_contrast = 1.08
	var we := WorldEnvironment.new()
	we.environment = env
	add_child(we)

func _build_camera_and_lights() -> void:
	_camera = Camera3D.new()
	_camera.fov = rad_to_deg(HRC.CAMERA_FOV_Y)
	_camera.near = HRC.CAMERA_NEAR
	_camera.far = HRC.CAMERA_FAR
	_camera.position = HRC.CAMERA_POS
	_camera.look_at_from_position(HRC.CAMERA_POS, HRC.CAMERA_TARGET, Vector3.UP)
	add_child(_camera)

	_sun_light = DirectionalLight3D.new()
	_sun_light.shadow_enabled = false
	add_child(_sun_light)
	_fill_light = DirectionalLight3D.new()
	_fill_light.shadow_enabled = false
	add_child(_fill_light)

func _orient_light(light: DirectionalLight3D, direction: Vector3) -> void:
	var dir := direction.normalized()
	var up := Vector3.UP if absf(dir.dot(Vector3.UP)) < 0.99 else Vector3.FORWARD
	# A directional light shines along its local -Z; look_at points -Z toward the
	# target, so aiming at (origin + dir) makes -Z == dir.
	light.look_at_from_position(Vector3.ZERO, dir, up)

# ── per-frame ──
func _process(delta: float) -> void:
	_frame_count += 1
	_accum += delta
	var space := _key(KEY_SPACE)
	var enter := _key(KEY_ENTER)
	var left := _key(KEY_A) or _key(KEY_LEFT)
	var right := _key(KEY_D) or _key(KEY_RIGHT)
	# Scripted deterministic input while capturing a shot.
	if _shot_at >= 0:
		enter = _frame_count == 2
		space = _frame_count == _shot_swing_at
	var swing_edge := space and not _prev_space
	var restart_edge := enter and not _prev_enter
	_prev_space = space
	_prev_enter = enter
	var move_x := -(float(1 if right else 0) - float(1 if left else 0))

	var first := true
	var ticks := 0
	while _accum >= TICK and ticks < MAX_TICKS_PER_FRAME:
		_accum -= TICK
		ticks += 1
		var intent := {
			"moveX": move_x,
			"swing": swing_edge and first,
			"start": (swing_edge or restart_edge) and first,
		}
		var prev_ready: String = _session.swing_state().state
		_session.advance(intent)
		_play_tick_audio(prev_ready)
		first = false

	_render()

	if _shot_at >= 0 and _frame_count >= _shot_at:
		_capture_and_quit()

func _capture_and_quit() -> void:
	_shot_at = -1
	await RenderingServer.frame_post_draw
	var img := get_viewport().get_texture().get_image()
	img.save_png(_shot_out)
	get_tree().quit()

func _key(code: int) -> bool:
	return Input.is_physical_key_pressed(code)

func _render() -> void:
	var view := _session.view()
	var now_ms := HRView.SUN_START_MS + float(Time.get_ticks_msec())
	var scene := HRView.scene_of(view, now_ms)
	_reconcile(scene.instances)
	_update_camera(scene.camera)
	_update_lights(scene.lights)
	_update_hud(view)

func _reconcile(instances: Array) -> void:
	var seen := {}
	for inst in instances:
		var key: String = inst.key
		var mi: MeshInstance3D = _nodes.get(key)
		if mi == null:
			mi = MeshInstance3D.new()
			mi.mesh = _mesh_for(inst.mesh)
			mi.material_override = _materials[inst.material]
			mi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
			_field.add_child(mi)
			_nodes[key] = mi
		mi.position = inst.position
		mi.quaternion = inst.rotation
		mi.scale = inst.scale
		seen[key] = true
	for key in _nodes.keys():
		if not seen.has(key):
			_nodes[key].queue_free()
			_nodes.erase(key)

func _update_camera(cam: Dictionary) -> void:
	_camera.fov = rad_to_deg(cam.fovY)
	_camera.near = cam.near
	_camera.far = cam.far
	var pos: Vector3 = cam.position
	var target: Vector3 = cam.target
	if pos.distance_to(target) > 1e-4:
		_camera.look_at_from_position(pos, target, Vector3.UP)
	else:
		_camera.position = pos

func _update_lights(lights: Array) -> void:
	for l in lights:
		if l.key == "sun":
			_sun_light.light_color = l.color
			_sun_light.light_energy = l.intensity
			_orient_light(_sun_light, l.direction)
		else:
			_fill_light.light_color = l.color
			_fill_light.light_energy = l.intensity
			_orient_light(_fill_light, l.direction)

# ── audio ──
func _build_audio_pool() -> void:
	for i in range(12):
		var p := AudioStreamPlayer.new()
		add_child(p)
		_audio_players.append(p)

func _free_player() -> AudioStreamPlayer:
	var p := _audio_players[_audio_next]
	_audio_next = (_audio_next + 1) % _audio_players.size()
	return p

func _make_wav(freq: float, dur: float, vol: float, wave: String) -> AudioStreamWAV:
	var sr := 22050
	var n := int(dur * sr)
	var bytes := PackedByteArray()
	bytes.resize(n * 2)
	var two_pi := PI * 2.0
	for i in range(n):
		var t := float(i) / sr
		var phase := freq * t
		var frac: float = phase - floor(phase)
		var s := 0.0
		match wave:
			"square":
				s = 1.0 if frac < 0.5 else -1.0
			"triangle":
				s = 4.0 * absf(frac - 0.5) - 1.0
			"sawtooth":
				s = 2.0 * frac - 1.0
			_:
				s = sin(two_pi * phase)
		# short attack/release envelope to avoid clicks
		var env := minf(1.0, minf(float(i) / 220.0, float(n - i) / 220.0))
		var v := int(clampf(s * vol * env, -1.0, 1.0) * 32767.0)
		bytes.encode_s16(i * 2, v)
	var w := AudioStreamWAV.new()
	w.format = AudioStreamWAV.FORMAT_16_BITS
	w.mix_rate = sr
	w.stereo = false
	w.data = bytes
	return w

func _play_tone(freq: float, dur: float, vol: float, wave: String, delay: float) -> void:
	if delay > 0.0:
		await get_tree().create_timer(delay).timeout
	var p := _free_player()
	p.stream = _make_wav(freq, dur, vol, wave)
	p.play()

func _tone_for(kind: String, big: bool) -> Array:
	match kind:
		"release":
			return [{"duration": 0.05, "freq": 660, "volume": 0.12, "wave": "square", "delay": 0.0}]
		"contact":
			return [
				{"duration": 0.07, "freq": 220 if big else 180, "volume": 0.5, "wave": "square", "delay": 0.0},
				{"duration": 0.05, "freq": 1400 if big else 900, "volume": 0.25, "wave": "triangle", "delay": 0.0},
			]
		"homer":
			var out := []
			var arp := [523, 659, 784, 1047]
			for i in range(arp.size()):
				out.append({"delay": i * 0.05, "duration": 0.16, "freq": arp[i], "volume": 0.3, "wave": "triangle"})
			return out
		"clean":
			return [{"duration": 0.12, "freq": 587, "volume": 0.22, "wave": "triangle", "delay": 0.0}]
		"miss":
			return [{"duration": 0.12, "freq": 110, "volume": 0.18, "wave": "sawtooth", "delay": 0.0}]
		"ball":
			return [{"duration": 0.1, "freq": 300, "volume": 0.12, "wave": "sine", "delay": 0.0}]
		"foul":
			return [{"duration": 0.08, "freq": 240, "volume": 0.18, "wave": "square", "delay": 0.0}]
		"caught", "fielded", "weak", "grounder", "popup":
			return [{"duration": 0.08, "freq": 160, "volume": 0.2, "wave": "sine", "delay": 0.0}]
		"cinematicAnticipation":
			return [
				{"duration": 0.1, "freq": 200, "volume": 0.14, "wave": "sine", "delay": 0.0},
				{"delay": 0.06, "duration": 0.14, "freq": 320, "volume": 0.16, "wave": "sine"},
			]
		"crowdErupt":
			return [
				{"duration": 0.22, "freq": 140, "volume": 0.2, "wave": "sawtooth", "delay": 0.0},
				{"delay": 0.05, "duration": 0.18, "freq": 210, "volume": 0.16, "wave": "triangle"},
			]
		_:
			return []

func _play_tick_audio(prev_swing_state: String) -> void:
	for event in _session.tick_events():
		for tone in _tone_for(event.kind, event.big):
			_play_tone(tone.freq, tone.duration, tone.volume, tone.wave, tone.get("delay", 0.0))
	var next_state: String = _session.swing_state().state
	if next_state == "ready" and prev_swing_state != "ready":
		_play_tone(880, 0.05, 0.14, "sine", 0.0)

# ── HUD ──
func _mk_label(size: int, color: Color) -> Label:
	var l := Label.new()
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", color)
	return l

func _build_hud() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)
	_hud = Control.new()
	_hud.set_anchors_preset(Control.PRESET_FULL_RECT)
	_hud.mouse_filter = Control.MOUSE_FILTER_IGNORE
	layer.add_child(_hud)

	# Letterbox bars (positioned/sized explicitly each frame in _update_hud).
	_letterbox_top = ColorRect.new()
	_letterbox_top.color = Color(0, 0, 0)
	_letterbox_top.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_hud.add_child(_letterbox_top)
	_letterbox_bottom = ColorRect.new()
	_letterbox_bottom.color = Color(0, 0, 0)
	_letterbox_bottom.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_hud.add_child(_letterbox_bottom)

	# Bottom stat bar.
	var bar := HBoxContainer.new()
	bar.add_theme_constant_override("separation", 26)
	bar.set_anchors_preset(Control.PRESET_CENTER_BOTTOM)
	bar.position = Vector2(0, -46)
	bar.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_hud.add_child(bar)
	var stat := Color(0.86, 0.9, 1.0)
	_lbl_score = _mk_label(20, Color(1, 0.85, 0.2))
	_lbl_pitch = _mk_label(20, stat)
	_lbl_hr = _mk_label(20, stat)
	_lbl_streak = _mk_label(20, Color(1, 0.7, 0.3))
	_lbl_speed = _mk_label(18, stat)
	_lbl_best = _mk_label(18, stat)
	bar.add_child(_lbl_score)
	bar.add_child(_lbl_pitch)
	bar.add_child(_lbl_hr)
	bar.add_child(_lbl_streak)
	bar.add_child(_lbl_speed)
	bar.add_child(_lbl_best)

	# Center outcome message.
	_lbl_message = _mk_label(64, Color(1, 0.95, 0.55))
	_lbl_message.set_anchors_preset(Control.PRESET_CENTER)
	_lbl_message.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_lbl_message.grow_vertical = Control.GROW_DIRECTION_BOTH
	_lbl_message.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_hud.add_child(_lbl_message)

	# Ready / over overlays.
	_lbl_ready = _mk_label(34, Color(0.9, 0.95, 1))
	_lbl_ready.text = "PRESS SPACE TO BAT"
	_lbl_ready.set_anchors_preset(Control.PRESET_CENTER)
	_lbl_ready.position = Vector2(-160, 120)
	_hud.add_child(_lbl_ready)
	_lbl_over = _mk_label(40, Color(1, 0.85, 0.3))
	_lbl_over.set_anchors_preset(Control.PRESET_CENTER)
	_lbl_over.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_lbl_over.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_hud.add_child(_lbl_over)

	# Ready meter (the swing cooldown).
	_meter_bg = ColorRect.new()
	_meter_bg.color = Color(0.1, 0.13, 0.2, 0.7)
	_meter_bg.set_anchors_preset(Control.PRESET_CENTER_BOTTOM)
	_meter_bg.position = Vector2(-90, -84)
	_meter_bg.size = Vector2(180, 10)
	_meter_bg.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_hud.add_child(_meter_bg)
	_meter_fill = ColorRect.new()
	_meter_fill.color = Color(1, 0.8, 0.25)
	_meter_fill.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_meter_bg.add_child(_meter_fill)

func _update_hud(view: Dictionary) -> void:
	_lbl_score.text = "SCORE %d" % _session.score()
	_lbl_pitch.text = "PITCH %d/%d" % [_session.pitch_number(), HRC.PITCHES_PER_ROUND]
	_lbl_hr.text = "HR %d" % _session.homers()
	_lbl_streak.text = "%dx" % _session.streak_multiplier()
	_lbl_speed.text = ("%d MPH %s" % [_session.last_mph(), _session.last_pitch_name()]) if _session.last_mph() > 0 else "—"
	_lbl_best.text = ("BEST %dm" % _session.best_distance()) if _session.best_distance() > 0 else "BEST —"

	var phase: String = view.phase
	var ready_now: bool = _session.swing_state().state == "ready"
	var live := phase != "ready" and phase != "over"
	_meter_bg.visible = live and not ready_now
	_meter_fill.size = Vector2(180.0 * _session.swing_state().readiness, 10)
	_lbl_ready.visible = phase == "ready"

	var over := phase == "over"
	_lbl_over.visible = over
	if over:
		_lbl_over.text = "ROUND OVER\nSCORE %d   HR %d   BEST %s" % [_session.score(), _session.homers(), ("%dm" % _session.best_distance()) if _session.best_distance() > 0 else "—"]

	# Latest meaningful outcome text as the center message.
	var msg := ""
	var events := _session.tick_events()
	for e in events:
		if e.text.length() > 0 and e.kind != "release":
			msg = e.text
	if msg.length() > 0:
		_lbl_message.text = msg
		_lbl_message.modulate.a = 1.0
	elif _lbl_message.modulate.a > 0.0:
		_lbl_message.modulate.a = maxf(0.0, _lbl_message.modulate.a - 0.02)

	# Letterbox + HUD dim.
	var bar_frac: float = view.letterboxProgress * HRCine.TUNING.letterboxScreenFraction
	var vp := get_viewport().get_visible_rect().size
	var bar_h := vp.y * bar_frac
	_letterbox_top.position = Vector2.ZERO
	_letterbox_top.size = Vector2(vp.x, bar_h)
	_letterbox_bottom.position = Vector2(0, vp.y - bar_h)
	_letterbox_bottom.size = Vector2(vp.x, bar_h)
	_hud.modulate.a = 1.0 if view.hudVisible else 0.25
