# Parts — shared helpers the actor views use to create and pose their persistent
# MeshInstance3D parts. Nodes are made once (make) and re-posed every tick (pose_*),
# so nothing is spawned or freed during play. Mesh conventions: box = unit cube
# (scale = extents); sphere = unit diameter (scale = 2r); cylinder = unit.
extends RefCounted

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")
const SunState = preload("res://scripts/sun.gd")

const MIN_EXTENT := 0.01

static func make(parent: Node3D, mesh: Mesh, material: Material) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.material_override = material
	mi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	parent.add_child(mi)
	return mi

static func _xform(pos: Vector3, scale: Vector3, rot: Quaternion) -> Transform3D:
	return Transform3D(Basis(rot) * Basis.from_scale(scale), pos)

static func pose(mi: MeshInstance3D, pos: Vector3, scale: Vector3, rot := Quaternion.IDENTITY) -> void:
	mi.transform = _xform(pos, scale, rot)
	mi.visible = true

static func pose_box(mi: MeshInstance3D, pos: Vector3, extents: Vector3, rot := Quaternion.IDENTITY) -> void:
	pose(mi, pos, Vector3(maxf(extents.x, MIN_EXTENT), maxf(extents.y, MIN_EXTENT), maxf(extents.z, MIN_EXTENT)), rot)

static func pose_orb(mi: MeshInstance3D, pos: Vector3, radius: float) -> void:
	pose(mi, pos, Vector3(radius * 2.0, radius * 2.0, radius * 2.0), Quaternion.IDENTITY)

# Ground height under XZ (infield dirt sits higher than the striped grass).
static func ground_y_at(x: float, z: float) -> float:
	var on_infield_dirt := absf(x) + absf(z - 7.5) <= 7.6
	var on_home_circle := sqrt(x * x + z * z) <= 2.7
	if on_home_circle:
		return 0.1
	return 0.066 if on_infield_dirt else 0.03

# A caster's projected sun-shadow ellipse: a flat translucent cylinder at the feet,
# running along the sun's shadow direction, height*stretch long.
static func pose_shadow(mi: MeshInstance3D, sun: SunState, x: float, z: float, height: float, width: float, lift: float) -> void:
	var length := maxf(height * sun.stretch, width * 1.2)
	var cx := x + sun.dx * (length / 2.0 - width * 0.25)
	var cz := z + sun.dz * (length / 2.0 - width * 0.25)
	var yaw := HRMath.quat_from_euler_xyz(0, atan2(sun.dx, sun.dz), 0)
	pose(mi, Vector3(cx, ground_y_at(cx, cz) + 0.012 + lift, cz), Vector3(width, 0.01, length), yaw)
