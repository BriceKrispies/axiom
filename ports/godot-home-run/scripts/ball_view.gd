# BallView — the ball, its ground shadow, a bounded 14-dot hit trail, and the impact
# flash. The trail dots and flash are persistent nodes toggled visible/hidden rather
# than spawned, so nothing allocates during play.
extends Node3D

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")
const Parts = preload("res://scripts/parts.gd")
const SunState = preload("res://scripts/sun.gd")
const SceneView = preload("res://scripts/scene_view.gd")

var _ball: MeshInstance3D
var _shadow: MeshInstance3D
var _impact: MeshInstance3D
var _trail: Array[MeshInstance3D] = []

func build(meshes: Dictionary, materials: Dictionary) -> void:
	_ball = Parts.make(self, meshes.sphere, materials.BallWhite)
	_shadow = Parts.make(self, meshes.cylinder, materials.shadow)
	for i in range(14):
		_trail.append(Parts.make(self, meshes.sphere, materials.trail))
	_impact = Parts.make(self, meshes.sphere, materials.impact)

func pose(view: SceneView, _sun: SunState) -> void:
	if view.ball_visible:
		var radius := HRC.BALL_RADIUS * 1.15
		var squash: float = view.impact_flash if view.cinematic_phase == "contact" else 0.0
		Parts.pose(_ball, view.ball, Vector3(radius * 2.0 * (1.0 + 0.5 * squash), radius * 2.0 * (1.0 - 0.35 * squash), radius * 2.0 * (1.0 + 0.5 * squash)))
		var gy := Parts.ground_y_at(view.ball.x, view.ball.z)
		var s := 0.36 * (1.0 - HRMath.clamp01(view.ball.y / 14.0) * 0.6)
		Parts.pose(_shadow, Vector3(view.ball.x, gy + 0.006, view.ball.z), Vector3(s, 0.01, s))
	else:
		_ball.visible = false
		_shadow.visible = false

	var cinematic_trail: bool = view.cinematic_phase == "contact" or view.cinematic_phase == "ballFollow"
	var trail_width := 1.5 if cinematic_trail else 1.0
	var n: int = view.trail.size()
	for i in range(14):
		var idx := n - 14 + i
		var p = view.trail[idx] if idx >= 0 else null
		if view.ball_in_play and p != null:
			Parts.pose_orb(_trail[i], p, (0.04 + 0.09 * (float(i + 1) / 14.0)) * trail_width)
		else:
			_trail[i].visible = false

	var f: float = view.impact_flash
	var anchor: Vector3 = view.trail[0] if view.trail.size() > 0 else view.ball
	if f > 0.02 and view.ball_in_play:
		Parts.pose_orb(_impact, anchor, 0.2 + (1.0 - f) * 0.9)
	else:
		_impact.visible = false
