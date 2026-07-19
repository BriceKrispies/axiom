// runtime.mjs — the JS runtime shim the transpiled GDScript imports. It reproduces
// the Godot built-in types and global functions the Axiom-port GDScript uses, with
// identical semantics, so transpiled code runs unchanged in a browser / Node (no
// wasm, no WebGL). Imported by every emitted module as `import * as gd`.
//
// Phase 1 covers the pure-sim surface (vectors, quaternion, colour, scalar math +
// the 32-bit integer helpers hash01 needs). The rasterizer types (Transform3D /
// Basis / Projection / Packed* / a Canvas2D "canvas_item_add_triangle_array") land
// in a later phase.

// ── scalar globals (GDScript builtins) ──────────────────────────────────────────
export const PI = Math.PI;
export const TAU = Math.PI * 2;
export const cos = Math.cos;
export const sin = Math.sin;
export const tan = Math.tan;
export const sqrt = Math.sqrt;
export const exp = Math.exp;
export const atan2 = Math.atan2;
export const floor = Math.floor;
export const ceil = Math.ceil;
export const absf = Math.abs;
export const minf = Math.min;
export const maxf = Math.max;
export const fmod = (a, b) => a % b;
export const clampf = (v, lo, hi) => (v < lo ? lo : v > hi ? hi : v);
export const clamp = clampf;
export const roundi = (v) => Math.round(v);
export const mini = (a, b) => (a < b ? a : b);
export const maxi = (a, b) => (a > b ? a : b);
export const clampi = (v, lo, hi) => (v < lo ? lo : v > hi ? hi : v);
export const absi = (v) => Math.abs(v) | 0;

// GDScript int(x) / float(x) casts.
export const toInt = (x) => Math.trunc(x);
export const toFloat = (x) => +x;

// 32-bit integer multiply (GDScript's 64-bit int * masked to 32 bits). The
// surrounding `& 0xFFFFFFFF` / `>>> 0` the transpiler emits handle the unsigned
// normalization, so this stays bit-identical to the original Math.imul-based hash.
export const imul32 = (a, b) => Math.imul(a, b);

// ── Vector2 (XZ helpers etc.) ───────────────────────────────────────────────────
export class Vector2 {
  constructor(x = 0, y = 0) { this.x = x; this.y = y; }
  static get ZERO() { return new Vector2(0, 0); }
  add(o) { return new Vector2(this.x + o.x, this.y + o.y); }
  sub(o) { return new Vector2(this.x - o.x, this.y - o.y); }
  mul(s) { return new Vector2(this.x * s, this.y * s); }
  length() { return Math.hypot(this.x, this.y); }
  normalized() { const l = this.length(); return l > 0 ? new Vector2(this.x / l, this.y / l) : new Vector2(); }
  distance_to(o) { return Math.hypot(this.x - o.x, this.y - o.y); }
}

// ── Vector3 (world frame) ───────────────────────────────────────────────────────
export class Vector3 {
  constructor(x = 0, y = 0, z = 0) { this.x = x; this.y = y; this.z = z; }
  static get ZERO() { return new Vector3(0, 0, 0); }
  static get ONE() { return new Vector3(1, 1, 1); }
  static get UP() { return new Vector3(0, 1, 0); }
  add(o) { return new Vector3(this.x + o.x, this.y + o.y, this.z + o.z); }
  sub(o) { return new Vector3(this.x - o.x, this.y - o.y, this.z - o.z); }
  mul(s) { return new Vector3(this.x * s, this.y * s, this.z * s); }
  div(s) { return new Vector3(this.x / s, this.y / s, this.z / s); }
  dot(o) { return this.x * o.x + this.y * o.y + this.z * o.z; }
  cross(o) { return new Vector3(this.y * o.z - this.z * o.y, this.z * o.x - this.x * o.z, this.x * o.y - this.y * o.x); }
  length() { return Math.hypot(this.x, this.y, this.z); }
  normalized() { const l = this.length(); return l > 0 ? new Vector3(this.x / l, this.y / l, this.z / l) : new Vector3(); }
  distance_to(o) { return Math.hypot(this.x - o.x, this.y - o.y, this.z - o.z); }
  lerp(o, t) { return new Vector3(this.x + (o.x - this.x) * t, this.y + (o.y - this.y) * t, this.z + (o.z - this.z) * t); }
}

// ── Quaternion (x, y, z, w) ─────────────────────────────────────────────────────
export class Quaternion {
  constructor(x = 0, y = 0, z = 0, w = 1) { this.x = x; this.y = y; this.z = z; this.w = w; }
  static get IDENTITY() { return new Quaternion(0, 0, 0, 1); }
}

// ── Color (rgba, 0..1) ──────────────────────────────────────────────────────────
export class Color {
  constructor(r = 0, g = 0, b = 0, a = 1) { this.r = r; this.g = g; this.b = b; this.a = a; }
  static get BLACK() { return new Color(0, 0, 0, 1); }
}
