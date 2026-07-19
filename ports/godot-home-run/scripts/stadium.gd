# Stadium — the static toy field + grandstand, built ONCE into MultiMeshInstance3D
# nodes (one per mesh x material combination) under the Static root. None of this
# geometry ever moves, so it's baked into MultiMeshes at startup and never touched
# again. Mesh conventions: box = unit cube (scale = extents); sphere = unit diameter
# (scale = 2r); cylinder = unit (radius 0.5, height 1, Y axis).
extends RefCounted

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")

const MIN_EXTENT := 0.01

static var YAW_POS := HRMath.quat_from_euler_xyz(0, PI / 4.0, 0)
static var YAW_NEG := HRMath.quat_from_euler_xyz(0, -PI / 4.0, 0)

static func _xform(pos: Vector3, scale: Vector3, rot: Quaternion) -> Transform3D:
	return Transform3D(Basis(rot) * Basis.from_scale(scale), pos)

static func _add(buckets: Dictionary, mesh: String, material: String, pos: Vector3, scale: Vector3, rot: Quaternion) -> void:
	var key := mesh + "|" + material
	if not buckets.has(key):
		buckets[key] = []
	buckets[key].append(_xform(pos, scale, rot))

static func _box(buckets: Dictionary, mat: String, pos: Vector3, scale: Vector3, rot := Quaternion.IDENTITY) -> void:
	_add(buckets, "box", mat, pos, Vector3(maxf(scale.x, MIN_EXTENT), maxf(scale.y, MIN_EXTENT), maxf(scale.z, MIN_EXTENT)), rot)

static func _cyl(buckets: Dictionary, mat: String, pos: Vector3, scale: Vector3, rot := Quaternion.IDENTITY) -> void:
	_add(buckets, "cylinder", mat, pos, scale, rot)

static func _orb(buckets: Dictionary, mat: String, pos: Vector3, radius: float) -> void:
	_add(buckets, "sphere", mat, pos, Vector3(radius * 2.0, radius * 2.0, radius * 2.0), Quaternion.IDENTITY)

static func _build_ground(b: Dictionary) -> void:
	_box(b, "GroundGreen", Vector3(0, -0.07, 14), Vector3(76, 0.1, 64))
	_box(b, "DeckBrown", Vector3(0, -0.005, -2), Vector3(46, 0.06, 15))
	for side: int in [1, -1]:
		_box(b, "DirtLight", Vector3(side * 2.6, 0.028, -1.8), Vector3(0.35, 0.02, 8))
	for k in range(14):
		var zc := 1.2 + k * 2.4
		var half_w: float = minf(zc + 1.2, HRC.WALL_LINE - zc + 1.2)
		if half_w <= 0.4:
			continue
		_box(b, "GrassLight" if k % 2 == 0 else "GrassDark", Vector3(0, 0.002, zc), Vector3(half_w * 2.0, 0.03, 2.4))
	_box(b, "Dirt", Vector3(0, 0.03, 7.5), Vector3(10.6, 0.05, 10.6), YAW_POS)
	_box(b, "GrassLight", Vector3(0, 0.045, 7.5), Vector3(8, 0.04, 8), YAW_POS)
	for k in range(4):
		var zc2 := 3.3 + k * 3.2
		var half_w2 := 5.66 - absf(zc2 - 7.5) - 0.35
		if half_w2 <= 0.3:
			continue
		_box(b, "GrassDark", Vector3(0, 0.068, zc2), Vector3(half_w2 * 2.0, 0.012, 1.6))
	_cyl(b, "DirtLight", Vector3(HRC.MOUND.x, 0.075, HRC.MOUND.z), Vector3(3.6, 0.14, 3.6))
	_cyl(b, "Dirt", Vector3(0, 0.045, 0), Vector3(5.4, 0.09, 5.4))
	_box(b, "BaseWhite", Vector3(0, 0.13, 0), Vector3(0.5, 0.02, 0.5), YAW_POS)
	for side: int in [1, -1]:
		_box(b, "Line", Vector3(side * 0.5, 0.125, 0), Vector3(0.14, 0.012, 1.33))
		_box(b, "Line", Vector3(side * 1.0, 0.125, 0.6), Vector3(1.14, 0.012, 0.14))
		_box(b, "Line", Vector3(side * 1.0, 0.125, -0.6), Vector3(1.14, 0.012, 0.14))
		_box(b, "Line", Vector3(side * 1.5, 0.125, 0), Vector3(0.14, 0.012, 1.33))
	var bc := HRC.BASE_CORNER
	for pt: Vector2 in [Vector2(-bc, bc), Vector2(0.0, 2.0 * bc), Vector2(bc, bc)]:
		_box(b, "BaseWhite", Vector3(pt.x, 0.12, pt.y), Vector3(0.6, 0.14, 0.6), YAW_POS)
	_box(b, "Line", Vector3(8.5, 0.105, 8.5), Vector3(24, 0.012, 0.32), YAW_NEG)
	_box(b, "Line", Vector3(-8.5, 0.105, 8.5), Vector3(24, 0.012, 0.32), YAW_POS)
	_box(b, "DirtLight", Vector3(7.86, 0.028, 24.86), Vector3(24.5, 0.02, 1.7), YAW_POS)
	_box(b, "DirtLight", Vector3(-7.86, 0.028, 24.86), Vector3(24.5, 0.02, 1.7), YAW_NEG)

static func _build_stadium(b: Dictionary) -> void:
	_box(b, "SkyBowl", Vector3(0, 16, 52), Vector3(150, 34, 1.5))
	for side: int in [1, -1]:
		_box(b, "SkyBowl", Vector3(side * 42, 16, 18), Vector3(1.5, 34, 80))
		var yaw: Quaternion = YAW_POS if side > 0 else YAW_NEG
		var cx := side * 8.9
		_box(b, "WallBlue", Vector3(cx, HRC.WALL_HEIGHT / 2.0, 25.9), Vector3(25.8, HRC.WALL_HEIGHT, 0.9), yaw)
		_box(b, "WallTrim", Vector3(cx, HRC.WALL_HEIGHT + 0.12, 25.9), Vector3(25.8, 0.26, 1.04), yaw)
		for k in range(4):
			var off := 1.4 + k * 1.55
			_box(b, "SeatBlue" if k % 2 == 0 else "SeatBlueDark", Vector3(cx + side * off * 0.707, 1.3 + k * 0.85, 25.9 + off * 0.707), Vector3(27.5 + k * 1.4, 1.7, 1.6), yaw)
		_box(b, "WallBlue", Vector3(side * 17.6, 1.1, 5), Vector3(0.9, 2.2, 25))
		_box(b, "WallTrim", Vector3(side * 17.6, 2.32, 5), Vector3(1.04, 0.24, 25))
		for k in range(3):
			_box(b, "SeatBlue" if k % 2 == 0 else "SeatBlueDark", Vector3(side * (19 + k * 1.5), 0.95 + k * 0.8, 5), Vector3(1.6, 1.5, 25))
		_box(b, "CornerBlue", Vector3(side * 14.2, 1.2, -5.2), Vector3(6.5, 2.6, 6))
		_box(b, "SeatBlueDark", Vector3(side * 15.4, 2.9, -5.6), Vector3(4.5, 1.2, 5))

static func _build_score_panels(b: Dictionary) -> void:
	_box(b, "PanelNavy", Vector3(4.7, 0.045, -2.7), Vector3(3.4, 0.08, 2.1))
	_box(b, "Line", Vector3(4.7, 0.05, -1.78), Vector3(3.4, 0.09, 0.18))
	for k in range(2):
		_box(b, "digit", Vector3(5.25 - k * 1.15, 0.1, -2.85), Vector3(0.62, 0.02, 1.05))
	_box(b, "PanelNavy", Vector3(-4.7, 0.045, -2.7), Vector3(3.4, 0.08, 2.1))
	var dot_rows := [["DotBlue", -2.15], ["DotYellow", -2.7], ["DotRed", -3.25]]
	for row in range(dot_rows.size()):
		var mat: String = dot_rows[row][0]
		var rz: float = dot_rows[row][1]
		_box(b, "Line", Vector3(-3.6, 0.09, rz), Vector3(0.3, 0.02, 0.3))
		for k in range(3):
			_orb(b, mat, Vector3(-4.35 - k * 0.62, 0.11, rz), 0.13)

static func _build_patrol_circles(b: Dictionary) -> void:
	for i in range(HRC.FIELDER_SPOTS.size()):
		var spot: Dictionary = HRC.FIELDER_SPOTS[i]
		var infield: bool = spot.z < 13.5
		var d: float = spot.radius * 1.9
		_cyl(b, "PatrolDirt" if infield else "PatrolGreen", Vector3(spot.x, 0.062 if infield else 0.026, spot.z), Vector3(d, 0.015, d))

# Build every static MultiMeshInstance3D under `parent`. `meshes` maps
# "box"/"sphere"/"cylinder" -> Mesh; `materials` maps name -> Material.
static func build(parent: Node3D, meshes: Dictionary, materials: Dictionary) -> void:
	var buckets: Dictionary = {}
	_build_ground(buckets)
	_build_stadium(buckets)
	_build_score_panels(buckets)
	_build_patrol_circles(buckets)
	for key: String in buckets:
		var parts := key.split("|")
		var xforms: Array = buckets[key]
		var mm := MultiMesh.new()
		mm.transform_format = MultiMesh.TRANSFORM_3D
		mm.mesh = meshes[parts[0]]
		mm.instance_count = xforms.size()
		for i in range(xforms.size()):
			mm.set_instance_transform(i, xforms[i])
		var mmi := MultiMeshInstance3D.new()
		mmi.multimesh = mm
		mmi.material_override = materials[parts[1]]
		mmi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		parent.add_child(mmi)
