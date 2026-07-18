# cinematic_camera.gd — the home-run cinematic's camera director, ported from
# cinematic-camera.ts. Pure functions from (batter | ball) + tuning to a camera pose
# {position, target}. session.gd owns blending between these and the gameplay camera.

const HRC = preload("res://scripts/constants.gd")
const HRMath = preload("res://scripts/math_util.gd")

static func contact_camera_pose(batter: Dictionary, tuning: Dictionary) -> Dictionary:
	return {
		"position": Vector3(batter.x + tuning.lowCameraLateralOffset, tuning.lowCameraHeight, batter.z - tuning.lowCameraBackwardOffset),
		"target": Vector3(batter.x - tuning.lowCameraLateralOffset * 0.4, tuning.lowCameraLookAtHeight, batter.z + 1.1),
	}

static func ground_tracking_camera_pose(batter: Dictionary, ball_pos: Vector3, tuning: Dictionary) -> Dictionary:
	return {
		"position": Vector3(batter.x + tuning.groundCameraLateralOffset, tuning.groundCameraHeight, batter.z - tuning.groundCameraBackwardOffset),
		"target": ball_pos,
	}

static func ground_tracking_zoom_target(ball_vel: Vector3, tuning: Dictionary) -> float:
	return tuning.groundCameraDescentZoomAmount if ball_vel.y < 0.0 else 0.0

static func cinematic_fov_y(zoom_blend: float, tuning: Dictionary) -> float:
	return HRMath.mix(HRC.CAMERA_FOV_Y, HRC.CAMERA_FOV_Y * (1.0 - tuning.cinematicZoomAmount), HRMath.clamp01(zoom_blend))
