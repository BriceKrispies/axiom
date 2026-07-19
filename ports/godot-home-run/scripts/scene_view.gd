# SceneView — the read-only snapshot HomeRunSession hands the presentation each tick:
# everything the 3D view needs and nothing it could use to mutate gameplay.
extends RefCounted

const Swing = preload("res://scripts/swing.gd")

var phase: String
var tick: int                 # the GATED gameplay tick (slows during a cinematic)
var batter_x: float
var swing: Swing
var ball: Vector3
var ball_visible: bool         # hidden between pitches
var ball_in_play: bool         # post-contact (drives the trail)
var trail: Array               # recent ball positions (Vector3), bounded
var windup: float              # machine wind-up compression 0..1
var muzzle_flash: float        # 0..1
var fielders: Array            # Fielder, in FIELDER_SPOTS order
var camera_pos: Vector3        # already composed: base + dolly + punch + follow + shake + cinematic blend
var camera_target: Vector3
var camera_fov_y: float
var impact_flash: float        # scene pulse on strong contact, 0..1
var hit_stop: bool
var cinematic_phase: String    # "none" for every ordinary pitch/swing
var letterbox_progress: float  # 0..1
var hud_visible: bool
