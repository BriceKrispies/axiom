# Canvas2DRenderer — a hand-rolled software "3D attempt", the direct analogue of
# Axiom's canvas2d backend. It reads the SAME scene the 3D pipeline draws (every
# MeshInstance3D / MultiMeshInstance3D under Field), projects each box/cylinder/
# sphere to screen space itself (view+projection math in GDScript, with near-plane
# clipping), painter-sorts the faces by depth, and fills them as flat-shaded 2D
# polygons/circles via Godot's CanvasItem API. The 3D *math* is software; only the
# final 2D fills are accelerated — exactly like the original's canvas2d path, which
# bypasses WebGL rather than routing through it.
#
# The host blanks the real 3D (Camera3D.cull_mask = 0) and shows this instead.
extends Node2D

var camera: Camera3D
var field: Node3D
var enabled := false
var sky := Color(0.62, 0.72, 0.95)

const W_EPS := 0.05
const AMBIENT := 0.42
const CYL_SEG := 8

const BOX_CORNERS: Array[Vector3] = [
	Vector3(-0.5, -0.5, -0.5), Vector3(0.5, -0.5, -0.5), Vector3(0.5, 0.5, -0.5), Vector3(-0.5, 0.5, -0.5),
	Vector3(-0.5, -0.5, 0.5), Vector3(0.5, -0.5, 0.5), Vector3(0.5, 0.5, 0.5), Vector3(-0.5, 0.5, 0.5),
]
const BOX_FACES := [[4, 5, 6, 7], [1, 0, 3, 2], [5, 1, 2, 6], [0, 4, 7, 3], [3, 7, 6, 2], [0, 1, 5, 4]]
const BOX_NORMALS: Array[Vector3] = [
	Vector3(0, 0, 1), Vector3(0, 0, -1), Vector3(1, 0, 0), Vector3(-1, 0, 0), Vector3(0, 1, 0), Vector3(0, -1, 0),
]

var _proj: Projection
var _view: Transform3D
var _vp: Vector2
var _light := Vector3(0, 1, 0)
var _cam_right := Vector3(1, 0, 0)
var _energy := 0.8

func setup(cam: Camera3D, field_node: Node3D) -> void:
	camera = cam
	field = field_node

func set_frame(sun_dir: Vector3, energy: float) -> void:
	_light = (-sun_dir).normalized()
	_energy = energy

func _draw() -> void:
	if not enabled or camera == null:
		return
	_vp = get_viewport_rect().size
	draw_rect(Rect2(Vector2.ZERO, _vp), sky)
	_proj = camera.get_camera_projection()
	_view = camera.get_camera_transform().affine_inverse()
	_cam_right = camera.global_transform.basis.x.normalized()
	var prims: Array = []
	_gather(field, prims)
	prims.sort_custom(func(a, b): return a[0] > b[0])
	for p in prims:
		if p[1] == 0:
			draw_colored_polygon(p[2], p[3])
		else:
			draw_circle(p[2], p[3], p[4])

func _gather(node: Node, prims: Array) -> void:
	for child in node.get_children():
		if child is MeshInstance3D:
			if child.visible:
				_emit(child.mesh, child.global_transform, child.material_override, prims)
		elif child is MultiMeshInstance3D and child.visible:
			var mm: MultiMesh = child.multimesh
			var base: Transform3D = child.global_transform
			for i in range(mm.instance_count):
				_emit(mm.mesh, base * mm.get_instance_transform(i), child.material_override, prims)
		if child.get_child_count() > 0:
			_gather(child, prims)

func _emit(mesh: Mesh, xform: Transform3D, mat: Material, prims: Array) -> void:
	var albedo := Color(1, 0, 1)
	var emission := Color(0, 0, 0)
	if mat is StandardMaterial3D:
		albedo = mat.albedo_color
		if mat.emission_enabled:
			emission = mat.emission * mat.emission_energy_multiplier
	if mesh is BoxMesh:
		_box(xform, albedo, emission, prims)
	elif mesh is CylinderMesh:
		_cyl(xform, albedo, emission, prims)
	elif mesh is SphereMesh:
		_sphere(xform, albedo, emission, prims)

# Flat Lambert with emission as a per-channel FLOOR (glow materials never go dark,
# but don't blow out to white the way additive emission would in this LDR fill).
func _shade(albedo: Color, emission: Color, shade: float) -> Color:
	return Color(
		clampf(maxf(albedo.r * shade, emission.r), 0.0, 1.0),
		clampf(maxf(albedo.g * shade, emission.g), 0.0, 1.0),
		clampf(maxf(albedo.b * shade, emission.b), 0.0, 1.0),
		albedo.a)

func _clip4(world: Vector3) -> Vector4:
	var c := _view * world
	return _proj * Vector4(c.x, c.y, c.z, 1.0)

func _screen(v: Vector4) -> Vector2:
	return Vector2((v.x / v.w * 0.5 + 0.5) * _vp.x, (0.5 - v.y / v.w * 0.5) * _vp.y)

# Sutherland-Hodgman clip of a Vector4 polygon against the w >= W_EPS half-space
# (drops geometry behind the camera without the near-plane coordinate blow-up).
func _clip_w(poly: Array) -> Array:
	var out: Array = []
	var n := poly.size()
	for i in range(n):
		var a: Vector4 = poly[i]
		var b: Vector4 = poly[(i + 1) % n]
		var a_in := a.w >= W_EPS
		if a_in:
			out.append(a)
		if a_in != (b.w >= W_EPS):
			out.append(a.lerp(b, (W_EPS - a.w) / (b.w - a.w)))
	return out

func _emit_poly(poly4: Array, col: Color, prims: Array) -> void:
	var clipped := _clip_w(poly4)
	if clipped.size() < 3:
		return
	var pts := PackedVector2Array()
	# Sort by the FARTHEST vertex (max clip-w), not the average: the huge ground/
	# deck/sky polygons span from right in front of the camera to the far outfield,
	# and an average depth makes them sort "near" and paint over the stands + actors.
	# The farthest-point key draws those big floor/backdrop polys first (behind).
	var depth := 0.0
	for v: Vector4 in clipped:
		pts.append(_screen(v))
		depth = maxf(depth, v.w)
	# Skip degenerate polygons (thin edges seen edge-on) so draw_colored_polygon's
	# triangulator never chokes on a zero-area shape.
	var area := 0.0
	var m := pts.size()
	for i in range(m):
		area += pts[i].x * pts[(i + 1) % m].y - pts[(i + 1) % m].x * pts[i].y
	if absf(area) < 1.0:
		return
	prims.append([depth, 0, pts, col, null])

func _box(xform: Transform3D, albedo: Color, emission: Color, prims: Array) -> void:
	var wc: Array[Vector4] = []
	for c in BOX_CORNERS:
		wc.append(_clip4(xform * c))
	for fi in range(6):
		var nrm := (xform.basis * BOX_NORMALS[fi]).normalized()
		var shade := AMBIENT + _energy * maxf(0.0, nrm.dot(_light))
		var face: Array = BOX_FACES[fi]
		_emit_poly([wc[face[0]], wc[face[1]], wc[face[2]], wc[face[3]]], _shade(albedo, emission, shade), prims)

func _cyl(xform: Transform3D, albedo: Color, emission: Color, prims: Array) -> void:
	var top: Array[Vector3] = []
	var bot: Array[Vector3] = []
	for s in range(CYL_SEG):
		var ang := TAU * s / CYL_SEG
		var lx := 0.5 * cos(ang)
		var lz := 0.5 * sin(ang)
		top.append(xform * Vector3(lx, 0.5, lz))
		bot.append(xform * Vector3(lx, -0.5, lz))
	for s in range(CYL_SEG):
		var s2 := (s + 1) % CYL_SEG
		var ang := TAU * (s + 0.5) / CYL_SEG
		var nrm := (xform.basis * Vector3(cos(ang), 0, sin(ang))).normalized()
		var shade := AMBIENT + _energy * maxf(0.0, nrm.dot(_light))
		_emit_poly([_clip4(top[s]), _clip4(top[s2]), _clip4(bot[s2]), _clip4(bot[s])], _shade(albedo, emission, shade), prims)
	var ntop := (xform.basis * Vector3(0, 1, 0)).normalized()
	var top_poly: Array = []
	for s in range(CYL_SEG):
		top_poly.append(_clip4(top[s]))
	_emit_poly(top_poly, _shade(albedo, emission, AMBIENT + _energy * maxf(0.0, ntop.dot(_light))), prims)
	var nbot := (xform.basis * Vector3(0, -1, 0)).normalized()
	var bot_poly: Array = []
	for s in range(CYL_SEG - 1, -1, -1):
		bot_poly.append(_clip4(bot[s]))
	_emit_poly(bot_poly, _shade(albedo, emission, AMBIENT + _energy * maxf(0.0, nbot.dot(_light))), prims)

func _sphere(xform: Transform3D, albedo: Color, emission: Color, prims: Array) -> void:
	var c4 := _clip4(xform.origin)
	if c4.w < W_EPS:
		return
	var rworld := 0.5 * xform.basis.get_scale().x
	var e4 := _clip4(xform.origin + _cam_right * rworld)
	if e4.w < W_EPS:
		return
	var center := _screen(c4)
	prims.append([c4.w, 1, center, center.distance_to(_screen(e4)), _shade(albedo, emission, AMBIENT + _energy * 0.55)])
