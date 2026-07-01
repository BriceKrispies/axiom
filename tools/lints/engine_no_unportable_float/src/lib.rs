#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::{in_zone, is_engine_file, is_in_crate_dir, markers};
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty;
use rustc_span::Span;

dylint_linting::declare_late_lint! {
    /// ### What it does
    /// Inside the engine's **deterministic step path** — the `axiom-physics`
    /// module and any `#[sim]` zone — flags float operations that are *not*
    /// bit-reproducible across targets:
    /// - **Fused multiply-add**: `f32::mul_add` / `f64::mul_add` and the
    ///   `core::intrinsics` FMA intrinsics (`fmaf32`, `fmuladdf64`, ...).
    /// - **Fast-math / algebraic intrinsics**: `fadd_fast`, `fmul_fast`,
    ///   `fdiv_algebraic`, ... — LLVM is licensed to reassociate/contract these.
    /// - **Transcendentals**: `sin`, `cos`, `tan`, `asin`, `acos`, `atan`,
    ///   `atan2`, `sin_cos`, the hyperbolics, `exp`, `exp2`, `exp_m1`, `ln`,
    ///   `ln_1p`, `log`, `log2`, `log10`, `cbrt`, `hypot`, `powf`, `powi` — and
    ///   their `core::intrinsics` equivalents.
    /// `sqrt` is **allowed**: IEEE-754 mandates a correctly-rounded result, so
    /// `sqrtf` is bit-identical on wasm32, SSE2, and NEON. The arithmetic the
    /// step path is restricted to is `{+, -, *, /, sqrt, min, max}`.
    /// ### Why is this bad?
    /// SPEC-10 §17.6 requires Axiom's simulation to produce **byte-identical**
    /// results on every target so a recorded tick replays the same on a server,
    /// a desktop, and a browser. The `{+,-,*,/,sqrt,min,max}` subset is the
    /// portion of IEEE-754 that wasm32 and the native SSE2/NEON backends round
    /// identically, and that Rust/LLVM never silently fuses without fast-math.
    /// `mul_add` contracts a multiply and an add into one rounding step (a
    /// *different* result from `a * b + c`) and is emitted as a hardware FMA on
    /// some targets and a software polyfill on others; the transcendentals are
    /// served by each platform's own `libm`, which is not bit-identical. Any of
    /// them in the step path is a latent cross-target desync.
    /// This is the float-determinism analogue of `engine_no_time_in_sim`: the
    /// same `#[sim]` zone that may not read the wall clock may not perform an
    /// unportable float op. Authoring-time trig (e.g. `Quat::from_axis_angle`'s
    /// `sin`/`cos` in `axiom-math`, mesh generation in `axiom-resources`, easing
    /// curves in `axiom-tween`) is **out of scope** — it runs once at setup, not
    /// per step, and is not on the replayed path.
    /// ### Example
    /// ```rust
    /// #[axiom_zones::sim]
    /// fn integrate(p: f32, v: f32, a: f32, dt: f32) -> f32 {
    ///     v.mul_add(dt, p) // fused — not bit-identical across targets
    /// }
    /// ```
    /// Use instead:
    /// ```rust
    /// #[axiom_zones::sim]
    /// fn integrate(p: f32, v: f32, a: f32, dt: f32) -> f32 {
    ///     p + v * dt // explicit multiply-then-add: one portable rounding each
    /// }
    /// ```
    pub ENGINE_NO_UNPORTABLE_FLOAT,
    Warn,
    "FMA / fast-math / transcendental float op in the deterministic step path"
}

/// The `axiom-<name>` crate directories whose **entire** non-test source is the
/// deterministic per-step spine, where unportable float ops are banned even
/// without a `#[sim]` marker. `axiom-physics` is wholly a step path (it carries
/// no authoring-time trig), so the whole module is held to the invariant; the
/// generalizable form is the `#[sim]` zone, checked alongside this.
const STEP_PATH_CRATES: &[&str] = &["axiom-physics"];

/// Float methods that are *not* bit-reproducible across targets. `mul_add`
/// fuses to a single rounding (an FMA on some targets, a polyfill on others);
/// the rest are `libm` transcendentals. `sqrt` is deliberately absent — it is
/// IEEE-correctly-rounded everywhere — as are the portable shape ops (`abs`,
/// `floor`, `recip`, `min`, `max`, `copysign`, `clamp`, `to_bits`, ...).
const BANNED_METHODS: &[&str] = &[
    "mul_add", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sin_cos", "sinh", "cosh",
    "tanh", "asinh", "acosh", "atanh", "exp", "exp2", "exp_m1", "ln", "ln_1p", "log", "log2",
    "log10", "cbrt", "hypot", "powf", "powi",
];

/// `core::intrinsics` symbols banned in the step path: the fused multiply-add
/// family, the fast-math (`*_fast`) and algebraic (`*_algebraic`) intrinsics
/// LLVM may reassociate/contract, and the transcendental intrinsics. The
/// `sqrtf32`/`sqrtf64` intrinsics are intentionally absent (sqrt is portable).
const BANNED_INTRINSICS: &[&str] = &[
    "fmaf16",
    "fmaf32",
    "fmaf64",
    "fmaf128",
    "fmuladdf16",
    "fmuladdf32",
    "fmuladdf64",
    "fmuladdf128",
    "fadd_fast",
    "fsub_fast",
    "fmul_fast",
    "fdiv_fast",
    "frem_fast",
    "fadd_algebraic",
    "fsub_algebraic",
    "fmul_algebraic",
    "fdiv_algebraic",
    "frem_algebraic",
    "sinf32",
    "sinf64",
    "cosf32",
    "cosf64",
    "tanf32",
    "tanf64",
    "expf32",
    "expf64",
    "exp2f32",
    "exp2f64",
    "logf32",
    "logf64",
    "log2f32",
    "log2f64",
    "log10f32",
    "log10f64",
    "powif32",
    "powif64",
    "powf32",
    "powf64",
];

/// Is this `Call` expression a banned free-function/UFCS form — a `core`
/// intrinsic in [`BANNED_INTRINSICS`], or a UFCS float call such as
/// `f32::sin(x)` / `f64::mul_add(a, b, c)`?
fn banned_call_path(path: &str) -> bool {
    let last = path.rsplit("::").next().unwrap_or(path);
    let is_intrinsic = path.contains("intrinsics") && BANNED_INTRINSICS.contains(&last);
    let is_float_ufcs = (path.contains("f32") | path.contains("f64"))
        & BANNED_METHODS.contains(&last);
    is_intrinsic | is_float_ufcs
}

/// The precise span to blame for `expr`, if it is a banned float op:
/// - a `MethodCall` whose method is in [`BANNED_METHODS`] **and** whose receiver
///   is an `f32`/`f64` (so a logger's `.log(...)` or a user `.sin()` on a
///   non-float type never trips) — blamed at the method name;
/// - a `Call` to a banned intrinsic or UFCS float function — blamed at the call.
fn banned_span<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) -> Option<Span> {
    match expr.kind {
        ExprKind::MethodCall(seg, receiver, _, _) => {
            let named = BANNED_METHODS.contains(&seg.ident.name.as_str());
            let recv_is_float =
                matches!(cx.typeck_results().expr_ty(receiver).peel_refs().kind(), ty::Float(_));
            (named & recv_is_float).then_some(seg.ident.span)
        }
        ExprKind::Call(callee, _) => match callee.kind {
            ExprKind::Path(ref qpath) => cx
                .qpath_res(qpath, callee.hir_id)
                .opt_def_id()
                .map(|def_id| cx.tcx.def_path_str(def_id))
                .filter(|path| banned_call_path(path))
                .map(|_| expr.span),
            _ => None,
        },
        _ => None,
    }
}

impl<'tcx> LateLintPass<'tcx> for EngineNoUnportableFloat {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let Some(span) = banned_span(cx, expr) else {
            return;
        };
        if expr.span.from_expansion() {
            return;
        }
        if is_in_test(cx.tcx, expr.hir_id) {
            return;
        }
        if !is_engine_file(cx, expr.span) {
            return;
        }
        let in_step_path = is_in_crate_dir(cx, expr.span, STEP_PATH_CRATES)
            | in_zone(cx, expr.hir_id, markers::SIM);
        if !in_step_path {
            return;
        }
        span_lint_and_help(
            cx,
            ENGINE_NO_UNPORTABLE_FLOAT,
            span,
            "this float op is not bit-reproducible across targets; it is banned in the deterministic step path",
            None,
            "use only `{+, -, *, /, sqrt, min, max}` here (e.g. `a * b + c` instead of `a.mul_add(b, c)`); \
             precompute any trig at authoring time outside the step",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
