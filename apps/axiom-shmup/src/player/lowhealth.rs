//! Low-health screen treatment, registered with `render` as a post pass.
//!
//! Ported from Claude-of-Duty `src/player/lowhealth.js:1-172` — the whole
//! file.
//!
//! Runs in the HDR/linear domain *before* tonemapping, which is the only place
//! this can go without fighting the film curve: desaturating after AgX crushes
//! the highlights instead of draining the colour.
//!
//! Three stacked cues, all driven from [`crate::player::health::Health`]:
//!
//! | cue | what it does |
//! |---|---|
//! | desaturation      | blood loss reads as the world losing colour |
//! | arterial vignette | a soft, warm-red edge that pulses with the heartbeat |
//! | hit flash         | a short, sharp radial wash on the frame you are hit |
//!
//! The pass sets `enabled = false` whenever it would be a no-op so a healthy
//! player pays nothing — not even the ping-pong blit.
//!
//! ## What is ported, and what a GPU binding still has to do
//!
//! The source is a `THREE.RawShaderMaterial` plus a fullscreen triangle and a
//! `render(renderer, inputTexture, target, r)` that blits it. This crate has
//! no post-pass pipeline yet, so what lands here is everything that is *not* a
//! GPU call:
//!
//! * [`VERT`] and [`FRAG`], the GLSL, verbatim — the eventual WGSL/GLSL
//!   binding transcribes from these strings, not from prose.
//! * The pass identity ([`LowHealthPass::NAME`], [`LowHealthPass::ORDER`]),
//!   the uniform state, [`LowHealthPass::sync`] and
//!   [`LowHealthPass::resize`] — all pure, all pinned.
//! * The fullscreen triangle's vertex data ([`TRIANGLE_POSITIONS`],
//!   [`TRIANGLE_UVS`]) and the 1×1 unit-exposure fallback
//!   ([`LowHealthPass::unit_exposure`]). All three are `Float32Array` in the
//!   source, so all three are `f32` here — storage width is part of the
//!   algorithm.
//! * [`LowHealthPass::shade`], a CPU `f64` transcription of [`FRAG`]'s body,
//!   which is what makes the shader checkable at all. There is no oracle to
//!   call for GLSL held in a JavaScript string, so `tests/player_system/
//!   capture.mjs` re-implements the same body independently and the two
//!   transcriptions are compared against each other. Precedent:
//!   `crate::sky::atmosphere` and `tests/sky/capture.mjs`.
//!
//! **Not ported:** `render(renderer, inputTexture, target, r)`
//! (`lowhealth.js:160-165`) — three lines of `THREE.WebGLRenderer` calls — and
//! the `Material`/`BufferGeometry`/`Mesh`/`Scene`/`Camera` objects it drives.
//! [`LowHealthPass::dispose`] therefore has nothing to free and is a no-op
//! marker for the same reason.

/// `lowhealth.js:21-30`, verbatim. `RawShaderMaterial`: three prepends
/// nothing, so every attribute and the precision qualifier are declared by
/// hand.
pub const VERT: &str = r"
precision highp float;
in vec3 position;
in vec2 uv;
out vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = vec4(position.xy, 0.0, 1.0);
}
";

/// `lowhealth.js:32-93`, verbatim. [`LowHealthPass::shade`] is the CPU
/// transcription of this body.
pub const FRAG: &str = r"
precision highp float;
in vec2 vUv;
uniform sampler2D uTex;
/** x amount, y pulse, z hitFlash, w critical */
uniform vec4 uState;
uniform vec2 uAspect;
/** 1x1, .r = the exposure scalar the composite will apply after us. */
uniform sampler2D uExposure;
out vec4 fragColor;

void main() {
  vec3 c = texture(uTex, vUv).rgb;
  float amount = uState.x;
  float pulse = uState.y;
  float flash = uState.z;

  // Radial distance in a square-corrected space so the vignette is round.
  vec2 d = (vUv - 0.5) * uAspect;
  float r = length(d) * 1.414;

  // Two lobes: a wide darkening and a narrower blood rim that breathes with the
  // heartbeat.
  float wide = smoothstep(0.18, 1.0, r);
  float rim = smoothstep(0.34, 1.1, r);
  float beat = amount * (0.32 + 0.68 * pulse);

  // ---- desaturation: Rec.709 luma, pulled toward a cold grey ------------
  // Note everything below is deliberately *relative* — multiplicative and
  // chromatic — because auto-exposure meters this pass's output and would
  // simply gain back any absolute brightness we removed.
  float luma = dot(c, vec3(0.2126, 0.7152, 0.0722));
  float sat = amount * (0.74 + 0.16 * pulse);
  c = mix(c, vec3(luma) * vec3(0.93, 0.97, 1.06), clamp(sat, 0.0, 0.94));

  // ---- edge darkening ----------------------------------------------------
  c *= 1.0 - wide * (0.40 + 0.28 * beat) * amount;

  // ---- arterial rim ------------------------------------------------------
  // Subtractive first: the rim loses green and blue rather than gaining red, so
  // it survives the film curve instead of clipping into a magenta halo.
  float k = rim * beat;
  c *= mix(vec3(1.0), vec3(1.16, 0.26, 0.22), clamp(k * 0.98, 0.0, 1.0));

  // Then a small additive glow so the rim still reads where the corners are
  // already black. This is a viewer-side overlay, not light in the scene, so it
  // is authored display-referred and divided by the exposure the composite is
  // about to apply — otherwise it vanishes at noon and blinds you at night.
  float invExp = 1.0 / max(1e-3, texture(uExposure, vec2(0.5)).r);
  c += vec3(0.115, 0.008, 0.005) * k * invExp;

  // ---- hit flash ---------------------------------------------------------
  if (flash > 0.001) {
    float ring = 0.3 + 0.7 * smoothstep(0.05, 0.95, r);
    float f = clamp(flash * ring, 0.0, 1.0);
    c *= mix(vec3(1.0), vec3(1.3, 0.4, 0.34), f);
    c += vec3(0.16, 0.012, 0.008) * f * invExp;
  }

  fragColor = vec4(c, 1.0);
}
";

/// `new Float32Array([-1, -1, 0, 3, -1, 0, -1, 3, 0])`. `lowhealth.js:130`.
/// `f32` because the source's storage is `f32`.
pub const TRIANGLE_POSITIONS: [f32; 9] = [-1.0, -1.0, 0.0, 3.0, -1.0, 0.0, -1.0, 3.0, 0.0];

/// `new Float32Array([0, 0, 2, 0, 0, 2])`. `lowhealth.js:132`.
pub const TRIANGLE_UVS: [f32; 6] = [0.0, 0.0, 2.0, 0.0, 0.0, 2.0];

/// `new THREE.Sphere(new THREE.Vector3(), 1e8)`. `lowhealth.js:133` — a
/// bounding sphere big enough that nothing ever culls the triangle.
pub const BOUNDING_SPHERE_RADIUS: f64 = 1e8;

/// GLSL `smoothstep(edge0, edge1, x)`. Not
/// [`crate::player::springs::smoothstep`], which takes only the interpolant.
fn glsl_smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// GLSL `mix(vec3, vec3, float)`.
fn mix3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// `class LowHealthPass`. `lowhealth.js:95-172`.
#[derive(Debug, Clone, PartialEq)]
pub struct LowHealthPass {
    pub enabled: bool,
    /// `uniforms.uState.value` — `x` amount, `y` pulse, `z` hitFlash,
    /// `w` critical.
    pub state: [f64; 4],
    /// `uniforms.uAspect.value`.
    pub aspect: [f64; 2],
    /// The 1×1 RGBA-float fallback exposure texture
    /// (`new Float32Array([1, 1, 1, 1])`, `lowhealth.js:104-106`). `f32`
    /// storage, matching the source.
    pub unit_exposure: [f32; 4],
}

impl Default for LowHealthPass {
    fn default() -> Self {
        LowHealthPass::new()
    }
}

impl LowHealthPass {
    /// `this.name`. `lowhealth.js:97`.
    pub const NAME: &'static str = "player:lowhealth";
    /// `this.order`. `lowhealth.js:99` — after fx/volumetrics, before
    /// metering, so the grade meters darker.
    pub const ORDER: i32 = 40;

    /// `constructor()`. `lowhealth.js:96-141`.
    pub fn new() -> Self {
        LowHealthPass {
            enabled: false,
            state: [0.0, 0.0, 0.0, 0.0],
            aspect: [1.0, 1.0],
            unit_exposure: [1.0, 1.0, 1.0, 1.0],
        }
    }

    /// `sync(health)`. `lowhealth.js:144-151`.
    pub fn sync(&mut self, health: &crate::player::health::Health) {
        self.sync_values(
            health.effect,
            health.hit_flash,
            health.pulse,
            health.critical(),
        );
    }

    /// [`LowHealthPass::sync`]'s body over the four scalars it actually reads,
    /// so a caller that has the numbers but not a whole [`Health`] (a test, a
    /// scripted debug timeline) can drive the pass the same way.
    ///
    /// [`Health`]: crate::player::health::Health
    pub fn sync_values(&mut self, amount: f64, flash: f64, pulse: f64, critical: bool) {
        self.enabled = amount > 0.004 || flash > 0.004;
        if !self.enabled {
            return;
        }
        self.state = [amount, pulse, flash, if critical { 1.0 } else { 0.0 }];
    }

    /// `resize(w, h)`. `lowhealth.js:153-158`. Keeps the vignette circular
    /// regardless of aspect.
    pub fn resize(&mut self, w: f64, h: f64) {
        if w >= h {
            self.aspect = [1.0, h / 1.0f64.max(w)];
        } else {
            self.aspect = [w / 1.0f64.max(h), 1.0];
        }
    }

    /// The body of [`FRAG`], evaluated on the CPU in `f64`.
    ///
    /// `uv` is `vUv`, `color` is `texture(uTex, vUv).rgb`, and `exposure` is
    /// `texture(uExposure, vec2(0.5)).r` — the caller supplies the two texture
    /// reads the shader does, and this reproduces everything between them.
    /// A hand transcription with no oracle: see the module doc comment.
    pub fn shade(&self, uv: [f64; 2], color: [f64; 3], exposure: f64) -> [f64; 3] {
        let mut c = color;
        let amount = self.state[0];
        let pulse = self.state[1];
        let flash = self.state[2];

        // Radial distance in a square-corrected space so the vignette is round.
        let d = [
            (uv[0] - 0.5) * self.aspect[0],
            (uv[1] - 0.5) * self.aspect[1],
        ];
        // GLSL `length(vec2)`, transcribed as the sqrt of the dot product —
        // NOT `f64::hypot`, which normalises by the larger magnitude first and
        // rounds differently.
        let r = (d[0] * d[0] + d[1] * d[1]).sqrt() * 1.414;

        let wide = glsl_smoothstep(0.18, 1.0, r);
        let rim = glsl_smoothstep(0.34, 1.1, r);
        let beat = amount * (0.32 + 0.68 * pulse);

        // ---- desaturation: Rec.709 luma, pulled toward a cold grey --------
        let luma = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
        let sat = amount * (0.74 + 0.16 * pulse);
        c = mix3(
            c,
            [luma * 0.93, luma * 0.97, luma * 1.06],
            sat.clamp(0.0, 0.94),
        );

        // ---- edge darkening ------------------------------------------------
        let edge = 1.0 - wide * (0.40 + 0.28 * beat) * amount;
        c = [c[0] * edge, c[1] * edge, c[2] * edge];

        // ---- arterial rim --------------------------------------------------
        let k = rim * beat;
        let rim_mix = mix3([1.0, 1.0, 1.0], [1.16, 0.26, 0.22], (k * 0.98).clamp(0.0, 1.0));
        c = [c[0] * rim_mix[0], c[1] * rim_mix[1], c[2] * rim_mix[2]];

        let inv_exp = 1.0 / 1e-3f64.max(exposure);
        c = [
            c[0] + 0.115 * k * inv_exp,
            c[1] + 0.008 * k * inv_exp,
            c[2] + 0.005 * k * inv_exp,
        ];

        // ---- hit flash -------------------------------------------------------
        if flash > 0.001 {
            let ring = 0.3 + 0.7 * glsl_smoothstep(0.05, 0.95, r);
            let f = (flash * ring).clamp(0.0, 1.0);
            let flash_mix = mix3([1.0, 1.0, 1.0], [1.3, 0.4, 0.34], f);
            c = [c[0] * flash_mix[0], c[1] * flash_mix[1], c[2] * flash_mix[2]];
            c = [
                c[0] + 0.16 * f * inv_exp,
                c[1] + 0.012 * f * inv_exp,
                c[2] + 0.008 * f * inv_exp,
            ];
        }
        c
    }

    /// `dispose()`. `lowhealth.js:167-171`. The source frees the material, the
    /// geometry and the fallback texture; none of those GPU objects exist here
    /// (see the module doc comment), so this only marks the pass dead so a
    /// disposed pass cannot keep drawing.
    pub fn dispose(&mut self) {
        self.enabled = false;
    }
}
