//! The **weathering** layer of the runtime material shader: rain runoff, ground
//! splash, and the dust wedge at the wall/ground junction — plus the airborne
//! dust that settles on up-facing surfaces, which opens the same source section.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js`: the `owRunoff` helper in
//! `PARS_FRAGMENT` (source lines 141–168) and the whole `#ifdef OW_WEATHER`
//! section of `MAIN_FRAGMENT` (source lines 492–566). The stage table is
//! `docs/work-manifests/shmup-port/08-material-shader-plan.md`; the measurements
//! and the source-fidelity notes are in
//! `docs/work-manifests/shmup-port/notes/material-weathering.md`.
//!
//! ## Why this layer is the reason `SurfaceIn` grew a world space
//!
//! Every other layer of this shader could, at a push, be argued into object
//! space. This one cannot, and the argument is not stylistic:
//!
//! * **Rain runs down.** A streak is anchored to a world *height* — the source
//!   picks its sources on a 2.85 m storey pitch measured in world `y`, and runs
//!   them downward. In object space a rotated wall would have its rain running
//!   sideways.
//! * **Ground splash is measured up from a world `groundY`.** `hAbove` is
//!   `world_pos.y - ground_y`: a difference against a plane the *world* owns,
//!   not the mesh.
//! * **The dust wedge sits where a wall meets the ground**, which is the same
//!   world plane again, and is gated on the face being near-vertical in world
//!   terms.
//!
//! An object-space version of this layer would be wrong in a way that only shows
//! when something moves — which is exactly the failure a parity test cannot see
//! and a frame can. Hence `SurfaceIn::world_pos` / `world_normal`.
//!
//! ## What this file contains
//!
//! [`WEATHERING_WGSL`] (the shader text), a CPU reference for the same maths
//! (the functions below), and — under `--features offscreen` — a parity test
//! driving both on a real adapter.
//!
//! The CPU reference mirrors **WGSL vector semantics**, not `axiom_math`'s: it
//! traffics in `[f32; 3]` and divides without asking, because a shader lane that
//! divides by zero produces an infinity and `axiom_math::Vec3::normalize`
//! deliberately produces a `MathResult` instead. A reference that refuses what
//! the GPU permits is not a reference.
//!
//! ## Fidelity notes that are part of the algorithm
//!
//! * **GLSL `fract` is `x - floor(x)`**, not a remainder. World coordinates go
//!   negative constantly (a street runs both ways from the origin) and
//!   `owRunoff`'s column index is `floor(sAxis * 1.55)`, so the two definitions
//!   disagree over half the map. [`fract`] here is the GLSL one.
//! * **`smoothstep` is written out** rather than delegated to the builtin, for
//!   the reason `surface_program::parity` gives: the builtin's factoring is
//!   unspecified, and two of this layer's calls run with `e0 > e1` on purpose
//!   (`owVert`'s `smoothstep(0.72, 0.34, …)` and the splash spray's
//!   `smoothstep(0.10, max(z, 1e-3), …)` whenever the splash height is under
//!   10 cm), where the sign of `e1 - e0` decides the whole result.
//! * **`mix` is written out and always takes its factor as a value**, so both
//!   sides evaluate `x * (1 - a) + y * a` in `f32` with the same `a`. Folding a
//!   literal factor at higher precision would move the result by an ULP for no
//!   reason.
//! * **The multiply chains are the source's, ungrouped and untidied.** Float
//!   arithmetic is not associative; `wedge *= wedge * (0.7 + …)` is
//!   `wedge * (wedge * (0.7 + …))` and is transcribed that way.
//! * **The colours arrive as hex sRGB** and are converted by
//!   [`srgb_hex_to_linear`], which is *three.js*'s `SRGBToLinear`
//!   (`(c * 0.9478672986 + 0.0521327014)^2.4`, and `c * 0.0773993808` below the
//!   0.04045 knee) — because in the source they reach the shader through
//!   `new THREE.Color(hex)`, i.e. on the CPU, already linear. The algebraically
//!   equivalent GLSL `((c + 0.055) / 1.055)^2.4` form differs numerically on 254
//!   of 256 byte values; this port has already paid for that once.
//! * **GLSL `sign` returns `0.0` at zero** — a trap for this shader, but not for
//!   this section: the weathering stack calls `sign` nowhere. The `sgn` in the
//!   neighbouring repair-patch code is the patches layer's problem.
//!
//! ## The `#ifdef`s, as data
//!
//! The source guards this section with `OW_WEATHER` (on when any of
//! `weather.x/.y/.z` is positive) and the stain block inside the runoff pass with
//! `OW_VCOL_MASKS`. A WGSL port that grew one program permutation per define
//! would fight the content-addressed program identity for nothing, so both are
//! *values*: `vcol_masks` is `1.0`/`0.0` and selects with a `mix`, and a zero
//! weather term disables its own sub-pass arithmetically. That is only sound if
//! it is bit-identical, so it is tested that way — see
//! [`tests::a_zero_weather_term_disables_its_sub_pass_bit_identically`], which
//! also records the one place the source does **not** manage it.

/// The weathering stack, as WGSL.
///
/// Entry points, all free functions taking explicit arguments (no globals, no
/// assumed binding indices):
///
/// | function | signature |
/// |---|---|
/// | `ow_hash11` | `(f32) -> f32` |
/// | `ow_runoff` | `(s_axis: f32, y: f32, wobble: f32) -> vec3<f32>` |
/// | `ow_weather_vert` | `(nw_y: f32) -> f32` |
/// | `ow_weather_s_axis` | `(world_pos: vec3<f32>, nw: vec3<f32>) -> f32` |
/// | `ow_weather_streak_uv` | `(s_axis: f32, world_y: f32) -> vec4<f32>` |
/// | `ow_weather_dust` | `(OwWeatherState, nw_y, weather_x, mac1_b, mac2_g, dust_col, n_flat) -> OwWeatherState` |
/// | `ow_weather_rain` | `(OwWeatherState, vert, s_axis, world_y, s_n, s_fine, weather_y, vcolor, vcol_masks, grime_col, rust_col) -> OwWeatherState` |
/// | `ow_weather_splash` | `(OwWeatherState, vert, h_above, weather_z, mac1_b, mac2_g, grime_col, dust_col) -> OwWeatherState` |
/// | `ow_weather_wedge` | `(OwWeatherState, vert, h_above, weather_z, mac1_r, mac2_b, mac2_g, dust_col, n_flat) -> OwWeatherState` |
/// | `ow_weather_stack` | the four in order, with the two macro-texture fetches |
///
/// `OwWeatherState` is exactly the set of fragment locals this section mutates:
/// `albedo` (the source's `alb.rgb`), `orm` (ao/roughness/metalness) and
/// `n_shade`. `n_flat` is the source's `owP2V * owNp` — the flat face normal in
/// whatever space `n_shade` lives in — supplied already transformed, because the
/// object-vs-world space choice (`OW_OBJECT_SPACE`) is made above this layer.
pub(crate) const WEATHERING_WGSL: &str = r#"
// ---------------------------------------------------------------------------
// Weathering — Claude-of-Duty src/materials/shader.js, PARS_FRAGMENT owRunoff
// (141-168) and MAIN_FRAGMENT #ifdef OW_WEATHER (492-566).
// ---------------------------------------------------------------------------

// GLSL smoothstep, written out. The builtin's factoring is unspecified and two
// of this layer's calls deliberately pass e0 > e1, where the sign of (e1 - e0)
// is the whole result.
fn ow_weather_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// GLSL mix, written out: x*(1-a) + y*a, with `a` always an f32 value so a
// literal factor cannot be folded at a wider precision than the source uses.
fn ow_weather_mix3(x: vec3<f32>, y: vec3<f32>, a: f32) -> vec3<f32> {
    return x * (1.0 - a) + y * a;
}

// float owHash11( float x )
fn ow_hash11(x: f32) -> f32 {
    var p = fract(x * 0.1031);
    p = p * (p + 33.33);
    p = p * (p + p);
    return fract(p);
}

// vec3 owRunoff( float sAxis, float y, float wobble )
//
// Runoff staining below a source. sAxis is the horizontal coordinate along the
// wall, y the WORLD height. .x = 0..1 fades in over the first 15 cm below the
// source and out over the next 1.5 m, in discrete ~65 cm columns; .y carries the
// per-column random used to pick rusted fixings; .z is the metres below source.
fn ow_runoff(s_axis: f32, y: f32, wobble: f32) -> vec3<f32> {
    let u = s_axis * 1.55;
    let cell = floor(u);
    let lat = fract(u);
    let r0 = ow_hash11(cell * 1.37 + 3.1);
    let r1 = ow_hash11(cell * 2.71 + 11.7);
    // Only some columns have anything dripping down them.
    var src_amt = ow_weather_smoothstep(0.30, 0.62, r0) * (0.55 + 0.45 * r1);
    // Feathered across the column, so a run has soft sides instead of cell walls.
    let bell = sin(lat * 3.14159265);
    src_amt = src_amt * (bell * bell * (0.8 + 0.45 * r0));
    // Sources sit roughly one storey apart (SPACING 2.85), jittered per column.
    let spacing = 2.85;
    let jitter = r1 * 1.2 + r0 * 0.5;
    let src_y = (floor((y + jitter) / spacing) + 1.0) * spacing - jitter + wobble * 0.2;
    let below = src_y - y;
    let run = ow_weather_smoothstep(0.0, 0.15, below)
        * (1.0 - ow_weather_smoothstep(0.15, 1.65, below));
    return vec3<f32>(clamp(run * src_amt, 0.0, 1.0), r1, below);
}

// float owVert  (source line 446). How near-vertical the face is.
fn ow_weather_vert(nw_y: f32) -> f32 {
    return ow_weather_smoothstep(0.72, 0.34, abs(nw_y));
}

// float owSAxis (source line 447). The horizontal coordinate along a wall.
fn ow_weather_s_axis(world_pos: vec3<f32>, nw: vec3<f32>) -> f32 {
    return world_pos.z * nw.x - world_pos.x * nw.z;
}

// The two macro-texture coordinates the runoff pass fetches at (source 507-508),
// as (sN.uv, sFine.uv). Split out so the coordinate derivation is provable
// without a texture in the way.
fn ow_weather_streak_uv(s_axis: f32, world_y: f32) -> vec4<f32> {
    return vec4<f32>(s_axis * 0.46, world_y * 0.155, s_axis * 1.35 + 0.4, world_y * 0.42);
}

// The fragment locals this section mutates: alb.rgb, orm (ao/rough/metal), nShade.
struct OwWeatherState {
    albedo: vec3<f32>,
    orm: vec3<f32>,
    n_shade: vec3<f32>,
};

// Airborne dust settling on up-facing surfaces (source 494-499).
fn ow_weather_dust(
    state: OwWeatherState,
    nw_y: f32,
    weather_x: f32,
    mac1_b: f32,
    mac2_g: f32,
    dust_col: vec3<f32>,
    n_flat: vec3<f32>,
) -> OwWeatherState {
    let up = clamp(nw_y, 0.0, 1.0);
    let dust = up * up * weather_x
        * ow_weather_smoothstep(0.30, 0.80, mac1_b * 0.7 + mac2_g * 0.5);
    var result = state;
    result.albedo = ow_weather_mix3(state.albedo, dust_col, dust * 0.75);
    result.orm.g = clamp(state.orm.g + dust * 0.30, 0.0, 1.0);
    result.orm.b = state.orm.b * (1.0 - dust * 0.85);
    result.n_shade = normalize(
        ow_weather_mix3(state.n_shade, normalize(n_flat), dust * 0.35)
    );
    return result;
}

// Rain runoff (source 501-535), including the OW_VCOL_MASKS stain block as a
// value-gated term: `vcol_masks` is 1.0 when the define is on, 0.0 when it is
// off, and 0.0 leaves `streak` bit-identical.
fn ow_weather_rain(
    state: OwWeatherState,
    vert: f32,
    s_axis: f32,
    world_y: f32,
    s_n: f32,
    s_fine: f32,
    weather_y: f32,
    vcolor: vec3<f32>,
    vcol_masks: f32,
    grime_col: vec3<f32>,
    rust_col: vec3<f32>,
) -> OwWeatherState {
    let runoff = ow_runoff(s_axis, world_y, s_n - 0.5);
    var streak = clamp(weather_y * 2.2, 0.0, 1.15) * vert * runoff.x
        * ow_weather_smoothstep(0.30, 0.66, s_n * 0.72 + s_fine * 0.38);
    streak = clamp(streak, 0.0, 1.0);
    // #ifdef OW_VCOL_MASKS. An authored stain mask drives the run outright
    // instead of merely modulating the procedural columns.
    let stain_m = ow_weather_smoothstep(0.58, 0.98, vcolor.g);
    let stained = clamp(
        streak * (0.45 + 0.75 * clamp(vcolor.g * 1.5 + vcolor.b * 0.6, 0.0, 1.0))
            + stain_m * vert
                * (0.55 + 0.45 * ow_weather_smoothstep(0.20, 0.70, s_n * 0.6 + s_fine * 0.55)),
        0.0,
        1.0,
    );
    streak = streak * (1.0 - vcol_masks) + stained * vcol_masks;
    // A wet-then-dried run on render is a real 20-35% drop in albedo.
    let run_col = ow_weather_mix3(state.albedo * 0.72, grime_col, 0.26);
    // Rust bleed under metal fixings: strongest right under the fixing,
    // thinning as it runs down.
    let rust = clamp(step(0.86, runoff.y) * 0.9 + state.orm.b * 0.5, 0.0, 1.0)
        * (0.30 + 0.70 * (1.0 - ow_weather_smoothstep(0.1, 0.9, runoff.z)));
    let rusted = ow_weather_mix3(
        run_col,
        ow_weather_mix3(state.albedo * 0.94, rust_col, 0.5),
        rust,
    );
    var result = state;
    result.albedo = ow_weather_mix3(state.albedo, rusted, streak);
    result.orm.g = clamp(state.orm.g + streak * 0.09, 0.0, 1.0);
    result.orm.b = state.orm.b * (1.0 - streak * 0.35);
    return result;
}

// Ground splash (source 537-550): a hard dirt band in the bottom ~20 cm above a
// WORLD ground plane, plus thinning splatter above it. `h_above` is
// `world_pos.y - ground_y`.
fn ow_weather_splash(
    state: OwWeatherState,
    vert: f32,
    h_above: f32,
    weather_z: f32,
    mac1_b: f32,
    mac2_g: f32,
    grime_col: vec3<f32>,
    dust_col: vec3<f32>,
) -> OwWeatherState {
    let band = 1.0 - ow_weather_smoothstep(0.02, 0.22, h_above);
    let spray = 1.0 - ow_weather_smoothstep(0.10, max(weather_z, 1e-3), h_above);
    var splash = vert * max(band, spray * spray * 0.85) * step(1e-4, weather_z);
    // Broken up at 1-2 m, but with a floor so the base of every wall darkens.
    splash = splash
        * (0.55 + 0.45 * ow_weather_smoothstep(0.25, 0.72, mac1_b * 0.7 + mac2_g * 0.4));
    // Dust and rain-thrown dirt, not soot: a blend of the two weathering colours.
    let splash_col = ow_weather_mix3(grime_col, dust_col * 0.9, 0.35);
    var result = state;
    result.albedo = ow_weather_mix3(
        state.albedo * (1.0 - splash * 0.35),
        splash_col,
        splash * 0.42,
    );
    // NOTE the `- band * vert * 0.10` term is NOT gated by the splash's
    // step(1e-4, weather_z), in the source. Transcribed as written.
    result.orm.g = clamp(state.orm.g + splash * 0.16 - band * vert * 0.10, 0.0, 1.0);
    result.orm.r = state.orm.r * (1.0 - splash * 0.18);
    result.orm.b = state.orm.b * (1.0 - splash * 0.7);
    return result;
}

// The dust wedge at the wall / ground junction (source 552-565). A wall does not
// meet the ground on a line: wind and foot traffic pile a 25-40 cm wedge of the
// ground's own dust against it.
fn ow_weather_wedge(
    state: OwWeatherState,
    vert: f32,
    h_above: f32,
    weather_z: f32,
    mac1_r: f32,
    mac2_b: f32,
    mac2_g: f32,
    dust_col: vec3<f32>,
    n_flat: vec3<f32>,
) -> OwWeatherState {
    let wedge_h = 0.26 + 0.18 * (mac1_r * 0.6 + mac2_b * 0.7);
    var wedge = vert * (1.0 - ow_weather_smoothstep(wedge_h * 0.25, wedge_h, h_above));
    wedge = wedge * (wedge * (0.7 + 0.5 * ow_weather_smoothstep(0.2, 0.8, mac2_g)));
    wedge = clamp(wedge, 0.0, 1.0) * step(1e-4, weather_z);
    var result = state;
    result.albedo = ow_weather_mix3(state.albedo, dust_col, wedge * 0.46);
    result.orm.g = clamp(state.orm.g + wedge * 0.07, 0.0, 1.0);
    result.orm.b = state.orm.b * (1.0 - wedge * 0.9);
    // dust is loose powder: kill the sharp tile relief inside the wedge
    result.n_shade = normalize(
        ow_weather_mix3(state.n_shade, normalize(n_flat), wedge * 0.45)
    );
    return result;
}

// The whole section in source order, with the two macro-texture fetches the
// runoff pass makes. Texture and sampler are parameters: WGSL permits handle
// types as function arguments, so this layer names no binding index.
fn ow_weather_stack(
    state: OwWeatherState,
    world_pos: vec3<f32>,
    nw: vec3<f32>,
    n_flat: vec3<f32>,
    mac1: vec4<f32>,
    mac2: vec4<f32>,
    vcolor: vec3<f32>,
    vcol_masks: f32,
    weather: vec4<f32>,
    ground_y: f32,
    dust_col: vec3<f32>,
    grime_col: vec3<f32>,
    rust_col: vec3<f32>,
    macro_tex: texture_2d<f32>,
    macro_smp: sampler,
) -> OwWeatherState {
    let vert = ow_weather_vert(nw.y);
    let s_axis = ow_weather_s_axis(world_pos, nw);
    let streak_uv = ow_weather_streak_uv(s_axis, world_pos.y);
    let s_n = textureSample(macro_tex, macro_smp, streak_uv.xy).a;
    let s_fine = textureSample(macro_tex, macro_smp, streak_uv.zw).g;
    let h_above = world_pos.y - ground_y;
    let dusted = ow_weather_dust(state, nw.y, weather.x, mac1.b, mac2.g, dust_col, n_flat);
    let rained = ow_weather_rain(
        dusted, vert, s_axis, world_pos.y, s_n, s_fine, weather.y,
        vcolor, vcol_masks, grime_col, rust_col,
    );
    let splashed = ow_weather_splash(
        rained, vert, h_above, weather.z, mac1.b, mac2.g, grime_col, dust_col,
    );
    return ow_weather_wedge(
        splashed, vert, h_above, weather.z, mac1.r, mac2.b, mac2.g, dust_col, n_flat,
    );
}
"#;

// ---------------------------------------------------------------------------
// `DEFAULT_PARAMS` — the entries this layer owns.
// ---------------------------------------------------------------------------

/// `DEFAULT_PARAMS.weather`: `[dust, rain streaks, ground-splash height,
/// cavity grime]`.
///
/// This layer reads `.x`, `.y` and `.z`. **`.w` (cavity grime) is read by the
/// `masks` layer**, in the source's "cavity + vertex masks" section immediately
/// below this one (lines 569-571) — it is carried in the same `vec4` and so is
/// documented here, but no function in this file touches it.
pub(crate) const DEFAULT_WEATHER: [f32; 4] = [0.35, 0.3, 0.55, 0.4];

/// `DEFAULT_PARAMS.groundY` — the **world** height the ground splash and the
/// dust wedge are measured up from.
pub(crate) const DEFAULT_GROUND_Y: f32 = 0.0;

/// `DEFAULT_PARAMS.dustColor`, as authored: hex sRGB.
pub(crate) const DEFAULT_DUST_COLOR_HEX: u32 = 0x006b_6154;

/// `DEFAULT_PARAMS.grimeColor`, as authored: hex sRGB.
pub(crate) const DEFAULT_GRIME_COLOR_HEX: u32 = 0x002A_2620;

/// `DEFAULT_PARAMS.rustColor`, as authored: hex sRGB.
pub(crate) const DEFAULT_RUST_COLOR_HEX: u32 = 0x006D_3A1C;

/// One channel of *three.js*'s `SRGBToLinear`, in `f64`.
///
/// `(c < 0.04045) ? c * 0.0773993808 : pow(c * 0.9478672986 + 0.0521327014, 2.4)`
/// — three's own constants, not the algebraically-equal GLSL
/// `((c + 0.055) / 1.055)^2.4`. The two agree in real arithmetic and differ
/// numerically on 254 of the 256 byte inputs, and it is three's that runs in the
/// source, on the CPU, before the value is ever a uniform. `f64` for the same
/// reason: `Math.pow` is a `f64` operation and the `f32` narrowing happens once,
/// at upload.
fn three_srgb_to_linear(c: f64) -> f64 {
    let low = c * 0.0773993808;
    let high = (c * 0.9478672986 + 0.0521327014).powf(2.4);
    [high, low][usize::from(c < 0.04045)]
}

/// A hex sRGB colour, as *three.js* delivers it to a uniform: `SRGBToLinear` per
/// channel in `f64`, narrowed to `f32` on upload.
pub(crate) fn srgb_hex_to_linear(hex: u32) -> [f32; 3] {
    [16_u32, 8, 0].map(|shift| {
        three_srgb_to_linear(f64::from((hex >> shift) & 0xFF) / 255.0) as f32
    })
}

// ---------------------------------------------------------------------------
// The GLSL builtins this layer leans on, as the CPU reference means them.
// ---------------------------------------------------------------------------

/// GLSL `fract(x)` = `x - floor(x)`. **Not** Rust's `%`: `fract(-0.25)` is
/// `0.75`, and `owRunoff`'s column index is taken over world coordinates that go
/// negative on half the map.
fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// GLSL `clamp(x, lo, hi)` = `min(max(x, lo), hi)`. Written out rather than
/// `f32::clamp`, which panics when `lo > hi` where GLSL simply returns `hi`.
fn clamp(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}

/// GLSL `step(edge, x)`: `0.0` when `x < edge`, `1.0` otherwise.
fn step(edge: f32, x: f32) -> f32 {
    f32::from(x >= edge)
}

/// GLSL `smoothstep(e0, e1, x)`, written out — see the module header for why the
/// builtin is not used on either side.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// GLSL `mix(x, y, a)` = `x * (1 - a) + y * a`, over three lanes.
fn mix3(x: [f32; 3], y: [f32; 3], a: f32) -> [f32; 3] {
    [0_usize, 1, 2].map(|lane| x[lane] * (1.0 - a) + y[lane] * a)
}

/// A three-lane vector times a scalar.
fn scale3(x: [f32; 3], k: f32) -> [f32; 3] {
    x.map(|lane| lane * k)
}

/// GLSL `normalize(v)` = `v / length(v)`, and `length` is
/// `sqrt(x*x + y*y + z*z)` — a real division, not a reciprocal multiply, and not
/// `Math.hypot`.
fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|lane| lane / length)
}

// ---------------------------------------------------------------------------
// The CPU reference.
// ---------------------------------------------------------------------------

/// The fragment locals the weathering section mutates: `alb.rgb`, `orm`
/// (ao / roughness / metalness) and `nShade`. The WGSL `OwWeatherState`, lane for
/// lane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WeatherState {
    /// The source's `alb.rgb`.
    pub(crate) albedo: [f32; 3],
    /// The source's `orm`: `.0` ao, `.1` roughness, `.2` metalness.
    pub(crate) orm: [f32; 3],
    /// The source's `nShade`, in whatever space the caller supplies it.
    pub(crate) n_shade: [f32; 3],
}

/// `float owHash11( float x )`.
pub(crate) fn hash11(x: f32) -> f32 {
    let p = fract(x * 0.1031);
    let p = p * (p + 33.33);
    let p = p * (p + p);
    fract(p)
}

/// `vec3 owRunoff( float sAxis, float y, float wobble )` — the rain-streak
/// column field. `.0` is the 0..1 run strength, `.1` the per-column random the
/// rust bleed keys on, `.2` the metres below the source.
pub(crate) fn runoff(s_axis: f32, y: f32, wobble: f32) -> [f32; 3] {
    let u = s_axis * 1.55;
    let cell = u.floor();
    let lat = fract(u);
    let r0 = hash11(cell * 1.37 + 3.1);
    let r1 = hash11(cell * 2.71 + 11.7);
    let src_amt = smoothstep(0.30, 0.62, r0) * (0.55 + 0.45 * r1);
    // The source writes the literal `3.14159265`, which rounds to exactly the
    // same `f32` as `PI` (0x4049_0FDB) — proved in
    // `tests::the_sources_pi_literal_is_the_f32_pi`. Named rather than repeated
    // because `clippy::approx_constant` is a deny, and an `#[allow]` to keep a
    // literal that IS the constant would be silencing, not transcribing. The
    // WGSL keeps the source's digits verbatim.
    let bell = (lat * core::f32::consts::PI).sin();
    let src_amt = src_amt * (bell * bell * (0.8 + 0.45 * r0));
    let spacing = 2.85_f32;
    let jitter = r1 * 1.2 + r0 * 0.5;
    let src_y = (((y + jitter) / spacing).floor() + 1.0) * spacing - jitter + wobble * 0.2;
    let below = src_y - y;
    let run = smoothstep(0.0, 0.15, below) * (1.0 - smoothstep(0.15, 1.65, below));
    [clamp(run * src_amt, 0.0, 1.0), r1, below]
}

/// `float owVert` (source line 446): how near-vertical the world face is. Note
/// the inverted edges — `e0 > e1` — which is the whole point of it.
pub(crate) fn vert_facing(nw_y: f32) -> f32 {
    smoothstep(0.72, 0.34, nw_y.abs())
}

/// `float owSAxis` (source line 447): the horizontal coordinate along a wall.
pub(crate) fn s_axis(world_pos: [f32; 3], nw: [f32; 3]) -> f32 {
    world_pos[2] * nw[0] - world_pos[0] * nw[2]
}

/// The macro-texture coordinates the runoff pass fetches `sN` and `sFine` at
/// (source lines 507-508), as `[sN.u, sN.v, sFine.u, sFine.v]`.
pub(crate) fn streak_uv(s_axis: f32, world_y: f32) -> [f32; 4] {
    [
        s_axis * 0.46,
        world_y * 0.155,
        s_axis * 1.35 + 0.4,
        world_y * 0.42,
    ]
}

/// Airborne dust settling on up-facing surfaces (source lines 494-499).
///
/// `n_flat` is the source's `owP2V * owNp`, already transformed into `n_shade`'s
/// space by the caller.
pub(crate) fn dust(
    state: WeatherState,
    nw_y: f32,
    weather_x: f32,
    mac1_b: f32,
    mac2_g: f32,
    dust_col: [f32; 3],
    n_flat: [f32; 3],
) -> WeatherState {
    let up = clamp(nw_y, 0.0, 1.0);
    let dust = up * up * weather_x * smoothstep(0.30, 0.80, mac1_b * 0.7 + mac2_g * 0.5);
    WeatherState {
        albedo: mix3(state.albedo, dust_col, dust * 0.75),
        orm: [
            state.orm[0],
            clamp(state.orm[1] + dust * 0.30, 0.0, 1.0),
            state.orm[2] * (1.0 - dust * 0.85),
        ],
        n_shade: normalize3(mix3(state.n_shade, normalize3(n_flat), dust * 0.35)),
    }
}

/// Rain runoff (source lines 501-535).
///
/// `vcol_masks` is the `OW_VCOL_MASKS` define as a value: `1.0` applies the
/// authored stain-mask term, `0.0` leaves `streak` bit-identical.
///
/// The argument list is the source's data flow under the brief's
/// explicit-argument calling convention (no globals, no `params.slots`), so it
/// is as long as the sub-pass genuinely reads. Bundling it into a struct would
/// hide which lanes the pass touches, which is the one thing a reader checking
/// the transcription needs to see.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rain(
    state: WeatherState,
    vert: f32,
    s_axis: f32,
    world_y: f32,
    s_n: f32,
    s_fine: f32,
    weather_y: f32,
    vcolor: [f32; 3],
    vcol_masks: f32,
    grime_col: [f32; 3],
    rust_col: [f32; 3],
) -> WeatherState {
    let runoff = runoff(s_axis, world_y, s_n - 0.5);
    let streak = clamp(weather_y * 2.2, 0.0, 1.15)
        * vert
        * runoff[0]
        * smoothstep(0.30, 0.66, s_n * 0.72 + s_fine * 0.38);
    let streak = clamp(streak, 0.0, 1.0);
    let stain_m = smoothstep(0.58, 0.98, vcolor[1]);
    let stained = clamp(
        streak * (0.45 + 0.75 * clamp(vcolor[1] * 1.5 + vcolor[2] * 0.6, 0.0, 1.0))
            + stain_m
                * vert
                * (0.55 + 0.45 * smoothstep(0.20, 0.70, s_n * 0.6 + s_fine * 0.55)),
        0.0,
        1.0,
    );
    let streak = streak * (1.0 - vcol_masks) + stained * vcol_masks;
    let run_col = mix3(scale3(state.albedo, 0.72), grime_col, 0.26);
    let rust = clamp(step(0.86, runoff[1]) * 0.9 + state.orm[2] * 0.5, 0.0, 1.0)
        * (0.30 + 0.70 * (1.0 - smoothstep(0.1, 0.9, runoff[2])));
    let rusted = mix3(
        run_col,
        mix3(scale3(state.albedo, 0.94), rust_col, 0.5),
        rust,
    );
    WeatherState {
        albedo: mix3(state.albedo, rusted, streak),
        orm: [
            state.orm[0],
            clamp(state.orm[1] + streak * 0.09, 0.0, 1.0),
            state.orm[2] * (1.0 - streak * 0.35),
        ],
        n_shade: state.n_shade,
    }
}

/// Ground splash (source lines 537-550). `h_above` is `world_pos.y - ground_y`:
/// the splash is a difference against a **world** ground plane, which is why
/// this layer needs `SurfaceIn::world_pos`.
///
/// The argument list is the source's data flow under the brief's
/// explicit-argument calling convention (no globals, no `params.slots`), so it
/// is as long as the sub-pass genuinely reads. Bundling it into a struct would
/// hide which lanes the pass touches, which is the one thing a reader checking
/// the transcription needs to see.
#[allow(clippy::too_many_arguments)]
pub(crate) fn splash(
    state: WeatherState,
    vert: f32,
    h_above: f32,
    weather_z: f32,
    mac1_b: f32,
    mac2_g: f32,
    grime_col: [f32; 3],
    dust_col: [f32; 3],
) -> WeatherState {
    let band = 1.0 - smoothstep(0.02, 0.22, h_above);
    let spray = 1.0 - smoothstep(0.10, weather_z.max(1e-3), h_above);
    let splash = vert * (band.max(spray * spray * 0.85)) * step(1e-4, weather_z);
    let splash = splash * (0.55 + 0.45 * smoothstep(0.25, 0.72, mac1_b * 0.7 + mac2_g * 0.4));
    let splash_col = mix3(grime_col, scale3(dust_col, 0.9), 0.35);
    WeatherState {
        albedo: mix3(
            scale3(state.albedo, 1.0 - splash * 0.35),
            splash_col,
            splash * 0.42,
        ),
        orm: [
            state.orm[0] * (1.0 - splash * 0.18),
            // The `- band * vert * 0.10` term is NOT gated by
            // `step(1e-4, weather_z)` in the source. Transcribed as written; see
            // the zero-term test, which records the consequence.
            clamp(
                state.orm[1] + splash * 0.16 - band * vert * 0.10,
                0.0,
                1.0,
            ),
            state.orm[2] * (1.0 - splash * 0.7),
        ],
        n_shade: state.n_shade,
    }
}

/// The dust wedge at the wall / ground junction (source lines 552-565).
///
/// The argument list is the source's data flow under the brief's
/// explicit-argument calling convention (no globals, no `params.slots`), so it
/// is as long as the sub-pass genuinely reads. Bundling it into a struct would
/// hide which lanes the pass touches, which is the one thing a reader checking
/// the transcription needs to see.
#[allow(clippy::too_many_arguments)]
pub(crate) fn wedge(
    state: WeatherState,
    vert: f32,
    h_above: f32,
    weather_z: f32,
    mac1_r: f32,
    mac2_b: f32,
    mac2_g: f32,
    dust_col: [f32; 3],
    n_flat: [f32; 3],
) -> WeatherState {
    let wedge_h = 0.26 + 0.18 * (mac1_r * 0.6 + mac2_b * 0.7);
    let wedge = vert * (1.0 - smoothstep(wedge_h * 0.25, wedge_h, h_above));
    let wedge = wedge * (wedge * (0.7 + 0.5 * smoothstep(0.2, 0.8, mac2_g)));
    let wedge = clamp(wedge, 0.0, 1.0) * step(1e-4, weather_z);
    WeatherState {
        albedo: mix3(state.albedo, dust_col, wedge * 0.46),
        orm: [
            state.orm[0],
            clamp(state.orm[1] + wedge * 0.07, 0.0, 1.0),
            state.orm[2] * (1.0 - wedge * 0.9),
        ],
        n_shade: normalize3(mix3(state.n_shade, normalize3(n_flat), wedge * 0.45)),
    }
}

/// Every argument `ow_weather_stack` takes, minus the texture and sampler — the
/// two macro fetches are the one thing a CPU reference cannot do, so `s_n` and
/// `s_fine` arrive already sampled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WeatherInput {
    /// The fragment's **world** position.
    pub(crate) world_pos: [f32; 3],
    /// The source's `owNw`: the world shading normal, face-flipped.
    pub(crate) nw: [f32; 3],
    /// The source's `owP2V * owNp`, in `n_shade`'s space.
    pub(crate) n_flat: [f32; 3],
    /// The first macro-noise fetch (`mac1`).
    pub(crate) mac1: [f32; 4],
    /// The second, coarser macro-noise fetch (`mac2`).
    pub(crate) mac2: [f32; 4],
    /// The interpolated vertex colour the stain mask rides in.
    pub(crate) vcolor: [f32; 3],
    /// `OW_VCOL_MASKS` as a value: `1.0` on, `0.0` off.
    pub(crate) vcol_masks: f32,
    /// `DEFAULT_PARAMS.weather`.
    pub(crate) weather: [f32; 4],
    /// `DEFAULT_PARAMS.groundY`, a **world** height.
    pub(crate) ground_y: f32,
    /// Linear `dustColor`.
    pub(crate) dust_col: [f32; 3],
    /// Linear `grimeColor`.
    pub(crate) grime_col: [f32; 3],
    /// Linear `rustColor`.
    pub(crate) rust_col: [f32; 3],
    /// The macro texture's alpha at `streak_uv`'s first pair.
    pub(crate) s_n: f32,
    /// The macro texture's green at `streak_uv`'s second pair.
    pub(crate) s_fine: f32,
}

/// The whole weathering section, in source order.
pub(crate) fn stack(state: WeatherState, input: WeatherInput) -> WeatherState {
    let vert = vert_facing(input.nw[1]);
    let axis = s_axis(input.world_pos, input.nw);
    let h_above = input.world_pos[1] - input.ground_y;
    let dusted = dust(
        state,
        input.nw[1],
        input.weather[0],
        input.mac1[2],
        input.mac2[1],
        input.dust_col,
        input.n_flat,
    );
    let rained = rain(
        dusted,
        vert,
        axis,
        input.world_pos[1],
        input.s_n,
        input.s_fine,
        input.weather[1],
        input.vcolor,
        input.vcol_masks,
        input.grime_col,
        input.rust_col,
    );
    let splashed = splash(
        rained,
        vert,
        h_above,
        input.weather[2],
        input.mac1[2],
        input.mac2[1],
        input.grime_col,
        input.dust_col,
    );
    wedge(
        splashed,
        vert,
        h_above,
        input.weather[2],
        input.mac1[0],
        input.mac2[2],
        input.mac2[1],
        input.dust_col,
        input.n_flat,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample inputs both the CPU-only tests and the GPU parity test drive.
    /// Chosen to hit what is easy to get wrong: **negative** world coordinates
    /// (GLSL `fract` and `floor` are where a `%` transcription dies), positions
    /// both below and above the ground plane, `s_axis` values straddling several
    /// runoff columns, faces from flat-up through vertical to overhanging, and
    /// two samples with `weather.z` at exactly zero.
    pub(super) fn samples() -> Vec<(WeatherState, WeatherInput)> {
        (0..24_usize)
            .map(|index| {
                let t = index as f32;
                let state = WeatherState {
                    albedo: [0.18 + t * 0.021, 0.62 - t * 0.019, 0.31 + t * 0.007],
                    orm: [0.9 - t * 0.02, 0.05 + t * 0.035, 0.02 + t * 0.03],
                    n_shade: normalize3([t * 0.11 - 1.3, 0.7 - t * 0.05, t * -0.09 + 0.85]),
                };
                let input = WeatherInput {
                    // The height progression is chosen, not arbitrary: it keeps
                    // every sample at least 0.10 of a cell clear of BOTH of
                    // `owRunoff`'s `floor` seams (the column boundary and the
                    // storey boundary), where a legal 2.5-ULP division on the GPU
                    // may floor to the other integer. See
                    // `parity::assert_samples_avoid_the_floor_seams`.
                    world_pos: [t * 0.83 - 9.5, t * 0.27 - 2.33, t * -1.13 + 6.25],
                    nw: normalize3([t * 0.09 - 1.0, 0.95 - t * 0.085, t * -0.07 + 0.4]),
                    n_flat: [t * 0.05 - 0.6, 0.8 - t * 0.03, t * -0.04 + 0.55],
                    mac1: [
                        fract(t * 0.317 + 0.11),
                        fract(t * 0.211 + 0.53),
                        fract(t * 0.437 + 0.29),
                        fract(t * 0.173 + 0.77),
                    ],
                    mac2: [
                        fract(t * 0.191 + 0.61),
                        fract(t * 0.383 + 0.07),
                        fract(t * 0.229 + 0.41),
                        fract(t * 0.157 + 0.93),
                    ],
                    vcolor: [fract(t * 0.13), fract(t * 0.27 + 0.2), fract(t * 0.41 + 0.6)],
                    vcol_masks: f32::from(index % 2 == 0),
                    weather: [
                        0.35 + t * 0.01,
                        0.3 + t * 0.012,
                        // Two samples pin the disabled path exactly.
                        [0.55 - t * 0.015, 0.0][usize::from((index == 5) | (index == 17))],
                        0.4,
                    ],
                    ground_y: -0.35,
                    dust_col: srgb_hex_to_linear(DEFAULT_DUST_COLOR_HEX),
                    grime_col: srgb_hex_to_linear(DEFAULT_GRIME_COLOR_HEX),
                    rust_col: srgb_hex_to_linear(DEFAULT_RUST_COLOR_HEX),
                    s_n: fract(t * 0.347 + 0.19),
                    s_fine: fract(t * 0.263 + 0.44),
                };
                (state, input)
            })
            .collect()
    }

    #[test]
    fn the_defaults_are_the_sources_default_params() {
        assert_eq!(DEFAULT_WEATHER, [0.35, 0.3, 0.55, 0.4]);
        assert_eq!(DEFAULT_GROUND_Y, 0.0);
        assert_eq!(DEFAULT_DUST_COLOR_HEX, 0x006b_6154);
        assert_eq!(DEFAULT_GRIME_COLOR_HEX, 0x002a_2620);
        assert_eq!(DEFAULT_RUST_COLOR_HEX, 0x006d_3a1c);
    }

    /// **The colour form is the one that runs, not the one that is equivalent.**
    ///
    /// The expected bits were captured from the source's own runtime — node with
    /// three 0.180, `new THREE.Color(hex)` then a `Float32Array` round trip —
    /// which is a real oracle, unlike the GLSL. `srgb_hex_to_linear` must
    /// reproduce them exactly.
    #[test]
    fn the_weathering_colours_are_threes_srgb_to_linear_bit_for_bit() {
        [
            (DEFAULT_DUST_COLOR_HEX, [0x3e16_8e51_u32, 0x3df4_d090, 0x3db5_910f]),
            (DEFAULT_GRIME_COLOR_HEX, [0x3cbd_ac21, 0x3c9e_c7c2, 0x3c6c_a5df]),
            (DEFAULT_RUST_COLOR_HEX, [0x3e1c_98ac, 0x3d2d_4ebb, 0x3c3e_4149]),
        ]
        .iter()
        .for_each(|(hex, expected)| {
            let bits = srgb_hex_to_linear(*hex).map(f32::to_bits);
            assert_eq!(
                bits, *expected,
                "three.Color({hex:#08x}) must round-trip bit-for-bit"
            );
        });
    }

    /// The knee is the branch three's conversion has and the GLSL form does not
    /// place identically, so drive both arms and the boundary itself. `0x0a` is
    /// `10/255 = 0.0392…`, below the 0.04045 knee; `0x0b` is `0.0431…`, above it.
    #[test]
    fn the_srgb_knee_selects_the_linear_arm_below_and_the_power_arm_above() {
        let below = f64::from(0x0a_u32) / 255.0;
        let above = f64::from(0x0b_u32) / 255.0;
        // Two asserts, not one `&&`: a short-circuit is a branch region, and its
        // never-taken arm is the only thing that keeps this file off 100%.
        assert!(below < 0.04045);
        assert!(above >= 0.04045);
        assert_eq!(three_srgb_to_linear(below), below * 0.0773993808);
        assert_eq!(
            three_srgb_to_linear(above),
            (above * 0.9478672986 + 0.0521327014).powf(2.4)
        );
        // Black and white are the two values where the two sRGB forms agree
        // exactly, and both must survive the round trip.
        assert_eq!(srgb_hex_to_linear(0x0000_0000), [0.0, 0.0, 0.0]);
        assert_eq!(srgb_hex_to_linear(0x00ff_ffff), [1.0, 1.0, 1.0]);
    }

    /// **GLSL `fract` is `x - floor(x)`, not a remainder.** The rain-streak
    /// columns are indexed by `floor(sAxis * 1.55)` over world coordinates that
    /// are negative across half of any street, so this is the single most likely
    /// place for a CPU reference to diverge.
    #[test]
    fn fract_is_x_minus_floor_x_and_disagrees_with_rem_on_negatives() {
        assert_eq!(fract(-0.25), 0.75);
        assert_eq!(fract(2.25), 0.25);
        assert_eq!(fract(-3.0), 0.0);
        // Rust's `%` would give -0.25 here. That difference is the bug.
        assert_ne!(fract(-0.25), -0.25_f32 % 1.0);
    }

    /// GLSL `step`, `clamp` and `smoothstep` as this layer means them — including
    /// the inverted-edge case two of its calls rely on.
    #[test]
    fn the_glsl_shims_mean_what_glsl_means() {
        assert_eq!(step(0.86, 0.86), 1.0);
        assert_eq!(step(0.86, 0.8599999), 0.0);
        assert_eq!(clamp(3.0, 0.0, 1.0), 1.0);
        assert_eq!(clamp(-3.0, 0.0, 1.0), 0.0);
        // lo > hi: GLSL returns hi, where `f32::clamp` would panic.
        assert_eq!(clamp(0.5, 1.0, 0.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
        assert_eq!(smoothstep(0.0, 1.0, -2.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 2.0), 1.0);
        // Inverted edges: `owVert` and the splash spray both run this way.
        assert_eq!(smoothstep(1.0, 0.0, 0.25), smoothstep(0.0, 1.0, 0.75));
        assert_eq!(mix3([0.0, 1.0, 2.0], [4.0, 5.0, 6.0], 0.5), [2.0, 3.0, 4.0]);
        assert_eq!(scale3([1.0, 2.0, 3.0], 2.0), [2.0, 4.0, 6.0]);
        assert_eq!(normalize3([0.0, 3.0, 4.0]), [0.0, 0.6, 0.8]);
    }

    /// The one place this port names a constant where the source spells out
    /// digits. `3.14159265` is not `PI` in `f64` — it differs at the ninth
    /// decimal — but the shader is `f32`, and there the two are the *same
    /// number*. The decimal is reconstructed from integers so the assertion
    /// cannot be written by the same habit it is checking.
    #[test]
    fn the_sources_pi_literal_is_the_f32_pi() {
        let source_literal = (314_159_265.0_f64 / 100_000_000.0) as f32;
        assert_eq!(source_literal.to_bits(), 0x4049_0FDB);
        assert_eq!(source_literal.to_bits(), core::f32::consts::PI.to_bits());
        // And the WGSL still carries the source's digits, not the name.
        assert!(WEATHERING_WGSL.contains("sin(lat * 3.14159265)"));
    }

    /// `owHash11` is a scrambler, and a scrambler that has lost a step still
    /// looks like noise. Pin its structure — the two self-multiplies in order —
    /// against a hand-evaluation, and prove it actually scatters.
    #[test]
    fn hash11_is_the_sources_two_step_scrambler() {
        let expected = |x: f32| {
            let p = fract(x * 0.1031);
            let p = p * (p + 33.33);
            fract(p * (p + p))
        };
        [0.0_f32, 3.1, 11.7, -4.37, 1234.5].iter().for_each(|x| {
            assert_eq!(hash11(*x), expected(*x));
            assert!((0.0..1.0).contains(&hash11(*x)));
        });
        let spread = (0..64)
            .map(|i| hash11(i as f32 * 1.37 + 3.1))
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        assert!(
            spread.1 - spread.0 > 0.8,
            "a column hash that does not scatter gives every wall the same streaks"
        );
    }

    /// `owRunoff` returns `(run, r1, below)`, and the third lane is a **signed**
    /// distance below the source: above the source it is negative, and the run
    /// must be exactly zero there.
    #[test]
    fn runoff_runs_downward_from_a_source_and_dies_within_the_next_metre_and_a_half() {
        // A column the source field actually drives hard (cell 27, r0 0.78,
        // r1 0.97 — so the rust-bleed `step(0.86, r1)` arm is live too).
        let axis = 17.74_f32;
        let column = ((0..400).map(|i| {
            let y = i as f32 * 0.02 - 4.0;
            (y, runoff(axis, y, 0.0))
        }))
        .collect::<Vec<_>>();
        assert!(
            column.iter().any(|(_, r)| r[0] > 0.2),
            "some height under this column must actually stain"
        );
        column.iter().for_each(|(y, r)| {
            // Bound before the assert, and interpolated by name: a format
            // ARGUMENT is only evaluated when the assert fires, so it is a
            // region no passing test can reach. Inline captures have no such
            // region, which is how a test stays at 100%.
            let (run, below) = (r[0], r[2]);
            assert!((0.0..=1.0).contains(&run), "run out of range at y {y}");
            // `below` <= 0 is at or above the source; > 1.65 is past the fade-out.
            let outside = f32::from((below <= 0.0) | (below > 1.65));
            assert!(
                outside * run == 0.0,
                "a run must be zero above its source and beyond 1.65 m below it \
                 (y {y}, below {below}, run {run})"
            );
        });
        // The per-column random is the second lane, and it is the column's, not
        // the height's: it must not move as you walk down the wall.
        assert_eq!(runoff(axis, -3.0, 0.0)[1], runoff(axis, 5.0, 0.0)[1]);
        // Two columns 65 cm apart are different columns.
        assert_ne!(runoff(axis, 0.0, 0.0)[1], runoff(axis + 0.65, 0.0, 0.0)[1]);
    }

    /// The streak columns must be **world**-anchored and survive negative
    /// coordinates: `floor` on the negative side is where a `%`-based fract
    /// silently folds two columns into one.
    #[test]
    fn runoff_columns_are_distinct_on_the_negative_side_of_the_origin() {
        let cell_of = |axis: f32| (axis * 1.55).floor();
        assert_eq!(cell_of(-0.1), -1.0);
        assert_eq!(cell_of(0.1), 0.0);
        // The column either side of the origin must not be the same column.
        assert_ne!(runoff(-0.1, 1.0, 0.0)[1], runoff(0.1, 1.0, 0.0)[1]);
    }

    /// `owVert` and `owSAxis`, the two shared derivations this layer reads.
    #[test]
    fn the_face_and_wall_axis_derivations_match_the_source() {
        // Flat up or flat down: not a wall.
        assert_eq!(vert_facing(1.0), 0.0);
        assert_eq!(vert_facing(-1.0), 0.0);
        // Dead vertical: fully a wall.
        assert_eq!(vert_facing(0.0), 1.0);
        // Symmetric in the sign of the normal's up component.
        assert_eq!(vert_facing(0.5), vert_facing(-0.5));
        assert_eq!(s_axis([2.0, 9.0, 3.0], [1.0, 0.0, 0.0]), 3.0);
        assert_eq!(s_axis([2.0, 9.0, 3.0], [0.0, 0.0, 1.0]), -2.0);
        // The wall axis is horizontal: the fragment's height never enters it.
        assert_eq!(
            s_axis([2.0, 9.0, 3.0], [0.6, 0.5, -0.4]),
            s_axis([2.0, -70.0, 3.0], [0.6, 0.5, -0.4])
        );
        // The two streak fetches: only the fine one carries the 0.4 offset, and
        // it is ~2.9x the coarse in u against ~2.7x in v — the roughly 3:1
        // vertical stretch the source's comment claims.
        assert_eq!(streak_uv(0.0, 0.0), [0.0, 0.0, 0.4, 0.0]);
        let uv = streak_uv(2.0, 4.0);
        assert_eq!([uv[0], uv[1], uv[3]], [0.92, 0.62, 1.68]);
        assert_eq!(uv[2], 2.0_f32 * 1.35 + 0.4);
    }

    /// **The splash is a difference against a WORLD ground height.** Move the
    /// ground and the band moves with it; move the fragment the same distance and
    /// nothing changes. An object-space port cannot do this, and this is the test
    /// that says so.
    #[test]
    fn the_splash_band_tracks_the_world_ground_plane() {
        let colour = srgb_hex_to_linear(DEFAULT_GRIME_COLOR_HEX);
        let state = WeatherState {
            albedo: [0.5, 0.5, 0.5],
            orm: [0.8, 0.4, 0.2],
            n_shade: [0.0, 0.0, 1.0],
        };
        let at = |h_above: f32| splash(state, 1.0, h_above, 0.55, 0.6, 0.5, colour, colour);
        // In the band, the albedo is pulled hard toward the splash colour.
        assert!(at(0.0).albedo[0] < 0.42);
        // Two metres up, nothing at all.
        assert_eq!(at(2.0).albedo, state.albedo);
        // The same fragment at two ground heights is two different results.
        assert_ne!(at(0.05), at(1.05));
    }

    /// The dust wedge sits at the wall/ground junction and only on a wall.
    #[test]
    fn the_dust_wedge_is_a_vertical_face_at_the_ground_junction_only() {
        let colour = srgb_hex_to_linear(DEFAULT_DUST_COLOR_HEX);
        let state = WeatherState {
            albedo: [0.5, 0.5, 0.5],
            orm: [0.8, 0.4, 0.2],
            n_shade: [0.0, 0.0, 1.0],
        };
        let at = |vert: f32, h: f32| wedge(state, vert, h, 0.55, 0.5, 0.5, 0.5, colour, [0.0, 1.0, 0.0]);
        assert!(at(1.0, 0.0).albedo[0] < 0.42, "the junction must gather dust");
        // A horizontal face has no junction, and a metre up there is no wedge.
        assert_eq!(at(0.0, 0.0).albedo, state.albedo);
        assert_eq!(at(1.0, 1.0).albedo, state.albedo);
        // Inside the wedge the normal is pulled toward the flat face: loose
        // powder has no tile relief.
        assert_ne!(at(1.0, 0.0).n_shade, state.n_shade);
    }

    /// **A zero weather term must disable its sub-pass bit-identically** — the
    /// port replaces the source's `#ifdef OW_WEATHER` with arithmetic, and that
    /// substitution is only honest if zero really is a no-op.
    ///
    /// Three of the four terms manage it. The fourth is a **source defect this
    /// port preserves**: the ground splash's roughness line subtracts
    /// `band * vert * 0.10` *outside* the `step(1e-4, weather.z)` gate, so a
    /// material with the splash switched off still has the roughness at the base
    /// of every vertical face pulled down. Transcribed as written, and pinned
    /// here so a future tidy-up is a deliberate divergence rather than an
    /// accident.
    #[test]
    fn a_zero_weather_term_disables_its_sub_pass_bit_identically() {
        samples().iter().for_each(|(state, input)| {
            let zeroed = |lane: usize| {
                let mut weather = input.weather;
                weather[lane] = 0.0;
                WeatherInput { weather, ..*input }
            };

            // weather.x — airborne dust.
            let no_dust = zeroed(0);
            assert_eq!(
                dust(
                    *state,
                    no_dust.nw[1],
                    no_dust.weather[0],
                    no_dust.mac1[2],
                    no_dust.mac2[1],
                    no_dust.dust_col,
                    no_dust.n_flat
                ),
                WeatherState {
                    n_shade: normalize3(state.n_shade),
                    ..*state
                }
            );

            // weather.y — rain streaks.
            let no_rain = zeroed(1);
            assert_eq!(
                rain(
                    *state,
                    vert_facing(no_rain.nw[1]),
                    s_axis(no_rain.world_pos, no_rain.nw),
                    no_rain.world_pos[1],
                    no_rain.s_n,
                    no_rain.s_fine,
                    no_rain.weather[1],
                    no_rain.vcolor,
                    // With the stain mask OFF, zero rain is exactly a no-op.
                    0.0,
                    no_rain.grime_col,
                    no_rain.rust_col
                ),
                *state
            );

            // weather.z — the ground splash's height, which gates BOTH the
            // splash and the wedge.
            let no_splash = zeroed(2);
            let vert = vert_facing(no_splash.nw[1]);
            let h_above = no_splash.world_pos[1] - no_splash.ground_y;
            let after = splash(
                *state,
                vert,
                h_above,
                no_splash.weather[2],
                no_splash.mac1[2],
                no_splash.mac2[1],
                no_splash.grime_col,
                no_splash.dust_col,
            );
            assert_eq!(after.albedo, state.albedo);
            assert_eq!(after.orm[0], state.orm[0]);
            assert_eq!(after.orm[2], state.orm[2]);
            // ...but NOT the roughness. The un-gated `band * vert * 0.10`.
            let band = 1.0 - smoothstep(0.02, 0.22, h_above);
            assert_eq!(
                after.orm[1],
                clamp(state.orm[1] - band * vert * 0.10, 0.0, 1.0)
            );
            assert_eq!(
                wedge(
                    *state,
                    vert,
                    h_above,
                    no_splash.weather[2],
                    no_splash.mac1[0],
                    no_splash.mac2[2],
                    no_splash.mac2[1],
                    no_splash.dust_col,
                    no_splash.n_flat
                ),
                WeatherState {
                    n_shade: normalize3(state.n_shade),
                    ..*state
                }
            );

            // weather.w — cavity grime. This layer never reads it, and moving it
            // must change nothing here. (Its consumer is the `masks` layer.)
            let mut louder = input.weather;
            louder[3] = 1.0;
            assert_eq!(
                stack(*state, WeatherInput { weather: louder, ..*input }),
                stack(*state, WeatherInput { weather: input.weather, ..*input })
            );
        });
    }

    /// The `OW_VCOL_MASKS` define, as a value: `0.0` must leave the streak
    /// exactly where it was, and `1.0` must actually do something.
    #[test]
    fn the_vertex_colour_stain_gate_is_a_bit_identical_no_op_at_zero() {
        let moved = samples()
            .iter()
            .map(|(state, input)| {
                let vert = vert_facing(input.nw[1]);
                let axis = s_axis(input.world_pos, input.nw);
                let of = |masks: f32| {
                    rain(
                        *state,
                        vert,
                        axis,
                        input.world_pos[1],
                        input.s_n,
                        input.s_fine,
                        input.weather[1],
                        input.vcolor,
                        masks,
                        input.grime_col,
                        input.rust_col,
                    )
                };
                let off = of(0.0);
                // With a strong rain term but the define off, the stain block is
                // invisible; with it on, the authored mask drives the run.
                assert_eq!(
                    off,
                    rain(
                        *state,
                        vert,
                        axis,
                        input.world_pos[1],
                        input.s_n,
                        input.s_fine,
                        input.weather[1],
                        [0.0, 0.0, 0.0],
                        0.0,
                        input.grime_col,
                        input.rust_col,
                    ),
                    "with the define off the vertex colour must not be read at all"
                );
                f32::from(of(1.0) != off)
            })
            .sum::<f32>();
        assert!(
            moved > 8.0,
            "the stain term must actually change the run on most samples"
        );
    }

    /// The stack is the four sub-passes in the source's order, threading one
    /// state. Order matters — the rain pass reads the metalness the dust pass
    /// wrote, and the splash reads the albedo the rain wrote — so prove the
    /// composition rather than just the parts.
    #[test]
    fn the_stack_is_the_four_sub_passes_threaded_in_source_order() {
        samples().iter().for_each(|(state, input)| {
            let vert = vert_facing(input.nw[1]);
            let axis = s_axis(input.world_pos, input.nw);
            let h_above = input.world_pos[1] - input.ground_y;
            let expected = wedge(
                splash(
                    rain(
                        dust(
                            *state,
                            input.nw[1],
                            input.weather[0],
                            input.mac1[2],
                            input.mac2[1],
                            input.dust_col,
                            input.n_flat,
                        ),
                        vert,
                        axis,
                        input.world_pos[1],
                        input.s_n,
                        input.s_fine,
                        input.weather[1],
                        input.vcolor,
                        input.vcol_masks,
                        input.grime_col,
                        input.rust_col,
                    ),
                    vert,
                    h_above,
                    input.weather[2],
                    input.mac1[2],
                    input.mac2[1],
                    input.grime_col,
                    input.dust_col,
                ),
                vert,
                h_above,
                input.weather[2],
                input.mac1[0],
                input.mac2[2],
                input.mac2[1],
                input.dust_col,
                input.n_flat,
            );
            assert_eq!(stack(*state, *input), expected);
        });
        // And it is not the identity: weathering that changes nothing is not
        // weathering.
        let (state, input) = samples()[0];
        assert_ne!(stack(state, input).albedo, state.albedo);
    }

    /// The shader text declares every entry point this layer promises, with the
    /// signature the orchestrator will compose against. A renamed or re-argumented
    /// function is a composition failure that no parity test can see, because the
    /// parity harness is written against the same names.
    #[test]
    fn the_wgsl_declares_the_layers_entry_points() {
        [
            "fn ow_hash11(x: f32) -> f32",
            "fn ow_runoff(s_axis: f32, y: f32, wobble: f32) -> vec3<f32>",
            "fn ow_weather_vert(nw_y: f32) -> f32",
            "fn ow_weather_s_axis(world_pos: vec3<f32>, nw: vec3<f32>) -> f32",
            "fn ow_weather_streak_uv(s_axis: f32, world_y: f32) -> vec4<f32>",
            "struct OwWeatherState",
            "fn ow_weather_dust(",
            "fn ow_weather_rain(",
            "fn ow_weather_splash(",
            "fn ow_weather_wedge(",
            "fn ow_weather_stack(",
            "macro_tex: texture_2d<f32>",
            "macro_smp: sampler,",
        ]
        .iter()
        .for_each(|needle| {
            assert!(
                WEATHERING_WGSL.contains(needle),
                "the weathering WGSL must declare `{needle}`"
            );
        });
        // The two macro fetches, and which channel each takes: swapping `.a` and
        // `.g` is a silent, plausible-looking transcription error.
        assert!(WEATHERING_WGSL.contains("textureSample(macro_tex, macro_smp, streak_uv.xy).a"));
        assert!(WEATHERING_WGSL.contains("textureSample(macro_tex, macro_smp, streak_uv.zw).g"));
        // No builtin smoothstep or mix anywhere: both are written out.
        assert!(!WEATHERING_WGSL.contains(" smoothstep("));
        assert!(!WEATHERING_WGSL.contains(" mix("));
    }
}

/// **CPU↔GPU parity, on a real adapter.**
///
/// The pattern is `surface_program::parity`'s: a fullscreen triangle over an
/// `Rgba32Float` target where fragment column *i* evaluates sample *i*, read
/// back and compared lane by lane. Its own harness rather than that module's
/// because `surface_program::parity` is private to its module and drives the
/// field algebra's *generated* programs; this drives hand-written WGSL, and needs
/// a texture binding it has no reason to grow.
///
/// Compiled only under `--features offscreen`, and it **asserts** an adapter was
/// acquired rather than skipping. A parity test that silently passes when
/// nothing ran is worse than no parity test.
///
/// Three rows, not one: a sub-pass returns an `OwWeatherState` — nine floats —
/// and one `Rgba32Float` pixel carries four. Row *r* of column *i* is sample
/// *i*'s albedo / orm / n_shade.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::tests::samples;
    use super::{
        dust, hash11, rain, runoff, s_axis, splash, stack, streak_uv, vert_facing, wedge,
        WeatherInput, WeatherState, WEATHERING_WGSL,
    };

    /// How many samples one run compares. Also the target's width.
    const SAMPLES: usize = 24;
    /// Rows of the target: albedo, orm, n_shade.
    const ROWS: usize = 3;
    /// `vec4` slots of the input uniform, per sample.
    const SLOTS: usize = 16;
    /// `copy_texture_to_buffer` row alignment.
    const ROW_ALIGN: u32 = 256;

    /// **The exact tier's measured budget.** Every entry point except
    /// `ow_runoff` agrees with its CPU reference to better than this on the
    /// adapter the gate runs on — the worst observed is `2.18e-6`, in
    /// `ow_weather_stack` (which carries a `sin` and eleven blends). The
    /// per-entry numbers are recorded in
    /// `docs/work-manifests/shmup-port/notes/material-weathering.md`.
    const TOLERANCE: f32 = 5.0e-6;

    /// **`ow_runoff`'s budget, derived rather than measured** — the one place in
    /// this layer where a legal hardware liberty is amplified into something
    /// visible in the numbers.
    ///
    /// The mechanism, confirmed by reading the two sides' bits: the GPU contracts
    /// `cell * 1.37 + 3.1` (the column hash's argument) into a single-rounding
    /// `fma`, which moves it one ULP — `0xbf8147b0` against `0xbf8147af`.
    /// `owHash11` then squares its way up: `p * (p + 33.33)` reaches at most
    /// `34.33`, and `p * (p + p)` at most `2 * 34.33^2 ~ 2357`. The final `fract`
    /// therefore works on a number whose own representation quantises at `2^-12`,
    /// so **any** sub-ULP disagreement upstream lands as at most one step of
    /// `2.44e-4`, and no more: the two `p` values are adjacent `f32`s, not
    /// divergent ones. The observed worst is `1.92e-4`, which is that step.
    ///
    /// This is the source's own sensitivity, not a transcription error, and it is
    /// present in the original GLSL too — where the same contraction is expressly
    /// permitted. Widening the *exact* tier to swallow it would hide seven other
    /// entry points behind one function's coarseness, so it gets its own budget.
    ///
    /// Note the consequence for the rust bleed: `step(0.86, runoff.y)` is a hard
    /// threshold on a value this coarse, so a column whose `r1` sits within
    /// `2.44e-4` of `0.86` may take different arms on the two sides. That is a
    /// property of the shader; none of the samples sits there.
    ///
    /// `ow_weather_rain` and `ow_weather_stack` consume `runoff` and inherit the
    /// same mechanism, yet measure at the exact tier: at these samples every
    /// column whose hash diverges has a **zero** run, so the difference is
    /// multiplied out. They are deliberately held to the tight budget anyway — if
    /// a future sample change makes one fail by ~2e-4, the mechanism is this
    /// comment, and the fix is to move the sample or lift that entry to this
    /// budget, never to widen [`TOLERANCE`].
    const RUNOFF_TOLERANCE: f32 = 3.0e-4;

    /// The macro texel the stack's two fetches return. Four *different* values,
    /// so a swapped `.a`/`.g` channel selection fails rather than passing by
    /// coincidence. `Rgba8Unorm` is a linear format: no sRGB decode is involved.
    const MACRO_TEXEL: [u8; 4] = [51, 102, 153, 204];

    /// The harness: a fullscreen triangle, one fragment entry point per WGSL
    /// entry point this layer defines.
    const HARNESS_WGSL: &str = r#"
struct WParityIn { items: array<vec4<f32>, 384> };
@group(0) @binding(0) var<uniform> w_in: WParityIn;
@group(0) @binding(1) var w_tex: texture_2d<f32>;
@group(0) @binding(2) var w_smp: sampler;

@vertex
fn w_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

fn w_slot(sample: u32, slot: u32) -> vec4<f32> { return w_in.items[sample * 16u + slot]; }

fn w_state(i: u32) -> OwWeatherState {
    return OwWeatherState(w_slot(i, 0u).xyz, w_slot(i, 1u).xyz, w_slot(i, 2u).xyz);
}

fn w_row(s: OwWeatherState, row: u32) -> vec4<f32> {
    var rows = array<vec3<f32>, 3>(s.albedo, s.orm, s.n_shade);
    return vec4<f32>(rows[row], 0.0);
}

@fragment
fn w_scalars_fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(p.x);
    return vec4<f32>(
        ow_hash11(w_slot(i, 11u).w),
        ow_weather_vert(w_slot(i, 11u).y),
        ow_weather_s_axis(w_slot(i, 12u).xyz, w_slot(i, 11u).xyz),
        0.0,
    );
}

@fragment
fn w_runoff_fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(p.x);
    return vec4<f32>(
        ow_runoff(w_slot(i, 1u).w, w_slot(i, 2u).w, w_slot(i, 10u).w),
        0.0,
    );
}

@fragment
fn w_streak_uv_fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(p.x);
    return ow_weather_streak_uv(w_slot(i, 1u).w, w_slot(i, 2u).w);
}

@fragment
fn w_dust_fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(p.x);
    return w_row(ow_weather_dust(
        w_state(i), w_slot(i, 11u).y, w_slot(i, 6u).x,
        w_slot(i, 4u).b, w_slot(i, 5u).g, w_slot(i, 8u).xyz, w_slot(i, 3u).xyz,
    ), u32(p.y));
}

@fragment
fn w_rain_fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(p.x);
    return w_row(ow_weather_rain(
        w_state(i), w_slot(i, 0u).w, w_slot(i, 1u).w, w_slot(i, 2u).w,
        w_slot(i, 8u).w, w_slot(i, 9u).w, w_slot(i, 6u).y,
        w_slot(i, 7u).xyz, w_slot(i, 7u).w, w_slot(i, 9u).xyz, w_slot(i, 10u).xyz,
    ), u32(p.y));
}

@fragment
fn w_splash_fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(p.x);
    return w_row(ow_weather_splash(
        w_state(i), w_slot(i, 0u).w, w_slot(i, 12u).w, w_slot(i, 6u).z,
        w_slot(i, 4u).b, w_slot(i, 5u).g, w_slot(i, 9u).xyz, w_slot(i, 8u).xyz,
    ), u32(p.y));
}

@fragment
fn w_wedge_fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(p.x);
    return w_row(ow_weather_wedge(
        w_state(i), w_slot(i, 0u).w, w_slot(i, 12u).w, w_slot(i, 6u).z,
        w_slot(i, 4u).r, w_slot(i, 5u).b, w_slot(i, 5u).g,
        w_slot(i, 8u).xyz, w_slot(i, 3u).xyz,
    ), u32(p.y));
}

@fragment
fn w_stack_fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(p.x);
    return w_row(ow_weather_stack(
        w_state(i), w_slot(i, 12u).xyz, w_slot(i, 11u).xyz, w_slot(i, 3u).xyz,
        w_slot(i, 4u), w_slot(i, 5u), w_slot(i, 7u).xyz, w_slot(i, 7u).w,
        w_slot(i, 6u), w_slot(i, 3u).w,
        w_slot(i, 8u).xyz, w_slot(i, 9u).xyz, w_slot(i, 10u).xyz,
        w_tex, w_smp,
    ), u32(p.y));
}
"#;

    /// A real GPU, or a loud failure.
    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
        module: wgpu::ShaderModule,
    }

    impl Gpu {
        fn acquire() -> Gpu {
            // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
            // ~50 tests each opening their own is what crashes the driver.
            let gpu = crate::test_gpu::TestGpu::shared();
            let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
            // The error scope is the SHARED device's, so it is entered exclusively;
            // see `crate::test_gpu::validating`.
            let (module, failure) = crate::test_gpu::validating(&device, || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-weathering-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl([WEATHERING_WGSL, HARNESS_WGSL].concat().into()),
                })
            });
            assert!(
                failure.is_none(),
                "the weathering WGSL must compile"
            );
            Gpu {
                device,
                queue,
                backend: gpu.backend,
                module,
            }
        }

        /// Render `entry` over a `SAMPLES x ROWS` `Rgba32Float` target and read
        /// every lane back, indexed `[row][sample]`.
        fn render(&self, entry: &str, inputs: &[u8]) -> Vec<Vec<[f32; 4]>> {
            let layout =
                self.device
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("axiom-weathering-parity-bgl"),
                        entries: &[
                            wgpu::BindGroupLayoutEntry {
                                binding: 0,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Uniform,
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 1,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Texture {
                                    sample_type: wgpu::TextureSampleType::Float {
                                        filterable: true,
                                    },
                                    view_dimension: wgpu::TextureViewDimension::D2,
                                    multisampled: false,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 2,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Sampler(
                                    wgpu::SamplerBindingType::Filtering,
                                ),
                                count: None,
                            },
                        ],
                    });
            let uniform = wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-weathering-parity-inputs"),
                    contents: inputs,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            // A 1x1 macro texture: the fetch returns this texel for any uv, so
            // the CPU reference knows exactly what the GPU sampled while the
            // four differing channels still pin which channel each fetch takes.
            let macro_tex = wgpu::util::DeviceExt::create_texture_with_data(
                &self.device,
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("axiom-weathering-parity-macro"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &MACRO_TEXEL,
            );
            let macro_view = macro_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("axiom-weathering-parity-sampler"),
                ..Default::default()
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-weathering-parity-bg"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&macro_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-weathering-parity-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-weathering-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &self.module,
                        entry_point: Some("w_vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &self.module,
                        entry_point: Some(entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba32Float,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
            let extent = wgpu::Extent3d {
                width: SAMPLES as u32,
                height: ROWS as u32,
                depth_or_array_layers: 1,
            };
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-weathering-parity-target"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-weathering-parity-readback"),
                size: u64::from(row_bytes) * ROWS as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-weathering-parity-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: Some(ROWS as u32),
                    },
                },
                extent,
            );
            self.queue.submit(Some(encoder.finish()));
            let slice = readback.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            self.device
                .poll(wgpu::PollType::Wait)
                .expect("the readback must complete");
            let mapped = slice.get_mapped_range();
            (0..ROWS)
                .map(|row| {
                    (0..SAMPLES)
                        .map(|sample| {
                            [0_usize, 1, 2, 3].map(|lane| {
                                let at =
                                    row * row_bytes as usize + sample * 16 + lane * 4;
                                f32::from_le_bytes([
                                    mapped[at],
                                    mapped[at + 1],
                                    mapped[at + 2],
                                    mapped[at + 3],
                                ])
                            })
                        })
                        .collect()
                })
                .collect()
        }
    }

    /// The uniform's bytes: `SLOTS` `vec4` per sample, in the layout the harness
    /// unpacks.
    fn input_bytes(cases: &[(WeatherState, WeatherInput)]) -> Vec<u8> {
        let mut bytes: Vec<u8> = cases
            .iter()
            .flat_map(|(state, input)| {
                let vert = vert_facing(input.nw[1]);
                let axis = s_axis(input.world_pos, input.nw);
                let h_above = input.world_pos[1] - input.ground_y;
                let mut slots = [[0.0_f32; 4]; SLOTS];
                slots[0] = [state.albedo[0], state.albedo[1], state.albedo[2], vert];
                slots[1] = [state.orm[0], state.orm[1], state.orm[2], axis];
                slots[2] = [
                    state.n_shade[0],
                    state.n_shade[1],
                    state.n_shade[2],
                    input.world_pos[1],
                ];
                slots[3] = [
                    input.n_flat[0],
                    input.n_flat[1],
                    input.n_flat[2],
                    input.ground_y,
                ];
                slots[4] = input.mac1;
                slots[5] = input.mac2;
                slots[6] = input.weather;
                slots[7] = [
                    input.vcolor[0],
                    input.vcolor[1],
                    input.vcolor[2],
                    input.vcol_masks,
                ];
                slots[8] = [
                    input.dust_col[0],
                    input.dust_col[1],
                    input.dust_col[2],
                    input.s_n,
                ];
                slots[9] = [
                    input.grime_col[0],
                    input.grime_col[1],
                    input.grime_col[2],
                    input.s_fine,
                ];
                slots[10] = [
                    input.rust_col[0],
                    input.rust_col[1],
                    input.rust_col[2],
                    input.s_n - 0.5,
                ];
                slots[11] = [input.nw[0], input.nw[1], input.nw[2], input.s_fine * 37.0];
                slots[12] = [
                    input.world_pos[0],
                    input.world_pos[1],
                    input.world_pos[2],
                    h_above,
                ];
                slots
            })
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect();
        bytes.resize(SAMPLES * SLOTS * 16, 0);
        bytes
    }

    /// A `WeatherState` as the three target rows.
    fn rows_of(state: WeatherState) -> [[f32; 4]; ROWS] {
        [state.albedo, state.orm, state.n_shade]
            .map(|lanes| [lanes[0], lanes[1], lanes[2], 0.0])
    }

    /// The worst absolute lane delta, naming the entry point — the measurement a
    /// tolerance is set from, not fitted to.
    fn assert_rows(
        entry: &str,
        tolerance: f32,
        expected: &[[[f32; 4]; ROWS]],
        actual: &[Vec<[f32; 4]>],
    ) -> f32 {
        (0..ROWS).fold(0.0_f32, |worst, row| {
            expected
                .iter()
                .enumerate()
                .fold(worst, |worst, (sample, want)| {
                    (0..4).fold(worst, |worst, lane| {
                        let delta = (want[row][lane] - actual[row][sample][lane]).abs();
                        assert!(
                            delta <= tolerance,
                            "{entry} disagrees at sample {sample} row {row} lane {lane}: \
                             CPU {} vs GPU {} (delta {delta}, tolerance {tolerance})",
                            want[row][lane],
                            actual[row][sample][lane]
                        );
                        worst.max(delta)
                    })
                })
        })
    }

    /// The same, for the entry points whose answer is one `vec4` — those write
    /// the same value into every row, so only row 0 is read.
    fn assert_row0(
        entry: &str,
        tolerance: f32,
        expected: &[[f32; 4]],
        actual: &[Vec<[f32; 4]>],
    ) -> f32 {
        expected
            .iter()
            .enumerate()
            .fold(0.0_f32, |worst, (sample, want)| {
                (0..4).fold(worst, |worst, lane| {
                    let delta = (want[lane] - actual[0][sample][lane]).abs();
                    assert!(
                        delta <= tolerance,
                        "{entry} disagrees at sample {sample} lane {lane}: \
                         CPU {} vs GPU {} (delta {delta}, tolerance {tolerance})",
                        want[lane],
                        actual[0][sample][lane]
                    );
                    worst.max(delta)
                })
            })
    }

    /// **The two `floor` seams this layer contains, and why the samples avoid
    /// them.**
    ///
    /// `owRunoff` takes `floor(sAxis * 1.55)` (the column index) and
    /// `floor((y + jitter) / 2.85)` (the storey the source sits on). Both are
    /// genuine discontinuities in the *algorithm* — a column boundary is where
    /// one streak stops and another starts — and WGSL guarantees division only
    /// to 2.5 ULP, so a fragment sitting exactly on a seam may legally floor to
    /// different integers on the two sides and diverge by the full height of the
    /// step. That is the hardware being within spec, not a transcription error,
    /// and a tolerance wide enough to swallow it would prove nothing.
    ///
    /// So the sample set stays clear of both seams, and this is the assertion
    /// that keeps it that way if someone edits the samples.
    fn assert_samples_avoid_the_floor_seams(cases: &[(WeatherState, WeatherInput)]) {
        cases.iter().enumerate().for_each(|(index, (_, input))| {
            let axis = s_axis(input.world_pos, input.nw);
            let column = super::fract(axis * 1.55);
            assert!(
                (0.02..0.98).contains(&column),
                "sample {index} sits on a runoff COLUMN seam (fract {column}); \
                 move it, do not widen the tolerance"
            );
            let cell = (axis * 1.55).floor();
            let jitter = super::hash11(cell * 2.71 + 11.7) * 1.2
                + super::hash11(cell * 1.37 + 3.1) * 0.5;
            let storey = super::fract((input.world_pos[1] + jitter) / 2.85);
            assert!(
                (0.02..0.98).contains(&storey),
                "sample {index} sits on a runoff STOREY seam (fract {storey}); \
                 move it, do not widen the tolerance"
            );
        });
    }

    /// **The whole layer, on a real GPU.** Every WGSL entry point against its CPU
    /// reference, at the documented tolerance, with the worst measured delta
    /// reported so the budget stays honest.
    #[test]
    fn every_weathering_entry_point_agrees_with_the_cpu_reference() {
        let gpu = Gpu::acquire();
        assert_ne!(
            gpu.backend,
            wgpu::Backend::Noop,
            "the parity proof is worthless unless a real backend ran it"
        );
        let cases = samples();
        assert_samples_avoid_the_floor_seams(&cases);
        let bytes = input_bytes(&cases);

        // --- the scalar helpers -------------------------------------------
        let scalars: Vec<[f32; 4]> = cases
            .iter()
            .map(|(_, input)| {
                [
                    hash11(input.s_fine * 37.0),
                    vert_facing(input.nw[1]),
                    s_axis(input.world_pos, input.nw),
                    0.0,
                ]
            })
            .collect();
        let worst_scalars = assert_row0(
            "ow_hash11 / ow_weather_vert / ow_weather_s_axis",
            TOLERANCE,
            &scalars,
            &gpu.render("w_scalars_fs", &bytes),
        );

        // --- owRunoff ------------------------------------------------------
        let runoffs: Vec<[f32; 4]> = cases
            .iter()
            .map(|(_, input)| {
                let r = runoff(
                    s_axis(input.world_pos, input.nw),
                    input.world_pos[1],
                    input.s_n - 0.5,
                );
                [r[0], r[1], r[2], 0.0]
            })
            .collect();
        let worst_runoff = assert_row0(
            "ow_runoff",
            RUNOFF_TOLERANCE,
            &runoffs,
            &gpu.render("w_runoff_fs", &bytes),
        );
        // A run field that read as a constant would satisfy a tolerance check
        // against a constant CPU side, so prove the signal actually varies —
        // and that at least one sample is a real, strong streak.
        let spread = runoffs
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), r| (lo.min(r[0]), hi.max(r[0])));
        assert!(
            spread.1 - spread.0 > 0.2,
            "the sampled runoff must vary, or the parity is vacuous (got {spread:?})"
        );

        // --- the streak fetch coordinates ----------------------------------
        let uvs: Vec<[f32; 4]> = cases
            .iter()
            .map(|(_, input)| {
                streak_uv(s_axis(input.world_pos, input.nw), input.world_pos[1])
            })
            .collect();
        let worst_uv = assert_row0(
            "ow_weather_streak_uv",
            TOLERANCE,
            &uvs,
            &gpu.render("w_streak_uv_fs", &bytes),
        );

        // --- the four sub-passes, each on its own -------------------------
        let of = |f: &dyn Fn(&WeatherState, &WeatherInput) -> WeatherState| {
            cases
                .iter()
                .map(|(state, input)| rows_of(f(state, input)))
                .collect::<Vec<_>>()
        };
        let worst_dust = assert_rows(
            "ow_weather_dust",
            TOLERANCE,
            &of(&|state, input| {
                dust(
                    *state,
                    input.nw[1],
                    input.weather[0],
                    input.mac1[2],
                    input.mac2[1],
                    input.dust_col,
                    input.n_flat,
                )
            }),
            &gpu.render("w_dust_fs", &bytes),
        );
        let worst_rain = assert_rows(
            "ow_weather_rain",
            TOLERANCE,
            &of(&|state, input| {
                rain(
                    *state,
                    vert_facing(input.nw[1]),
                    s_axis(input.world_pos, input.nw),
                    input.world_pos[1],
                    input.s_n,
                    input.s_fine,
                    input.weather[1],
                    input.vcolor,
                    input.vcol_masks,
                    input.grime_col,
                    input.rust_col,
                )
            }),
            &gpu.render("w_rain_fs", &bytes),
        );
        let worst_splash = assert_rows(
            "ow_weather_splash",
            TOLERANCE,
            &of(&|state, input| {
                splash(
                    *state,
                    vert_facing(input.nw[1]),
                    input.world_pos[1] - input.ground_y,
                    input.weather[2],
                    input.mac1[2],
                    input.mac2[1],
                    input.grime_col,
                    input.dust_col,
                )
            }),
            &gpu.render("w_splash_fs", &bytes),
        );
        let worst_wedge = assert_rows(
            "ow_weather_wedge",
            TOLERANCE,
            &of(&|state, input| {
                wedge(
                    *state,
                    vert_facing(input.nw[1]),
                    input.world_pos[1] - input.ground_y,
                    input.weather[2],
                    input.mac1[0],
                    input.mac2[2],
                    input.mac2[1],
                    input.dust_col,
                    input.n_flat,
                )
            }),
            &gpu.render("w_wedge_fs", &bytes),
        );

        // --- the whole stack, including the two real texture fetches -------
        //
        // The 1x1 macro texture returns MACRO_TEXEL for any uv, so `s_n` is its
        // ALPHA and `s_fine` its GREEN — four distinct values, so a swapped
        // channel selection cannot pass.
        let sampled = |channel: usize| f32::from(MACRO_TEXEL[channel]) / 255.0;
        let worst_stack = assert_rows(
            "ow_weather_stack",
            TOLERANCE,
            &of(&|state, input| {
                stack(
                    *state,
                    WeatherInput {
                        s_n: sampled(3),
                        s_fine: sampled(1),
                        ..*input
                    },
                )
            }),
            &gpu.render("w_stack_fs", &bytes),
        );

        // This was an `eprintln!` narrating the eight worst deltas on the
        // success path. It is gone: no layer or module in this engine emits
        // console output, tests included (Module Law #10, enforced by the
        // architecture checker). The recorded figures live in
        // `notes/material-weathering.md`.
        //
        // It is NOT replaced by an aggregate `max(..) <= TOLERANCE`. That was
        // tried and it failed at 1.92e-4, which is the correct answer to the
        // wrong question: `runoff` is deliberately held to [`RUNOFF_TOLERANCE`]
        // (3.0e-4), a separately derived budget for a genuine hash-arm
        // divergence, precisely so that `TOLERANCE` never had to be widened for
        // it. Folding the eight into one number erases that distinction and
        // would have pressured exactly the widening the split exists to avoid.
        //
        // Each sub-pass is already asserted against its own budget by
        // `assert_rows`, which names the sub-pass, the row and the delta on
        // failure. That is the check; there is nothing left for an aggregate to
        // add.
        let _ = (
            worst_scalars, worst_runoff, worst_uv, worst_dust,
            worst_rain, worst_splash, worst_wedge, worst_stack,
        );
    }
}
