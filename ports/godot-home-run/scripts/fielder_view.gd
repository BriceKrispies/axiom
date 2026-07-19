# FielderView — one toy defender as persistent parts, instanced ten times by the
# host. Poses itself from a Fielder state (position + chasing flag), with a seeded
# idle bob that amplifies during the home-run celebration and a forward lean while
# chasing.
extends Node3D

const HRMath = preload("res://scripts/math_util.gd")
const Parts = preload("res://scripts/parts.gd")
const Fielder = preload("res://scripts/fielders.gd")
const SunState = preload("res://scripts/sun.gd")

var _shadow: MeshInstance3D
var _puck: MeshInstance3D
var _leg_l: MeshInstance3D
var _leg_r: MeshInstance3D
var _hips: MeshInstance3D
var _torso: MeshInstance3D
var _arm_l: MeshInstance3D
var _arm_r: MeshInstance3D
var _head: MeshInstance3D
var _cap: MeshInstance3D

func build(meshes: Dictionary, materials: Dictionary) -> void:
	_shadow = Parts.make(self, meshes.cylinder, materials.shadow)
	_puck = Parts.make(self, meshes.cylinder, materials.FielderBase)
	_leg_l = Parts.make(self, meshes.box, materials.FielderWhite)
	_leg_r = Parts.make(self, meshes.box, materials.FielderWhite)
	_hips = Parts.make(self, meshes.box, materials.FielderWhite)
	_torso = Parts.make(self, meshes.box, materials.FielderWhite)
	_arm_l = Parts.make(self, meshes.box, materials.FielderCap)
	_arm_r = Parts.make(self, meshes.box, materials.FielderCap)
	_head = Parts.make(self, meshes.sphere, materials.FielderWhite)
	_cap = Parts.make(self, meshes.sphere, materials.FielderCap)

func pose(f: Fielder, index: int, tick: int, celebration_boost: float, sun: SunState) -> void:
	var bob := absf(sin(tick * (0.24 if f.chasing else 0.11) + index * 1.7)) * (0.07 if f.chasing else 0.035) * celebration_boost
	var lean := 0.18 if f.chasing else 0.0
	var rot := HRMath.quat_from_euler_xyz(lean, 0, 0)
	var x := f.x
	var z := f.z
	Parts.pose_shadow(_shadow, sun, x, z, 1.15, 0.7, 0.006 + index * 0.002)
	Parts.pose(_puck, Vector3(x, 0.07, z), Vector3(0.95, 0.12, 0.68))
	Parts.pose_box(_leg_l, Vector3(x - 0.09, 0.3, z), Vector3(0.13, 0.34, 0.15))
	Parts.pose_box(_leg_r, Vector3(x + 0.09, 0.3, z), Vector3(0.13, 0.34, 0.15))
	Parts.pose_box(_hips, Vector3(x, 0.52 + bob, z), Vector3(0.28, 0.14, 0.19), rot)
	Parts.pose_box(_torso, Vector3(x, 0.74 + bob, z), Vector3(0.3, 0.32, 0.2), rot)
	Parts.pose_box(_arm_l, Vector3(x - 0.2, 0.74 + bob, z), Vector3(0.09, 0.28, 0.09), rot)
	Parts.pose_box(_arm_r, Vector3(x + 0.2, 0.74 + bob, z), Vector3(0.09, 0.28, 0.09), rot)
	Parts.pose_orb(_head, Vector3(x, 1.02 + bob, z + lean * 0.1), 0.11)
	Parts.pose_orb(_cap, Vector3(x, 1.08 + bob, z + lean * 0.1), 0.12)
