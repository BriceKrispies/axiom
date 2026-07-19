# MachineView — the pitching machine: a static chassis plus animated body/barrel/
# hopper that compress on the wind-up, and a muzzle flash that blinks near release.
extends Node3D

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")
const Parts = preload("res://scripts/parts.gd")
const SunState = preload("res://scripts/sun.gd")
const SceneView = preload("res://scripts/scene_view.gd")

var _shadow: MeshInstance3D
var _base: MeshInstance3D
var _wheel_l: MeshInstance3D
var _wheel_r: MeshInstance3D
var _muzzle: MeshInstance3D
var _body: MeshInstance3D
var _barrel: MeshInstance3D
var _hopper: MeshInstance3D
var _flash: MeshInstance3D

func build(meshes: Dictionary, materials: Dictionary) -> void:
	_shadow = Parts.make(self, meshes.cylinder, materials.shadow)
	_base = Parts.make(self, meshes.box, materials.MachineDark)
	_wheel_l = Parts.make(self, meshes.cylinder, materials.MachineDark)
	_wheel_r = Parts.make(self, meshes.cylinder, materials.MachineDark)
	_muzzle = Parts.make(self, meshes.cylinder, materials.BaseWhite)
	_body = Parts.make(self, meshes.box, materials.MachineOrange)
	_barrel = Parts.make(self, meshes.cylinder, materials.MachineDark)
	_hopper = Parts.make(self, meshes.box, materials.MachineOrange)
	_flash = Parts.make(self, meshes.sphere, materials.flash)

func pose(view: SceneView, sun: SunState) -> void:
	var mz := HRC.MOUND.z
	var side_rot := HRMath.quat_from_euler_xyz(0, 0, PI / 2.0)
	var up_rot := HRMath.quat_from_euler_xyz(PI / 2.0, 0, 0)

	Parts.pose_shadow(_shadow, sun, 0, mz, 1.35, 1.15, 0.002)
	Parts.pose_box(_base, Vector3(0, 0.28, mz), Vector3(1.15, 0.26, 0.9))
	Parts.pose(_wheel_l, Vector3(0.62, 0.3, mz), Vector3(0.4, 0.14, 0.4), side_rot)
	Parts.pose(_wheel_r, Vector3(-0.62, 0.3, mz), Vector3(0.4, 0.14, 0.4), side_rot)
	Parts.pose(_muzzle, Vector3(0, 1.16, mz - 0.88), Vector3(0.3, 0.06, 0.3), up_rot)

	var squash := 1.0 - 0.2 * view.windup
	var recoil := 0.26 * view.windup - 0.34 * view.muzzle_flash
	Parts.pose_box(_body, Vector3(0, 0.41 + 0.62 * squash * 0.5, mz), Vector3(0.9 * (1.0 + 0.12 * view.windup), 0.62 * squash, 0.78))
	Parts.pose(_barrel, Vector3(0, 1.16, mz - 0.35 + recoil), Vector3(0.26, 1.1, 0.26), up_rot)
	Parts.pose_box(_hopper, Vector3(0, 0.98 + 0.16 * squash, mz + 0.42), Vector3(0.56, 0.34, 0.46))

	var blink := 0.12 + 0.07 * sin(view.tick * 0.9) if view.windup > 0.82 else 0.0
	var flash := maxf(view.muzzle_flash * 0.4, blink)
	if flash > 0.01:
		Parts.pose_orb(_flash, Vector3(0, 1.16, mz - 0.95), flash)
	else:
		_flash.visible = false
