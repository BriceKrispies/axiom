//! Ported from Claude-of-Duty `src/ui/minimap.js:1-603`.
//!
//! **Tactical minimap, top left.** The widget that draws the dark inset in
//! `docs/work-manifests/shmup-port/reference/original-street.png`: roof
//! footprints, a 10 m grid, a view cone, enemy blips and the player arrow.
//!
//! # Why this was deferred, and why the deferral had expired
//!
//! The port status recorded this file as *"blocked on the render work — needs
//! an orthographic depth bake read back once, then a Sobel pass for roof
//! outlines"*. Reading the source, both halves of that are wrong:
//!
//! * **There is no Sobel pass.** The module's own doc comment
//!   (`minimap.js:10-23`) still describes one, but the code that would have
//!   done it was replaced: [`Minimap::build_bitmap`] derives its rim from a
//!   blurred *coverage* field (`rim = 4w(1-w)`, `minimap.js:415`), not an edge
//!   filter. The stale comment is what the deferral note was written from.
//! * **The depth bake is the *fallback*, not the map.** `tryBake`
//!   (`minimap.js:71-164`) tries `_buildVectorMap` **first** and only falls
//!   through to the GPU path for *"a scene that has no world subsystem in
//!   it"* (`minimap.js:74-76`). The map you can see in the reference
//!   screenshot is the vector map — the rectangles are visibly rotated by the
//!   level yaw, which only the `levelToWorld` affine produces; a top-down
//!   depth bake would be axis-aligned to the camera.
//!
//! The vector path needs `world.buildings`, `world.levelToWorld` and
//! `world.isOpen`, all three of which [`crate::world::system::WorldSystem`]
//! already exposes. So the primary map is **fully ported here**, CPU-side,
//! with no engine capability added.
//!
//! # The two seams
//!
//! [`LayoutSource`] is the three duck-typed calls `_buildVectorMap` makes on
//! `ctx.peek('world')`. It is satisfied today by `WorldSystem`
//! (`level_to_world`, `is_open`, `buildings`) — an adapter, not a gap.
//!
//! [`DepthBakeSource`] is the one call the fallback needs and this port cannot
//! make: `renderer.readRenderTargetPixels` of a 512² orthographic
//! `MeshDepthMaterial` render. Everything the source does *with* those bytes —
//! the height field, the occupancy mask, the fake NW key light, the separable
//! tent blur, the coverage rim, the grain — is ported in full in
//! [`Minimap::build_bitmap`] and pinned by the golden against a synthetic
//! buffer. Only the byte fetch is behind the seam. See the notes file for
//! exactly what engine capability would satisfy it.
//!
//! # Output shape: a display list
//!
//! Every other `ui/` widget computes a numeric frame that a `wasm32` view
//! writes onto DOM nodes. This widget is not a DOM widget — it is a canvas
//! painter, and its output *is* a sequence of canvas2d calls. So
//! [`Minimap::draw`] returns [`Vec<DrawOp>`], one entry per call the source
//! makes, in source order. That keeps the port 1:1 with the JavaScript (the
//! golden compares op-for-op) and leaves rasterisation — which is not
//! behaviour, it is painting — to the view.
//!
//! # Source defects ported faithfully
//!
//! * **The street network west of level x = 0 never draws.**
//!   `minimap.js:229-241` uses `run = -1` as the "no run open" sentinel, but
//!   `run` holds an `lx` that ranges `[-44, 44]`, so every negative `lx`
//!   *is* the sentinel. While a run's start is negative, `open && run < 0`
//!   keeps re-firing and walks the start forward one cell per iteration, and
//!   the close arm `!open && run >= 0` can never fire. The result: a run is
//!   only ever emitted from the first open cell at `lx >= 0`, and a run that
//!   ends before `lx = 0` is dropped entirely. Pinned by
//!   `negative_lx_street_runs_are_never_emitted`. It is visible in the
//!   reference screenshot: the minimap has no lighter street network in it at
//!   all, only footprints on flat ground, which is exactly what this defect
//!   produces for a level whose streets straddle `lx = 0`.
//! * **`bakeTries` is incremented but never re-read once `> 6`** in a way the
//!   caller can reach: `index.js:524`'s gate only calls `tryBake` every 20th
//!   frame, so the seven attempts are spread over 140 frames. Ported as-is.
//!
//! # JS-semantics traps handled here
//!
//! * `Math.round` ties toward `+Infinity` — [`crate::jsmath::round`], used for
//!   the grid lines (`minimap.js:486,493`), the canvas pixel size
//!   (`minimap.js:61`) and the footprint colour channels (`minimap.js:251`).
//! * `(x * Math.PI) / 180` is **not** `to_radians()` (`self * (PI / 180)`) —
//!   a different grouping, and float multiplication is not associative. The
//!   source writes the former at all three of its angle sites
//!   (`minimap.js:500`, `501`, `552`), so this port does too.
//! * `Number.prototype.toFixed(1)` breaks ties toward the **larger** integer;
//!   Rust's `{:.1}` breaks them to even. See [`to_fixed_1`].
//! * `Float32Array` storage in [`Minimap::build_bitmap`]: `hgt`, `cov`, `cr`,
//!   `cg`, `cb` and the blur scratch are all f32 (`minimap.js:310,328-331,350`)
//!   while every intermediate is f64. Narrowing on store is part of the
//!   algorithm.
//! * `Uint8ClampedArray` assignment clamps to `0..=255` and rounds **half to
//!   even** — see [`u8_clamped`]. Both grain loops and the bitmap writer go
//!   through it.
//! * `Math.min(2, window.devicePixelRatio || 1)`: JS `||` falls through on `0`
//!   *and* `NaN` — [`crate::jsmath::or_one`].
//!
//! # The rng
//!
//! The minimap owns `ctx.rng.fork()`'s second fork (`index.js:86`), already
//! spent by [`crate::ui::system::UiCore::new`]. Both bake paths draw
//! `BAKE * BAKE` floats from it for the grain, in raster order. Only one path
//! runs per bake, so the streams do not interleave.

use crate::jsmath;
use crate::rng::Rng;

use super::system::MinimapState;
use super::util::{clamp, clamp01, lerp};

/// One-time top-down render resolution (`minimap.js:4`).
pub const BAKE: usize = 512;
/// Ortho camera height above `y = 0` (`minimap.js:5`).
pub const CAM_Y: f64 = 26.0;
/// (`minimap.js:6`)
pub const NEAR: f64 = 0.1;
/// Reaches 8 m below `y = 0`, so basements/slopes still register
/// (`minimap.js:7`).
pub const FAR: f64 = 34.0;
/// Metres of vertical range mapped into the height ramp (`minimap.js:8`).
pub const HEIGHT_RANGE: f64 = CAM_Y;

/// `STEP` in the street run-length loop (`minimap.js:227`).
const STEP: f64 = 0.5;

/* ================================================================ */
/* JS numeric semantics                                             */
/* ================================================================ */

/// Assignment into a `Uint8ClampedArray`.
///
/// ECMA-262's `ToUint8Clamp`: `NaN` becomes `0`, out-of-range clamps to
/// `0`/`255`, and — unlike every other JS rounding — an exact `.5` rounds
/// **half to even**, so `2.5` stores `2` and `3.5` stores `4`. Rust's
/// `f64 as u8` truncates and would be wrong for every fractional value here;
/// the grain loops (`minimap.js:269-275`, `424-426`) add a signed fraction to
/// every channel, so this runs on all 786 432 of them.
pub fn u8_clamped(v: f64) -> u8 {
    if v.is_nan() {
        return 0;
    }
    if v <= 0.0 {
        return 0;
    }
    if v >= 255.0 {
        return 255;
    }
    let f = v.floor();
    let frac = v - f;
    // Half to even; anything else to nearest.
    let up = if frac > 0.5 {
        true
    } else if frac < 0.5 {
        false
    } else {
        (f as u64) % 2 == 1
    };
    (f as u64 + u64::from(up)) as u8
}

/// `Number.prototype.toFixed(1)` (`minimap.js:520`).
///
/// ECMA-262 picks the integer `n` minimising `|n/10 - x|` and, **"if there are
/// two such n, pick the larger n"** — ties toward `+Infinity`. Rust's
/// `format!("{:.1}", x)` ties to even, so `9.25` prints `"9.3"` here and
/// `"9.2"` there. That difference is reachable: the argument is `9.5 * u` and
/// exact quarters are dyadic, so `u = 19/38`-style scales land on one.
///
/// `x * 10.0` is exact for every value that can *be* a tie (a dyadic rational
/// with two decimal places has at most two significant fractional bits), so
/// rounding the product with [`jsmath::round`] — which itself ties toward
/// `+Infinity` — reproduces the spec's `n` on the whole reachable domain.
pub fn to_fixed_1(x: f64) -> String {
    let n = jsmath::round(x * 10.0);
    let neg = n < 0.0 || (n == 0.0 && x.is_sign_negative());
    let a = n.abs();
    let whole = (a / 10.0).floor();
    let dec = (a - whole * 10.0) as u8;
    format!("{}{}.{}", if neg { "-" } else { "" }, whole, dec)
}

/* ================================================================ */
/* Seams                                                            */
/* ================================================================ */

/// One `world.buildings[i].spec` (`minimap.js:245-250`).
///
/// Every field is optional because the source treats them so: `w`/`d` absent
/// makes the entry `continue`, and `x`/`z`/`floors` fall back through `??` to
/// `0`/`0`/`2`. [`crate::world::layout::Building`] supplies all five, so from
/// the real world subsystem the fallbacks never fire — but `_buildVectorMap`
/// also accepts a bare `infos[i]` with no `.spec`, and this keeps those arms
/// reachable and tested.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FootprintSpec {
    pub x: Option<f64>,
    pub z: Option<f64>,
    pub w: Option<f64>,
    pub d: Option<f64>,
    pub floors: Option<f64>,
}

/// `ctx.peek('world')` as `_buildVectorMap` uses it (`minimap.js:184-201`,
/// `234`).
///
/// Satisfied by [`crate::world::system::WorldSystem`]:
/// `buildings()` ← `self.buildings[i].building`, `level_to_world` and
/// `is_open` are already its public queries with the same signatures.
pub trait LayoutSource {
    /// `world.buildings` — empty means "not built yet", and `tryBake` falls
    /// through to the depth path.
    fn buildings(&self) -> Vec<FootprintSpec>;
    /// `world.levelToWorld(x, y, z, out)` — only `.x` and `.z` are read.
    fn level_to_world(&self, x: f64, y: f64, z: f64) -> (f64, f64);
    /// `world.isOpen(x, z, margin)`, called with `margin = 0` throughout.
    fn is_open(&self, x: f64, z: f64, margin: f64) -> bool;
}

/// The one thing this port cannot do CPU-side: the orthographic depth
/// readback the *fallback* bake needs (`minimap.js:87-144`).
///
/// The implementor owns the render target, the `MeshDepthMaterial`, the
/// oversize-object cull (`minimap.js:110-127`) and the renderer state
/// save/restore (`minimap.js:129-150`). It hands back exactly what
/// `readRenderTargetPixels` produced.
pub trait DepthBakeSource {
    /// `BAKE * BAKE * 4` bytes, **bottom-up** (`minimap.js:399`), from a
    /// `BasicDepthPacking` `MeshDepthMaterial` render of the scene through an
    /// orthographic camera at `(centre_x, CAM_Y, centre_z)` looking down, with
    /// `up = (0, 0, -1)`, half-extent `span * 0.5`, clip `NEAR..FAR`, cleared
    /// to opaque black.
    ///
    /// `Err(())` is the source's `catch` arm (`minimap.js:159-163`): the bake
    /// is abandoned permanently and the procedural fallback stands.
    fn read_ortho_depth(&mut self, req: &DepthBakeRequest) -> Result<Vec<u8>, ()>;

    /// `_releaseGpu()` (`minimap.js:282-288`).
    fn release(&mut self);
}

/// The orthographic camera `tryBake` configures (`minimap.js:98-108`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthBakeRequest {
    pub size: usize,
    pub centre_x: f64,
    pub centre_z: f64,
    /// `this.span` — the full width in metres; the camera half-extent is
    /// `span * 0.5`.
    pub span: f64,
    pub cam_y: f64,
    pub near: f64,
    pub far: f64,
}

/* ================================================================ */
/* Display list                                                     */
/* ================================================================ */

/// One canvas2d call, in the order the source makes it.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawOp {
    /// `g.setTransform(a, b, c, d, e, f)`
    SetTransform([f64; 6]),
    /// `g.clearRect(x, y, w, h)`
    ClearRect([f64; 4]),
    /// `g.fillStyle = <css colour>`
    FillStyle(String),
    /// `g.fillStyle = <the gradient built by the preceding
    /// `CreateRadialGradient` + `AddColorStop`s>`
    FillStyleGradient,
    /// `g.strokeStyle = <css colour>`
    StrokeStyle(&'static str),
    /// `g.fillRect(x, y, w, h)`
    FillRect([f64; 4]),
    Save,
    Restore,
    BeginPath,
    ClosePath,
    Fill,
    Stroke,
    Clip,
    /// `g.rect(x, y, w, h)`
    Rect([f64; 4]),
    /// `g.drawImage(this.baked, sx, sy, sw, sh, dx, dy, dw, dh)`
    DrawBaked {
        sx: f64,
        sy: f64,
        sw: f64,
        sh: f64,
        dx: f64,
        dy: f64,
        dw: f64,
        dh: f64,
    },
    /// `g.imageSmoothingEnabled = <b>`
    ImageSmoothingEnabled(bool),
    /// `g.imageSmoothingQuality = <q>`
    ImageSmoothingQuality(&'static str),
    /// `g.lineWidth = <w>`
    LineWidth(f64),
    /// `g.lineJoin = <j>`
    LineJoin(&'static str),
    /// `g.moveTo(x, y)`
    MoveTo(f64, f64),
    /// `g.lineTo(x, y)`
    LineTo(f64, f64),
    /// `g.arc(x, y, r, start, end)`
    Arc {
        x: f64,
        y: f64,
        r: f64,
        start: f64,
        end: f64,
    },
    /// `g.createRadialGradient(x0, y0, r0, x1, y1, r1)`
    CreateRadialGradient {
        x0: f64,
        y0: f64,
        r0: f64,
        x1: f64,
        y1: f64,
        r1: f64,
    },
    /// `grad.addColorStop(offset, colour)`
    AddColorStop(f64, &'static str),
    /// `g.font = <css font>`
    Font(String),
    /// `g.textAlign = <a>`
    TextAlign(&'static str),
    /// `g.textBaseline = <b>`
    TextBaseline(&'static str),
    /// `g.fillText(text, x, y)`
    FillText(String, f64, f64),
    /// `g.translate(x, y)`
    Translate(f64, f64),
    /// `g.rotate(theta)`
    Rotate(f64),
    /// `g.shadowColor = <c>`
    ShadowColor(&'static str),
    /// `g.shadowBlur = <b>`
    ShadowBlur(f64),
}

/// One [`DrawOp`] argument, for golden comparison.
#[derive(Debug, Clone, PartialEq)]
pub enum OpArg {
    N(f64),
    S(String),
    B(bool),
}

impl DrawOp {
    /// `(tag, args)` in the shape the capture script journals a canvas call.
    pub fn encode(&self) -> (&'static str, Vec<OpArg>) {
        fn n4(v: &[f64; 4]) -> Vec<OpArg> {
            v.iter().map(|&x| OpArg::N(x)).collect()
        }
        match self {
            DrawOp::SetTransform(v) => ("setTransform", v.iter().map(|&x| OpArg::N(x)).collect()),
            DrawOp::ClearRect(v) => ("clearRect", n4(v)),
            DrawOp::FillStyle(s) => ("fillStyle", vec![OpArg::S(s.clone())]),
            DrawOp::FillStyleGradient => ("fillStyleGradient", vec![]),
            DrawOp::StrokeStyle(s) => ("strokeStyle", vec![OpArg::S((*s).to_string())]),
            DrawOp::FillRect(v) => ("fillRect", n4(v)),
            DrawOp::Save => ("save", vec![]),
            DrawOp::Restore => ("restore", vec![]),
            DrawOp::BeginPath => ("beginPath", vec![]),
            DrawOp::ClosePath => ("closePath", vec![]),
            DrawOp::Fill => ("fill", vec![]),
            DrawOp::Stroke => ("stroke", vec![]),
            DrawOp::Clip => ("clip", vec![]),
            DrawOp::Rect(v) => ("rect", n4(v)),
            DrawOp::DrawBaked {
                sx,
                sy,
                sw,
                sh,
                dx,
                dy,
                dw,
                dh,
            } => (
                "drawImage",
                vec![*sx, *sy, *sw, *sh, *dx, *dy, *dw, *dh]
                    .into_iter()
                    .map(OpArg::N)
                    .collect(),
            ),
            DrawOp::ImageSmoothingEnabled(b) => ("imageSmoothingEnabled", vec![OpArg::B(*b)]),
            DrawOp::ImageSmoothingQuality(q) => (
                "imageSmoothingQuality",
                vec![OpArg::S((*q).to_string())],
            ),
            DrawOp::LineWidth(w) => ("lineWidth", vec![OpArg::N(*w)]),
            DrawOp::LineJoin(j) => ("lineJoin", vec![OpArg::S((*j).to_string())]),
            DrawOp::MoveTo(x, y) => ("moveTo", vec![OpArg::N(*x), OpArg::N(*y)]),
            DrawOp::LineTo(x, y) => ("lineTo", vec![OpArg::N(*x), OpArg::N(*y)]),
            DrawOp::Arc { x, y, r, start, end } => (
                "arc",
                vec![*x, *y, *r, *start, *end].into_iter().map(OpArg::N).collect(),
            ),
            DrawOp::CreateRadialGradient {
                x0,
                y0,
                r0,
                x1,
                y1,
                r1,
            } => (
                "createRadialGradient",
                vec![*x0, *y0, *r0, *x1, *y1, *r1]
                    .into_iter()
                    .map(OpArg::N)
                    .collect(),
            ),
            DrawOp::AddColorStop(o, c) => {
                ("addColorStop", vec![OpArg::N(*o), OpArg::S((*c).to_string())])
            }
            DrawOp::Font(s) => ("font", vec![OpArg::S(s.clone())]),
            DrawOp::TextAlign(s) => ("textAlign", vec![OpArg::S((*s).to_string())]),
            DrawOp::TextBaseline(s) => ("textBaseline", vec![OpArg::S((*s).to_string())]),
            DrawOp::FillText(t, x, y) => (
                "fillText",
                vec![OpArg::S(t.clone()), OpArg::N(*x), OpArg::N(*y)],
            ),
            DrawOp::Translate(x, y) => ("translate", vec![OpArg::N(*x), OpArg::N(*y)]),
            DrawOp::Rotate(t) => ("rotate", vec![OpArg::N(*t)]),
            DrawOp::ShadowColor(c) => ("shadowColor", vec![OpArg::S((*c).to_string())]),
            DrawOp::ShadowBlur(b) => ("shadowBlur", vec![OpArg::N(*b)]),
        }
    }
}

/* ================================================================ */
/* Bake products                                                    */
/* ================================================================ */

/// What a successful bake produced, standing in for `this.baked` (a canvas).
#[derive(Debug, Clone, PartialEq)]
pub enum Baked {
    /// `_buildVectorMap` — a `BAKE`² draw list authored in LEVEL metres, with
    /// the level→canvas affine as its `SetTransform`. The view rasterises it
    /// and then applies [`Minimap::apply_grain`].
    Vector(Vec<DrawOp>),
    /// `_buildBitmap` — `BAKE`² RGBA, image space (top-down), fully opaque.
    Bitmap(Vec<u8>),
}

/// What one [`Minimap::try_bake`] did — every arm of `minimap.js:71-164`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BakeAttempt {
    /// `if (this.bakeDone || this.bakeTries > 6) return;`
    Skipped,
    /// `_buildVectorMap` returned true.
    Vector,
    /// `_buildVectorMap` returned false and there is no renderer
    /// (`if (!renderer) return;`) — `bakeTries` was still spent.
    NoRenderer,
    /// The depth path ran but `_buildBitmap` returned false: too little
    /// occupancy for the scene to be considered built. Retries.
    BitmapRejected,
    /// `_buildBitmap` returned true.
    Bitmap,
    /// The `catch` arm: the bake is abandoned and `bakeDone` set anyway.
    Failed,
}

/* ================================================================ */
/* The widget                                                       */
/* ================================================================ */

/// `minimap.js:24-603`'s `Minimap`, minus its DOM construction (the root div,
/// the four corner ticks, the `N` marker and the `ZONE 07 / 60M` tag are
/// static chrome a `wasm32` view builds once, and every `.ow-minimap*` rule is
/// already in `style.css.tpl`).
pub struct Minimap {
    pub rng: Rng,
    /// `this.k` — the HUD scale factor.
    pub k: f64,
    /// `this.cssSize` (`minimap.js:37`).
    pub css_size: f64,
    /// `this.span` — metres covered by the bake (`minimap.js:38`).
    pub span: f64,
    /// `this.viewSpan` — metres visible in the widget (`minimap.js:39`).
    pub view_span: f64,
    /// `this.centre` (`minimap.js:40`). Never written by the source.
    pub centre: (f64, f64),
    /// `this.px` — the backing-store edge in canvas pixels.
    pub px: f64,

    pub baked: Option<Baked>,
    pub bake_tries: i64,
    pub bake_done: bool,
    /// `_releaseGpu()` having run — the source's three `null` assignments
    /// (`_rt`, `_depthMat`, `_pixels`) collapse to one observable here,
    /// because the resources themselves live behind [`DepthBakeSource`].
    pub gpu_released: bool,
}

impl Minimap {
    /// `constructor(parent, rng)` (`minimap.js:25-56`), which ends with
    /// `this.resize(1)`.
    pub fn new(rng: Rng, device_pixel_ratio: f64) -> Self {
        let mut m = Minimap {
            rng,
            k: 1.0,
            css_size: 178.0,
            span: 190.0,
            view_span: 60.0,
            centre: (0.0, 0.0),
            px: 0.0,
            baked: None,
            bake_tries: 0,
            bake_done: false,
            gpu_released: false,
        };
        m.resize(1.0, device_pixel_ratio);
        m
    }

    /// `resize(k)` (`minimap.js:58-67`).
    ///
    /// `Math.min(2, window.devicePixelRatio || 1)`: JS `||` is falsy on `0`,
    /// `-0` **and** `NaN`, so all three collapse to `1`
    /// ([`jsmath::or_one`]) before the `min`.
    pub fn resize(&mut self, k: f64, device_pixel_ratio: f64) {
        self.k = k;
        let dpr = 2.0_f64.min(jsmath::or_one(device_pixel_ratio));
        // The source only writes `canvas.width` when it changed; `this.px` is
        // assigned unconditionally and is the only value `draw` reads.
        self.px = jsmath::round(self.css_size * k * dpr);
    }

    /* --------------------------------------------------------- bake --- */

    /// `tryBake(ctx)` (`minimap.js:71-164`).
    ///
    /// `layout` is `ctx.peek('world')`; `depth` is `ctx.peek('render')
    /// ?.renderer` — `None` for either is the source's missing-subsystem arm.
    pub fn try_bake(
        &mut self,
        layout: Option<&dyn LayoutSource>,
        depth: Option<&mut dyn DepthBakeSource>,
    ) -> BakeAttempt {
        if self.bake_done || self.bake_tries > 6 {
            return BakeAttempt::Skipped;
        }
        self.bake_tries += 1;

        // The honest map first: real layout polygons.
        if let Some(l) = layout {
            if let Some(ops) = self.build_vector_map(l) {
                self.baked = Some(Baked::Vector(ops));
                self.bake_done = true;
                self.release_gpu(depth);
                return BakeAttempt::Vector;
            }
        }

        let Some(depth) = depth else {
            return BakeAttempt::NoRenderer;
        };

        let req = DepthBakeRequest {
            size: BAKE,
            centre_x: self.centre.0,
            centre_z: self.centre.1,
            span: self.span,
            cam_y: CAM_Y,
            near: NEAR,
            far: FAR,
        };
        // Bound before the `let-else` so the mutable borrow of `depth` is
        // unambiguously over before the else arm reaches for it again.
        let read = depth.read_ortho_depth(&req);
        let Ok(pixels) = read else {
            // `catch (err) { console.warn(...); this._releaseGpu(); this.bakeDone = true; }`
            // — fall back to the procedural grid, permanently.
            depth.release();
            self.gpu_released = true;
            self.bake_done = true;
            return BakeAttempt::Failed;
        };

        match self.build_bitmap(&pixels) {
            Some(rgba) => {
                self.baked = Some(Baked::Bitmap(rgba));
                self.bake_done = true;
                depth.release();
                self.gpu_released = true;
                BakeAttempt::Bitmap
            }
            None => BakeAttempt::BitmapRejected,
        }
    }

    /// `_releaseGpu()` (`minimap.js:282-288`), through the optional seam —
    /// `this._rt?.dispose()` is a no-op when the depth path never ran.
    fn release_gpu(&mut self, depth: Option<&mut dyn DepthBakeSource>) {
        if let Some(d) = depth {
            d.release();
        }
        self.gpu_released = true;
    }

    /// `dispose()` (`minimap.js:598-602`), minus `this.root.remove()`.
    pub fn dispose(&mut self, depth: Option<&mut dyn DepthBakeSource>) {
        self.release_gpu(depth);
        self.baked = None;
    }

    /// `_buildVectorMap(ctx)` (`minimap.js:183-280`), minus the rasterisation
    /// and the grain (see [`Minimap::apply_grain`]).
    ///
    /// The level→canvas affine is recovered from three probe points through
    /// `levelToWorld`, so the map inherits the world's level yaw without this
    /// module knowing anything about it. The source reuses one scratch
    /// `Vector3` for all three probes; `ox`/`oz` are read out as numbers
    /// before the second probe overwrites it, so the aliasing is harmless.
    pub fn build_vector_map(&mut self, world: &dyn LayoutSource) -> Option<Vec<DrawOp>> {
        let infos = world.buildings();
        if infos.is_empty() {
            return None;
        }
        // `typeof world.levelToWorld !== 'function' || typeof world.isOpen
        // !== 'function'` is structurally impossible behind `LayoutSource`.

        let n = BAKE as f64;
        let ppm = n / self.span;
        // `o`, `ex` and `ez` in the source: the origin and the two unit-axis
        // images, `.x` and `.z` of each.
        let (ox, oz) = world.level_to_world(0.0, 0.0, 0.0);
        let (ex_x, ex_z) = world.level_to_world(1.0, 0.0, 0.0);
        let xx = ex_x - ox;
        let xz = ex_z - oz;
        let (ez_x, ez_z) = world.level_to_world(0.0, 0.0, 1.0);
        let zx = ez_x - ox;
        let zz = ez_z - oz;
        if !xx.is_finite() || !zz.is_finite() {
            return None;
        }

        let mut ops: Vec<DrawOp> = Vec::new();

        // out-of-play ground: the darkest tone on the panel, but nowhere near
        // black
        ops.push(DrawOp::FillStyle("#2b343d".to_string()));
        ops.push(DrawOp::FillRect([0.0, 0.0, n, n]));

        // level -> canvas, so everything below is authored in metres of LEVEL
        // space
        ops.push(DrawOp::SetTransform([
            xx * ppm,
            xz * ppm,
            zx * ppm,
            zz * ppm,
            (ox - self.centre.0) * ppm + n * 0.5,
            (oz - self.centre.1) * ppm + n * 0.5,
        ]));

        // ---- street / alley network, as run-length rects in level space ----
        ops.push(DrawOp::FillStyle("#63717e".to_string()));
        let mut lz = -64.0_f64;
        while lz < 54.0 {
            // `run = -1` is the "no run open" sentinel AND a legal `lx`.
            // See the module doc: this is the ported defect.
            let mut run = -1.0_f64;
            let mut lx = -44.0_f64;
            while lx <= 44.0 + STEP {
                let cxw = ox + lx * xx + (lz + STEP * 0.5) * zx;
                let czw = oz + lx * xz + (lz + STEP * 0.5) * zz;
                let open = lx <= 44.0 && world.is_open(cxw, czw, 0.0);
                if open && run < 0.0 {
                    run = lx;
                } else if !open && run >= 0.0 {
                    ops.push(DrawOp::FillRect([run, lz, lx - run, STEP * 1.16]));
                    run = -1.0;
                }
                lx += STEP;
            }
            lz += STEP;
        }

        // ---- building footprints -------------------------------------------
        for spec in &infos {
            let (Some(w), Some(d)) = (spec.w, spec.d) else {
                continue;
            };
            let x0 = spec.x.unwrap_or(0.0) - w * 0.5;
            let z0 = spec.z.unwrap_or(0.0) - d * 0.5;
            // taller mass reads slightly lighter
            let t = clamp01((spec.floors.unwrap_or(2.0) - 1.0) / 3.0);
            ops.push(DrawOp::FillStyle(format!(
                "rgb({},{},{})",
                fmt_int(jsmath::round(lerp(50.0, 68.0, t))),
                fmt_int(jsmath::round(lerp(59.0, 79.0, t))),
                fmt_int(jsmath::round(lerp(68.0, 90.0, t)))
            )));
            ops.push(DrawOp::FillRect([x0, z0, w, d]));
            // a light return on the north and west edges
            ops.push(DrawOp::FillStyle("rgba(206,228,244,.20)".to_string()));
            ops.push(DrawOp::FillRect([x0, z0, w, 0.34]));
            ops.push(DrawOp::FillRect([x0, z0, 0.34, d]));
            ops.push(DrawOp::FillStyle("rgba(3,7,10,.34)".to_string()));
            ops.push(DrawOp::FillRect([x0, z0 + d - 0.34, w, 0.34]));
            ops.push(DrawOp::FillRect([x0 + w - 0.34, z0, 0.34, d]));
        }
        ops.push(DrawOp::SetTransform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));

        Some(ops)
    }

    /// The grain pass both bakes end with (`minimap.js:266-276` for the vector
    /// map, `minimap.js:401,424-427` for the bitmap): *no HUD surface is a
    /// flat colour*.
    ///
    /// Applied by the view, after it has rasterised a [`Baked::Vector`] draw
    /// list into `rgba` (`BAKE`² RGBA). One `rng.float()` per pixel, in raster
    /// order — the same draw count and order as the source, so a later
    /// consumer of this fork sees the same stream.
    ///
    /// Alpha is forced to `255`: the widget must composite as one solid layer
    /// so nothing in the frame can ever read *through* the map.
    pub fn apply_grain(&mut self, rgba: &mut [u8]) {
        for px in rgba.chunks_exact_mut(4) {
            let g = (self.rng.float() - 0.5) * 5.5;
            px[0] = u8_clamped(f64::from(px[0]) + g);
            px[1] = u8_clamped(f64::from(px[1]) + g);
            px[2] = u8_clamped(f64::from(px[2]) + g);
            px[3] = 255;
        }
    }

    /// `_buildBitmap()` (`minimap.js:306-433`) — height field → stylised map
    /// bitmap, in full.
    ///
    /// `pixels` is the seam's output: `BAKE`² RGBA, bottom-up. Returns
    /// `BAKE`² RGBA in image space, or `None` for the source's two `false`
    /// returns (no pixels; too little occupancy for the scene to be built
    /// yet).
    ///
    /// Storage widths are the algorithm: `hgt`, `cov`, `cr`, `cg`, `cb` and
    /// the blur scratch are `Float32Array`, so every store narrows to f32
    /// while every intermediate stays f64.
    pub fn build_bitmap(&mut self, pixels: &[u8]) -> Option<Vec<u8>> {
        let n = BAKE;
        if pixels.len() < n * n * 4 {
            return None;
        }
        let mut hgt = vec![0.0_f32; n * n];
        let mut occ = vec![0_u8; n * n];
        let mut occupied = 0_usize;
        for i in 0..n * n {
            // MeshDepthMaterial + BasicDepthPacking stores (1 - fragCoordZ),
            // and fragCoordZ is linear for an ortho camera: recover world
            // height.
            let d = f64::from(pixels[i * 4]) / 255.0;
            let h = clamp(CAM_Y - NEAR - (1.0 - d) * (FAR - NEAR), 0.0, HEIGHT_RANGE);
            hgt[i] = h as f32;
            // The source tests `h`, the f64 — **not** `hgt[i]`, the value it
            // just narrowed to f32. The two disagree for an `h` within half
            // an f32 ulp of `1.35`, so the occupancy mask is decided at f64
            // width while everything downstream reads f32.
            if h > 1.35 {
                occ[i] = 1;
                occupied += 1;
            }
        }
        // Nothing built yet (or an empty scene) — try again in a few frames.
        if (occupied as f64) < (n * n) as f64 * 0.004 {
            return None;
        }

        // ---- roof colour, premultiplied by occupancy (bake space) ----------
        let mut cov = vec![0.0_f32; n * n];
        let mut cr = vec![0.0_f32; n * n];
        let mut cg = vec![0.0_f32; n * n];
        let mut cb = vec![0.0_f32; n * n];
        for by in 0..n {
            for bx in 0..n {
                let bi = by * n + bx;
                if occ[bi] == 0 {
                    continue;
                }
                let h = f64::from(hgt[bi]);
                let t = clamp01((h - 1.35) / 11.0);
                // fake key light from the north-west so blocks read as volumes
                let in_x = if bx > 0 { f64::from(hgt[bi - 1]) } else { h };
                let in_y = if by < n - 1 { f64::from(hgt[bi + n]) } else { h };
                let key = clamp(((h - in_x) + (h - in_y)) * 0.22, -0.25, 0.35);
                cov[bi] = 1.0;
                cr[bi] = (lerp(18.0, 36.0, t) * (1.0 + key)) as f32;
                cg[bi] = (lerp(24.0, 47.0, t) * (1.0 + key)) as f32;
                cb[bi] = (lerp(30.0, 55.0, t) * (1.0 + key)) as f32;
            }
        }

        // ---- separable radius-1 box blur (one pass = a 3x3 tent) -----------
        let mut tmp = vec![0.0_f32; n * n];
        blur(&mut cov, &mut tmp, n, 1);
        blur(&mut cr, &mut tmp, n, 1);
        blur(&mut cg, &mut tmp, n, 1);
        blur(&mut cb, &mut tmp, n, 1);

        let mut out = vec![0_u8; n * n * 4];

        // street tone — never pure black, always slightly blue
        const FR: f64 = 9.0;
        const FG: f64 = 13.0;
        const FB: f64 = 17.0;
        // boundary rim: a whisper above the fill, not a wireframe
        const RR: f64 = 62.0;
        const RG: f64 = 82.0;
        const RB: f64 = 97.0;

        for y in 0..n {
            for x in 0..n {
                // readRenderTargetPixels is bottom-up; flip into image space
                let si = (n - 1 - y) * n + x;
                let grain = (self.rng.float() - 0.5) * 3.2;

                let w = f64::from(cov[si]);
                let mut r = FR;
                let mut g = FG;
                let mut b = FB;
                if w > 0.002 {
                    let iw = 1.0 / w;
                    r = lerp(FR, f64::from(cr[si]) * iw, w);
                    g = lerp(FG, f64::from(cg[si]) * iw, w);
                    b = lerp(FB, f64::from(cb[si]) * iw, w);
                }

                // soft footprint rim, peaking on the coverage midline
                let rim = 4.0 * w * (1.0 - w);
                if rim > 0.002 {
                    let a = rim * rim * 0.66;
                    r = lerp(r, RR, a);
                    g = lerp(g, RG, a);
                    b = lerp(b, RB, a);
                }

                let di = (y * n + x) * 4;
                out[di] = u8_clamped(r + grain);
                out[di + 1] = u8_clamped(g + grain);
                out[di + 2] = u8_clamped(b + grain);
                out[di + 3] = 255;
            }
        }
        Some(out)
    }

    /* --------------------------------------------------------- draw --- */

    /// `draw(s)` (`minimap.js:441-596`) — one frame's canvas2d call sequence.
    ///
    /// `s` is the facade's `_mmState` ([`MinimapState`], assembled by
    /// [`crate::ui::system::UiCore::late_update`]). Its fields are
    /// non-optional, so the source's `?? 0` / `?? 80` fallbacks and its
    /// `if (objs)` / `if (blips)` null guards are unreachable from the real
    /// caller; the empty-list arms are still exercised.
    ///
    /// Returns an empty list for `if (!S) return;` — a zero (or NaN) backing
    /// store, which is what `resize` produces for `dpr = 0` before the
    /// `|| 1` … it cannot, actually: `or_one` prevents it. `px` can only be
    /// `0` if `k` is `0`, which `style::scale_factor` never returns. The arm
    /// is kept because the source has it and a caller could still drive it.
    pub fn draw(&self, s: &MinimapState) -> Vec<DrawOp> {
        let mut ops: Vec<DrawOp> = Vec::new();
        let ss = self.px;
        if ss == 0.0 || ss.is_nan() {
            return ops;
        }
        let half = ss * 0.5;
        let ppm = ss / self.view_span; // canvas pixels per metre

        ops.push(DrawOp::SetTransform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
        ops.push(DrawOp::ClearRect([0.0, 0.0, ss, ss]));

        // base plate — never pure black, always slightly blue
        ops.push(DrawOp::FillStyle("#2b343d".to_string()));
        ops.push(DrawOp::FillRect([0.0, 0.0, ss, ss]));

        ops.push(DrawOp::Save);
        ops.push(DrawOp::BeginPath);
        ops.push(DrawOp::Rect([0.0, 0.0, ss, ss]));
        ops.push(DrawOp::Clip);

        let cx = s.x;
        let cz = s.z;

        if self.baked.is_some() {
            let bppm = BAKE as f64 / self.span;
            let src_w = self.view_span * bppm;
            let sx = (cx - self.centre.0) * bppm + BAKE as f64 * 0.5 - src_w * 0.5;
            let sy = (cz - self.centre.1) * bppm + BAKE as f64 * 0.5 - src_w * 0.5;
            ops.push(DrawOp::ImageSmoothingEnabled(true));
            ops.push(DrawOp::ImageSmoothingQuality("high"));
            ops.push(DrawOp::DrawBaked {
                sx,
                sy,
                sw: src_w,
                sh: src_w,
                dx: 0.0,
                dy: 0.0,
                dw: ss,
                dh: ss,
            });
        } else {
            ops.push(DrawOp::FillStyle("#2b333b".to_string()));
            ops.push(DrawOp::FillRect([0.0, 0.0, ss, ss]));
        }

        // 10m grid, phase-locked to world space so it scrolls with the player
        let u = ss / self.css_size; // canvas pixels per css reference pixel
        ops.push(DrawOp::LineWidth(1.0));
        ops.push(DrawOp::StrokeStyle("rgba(10,17,23,.20)"));
        ops.push(DrawOp::BeginPath);
        let n0x = ((cx - self.view_span * 0.5) / 10.0).floor();
        let n1x = ((cx + self.view_span * 0.5) / 10.0).ceil();
        let mut nx = n0x;
        while nx <= n1x {
            let gx = jsmath::round((nx * 10.0 - cx) * ppm + half) + 0.5;
            ops.push(DrawOp::MoveTo(gx, 0.0));
            ops.push(DrawOp::LineTo(gx, ss));
            nx += 1.0;
        }
        let n0z = ((cz - self.view_span * 0.5) / 10.0).floor();
        let n1z = ((cz + self.view_span * 0.5) / 10.0).ceil();
        let mut nz = n0z;
        while nz <= n1z {
            let gy = jsmath::round((nz * 10.0 - cz) * ppm + half) + 0.5;
            ops.push(DrawOp::MoveTo(0.0, gy));
            ops.push(DrawOp::LineTo(ss, gy));
            nz += 1.0;
        }
        ops.push(DrawOp::Stroke);

        // view cone. `(x * PI) / 180`, NOT `to_radians` (`x * (PI / 180)`) —
        // a different grouping, and float multiplication is not associative.
        let heading = (s.heading * std::f64::consts::PI) / 180.0;
        let fov = ((s.fov * 0.5) * std::f64::consts::PI) / 180.0;
        let cone_r = ss * 0.42;
        ops.push(DrawOp::CreateRadialGradient {
            x0: half,
            y0: half,
            r0: 2.0,
            x1: half,
            y1: half,
            r1: cone_r,
        });
        ops.push(DrawOp::AddColorStop(0.0, "rgba(222,242,255,.26)"));
        ops.push(DrawOp::AddColorStop(0.7, "rgba(222,242,255,.075)"));
        ops.push(DrawOp::AddColorStop(1.0, "rgba(214,238,255,0)"));
        ops.push(DrawOp::FillStyleGradient);
        ops.push(DrawOp::BeginPath);
        ops.push(DrawOp::MoveTo(half, half));
        ops.push(DrawOp::Arc {
            x: half,
            y: half,
            r: cone_r,
            start: -std::f64::consts::PI / 2.0 + heading - fov,
            end: -std::f64::consts::PI / 2.0 + heading + fov,
        });
        ops.push(DrawOp::ClosePath);
        ops.push(DrawOp::Fill);
        ops.push(DrawOp::StrokeStyle("rgba(226,244,255,.17)"));
        ops.push(DrawOp::LineWidth(1.0));
        ops.push(DrawOp::Stroke);

        // objectives
        if !s.objectives.is_empty() {
            ops.push(DrawOp::Font(format!(
                "700 {}px system-ui, sans-serif",
                to_fixed_1(9.5 * u)
            )));
            ops.push(DrawOp::TextAlign("center"));
            ops.push(DrawOp::TextBaseline("middle"));
            let r = 6.0 * u;
            for o in &s.objectives {
                let dx = clamp((o.x - cx) * ppm + half, r + 1.0, ss - r - 1.0);
                let dy = clamp((o.z - cz) * ppm + half, r + 1.0, ss - r - 1.0);
                ops.push(DrawOp::FillStyle("rgba(121,210,255,.94)".to_string()));
                ops.push(DrawOp::StrokeStyle("rgba(4,14,20,.8)"));
                ops.push(DrawOp::LineWidth(1.0));
                ops.push(DrawOp::BeginPath);
                ops.push(DrawOp::Rect([dx - r, dy - r, r * 2.0, r * 2.0]));
                ops.push(DrawOp::Fill);
                ops.push(DrawOp::Stroke);
                ops.push(DrawOp::FillStyle("#06171f".to_string()));
                ops.push(DrawOp::FillText(o.label.clone(), dx, dy + 0.5));
            }
        }

        // blips
        for b in &s.blips {
            let dx = (f64::from(b.x) - cx) * ppm + half;
            let dy = (f64::from(b.z) - cz) * ppm + half;
            // The cull margin is 8 CANVAS pixels, not `8 * u` — it does not
            // scale with the HUD, exactly as the source writes it.
            if dx < -8.0 || dy < -8.0 || dx > ss + 8.0 || dy > ss + 8.0 {
                continue;
            }
            let enemy = !b.friendly; // `b.kind !== 'friend'`
            let r = 3.4 * u;
            ops.push(DrawOp::Save);
            ops.push(DrawOp::Translate(dx, dy));
            ops.push(DrawOp::Rotate((b.heading_deg * std::f64::consts::PI) / 180.0));
            ops.push(DrawOp::FillStyle(
                if enemy {
                    "rgba(255,74,58,.96)"
                } else {
                    "rgba(126,196,255,.95)"
                }
                .to_string(),
            ));
            ops.push(DrawOp::ShadowColor(if enemy {
                "rgba(255,60,40,.85)"
            } else {
                "rgba(120,190,255,.7)"
            }));
            ops.push(DrawOp::ShadowBlur(6.0 * u));
            ops.push(DrawOp::BeginPath);
            ops.push(DrawOp::MoveTo(0.0, -r * 1.5));
            ops.push(DrawOp::LineTo(r * 1.15, r * 1.1));
            ops.push(DrawOp::LineTo(-r * 1.15, r * 1.1));
            ops.push(DrawOp::ClosePath);
            ops.push(DrawOp::Fill);
            ops.push(DrawOp::Restore);
        }

        // player arrow — stays centred and rotates (the map is north-up,
        // matching the compass strip)
        ops.push(DrawOp::Save);
        ops.push(DrawOp::Translate(half, half));
        ops.push(DrawOp::Rotate(heading));
        let pr = 4.8 * u;
        ops.push(DrawOp::BeginPath);
        ops.push(DrawOp::MoveTo(0.0, -pr * 1.55));
        ops.push(DrawOp::LineTo(pr * 1.15, pr * 1.3));
        ops.push(DrawOp::LineTo(0.0, pr * 0.6));
        ops.push(DrawOp::LineTo(-pr * 1.15, pr * 1.3));
        ops.push(DrawOp::ClosePath);
        ops.push(DrawOp::FillStyle("#f6fcff".to_string()));
        ops.push(DrawOp::StrokeStyle("rgba(2,6,10,.85)"));
        ops.push(DrawOp::LineWidth(1.6 * u));
        ops.push(DrawOp::LineJoin("round"));
        ops.push(DrawOp::ShadowColor("rgba(180,225,255,.85)"));
        ops.push(DrawOp::ShadowBlur(5.0 * u));
        ops.push(DrawOp::Stroke);
        ops.push(DrawOp::Fill);
        ops.push(DrawOp::ShadowBlur(0.0));
        ops.push(DrawOp::Restore);

        // edge falloff so the map sinks into the frame instead of ending
        // abruptly
        ops.push(DrawOp::CreateRadialGradient {
            x0: half,
            y0: half,
            r0: ss * 0.28,
            x1: half,
            y1: half,
            r1: ss * 0.72,
        });
        ops.push(DrawOp::AddColorStop(0.0, "rgba(0,0,0,0)"));
        ops.push(DrawOp::AddColorStop(1.0, "rgba(0,0,0,.17)"));
        ops.push(DrawOp::FillStyleGradient);
        ops.push(DrawOp::FillRect([0.0, 0.0, ss, ss]));

        ops.push(DrawOp::Restore);
        ops
    }
}

/// `blur(buf, passes)` (`minimap.js:351-371`) — a separable radius-1 box blur;
/// one pass is a 3×3 tent. Edge taps clamp to the border.
///
/// `buf` and `tmp` are `Float32Array` in the source: the sum and the divide
/// happen in f64, the store narrows to f32.
fn blur(buf: &mut [f32], tmp: &mut [f32], n: usize, passes: usize) {
    for _ in 0..passes {
        for y in 0..n {
            let row = y * n;
            for x in 0..n {
                let a = f64::from(buf[row + if x > 0 { x - 1 } else { 0 }]);
                let b = f64::from(buf[row + x]);
                let c = f64::from(buf[row + if x < n - 1 { x + 1 } else { n - 1 }]);
                tmp[row + x] = ((a + b + c) / 3.0) as f32;
            }
        }
        for x in 0..n {
            for y in 0..n {
                let a = f64::from(tmp[(if y > 0 { y - 1 } else { 0 }) * n + x]);
                let b = f64::from(tmp[y * n + x]);
                let c = f64::from(tmp[(if y < n - 1 { y + 1 } else { n - 1 }) * n + x]);
                buf[y * n + x] = ((a + b + c) / 3.0) as f32;
            }
        }
    }
}

/// `String(x)` for a `Math.round` result: an integral `f64` prints with no
/// decimal point in JS, and `-0` prints as `"0"`.
fn fmt_int(v: f64) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v}")
}

/// The canvas realiser for [`Minimap::draw`]'s display list — `wasm32` only.
///
/// This is the widget's missing half. Every other `ui/` widget splits into a
/// pure frame and a `view` that writes it onto DOM nodes; this one is a canvas
/// painter, so its "frame" is a [`DrawOp`] list and its view is the interpreter
/// that replays it against a `CanvasRenderingContext2D`. There is one arm per
/// [`DrawOp`] variant and no arithmetic anywhere in it: every number was
/// computed by [`Minimap::draw`] on the native side and is pinned by that
/// function's own golden.
///
/// Two arms are deliberately inert, and both are unreachable today:
///
/// * [`DrawOp::DrawBaked`] needs `this.baked` to be a real image. A bake only
///   exists once [`Minimap::try_bake`] has been handed a [`LayoutSource`], and
///   nothing in this port can hand it one — `scene::level::Level` keeps the
///   `WorldSystem`'s *products* and drops the system itself, so the building
///   footprints `_buildVectorMap` reads are not reachable from a running
///   `Game`. Until that seam exists, `Minimap::baked` is always `None` and
///   `draw` never emits this op. It is logged rather than approximated.
/// * [`DrawOp::ImageSmoothingQuality`] has no `web-sys` binding in 0.3.99 and
///   is only ever emitted immediately before `DrawBaked`, so it is skipped for
///   the same reason.
#[cfg(target_arch = "wasm32")]
pub mod view {
    use wasm_bindgen::JsCast;
    use web_sys::{CanvasGradient, CanvasRenderingContext2d, Element, HtmlCanvasElement};

    use super::super::util::dom;
    use super::{DrawOp, Minimap};

    /// The minimap's DOM: the framed root, the canvas it paints into, and the
    /// static chrome (`minimap.js:26-33`).
    pub struct MinimapView {
        root: Element,
        canvas: HtmlCanvasElement,
        g: CanvasRenderingContext2d,
        /// The gradient built by the last `CreateRadialGradient` + its
        /// `AddColorStop`s, waiting for the `FillStyleGradient` that installs
        /// it. The source builds and assigns in one expression; the display
        /// list splits that into three ops, so the half-built object lives
        /// here between them.
        gradient: Option<CanvasGradient>,
        /// The backing-store edge last written, so `resize` only touches
        /// `canvas.width` when it changed (`minimap.js:61-64`) — assigning it
        /// clears the canvas even when the value is unchanged.
        px: f64,
    }

    impl MinimapView {
        /// `constructor(parent, rng)`'s DOM half (`minimap.js:26-33`).
        pub fn new(parent: &Element) -> MinimapView {
            let root = dom::el("div", Some("ow-minimap"), Some(parent));
            let canvas: HtmlCanvasElement =
                dom::el("canvas", None, Some(&root)).unchecked_into();
            let g: CanvasRenderingContext2d = canvas
                .get_context("2d")
                .expect("a 2d context request")
                .expect("a browser that grants a 2d context")
                .unchecked_into();
            ["ow-mm-corner tl", "ow-mm-corner tr", "ow-mm-corner bl", "ow-mm-corner br"]
                .into_iter()
                .for_each(|class| {
                    dom::el("div", Some(class), Some(&root));
                });
            let n = dom::el("div", Some("ow-mm-n"), Some(&root));
            dom::set_text(&n, "N");
            let tag = dom::el("div", Some("ow-mm-tag"), Some(&root));
            let zone = dom::el("span", None, Some(&tag));
            dom::set_text(&zone, "ZONE 07");
            let scale = dom::el("span", None, Some(&tag));
            dom::set_text(&scale, "60M");
            MinimapView { root, canvas, g, gradient: None, px: 0.0 }
        }

        /// `resize(k)`'s DOM half (`minimap.js:58-67`). The arithmetic is
        /// [`Minimap::resize`]'s; this only pushes the result at the backing
        /// store, and only when it moved.
        pub fn resize(&mut self, minimap: &Minimap) {
            if self.px == minimap.px {
                return;
            }
            self.px = minimap.px;
            self.canvas.set_width(minimap.px as u32);
            self.canvas.set_height(minimap.px as u32);
        }

        /// Replay one [`Minimap::draw`] display list.
        pub fn execute(&mut self, ops: &[DrawOp]) {
            for op in ops {
                self.apply(op);
            }
        }

        fn apply(&mut self, op: &DrawOp) {
            let g = &self.g;
            match op {
                DrawOp::SetTransform(v) => {
                    let _ = g.set_transform(v[0], v[1], v[2], v[3], v[4], v[5]);
                }
                DrawOp::ClearRect(v) => g.clear_rect(v[0], v[1], v[2], v[3]),
                DrawOp::FillStyle(s) => g.set_fill_style_str(s),
                DrawOp::FillStyleGradient => {
                    if let Some(grad) = self.gradient.take() {
                        g.set_fill_style_canvas_gradient(&grad);
                    }
                }
                DrawOp::StrokeStyle(s) => g.set_stroke_style_str(s),
                DrawOp::FillRect(v) => g.fill_rect(v[0], v[1], v[2], v[3]),
                DrawOp::Save => g.save(),
                DrawOp::Restore => g.restore(),
                DrawOp::BeginPath => g.begin_path(),
                DrawOp::ClosePath => g.close_path(),
                DrawOp::Fill => g.fill(),
                DrawOp::Stroke => g.stroke(),
                DrawOp::Clip => g.clip(),
                DrawOp::Rect(v) => g.rect(v[0], v[1], v[2], v[3]),
                // See the module doc: unreachable while no bake can exist.
                DrawOp::DrawBaked { .. } => {}
                DrawOp::ImageSmoothingEnabled(b) => g.set_image_smoothing_enabled(*b),
                DrawOp::ImageSmoothingQuality(_) => {}
                DrawOp::LineWidth(w) => g.set_line_width(*w),
                DrawOp::LineJoin(j) => g.set_line_join(j),
                DrawOp::MoveTo(x, y) => g.move_to(*x, *y),
                DrawOp::LineTo(x, y) => g.line_to(*x, *y),
                DrawOp::Arc { x, y, r, start, end } => {
                    let _ = g.arc(*x, *y, *r, *start, *end);
                }
                DrawOp::CreateRadialGradient { x0, y0, r0, x1, y1, r1 } => {
                    self.gradient = g.create_radial_gradient(*x0, *y0, *r0, *x1, *y1, *r1).ok();
                }
                DrawOp::AddColorStop(offset, colour) => {
                    if let Some(grad) = self.gradient.as_ref() {
                        let _ = grad.add_color_stop(*offset as f32, colour);
                    }
                }
                DrawOp::Font(s) => g.set_font(s),
                DrawOp::TextAlign(s) => g.set_text_align(s),
                DrawOp::TextBaseline(s) => g.set_text_baseline(s),
                DrawOp::FillText(t, x, y) => {
                    let _ = g.fill_text(t, *x, *y);
                }
                DrawOp::Translate(x, y) => {
                    let _ = g.translate(*x, *y);
                }
                DrawOp::Rotate(t) => {
                    let _ = g.rotate(*t);
                }
                DrawOp::ShadowColor(c) => g.set_shadow_color(c),
                DrawOp::ShadowBlur(b) => g.set_shadow_blur(*b),
            }
        }

        /// `dispose()`'s DOM half (`minimap.js:601`).
        pub fn dispose(&self) {
            dom::remove(&self.root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::system::MinimapObjective;
    use crate::ui::Blip;

    fn mm() -> Minimap {
        Minimap::new(Rng::new(0x5eed_1234), 1.0)
    }

    /// `world.isOpen` true on one contiguous band per row, so the run-length
    /// coder has something to close.
    struct BandWorld {
        /// open band in LEVEL x
        x0: f64,
        x1: f64,
        specs: Vec<FootprintSpec>,
    }

    impl LayoutSource for BandWorld {
        fn buildings(&self) -> Vec<FootprintSpec> {
            self.specs.clone()
        }
        fn level_to_world(&self, x: f64, _y: f64, z: f64) -> (f64, f64) {
            // identity affine, so world == level and the queries are readable
            (x, z)
        }
        fn is_open(&self, x: f64, _z: f64, _margin: f64) -> bool {
            x >= self.x0 && x <= self.x1
        }
    }

    fn one_spec() -> Vec<FootprintSpec> {
        vec![FootprintSpec {
            x: Some(0.0),
            z: Some(0.0),
            w: Some(10.0),
            d: Some(6.0),
            floors: Some(4.0),
        }]
    }

    #[test]
    fn u8_clamped_rounds_half_to_even_and_clamps() {
        assert_eq!(u8_clamped(2.5), 2);
        assert_eq!(u8_clamped(3.5), 4);
        assert_eq!(u8_clamped(-3.0), 0);
        assert_eq!(u8_clamped(300.0), 255);
        assert_eq!(u8_clamped(f64::NAN), 0);
        assert_eq!(u8_clamped(9.4), 9);
    }

    #[test]
    fn to_fixed_1_breaks_ties_upward_where_rust_breaks_them_to_even() {
        assert_eq!(to_fixed_1(9.25), "9.3", "ECMA picks the LARGER n");
        assert_eq!(format!("{:.1}", 9.25_f64), "9.2", "Rust ties to even");
        assert_eq!(to_fixed_1(9.5), "9.5");
        assert_eq!(to_fixed_1(9.75), "9.8");
    }

    #[test]
    fn resize_matches_the_source_dpr_clamp() {
        let mut m = mm();
        m.resize(1.0, 1.0);
        assert_eq!(m.px, 178.0);
        m.resize(1.0, 3.0); // Math.min(2, 3)
        assert_eq!(m.px, 356.0);
        m.resize(1.0, 0.0); // `|| 1`
        assert_eq!(m.px, 178.0);
        m.resize(1.0, f64::NAN); // NaN is falsy in JS too
        assert_eq!(m.px, 178.0);
    }

    /// The ported defect. A street band entirely west of `lx = 0` produces no
    /// rect at all, because `run = -1` is both the sentinel and a legal `lx`.
    #[test]
    fn negative_lx_street_runs_are_never_emitted() {
        let mut m = mm();
        let west = BandWorld {
            x0: -20.0,
            x1: -10.0,
            specs: one_spec(),
        };
        let ops = m.build_vector_map(&west as &dyn LayoutSource).expect("a vector map");
        let street_rects = count_street_rects(&ops);
        assert_eq!(
            street_rects, 0,
            "a band at lx in [-20, -10] must emit nothing — see the module doc"
        );

        let mut m2 = mm();
        let east = BandWorld {
            x0: 10.0,
            x1: 20.0,
            specs: one_spec(),
        };
        let ops2 = m2.build_vector_map(&east as &dyn LayoutSource).expect("a vector map");
        assert_eq!(
            count_street_rects(&ops2),
            236,
            "the identical band mirrored to lx in [10, 20] emits one rect per row"
        );
    }

    /// A band straddling zero is clipped to its positive half.
    #[test]
    fn a_straddling_street_run_loses_its_western_half() {
        let mut m = mm();
        let ops = m
            .build_vector_map(&BandWorld {
                x0: -20.0,
                x1: 20.0,
                specs: one_spec(),
            })
            .expect("a vector map");
        let first = ops
            .iter()
            .skip_while(|o| !matches!(o, DrawOp::FillStyle(s) if s == "#63717e"))
            .find_map(|o| match o {
                DrawOp::FillRect(v) => Some(*v),
                _ => None,
            })
            .expect("one street rect");
        assert_eq!(first[0], 0.0, "the run starts at lx = 0, not -20");
        assert_eq!(first[2], 20.5, "and runs to the first closed cell at 20.5");
        assert_eq!(first[3], STEP * 1.16);
    }

    fn count_street_rects(ops: &[DrawOp]) -> usize {
        // every FillRect between the `#63717e` style and the next FillStyle
        ops.iter()
            .skip_while(|o| !matches!(o, DrawOp::FillStyle(s) if s == "#63717e"))
            .skip(1)
            .take_while(|o| !matches!(o, DrawOp::FillStyle(_)))
            .filter(|o| matches!(o, DrawOp::FillRect(_)))
            .count()
    }

    #[test]
    fn a_spec_without_width_or_depth_is_skipped() {
        let mut m = mm();
        let ops = m
            .build_vector_map(&BandWorld {
                x0: 100.0,
                x1: 100.0, // nothing open
                specs: vec![
                    FootprintSpec::default(),
                    FootprintSpec {
                        w: Some(4.0),
                        d: None,
                        ..FootprintSpec::default()
                    },
                    FootprintSpec {
                        w: Some(4.0),
                        d: Some(4.0),
                        ..FootprintSpec::default()
                    },
                ],
            })
            .expect("a vector map");
        // one accepted spec -> five rects and three fill styles
        assert_eq!(
            ops.iter().filter(|o| matches!(o, DrawOp::FillRect(_))).count(),
            1 + 5,
            "the base plate plus one footprint's five rects"
        );
    }

    #[test]
    fn a_footprint_with_no_floors_defaults_to_two_and_the_darkest_tone() {
        let mut m = mm();
        let ops = m
            .build_vector_map(&BandWorld {
                x0: 100.0,
                x1: 100.0,
                specs: vec![FootprintSpec {
                    w: Some(4.0),
                    d: Some(4.0),
                    ..FootprintSpec::default()
                }],
            })
            .expect("a vector map");
        // floors ?? 2 -> t = 1/3
        let want = format!(
            "rgb({},{},{})",
            fmt_int(jsmath::round(lerp(50.0, 68.0, 1.0 / 3.0))),
            fmt_int(jsmath::round(lerp(59.0, 79.0, 1.0 / 3.0))),
            fmt_int(jsmath::round(lerp(68.0, 90.0, 1.0 / 3.0)))
        );
        assert!(
            ops.contains(&DrawOp::FillStyle(want.clone())),
            "expected {want} among the ops"
        );
    }

    #[test]
    fn an_empty_building_list_declines_the_vector_map() {
        let mut m = mm();
        assert!(m
            .build_vector_map(&BandWorld {
                x0: 0.0,
                x1: 1.0,
                specs: vec![]
            })
            .is_none());
    }

    struct NanWorld;
    impl LayoutSource for NanWorld {
        fn buildings(&self) -> Vec<FootprintSpec> {
            one_spec()
        }
        fn level_to_world(&self, x: f64, _y: f64, z: f64) -> (f64, f64) {
            (x * f64::NAN, z)
        }
        fn is_open(&self, _x: f64, _z: f64, _m: f64) -> bool {
            false
        }
    }

    #[test]
    fn a_non_finite_affine_declines_the_vector_map() {
        let mut m = mm();
        assert!(m.build_vector_map(&NanWorld as &dyn LayoutSource).is_none());
    }

    /* ---- bake state machine ---- */

    struct Depth {
        pixels: Vec<u8>,
        fail: bool,
        released: usize,
        reads: usize,
    }

    impl DepthBakeSource for Depth {
        fn read_ortho_depth(&mut self, req: &DepthBakeRequest) -> Result<Vec<u8>, ()> {
            self.reads += 1;
            assert_eq!(req.size, BAKE);
            assert_eq!(req.span, 190.0);
            assert_eq!(req.cam_y, CAM_Y);
            if self.fail {
                return Err(());
            }
            Ok(self.pixels.clone())
        }
        fn release(&mut self) {
            self.released += 1;
        }
    }

    fn depth_pixels(fill: u8, rows: usize) -> Vec<u8> {
        let mut p = vec![0_u8; BAKE * BAKE * 4];
        for i in 0..rows * BAKE {
            p[i * 4] = fill;
            p[i * 4 + 3] = 255;
        }
        p
    }

    #[test]
    fn the_vector_map_wins_over_the_depth_bake() {
        let mut m = mm();
        let mut d = Depth {
            pixels: depth_pixels(255, BAKE),
            fail: false,
            released: 0,
            reads: 0,
        };
        let w = BandWorld {
            x0: 0.0,
            x1: 10.0,
            specs: one_spec(),
        };
        assert_eq!(m.try_bake(Some(&w as &dyn LayoutSource), Some(&mut d as &mut dyn DepthBakeSource)), BakeAttempt::Vector);
        assert_eq!(d.reads, 0, "the GPU path must not run");
        assert_eq!(d.released, 1);
        assert!(m.bake_done);
        assert!(matches!(m.baked, Some(Baked::Vector(_))));
        // and a second call is skipped
        assert_eq!(m.try_bake(Some(&w as &dyn LayoutSource), Some(&mut d as &mut dyn DepthBakeSource)), BakeAttempt::Skipped);
        assert_eq!(m.bake_tries, 1);
    }

    #[test]
    fn no_world_and_no_renderer_spends_a_try_and_does_nothing() {
        let mut m = mm();
        assert_eq!(m.try_bake(None, None), BakeAttempt::NoRenderer);
        assert_eq!(m.bake_tries, 1);
        assert!(!m.bake_done);
    }

    #[test]
    fn seven_tries_is_the_cap() {
        let mut m = mm();
        for _ in 0..7 {
            assert_eq!(m.try_bake(None, None), BakeAttempt::NoRenderer);
        }
        assert_eq!(m.bake_tries, 7);
        assert_eq!(m.try_bake(None, None), BakeAttempt::Skipped);
        assert_eq!(m.bake_tries, 7, "`bakeTries > 6` stops the increment");
    }

    #[test]
    fn a_thrown_bake_is_abandoned_permanently() {
        let mut m = mm();
        let mut d = Depth {
            pixels: vec![],
            fail: true,
            released: 0,
            reads: 0,
        };
        assert_eq!(m.try_bake(None, Some(&mut d as &mut dyn DepthBakeSource)), BakeAttempt::Failed);
        assert!(m.bake_done, "the catch arm sets bakeDone anyway");
        assert!(m.baked.is_none(), "so the procedural fallback stands");
        assert_eq!(d.released, 1);
    }

    #[test]
    fn a_nearly_empty_depth_bake_is_rejected_and_retries() {
        let mut m = mm();
        // 1 row of 512 occupied = 512 px, well under 512*512*0.004 = 1048
        let mut d = Depth {
            pixels: depth_pixels(255, 1),
            fail: false,
            released: 0,
            reads: 0,
        };
        assert_eq!(m.try_bake(None, Some(&mut d as &mut dyn DepthBakeSource)), BakeAttempt::BitmapRejected);
        assert!(!m.bake_done);
        assert_eq!(d.released, 0);
    }

    #[test]
    fn a_populated_depth_bake_produces_an_opaque_bitmap() {
        let mut m = mm();
        let mut d = Depth {
            pixels: depth_pixels(255, BAKE),
            fail: false,
            released: 0,
            reads: 0,
        };
        assert_eq!(m.try_bake(None, Some(&mut d as &mut dyn DepthBakeSource)), BakeAttempt::Bitmap);
        let Some(Baked::Bitmap(px)) = &m.baked else {
            panic!("expected a bitmap");
        };
        assert_eq!(px.len(), BAKE * BAKE * 4);
        assert!(
            px.chunks_exact(4).all(|c| c[3] == 255),
            "every pixel is written fully opaque"
        );
    }

    #[test]
    fn a_short_pixel_buffer_is_declined() {
        let mut m = mm();
        assert!(m.build_bitmap(&[0, 0, 0, 255]).is_none());
    }

    /* ---- draw ---- */

    fn state() -> MinimapState {
        MinimapState {
            x: 3.0,
            z: -4.0,
            heading: 30.0,
            fov: 80.0,
            blips: vec![
                Blip {
                    x: 8.0,
                    z: -2.0,
                    friendly: false,
                    heading_deg: 90.0,
                },
                Blip {
                    x: 1.0,
                    z: -9.0,
                    friendly: true,
                    heading_deg: -45.0,
                },
                Blip {
                    x: 900.0,
                    z: 0.0,
                    friendly: false,
                    heading_deg: 0.0,
                },
            ],
            objectives: vec![MinimapObjective {
                x: 40.0,
                z: 40.0,
                label: "A".to_string(),
            }],
        }
    }

    #[test]
    fn a_zero_backing_store_draws_nothing() {
        let mut m = mm();
        m.px = 0.0;
        assert!(m.draw(&state()).is_empty());
    }

    #[test]
    fn the_draw_list_opens_and_closes_balanced() {
        let m = mm();
        let ops = m.draw(&state());
        let saves = ops.iter().filter(|o| **o == DrawOp::Save).count();
        let restores = ops.iter().filter(|o| **o == DrawOp::Restore).count();
        assert_eq!(saves, restores, "every save() is matched");
        assert_eq!(ops[0], DrawOp::SetTransform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
        assert_eq!(*ops.last().unwrap(), DrawOp::Restore);
    }

    #[test]
    fn an_offscreen_blip_is_culled_and_the_others_are_not() {
        let m = mm();
        let ops = m.draw(&state());
        // one Translate per drawn blip, plus one for the player arrow
        let translates = ops
            .iter()
            .filter(|o| matches!(o, DrawOp::Translate(_, _)))
            .count();
        assert_eq!(translates, 3, "two blips survive the cull, plus the arrow");
        assert!(ops.contains(&DrawOp::FillStyle("rgba(255,74,58,.96)".to_string())));
        assert!(ops.contains(&DrawOp::FillStyle("rgba(126,196,255,.95)".to_string())));
    }

    #[test]
    fn an_objective_pip_is_clamped_inside_the_widget() {
        let m = mm();
        let ops = m.draw(&state());
        let u = m.px / m.css_size;
        let r = 6.0 * u;
        let rect = ops
            .iter()
            .find_map(|o| match o {
                DrawOp::Rect(v) if v[2] == r * 2.0 => Some(*v),
                _ => None,
            })
            .expect("an objective pip");
        let dx = rect[0] + r;
        assert_eq!(
            dx,
            m.px - r - 1.0,
            "an objective 37m east of a 60m-span map clamps to the edge"
        );
    }

    #[test]
    fn no_baked_map_falls_back_to_the_flat_plate() {
        let m = mm();
        let ops = m.draw(&state());
        assert!(ops.contains(&DrawOp::FillStyle("#2b333b".to_string())));
        assert!(!ops.iter().any(|o| matches!(o, DrawOp::DrawBaked { .. })));
    }

    #[test]
    fn a_baked_map_is_blitted_with_the_span_ratio() {
        let mut m = mm();
        m.baked = Some(Baked::Bitmap(vec![]));
        let ops = m.draw(&state());
        let blit = ops
            .iter()
            .find_map(|o| match o {
                DrawOp::DrawBaked { sx, sw, .. } => Some((*sx, *sw)),
                _ => None,
            })
            .expect("a blit");
        let bppm = BAKE as f64 / 190.0;
        assert_eq!(blit.1, 60.0 * bppm);
        assert_eq!(blit.0, 3.0 * bppm + 256.0 - 60.0 * bppm * 0.5);
    }

    #[test]
    fn the_heading_uses_the_sources_grouping_not_to_radians() {
        // `(x * PI) / 180` and `x * (PI / 180)` differ in the last ulp for
        // some x; the port must use the former.
        let deg = 137.0_f64;
        let src = (deg * std::f64::consts::PI) / 180.0;
        let mut m = mm();
        m.px = 178.0;
        let mut s = state();
        s.heading = deg;
        s.blips.clear();
        s.objectives.clear();
        let ops = m.draw(&s);
        let rot = ops
            .iter()
            .find_map(|o| match o {
                DrawOp::Rotate(t) => Some(*t),
                _ => None,
            })
            .expect("the player arrow rotation");
        assert_eq!(rot, src);
    }

    #[test]
    fn apply_grain_forces_alpha_and_spends_exactly_one_draw_per_pixel() {
        let mut m = mm();
        let mut buf = vec![100_u8, 100, 100, 0, 100, 100, 100, 0];
        m.apply_grain(&mut buf);
        assert_eq!(buf[3], 255, "alpha is forced opaque");
        assert_eq!(buf[7], 255);
        // Two pixels == two rng draws, no more and no fewer.
        let mut probe = Rng::new(0x5eed_1234);
        let g0 = (probe.float() - 0.5) * 5.5;
        probe.float();
        assert_eq!(
            m.rng.state(),
            probe.state(),
            "the grain must spend one float per pixel"
        );
        assert_eq!(buf[0], u8_clamped(100.0 + g0));
        assert_eq!(buf[1], buf[2], "one grain value drives all three channels");
    }

    #[test]
    fn dispose_drops_the_bake_and_releases_the_seam() {
        let mut m = mm();
        m.baked = Some(Baked::Bitmap(vec![]));
        let mut d = Depth {
            pixels: vec![],
            fail: false,
            released: 0,
            reads: 0,
        };
        m.dispose(Some(&mut d as &mut dyn DepthBakeSource));
        assert!(m.baked.is_none());
        assert_eq!(d.released, 1);
        assert!(m.gpu_released);
    }

    #[test]
    fn encode_round_trips_every_op_kind() {
        let m = mm();
        let ops = m.draw(&state());
        assert!(ops.iter().all(|o| !o.encode().0.is_empty()));
        assert_eq!(
            DrawOp::FillRect([1.0, 2.0, 3.0, 4.0]).encode().1.len(),
            4
        );
    }
}
