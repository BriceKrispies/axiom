# constants.gd — every tuning number for Home Run!, ported from the original
# constants.ts. Gameplay contract (field geometry, batter, swing, pitch profiles,
# fielders, outcomes, scoring) on top; presentation anchors (camera, palette) below.
#
# World frame: home plate at the origin, +Z toward the pitcher and center field,
# +Y up. The camera sits behind home plate at -Z, so world +X projects to
# screen-LEFT.

# ── fixed-step clock ──
const FIXED_HZ := 60
const TICK_SECONDS := 1.0 / 60.0

# ── field geometry (a toy square diamond, corner at home) ──
const FIELD_CORNER := 17.0
const WALL_LINE := FIELD_CORNER * 2.0
const WALL_HEIGHT := 2.6
const BASE_CORNER := 7.5
const MOUND := Vector3(0, 0, 10.2)
const PITCH_RELEASE := Vector3(0, 1.12, 9.7)
const CATCHER_Z := -2.2
const STRIKE_ZONE_HALF_X := 0.45
const STRIKE_ZONE_LOW := 0.4
const STRIKE_ZONE_HIGH := 1.3
const INFIELD_RADIUS := 14.0

# ── ball ──
const BALL_RADIUS := 0.12
const GRAVITY := 22.0
const BOUNCE_RESTITUTION := 0.42
const BOUNCE_FRICTION := 0.68
const ROLL_DECAY := 0.965
const REST_SPEED := 0.5
const WALL_RESTITUTION := 0.35
const FLIGHT_TIMEOUT_TICKS := 420

# ── batter movement ──
const BATTER_MIN_X := 0.55
const BATTER_MAX_X := 1.35
const BATTER_START_X := 0.95
const BATTER_STEP_SPEED := 0.055
const BATTER_Z := -0.15

# ── the always-armed swing ──
const THETA_READY := -0.5
const THETA_SWEET := PI / 2.0
const THETA_FOLLOW_START := 2.3
const THETA_FOLLOW_END := 3.05
const OMEGA_SWING := 0.3
const SNAP_TICKS := 2
const SNAP_START := 0.55
const FOLLOW_DRAG := 0.86
const FOLLOW_MIN_OMEGA := 0.02
const REWIND_RATE := 0.09
const REWIND_EPSILON := 0.02

# ── bat geometry + contact model ──
const BAT_GRIP_R := 0.14
const BAT_TIP_R := 1.18
const BAT_BARREL_R := 0.55
const BAT_HANDLE_W := 0.09
const BAT_BARREL_W := 0.17
const BAT_TIP_W := 0.24
const CONTACT_RADIUS := 0.24
const CONTACT_HEIGHT := 0.3
const BAT_PLANE_Y := 0.85
const BAT_UPPERCUT := 0.22
const BAT_UPPERCUT_CLAMP := 0.18
const SWEET_SPOT_R := 0.88
const SWEET_SPOT_WIDTH := 0.4
const HIT_POWER := 2.25
const PITCH_BOUNCE_SHARE := 0.35
const LOFT_BASE := 0.34
const LOFT_GAIN := 2.0
const LOFT_MIN := -0.5
const LOFT_MAX := 1.15
const VERT_CLEAN_DY := 0.06
const VERT_MISHIT_KEEP := 0.4
const TIMING_WIDTH := 0.38
const TIMING_SPEED_SHARE := 0.35
const CONTACT_SUBSTEPS := 8
const FOUL_ANGLE := PI / 4.0

# ── pitch profiles ──
# Each: {id, name, speed, gravity, targetX, targetY, tier}.
const PITCH_PROFILES := [
	{"id": "slow-straight", "name": "SLOW BALL", "speed": 12.5, "gravity": 8.0, "targetX": 0.0, "targetY": 0.95, "tier": "easy"},
	{"id": "medium-straight", "name": "FASTBALL", "speed": 17.0, "gravity": 8.0, "targetX": 0.0, "targetY": 0.95, "tier": "easy"},
	{"id": "fast-straight", "name": "HEATER", "speed": 23.0, "gravity": 8.0, "targetX": 0.0, "targetY": 1.0, "tier": "hard"},
	{"id": "slow-drop", "name": "SINKER", "speed": 12.0, "gravity": 16.0, "targetX": 0.0, "targetY": 0.72, "tier": "medium"},
	{"id": "fast-flat", "name": "RISER", "speed": 24.0, "gravity": 3.5, "targetX": 0.0, "targetY": 1.1, "tier": "hard"},
	{"id": "inside", "name": "INSIDE", "speed": 16.5, "gravity": 8.0, "targetX": 0.34, "targetY": 0.9, "tier": "medium"},
	{"id": "outside", "name": "OUTSIDE", "speed": 16.5, "gravity": 8.0, "targetX": -0.34, "targetY": 0.9, "tier": "medium"},
]
const EASY_ONLY_BEFORE := 2
const HARD_ALLOWED_FROM := 5
const HARD_LATE_WEIGHT := 2
const JITTER_X := 0.18
const JITTER_Y := 0.09
const JITTER_SPEED := 0.04
const MPH_PER_UNIT := 3.4

# ── round pacing ──
const PITCHES_PER_ROUND := 10
const GAP_TICKS := 25
const GAP_JITTER_TICKS := 35
const WINDUP_TICKS := 48
const FLASH_TICKS := 8
const RESULT_TICKS := 85
const HOMER_RESULT_TICKS := 150

# ── fielders ──
# Each: {name, x, z, radius}.
const FIELDER_SPOTS := [
	{"name": "1B", "radius": 1.7, "x": -6.9, "z": 7.9},
	{"name": "2B", "radius": 1.7, "x": -3.4, "z": 11.8},
	{"name": "SS", "radius": 1.7, "x": 3.4, "z": 11.8},
	{"name": "3B", "radius": 1.7, "x": 6.9, "z": 7.9},
	{"name": "LF", "radius": 2.4, "x": 12.5, "z": 17.5},
	{"name": "LC", "radius": 2.4, "x": 6.8, "z": 22.5},
	{"name": "CF", "radius": 2.4, "x": 0.0, "z": 24.5},
	{"name": "RC", "radius": 2.4, "x": -6.8, "z": 22.5},
	{"name": "RF", "radius": 2.4, "x": -12.5, "z": 17.5},
	{"name": "OP", "radius": 0.7, "x": 2.2, "z": 10.0},
]
const WANDER_AMPLITUDE := 0.72
const WANDER_FREQ_LO := 0.011
const WANDER_FREQ_HI := 0.031
const FIELDER_SPEED := 0.075
const FIELDER_REACH_MULT := 2.0
const FIELDER_CHASE_CLAMP := 1.45
const CATCH_RADIUS := 0.6
const CATCH_HEIGHT := 1.6

# ── outcome thresholds ──
const WEAK_EXIT_SPEED := 15.0
const GROUNDER_LOFT := 0.16
const POPUP_LOFT := 0.62
const POPUP_MAX_DIST := 19.0

# ── scoring ──
const SCORE_TABLE := {
	"ball": 0, "clean": 100, "foul": 0, "grounder": 50,
	"homer": 500, "miss": 0, "popup": 50, "weak": 25,
}
const CLEAN_DIST_BONUS := 1.0
const HOMER_DIST_BONUS := 2.0
const STREAK_MULT_CAP := 4

# ── hit feel ──
const HIT_STOP_QUALITY := 0.5
const HIT_STOP_BASE_TICKS := 2
const HIT_STOP_MAX_EXTRA := 4
const SHAKE_CONTACT := 0.09
const SHAKE_HOMER := 0.2
const SHAKE_TICKS := 14
const SHAKE_TICKS_HOMER := 24

# ── camera (fixed, elevated, behind home plate) ──
const CAMERA_POS := Vector3(0, 6.1, -6.4)
const CAMERA_TARGET := Vector3(0, 0.9, 12)
const CAMERA_FOV_Y := 0.98
const CAMERA_NEAR := 3.5
const CAMERA_FAR := 140.0
const CAMERA_WINDUP_DOLLY := 0.5
const CAMERA_RELEASE_PUNCH := 0.3
const CAMERA_PUNCH_TICKS := 8
const CAMERA_FOLLOW_MAX := 0.42
const CAMERA_FOLLOW_RATE := 0.05
