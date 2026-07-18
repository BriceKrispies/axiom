# math_util.gd — the pure math the whole port runs on: the deterministic integer
# hash `hash01` (a bit-exact port of the original vec.ts, JS `Math.imul` + unsigned
# shifts emulated with 32-bit masking, so the SAME seed reproduces the SAME round
# as the TypeScript original), the intrinsic-XYZ Euler→quaternion the scene builder
# poses everything with, and a couple of scalar helpers. Vectors are Godot Vector3
# (structurally the original's `{x,y,z}`); quaternions are Godot Quaternion.

const U32 := 0xFFFFFFFF

static func _imul(a: int, b: int) -> int:
	# 32-bit truncated multiply — the twin of JavaScript's Math.imul.
	return ((a & U32) * (b & U32)) & U32

# A tiny deterministic hash -> [0, 1). All gameplay variation (pitch selection, aim
# jitter, fielder wander phases) derives from hash01(seed, keys), so the same seed
# reproduces the same round bit-for-bit. `keys` is the ordered fold list.
static func hash01(seed: int, keys: Array) -> float:
	var h: int = (seed & U32) ^ 0x9e3779b9
	h &= U32
	for k in keys:
		h = _imul(h ^ (int(k) & U32), 0x85ebca6b)
		h = (h ^ (h >> 13)) & U32
		h = _imul(h, 0xc2b2ae35)
		h = (h ^ (h >> 16)) & U32
	return float(h >> 8) / 16777216.0

# A quaternion from intrinsic XYZ Euler angles (radians) — the exact twin of the
# original vec.ts `quatFromEulerXyz`, computed component-wise so authored rotations
# compose identically regardless of Godot's own Basis euler ordering.
static func quat_from_euler_xyz(rx: float, ry: float, rz: float) -> Quaternion:
	var hx := rx * 0.5
	var hy := ry * 0.5
	var hz := rz * 0.5
	var cx := cos(hx)
	var sx := sin(hx)
	var cy := cos(hy)
	var sy := sin(hy)
	var cz := cos(hz)
	var sz := sin(hz)
	return Quaternion(
		sx * cy * cz + cx * sy * sz,
		cx * sy * cz - sx * cy * sz,
		cx * cy * sz + sx * sy * cz,
		cx * cy * cz - sx * sy * sz,
	)

static func mix(a: float, b: float, t: float) -> float:
	return a + (b - a) * t

static func clamp01(v: float) -> float:
	return clampf(v, 0.0, 1.0)

static func length_xz(v: Vector3) -> float:
	return sqrt(v.x * v.x + v.z * v.z)
