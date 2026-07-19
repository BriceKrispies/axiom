# CameraPose — a position/target pair, plus the home-run cinematic's camera director:
# pure functions from (batter | ball) + tuning to a pose. The session owns blending
# between these poses and the ordinary gameplay camera.
extends RefCounted

const CameraPose = preload("res://scripts/cinematic_camera.gd")
const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")

var position: Vector3
var target: Vector3

static func _pose(position: Vector3, target: Vector3) -> CameraPose:
	var p := CameraPose.new()
	p.position = position
	p.target = target
	return p

static func contact_pose(batter_x: float, batter_z: float, tuning: Dictionary) -> CameraPose:
	return _pose(
		Vector3(batter_x + tuning.lowCameraLateralOffset, tuning.lowCameraHeight, batter_z - tuning.lowCameraBackwardOffset),
		Vector3(batter_x - tuning.lowCameraLateralOffset * 0.4, tuning.lowCameraLookAtHeight, batter_z + 1.1))

static func ground_tracking_pose(batter_x: float, batter_z: float, ball_pos: Vector3, tuning: Dictionary) -> CameraPose:
	return _pose(
		Vector3(batter_x + tuning.groundCameraLateralOffset, tuning.groundCameraHeight, batter_z - tuning.groundCameraBackwardOffset),
		ball_pos)

static func ground_tracking_zoom_target(ball_vel: Vector3, tuning: Dictionary) -> float:
	return tuning.groundCameraDescentZoomAmount if ball_vel.y < 0.0 else 0.0

static func cinematic_fov_y(zoom_blend: float, tuning: Dictionary) -> float:
	return HRMath.mix(HRC.CAMERA_FOV_Y, HRC.CAMERA_FOV_Y * (1.0 - tuning.cinematicZoomAmount), HRMath.clamp01(zoom_blend))
