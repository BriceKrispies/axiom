# cinematic_constants.gd — the ONE tuning object for the home-run cinematic, ported
# from cinematic-constants.ts. Field geometry is read straight from HRC so the
# trajectory projection and the real ball physics can never drift apart.
class_name HRCine

static var TUNING := {
	# field geometry the trajectory projection classifies against (shared)
	"foulLineHalfAngle": HRC.FOUL_ANGLE,
	"outfieldWallDistance": HRC.WALL_LINE,
	"outfieldWallHeight": HRC.WALL_HEIGHT,
	"gravity": HRC.GRAVITY,

	# trajectory prediction (evaluateSwingOutcome)
	"trajectoryPredictionStepTicks": 1,
	"maxPredictionSteps": 300,
	"swingContactSearchMaxTicks": 240,

	# cinematic timeline (ticks at the base 60 Hz sim rate)
	"preContactCinematicLeadTicks": 40,
	"cinematicCameraBlendDurationTicks": 30,
	"contactSlowMotionScale": 0.2,
	"contactSlowMotionDurationTicks": 40,
	"postContactSlowMotionScale": 0.55,
	"timeScaleRecoveryDurationTicks": 55,
	"impactHoldDurationTicks": 5,
	"letterboxEntranceDurationTicks": 32,
	"letterboxExitDurationTicks": 38,
	"letterboxScreenFraction": 0.12,
	"cinematicZoomAmount": 0.22,

	# low-angle contact camera (offsets from the batter's transform)
	"lowCameraLateralOffset": 3.0,
	"lowCameraHeight": 0.5,
	"lowCameraBackwardOffset": 2.6,
	"lowCameraLookAtHeight": 1.1,

	# ground-tracking camera (offsets from the batter's transform)
	"groundCameraLateralOffset": 1.2,
	"groundCameraHeight": 1.8,
	"groundCameraBackwardOffset": 4.5,
	"groundCameraDescentZoomAmount": 0.5,

	"landingCameraDurationTicks": 55,

	# bounded effects
	"confettiMaxCount": 36,
	"impactParticleMaxCount": 10,
	"impactFlashDurationTicks": 10,
	"cameraShakeStrength": 0.16,
	"cameraShakeDurationTicks": 18,
}
