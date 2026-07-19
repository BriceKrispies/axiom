# Canvas2DRenderer — a hand-rolled software "3D attempt", the analogue of Axiom's
# canvas2d backend. It reads the same 3D scene the GPU pipeline draws, projects each
# box/cylinder/sphere to screen itself (view+projection in GDScript, Sutherland-
# Hodgman near-plane clip), and fills flat-shaded polygons on Godot's 2D canvas.
#
# Performance (this is CPU rasterization in GDScript, on phones too), three levers:
#   * BACKFACE CULLING — only front faces of each convex box/cylinder are emitted,
#     halving the work and removing intra-object overlap (so no per-face sort needed).
#   * STATIC CACHE — the stadium is fixed geometry; its projected triangles are built
#     once and reused until the camera actually moves.
#   * BATCHING — every face is triangulated into ONE big triangle array drawn in a
#     single canvas_item_add_triangle_array call, instead of thousands of draw calls.
# Instances are painter-sorted by their FARTHEST vertex so the huge ground/backdrop
# polys draw behind everything.
extends Node2D

const HRC = preload("res://scripts/constants.gd")

var camera: Camera3D
var static_root: Node3D
var actors_root: Node3D
var enabled := false
var sky := Color(0.62, 0.72, 0.95)

const W_EPS := 0.05
const AMBIENT := 0.42
const CYL_SEG := 7
const SPH_SEG := 9

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
var _energy := 0.8
var _cam_pos := Vector3.ZERO
var _cam_right := Vector3(1, 0, 0)

# static cache (rebuilt only when the camera moves)
var _static_pts := PackedVector2Array()
var _static_cols := PackedColorArray()
var _have_static := false
var _last_view: Transform3D
var _last_proj: Projection

func setup(cam: Camera3D, static_node: Node3D, actors_node: Node3D) -> void:
	camera = cam
	static_root = static_node
	actors_root = actors_node

func set_frame(sun_dir: Vector3, energy: float) -> void:
	_light = (-sun_dir).normalized()
	_energy = energy

func _draw() -> void:
	if not enabled or camera == null:
		return
	_vp = get_viewport_rect().size
	_proj = camera.get_camera_projection()
	_view = camera.get_camera_transform().affine_inverse()
	_cam_pos = camera.global_position
	_cam_right = camera.global_transform.basis.x.normalized()
	draw_rect(Rect2(Vector2.ZERO, _vp), sky)

	if not _have_static or _view != _last_view or _proj != _last_proj:
		_last_view = _view
		_last_proj = _proj
		var s_insts: Array = []
		_collect(static_root, s_insts)
		var packed := _pack(s_insts)
		_static_pts = packed[0]
		_static_cols = packed[1]
		_have_static = true
	_blit(_static_pts, _static_cols)

	var d_insts: Array = []
	_collect(actors_root, d_insts)
	var dp := _pack(d_insts)
	_blit(dp[0], dp[1])

func _blit(pts: PackedVector2Array, cols: PackedColorArray) -> void:
	var n := pts.size()
	if n < 3:
		return
	var idx := PackedInt32Array()
	idx.resize(n)
	for i in range(n):
		idx[i] = i
	RenderingServer.canvas_item_add_triangle_array(get_canvas_item(), idx, pts, cols)

# Sort instances back-to-front and fan-triangulate every face into one soup.
func _pack(insts: Array) -> Array:
	insts.sort_custom(func(a, b): return a[0] > b[0])
	var pts := PackedVector2Array()
	var cols := PackedColorArray()
	for inst in insts:
		for face in inst[1]:
			var poly: PackedVector2Array = face[0]
			var col: Color = face[1]
			for i in range(1, poly.size() - 1):
				pts.append(poly[0])
				pts.append(poly[i])
				pts.append(poly[i + 1])
				cols.append(col)
				cols.append(col)
				cols.append(col)
	return [pts, cols]

func _collect(node: Node, out: Array) -> void:
	for child in node.get_children():
		if child is MeshInstance3D:
			if child.visible:
				_emit(child.mesh, child.global_transform, child.material_override, out)
		elif child is MultiMeshInstance3D and child.visible:
			var mm: MultiMesh = child.multimesh
			var base: Transform3D = child.global_transform
			for i in range(mm.instance_count):
				_emit(mm.mesh, base * mm.get_instance_transform(i), child.material_override, out)
		if child.get_child_count() > 0:
			_collect(child, out)

func _emit(mesh: Mesh, xform: Transform3D, mat: Material, out: Array) -> void:
	var albedo := Color(1, 0, 1)
	var emission := Color(0, 0, 0)
	if mat is StandardMaterial3D:
		albedo = mat.albedo_color
		if mat.emission_enabled:
			emission = mat.emission * mat.emission_energy_multiplier
	if mesh is BoxMesh:
		_box(xform, albedo, emission, out)
	elif mesh is CylinderMesh:
		_cyl(xform, albedo, emission, out)
	elif mesh is SphereMesh:
		_sphere(xform, albedo, emission, out)

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

# Clip a clip-space polygon, project to screen, drop degenerate (edge-on) polys.
func _project_face(poly4: Array) -> PackedVector2Array:
	var clipped := _clip_w(poly4)
	if clipped.size() < 3:
		return PackedVector2Array()
	var pts := PackedVector2Array()
	for v: Vector4 in clipped:
		pts.append(_screen(v))
	var area := 0.0
	var m := pts.size()
	for i in range(m):
		area += pts[i].x * pts[(i + 1) % m].y - pts[(i + 1) % m].x * pts[i].y
	if absf(area) < 1.0:
		return PackedVector2Array()
	return pts

func _box(xform: Transform3D, albedo: Color, emission: Color, out: Array) -> void:
	var cc: Array[Vector4] = []
	for c in BOX_CORNERS:
		cc.append(_clip4(xform * c))
	var faces: Array = []
	var maxw := 0.0
	for fi in range(6):
		var wn := (xform.basis * BOX_NORMALS[fi]).normalized()
		var fc := xform * (BOX_NORMALS[fi] * 0.5)
		if wn.dot(fc - _cam_pos) >= 0.0:  # backface: skip
			continue
		var face: Array = BOX_FACES[fi]
		var poly4 := [cc[face[0]], cc[face[1]], cc[face[2]], cc[face[3]]]
		var scr := _project_face(poly4)
		if scr.size() < 3:
			continue
		var shade := AMBIENT + _energy * maxf(0.0, wn.dot(_light))
		faces.append([scr, _shade(albedo, emission, shade)])
		for v: Vector4 in poly4:
			maxw = maxf(maxw, v.w)
	if faces.size() > 0:
		out.append([maxw, faces])

func _cyl(xform: Transform3D, albedo: Color, emission: Color, out: Array) -> void:
	var top: Array[Vector4] = []
	var bot: Array[Vector4] = []
	for s in range(CYL_SEG):
		var ang := TAU * s / CYL_SEG
		var lx := 0.5 * cos(ang)
		var lz := 0.5 * sin(ang)
		top.append(_clip4(xform * Vector3(lx, 0.5, lz)))
		bot.append(_clip4(xform * Vector3(lx, -0.5, lz)))
	var faces: Array = []
	var maxw := 0.0
	for s in range(CYL_SEG):
		var s2 := (s + 1) % CYL_SEG
		var ang := TAU * (s + 0.5) / CYL_SEG
		var wn := (xform.basis * Vector3(cos(ang), 0, sin(ang))).normalized()
		var fc := xform * Vector3(cos(ang) * 0.5, 0, sin(ang) * 0.5)
		if wn.dot(fc - _cam_pos) >= 0.0:
			continue
		var scr := _project_face([top[s], top[s2], bot[s2], bot[s]])
		if scr.size() < 3:
			continue
		faces.append([scr, _shade(albedo, emission, AMBIENT + _energy * maxf(0.0, wn.dot(_light)))])
		maxw = maxf(maxw, maxf(top[s].w, bot[s].w))
	_cap(xform, Vector3(0, 1, 0), top, albedo, emission, faces)
	_cap(xform, Vector3(0, -1, 0), bot, albedo, emission, faces)
	for v: Vector4 in top:
		maxw = maxf(maxw, v.w)
	if faces.size() > 0:
		out.append([maxw, faces])

func _cap(xform: Transform3D, local_n: Vector3, ring: Array, albedo: Color, emission: Color, faces: Array) -> void:
	var wn := (xform.basis * local_n).normalized()
	var fc := xform * (local_n * 0.5)
	if wn.dot(fc - _cam_pos) >= 0.0:
		return
	var poly4: Array = []
	var order := range(ring.size()) if local_n.y > 0.0 else range(ring.size() - 1, -1, -1)
	for i in order:
		poly4.append(ring[i])
	var scr := _project_face(poly4)
	if scr.size() >= 3:
		faces.append([scr, _shade(albedo, emission, AMBIENT + _energy * maxf(0.0, wn.dot(_light)))])

func _sphere(xform: Transform3D, albedo: Color, emission: Color, out: Array) -> void:
	var c4 := _clip4(xform.origin)
	if c4.w < W_EPS:
		return
	var rworld := 0.5 * xform.basis.get_scale().x
	var e4 := _clip4(xform.origin + _cam_right * rworld)
	if e4.w < W_EPS:
		return
	var center := _screen(c4)
	var rad := center.distance_to(_screen(e4))
	if rad < 0.5:
		return
	var poly := PackedVector2Array()
	for s in range(SPH_SEG):
		var ang := TAU * s / SPH_SEG
		poly.append(center + Vector2(cos(ang), sin(ang)) * rad)
	out.append([c4.w, [[poly, _shade(albedo, emission, AMBIENT + _energy * 0.55)]]])
