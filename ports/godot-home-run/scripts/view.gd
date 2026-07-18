# view.gd — the PURE presentation of the game, ported from view.ts. scene_of(view,
# now_ms) reads the session's read-only SceneView (+ wall-clock ms for the sun) and
# RETURNS the whole toy stadium described from scratch every frame as keyed instances.
# game.gd's reconciler turns the returned data into MeshInstance3D spawn/re-pose/despawn.
#
# Each instance is {key, mesh, material, position, rotation, scale}. Mesh conventions:
# `box` is a UNIT CUBE (scale = full extents); `sphere` is UNIT DIAMETER (scale = 2r);
# `cylinder` is UNIT (radius 0.5, height 1, Y axis — scale = (diameter, height, diameter)).
class_name HRView

const MIN_EXTENT := 0.01

static func _box_scale(s: Vector3) -> Vector3:
	return Vector3(maxf(s.x, MIN_EXTENT), maxf(s.y, MIN_EXTENT), maxf(s.z, MIN_EXTENT))

static func _sphere_scale(r: float) -> Vector3:
	return Vector3(r * 2.0, r * 2.0, r * 2.0)

static func _mk(key: String, mesh: String, material: String, position: Vector3, scale: Vector3, rotation: Quaternion) -> Dictionary:
	return {"key": key, "material": material, "mesh": mesh, "position": position, "rotation": rotation, "scale": scale}

static func _box(key: String, mat: String, pos: Vector3, scale: Vector3, rot := Quaternion.IDENTITY) -> Dictionary:
	return _mk(key, "box", mat, pos, _box_scale(scale), rot)

static func _cyl(key: String, mat: String, pos: Vector3, scale: Vector3, rot := Quaternion.IDENTITY) -> Dictionary:
	return _mk(key, "cylinder", mat, pos, scale, rot)

static func _orb(key: String, mat: String, pos: Vector3, radius: float) -> Dictionary:
	return _mk(key, "sphere", mat, pos, _sphere_scale(radius), Quaternion.IDENTITY)

static var YAW_POS := HRMath.quat_from_euler_xyz(0, PI / 4.0, 0)
static var YAW_NEG := HRMath.quat_from_euler_xyz(0, -PI / 4.0, 0)

# ── the sun (pure wall-clock presentation) ──
const SUN_LAP_MS := 40 * 60 * 1000
const SUN_ELEV_LOW := 0.14
const SUN_ELEV_HIGH := 0.42
const SUN_GROUND := 0.28
const SUN_GLARE_MAX := 1.5
const SUN_NOON_MS := (40 * 60 * 1000) / 2
const SUN_START_MS := (40 * 60 * 1000) * 0.3
const SHADOW_STRETCH_MAX := 1.5

# Returns {light, dx, dz, stretch}. light = {key, color:Color, direction:Vector3, intensity}.
static func _compute_sun(time_ms: float) -> Dictionary:
	var azimuth := (fmod(time_ms, SUN_LAP_MS) / SUN_LAP_MS) * PI * 2.0
	var height := 0.5 - 0.5 * cos(azimuth)
	var elev := HRMath.mix(SUN_ELEV_LOW, SUN_ELEV_HIGH, height)
	var sun_x := cos(elev) * sin(azimuth)
	var sun_y := sin(elev)
	var sun_z := cos(elev) * cos(azimuth)
	var horiz := sqrt(sun_x * sun_x + sun_z * sun_z)
	var glow := sqrt(height)
	return {
		"dx": -sun_x / horiz,
		"dz": -sun_z / horiz,
		"light": {
			"key": "sun",
			"color": Color(1.0, HRMath.mix(0.62, 0.82, glow), HRMath.mix(0.34, 0.6, glow)),
			"direction": Vector3(-sun_x, -sun_y, -sun_z),
			"intensity": minf(SUN_GROUND / sin(elev), SUN_GLARE_MAX),
		},
		"stretch": minf(horiz / sun_y, SHADOW_STRETCH_MAX),
	}

static var FILL_LIGHT := {"key": "fill", "color": Color(0.72, 0.8, 1.0), "direction": Vector3(0.45, -0.5, -0.4), "intensity": 0.65}

static func _ground_y_at(x: float, z: float) -> float:
	var on_infield_dirt := absf(x) + absf(z - 7.5) <= 7.6
	var on_home_circle := sqrt(x * x + z * z) <= 2.7
	if on_home_circle:
		return 0.1
	return 0.066 if on_infield_dirt else 0.03

static func _cast_shadow(key: String, sun: Dictionary, x: float, z: float, height: float, width: float, lift: float) -> Dictionary:
	var length := maxf(height * sun.stretch, width * 1.2)
	var cx: float = x + sun.dx * (length / 2.0 - width * 0.25)
	var cz: float = z + sun.dz * (length / 2.0 - width * 0.25)
	var yaw := HRMath.quat_from_euler_xyz(0, atan2(sun.dx, sun.dz), 0)
	return _cyl(key, "shadow", Vector3(cx, _ground_y_at(cx, cz) + 0.012 + lift, cz), Vector3(width, 0.01, length), yaw)

# ── static field + stadium ──
static func _build_ground(out: Array) -> void:
	out.append(_box("g/ground", "GroundGreen", Vector3(0, -0.07, 14), Vector3(76, 0.1, 64)))
	out.append(_box("g/deck", "DeckBrown", Vector3(0, -0.005, -2), Vector3(46, 0.06, 15)))
	for side: int in [1, -1]:
		out.append(_box("g/seam/%d" % side, "DirtLight", Vector3(side * 2.6, 0.028, -1.8), Vector3(0.35, 0.02, 8)))
	for k in range(14):
		var zc := 1.2 + k * 2.4
		var half_w: float = minf(zc + 1.2, HRC.WALL_LINE - zc + 1.2)
		if half_w <= 0.4:
			continue
		out.append(_box("g/grass/%d" % k, "GrassLight" if k % 2 == 0 else "GrassDark", Vector3(0, 0.002, zc), Vector3(half_w * 2.0, 0.03, 2.4)))
	out.append(_box("g/idirt", "Dirt", Vector3(0, 0.03, 7.5), Vector3(10.6, 0.05, 10.6), YAW_POS))
	out.append(_box("g/igrass", "GrassLight", Vector3(0, 0.045, 7.5), Vector3(8, 0.04, 8), YAW_POS))
	for k in range(4):
		var zc2 := 3.3 + k * 3.2
		var half_w2 := 5.66 - absf(zc2 - 7.5) - 0.35
		if half_w2 <= 0.3:
			continue
		out.append(_box("g/idiamond/%d" % k, "GrassDark", Vector3(0, 0.068, zc2), Vector3(half_w2 * 2.0, 0.012, 1.6)))
	out.append(_cyl("g/mound", "DirtLight", Vector3(HRC.MOUND.x, 0.075, HRC.MOUND.z), Vector3(3.6, 0.14, 3.6)))
	out.append(_cyl("g/homecircle", "Dirt", Vector3(0, 0.045, 0), Vector3(5.4, 0.09, 5.4)))
	out.append(_box("g/plate", "BaseWhite", Vector3(0, 0.13, 0), Vector3(0.5, 0.02, 0.5), YAW_POS))
	for side: int in [1, -1]:
		var s := "p" if side > 0 else "n"
		out.append(_box("g/box/%s/0" % s, "Line", Vector3(side * 0.5, 0.125, 0), Vector3(0.14, 0.012, 1.33)))
		out.append(_box("g/box/%s/1" % s, "Line", Vector3(side * 1.0, 0.125, 0.6), Vector3(1.14, 0.012, 0.14)))
		out.append(_box("g/box/%s/2" % s, "Line", Vector3(side * 1.0, 0.125, -0.6), Vector3(1.14, 0.012, 0.14)))
		out.append(_box("g/box/%s/3" % s, "Line", Vector3(side * 1.5, 0.125, 0), Vector3(0.14, 0.012, 1.33)))
	var b := HRC.BASE_CORNER
	var bases := [[-b, b], [0.0, 2.0 * b], [b, b]]
	for i in range(bases.size()):
		out.append(_box("g/base/%d" % i, "BaseWhite", Vector3(bases[i][0], 0.12, bases[i][1]), Vector3(0.6, 0.14, 0.6), YAW_POS))
	out.append(_box("g/foul/p", "Line", Vector3(8.5, 0.105, 8.5), Vector3(24, 0.012, 0.32), YAW_NEG))
	out.append(_box("g/foul/n", "Line", Vector3(-8.5, 0.105, 8.5), Vector3(24, 0.012, 0.32), YAW_POS))
	out.append(_box("g/track/p", "DirtLight", Vector3(7.86, 0.028, 24.86), Vector3(24.5, 0.02, 1.7), YAW_POS))
	out.append(_box("g/track/n", "DirtLight", Vector3(-7.86, 0.028, 24.86), Vector3(24.5, 0.02, 1.7), YAW_NEG))

static func _build_stadium(out: Array) -> void:
	out.append(_box("s/bowl/c", "SkyBowl", Vector3(0, 16, 52), Vector3(150, 34, 1.5)))
	for side: int in [1, -1]:
		var s := "p" if side > 0 else "n"
		out.append(_box("s/bowl/%s" % s, "SkyBowl", Vector3(side * 42, 16, 18), Vector3(1.5, 34, 80)))
		var yaw: Quaternion = YAW_POS if side > 0 else YAW_NEG
		var cx := side * 8.9
		out.append(_box("s/wall/%s" % s, "WallBlue", Vector3(cx, HRC.WALL_HEIGHT / 2.0, 25.9), Vector3(25.8, HRC.WALL_HEIGHT, 0.9), yaw))
		out.append(_box("s/trim/%s" % s, "WallTrim", Vector3(cx, HRC.WALL_HEIGHT + 0.12, 25.9), Vector3(25.8, 0.26, 1.04), yaw))
		for k in range(4):
			var off := 1.4 + k * 1.55
			out.append(_box("s/seat/%s/%d" % [s, k], "SeatBlue" if k % 2 == 0 else "SeatBlueDark", Vector3(cx + side * off * 0.707, 1.3 + k * 0.85, 25.9 + off * 0.707), Vector3(27.5 + k * 1.4, 1.7, 1.6), yaw))
		out.append(_box("s/fence/%s" % s, "WallBlue", Vector3(side * 17.6, 1.1, 5), Vector3(0.9, 2.2, 25)))
		out.append(_box("s/fencetrim/%s" % s, "WallTrim", Vector3(side * 17.6, 2.32, 5), Vector3(1.04, 0.24, 25)))
		for k in range(3):
			out.append(_box("s/sideseat/%s/%d" % [s, k], "SeatBlue" if k % 2 == 0 else "SeatBlueDark", Vector3(side * (19 + k * 1.5), 0.95 + k * 0.8, 5), Vector3(1.6, 1.5, 25)))
		out.append(_box("s/corner/%s" % s, "CornerBlue", Vector3(side * 14.2, 1.2, -5.2), Vector3(6.5, 2.6, 6)))
		out.append(_box("s/cornercap/%s" % s, "SeatBlueDark", Vector3(side * 15.4, 2.9, -5.6), Vector3(4.5, 1.2, 5)))

static func _build_score_panels(out: Array) -> void:
	out.append(_box("sp/panelL", "PanelNavy", Vector3(4.7, 0.045, -2.7), Vector3(3.4, 0.08, 2.1)))
	out.append(_box("sp/panelLbar", "Line", Vector3(4.7, 0.05, -1.78), Vector3(3.4, 0.09, 0.18)))
	for k in range(2):
		out.append(_box("sp/digit/%d" % k, "digit", Vector3(5.25 - k * 1.15, 0.1, -2.85), Vector3(0.62, 0.02, 1.05)))
	out.append(_box("sp/panelR", "PanelNavy", Vector3(-4.7, 0.045, -2.7), Vector3(3.4, 0.08, 2.1)))
	var dot_rows := [["DotBlue", -2.15], ["DotYellow", -2.7], ["DotRed", -3.25]]
	for row in range(dot_rows.size()):
		var mat: String = dot_rows[row][0]
		var rz: float = dot_rows[row][1]
		out.append(_box("sp/dotbar/%d" % row, "Line", Vector3(-3.6, 0.09, rz), Vector3(0.3, 0.02, 0.3)))
		for k in range(3):
			out.append(_orb("sp/dot/%d/%d" % [row, k], mat, Vector3(-4.35 - k * 0.62, 0.11, rz), 0.13))

static func _build_patrol_circles(out: Array) -> void:
	for i in range(HRC.FIELDER_SPOTS.size()):
		var spot: Dictionary = HRC.FIELDER_SPOTS[i]
		var infield: bool = spot.z < 13.5
		var d: float = spot.radius * 1.9
		out.append(_cyl("pc/%d" % i, "PatrolDirt" if infield else "PatrolGreen", Vector3(spot.x, 0.062 if infield else 0.026, spot.z), Vector3(d, 0.015, d)))

# ── dynamic actors ──
static var BAT_SEGMENTS := [
	[HRC.BAT_GRIP_R, HRC.BAT_BARREL_R, HRC.BAT_HANDLE_W],
	[HRC.BAT_BARREL_R, (HRC.BAT_BARREL_R + HRC.BAT_TIP_R) / 2.0, HRC.BAT_BARREL_W],
	[(HRC.BAT_BARREL_R + HRC.BAT_TIP_R) / 2.0, HRC.BAT_TIP_R, HRC.BAT_TIP_W],
]

static func _bat_tilt(state: String, readiness: float) -> float:
	if state == "swing" or state == "follow":
		return 0.1
	return HRMath.mix(0.1, 0.68, readiness)

static func _build_batter(out: Array, sun: Dictionary, view: Dictionary) -> void:
	var bx: float = view.batterX
	var bz := HRC.BATTER_Z
	var s: Dictionary = view.swing
	var coil: float = 0.0 if s.state == "swing" or s.state == "follow" else s.readiness
	var twist := HRMath.clamp01(1.0 - absf(s.theta - HRC.THETA_SWEET) / 2.4)
	var yaw_angle: float = HRMath.mix(-0.55, 0.5, twist) + coil * -0.35
	var crouch: float = coil * 0.07
	var yaw := HRMath.quat_from_euler_xyz(0, yaw_angle, coil * 0.12)

	out.append(_cast_shadow("batter/shadow", sun, bx, bz, 1.5 - crouch, 0.95, 0.004))
	out.append(_cyl("batter/puck", "BatterPuck", Vector3(bx, 0.16, bz), Vector3(1.05, 0.12, 0.78)))
	out.append(_box("batter/legL", "BatterBlue", Vector3(bx, 0.42 - crouch * 0.5, bz - 0.16), Vector3(0.17, 0.42 - crouch, 0.17)))
	out.append(_box("batter/legR", "BatterBlue", Vector3(bx, 0.42 - crouch * 0.5, bz + 0.16), Vector3(0.17, 0.42 - crouch, 0.17)))
	out.append(_box("batter/hips", "BatterBlue", Vector3(bx, 0.68 - crouch, bz), Vector3(0.3, 0.18, 0.42), yaw))
	out.append(_box("batter/torso", "BatterBlue", Vector3(bx, 0.98 - crouch, bz), Vector3(0.32, 0.46, 0.4), yaw))
	out.append(_orb("batter/head", "BatterHelmet", Vector3(bx, 1.4 - crouch, bz), 0.14))
	out.append(_orb("batter/cap", "BatterHelmet", Vector3(bx, 1.47 - crouch, bz), 0.15))

	var tilt := _bat_tilt(s.state, s.readiness)
	var d := HRSwing.bat_dir(s.theta)
	var pivot_y := HRSwing.bat_plane_y(s.theta) + 0.05
	var reach := cos(tilt)
	var rot := HRMath.quat_from_euler_xyz(0, s.theta + PI / 2.0, tilt)
	for i in range(BAT_SEGMENTS.size()):
		var r0: float = BAT_SEGMENTS[i][0]
		var r1: float = BAT_SEGMENTS[i][1]
		var w: float = BAT_SEGMENTS[i][2]
		var rc := (r0 + r1) / 2.0
		var center := Vector3(bx + d.x * rc * reach, pivot_y + sin(tilt) * rc, bz + d.z * rc * reach)
		out.append(_box("bat/%d" % i, "bat", center, Vector3(r1 - r0 + 0.02, w, w), rot))
	out.append(_box("bat/knob", "BatKnob", Vector3(bx + d.x * HRC.BAT_GRIP_R, pivot_y, bz + d.z * HRC.BAT_GRIP_R), Vector3(0.15, 0.15, 0.15), rot))

	var hand_x := bx + d.x * (HRC.BAT_GRIP_R + 0.12) * reach
	var hand_z := bz + d.z * (HRC.BAT_GRIP_R + 0.12) * reach
	var arms := [["armL", -0.2], ["armR", 0.2]]
	for arm in arms:
		var name: String = arm[0]
		var side_x: float = arm[1]
		var ax := HRMath.mix(bx + side_x, hand_x, 0.55)
		var az := HRMath.mix(bz, hand_z, 0.55)
		out.append(_box("batter/%s" % name, "BatterBlue", Vector3(ax, 1.02 - crouch, az), Vector3(0.11, 0.3, 0.11), yaw))

static func _build_machine(out: Array, sun: Dictionary, view: Dictionary) -> void:
	var mz := HRC.MOUND.z
	out.append(_cast_shadow("machine/shadow", sun, 0, mz, 1.35, 1.15, 0.002))
	out.append(_box("machine/base", "MachineDark", Vector3(0, 0.28, mz), Vector3(1.15, 0.26, 0.9)))
	for side: int in [1, -1]:
		out.append(_cyl("machine/wheel/%d" % side, "MachineDark", Vector3(side * 0.62, 0.3, mz), Vector3(0.4, 0.14, 0.4), HRMath.quat_from_euler_xyz(0, 0, PI / 2.0)))
	out.append(_cyl("machine/muzzle", "BaseWhite", Vector3(0, 1.16, mz - 0.88), Vector3(0.3, 0.06, 0.3), HRMath.quat_from_euler_xyz(PI / 2.0, 0, 0)))
	var squash: float = 1.0 - 0.2 * view.windup
	var recoil: float = 0.26 * view.windup - 0.34 * view.muzzleFlash
	out.append(_box("machine/body", "MachineOrange", Vector3(0, 0.41 + 0.62 * squash * 0.5, mz), Vector3(0.9 * (1.0 + 0.12 * view.windup), 0.62 * squash, 0.78)))
	out.append(_cyl("machine/barrel", "MachineDark", Vector3(0, 1.16, mz - 0.35 + recoil), Vector3(0.26, 1.1, 0.26), HRMath.quat_from_euler_xyz(PI / 2.0, 0, 0)))
	out.append(_box("machine/hopper", "MachineOrange", Vector3(0, 0.98 + 0.16 * squash, mz + 0.42), Vector3(0.56, 0.34, 0.46)))
	var blink: float = 0.12 + 0.07 * sin(view.tick * 0.9) if view.windup > 0.82 else 0.0
	var flash := maxf(view.muzzleFlash * 0.4, blink)
	if flash > 0.01:
		out.append(_orb("machine/flash", "flash", Vector3(0, 1.16, mz - 0.95), flash))

static func _ball_squash_scale(view: Dictionary) -> Vector3:
	var radius := HRC.BALL_RADIUS * 1.15
	var squash: float = view.impactFlash if view.cinematicPhase == "contact" else 0.0
	return Vector3(radius * 2.0 * (1.0 + 0.5 * squash), radius * 2.0 * (1.0 - 0.35 * squash), radius * 2.0 * (1.0 + 0.5 * squash))

static func _build_ball(out: Array, view: Dictionary) -> void:
	if view.ballVisible:
		out.append(_mk("ball", "sphere", "BallWhite", view.ball, _ball_squash_scale(view), Quaternion.IDENTITY))
		var gy := _ground_y_at(view.ball.x, view.ball.z)
		var s := 0.36 * (1.0 - HRMath.clamp01(view.ball.y / 14.0) * 0.6)
		out.append(_cyl("ball/shadow", "shadow", Vector3(view.ball.x, gy + 0.006, view.ball.z), Vector3(s, 0.01, s)))
	var cinematic_trail: bool = view.cinematicPhase == "contact" or view.cinematicPhase == "ballFollow"
	var trail_width: float = 1.5 if cinematic_trail else 1.0
	var n: int = view.trail.size()
	for i in range(14):
		var idx := n - 14 + i
		var p = view.trail[idx] if idx >= 0 else null
		if view.ballInPlay and p != null:
			out.append(_orb("trail/%d" % i, "trail", p, (0.04 + 0.09 * (float(i + 1) / 14.0)) * trail_width))
	var f: float = view.impactFlash
	var anchor: Vector3 = view.trail[0] if view.trail.size() > 0 else view.ball
	if f > 0.02 and view.ballInPlay:
		out.append(_orb("impact", "impact", anchor, 0.2 + (1.0 - f) * 0.9))

static func _build_fielders(out: Array, sun: Dictionary, view: Dictionary) -> void:
	var celebration_boost := 1.8 if view.cinematicPhase == "celebration" else 1.0
	var fielders: Array = view.fielders
	for i in range(fielders.size()):
		var f: Dictionary = fielders[i]
		var bob := absf(sin(view.tick * (0.24 if f.chasing else 0.11) + i * 1.7)) * (0.07 if f.chasing else 0.035) * celebration_boost
		var lean := 0.18 if f.chasing else 0.0
		var rot := HRMath.quat_from_euler_xyz(lean, 0, 0)
		var x: float = f.x
		var z: float = f.z
		out.append(_cast_shadow("fielder/%d/shadow" % i, sun, x, z, 1.15, 0.7, 0.006 + i * 0.002))
		out.append(_cyl("fielder/%d/puck" % i, "FielderBase", Vector3(x, 0.07, z), Vector3(0.95, 0.12, 0.68)))
		out.append(_box("fielder/%d/legL" % i, "FielderWhite", Vector3(x - 0.09, 0.3, z), Vector3(0.13, 0.34, 0.15)))
		out.append(_box("fielder/%d/legR" % i, "FielderWhite", Vector3(x + 0.09, 0.3, z), Vector3(0.13, 0.34, 0.15)))
		out.append(_box("fielder/%d/hips" % i, "FielderWhite", Vector3(x, 0.52 + bob, z), Vector3(0.28, 0.14, 0.19), rot))
		out.append(_box("fielder/%d/torso" % i, "FielderWhite", Vector3(x, 0.74 + bob, z), Vector3(0.3, 0.32, 0.2), rot))
		out.append(_box("fielder/%d/armL" % i, "FielderCap", Vector3(x - 0.2, 0.74 + bob, z), Vector3(0.09, 0.28, 0.09), rot))
		out.append(_box("fielder/%d/armR" % i, "FielderCap", Vector3(x + 0.2, 0.74 + bob, z), Vector3(0.09, 0.28, 0.09), rot))
		out.append(_orb("fielder/%d/head" % i, "FielderWhite", Vector3(x, 1.02 + bob, z + lean * 0.1), 0.11))
		out.append(_orb("fielder/%d/cap" % i, "FielderCap", Vector3(x, 1.08 + bob, z + lean * 0.1), 0.12))

# The whole frame as pure data: the toy stadium arranged for this `view`, lit by the
# wall-clock sun.
static func scene_of(view: Dictionary, now_ms: float) -> Dictionary:
	var sun := _compute_sun(now_ms)
	var instances: Array = []
	_build_ground(instances)
	_build_stadium(instances)
	_build_score_panels(instances)
	_build_patrol_circles(instances)
	_build_batter(instances, sun, view)
	_build_machine(instances, sun, view)
	_build_ball(instances, view)
	_build_fielders(instances, sun, view)
	return {
		"camera": {"far": HRC.CAMERA_FAR, "fovY": view.cameraFovY, "near": HRC.CAMERA_NEAR, "position": view.cameraPos, "target": view.cameraTarget},
		"clearColor": Color(0.62, 0.72, 0.95),
		"instances": instances,
		"lights": [sun.light, FILL_LIGHT],
	}
