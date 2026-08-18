# UI (HUD) port notes

Ported `C:/dev/Claude-of-Duty/src/ui/` (3,818 lines across 15 files) into
`apps/claude-of-duty/src/ui/` as a new module tree, wired via one
`pub mod ui;` line in `src/lib.rs`.

## What was ported

| this crate           | source              | lines |
|-----------------------|---------------------|-------|
| `ui::util`             | `util.js`           | 215   |
| `ui::style`            | `style.js`          | 716   |
| `ui::crosshair`        | `crosshair.js`      | 104   |
| `ui::hitmarkers`       | `hitmarkers.js`     | 125   |
| `ui::damage`           | `damage.js`         | 100   |
| `ui::health`           | `health.js`         | 178   |
| `ui::ammo`             | `ammo.js`           | 199   |
| `ui::killfeed`         | `killfeed.js`       | 95    |
| `ui::compass`          | `compass.js`        | 117   |
| `ui::prompts`          | `prompts.js`        | 94    |
| `ui::menu`             | `menu.js`           | 199   |
| `ui::markers`          | `markers.js`        | 263   |
| `ui` (`mod.rs`)        | `index.js`          | 613   |

All twelve widget files plus `index.js`'s `UiSystem` are represented. Style
is ported to `style.rs` (the `--k` scale derivation, natively tested) +
`style.css.tpl` (the literal CSS/DEFS text, loaded via `include_str!` and
runtime-substituted for the three font-stack interpolations, since `format!`
requires a literal template).

## The split every module follows

Every widget is **pure state + `update()` math**, natively testable with no
DOM, plus a `#[cfg(target_arch = "wasm32")] pub mod view` that owns the real
`web_sys::Element`s and paints the computed frame into `style`/attributes.
This is the same split `src/audio/` uses between its recorded `graph` and its
`web_audio` realiser. `update()` never touches a node; `view::*::apply()`
never computes a curve. 99 tests, all pure-side, all passing natively; the
wasm arm additionally compiles clean under
`cargo build --target wasm32-unknown-unknown` (not run by the recipe's gate
list, but checked here since a large fraction of this port's code is
wasm-only and would otherwise ship unverified — it caught one real bug, see
below).

`Pool<N>` in `util.rs` generalises the source's `Pool` class: the oldest-first
reuse policy is pure and generic over the node payload `N` (native tests use
`Pool<()>`; a wasm view uses `Pool<Element>` or a small element-bundle
struct). `d`/`s`, two of the source's five per-slot scratch fields, are
dropped — nothing outside `minimap.js` (deferred, see below) reads them.

## Deferred

- **`minimap.js` (603 lines), per the task brief.** It bakes an orthographic
  depth render target of the level once at load and reads it back to draw a
  top-down minimap. No render target / depth bake / readback exists anywhere
  in this port yet. `style.css.tpl` keeps every `.ow-minimap*` rule,
  unreferenced, so a future minimap port needs no CSS changes. The one
  consumer of `ui.setBlips()`'s AI-actor list is the minimap; `Blip`/
  `FramePull::blips` are carried forward as a data shape with no current
  consumer.
- **`demo.js` (198 lines)** is a scripted combat timeline for
  screenshot/critic capture (`UiSystem.debugState('combat')`), not part of
  the HUD itself. Left for whichever slice ports the capture tooling.
- **`UiSystem` as a real `crate::registry::Subsystem`.** The source's
  `index.js` reads `ctx.camera`, `ctx.canvas`, `ctx.input`, and pulls
  duck-typed state off `ctx.peek('weapons')`/`ctx.peek('player')`/
  `ctx.peek('ai')`. None of those exist on `crate::engine::Ctx` yet — camera,
  input, weapons, player and ai are other agents' concurrent slices. Rather
  than block on that or invent throwaway placeholder subsystems, `ui::Hud`
  (in `mod.rs`) is the source's `UiSystem` minus the `Subsystem` impl: every
  value the source pulls from another subsystem is an explicit, optional
  parameter to `Hud::late_update` (`FramePull`, `CameraBasis`). When those
  subsystems land, wiring `Hud` behind a real `Subsystem` that reads `ctx`
  and calls `late_update` is a thin adapter, not a redesign. This mirrors two
  precedents already in the file: `markers::ScreenProjector` (a narrow trait
  for the camera projection markers.js needs, so a future camera binding
  slots in without touching `project()`) and `menu::MenuHost` (a narrow trait
  for the four subsystem effects `menu.js`'s `show()`/`close()` reach for:
  freezing time, disabling player control, and pointer lock).
- **`PauseMenu`'s click wiring.** The slider/button DOM elements are built in
  `menu::view::MenuView`, but no `Closure`-based event listeners are attached
  yet (would need an input-event story this slice doesn't own). `Config` and
  `EventBus` — both already real, ported crate.-level types — are wired for
  real: `PauseMenu::set_quality`/`set_sensitivity_multiplier`/`set_fov`/
  `set_invert_y` mutate a `&mut Config` and emit the exact five events the
  source does (`ui:pause`, `ui:quality`, `ui:sensitivity`, `ui:fov`,
  `ui:setting`), pinned by tests using a real `EventBus` subscriber.
- **`ammo::AmmoPanel::_fitName`'s DOM measurement** (`scrollWidth`/
  `clientWidth`) has no pure equivalent — it stays in `ammo::view::AmmoView`,
  the only place that can ask a real element how wide its text ran.

## Verification

99 new tests in `ui::*`, all native, all passing (`cargo test -p
axiom-claude-of-duty` — 153 unit tests total in the lib, plus the existing
integration test files). Covers, per the task's explicit list:

- **`--k` scale derivation** (`style::scale_factor`): exact at 1080p, clamped
  at both ends (0.62 / 2.4), linear in between (`style.rs` tests).
- **dt-integrated animation channels given a fixed dt sequence**: the
  crosshair's spring-kick settling and ADS fade (`crosshair.rs`), the
  health vitals' heartbeat/regen/flash channels (`health.rs`), the banner's
  punch-in/hold/fade curve (`prompts.rs`), the ammo counter's fire-punch decay
  (`ammo.rs`) — each driven through tens to hundreds of fixed `1/60`s steps
  and asserted at settle.
- **marker projection/clamping given fixed camera and world positions**
  (`markers.rs`): a hand-built `FixedCamera` (a real `axiom_math::Mat4`
  view-projection) checks dead-ahead projects to screen centre, a point
  behind the camera mirrors through centre, off-screen points clamp into the
  margin rectangle, and distance is Euclidean from the eye.
- **killfeed/hitmarker timing** (`killfeed.rs`, `hitmarkers.rs`): fade-in
  (`outQuint`) / hold / fade-out (`inQuad`) shape at fixed `t` offsets, pool
  reuse (6/10-slot oldest-first), and release-on-expiry.

No values were captured from the original JavaScript via a Node script (the
recipe's golden-capture method) for this slice: the source's UI classes call
`document.createElement` in their constructors, and the source repo has no
`jsdom` dependency to satisfy that outside a real browser. Every curve is
instead pinned against its own closed-form shape (asserting the exact
`ease.*` formula's known values, e.g. `outBack(0)=0`, `outElastic` clamps
outside `[0,1]`) and against settle/boundary behaviour computed by hand from
the ported formula — the same rigor as a golden, without needing a browser
stub. If a shared minimal DOM stub is ever added for another slice's
Node-based captures, this module's tests are a natural target to upgrade to
byte-exact captured goldens.

## One real bug the wasm build caught

`style::install::install_styles` originally called `doc.head()` /
`doc.body()` without the `web_sys` `HtmlHeadElement`/`HtmlBodyElement`
features enabled — `web-sys` gates a method by its *return type's* feature,
not just the receiver's, so this compiled fine natively (the DOM module is
`cfg(wasm32)`-only and never built by `cargo test`) but failed under
`cargo build --target wasm32-unknown-unknown`. Fixed by adding
`HtmlHeadElement`/`HtmlBodyElement` (and `HtmlInputElement`, for the menu's
range sliders) to `Cargo.toml`'s `web-sys` feature list.

## Divergences from the source, and why

- **`Pool` drops the `d`/`s` scratch fields** (see above) — dead weight with
  no reader outside the deferred `minimap.js`.
- **`ease::punch` and a few other curves are ported as free functions in a
  `pub mod ease`, not a JS object literal** — the natural Rust shape for a
  fixed table of pure functions; call sites read identically
  (`ease::out_quad(t)` vs `ease.outQuad(t)`).
- **`hitmarkers::HitKind` is a 4-variant enum, not a string key into a map.**
  The source's `KINDS[kind] ?? KINDS.hit` fallback-on-unknown-string becomes
  unrepresentable: the type system only admits the four real kinds, so the
  fallback path is structurally unreachable rather than dead code kept
  reachable for fidelity.
