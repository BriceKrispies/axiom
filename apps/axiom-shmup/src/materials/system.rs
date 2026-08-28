//! Ported from Claude-of-Duty `src/materials/index.js:1-353` — the materials
//! **facade**: the two caches (`_sets`, `_materials`), the name/alias
//! resolution in front of them, the parameter merge behind them, and the
//! scratch-release idle timer.
//!
//! ```text
//! PUBLIC API   const materials = ctx.get('materials')
//!   get(name, opts)             -> MaterialDef   (cached; same opts, same entry)
//!   get_texture_set(name, opts) -> TextureSet    (cached on the BAKE key only)
//!   variant(name, opts)         -> alias for get()
//!   names() / surface_of(name)
//!   tune(mat_key, changes)      -> live uniform edit
//!   set_ground_level(y)
//!   bake_masks(geo, opts, rng) / set_mask(geo, ...)
//!   debug_board(opts)
//!   dispose()
//! ```
//!
//! The nineteen surface generators, the noise library, the mask bake and the
//! CPU bake pipeline are already ported — [`super::surfaces`],
//! [`super::noise`], [`super::masks`], [`super::bake`]. This module is the
//! cache and orchestration layer on top of them, and its whole job is
//! *identity*: which requests collapse onto one bake, which stay apart, and
//! what parameters each material ends up with.
//!
//! ## The two caches, and why their keys differ
//!
//! ```text
//! _sets      bakeKey  = `${key}|${size}|${seed}|${tintA}|${tintB}|${param.join('_')}`
//! _materials matKey   = `${key}|${stableKey(opts)}`
//! ```
//!
//! The bake key names **only what changes texels**; the material key names
//! **every option**. That is the collapse the whole system is built on: the
//! level's 46 palette entries — five plasters, four woods, three fabrics —
//! are 46 distinct materials over **19** bakes, because a `tint` or a `scale`
//! is a shader uniform, not a different texture. Get the key wrong in either
//! direction and the engine silently bakes more (slow) or fewer (wrong)
//! textures than the original.
//!
//! Both keys are **strings assembled by JavaScript string coercion**, so this
//! port reproduces that coercion rather than approximating it — see
//! [`js_number`], and [`OptValue::to_json`] for `JSON.stringify`. Two details
//! that a "close enough" key gets wrong:
//!
//! - `stableKey` sorts the **top-level** keys and then `JSON.stringify`s each
//!   value. A nested object (`three: {…}`, `bake: {…}`) is stringified in
//!   **insertion order**, so `{opacity, envMapIntensity}` and
//!   `{envMapIntensity, opacity}` are *different materials*. [`MaterialOpts`]
//!   is therefore insertion-ordered, and only [`stable_key`] sorts.
//! - A hex colour is a JS `Number`, so `tint: 0xcfc0a4` keys as
//!   `tint=13615268`, in decimal.
//!
//! ## Three source defects, ported and pinned
//!
//! 1. **`worldSize` and `relief` are not part of the bake key.** Asking for
//!    `{ bake: { worldSize: 9 } }` after the default bake exists returns the
//!    *cached* set, built at the library's `worldSize` — the override is
//!    silently dropped (`index.js:117-121` lists five fields; the def passes
//!    seven).
//! 2. **The "built without textures" warning is unreachable.**
//!    `index.js:214`'s `else if (!this._warned)` can only fire when there is
//!    no texture set, and the only thing that produces no texture set —
//!    `_tryBuild` failing — has already set `_warned` on the line above.
//! 3. **Medium quality bakes at high quality's resolution.** `_size` scales
//!    by `0.75` and then snaps to the nearest power of two, and
//!    `round(log2(768)) == 10`, so every 1024 base and every 512 base comes
//!    back unchanged. Only `low` (0.5, an exact halving) actually reduces
//!    anything.
//!
//! ## Three seams this port has to name
//!
//! 1. **The renderer.** `_renderer()` reads an injected renderer, else
//!    `ctx.peek('render')?.renderer`. There is no render subsystem in this
//!    crate yet, so only the injected arm exists — as [`RendererCaps`], which
//!    carries the single thing `TextureForge` asks of a `WebGLRenderer`
//!    (`capabilities.getMaxAnisotropy()`, `generator.js:147-150`). When a
//!    render subsystem lands it calls [`MaterialSystem::set_renderer`].
//! 2. **Texels are baked on demand, not at cache-fill time.** The source's
//!    `TextureForge.build` is four full-screen GPU draws with no readback;
//!    the port's [`super::bake::bake`] is a CPU loop over `size²` texels, and
//!    the library's nineteen 1024²/512² surfaces are ~15 million noise-stack
//!    evaluations. So [`MaterialSystem::get_texture_set`] caches the
//!    *descriptor* — every input `build` would have consumed, resolved
//!    exactly as the source resolves it, under exactly the source's cache key
//!    — and [`TextureSet::bake`] runs the pixels when a caller wants them.
//!    Cache identity, bake count and bake order are unaffected; only *when*
//!    the arithmetic happens moves.
//! 3. **`extendMaterial` is out of slice.** `shader.js` is a separate port
//!    (it becomes WGSL in the GPU backend). What this facade owns of it is
//!    the mutable subset [`MaterialSystem::tune`] and
//!    [`MaterialSystem::set_ground_level`] write to, which is
//!    [`LiveUniforms`]; the rest of the uniform block and the `#define` set
//!    land with `shader.js`.
//!
//! Two further pieces of `index.js` have no counterpart here.
//! `applyProps` (`index.js:305-314`) is a THREE-specific guard — assigning a
//! hex over a `THREE.Color` property replaces the object and produces a black
//! material, so colour-valued props must go through `.set()`. There is no
//! THREE material here; the facade's own output is the merged [`ThreeProps`]
//! map, and whoever binds it to a renderer owns the guard. And
//! `buildDebugBoard`'s geometry (a `SphereGeometry` and a bevelled
//! `BoxGeometry`) is Three.js scene-graph construction; [`MaterialSystem::debug_board`]
//! ports its placement arithmetic and its material requests, which is the
//! part that exercises this file.

use std::any::Any;
use std::collections::{HashMap, HashSet};

use axiom_kernel::Seconds;

use crate::config::Quality;
use crate::engine::Ctx;
use crate::materials::bake::{bake, BakeDef, BakedSet, SurfaceSample};
use crate::materials::masks::BakeMaskOpts;
use crate::materials::noise::{Vec2, Vec3, Vec4};
use crate::materials::surfaces::metal::hex_to_linear_tint;
use crate::materials::surfaces::{arch, ground, metal, organic};
use crate::materials::{LibraryEntry, ALIASES, LIBRARY};
use crate::registry::{Phase, Subsystem};
use crate::rng::Rng;
use crate::weapons::geometry::Geo;
use crate::world::palette::Surface;

/// `export { bakeMasks, setMask, LIBRARY }` (`index.js:353`) — the facade
/// re-exports the mask helpers so a caller with a `materials` handle does not
/// also need a `masks` import.
pub use crate::materials::masks::{bake_masks, set_mask};

// ===========================================================================
// JavaScript number and JSON coercion.
//
// Both cache keys are built by string interpolation over JS `Number`s, so the
// port needs ECMAScript's `Number::toString`, not Rust's `Display` — they
// disagree at both ends of the range (`1e-7` vs `0.0000001`, `1e+21` vs
// twenty-two digits) and on negative zero.
// ===========================================================================

/// ECMAScript `Number::toString(x, 10)` (ECMA-262 §6.1.6.1.20), which is what
/// both `` `${n}` `` and `JSON.stringify(n)` produce for a finite number.
///
/// Rust's `Display` never uses exponent notation and prints `-0` for negative
/// zero; JavaScript switches to exponent notation outside `[1e-6, 1e21)` and
/// prints `0`. The shortest round-trip digit string is taken from Rust's
/// `LowerExp`, which uses the same shortest-representation algorithm
/// JavaScript does, and is then re-laid-out by the spec's five cases.
pub fn js_number(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    // `ToString(-0)` is `"0"`, not `"-0"`.
    if x == 0.0 {
        return "0".to_string();
    }
    let negative = x < 0.0;
    let magnitude = x.abs();

    // `{:e}` gives `d[.ddd]e<exp>` with the shortest round-tripping mantissa.
    let exponential = format!("{magnitude:e}");
    let (mantissa, exponent) = exponential
        .split_once('e')
        .expect("LowerExp always emits an exponent");
    let exponent: i32 = exponent.parse().expect("LowerExp emits a decimal exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = i32::try_from(digits.len()).expect("a shortest f64 mantissa is at most 17 digits");
    // The spec's `n`: the value is `digits * 10^(n - k)`.
    let n = exponent + 1;

    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        let (head, tail) = digits.split_at(n as usize);
        format!("{head}.{tail}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e >= 0 { '+' } else { '-' };
        let magnitude_exp = e.abs();
        if k == 1 {
            format!("{digits}e{sign}{magnitude_exp}")
        } else {
            let (head, tail) = digits.split_at(1);
            format!("{head}.{tail}e{sign}{magnitude_exp}")
        }
    };

    if negative {
        format!("-{body}")
    } else {
        body
    }
}

/// `Math.round(x)`: ties round toward `+Infinity`, unlike [`f64::round`]'s
/// ties-away-from-zero.
///
/// **Not** `floor(x + 0.5)`, which the obvious transcription reaches for and
/// which is wrong for `x = 0.49999999999999994` (the largest double below
/// `0.5`): adding `0.5` rounds up to exactly `1.0`, so `floor` yields `1`
/// where `Math.round` yields `+0`. ECMA-262 states the two sub-`0.5` clauses
/// before it mentions flooring at all, precisely to head off that double
/// rounding; the same carry can happen at larger magnitudes, hence the
/// back-out below.
///
/// Duplicated: `crate::jsmath::round` is the same function, arrived at
/// independently in a sibling slice of this port (its golden caught the naive
/// form), and `materials::masks` carries a third, still-naive copy. Once
/// `jsmath` is wired into `lib.rs` this should call it, and `masks.rs`'s
/// should too — see the notes file. It is written out here rather than
/// imported so this slice does not depend on a module that is not yet
/// declared.
use crate::jsmath::round as js_round;

/// `JSON.stringify` of a string: the two mandatory escapes, the five
/// short-form control escapes, and `\u00XX` for the rest. Non-ASCII passes
/// through, which is what `JSON.stringify` does without a replacer.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ===========================================================================
// `opts` — the caller's options bag.
// ===========================================================================

/// One value in an `opts` bag, with JavaScript's own type set.
///
/// [`OptValue::Undefined`] is not redundant with [`OptValue::Null`]: a key
/// *present* with value `undefined` still overrides the merged default (the
/// object spread copies it), still keys as the literal text `undefined` in
/// `stableKey`, and is still nullish for `??`. Two of the three behaviours
/// differ from `null`, which keys as `null`.
#[derive(Debug, Clone, PartialEq)]
pub enum OptValue {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<OptValue>),
    /// Insertion-ordered, because `JSON.stringify` does not sort object keys.
    Obj(Vec<(String, OptValue)>),
}

impl OptValue {
    /// `JSON.stringify(value)`. `None` is JavaScript's `undefined` return —
    /// the caller decides whether that becomes the text `undefined` (top
    /// level of `stableKey`), `null` (inside an array) or a dropped key
    /// (inside an object).
    pub fn to_json(&self) -> Option<String> {
        match self {
            OptValue::Undefined => None,
            OptValue::Null => Some("null".to_string()),
            OptValue::Bool(b) => Some(if *b { "true" } else { "false" }.to_string()),
            // `JSON.stringify(NaN)` and `JSON.stringify(Infinity)` are both
            // `"null"` — this is the one place JSON diverges from `${n}`.
            OptValue::Num(n) if !n.is_finite() => Some("null".to_string()),
            OptValue::Num(n) => Some(js_number(*n)),
            OptValue::Str(s) => Some(json_string(s)),
            OptValue::Arr(items) => {
                let body: Vec<String> = items
                    .iter()
                    .map(|v| v.to_json().unwrap_or_else(|| "null".to_string()))
                    .collect();
                Some(format!("[{}]", body.join(",")))
            }
            OptValue::Obj(entries) => {
                let body: Vec<String> = entries
                    .iter()
                    .filter_map(|(k, v)| v.to_json().map(|j| format!("{}:{j}", json_string(k))))
                    .collect();
                Some(format!("{{{}}}", body.join(",")))
            }
        }
    }

    /// The value as a JS number, when it is one. Used by every param that
    /// reads a scalar.
    pub fn as_num(&self) -> Option<f64> {
        match self {
            OptValue::Num(n) => Some(*n),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            OptValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            OptValue::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A numeric array, as the params want it. A non-numeric element is
    /// dropped — see [`MaterialOpts`]'s note on ill-typed options.
    fn as_num_vec(&self) -> Option<Vec<f64>> {
        match self {
            OptValue::Arr(items) => Some(items.iter().filter_map(OptValue::as_num).collect()),
            _ => None,
        }
    }

    fn as_obj(&self) -> Option<&[(String, OptValue)]> {
        match self {
            OptValue::Obj(entries) => Some(entries),
            _ => None,
        }
    }

    /// `value ?? fallback` — nullish, so `null` and `undefined` both fall
    /// back but `0` and `false` do not.
    fn is_nullish(&self) -> bool {
        matches!(self, OptValue::Undefined | OptValue::Null)
    }
}

/// The `opts` object literal callers pass to `get`/`getTextureSet`.
///
/// Insertion-ordered, exactly like a JS object: [`stable_key`] sorts a copy of
/// the top-level keys, and nested [`OptValue::Obj`]s are stringified in the
/// order they were written — which is observable, and is the difference
/// between one material and two.
///
/// **Ill-typed options.** JavaScript will happily accept `{ scale: 'big' }`
/// and carry it into the shader; this port ignores a value whose type does
/// not match the parameter (see [`MaterialOpts::apply_to_params`]) while
/// still keying on it, so the cache identity stays exact and the resolved
/// parameters stay well-typed. Nothing in the source or in the level palette
/// passes an ill-typed option.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaterialOpts {
    entries: Vec<(String, OptValue)>,
}

impl MaterialOpts {
    /// `{}`.
    pub fn new() -> Self {
        MaterialOpts::default()
    }

    /// Set one key. Re-setting an existing key keeps its original position,
    /// matching JS object-property assignment.
    pub fn set(&mut self, key: &str, value: OptValue) -> &mut Self {
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((key.to_string(), value)),
        }
        self
    }

    /// Builder form of [`MaterialOpts::set`].
    #[must_use]
    pub fn with(mut self, key: &str, value: OptValue) -> Self {
        self.set(key, value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&OptValue> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, OptValue)> + '_ {
        self.entries.iter()
    }

    /// `{ ...DEFAULT_PARAMS, ...def.mat, ...opts }`'s third spread
    /// (`index.js:188`), restricted to the keys `DEFAULT_PARAMS` declares.
    /// Anything else the caller passed — `key`, `normalScale`, `ao` in
    /// `ai/soldier.js`, and the `three`/`bake` sub-objects `get` deletes on
    /// the next two lines — lands in the cache key and nowhere else, which is
    /// exactly what the source does with it.
    fn apply_to_params(&self, p: &mut ResolvedParams) {
        for (key, value) in &self.entries {
            match key.as_str() {
                "uvMode" => {
                    if let Some(s) = value.as_str() {
                        p.uv_mode = s.to_string();
                    }
                }
                "localSpace" => set_bool(&mut p.local_space, value),
                "scale" => set_num(&mut p.scale, value),
                "offset" => set_vec(&mut p.offset, value),
                "parallax" => set_num(&mut p.parallax, value),
                "parallaxFade" => set_vec(&mut p.parallax_fade, value),
                "parallaxLayers" => set_num(&mut p.parallax_layers, value),
                "detail" => set_vec(&mut p.detail, value),
                "detailWorld" => set_num(&mut p.detail_world, value),
                "macro" => set_vec(&mut p.macro_, value),
                "macroBig" => set_vec(&mut p.macro_big, value),
                "patch" => set_vec(&mut p.patch, value),
                "cloth" => set_vec(&mut p.cloth, value),
                "macroRelief" => set_num(&mut p.macro_relief, value),
                "detile" => set_num(&mut p.detile, value),
                "weather" => set_vec(&mut p.weather, value),
                // Overwritten unconditionally by `index.js:191`; listed here
                // so the spread's shape stays recognisable.
                "groundY" => set_num(&mut p.ground_y, value),
                "wear" => set_vec(&mut p.wear, value),
                "wearMaterial" => set_vec(&mut p.wear_material, value),
                "wearColor" => set_hex(&mut p.wear_color, value),
                "dustColor" => set_hex(&mut p.dust_color, value),
                "grimeColor" => set_hex(&mut p.grime_color, value),
                "rustColor" => set_hex(&mut p.rust_color, value),
                "tint" => set_hex(&mut p.tint, value),
                "normalStrength" => set_num(&mut p.normal_strength, value),
                "roughness" => set_vec(&mut p.roughness, value),
                "aoStrength" => set_num(&mut p.ao_strength, value),
                "alphaMask" => set_bool(&mut p.alpha_mask, value),
                "vertexMasks" => set_bool(&mut p.vertex_masks, value),
                "noGrad" => set_bool(&mut p.no_grad, value),
                _ => {}
            }
        }
    }
}

fn set_num(slot: &mut f64, value: &OptValue) {
    if let Some(n) = value.as_num() {
        *slot = n;
    }
}

fn set_bool(slot: &mut bool, value: &OptValue) {
    if let Some(b) = value.as_bool() {
        *slot = b;
    }
}

fn set_vec(slot: &mut Vec<f64>, value: &OptValue) {
    if let Some(v) = value.as_num_vec() {
        *slot = v;
    }
}

fn set_hex(slot: &mut u32, value: &OptValue) {
    if let Some(n) = value.as_num() {
        // A JS hex literal is a Number; the source hands it straight to
        // `new THREE.Color(n)`, which truncates to 24 bits.
        *slot = n as u32;
    }
}

/// `stableKey(opts)` (`index.js:316-320`).
///
/// ```js
/// const keys = Object.keys(opts).sort();
/// if (!keys.length) return '';
/// return keys.map((k) => `${k}=${JSON.stringify(opts[k])}`).join(',');
/// ```
///
/// `Array.prototype.sort()` with no comparator orders by UTF-16 code unit;
/// Rust's `str` `Ord` orders by UTF-8 byte, and the two agree for every code
/// point below `U+10000`. Option keys are ASCII identifiers.
///
/// `JSON.stringify(undefined)` returns the *value* `undefined`, which the
/// template literal then coerces to the text `"undefined"` — the one place a
/// key can read as an unquoted word.
pub fn stable_key(opts: &MaterialOpts) -> String {
    let mut keys: Vec<&str> = opts.entries.iter().map(|(k, _)| k.as_str()).collect();
    keys.sort_unstable();
    if keys.is_empty() {
        return String::new();
    }
    keys.into_iter()
        .map(|k| {
            let value = opts.get(k).expect("key came from this map");
            let json = value.to_json().unwrap_or_else(|| "undefined".to_string());
            format!("{k}={json}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

// ===========================================================================
// Resolved material parameters — `{ ...DEFAULT_PARAMS, ...def.mat, ...opts }`.
// ===========================================================================

/// The merged parameter set `extendMaterial` is handed (`index.js:188-191`),
/// i.e. `DEFAULT_PARAMS` (`shader.js:697-777`) with the library entry's `mat`
/// block and then the caller's `opts` laid over it.
///
/// Every array is a `Vec<f64>` rather than a fixed array because JavaScript
/// arrays are variably long and the source relies on it: the level palette's
/// `window_glass` passes `roughness: [0.3, 0.06]`, two elements, and
/// `shader.js` reads `p.roughness[2] ?? DEFAULT_PARAMS.roughness[2]` to
/// absorb the missing third.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedParams {
    /// `'planar'` | `'triplanar'` | `'mesh'`. A string, not an enum: the
    /// source compares it for equality and treats anything unrecognised as
    /// planar, and it is never used as a table index.
    pub uv_mode: String,
    pub local_space: bool,
    pub scale: f64,
    pub offset: Vec<f64>,
    pub parallax: f64,
    pub parallax_fade: Vec<f64>,
    pub parallax_layers: f64,
    pub detail: Vec<f64>,
    pub detail_world: f64,
    pub macro_: Vec<f64>,
    pub macro_big: Vec<f64>,
    pub patch: Vec<f64>,
    pub cloth: Vec<f64>,
    pub macro_relief: f64,
    pub detile: f64,
    pub weather: Vec<f64>,
    pub ground_y: f64,
    pub wear: Vec<f64>,
    pub wear_material: Vec<f64>,
    pub wear_color: u32,
    pub dust_color: u32,
    pub grime_color: u32,
    pub rust_color: u32,
    pub tint: u32,
    pub normal_strength: f64,
    pub roughness: Vec<f64>,
    pub ao_strength: f64,
    pub alpha_mask: bool,
    pub vertex_masks: bool,
    pub no_grad: bool,
}

impl Default for ResolvedParams {
    /// `DEFAULT_PARAMS` (`shader.js:697-777`), field for field and in source
    /// order.
    fn default() -> Self {
        ResolvedParams {
            uv_mode: "planar".to_string(),
            local_space: false,
            scale: 2.0,
            offset: vec![0.0, 0.0],
            parallax: 0.0,
            parallax_fade: vec![6.0, 14.0],
            parallax_layers: 22.0,
            detail: vec![11.0, 0.55, 0.35, 16.0],
            detail_world: 0.26,
            macro_: vec![0.045, 0.35, 0.1, 0.35],
            macro_big: vec![1.0, 0.0, 0.03, 0.0],
            patch: vec![0.0, 2.6, 0.12, -0.08],
            cloth: vec![0.0, 1.0, 0.0, 0.0],
            macro_relief: 0.0,
            detile: 0.0,
            weather: vec![0.35, 0.3, 0.55, 0.4],
            ground_y: 0.0,
            wear: vec![0.5, 0.7, 0.5, 0.0],
            wear_material: vec![0.42, 0.0, 0.0, 0.5],
            wear_color: 0x8d_8b_86,
            dust_color: 0x6b_61_54,
            grime_color: 0x2a_26_20,
            rust_color: 0x6d_3a_1c,
            tint: 0xff_ff_ff,
            normal_strength: 1.0,
            roughness: vec![1.0, 0.0, 0.06],
            ao_strength: 1.0,
            alpha_mask: false,
            vertex_masks: false,
            no_grad: false,
        }
    }
}

/// `{ ...DEFAULT_PARAMS, ...def.mat }` — the second spread, from the ported
/// [`crate::materials::MatParams`].
///
/// Those fields are stored as `f32` while the source authors and computes in
/// `f64`, so `0.085` arrives here as `0.085000000894069671875`. That is a
/// property of the existing library port, not of this merge; see the notes
/// file for the one-line fix and why this module does not make it.
fn library_params(entry: &LibraryEntry) -> ResolvedParams {
    let m = &entry.mat;
    let mut p = ResolvedParams::default();
    if let Some(mode) = m.uv_mode {
        p.uv_mode = mode.to_string();
    }
    p.scale = f64::from(m.scale);
    if let Some(v) = m.parallax {
        p.parallax = f64::from(v);
    }
    if let Some(v) = m.parallax_layers {
        p.parallax_layers = f64::from(v);
    }
    if let Some(v) = m.detile {
        p.detile = f64::from(v);
    }
    if let Some(v) = m.detail {
        p.detail = widen4(v);
    }
    if let Some(v) = m.macro_ {
        p.macro_ = widen4(v);
    }
    if let Some(v) = m.macro_big {
        p.macro_big = widen4(v);
    }
    if let Some(v) = m.patch {
        p.patch = widen4(v);
    }
    if let Some(v) = m.cloth {
        p.cloth = widen4(v);
    }
    if let Some(v) = m.weather {
        p.weather = widen4(v);
    }
    if let Some(v) = m.wear_material {
        p.wear_material = widen4(v);
    }
    if let Some(v) = m.wear_color {
        p.wear_color = v;
    }
    if let Some(v) = m.dust_color {
        p.dust_color = v;
    }
    if let Some(v) = m.grime_color {
        p.grime_color = v;
    }
    if let Some(v) = m.tint {
        p.tint = v;
    }
    if let Some(v) = m.normal_strength {
        p.normal_strength = f64::from(v);
    }
    if let Some(v) = m.roughness {
        p.roughness = v.iter().copied().map(f64::from).collect();
    }
    if let Some(v) = m.macro_relief {
        p.macro_relief = f64::from(v);
    }
    if let Some(v) = m.alpha_mask {
        p.alpha_mask = v;
    }
    p
}

fn widen4(v: [f32; 4]) -> Vec<f64> {
    v.iter().copied().map(f64::from).collect()
}

// ===========================================================================
// `three` props.
// ===========================================================================

/// A value in a `three` block. Every one in the library and in the level
/// palette is a JS number (including hex colours and `THREE.DoubleSide`) or a
/// boolean.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThreeValue {
    Num(f64),
    Bool(bool),
}

/// `{ ...(def.three ?? {}), ...(opts.three ?? {}) }` with `physical` removed
/// (`index.js:193-195`).
///
/// Insertion-ordered for the same reason [`MaterialOpts`] is, though this map
/// is never stringified — the source never puts `threeProps` in a cache key,
/// only the caller's raw `opts.three`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThreeProps {
    entries: Vec<(String, ThreeValue)>,
}

impl ThreeProps {
    fn set(&mut self, key: &str, value: ThreeValue) {
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((key.to_string(), value)),
        }
    }

    fn remove(&mut self, key: &str) -> Option<ThreeValue> {
        let at = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(at).1)
    }

    pub fn get(&self, key: &str) -> Option<ThreeValue> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }

    pub fn num(&self, key: &str) -> Option<f64> {
        match self.get(key) {
            Some(ThreeValue::Num(n)) => Some(n),
            _ => None,
        }
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key) {
            Some(ThreeValue::Bool(b)) => Some(b),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, ThreeValue)> + '_ {
        self.entries.iter()
    }

    /// The finished material's `transparent`, collapsing `index.js:213` and
    /// `applyProps`.
    ///
    /// `if (!(p.alphaMask || threeProps.transparent)) mat.transparent = false;`
    /// writes `false` over a `THREE.Material` whose constructor already
    /// defaulted it to `false`, so the line is inert; the value that survives
    /// is whatever `applyProps` then copies out of `threeProps`. Ported as
    /// the outcome, with the dead line named rather than dropped.
    pub fn transparent(&self) -> bool {
        self.bool("transparent").unwrap_or(false)
    }
}

/// `library.js:376` — `glass`'s `three.transparent: true`, which
/// [`crate::materials::ThreeOptions`] has no field for.
///
/// The structurally correct fix is a `transparent: Option<bool>` on that
/// struct with `Some(true)` on the `glass` entry; this slice is not permitted
/// to edit `materials/mod.rs`, so the missing datum is named here, exactly
/// once, and reported. Delete this constant the moment the field exists.
const MISSING_LIBRARY_THREE: &[(&str, &str, ThreeValue)] =
    &[("glass", "transparent", ThreeValue::Bool(true))];

/// `def.three` as an ordered map. The source's literal key order is not
/// observable — `def.three` is never stringified, only merged — so this emits
/// in [`crate::materials::ThreeOptions`] field order.
fn library_three(entry: &LibraryEntry) -> ThreeProps {
    let mut props = ThreeProps::default();
    if let Some(t) = entry.three {
        let numbers: [(&str, Option<f64>); 10] = [
            ("side", t.side.map(f64::from)),
            ("anisotropy", t.anisotropy.map(f64::from)),
            ("anisotropyRotation", t.anisotropy_rotation.map(f64::from)),
            ("sheen", t.sheen.map(f64::from)),
            ("sheenRoughness", t.sheen_roughness.map(f64::from)),
            ("sheenColor", t.sheen_color.map(f64::from)),
            ("alphaTest", t.alpha_test.map(f64::from)),
            ("opacity", t.opacity.map(f64::from)),
            ("envMapIntensity", t.env_map_intensity.map(f64::from)),
            ("ior", t.ior.map(f64::from)),
        ];
        for (key, value) in numbers {
            if let Some(v) = value {
                props.set(key, ThreeValue::Num(v));
            }
        }
        if let Some(v) = t.specular_intensity {
            props.set("specularIntensity", ThreeValue::Num(f64::from(v)));
        }
        if let Some(v) = t.physical {
            props.set("physical", ThreeValue::Bool(v));
        }
        if let Some(v) = t.double_sided {
            // No library entry sets this; the ported struct carries it and
            // `side` both. Emitted for completeness.
            props.set("doubleSided", ThreeValue::Bool(v));
        }
        if let Some(v) = t.depth_write {
            props.set("depthWrite", ThreeValue::Bool(v));
        }
    }
    for (name, key, value) in MISSING_LIBRARY_THREE {
        if entry.name == *name {
            props.set(key, *value);
        }
    }
    props
}

/// `index.js:193-195`. Returns the merged props with `physical` stripped, and
/// whether the material is a `MeshPhysicalMaterial`.
///
/// `threeProps.physical === true` is a strict comparison, so a truthy-but-not-
/// `true` value (`1`) selects the standard material.
fn merge_three(entry: &LibraryEntry, opts: &MaterialOpts) -> (ThreeProps, bool) {
    let mut props = library_three(entry);
    if let Some(entries) = opts.get("three").and_then(OptValue::as_obj) {
        for (key, value) in entries {
            match value {
                OptValue::Num(n) => props.set(key, ThreeValue::Num(*n)),
                OptValue::Bool(b) => props.set(key, ThreeValue::Bool(*b)),
                // See `MaterialOpts`'s note on ill-typed options.
                _ => {}
            }
        }
    }
    let physical = props.remove("physical") == Some(ThreeValue::Bool(true));
    (props, physical)
}

// ===========================================================================
// The texture set.
// ===========================================================================

/// `{ ...def.bake, ...(opts.bake ?? {}) }` with `size` already through
/// [`MaterialSystem::size_of`] (`index.js:129-130`).
///
/// `seed`, `tint_a`, `tint_b` and `param` are `Option` because a caller can
/// clear an inherited value with `{ bake: { param: undefined } }`, and the
/// cleared and defaulted states key differently.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBake {
    pub size: u32,
    pub world_size: Option<f64>,
    pub relief: Option<f64>,
    pub seed: Option<f64>,
    pub tint_a: Option<f64>,
    pub tint_b: Option<f64>,
    pub param: Option<Vec<f64>>,
}

/// `_bakeKey(name, bake)` (`index.js:117-121`).
///
/// ```js
/// `${name}|${bake.size}|${bake.seed}|${bake.tintA ?? ''}|${bake.tintB ?? ''}|${(bake.param ?? []).join('_')}`
/// ```
///
/// Note what is **absent**: `worldSize` and `relief`. Both are real inputs to
/// the bake — `relief / worldSize` is the Sobel's slope — and neither is in
/// the key, so an override of either silently returns whatever set the
/// unmodified key already names. Ported as the source has it; pinned as a
/// defect.
///
/// `Array.prototype.join` coerces with `String(v)`, not `JSON.stringify`, so a
/// `null` or `undefined` element joins as the empty string. Every array in the
/// library is four finite numbers.
pub fn bake_key(name: &str, bake: &ResolvedBake) -> String {
    let seed = bake
        .seed
        .map_or_else(|| "undefined".to_string(), js_number);
    let tint_a = bake.tint_a.map_or_else(String::new, js_number);
    let tint_b = bake.tint_b.map_or_else(String::new, js_number);
    let param = bake.param.as_ref().map_or_else(String::new, |p| {
        p.iter()
            .map(|v| js_number(*v))
            .collect::<Vec<_>>()
            .join("_")
    });
    format!("{name}|{}|{seed}|{tint_a}|{tint_b}|{param}", bake.size)
}

/// `TextureForge.build`'s return value (`generator.js:313-320`) plus the
/// inputs that produced it, and `set.name = key` (`index.js:149`).
///
/// The pixels are not here — see seam 2 in the module doc. [`TextureSet::bake`]
/// produces them.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureSet {
    /// The resolved library key. `set.name`, `index.js:149`.
    pub name: String,
    /// [`LibraryEntry::generator`] — which `owSurface` body to evaluate.
    pub generator: &'static str,
    pub size: u32,
    /// `def.worldSize ?? 2` (`generator.js:318`).
    pub world_size: f64,
    /// `def.relief ?? 0.02` (`generator.js:319`).
    pub relief: f64,
    /// `bake.seed ?? 1` (`index.js:142`). Note the source then passes it
    /// through `generator.js`'s own `def.seed ?? 0`, which can never fire.
    pub seed: f64,
    pub tint_a: Option<u32>,
    pub tint_b: Option<u32>,
    pub param: Option<Vec<f64>>,
}

impl TextureSet {
    /// Run the CPU bake for this set — [`super::bake::bake`] over the surface
    /// [`Self::generator`] names.
    ///
    /// `linear_albedo: false, want_orm: true, want_normal: true`: `index.js`
    /// passes none of the three flags, so `generator.js`'s `!== false` /
    /// `!== true` defaults all apply.
    pub fn bake(&self) -> BakedSet {
        self.bake_at(self.size, true, true)
    }

    /// [`Self::bake`] at an explicit `size`, and with the ORM and normal passes
    /// individually switchable.
    ///
    /// **Not a variant the source has** — `index.js` always asks for all three
    /// maps at the library's own size. It exists because the two knobs it
    /// exposes are the only two that change what a CPU bake *costs*: `size`
    /// quadratically, and each output pass linearly (`bake` evaluates the
    /// surface once per pass per texel, and the normal pass costs a second
    /// full evaluation for the scratch height field). At the library's
    /// authored sizes a full nineteen-surface CPU bake is minutes of work — see
    /// `materials::upload`, which is the only caller and documents why.
    ///
    /// Nothing else moves: same generator, same seed, same tints, same param,
    /// same `linear_albedo: false`. A `bake_at(self.size, true, true)` is
    /// bit-identical to [`Self::bake`], which is how [`Self::bake`] is now
    /// implemented.
    pub fn bake_at(&self, size: u32, want_orm: bool, want_normal: bool) -> BakedSet {
        let tint_a = self.tint_a.map_or(Vec3::new(1.0, 1.0, 1.0), hex_to_linear_tint);
        let tint_b = self.tint_b.map_or(Vec3::new(1.0, 1.0, 1.0), hex_to_linear_tint);
        let param = self.param.as_ref().map_or(Vec4::new(0.0, 0.0, 0.0, 0.0), |p| {
            Vec4::new(
                p.first().copied().unwrap_or(0.0),
                p.get(1).copied().unwrap_or(0.0),
                p.get(2).copied().unwrap_or(0.0),
                p.get(3).copied().unwrap_or(0.0),
            )
        });
        let generator = self.generator;
        let seed = self.seed;
        let surface = move |uv: Vec2| sample_surface(generator, uv, seed, tint_a, tint_b, param);
        bake(&BakeDef {
            surface: &surface,
            size,
            world_size: self.world_size as f32,
            relief: self.relief as f32,
            linear_albedo: false,
            want_orm,
            want_normal,
        })
    }
}

/// `def.glsl` -> the ported `owSurface` body (`library.js:2-5` names the four
/// generator files; `super::surfaces` holds them).
///
/// Dispatch is on the generator's **name**, deliberately: an enum indexing a
/// recipe table is the shape that silently reindexed every per-surface audio
/// recipe earlier in this port, and there is nothing here an index buys.
///
/// # Panics
///
/// On a generator name no `surfaces` module implements. Every
/// [`LibraryEntry::generator`] in [`LIBRARY`] resolves — pinned by a test.
pub fn sample_surface(
    generator: &str,
    uv: Vec2,
    seed: f64,
    tint_a: Vec3,
    tint_b: Vec3,
    param: Vec4,
) -> SurfaceSample {
    match generator {
        // `concrete` and `concrete_floor` share one body, selected by
        // `uParam.x`/`uParam.y` (`surfaces-arch.js`).
        "concrete" => arch::concrete_surface(uv, seed, param),
        "brick" => arch::brick_surface(uv, seed),
        "plaster" => arch::plaster_surface(uv, seed),
        "tile" => arch::tile_surface(uv, seed),
        "asphalt" => ground::asphalt(uv, seed),
        "sand" => ground::sand(uv, seed),
        "dirt" => ground::dirt(uv, seed),
        "gravel" => ground::gravel(uv, seed),
        "metal_rust" => metal::metal_rust(uv, seed),
        "metal_painted" => metal::metal_painted(uv, seed, tint_a, param.z),
        "metal_brushed" => metal::metal_brushed(uv, seed),
        "corrugated" => metal::corrugated(uv, seed),
        "wood" => organic::wood(uv, seed),
        "fabric" => organic::fabric(uv, seed, tint_a, tint_b),
        "burlap" => organic::burlap(uv, seed),
        "foliage" => organic::foliage(uv, seed),
        "rubber" => organic::rubber(uv, seed),
        "glass" => organic::glass(uv, seed),
        other => panic!("materials: no owSurface generator named \"{other}\""),
    }
}

// ===========================================================================
// The material.
// ===========================================================================

/// The subset of `extendMaterial`'s uniform block this facade owns: the six
/// [`MaterialSystem::tune`] and [`MaterialSystem::set_ground_level`] write to.
/// The rest of the block, and the `#define` set, belong to the `shader.js`
/// slice — see seam 3 in the module doc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveUniforms {
    /// `[tileScale, tileScale, offset.x, offset.y]` (`shader.js:814`).
    pub tile: [f64; 4],
    /// `col(p.tint)` — a `THREE.Color` from a hex is sRGB-decoded into the
    /// linear working space, so this is linear, not the raw hex.
    pub tint: Vec3,
    /// `[parallax, parallaxFade.0, parallaxFade.1, parallaxLayers]`.
    pub parallax: [f64; 4],
    pub ground_y: f64,
    pub normal_amp: f64,
    pub weather: [f64; 4],
}

/// `p.uvMode === 'mesh' ? p.scale : 1 / p.scale` (`shader.js:794`, and again
/// inside `tune`, `index.js:247`). Mesh-UV mode treats `scale` as a repeat
/// count; the projected modes treat it as metres per tile.
fn tile_scale(uv_mode: &str, scale: f64) -> f64 {
    if uv_mode == "mesh" {
        scale
    } else {
        1.0 / scale
    }
}

fn take4(v: &[f64]) -> [f64; 4] {
    let mut out = [0.0; 4];
    for (slot, value) in out.iter_mut().zip(v.iter()) {
        *slot = *value;
    }
    out
}

impl LiveUniforms {
    fn from_params(p: &ResolvedParams) -> Self {
        let s = tile_scale(&p.uv_mode, p.scale);
        LiveUniforms {
            tile: [
                s,
                s,
                p.offset.first().copied().unwrap_or(0.0),
                p.offset.get(1).copied().unwrap_or(0.0),
            ],
            tint: hex_to_linear_tint(p.tint),
            parallax: [
                p.parallax,
                p.parallax_fade.first().copied().unwrap_or(0.0),
                p.parallax_fade.get(1).copied().unwrap_or(0.0),
                p.parallax_layers,
            ],
            ground_y: p.ground_y,
            normal_amp: p.normal_strength,
            weather: take4(&p.weather),
        }
    }
}

/// `tune(material, changes)`'s `changes` bag (`index.js:243-257`). Every
/// field is `!== undefined`-gated in the source, which is exactly `Option`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TuneChanges {
    pub scale: Option<f64>,
    pub tint: Option<u32>,
    pub parallax: Option<f64>,
    pub ground_y: Option<f64>,
    pub normal_strength: Option<f64>,
    pub weather: Option<Vec<f64>>,
}

/// One entry of `_materials` — the material `get()` built, as data.
///
/// The source builds a `THREE.MeshStandardMaterial`/`MeshPhysicalMaterial`
/// here. There is no THREE material in this crate, so what is stored is the
/// *description*: which constructor, which parameters, which THREE props, and
/// which texture set. Binding that to a renderer is the caller's job — the
/// same split every other ported subsystem in this crate makes at the
/// platform edge.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialDef {
    /// The resolved library key.
    pub key: String,
    /// `mat.name = matKey` (`index.js:204`) — this entry's cache key.
    pub mat_key: String,
    /// `usePhysical`: `MeshPhysicalMaterial` rather than
    /// `MeshStandardMaterial`.
    pub physical: bool,
    pub params: ResolvedParams,
    pub three: ThreeProps,
    /// `if (p.vertexMasks) mat.vertexColors = true;` (`index.js:218`).
    pub vertex_colors: bool,
    /// The `_sets` key this material samples, or `None` when the bake was
    /// unavailable (`index.js:206`, `if (set)`).
    pub set_key: Option<String>,
    /// `None` when there was no set: `extendMaterial` is only called
    /// `if (set)` (`index.js:221`), so a textureless material has no uniform
    /// block at all — which is why `tune` and `setGroundLevel` skip it.
    pub uniforms: Option<LiveUniforms>,
}

impl MaterialDef {
    /// The constructor's `transparent`, after `applyProps`. See
    /// [`ThreeProps::transparent`].
    pub fn transparent(&self) -> bool {
        self.three.transparent()
    }
}

// ===========================================================================
// The forge.
// ===========================================================================

/// What `TextureForge` reads off a `THREE.WebGLRenderer`
/// (`generator.js:147-150`): one number, and only to clamp the anisotropy
/// request. Everything else it touches is render-target plumbing with no CPU
/// analogue.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RendererCaps {
    /// `renderer.capabilities.getMaxAnisotropy?.()`. `None` is the source's
    /// missing-method arm, which falls back to 8.
    pub max_anisotropy: Option<f64>,
}

/// `TextureForge`, reduced to the state the facade observes: the resolved
/// anisotropy and the scratch height targets the release timer frees.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Forge {
    /// `Math.min(opts.anisotropy ?? 8, renderer.capabilities.getMaxAnisotropy?.() ?? 8)`.
    pub anisotropy: f64,
    /// `_heightRTs`, a `Map` keyed by size (`generator.js:174-198`). Held as
    /// the ordered key set: the port has no render targets, and what the
    /// facade actually cares about is *which sizes are allocated* and *when
    /// they are freed*.
    height_targets: Vec<u32>,
}

impl Forge {
    /// `_heightRT(size)` — allocate on first use, reuse thereafter.
    fn height_rt(&mut self, size: u32) {
        if !self.height_targets.contains(&size) {
            self.height_targets.push(size);
        }
    }

    /// `releaseScratch()` (`generator.js:333-345`), returning the freed count.
    fn release_scratch(&mut self) -> usize {
        let freed = self.height_targets.len();
        self.height_targets.clear();
        freed
    }

    /// The scratch height targets currently allocated, in allocation order.
    pub fn height_targets(&self) -> &[u32] {
        &self.height_targets
    }
}

/// `_shared` (`index.js:84-88`) — the two maps every material samples.
///
/// Descriptors, not pixels, for the same reason [`TextureSet`] is:
/// `buildDetail` at 1024² is a million evaluations of the detail noise stack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SharedMaps {
    /// `this._size(1024)` — quality-scaled.
    pub detail_size: u32,
    /// A literal `256`. **Not** `this._size(256)`: the macro map's resolution
    /// does not follow the quality preset (`index.js:83`).
    pub macro_size: u32,
}

impl SharedMaps {
    /// `TextureForge.buildDetail(size, seed = 1)` — the shared micro-detail
    /// normal + albedo.
    pub fn build_detail(self) -> BakedSet {
        super::bake::build_detail(self.detail_size, 1.0)
    }

    /// `TextureForge.buildMacro(size, seed = 2)` — the shared 4-band
    /// low-frequency variation map.
    pub fn build_macro(self) -> BakedSet {
        super::bake::build_macro(self.macro_size, 2.0)
    }
}

// ===========================================================================
// Debug board.
// ===========================================================================

/// `buildDebugBoard(system, { columns, spacing, radius })`'s destructuring
/// defaults (`index.js:327`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DebugBoardOpts {
    pub columns: usize,
    pub spacing: f64,
    /// The sphere's radius. Carried because it is part of the call shape, and
    /// unread because this port does not build the `SphereGeometry` it sizes.
    pub radius: f64,
}

impl Default for DebugBoardOpts {
    fn default() -> Self {
        DebugBoardOpts {
            columns: 6,
            spacing: 1.25,
            radius: 0.42,
        }
    }
}

/// One library entry's cell on the debug board: a sphere at `z = 0` and a
/// bevelled panel behind it at `z = -0.9`, with the two different material
/// requests the source makes for them.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardItem {
    pub index: usize,
    pub name: &'static str,
    pub sphere_position: [f64; 3],
    pub panel_position: [f64; 3],
    /// `system.get(name, { vertexMasks: false })`.
    pub sphere_material: String,
    /// `system.get(name, { vertexMasks: true, localSpace: true })`.
    pub panel_material: String,
}

// ===========================================================================
// The system.
// ===========================================================================

/// `resolveName(name)` (`library.js:403-405`): a real library key passes
/// through, anything else goes through the alias table, and an unknown name
/// comes back unchanged for `_resolve` to reject.
pub fn resolve_name(name: &str) -> &str {
    if library_entry(name).is_some() {
        return name;
    }
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map_or(name, |(_, target)| *target)
}

fn library_entry(name: &str) -> Option<&'static LibraryEntry> {
    LIBRARY.iter().find(|e| e.name == name)
}

/// An insertion-ordered map, which is what a JS `Map` is. `setGroundLevel`
/// iterates `_materials.values()`, so the order is part of the contract even
/// though the operation itself is order-independent.
#[derive(Debug, Clone)]
struct OrderedMap<V> {
    order: Vec<String>,
    values: HashMap<String, V>,
}

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        OrderedMap {
            order: Vec::new(),
            values: HashMap::new(),
        }
    }
}

impl<V> OrderedMap<V> {
    fn get(&self, key: &str) -> Option<&V> {
        self.values.get(key)
    }

    fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    fn insert(&mut self, key: String, value: V) {
        if !self.values.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.values.insert(key, value);
    }

    fn keys(&self) -> &[String] {
        &self.order
    }

    fn len(&self) -> usize {
        self.order.len()
    }

    fn clear(&mut self) {
        self.order.clear();
        self.values.clear();
    }
}

/// The materials subsystem — `MaterialSystem` (`index.js:31-298`).
#[derive(Debug)]
pub struct MaterialSystem {
    /// `opts.renderer ?? null` (`index.js:37`). See seam 1.
    injected_renderer: Option<RendererCaps>,
    sets: OrderedMap<TextureSet>,
    materials: OrderedMap<MaterialDef>,
    forge: Option<Forge>,
    shared: Option<SharedMaps>,
    ground_y: f64,
    built: bool,
    warned: bool,
    quality: f64,
    idle: f64,
    scratch_freed: bool,
    /// `this._anisotropy`, set by `init` from `ctx.config.q.anisotropy`.
    /// `None` until then, which is the source's `undefined` and coerces to 8
    /// through `?? 8` in the forge.
    anisotropy: Option<f64>,
    /// `this._missing`, the once-per-name gate on the unknown-surface warning.
    missing: HashSet<String>,
    /// Every `console.warn` the source would have emitted, in order. Kept
    /// rather than printed so the warning transcript is testable — and
    /// because a layer or module may not `println!` at all.
    pub warnings: Vec<String>,
    /// How many texture sets have actually been built. The source has no such
    /// counter; this is the observable the golden's cache script pins.
    bakes: u32,
}

impl Default for MaterialSystem {
    fn default() -> Self {
        MaterialSystem::new(None)
    }
}

impl MaterialSystem {
    /// `new MaterialSystem(opts)` (`index.js:35-49`) — every field's
    /// constructor value, in source order.
    pub fn new(renderer: Option<RendererCaps>) -> Self {
        MaterialSystem {
            injected_renderer: renderer,
            sets: OrderedMap::default(),
            materials: OrderedMap::default(),
            forge: None,
            shared: None,
            ground_y: 0.0,
            built: false,
            warned: false,
            quality: 1.0,
            idle: 0.0,
            scratch_freed: false,
            anisotropy: None,
            missing: HashSet::new(),
            warnings: Vec::new(),
            bakes: 0,
        }
    }

    /// Seam 1: hand the system a renderer after construction, which is what
    /// `_renderer()`'s `ctx.peek('render')` arm does in the source once the
    /// render subsystem has initialised.
    pub fn set_renderer(&mut self, renderer: RendererCaps) {
        self.injected_renderer = Some(renderer);
    }

    /// `init(ctx)`'s body (`index.js:51-59`) with the two values it reads off
    /// `ctx` passed directly. [`Subsystem::init`] is a thin wrapper over this;
    /// a caller driving the system without an engine (the source's "standalone
    /// harness" case, `index.js:36`) uses it too.
    pub fn configure(&mut self, quality: Quality, anisotropy: u32) -> bool {
        self.anisotropy = Some(f64::from(anisotropy));
        // Texture budget scales with the quality preset; 1K is the reference.
        self.quality = quality_scalar(quality);
        self.try_build()
    }

    // --------------------------------------------------------- internals --

    /// `_renderer()` (`index.js:62-66`).
    fn renderer(&self) -> Option<RendererCaps> {
        self.injected_renderer
    }

    /// `_tryBuild()` (`index.js:68-93`).
    fn try_build(&mut self) -> bool {
        if self.built {
            return true;
        }
        let Some(caps) = self.renderer() else {
            if !self.warned {
                self.warnings.push(
                    "[materials] no WebGLRenderer available yet — deferring texture bake"
                        .to_string(),
                );
                self.warned = true;
            }
            return false;
        };

        let anisotropy = f64::min(
            self.anisotropy.unwrap_or(8.0),
            caps.max_anisotropy.unwrap_or(8.0),
        );
        let mut forge = Forge {
            anisotropy,
            height_targets: Vec::new(),
        };
        // 1K, not 512: the micro tooth is 1.6-4 mm over a 0.25 m tile, which
        // needs ~6 texels per grain to survive mip 1 (`index.js:80-82`).
        let detail_size = self.size_of(1024.0);
        // `buildDetail` bakes a normal, so it takes a scratch height target;
        // `buildMacro` does not (`generator.js:365-381`, `normal: false`).
        forge.height_rt(detail_size);
        self.shared = Some(SharedMaps {
            detail_size,
            macro_size: 256,
        });
        self.forge = Some(forge);
        self.built = true;
        true
    }

    /// `_size(base)` (`index.js:95-99`).
    ///
    /// ```js
    /// const s = Math.max(128, Math.round((base * this._quality) / 128) * 128);
    /// return 1 << Math.round(Math.log2(s));
    /// ```
    ///
    /// Snap to a 128 multiple, floor at 128, then snap **again** to the
    /// nearest power of two so mip chains stay clean. The second snap is what
    /// makes `medium` (0.75) a no-op at every size the library actually uses:
    /// `round(log2(768)) == 10`, so a 1024 base scaled to 768 comes back
    /// 1024. `low` (0.5) is an exact halving and does reduce.
    ///
    /// `1 << n` is a JavaScript shift, which takes its count mod 32.
    pub fn size_of(&self, base: f64) -> u32 {
        let s = f64::max(128.0, js_round(base * self.quality / 128.0) * 128.0);
        let n = js_round(s.log2());
        1u32 << ((n as i64).rem_euclid(32) as u32)
    }

    /// `_resolve(name)` (`index.js:106-115`). An unknown name warns **once**
    /// and falls back to concrete rather than throwing — a typo in one
    /// subsystem must not take the whole boot down.
    pub fn resolve(&mut self, name: &str) -> String {
        let key = resolve_name(name);
        if library_entry(key).is_some() {
            return key.to_string();
        }
        if self.missing.insert(name.to_string()) {
            self.warnings.push(format!(
                "[materials] unknown surface \"{name}\" — falling back to concrete"
            ));
        }
        "concrete".to_string()
    }

    /// `{ ...def.bake, ...(opts.bake ?? {}) }` then `bake.size = this._size(bake.size)`
    /// (`index.js:129-130`).
    fn resolve_bake(&self, entry: &LibraryEntry, opts: &MaterialOpts) -> ResolvedBake {
        let b = &entry.bake;
        let mut out = ResolvedBake {
            // Pre-`_size`; scaled at the end, as the source does.
            size: b.size,
            world_size: Some(f64::from(b.world_size)),
            relief: Some(f64::from(b.relief)),
            seed: Some(f64::from(b.seed)),
            tint_a: b.tint_a.map(f64::from),
            tint_b: b.tint_b.map(f64::from),
            param: Some(b.param.iter().copied().map(f64::from).collect()),
        };
        // `library.js` gives `param` to exactly two entries; the ported
        // `BakeParams` stores a zero array for the other seventeen, which
        // would key as `0_0_0_0` instead of the source's empty string.
        // `param` is absent, not zero, unless the entry declares one.
        if b.param == [0.0; 4] && !LIBRARY_HAS_PARAM.contains(&entry.name) {
            out.param = None;
        }

        let mut base_size = f64::from(b.size);
        if let Some(entries) = opts.get("bake").and_then(OptValue::as_obj) {
            for (key, value) in entries {
                match key.as_str() {
                    "size" => {
                        if let Some(n) = value.as_num() {
                            base_size = n;
                        }
                    }
                    "worldSize" => out.world_size = value.as_num(),
                    "relief" => out.relief = value.as_num(),
                    "seed" => out.seed = value.as_num(),
                    "tintA" => out.tint_a = value.as_num(),
                    "tintB" => out.tint_b = value.as_num(),
                    "param" => out.param = value.as_num_vec(),
                    _ => {}
                }
            }
        }
        out.size = self.size_of(base_size);
        out
    }

    /// `getTextureSet(name, opts)` (`index.js:124-154`), split so `get()` can
    /// reuse it without re-borrowing: returns the `_sets` key, or `None` when
    /// `_tryBuild` failed.
    fn ensure_texture_set(&mut self, key: &str, opts: &MaterialOpts) -> Option<String> {
        let entry = library_entry(key).expect("caller resolved this key");
        if !self.try_build() {
            return None;
        }
        let bake = self.resolve_bake(entry, opts);
        let cache_key = bake_key(key, &bake);
        if self.sets.contains(&cache_key) {
            return Some(cache_key);
        }

        // A MISS restarts the scratch-release clock; a hit does not (the
        // reset at `index.js:136-137` sits after the early return above).
        self.idle = 0.0;
        self.scratch_freed = false;

        let set = TextureSet {
            name: key.to_string(),
            generator: entry.generator,
            size: bake.size,
            world_size: bake.world_size.unwrap_or(2.0),
            relief: bake.relief.unwrap_or(0.02),
            seed: bake.seed.unwrap_or(1.0),
            tint_a: bake.tint_a.map(|v| v as u32),
            tint_b: bake.tint_b.map(|v| v as u32),
            param: bake.param.clone(),
        };
        if let Some(forge) = self.forge.as_mut() {
            forge.height_rt(bake.size);
        }
        self.bakes += 1;
        self.sets.insert(cache_key.clone(), set);
        Some(cache_key)
    }

    // --------------------------------------------------------------- API --

    /// `getTextureSet(name, opts)` (`index.js:124-154`).
    ///
    /// The set is keyed on the **bake** alone, so two materials that differ
    /// only in a shader parameter — a tint, a scale, a weather vector — share
    /// one entry here.
    pub fn get_texture_set(&mut self, name: &str, opts: &MaterialOpts) -> Option<&TextureSet> {
        let key = self.texture_set_key(name, opts)?;
        self.sets.get(&key)
    }

    /// [`MaterialSystem::get_texture_set`], returning the `_sets` cache key
    /// instead of the entry.
    ///
    /// The source has no such method — a JS caller compares the returned
    /// object by identity. This is that identity, named: the string the two
    /// caches are keyed on is the whole contract of this file, and a caller
    /// (or a test) that wants to know *which* set a request collapsed onto
    /// needs it spelled out.
    pub fn texture_set_key(&mut self, name: &str, opts: &MaterialOpts) -> Option<String> {
        let key = self.resolve(name);
        self.ensure_texture_set(&key, opts)
    }

    /// `get(name, opts)` (`index.js:179-225`). Identical `(name, opts)` return
    /// the identical entry so meshes batch; any override is a distinct
    /// variant.
    pub fn get(&mut self, name: &str, opts: &MaterialOpts) -> &MaterialDef {
        let key = self.resolve(name);
        let mat_key = format!("{key}|{}", stable_key(opts));
        if self.materials.contains(&mat_key) {
            return self.materials.get(&mat_key).expect("just checked");
        }

        let set_key = self.ensure_texture_set(&key, opts);
        let entry = library_entry(&key).expect("resolve returns a library key");

        let mut params = library_params(entry);
        opts.apply_to_params(&mut params);
        // `p.groundY = opts.groundY ?? this._groundY` (`index.js:191`) —
        // after the spread, so it always wins.
        params.ground_y = opts
            .get("groundY")
            .filter(|v| !v.is_nullish())
            .and_then(OptValue::as_num)
            .unwrap_or(self.ground_y);

        let (three, physical) = merge_three(entry, opts);
        let vertex_colors = params.vertex_masks;
        // `if (set) extendMaterial(mat, p, this._shared)` (`index.js:221`).
        let uniforms = set_key
            .as_ref()
            .map(|_| LiveUniforms::from_params(&params));

        let def = MaterialDef {
            key,
            mat_key: mat_key.clone(),
            physical,
            params,
            three,
            vertex_colors,
            set_key,
            uniforms,
        };
        self.materials.insert(mat_key.clone(), def);
        self.materials.get(&mat_key).expect("just inserted")
    }

    /// `variant(name, opts)` (`index.js:228-230`) — literally `get()`, kept
    /// because it reads better at the call site.
    pub fn variant(&mut self, name: &str, opts: &MaterialOpts) -> &MaterialDef {
        self.get(name, opts)
    }

    /// `names()` (`index.js:233-235`) — all library names, aliases excluded.
    pub fn names(&self) -> Vec<&'static str> {
        LIBRARY.iter().map(|e| e.name).collect()
    }

    /// `surfaceOf(name)` (`index.js:238-240`) — the physics/FX surface tag.
    ///
    /// Note it goes through `resolveName`, **not** `_resolve`: an unknown name
    /// falls back to concrete here without warning and without being recorded
    /// in `_missing`.
    pub fn surface_of(&self, name: &str) -> Surface {
        library_entry(resolve_name(name)).map_or(Surface::Concrete, |e| e.surface)
    }

    /// `update(dt)` (`index.js:166-172`) — release the scratch height targets
    /// once the bake burst has clearly finished.
    ///
    /// `dt > 0.25 ? 0.25 : dt` clamps a load-hitch spike but does **not**
    /// clamp a negative `dt`, which is added raw and winds the clock back.
    pub fn update(&mut self, dt: f64) {
        if self.scratch_freed || self.forge.is_none() {
            return;
        }
        self.idle += if dt > 0.25 { 0.25 } else { dt };
        if self.idle < 5.0 {
            return;
        }
        self.scratch_freed = true;
        if let Some(forge) = self.forge.as_mut() {
            forge.release_scratch();
        }
    }

    /// `tune(material, changes)` (`index.js:243-257`) — live-edit a
    /// material's uniforms after creation. A material built without a texture
    /// set has no uniform block, so this is a no-op on it.
    ///
    /// Takes the material's cache key rather than the material itself: the
    /// system owns the entries, and the source's `material` argument is a
    /// handle into exactly this map.
    pub fn tune(&mut self, mat_key: &str, changes: &TuneChanges) -> Option<&MaterialDef> {
        {
            let def = self.materials.values.get_mut(mat_key)?;
            let uv_mode = def.params.uv_mode.clone();
            // `const u = material.userData?.owUniforms; if (!u) return material;`
            // — no uniform block means no edit, and the material comes back
            // unchanged rather than absent.
            if let Some(uniforms) = def.uniforms.as_mut() {
                if let Some(scale) = changes.scale {
                    let s = tile_scale(&uv_mode, scale);
                    uniforms.tile[0] = s;
                    uniforms.tile[1] = s;
                }
                if let Some(tint) = changes.tint {
                    uniforms.tint = hex_to_linear_tint(tint);
                }
                if let Some(parallax) = changes.parallax {
                    uniforms.parallax[0] = parallax;
                }
                if let Some(ground_y) = changes.ground_y {
                    uniforms.ground_y = ground_y;
                }
                if let Some(amp) = changes.normal_strength {
                    uniforms.normal_amp = amp;
                }
                if let Some(weather) = changes.weather.as_ref() {
                    // `Vector4.fromArray` copies as many components as the
                    // array has, leaving any remainder untouched.
                    for (slot, value) in uniforms.weather.iter_mut().zip(weather.iter()) {
                        *slot = *value;
                    }
                }
            }
        }
        self.materials.get(mat_key)
    }

    /// `setGroundLevel(y)` (`index.js:260-266`) — where the ground-splash
    /// weathering band sits, in world Y. Retroactive on every existing
    /// material that has a uniform block, and inherited by every later one
    /// through `p.groundY = opts.groundY ?? this._groundY`.
    pub fn set_ground_level(&mut self, y: f64) {
        self.ground_y = y;
        // The source walks `_materials.values()` in insertion order; every
        // iteration writes the same constant, so the traversal order is not
        // observable and the hash iteration is equivalent.
        for def in self.materials.values.values_mut() {
            if let Some(u) = def.uniforms.as_mut() {
                u.ground_y = y;
            }
        }
    }

    /// `get detailNormal()` / `get macroTexture()` (`index.js:268-274`), as
    /// the one descriptor both live on.
    pub fn shared(&self) -> Option<SharedMaps> {
        self.shared
    }

    /// `bakeMasks(geometry, opts)` (`index.js:276-278`) — a pure forward.
    pub fn bake_masks(
        &self,
        geo: &Geo,
        opts: BakeMaskOpts,
        rng: Option<&mut Rng>,
    ) -> Vec<[f32; 3]> {
        bake_masks(geo, opts, rng)
    }

    /// `setMask(geometry, opts)` (`index.js:280-282`) — a pure forward.
    pub fn set_mask(&self, geo: &Geo, wear: f32, grime: f32, ao: f32) -> Vec<[f32; 3]> {
        set_mask(geo, wear, grime, ao)
    }

    /// `debugBoard(opts)` -> `buildDebugBoard` (`index.js:285-287, 327-351`).
    ///
    /// A grid of one sphere plus one bevelled panel per surface. The geometry
    /// — `SphereGeometry(radius, 64, 48)`, `BoxGeometry(0.92, 0.92, 0.14, 8,
    /// 8, 2)`, and the `bakeMasks(panel, { wear: 1, grime: 0.9 })` run over
    /// it — is Three.js scene-graph construction with no counterpart here;
    /// what is ported is the placement arithmetic and the two material
    /// requests per cell, which is the part that exercises this file.
    pub fn debug_board(&mut self, opts: DebugBoardOpts) -> Vec<BoardItem> {
        let names = self.names();
        let mut out = Vec::with_capacity(names.len());
        for (i, name) in names.into_iter().enumerate() {
            let x = ((i % opts.columns) as f64) * opts.spacing;
            let y = -((i / opts.columns) as f64) * opts.spacing;
            let sphere_material = self
                .get(
                    name,
                    &MaterialOpts::new().with("vertexMasks", OptValue::Bool(false)),
                )
                .mat_key
                .clone();
            let panel_material = self
                .get(
                    name,
                    &MaterialOpts::new()
                        .with("vertexMasks", OptValue::Bool(true))
                        .with("localSpace", OptValue::Bool(true)),
                )
                .mat_key
                .clone();
            out.push(BoardItem {
                index: i,
                name,
                sphere_position: [x, y, 0.0],
                panel_position: [x, y, -0.9],
                sphere_material,
                panel_material,
            });
        }
        out
    }

    /// `dispose()` (`index.js:289-297`). Clears the two caches, the forge and
    /// the shared maps; `_groundY`, `_quality`, `_warned`, `_missing`,
    /// `_idle` and `_scratchFreed` all survive, exactly as in the source.
    pub fn dispose(&mut self) {
        self.materials.clear();
        self.sets.clear();
        self.forge = None;
        self.shared = None;
        self.built = false;
    }

    // ------------------------------------------------------ observables --

    /// `_sets` keys, in insertion order.
    pub fn set_keys(&self) -> &[String] {
        self.sets.keys()
    }

    /// `_materials` keys, in insertion order.
    pub fn material_keys(&self) -> &[String] {
        self.materials.keys()
    }

    pub fn set_count(&self) -> usize {
        self.sets.len()
    }

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    /// How many texture sets have been built since construction — the
    /// number of `TextureForge.build` calls the source would have made.
    pub fn bake_count(&self) -> u32 {
        self.bakes
    }

    pub fn material(&self, mat_key: &str) -> Option<&MaterialDef> {
        self.materials.get(mat_key)
    }

    pub fn texture_set(&self, bake_key: &str) -> Option<&TextureSet> {
        self.sets.get(bake_key)
    }

    pub fn forge(&self) -> Option<&Forge> {
        self.forge.as_ref()
    }

    pub fn is_built(&self) -> bool {
        self.built
    }

    /// `this._idle`, seconds since the last bake.
    pub fn idle(&self) -> f64 {
        self.idle
    }

    pub fn scratch_freed(&self) -> bool {
        self.scratch_freed
    }

    pub fn ground_level(&self) -> f64 {
        self.ground_y
    }

    /// `this._quality`, the texture-budget scalar.
    pub fn quality(&self) -> f64 {
        self.quality
    }
}

/// The two `LIBRARY` entries that declare a `bake.param`
/// (`library.js:21, 42`). The ported [`crate::materials::BakeParams`] stores
/// `param` as a plain `[f32; 4]`, so a zero array is ambiguous between
/// "declared `[0, 0, 0, 0]`" and "not declared" — and the two key
/// differently (`0_0_0_0` vs the empty string). Neither of the two declares
/// all-zeros, so the ambiguity is resolvable by name.
///
/// The structurally correct fix is `param: Option<[f32; 4]>` on `BakeParams`;
/// see the notes file, and [`MISSING_LIBRARY_THREE`] for the same shape of
/// gap on `ThreeOptions`.
const LIBRARY_HAS_PARAM: &[&str] = &["concrete", "concrete_floor"];

/// `cfg.quality === 'low' ? 0.5 : cfg.quality === 'medium' ? 0.75 : 1`
/// (`index.js:56-57`). `high` and `ultra` both fall through to 1.
fn quality_scalar(quality: Quality) -> f64 {
    match quality {
        Quality::Low => 0.5,
        Quality::Medium => 0.75,
        Quality::High | Quality::Ultra => 1.0,
    }
}

impl Subsystem for MaterialSystem {
    /// `static id = 'materials'`.
    fn id(&self) -> &'static str {
        "materials"
    }

    /// `static deps = ['render']`.
    fn deps(&self) -> &'static [&'static str] {
        &["render"]
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Update]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// `async init(ctx)` (`index.js:51-59`). The source also stores `ctx` so
    /// `_renderer()` can `peek('render')` later; this port takes the renderer
    /// through [`MaterialSystem::set_renderer`] instead — seam 1.
    fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), crate::error::CoreError> {
        self.configure(ctx.config.quality, ctx.config.q.anisotropy);
        Ok(())
    }

    fn update(&mut self, dt: Seconds, _ctx: &Ctx<'_>) {
        MaterialSystem::update(self, f64::from(dt.get()));
    }

    fn dispose(&mut self) {
        MaterialSystem::dispose(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_number_matches_ecmascript_layout() {
        // Positional inside [1e-6, 1e21), exponential outside it.
        assert_eq!(js_number(2.0), "2");
        assert_eq!(js_number(2.35), "2.35");
        assert_eq!(js_number(-0.08), "-0.08");
        assert_eq!(js_number(0.5), "0.5");
        assert_eq!(js_number(13_615_268.0), "13615268");
        assert_eq!(js_number(1e-6), "0.000001");
        assert_eq!(js_number(1e-7), "1e-7");
        assert_eq!(js_number(1e21), "1e+21");
        assert_eq!(js_number(1.5e-7), "1.5e-7");
        // `ToString(-0)` is "0".
        assert_eq!(js_number(-0.0), "0");
        assert_eq!(js_number(0.0), "0");
        assert_eq!(js_number(f64::NAN), "NaN");
        assert_eq!(js_number(f64::INFINITY), "Infinity");
        assert_eq!(js_number(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn every_library_generator_resolves() {
        for entry in LIBRARY {
            let sample = sample_surface(
                entry.generator,
                Vec2::new(0.25, 0.75),
                f64::from(entry.bake.seed),
                Vec3::new(1.0, 1.0, 1.0),
                Vec3::new(1.0, 1.0, 1.0),
                Vec4::new(0.0, 0.0, 0.0, 0.0),
            );
            assert!(
                sample.height.is_finite(),
                "{} produced a non-finite height",
                entry.name
            );
        }
    }

    #[test]
    #[should_panic(expected = "no owSurface generator named")]
    fn an_unknown_generator_panics() {
        sample_surface(
            "not-a-generator",
            Vec2::new(0.0, 0.0),
            1.0,
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec4::new(0.0, 0.0, 0.0, 0.0),
        );
    }

    /// The one datum [`MISSING_LIBRARY_THREE`] compensates for must stay the
    /// only one: if `ThreeOptions` gains a `transparent` field, this fails
    /// and the constant goes.
    #[test]
    fn only_glass_needs_a_three_compensation() {
        assert_eq!(MISSING_LIBRARY_THREE.len(), 1);
        let glass = library_entry("glass").expect("glass is in the library");
        assert_eq!(library_three(glass).bool("transparent"), Some(true));
    }
}
