//! How screen-space occlusion, contact shadow, reflection and the two-band
//! bounce fill compose into the main pass's lighting — the port of
//! `C:/dev/Claude-of-Duty/src/render/materialpatch.js` (321 lines).
//!
//! # What that file actually is
//!
//! Its name says "material patch" and its mechanism is `onBeforeCompile`
//! surgery, so it reads like the usual Three.js chunk-injection scaffolding.
//! It is not. Strip the scaffolding and what remains is **one lighting
//! decision, expressed as five functions and three injection sites**: ambient
//! occlusion belongs on *indirect* light (never on the sun), a contact shadow
//! belongs on *the sun only*, a screen-space reflection *replaces* the
//! image-based specular rather than adding to it, and a two-band hemispheric
//! fill — cool sky above, warm street below, gated by the normal and by a
//! coarse interior-volume test — is what keeps shadowed geometry from
//! collapsing to black once AO has eaten the indirect term.
//!
//! It patches nothing to do with velocity, the depth prepass, the G-buffer or
//! TAA jitter. Those lanes are [`crate::gbuffer`]'s and are already ported.
//!
//! # Verdict, category by category
//!
//! **Category 1 — already solved structurally by the splice, and dropped.**
//!
//! | source mechanism | why it does not exist here |
//! |---|---|
//! | `onBeforeCompile` + three `String.replace` calls on `#include <…>` markers | Axiom owns its whole shader text. [`crate::scene_wgsl`] is a prefix, a program-shaped hole and a suffix, spliced by concatenation in `crate::surface_program::wgsl_template::scene_shader`. There is no chunk to inject into because there is no chunk system: the splice point already exists. |
//! | `PATCH_VERSION = 9` folded into `customProgramCacheKey` | `crate::surface_program::cache` is keyed on `axiom_surface::Surface::digest`, a structural content hash. A hand-bumped version integer is what you need when the key cannot see the text; this key *is* the text. |
//! | `this._patched` (a `WeakSet` re-entrancy guard) and `prevHook` chaining | A program is generated once from its digest and deduplicated by it. There is no second patcher to chain behind, and applying the same program twice is not expressible. |
//! | one shared `uniforms` object so a single write updates every material | The frame's lighting uniform is group 1, written once per frame and set once per pass. Sharing is the construction, not a trick. |
//! | `setScreenSize(w, h)` → `owScreenTexel` | A screen-space consumer reads `textureDimensions` of the buffer it samples, which cannot go stale against a resize. |
//! | `owWP = cameraPosition + geometryPosition * mat3(viewMatrix)` and `owWN = inverseTransformDirection(normal, viewMatrix)` | Both are conversions *back* out of three's view-space fragment stage. Axiom's fragment stage interpolates `world_pos` and shades in world space already, so the two conversions have no term to convert. |
//!
//! **Category 2 — real capability Axiom lacks. Ported here.** Every function
//! below: [`sample_ao`], [`contact_shadow`], [`multi_bounce`],
//! [`specular_occlusion`], [`interior_gate`], [`sun_bounce`], the direct-light
//! composition [`direct_light`], and the whole `lights_fragment_maps` body as
//! [`indirect`]. None of it exists anywhere in this crate today: the main pass
//! has no AO input, no contact-shadow input, no reflection input and no fill
//! bands — its entire indirect term is `hemi * ambient_shade`.
//!
//! **Category 3 — Three-specific, with no analogue. Named and dropped.**
//! `MaterialPatcher.isLit`, the five `m.isMeshStandardMaterial`-style duck
//! tests. Axiom states participation in lighting as a value —
//! `axiom_surface::LightingModel`, read by `axiom_lighting_model()` in the
//! suffix — so "does this material run the lighting pipeline" is a
//! discriminant the program already carries, not a class sniff. `dispose()`
//! (clearing the `WeakSet`) and `material.needsUpdate = true` go with it.
//!
//! # What this module is *not* wired to yet, and what would make it live
//!
//! Nothing calls it. Three separate things must land first, and each is a
//! different slice:
//!
//! 1. **The AO and contact inputs.** [`sample_ao`] and [`contact_shadow`] take
//!    the already-sampled texel value, so they are complete — but nobody
//!    produces those texels. `render/gtao.js` (324 lines) and
//!    `render/contact.js` are unported. **Expiry check:** when a `gtao` module
//!    lands in this crate, `crate::scene_renderer` must bind its output and the
//!    suffix in `scene_wgsl.rs` must call `axiom_indirect_sample_ao`.
//! 2. **The `radiance` / `iblIrradiance` terms.** Axiom has **no environment
//!    probe**: `scene_wgsl.rs`'s own comment records that `indirectSpecular` is
//!    an exact zero because there is no cubemap. So [`indirect`]'s
//!    `ibl_irradiance` and `radiance` lanes, and [`ssr_blend`] entirely,
//!    operate on a term that is currently zero — the maths is ported and
//!    correct, and it multiplies nothing. **Expiry check:** when
//!    `render/probe.js` (306 lines) lands and the main pass gains a PMREM
//!    binding, those two lanes stop being zero and this module is already
//!    right. Do not delete them in the meantime; a zero input is not a reason
//!    to drop a term.
//! 3. **The room volumes.** [`interior_gate`] is the *test*; the *builder* is
//!    `RenderSystem._updateRooms` in `render/index.js:1167`, which is app-tier
//!    frame-graph work and is unported. Until it runs, `indirect[2]` (the live
//!    room count) is zero and the gate degrades to its AO arm alone — which is
//!    exactly what the source does before the world appears.
//!
//! # Two source facts that read as uniforms and are not
//!
//! Grepped across the whole of `C:/dev/Claude-of-Duty/src`: **`owAoStrength` is
//! never written** after the constructor and **`owFillDir` is never written at
//! all.** Only `owFillGain`, `owSkyFill`, `owGroundFill`, `owIndirect`,
//! `owRoomXf`, `owRooms`/`owRoomsY` and `owFeat` are driven per-frame. So
//! [`AO_STRENGTH`] and [`FILL_DIR`] are constants of the algorithm wearing a
//! uniform's clothes, and the micro-shadow fraction the direct light actually
//! receives is a fixed `1.0 * 0.35`. They are still *carried* as parameters
//! here, because the source carries them and a frame graph may yet drive them;
//! their shipped values are pinned by test so the fact cannot rot.
//!
//! # Transcription
//!
//! From the GLSL text in `materialpatch.js`'s `EXTRA_PARS` and its three
//! injected bodies, never from the Rust. Specifically preserved:
//!
//! - `owSunBounce`'s `/ 1.12` is a **division**, not a multiply by
//!   `0.892857…`. It is the shape this port has already been bitten by five
//!   times.
//! - `owMultiBounce`'s Horner nesting `ao * ( ao * ( ao * a + b ) + c )`.
//! - `owInteriorGate`'s `min( min( A, B ), min( C, D ) )` pairing.
//! - the fill add's `( skyFill * skyG + groundFill * gndG * indoor ) * ( fillAo
//!   * fillGain.x )` — one vector sum scaled once, not two scaled adds.
//! - the sun-bounce add's left-to-right scalar chain `sunBounce * fillGain.y *
//!   fillAo * indoor`.
//! - two **successive** multiplies onto the direct light (shadow, then
//!   micro-shadow), never folded into one gain: float multiply is not
//!   associative.
//!
//! `mix`, `smoothstep` and `clamp` are written out in the factoring the GLSL ES
//! spec gives them — `mix(x, y, a)` is `x * (1 - a) + y * a`, **not**
//! `x + (y - x) * a` — on both the CPU and the WGSL side, so a driver is never
//! handed a licence the source did not give it. `normalize` is written out as
//! `v / sqrt(dot(v, v))` on both sides for the same reason: neither GLSL nor
//! WGSL pins the builtin's factoring, and pinning it to the same one on both
//! sides is the property a parity proof can actually assert.

/// The **CPU half** of this port: `index.js::_updateRooms`, which derives the
/// interior volumes this module's [`interior_gate`] then reads out of a uniform.
///
/// The two were briefed as separate slices and converged on the same source,
/// which is how the port got two independent transcriptions of `sun_bounce`,
/// `interior_gate` and the room-depth reduction. **They agreed everywhere** —
/// same epsilon placement, same written-out `normalize`, same trailing division
/// left as a division. That agreement is the third reference the CSM slice's
/// self-confirming proof turned out to need, so it is recorded rather than
/// discarded: see `docs/work-manifests/shmup-port/notes/render-indirect-probe-env.md`.
///
/// What survives here is the split by *side*, not by source file. This file is
/// the fragment-shader composition (`materialpatch.js`); [`volumes`] is the CPU
/// derivation that feeds it, plus the band constants the fill is authored in.
pub(crate) mod volumes;


/// `MAX_ROOMS` — the coarse interior volumes the indirect gate can hold. The
/// GLSL declares `#define OW_ROOMS 10` from this same constant and loops to it
/// unconditionally, breaking at the live count.
pub(crate) const MAX_ROOMS: usize = 10;

/// `owAoStrength`'s shipped value: `x` = how much of the AO buffer reaches the
/// indirect diffuse, `y` = how much of the specular occlusion reaches the
/// image-based specular.
///
/// Never written after construction anywhere in the source (see the module
/// header), so this is the value every frame of the original runs.
pub(crate) const AO_STRENGTH: [f32; 2] = [1.0, 0.6];

/// `owFillDir`'s shipped value: `xy` = the sky band's `smoothstep` edges over
/// `dot(N, up)`, `zw` = the ground band's over `-dot(N, up)`.
///
/// Never written anywhere in the source. Note `x` is `-0.95`: the sky gate is a
/// cosine-hemisphere visibility ramp that reaches almost the whole sphere, not
/// a narrow band — the source's comment records that clipping it to a third of
/// the band was most of why a shaded facade came back dead neutral.
pub(crate) const FILL_DIR: [f32; 4] = [-0.95, 0.85, -0.05, 0.7];

/// The fraction of the AO term the **direct** light receives, from the second
/// injected multiply in the directional-light loop:
/// `directLight.color *= mix( 1.0, owSampleAO(), owAoStrength.x * 0.35 );`
///
/// AO belongs on indirect light, and the source says so in the same breath —
/// this is the deliberate exception. A cascade texel is tens of centimetres
/// wide and the contact ray runs only along the sun direction, so the last
/// centimetre of a wall/soffit junction gets occlusion from neither, and the
/// frame comes back with razor-sharp junctions and nothing grounded.
pub(crate) const MICRO_SHADOW_FRACTION: f32 = 0.35;

/// GLSL `mix(x, y, a)`, in the factoring the spec gives it: `x⋅(1−a)+y⋅a`.
///
/// **Not** `x + (y - x) * a`. The two are algebraically equal and numerically
/// different, and this crate contains both forms today (see
/// `docs/work-manifests/shmup-port/notes/material-patch.md` §5).
fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// Componentwise [`mix`].
fn mix3(x: [f32; 3], y: [f32; 3], a: f32) -> [f32; 3] {
    [0_usize, 1, 2].map(|lane| mix(x[lane], y[lane], a))
}

/// GLSL `clamp(x, lo, hi)` — `min(max(x, lo), hi)`, written out.
fn clamp(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}

/// GLSL `smoothstep(e0, e1, x)`, written out.
///
/// Called with `e0 > e1` on purpose by [`ssr_blend`]'s roughness ramp. The spec
/// leaves that case to the implementation and every implementation evaluates
/// this formula, which yields a descending ramp — the same reading
/// `crate::cascade::shading` records for the CSM far fade.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Componentwise product.
fn mul3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [0_usize, 1, 2].map(|lane| a[lane] * b[lane])
}

/// Vector times scalar.
fn scale3(a: [f32; 3], s: f32) -> [f32; 3] {
    [0_usize, 1, 2].map(|lane| a[lane] * s)
}

/// Componentwise sum.
fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [0_usize, 1, 2].map(|lane| a[lane] + b[lane])
}

/// `owSampleAO()` — the AO buffer's visibility, floored and strength-lerped.
///
/// ```glsl
/// float owSampleAO() {
///   if ( owFeat.x < 0.5 ) return 1.0;
///   float ao = texture2D( owAoTex, gl_FragCoord.xy * owScreenTexel ).r;
///   return mix( 1.0, max( ao, 0.25 ), owAoStrength.x );
/// }
/// ```
///
/// `ao_texel_r` is the already-sampled red channel, so this function has no
/// texture in it and the same definition drives the CPU reference and the
/// adapter proof. The `0.25` floor is the source's: real crevices are filled by
/// multiply-scattered light, and a visibility term that reaches 0 is a dark
/// halo rather than occlusion.
pub(crate) fn sample_ao(feat_x: f32, ao_texel_r: f32, ao_strength_x: f32) -> f32 {
    // `if ( owFeat.x < 0.5 ) return 1.0;` — the early return as a value select.
    let enabled = feat_x >= 0.5;
    let sampled = mix(1.0, ao_texel_r.max(0.25), ao_strength_x);
    [1.0, sampled][usize::from(enabled)]
}

/// `owContactShadow( vec3 lightDirView )` — the screen-space contact ray,
/// multiplied onto the **sun** term only.
///
/// ```glsl
/// float owContactShadow( vec3 lightDirView ) {
///   if ( owFeat.y < 0.5 ) return 1.0;
///   if ( dot( lightDirView, owSunDirView ) < 0.999 ) return 1.0;
///   return texture2D( owContactTex, gl_FragCoord.xy * owScreenTexel ).r;
/// }
/// ```
///
/// `light_dot_sun_view` is that dot product, computed by the caller. The
/// `0.999` test is how the source picks the sun out of an unrolled light loop,
/// and **Axiom needs it more than three does, not less**: the main pass's loop
/// runs up to 16 lights and applies its one `shadow_factor` to *every*
/// directional among them, so without this test a second directional would
/// receive the sun's contact ray. `crate::cascade::adapter_proof` drops the
/// same test on the grounds that there is one shadow-casting directional; that
/// is true of the *shadow map* and not of the *loop*, and the two slices should
/// be reconciled when either is wired.
pub(crate) fn contact_shadow(feat_y: f32, light_dot_sun_view: f32, contact_texel_r: f32) -> f32 {
    let enabled = (feat_y >= 0.5) & (light_dot_sun_view >= 0.999);
    [1.0, contact_texel_r][usize::from(enabled)]
}

/// `owMultiBounce( float ao, vec3 albedo )` — Jimenez's GTAO multi-bounce fit.
///
/// ```glsl
/// vec3 a = 2.0404 * albedo - 0.3324;
/// vec3 b = -4.7951 * albedo + 0.6417;
/// vec3 c = 2.7552 * albedo + 0.6903;
/// return clamp( ao * ( ao * ( ao * a + b ) + c ), vec3( ao ), vec3( 1.0 ) );
/// ```
///
/// Dark albedos occlude more than bright ones, which is what stops AO turning
/// white plaster into grey mud. The lower clamp is `ao` itself, so the fit can
/// never darken *below* the raw visibility.
pub(crate) fn multi_bounce(ao: f32, albedo: [f32; 3]) -> [f32; 3] {
    [0_usize, 1, 2].map(|lane| {
        let channel = albedo[lane];
        let a = 2.0404 * channel - 0.3324;
        let b = -4.7951 * channel + 0.6417;
        let c = 2.7552 * channel + 0.6903;
        clamp(ao * (ao * (ao * a + b) + c), ao, 1.0)
    })
}

/// `owSpecularOcclusion( float ao, float rough )` — rough surfaces gather from
/// a wide cone, so they see more occlusion.
///
/// ```glsl
/// float r2 = rough * rough;
/// return clamp( pow( max( ao, 0.0 ), 1.0 + r2 * 2.0 ), 0.0, 1.0 );
/// ```
pub(crate) fn specular_occlusion(ao: f32, rough: f32) -> f32 {
    let r2 = rough * rough;
    clamp(ao.max(0.0).powf(1.0 + r2 * 2.0), 0.0, 1.0)
}

/// `owSunBounce( vec3 worldNormal )` — wrapped diffuse from the anti-sun
/// hemisphere: the wall in shade lit by the sunlit wall across the street.
///
/// ```glsl
/// vec3 anti = normalize( vec3( -owSunDirWorld.x, 0.28, -owSunDirWorld.z ) + vec3( 1e-4 ) );
/// return clamp( ( dot( worldNormal, anti ) + 0.12 ) / 1.12, 0.0, 1.0 );
/// ```
///
/// The wrap is `0.12`, not the more usual `0.35`: a face turned away from the
/// sunlit side of the street receives none of the bounce, and a wide wrap is
/// what let this warm term reach every surface in the frame at once. The `1e-4`
/// lands on **all three** components (`+ vec3( 1e-4 )`), including the constant
/// `0.28`, so the vector is never exactly zero and `normalize` is never
/// undefined. The trailing `/ 1.12` is a division and stays one.
pub(crate) fn sun_bounce(world_normal: [f32; 3], sun_dir_world: [f32; 3]) -> f32 {
    let raw = [
        -sun_dir_world[0] + 1e-4,
        0.28 + 1e-4,
        -sun_dir_world[2] + 1e-4,
    ];
    let length = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
    let anti = [raw[0] / length, raw[1] / length, raw[2] / length];
    let facing = world_normal[0] * anti[0] + world_normal[1] * anti[1] + world_normal[2] * anti[2];
    clamp((facing + 0.12) / 1.12, 0.0, 1.0)
}

/// The uniform block `materialpatch.js` shares across every patched material,
/// as this port carries it. One value per source uniform, same names.
///
/// `sun_dir_world` is not the patcher's own — it arrives from the CSM chunk
/// (`csm.js` declares `owSunDirWorld`/`owSunDirView`), which is why the patcher
/// spreads `csmUniforms` into its object before adding its own. Carried here
/// because [`sun_bounce`] reads it and this module must be evaluable on its
/// own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct IndirectUniforms {
    /// `owFeat`: `x` = AO enable, `y` = contact enable, `z` = SSR enable,
    /// `w` = AO power (declared `1.0`; read by nothing in the source).
    pub(crate) feat: [f32; 4],
    /// `owAoStrength`: diffuse, specular. See [`AO_STRENGTH`].
    pub(crate) ao_strength: [f32; 2],
    /// `owSkyFill`: the cool upper band's irradiance.
    pub(crate) sky_fill: [f32; 3],
    /// `owGroundFill`: the warm lower band's irradiance.
    pub(crate) ground_fill: [f32; 3],
    /// `owFillGain`: `x` = hemispheric gain, `y` = warm sun-bounce wrap gain.
    pub(crate) fill_gain: [f32; 2],
    /// `owFillDir`. See [`FILL_DIR`].
    pub(crate) fill_dir: [f32; 4],
    /// `owIndirect`: `x` = the image-based diffuse budget, `y` = the indirect
    /// floor inside an interior volume, `z` = the live room count, `w` unused.
    pub(crate) indirect: [f32; 4],
    /// `owRoomXf`: the world → level-space 2D transform, `(cos, sin, tx, tz)`.
    pub(crate) room_xf: [f32; 4],
    /// `owRooms`: coarse interior volumes in level space, `(cx, cz, hx, hz)`.
    pub(crate) rooms: [[f32; 4]; MAX_ROOMS],
    /// `owRoomsY`: the same volumes' vertical extent, `(y0, y1, _, _)`.
    pub(crate) rooms_y: [[f32; 4]; MAX_ROOMS],
    /// `owSunDirWorld`, from the CSM chunk.
    pub(crate) sun_dir_world: [f32; 3],
}

impl IndirectUniforms {
    /// The block exactly as `MaterialPatcher`'s constructor initialises it,
    /// before any frame writes to it: every feature off, no rooms, no fill.
    ///
    /// This is a real state the original runs in — the first frame, and every
    /// frame on a quality tier with `gtao`/`ssr` disabled — so it is the
    /// identity this module is measured against, not a convenience default.
    pub(crate) const fn shipped() -> IndirectUniforms {
        IndirectUniforms {
            feat: [0.0, 0.0, 0.0, 1.0],
            ao_strength: AO_STRENGTH,
            sky_fill: [0.0, 0.0, 0.0],
            ground_fill: [0.0, 0.0, 0.0],
            fill_gain: [1.0, 1.0],
            fill_dir: FILL_DIR,
            indirect: [1.0, 1.0, 0.0, 0.0],
            room_xf: [1.0, 0.0, 0.0, 0.0],
            rooms: [[0.0; 4]; MAX_ROOMS],
            rooms_y: [[0.0; 4]; MAX_ROOMS],
            sun_dir_world: [0.0, 1.0, 0.0],
        }
    }
}

/// `owInteriorGate( vec3 worldPos, float ao )` — `1.0` outdoors, falling to
/// `owIndirect.y` deep inside a coarse interior volume.
///
/// ```glsl
/// float indoor = 0.0;
/// if ( owIndirect.z > 0.5 ) {
///   float lx = worldPos.x * owRoomXf.x + worldPos.z * owRoomXf.y + owRoomXf.z;
///   float lz = -worldPos.x * owRoomXf.y + worldPos.z * owRoomXf.x + owRoomXf.w;
///   int n = int( owIndirect.z );
///   for ( int i = 0; i < OW_ROOMS; i ++ ) {
///     if ( i >= n ) break;
///     vec4 r = owRooms[ i ];
///     vec4 ry = owRoomsY[ i ];
///     float d = min(
///       min( r.z - abs( lx - r.x ), r.w - abs( lz - r.y ) ),
///       min( worldPos.y - ry.x, ry.y - worldPos.y ) );
///     indoor = max( indoor, smoothstep( 0.06, 0.30, d ) );
///   }
/// }
/// float aoGate = mix( 1.0, smoothstep( 0.45, 0.98, ao ), 0.6 );
/// float g = min( 1.0 - indoor, aoGate );
/// return mix( owIndirect.y, 1.0, clamp( g, 0.0, 1.0 ) );
/// ```
///
/// The volumes are tested by **depth inside the box**, not by containment: a
/// facade's outer skin sits exactly on the footprint boundary at depth 0 and
/// its inner skin one wall thickness in, so the 6 cm → 30 cm feather separates
/// the two faces of one wall without any per-room geometry. That is the whole
/// reason the gate can be four numbers per building.
///
/// The `for`/`break` becomes `take(n).fold(max)` — the source's break is a
/// bound, and the accumulator is a maximum, so the two are the same value. The
/// `owIndirect.z > 0.5` guard becomes a count of zero, which is the same
/// `indoor = 0.0`.
pub(crate) fn interior_gate(world_pos: [f32; 3], ao: f32, u: &IndirectUniforms) -> f32 {
    let live = u.indirect[2] > 0.5;
    // `int( owIndirect.z )`, then the loop's own `i < OW_ROOMS` bound.
    let declared = (u.indirect[2] as i32).max(0) as usize;
    let counted = [0, declared.min(MAX_ROOMS)][usize::from(live)];
    let lx = world_pos[0] * u.room_xf[0] + world_pos[2] * u.room_xf[1] + u.room_xf[2];
    let lz = -world_pos[0] * u.room_xf[1] + world_pos[2] * u.room_xf[0] + u.room_xf[3];
    let indoor = u
        .rooms
        .iter()
        .zip(u.rooms_y.iter())
        .take(counted)
        .fold(0.0_f32, |acc, (r, ry)| {
            let depth = (r[2] - (lx - r[0]).abs())
                .min(r[3] - (lz - r[1]).abs())
                .min((world_pos[1] - ry[0]).min(ry[1] - world_pos[1]));
            acc.max(smoothstep(0.06, 0.30, depth))
        });
    // Even outside a tagged volume, a pocket the sky genuinely cannot see keeps
    // less skylight — arcades, stairwells, under-awning stalls. Mixed at 0.6 so
    // it shapes rather than doubling up as a second AO multiply.
    let ao_gate = mix(1.0, smoothstep(0.45, 0.98, ao), 0.6);
    let gate = (1.0 - indoor).min(ao_gate);
    mix(u.indirect[1], 1.0, clamp(gate, 0.0, 1.0))
}

/// One directional light's colour after both injected multiplies, applied in
/// the source's order.
///
/// ```glsl
/// directLight.color *= receiveShadow ? owSunShadow( … ) * owContactShadow( … ) : 1.0;
/// directLight.color *= mix( 1.0, owSampleAO(), owAoStrength.x * 0.35 );
/// ```
///
/// **Two multiplies, not one gain.** `(c * s) * m` and `c * (s * m)` are
/// different f32 numbers, and the source writes the first.
///
/// `sun_shadow` is `crate::cascade::shading::sun_shadow`'s result and
/// `contact` is [`contact_shadow`]'s; both are passed in so this composition
/// has no cascade atlas and no texture in it.
pub(crate) fn direct_light(
    color: [f32; 3],
    receive_shadow: bool,
    sun_shadow: f32,
    contact: f32,
    ao: f32,
    ao_strength_x: f32,
) -> [f32; 3] {
    let shadowed = [1.0, sun_shadow * contact][usize::from(receive_shadow)];
    let micro = mix(1.0, ao, ao_strength_x * MICRO_SHADOW_FRACTION);
    scale3(scale3(color, shadowed), micro)
}

/// Everything the fragment stage hands the indirect composition.
///
/// The three light terms are three's names, and two of them are **zero in
/// Axiom today** — see the module header's expiry checks:
///
/// * `irradiance` — the analytic ambient. Axiom's peer exists: the suffix's
///   `hemi * ambient_shade`, which `scene_wgsl.rs` records as being three's
///   `getHemisphereLightIrradiance` expression exactly.
/// * `ibl_irradiance` — the PMREM diffuse. Axiom has no probe; zero.
/// * `radiance` — the PMREM specular. Axiom has no probe; zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct IndirectIn {
    /// three's `irradiance`.
    pub(crate) irradiance: [f32; 3],
    /// three's `iblIrradiance`.
    pub(crate) ibl_irradiance: [f32; 3],
    /// three's `radiance`.
    pub(crate) radiance: [f32; 3],
    /// three's `diffuseColor.rgb` — the pre-metalness base colour, which is
    /// what the multi-bounce fit is a function of.
    pub(crate) diffuse_color: [f32; 3],
    /// three's `material.roughness`.
    pub(crate) roughness: f32,
    /// The fragment's world position. three reconstructs this as
    /// `cameraPosition + geometryPosition * mat3( viewMatrix )`; Axiom's
    /// fragment stage interpolates it directly.
    pub(crate) world_pos: [f32; 3],
    /// The fragment's world-space shading normal. three reconstructs this as
    /// `inverseTransformDirection( normal, viewMatrix )`; Axiom's `N` is
    /// already world-space.
    pub(crate) world_normal: [f32; 3],
    /// [`sample_ao`]'s result, computed once by the caller — the source calls
    /// `owSampleAO()` twice in this block and once more in the light loop, and
    /// it is the same value each time.
    pub(crate) ao: f32,
}

/// The three light terms after the composition, plus the interior gate the
/// caller may want for its own terms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct IndirectOut {
    /// three's `irradiance`, occluded and with both fill bands added.
    pub(crate) irradiance: [f32; 3],
    /// three's `iblIrradiance`, occluded and budgeted.
    pub(crate) ibl_irradiance: [f32; 3],
    /// three's `radiance`, specular-occluded. [`ssr_blend`] runs *after* this.
    pub(crate) radiance: [f32; 3],
    /// [`interior_gate`]'s result for this fragment.
    pub(crate) indoor: f32,
}

/// The whole `#include <lights_fragment_maps>` injection — AO onto the indirect
/// terms, the interior gate, and the two-band hemispheric fill.
///
/// The source's `if ( owAo < 1.0 ) { … }` guard is reproduced as a **value
/// select over the whole block**, not argued away. It happens to be an exact
/// identity at `ao == 1` (the multi-bounce clamps to `[ao, 1] = [1, 1]` and the
/// specular occlusion is `pow(1, k) = 1`), but "happens to be" is not a
/// transcription, and `mix(1, 1, s)` reducing to exactly `1` is a property of
/// the rounding rather than of the algebra. Reproducing the guard costs one
/// index and removes the argument.
pub(crate) fn indirect(input: IndirectIn, u: &IndirectUniforms) -> IndirectOut {
    let ao = input.ao;
    // ---- AO on indirect only ------------------------------------------------
    let bounce = multi_bounce(ao, input.diffuse_color);
    let occluded_irradiance = mul3(input.irradiance, bounce);
    let occluded_ibl = mul3(input.ibl_irradiance, bounce);
    // `radiance *= mix( 1.0, owSpecularOcclusion( owAo, material.roughness ), owAoStrength.y );`
    let spec_occ = mix(
        1.0,
        specular_occlusion(ao, input.roughness),
        u.ao_strength[1],
    );
    let occluded_radiance = scale3(input.radiance, spec_occ);
    let occluding = usize::from(ao < 1.0);
    let irradiance = [input.irradiance, occluded_irradiance][occluding];
    let ibl_irradiance = [input.ibl_irradiance, occluded_ibl][occluding];
    let radiance = [input.radiance, occluded_radiance][occluding];

    // ---- interior/exterior indirect budget ----------------------------------
    let indoor = interior_gate(input.world_pos, ao, u);

    // ---- two-band hemispheric bounce fill -----------------------------------
    // Occluded with sqrt(AO), never AO: a fill term that AO can drive to zero
    // is not a fill, it is another way to make a black hole.
    let fill_ao = ao.max(0.0).sqrt();
    let up = clamp(input.world_normal[1], -1.0, 1.0);
    let sky_gate = smoothstep(u.fill_dir[0], u.fill_dir[1], up) * indoor;
    let ground_gate = smoothstep(u.fill_dir[2], u.fill_dir[3], -up);
    // `irradiance += ( owSkyFill * owSkyG + owGroundFill * owGndG * owIndoor )
    //   * ( owFillAo * owFillGain.x );`
    let bands = add3(
        scale3(u.sky_fill, sky_gate),
        scale3(scale3(u.ground_fill, ground_gate), indoor),
    );
    let irradiance = add3(irradiance, scale3(bands, fill_ao * u.fill_gain[0]));

    // `iblIrradiance *= owIndirect.x * owIndoor;`
    let ibl_irradiance = scale3(ibl_irradiance, u.indirect[0] * indoor);

    // ---- warm sun bounce off whatever the sun is actually hitting ------------
    // `irradiance += owGroundFill * ( owSunBounce( owWN ) * owFillGain.y * owFillAo * owIndoor );`
    let wrap = sun_bounce(input.world_normal, u.sun_dir_world) * u.fill_gain[1] * fill_ao * indoor;
    let irradiance = add3(irradiance, scale3(u.ground_fill, wrap));

    IndirectOut {
        irradiance,
        ibl_irradiance,
        radiance,
        indoor,
    }
}

/// Screen-space reflection blended into the image-based specular **by
/// confidence**, so energy is replaced rather than added on top.
///
/// ```glsl
/// if ( owFeat.z > 0.5 && material.roughness < 0.62 ) {
///   vec4 owSsr = texture2D( owSsrTex, gl_FragCoord.xy * owScreenTexel );
///   float owW = owSsr.a * smoothstep( 0.62, 0.14, material.roughness );
///   radiance = mix( radiance, owSsr.rgb, clamp( owW, 0.0, 1.0 ) );
/// }
/// ```
///
/// `ssr` is the already-sampled RGBA texel: `rgb` the reflected colour, `a` the
/// tracer's confidence. Note the descending `smoothstep( 0.62, 0.14, … )` —
/// `e0 > e1` deliberately, so a mirror gets the full weight and the ramp is
/// gone by the same `0.62` the outer test uses.
///
/// **This currently multiplies nothing.** Axiom has no environment probe, so
/// `radiance` is an exact zero everywhere and `mix(0, ssr, w)` is the SSR
/// colour arriving as pure addition. The maths is right; the input is missing.
/// See the module header's expiry check 2.
pub(crate) fn ssr_blend(radiance: [f32; 3], feat_z: f32, roughness: f32, ssr: [f32; 4]) -> [f32; 3] {
    let enabled = (feat_z > 0.5) & (roughness < 0.62);
    let weight = ssr[3] * smoothstep(0.62, 0.14, roughness);
    let blended = mix3(
        radiance,
        [ssr[0], ssr[1], ssr[2]],
        clamp(weight, 0.0, 1.0),
    );
    [radiance, blended][usize::from(enabled)]
}

/// The WGSL this slice asks to be spliced into `crate::scene_wgsl`'s **suffix**,
/// verbatim apart from the uniform's group/binding number, which the main pass
/// owns.
///
/// Every function is the GLSL text written out in the same factoring the CPU
/// reference above uses, so `indirect_lighting::adapter_proof` compares two
/// transcriptions of one source rather than a shader against itself. The
/// source's control flow is kept where it is — shader text is data, and the
/// `engine_no_branching` dylint reads Rust HIR — with two stated changes:
///
/// - `texture2D( tex, gl_FragCoord.xy * owScreenTexel )` becomes a caller-side
///   sample. The screen texel is not carried: a consumer reads
///   `textureDimensions` of the buffer it is sampling, which cannot go stale
///   against a resize the way `setScreenSize` can. Every function here takes
///   the sampled value, which is also what makes them testable without a
///   texture.
/// - the `#if defined( STANDARD ) && defined( RE_IndirectSpecular )` and
///   `USE_ENVMAP` preprocessor gates are gone: WGSL has no preprocessor, and
///   this engine compiles one lighting model per program. The terms they
///   guarded are zero-valued rather than absent, which is the same result and
///   the shape `scene_wgsl.rs` already uses for its capability gates.
pub(crate) const INDIRECT_LIGHTING_WGSL: &str = r#"
// `materialpatch.js`'s shared uniform block, plus `owSunDirWorld` from the CSM
// chunk (the patcher spreads `csmUniforms` into its own object, so in the
// source the two arrive as one). `feat.w` is `owFeat`'s declared AO power,
// which nothing in the source reads; it is carried because the source carries
// it.
struct AxiomIndirectU {
    feat: vec4<f32>,
    ao_strength: vec4<f32>,
    sky_fill: vec4<f32>,
    ground_fill: vec4<f32>,
    fill_gain: vec4<f32>,
    fill_dir: vec4<f32>,
    indirect: vec4<f32>,
    room_xf: vec4<f32>,
    sun_dir_world: vec4<f32>,
    rooms: array<vec4<f32>, 10>,
    rooms_y: array<vec4<f32>, 10>,
};

// GLSL `mix(x, y, a)` in the factoring the spec gives it: x*(1-a) + y*a. NOT
// x + (y-x)*a, and not the builtin, whose factoring WGSL leaves open.
fn axiom_indirect_mix(x: f32, y: f32, a: f32) -> f32 {
    return x * (1.0 - a) + y * a;
}

fn axiom_indirect_mix3(x: vec3<f32>, y: vec3<f32>, a: f32) -> vec3<f32> {
    return vec3<f32>(
        axiom_indirect_mix(x.x, y.x, a),
        axiom_indirect_mix(x.y, y.y, a),
        axiom_indirect_mix(x.z, y.z, a),
    );
}

// GLSL `clamp(x, lo, hi)` = min(max(x, lo), hi), written out.
fn axiom_indirect_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    return min(max(x, lo), hi);
}

// GLSL `smoothstep`, written out. Called with e0 > e1 by the SSR roughness
// ramp, which WGSL's builtin leaves indeterminate and this formula does not.
fn axiom_indirect_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = axiom_indirect_clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// `owSampleAO()`, with the texture fetch hoisted to the caller.
fn axiom_indirect_sample_ao(u: AxiomIndirectU, ao_texel_r: f32) -> f32 {
    if ( u.feat.x < 0.5 ) { return 1.0; }
    return axiom_indirect_mix(1.0, max(ao_texel_r, 0.25), u.ao_strength.x);
}

// `owContactShadow( vec3 lightDirView )`, with the texture fetch hoisted and
// the sun test taking the caller's dot product.
fn axiom_indirect_contact_shadow(u: AxiomIndirectU, light_dot_sun_view: f32, contact_texel_r: f32) -> f32 {
    if ( u.feat.y < 0.5 ) { return 1.0; }
    if ( light_dot_sun_view < 0.999 ) { return 1.0; }
    return contact_texel_r;
}

// `owMultiBounce( float ao, vec3 albedo )` — Jimenez GTAO multi-bounce.
fn axiom_indirect_multi_bounce(ao: f32, albedo: vec3<f32>) -> vec3<f32> {
    let a = 2.0404 * albedo - vec3<f32>(0.3324);
    let b = -4.7951 * albedo + vec3<f32>(0.6417);
    let c = 2.7552 * albedo + vec3<f32>(0.6903);
    let v = ao * (ao * (ao * a + b) + c);
    return vec3<f32>(
        axiom_indirect_clamp(v.x, ao, 1.0),
        axiom_indirect_clamp(v.y, ao, 1.0),
        axiom_indirect_clamp(v.z, ao, 1.0),
    );
}

// `owSpecularOcclusion( float ao, float rough )`.
fn axiom_indirect_specular_occlusion(ao: f32, rough: f32) -> f32 {
    let r2 = rough * rough;
    return axiom_indirect_clamp(pow(max(ao, 0.0), 1.0 + r2 * 2.0), 0.0, 1.0);
}

// `owSunBounce( vec3 worldNormal )`. `normalize` and `dot` are written out so
// the CPU reference and this text are pinned to one factoring; neither GLSL nor
// WGSL pins the builtins'. The trailing `/ 1.12` is the source's division.
fn axiom_indirect_sun_bounce(u: AxiomIndirectU, world_normal: vec3<f32>) -> f32 {
    let raw = vec3<f32>(-u.sun_dir_world.x, 0.28, -u.sun_dir_world.z) + vec3<f32>(1e-4);
    let length_raw = sqrt(raw.x * raw.x + raw.y * raw.y + raw.z * raw.z);
    let anti = vec3<f32>(raw.x / length_raw, raw.y / length_raw, raw.z / length_raw);
    let facing = world_normal.x * anti.x + world_normal.y * anti.y + world_normal.z * anti.z;
    return axiom_indirect_clamp((facing + 0.12) / 1.12, 0.0, 1.0);
}

// `owInteriorGate( vec3 worldPos, float ao )`. The source's loop and break are
// kept: this is shader text, and a bounded loop is a bounded loop.
fn axiom_indirect_interior_gate(u: AxiomIndirectU, world_pos: vec3<f32>, ao: f32) -> f32 {
    var indoor = 0.0;
    if ( u.indirect.z > 0.5 ) {
        let lx = world_pos.x * u.room_xf.x + world_pos.z * u.room_xf.y + u.room_xf.z;
        let lz = -world_pos.x * u.room_xf.y + world_pos.z * u.room_xf.x + u.room_xf.w;
        let n = i32(u.indirect.z);
        for ( var i: i32 = 0; i < 10; i = i + 1 ) {
            if ( i >= n ) { break; }
            let r = u.rooms[i];
            let ry = u.rooms_y[i];
            let d = min(
                min( r.z - abs( lx - r.x ), r.w - abs( lz - r.y ) ),
                min( world_pos.y - ry.x, ry.y - world_pos.y ) );
            indoor = max( indoor, axiom_indirect_smoothstep( 0.06, 0.30, d ) );
        }
    }
    let ao_gate = axiom_indirect_mix( 1.0, axiom_indirect_smoothstep( 0.45, 0.98, ao ), 0.6 );
    let g = min( 1.0 - indoor, ao_gate );
    return axiom_indirect_mix( u.indirect.y, 1.0, axiom_indirect_clamp( g, 0.0, 1.0 ) );
}

// The directional-light loop's two injected multiplies, in the source's order.
// `(c * s) * m`, never `c * (s * m)`.
fn axiom_indirect_direct_light(
    u: AxiomIndirectU,
    color: vec3<f32>,
    receive_shadow: bool,
    sun_shadow: f32,
    contact: f32,
    ao: f32,
) -> vec3<f32> {
    let shadowed = select( 1.0, sun_shadow * contact, receive_shadow );
    let micro = axiom_indirect_mix( 1.0, ao, u.ao_strength.x * 0.35 );
    return (color * shadowed) * micro;
}

struct AxiomIndirectOut {
    irradiance: vec3<f32>,
    ibl_irradiance: vec3<f32>,
    radiance: vec3<f32>,
    indoor: f32,
};

// The `#include <lights_fragment_maps>` injection, whole.
fn axiom_indirect_apply(
    u: AxiomIndirectU,
    irradiance_in: vec3<f32>,
    ibl_in: vec3<f32>,
    radiance_in: vec3<f32>,
    diffuse_color: vec3<f32>,
    roughness: f32,
    world_pos: vec3<f32>,
    world_normal: vec3<f32>,
    ao: f32,
) -> AxiomIndirectOut {
    var irradiance = irradiance_in;
    var ibl_irradiance = ibl_in;
    var radiance = radiance_in;
    if ( ao < 1.0 ) {
        let bounce = axiom_indirect_multi_bounce( ao, diffuse_color );
        irradiance = irradiance * bounce;
        ibl_irradiance = ibl_irradiance * bounce;
        radiance = radiance * axiom_indirect_mix( 1.0, axiom_indirect_specular_occlusion( ao, roughness ), u.ao_strength.y );
    }

    let indoor = axiom_indirect_interior_gate( u, world_pos, ao );

    let fill_ao = sqrt( max( ao, 0.0 ) );
    let up = axiom_indirect_clamp( world_normal.y, -1.0, 1.0 );
    let sky_g = axiom_indirect_smoothstep( u.fill_dir.x, u.fill_dir.y, up ) * indoor;
    let gnd_g = axiom_indirect_smoothstep( u.fill_dir.z, u.fill_dir.w, -up );
    irradiance = irradiance + ( u.sky_fill.xyz * sky_g + u.ground_fill.xyz * gnd_g * indoor )
        * ( fill_ao * u.fill_gain.x );

    ibl_irradiance = ibl_irradiance * ( u.indirect.x * indoor );

    irradiance = irradiance + u.ground_fill.xyz *
        ( axiom_indirect_sun_bounce( u, world_normal ) * u.fill_gain.y * fill_ao * indoor );

    var out: AxiomIndirectOut;
    out.irradiance = irradiance;
    out.ibl_irradiance = ibl_irradiance;
    out.radiance = radiance;
    out.indoor = indoor;
    return out;
}

// Screen-space reflection blended into the image-based specular by confidence.
fn axiom_indirect_ssr_blend(u: AxiomIndirectU, radiance: vec3<f32>, roughness: f32, ssr: vec4<f32>) -> vec3<f32> {
    if ( u.feat.z > 0.5 && roughness < 0.62 ) {
        let w = ssr.a * axiom_indirect_smoothstep( 0.62, 0.14, roughness );
        return axiom_indirect_mix3( radiance, ssr.rgb, axiom_indirect_clamp( w, 0.0, 1.0 ) );
    }
    return radiance;
}
"#;

#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "offscreen"))]
mod adapter_proof;
