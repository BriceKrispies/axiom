# BatterView — the batter + bat as persistent MeshInstance3D parts, built once and
# posed each tick from the SceneView's swing pose. The bat is a stepped taper of
# three box segments swept about the batter's hands.
extends Node3D

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")
const Parts = preload("res://scripts/parts.gd")
const Swing = preload("res://scripts/swing.gd")
const SunState = preload("res://scripts/sun.gd")
const SceneView = preload("res://scripts/scene_view.gd")

const BAT_SEGMENTS := [
	[HRC.BAT_GRIP_R, HRC.BAT_BARREL_R, HRC.BAT_HANDLE_W],
	[HRC.BAT_BARREL_R, (HRC.BAT_BARREL_R + HRC.BAT_TIP_R) / 2.0, HRC.BAT_BARREL_W],
	[(HRC.BAT_BARREL_R + HRC.BAT_TIP_R) / 2.0, HRC.BAT_TIP_R, HRC.BAT_TIP_W],
]

var _shadow: MeshInstance3D
var _puck: MeshInstance3D
var _leg_l: MeshInstance3D
var _leg_r: MeshInstance3D
var _hips: MeshInstance3D
var _torso: MeshInstance3D
var _head: MeshInstance3D
var _cap: MeshInstance3D
var _bat: Array[MeshInstance3D] = []
var _knob: MeshInstance3D
var _arm_l: MeshInstance3D
var _arm_r: MeshInstance3D

func build(meshes: Dictionary, materials: Dictionary) -> void:
	_shadow = Parts.make(self, meshes.cylinder, materials.shadow)
	_puck = Parts.make(self, meshes.cylinder, materials.BatterPuck)
	_leg_l = Parts.make(self, meshes.box, materials.BatterBlue)
	_leg_r = Parts.make(self, meshes.box, materials.BatterBlue)
	_hips = Parts.make(self, meshes.box, materials.BatterBlue)
	_torso = Parts.make(self, meshes.box, materials.BatterBlue)
	_head = Parts.make(self, meshes.sphere, materials.BatterHelmet)
	_cap = Parts.make(self, meshes.sphere, materials.BatterHelmet)
	for i in range(3):
		_bat.append(Parts.make(self, meshes.box, materials.bat))
	_knob = Parts.make(self, meshes.box, materials.BatKnob)
	_arm_l = Parts.make(self, meshes.box, materials.BatterBlue)
	_arm_r = Parts.make(self, meshes.box, materials.BatterBlue)

func pose(view: SceneView, sun: SunState) -> void:
	var bx := view.batter_x
	var bz := HRC.BATTER_Z
	var s := view.swing
	var coil := 0.0 if s.state == "swing" or s.state == "follow" else s.readiness
	var twist := HRMath.clamp01(1.0 - absf(s.theta - HRC.THETA_SWEET) / 2.4)
	var yaw_angle := HRMath.mix(-0.55, 0.5, twist) + coil * -0.35
	var crouch := coil * 0.07
	var yaw := HRMath.quat_from_euler_xyz(0, yaw_angle, coil * 0.12)

	Parts.pose_shadow(_shadow, sun, bx, bz, 1.5 - crouch, 0.95, 0.004)
	Parts.pose(_puck, Vector3(bx, 0.16, bz), Vector3(1.05, 0.12, 0.78))
	Parts.pose_box(_leg_l, Vector3(bx, 0.42 - crouch * 0.5, bz - 0.16), Vector3(0.17, 0.42 - crouch, 0.17))
	Parts.pose_box(_leg_r, Vector3(bx, 0.42 - crouch * 0.5, bz + 0.16), Vector3(0.17, 0.42 - crouch, 0.17))
	Parts.pose_box(_hips, Vector3(bx, 0.68 - crouch, bz), Vector3(0.3, 0.18, 0.42), yaw)
	Parts.pose_box(_torso, Vector3(bx, 0.98 - crouch, bz), Vector3(0.32, 0.46, 0.4), yaw)
	Parts.pose_orb(_head, Vector3(bx, 1.4 - crouch, bz), 0.14)
	Parts.pose_orb(_cap, Vector3(bx, 1.47 - crouch, bz), 0.15)

	var tilt := 0.1 if s.state == "swing" or s.state == "follow" else HRMath.mix(0.1, 0.68, s.readiness)
	var d := Swing.bat_dir(s.theta)
	var pivot_y := Swing.bat_plane_y(s.theta) + 0.05
	var reach := cos(tilt)
	var rot := HRMath.quat_from_euler_xyz(0, s.theta + PI / 2.0, tilt)
	for i in range(3):
		var r0: float = BAT_SEGMENTS[i][0]
		var r1: float = BAT_SEGMENTS[i][1]
		var w: float = BAT_SEGMENTS[i][2]
		var rc := (r0 + r1) / 2.0
		var center := Vector3(bx + d.x * rc * reach, pivot_y + sin(tilt) * rc, bz + d.z * rc * reach)
		Parts.pose_box(_bat[i], center, Vector3(r1 - r0 + 0.02, w, w), rot)
	Parts.pose_box(_knob, Vector3(bx + d.x * HRC.BAT_GRIP_R, pivot_y, bz + d.z * HRC.BAT_GRIP_R), Vector3(0.15, 0.15, 0.15), rot)

	var hand_x := bx + d.x * (HRC.BAT_GRIP_R + 0.12) * reach
	var hand_z := bz + d.z * (HRC.BAT_GRIP_R + 0.12) * reach
	Parts.pose_box(_arm_l, Vector3(HRMath.mix(bx - 0.2, hand_x, 0.55), 1.02 - crouch, HRMath.mix(bz, hand_z, 0.55)), Vector3(0.11, 0.3, 0.11), yaw)
	Parts.pose_box(_arm_r, Vector3(HRMath.mix(bx + 0.2, hand_x, 0.55), 1.02 - crouch, HRMath.mix(bz, hand_z, 0.55)), Vector3(0.11, 0.3, 0.11), yaw)
