//! **The HUD, mounted.** `ui/index.js`'s `UiSystem`, constructed, subscribed,
//! stepped, and — on `wasm32` — painted onto the page.
//!
//! # The defect this file closes
//!
//! `apps/shmup/src/ui/` is 8,540 lines: eleven widgets, each with a pure core
//! and a `wasm32` `view`, plus [`crate::ui::system::UiCore`], the facade that
//! owns all eleven. Before this file, `UiCore` had **zero references outside
//! its own file**, every `view` module had zero, and what actually ran was
//! `ui::Hud` — a second, thinner port of the same `index.js` with no view at
//! all. The comment above its per-frame call said so outright: *"The HUD model
//! is advanced every frame whether or not a view is mounted."* None was.
//!
//! `Hud` is now deleted (see [`crate::ui`]'s module docs). This is the one HUD.
//!
//! # What this file is, and what it is not
//!
//! It is **wiring**: a constructor, four setters, a frame drive, and a mount.
//! It decides nothing. Every number it hands the DOM was computed by
//! [`UiCore::late_update`] and is pinned by that module's own tests.
//!
//! The three seams `UiCore`'s docs name are closed here, and only here:
//!
//! | seam | what fills it |
//! |---|---|
//! | `ctx.peek('weapons' / 'player' / 'ai')` | [`HudPull::weapon`], [`HudPull::player`], [`HudPull::actors`] |
//! | `ctx.camera` / `ctx.input` | [`HudPull::pose`] + [`HudPull::aspect`], [`HudPull::input`] |
//! | `ctx.time` | [`HudPull::clock`] |
//!
//! # Why the views are driven from the effect journal
//!
//! Six of the eleven widget views are frame-driven — hand them the matching
//! field of [`UiFrame`] and they paint it. The other five
//! (`hitmarkers`, `damage`, `killfeed`, `markers`, and `compass`) are
//! **self-driving**: each owns its own copy of the widget's pure core,
//! parameterised over its DOM node type (`Hitmarkers<HitNode>` where the facade
//! holds `Hitmarkers<()>`), and exposes `spawn`/`push`/`update(dt)` rather than
//! an `apply`. That is the shape the port authored, and
//! [`crate::ui::system::UiEffect`] is the channel it authored for driving it:
//! its own docs say the journal carries "the three things a numeric widget
//! frame cannot: the killfeed row's names, the banner's strings, and the damage
//! number's value and kind — all of which the `wasm32` view writes as text".
//!
//! So [`view::HudViews::apply`] replays the journal's spawns and then steps each
//! self-driving view with the same `dt`. The two pools stay in lockstep because
//! they are the same code over the same sequence: the spawns all originate in
//! event handlers or API calls, which run *before* `late_update`, so replaying
//! them before the view's `update(dt)` reproduces the facade's own order
//! exactly. `Pool::acquire` is oldest-first and deterministic, so slot `i` on
//! one side is slot `i` on the other — which the journal's `slot` field also
//! states, and which is asserted below.
//!
//! It is still two state machines where one would do, and that is recorded as a
//! finding in `docs/work-manifests/shmup-port/notes/hud.md` rather than fixed
//! here: making the five views frame-driven means changing five `view` modules'
//! public shape, and this wave connects what exists rather than reshaping it.
//!
//! # RNG position is load-bearing
//!
//! [`UiCore::new`] spends two forks of the stream it is handed, in order:
//! `WorldMarkers`'s (`index.js:82`) and the minimap's (`:86`). Construct
//! [`HudRig`] anywhere but the `ui` slot of `Game::new`'s sequence and every
//! later draw in the level moves. See `crate::scene::wiring`'s module docs.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_math::Mat4;

use crate::events::EventBus;
use crate::input::Input;
use crate::rng::Rng;
use crate::scene::game::CameraPose;
use crate::scene::wiring::ai::{camera_state, ActorPose};
use crate::ui::minimap::{FootprintSpec, LayoutSource, Minimap};
use crate::ui::system::{
    CameraState, FrameLinks, HudActor, PlayerLink, UiClock, UiCore, UiEffect, UiFrame, UiInput,
    UiSystem,
};
use crate::ui::{PlayerPull, WeaponPull};
use crate::world::system::WorldSystem;

/// `document.getElementById('ui') ?? document.body` (`index.js:72`). The page
/// is expected to carry an empty `<div id="ui">`; the body is the source's own
/// fallback and is kept.
pub const HOST_ID: &str = "ui";

/// The viewport the HUD sizes itself to until someone calls
/// [`HudRig::resize`] — `index.js:129-130`'s `vw`/`vh`.
pub const DEFAULT_VIEWPORT: (f64, f64) = (1920.0, 1080.0);

/// Everything one HUD frame needs that the HUD does not own.
///
/// Every field is one of the `ctx` reaches `index.js` makes and this port
/// cannot: there is no registry behind `Game`, so the values arrive as
/// parameters instead of through `ctx.peek`.
pub struct HudPull<'a> {
    /// The scaled step the rest of the frame ran at (`ctx.time.dt`).
    pub dt: f64,
    /// `ctx.time` — `raw` is unscaled wall-clock seconds since start, `elapsed`
    /// is scaled. `UiCore` derives `rawDt` from the former itself.
    pub clock: UiClock,
    /// The camera pose this frame resolved to, already final.
    pub pose: CameraPose,
    /// The canvas aspect. Only the projection uses it, and only the world
    /// markers project.
    pub aspect: f64,
    pub input: &'a Input,
    /// `weapons.getHudState()`. `None` means "no weapons subsystem", which is
    /// the arm where the HUD counts its own rounds.
    pub weapon: Option<WeaponPull>,
    /// `player.getHudState()`.
    pub player: PlayerPull,
    /// `player.position` — the arc/compass/minimap origin, at `f64`, which is
    /// the width the facade does its bearing arithmetic in.
    pub player_position: [f64; 3],
    /// `ai.getHudActors()` — every soldier, alive or not; the facade filters.
    pub actors: &'a [ActorPose],
}

/// A constructed, subscribed, mountable HUD.
pub struct HudRig {
    /// Holds the seven event subscriptions.
    system: UiSystem,
    /// The same guts `system` holds, so the frame drive does not go through a
    /// `Registry` this app has no instance of.
    core: Rc<RefCell<UiCore>>,
    /// `index.js:86`'s widget. It lives beside the facade rather than inside
    /// it because the facade only owns the *gate* — it emits
    /// [`UiEffect::MinimapTryBake`]/[`UiEffect::MinimapDraw`] and the host
    /// realises them.
    minimap: Minimap,
    /// `sfx(id, gain)` calls this frame, for whoever owns the audio graph.
    sfx: Vec<(&'static str, f64)>,
    /// Whether [`UiCore::init`] has run. It needs the player position and the
    /// raw clock (`index.js:147`, `:258`) and neither exists at
    /// [`HudRig::new`], so it is deferred to the first [`HudRig::frame`] —
    /// which is the first moment a [`HudPull`] carries both.
    initialised: bool,
    vw: f64,
    vh: f64,
    dpr: f64,
    /// `WorldMarkers`'s fork, held until [`HudRig::mount`] hands it to the
    /// view's own `WorldMarkers`. See the module docs.
    #[cfg(target_arch = "wasm32")]
    markers_rng: Option<Rng>,
    #[cfg(target_arch = "wasm32")]
    views: Option<view::HudViews>,
}

impl HudRig {
    /// Construct the HUD. `rng` is `ctx.rng.fork()` — the `ui` slot.
    pub fn new(rng: Rng) -> HudRig {
        // `UiCore::new` forks twice out of this stream. The `wasm32` view owns
        // a SECOND `WorldMarkers` (its pool's payload is the DOM nodes), and it
        // must draw from the SAME fork or its damage-number jitter diverges
        // from the facade's. Cloning before the move reproduces that fork
        // exactly rather than inventing a parallel stream.
        #[cfg(target_arch = "wasm32")]
        let markers_rng = rng.clone().fork();
        let mut system = UiSystem::new(rng);
        let core = system.core();
        let minimap_rng = core
            .borrow_mut()
            .take_minimap_rng()
            .expect("a freshly built UiCore still holds the minimap fork");
        let dpr = device_pixel_ratio();
        let mut rig = HudRig {
            system,
            core,
            minimap: Minimap::new(minimap_rng, dpr),
            sfx: Vec::new(),
            initialised: false,
            vw: DEFAULT_VIEWPORT.0,
            vh: DEFAULT_VIEWPORT.1,
            dpr,
            #[cfg(target_arch = "wasm32")]
            markers_rng: Some(markers_rng),
            #[cfg(target_arch = "wasm32")]
            views: None,
        };
        rig.resize(DEFAULT_VIEWPORT.0, DEFAULT_VIEWPORT.1);
        rig
    }

    /// The seven `ctx.events.on(...)` subscriptions (`index.js:152-241`).
    ///
    /// Separate from [`HudRig::new`] because the bus lives on `Game`, which
    /// does not exist until after every subsystem in it has been constructed.
    pub fn wire(&mut self, events: &EventBus) {
        self.system.wire_events_on(events);
    }

    /// The facade, for a caller that needs to reach past the frame — the
    /// killfeed/banner/objective/blip API, `state`, `debug_state`.
    pub fn core(&self) -> Rc<RefCell<UiCore>> {
        Rc::clone(&self.core)
    }

    /// `resize(w, h, ctx)` (`index.js:584-592`), including the two the facade
    /// documents as the host's job: the `--k` custom property and the
    /// minimap's backing store.
    pub fn resize(&mut self, w: f64, h: f64) {
        self.vw = w;
        self.vh = h;
        let k = {
            let mut core = self.core.borrow_mut();
            core.resize(w, h);
            core.k
        };
        self.minimap.resize(k, self.dpr);
        self.resize_views(k);
    }

    #[cfg(target_arch = "wasm32")]
    fn resize_views(&mut self, k: f64) {
        let minimap = &self.minimap;
        if let Some(views) = self.views.as_mut() {
            views.resize(k, minimap);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn resize_views(&mut self, _k: f64) {}

    /// `this.menu.toggle()` — the Escape edge, forwarded by whoever owns the
    /// pause decision.
    ///
    /// [`UiInput::pause_pressed`] is deliberately left `false` in
    /// [`ui_input`]: `Game::handle_pause` already reads the key, and letting
    /// `UiCore` read it too would toggle the menu twice per press.
    pub fn toggle_menu(&mut self, events: &EventBus) {
        let open = self.core.borrow().menu.open;
        let mut core = self.core.borrow_mut();
        match open {
            true => core.resume(events),
            false => core.pause(events),
        }
    }

    /// The viewport the HUD last sized itself to — `this.vw`/`this.vh`. The
    /// world markers project into it, so it is the canvas's CSS pixel size, not
    /// its backing-store size.
    pub fn viewport(&self) -> (f64, f64) {
        (self.vw, self.vh)
    }

    /// Whether the pause menu is showing. [`UiCore`] is the single owner of
    /// that bit; anything else that tracks a "paused" flag mirrors this.
    pub fn menu_open(&self) -> bool {
        self.core.borrow().menu.open
    }

    /// `sfx(id, gain)` (`index.js:277-287`) — the HUD's fire-and-forget calls
    /// into the audio subsystem, drained in the order they were made. The ids
    /// are the ones `audio/index.js`'s `UI_ALIAS` table resolves.
    pub fn take_sfx(&mut self) -> Vec<(&'static str, f64)> {
        std::mem::take(&mut self.sfx)
    }

    /// Highlight the live graphics preset and invert-look setting in the pause
    /// menu (`menu.js`'s `_sync`). Idempotent; call it whenever either changes.
    #[cfg(target_arch = "wasm32")]
    pub fn sync_menu(&self, quality_index: usize, invert_y: bool) {
        if let Some(views) = self.views.as_ref() {
            views.menu.sync(quality_index, invert_y);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn sync_menu(&self, _quality_index: usize, _invert_y: bool) {}

    /// Build the overlay and every widget's DOM under the page's `#ui` host
    /// (or the body), and inject `style.css.tpl`. `wasm32` only, and safe to
    /// call exactly once.
    #[cfg(target_arch = "wasm32")]
    pub fn mount(&mut self) {
        use wasm_bindgen::JsCast;

        let document = web_sys::window()
            .expect("a browser window")
            .document()
            .expect("a document");
        let host: web_sys::Element = document
            .get_element_by_id(HOST_ID)
            .unwrap_or_else(|| document.body().expect("a document body").unchecked_into());
        let rng = self
            .markers_rng
            .take()
            .expect("HudRig::mount is called exactly once");
        self.views = Some(view::HudViews::new(&host, rng));
        let (w, h) = (self.vw, self.vh);
        self.resize(w, h);
    }

    /// `lateUpdate(dt, ctx)` (`index.js:401-546`) with its four `ctx` reads
    /// pushed in first, then — on `wasm32` — painted.
    ///
    /// Call it after the camera has reached its final transform for the frame.
    /// That ordering is the entire reason the source uses `lateUpdate`
    /// (`index.js:24-25`): the damage arcs, the compass and the world markers
    /// all read the camera basis, and reading it mid-`update` aims them at last
    /// frame's view.
    pub fn frame(&mut self, pull: HudPull<'_>, events: &EventBus) -> UiFrame {
        let camera = ui_camera(pull.pose, pull.aspect);
        let actors: Vec<HudActor> = pull.actors.iter().map(hud_actor).collect();
        // `init(ctx)`'s two seeds (`index.js:147`, `:258`). Run here rather
        // than in `new` because both values arrive with the frame; see
        // [`UiCore::init`] for what an unseeded first frame costs.
        //
        // `raw - dt`, not `raw`: in the source `init` runs a whole frame
        // before the first `lateUpdate`, so that first `rawDt` is one frame,
        // not zero. Seeding it to `raw` exactly would make frame one's
        // `rawDt` zero, and the movement bloom divides by it.
        if !self.initialised {
            self.initialised = true;
            self.core.borrow_mut().init(
                self.vw,
                self.vh,
                pull.player_position,
                pull.clock.raw - pull.dt,
            );
        }
        {
            let mut core = self.core.borrow_mut();
            core.set_clock(pull.clock);
            core.set_camera(camera);
            core.set_input(ui_input(pull.input));
            core.set_links(FrameLinks {
                weapon: pull.weapon,
                player: Some(PlayerLink {
                    hud: Some(pull.player),
                    health: None,
                    position: Some(pull.player_position),
                }),
                ai: Some(actors),
            });
        }
        let frame = self.core.borrow_mut().late_update(pull.dt, events);

        self.sfx
            .extend(frame.effects.iter().filter_map(|e| match e {
                UiEffect::Sfx { id, gain } => Some((*id, *gain)),
                _ => None,
            }));
        self.run_bake_gate(&frame);
        self.paint(&frame, pull.dt, &camera);
        frame
    }

    /// Hand the frame to the DOM. The whole browser edge is behind this one
    /// call, so the native test path — which has no `document` — steps the
    /// identical model and simply paints nothing.
    #[cfg(target_arch = "wasm32")]
    fn paint(&mut self, frame: &UiFrame, dt: f64, camera: &CameraState) {
        let (minimap, core) = (&self.minimap, &self.core);
        if let Some(views) = self.views.as_mut() {
            views.apply(&core.borrow(), frame, dt, camera, minimap);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn paint(&mut self, _frame: &UiFrame, _dt: f64, _camera: &CameraState) {}

    /// `if (!minimap.bakeDone && ...) minimap.tryBake(ctx)` (`index.js:524`).
    ///
    /// Both of `try_bake`'s inputs are `None`, and that is not a stub — it is
    /// the state of the port. It is also **not** blocked on the render work
    /// the manifests recorded: the orthographic depth bake is `tryBake`'s
    /// *fallback* (`minimap.js:74-76`), and the primary path,
    /// `_buildVectorMap`, is pure CPU. [`Minimap::build_vector_map`] is
    /// ported in full and only wants a [`LayoutSource`] — `world.buildings`,
    /// `levelToWorld`, `isOpen` — which [`WorldLayout`], just above, already
    /// adapts out of [`WorldSystem`].
    ///
    /// What is missing is one field one tier up: `scene::level::build_level`
    /// keeps the `WorldSystem`'s *products* and drops the system, so nothing
    /// reachable from a running `Game` can hand one over. Closing it takes
    /// three edits, none of them in this file:
    ///
    /// 1. `scene/level.rs` — keep the system on `Level` (`pub world_system:
    ///    WorldSystem`) instead of dropping it.
    /// 2. this file — give `run_bake_gate` the layout, and pass
    ///    `Some(&WorldLayout(&level.world_system))` into `try_bake`.
    /// 3. `ui/minimap.rs`'s `view` — make `DrawOp::DrawBaked` non-inert. The
    ///    source rasterises `_buildVectorMap` into an **offscreen canvas**
    ///    once (`minimap.js:204-278`) and `drawImage`s it every frame;
    ///    [`crate::ui::minimap::Baked::Vector`] is that display list, so the
    ///    view has to replay it into a `BAKE`x`BAKE` canvas on the first frame
    ///    it appears and blit from it thereafter.
    ///
    /// Until then the minimap draws its procedural plate — grid, view cone,
    /// blips, objectives — and no building footprints.
    fn run_bake_gate(&mut self, frame: &UiFrame) {
        if frame.minimap_bake_requested {
            self.minimap.try_bake(None, None);
        }
        let done = self.minimap.bake_done;
        self.core.borrow_mut().set_minimap_bake_done(done);
    }
}

/* ==================================================================== */
/* Seam adapters                                                        */
/* ==================================================================== */

/// `ctx.peek('world')` as the minimap's vector bake reads it
/// (`minimap.js:184-201`, `:234`), over the world subsystem.
///
/// A newtype rather than `impl LayoutSource for WorldSystem` on purpose:
/// [`crate::world::system::WorldSystem`] already has *inherent* methods named
/// `level_to_world` and `is_open`, and implementing the trait on it directly
/// would put two same-named methods in scope at every call site inside the
/// impl. Wrapping means `self.0` is a plain `&WorldSystem`, which does not
/// implement the trait, so each call unambiguously reaches the inherent
/// method it is meant to forward to.
///
/// **This has no call site yet, and cannot have one from inside this file.**
/// See [`HudRig::run_bake_gate`] for the one field that is missing a tier up.
pub struct WorldLayout<'a>(pub &'a WorldSystem);

impl LayoutSource for WorldLayout<'_> {
    /// `world.buildings[i].spec` — `BuildingInfo::building` is that spec, and
    /// supplies all five fields, so none of `FootprintSpec`'s `??` fallbacks
    /// fire from this source.
    fn buildings(&self) -> Vec<FootprintSpec> {
        self.0
            .buildings
            .iter()
            .map(|info| FootprintSpec {
                x: Some(info.building.x),
                z: Some(info.building.z),
                w: Some(info.building.w),
                d: Some(info.building.d),
                floors: Some(f64::from(info.building.floors)),
            })
            .collect()
    }

    /// `world.levelToWorld(x, y, z, out)` — only `.x` and `.z` are read.
    fn level_to_world(&self, x: f64, y: f64, z: f64) -> (f64, f64) {
        let p = self.0.level_to_world(x, y, z);
        (p.x, p.z)
    }

    /// `world.isOpen(x, z, margin)`, called with `margin = 0` throughout.
    fn is_open(&self, x: f64, z: f64, margin: f64) -> bool {
        self.0.is_open(x, z, margin)
    }
}

/// `ctx.camera`, as the HUD reads it.
///
/// [`crate::scene::wiring::ai::camera_state`] already builds the source's
/// camera from a [`CameraPose`] — the same `YXZ` quaternion, the same
/// `Matrix4.compose`, the same `makePerspective` — so this reuses it rather
/// than composing a second, subtly different one. Its `CAMERA_NEAR`/`FAR` are
/// the source camera's own; the only thing the HUD reads the projection for is
/// the world markers' `behind` test (`ndc_z > 1`), which the clip planes do not
/// move.
fn ui_camera(pose: CameraPose, aspect: f64) -> CameraState {
    let c = camera_state(pose, aspect);
    CameraState {
        matrix_world: c.matrix_world,
        position: pose.eye,
        fov: pose.fov_degrees,
        // `projectionMatrix * matrixWorldInverse`, which is what
        // `Vector3.project(camera)` applies.
        view_projection: narrow_mat4(c.projection_matrix).multiply(narrow_mat4(c.matrix_world_inverse)),
    }
}

/// A column-major `[f64; 16]` as the `f32` [`Mat4`]
/// [`crate::ui::markers::ScreenProjector`] is written against. The narrowing is
/// the projector's own contract, not a choice made here.
fn narrow_mat4(e: [f64; 16]) -> Mat4 {
    Mat4::from_cols_array(std::array::from_fn(|i| e[i] as f32))
}

/// `ctx.input`, as much of it as the HUD reads (`index.js:409-416`, `462`).
fn ui_input(input: &Input) -> UiInput {
    UiInput {
        enabled: input.enabled,
        frozen: input.frozen,
        pointer_locked: input.pointer_locked,
        // See `HudRig::toggle_menu` — the pause edge arrives through the
        // facade's own API, not through this flag, so the menu toggles once.
        pause_pressed: false,
        ads: input.ads(),
    }
}

/// One `ai.getHudActors()` element (`index.js:551-562`).
///
/// `friendly` is hard `false`: every actor `ai/index.js` spawns in this level
/// is a hostile, and [`ActorPose`] publishes no team at all. When a friendly
/// garrison lands, this is the one line that changes.
fn hud_actor(a: &ActorPose) -> HudActor {
    HudActor {
        position: Some(a.position),
        alive: Some(a.alive),
        dead: None,
        friendly: false,
        // The source prefers `a.heading` (degrees) and falls back to
        // `(a.yaw * 180) / PI`. `ActorPose` publishes yaw, in radians.
        heading: None,
        yaw: Some(a.yaw),
    }
}

#[cfg(target_arch = "wasm32")]
fn device_pixel_ratio() -> f64 {
    web_sys::window().map_or(1.0, |w| w.device_pixel_ratio())
}

#[cfg(not(target_arch = "wasm32"))]
fn device_pixel_ratio() -> f64 {
    1.0
}

/* ==================================================================== */
/* The browser edge                                                     */
/* ==================================================================== */

/// Every widget's DOM, mounted in `init`'s order and painted in
/// `lateUpdate`'s.
///
/// The construction order (`index.js:81-93`) is not cosmetic: within one
/// stacking layer the DOM order *is* the z-order, so the minimap sits under the
/// compass, which sits under the killfeed, and so on.
#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use crate::rng::Rng;
    use crate::ui::ammo::view::AmmoView;
    use crate::ui::compass::view::{CompassView, MatchBarView};
    use crate::ui::crosshair::view::CrosshairView;
    use crate::ui::damage::view::DamageArcsView;
    use crate::ui::health::view::HealthView;
    use crate::ui::hitmarkers::view::HitmarkersView;
    use crate::ui::killfeed::view::KillfeedView;
    use crate::ui::markers::view::WorldMarkersView;
    use crate::ui::markers::Objective;
    use crate::ui::menu::view::MenuView;
    use crate::ui::minimap::view::MinimapView;
    use crate::ui::minimap::Minimap;
    use crate::ui::prompts::view::{BannerView, PromptView};
    use crate::ui::system::view::HudRoot;
    use crate::ui::system::{CameraState, DamageKind, UiCore, UiEffect, UiFrame};

    /// `o.color ?? 'var(--cyan)'` (`compass.js:74`).
    const OBJECTIVE_COLOUR: &str = "var(--cyan)";

    pub struct HudViews {
        root: HudRoot,
        health: HealthView,
        markers: WorldMarkersView,
        arcs: DamageArcsView,
        crosshair: CrosshairView,
        hit: HitmarkersView,
        minimap: MinimapView,
        compass: CompassView,
        match_bar: MatchBarView,
        killfeed: KillfeedView,
        ammo: AmmoView,
        prompt: PromptView,
        banner: BannerView,
        pub menu: MenuView,
    }

    impl HudViews {
        /// `installStyles()` + `index.js:72-93`, in the source's order.
        pub fn new(host: &Element, markers_rng: Rng) -> HudViews {
            let root = HudRoot::install(Some(host));
            let health = HealthView::new(&root.hurt_layer, &root.chrome_layer);
            let markers = WorldMarkersView::new(&root.world_layer, markers_rng);
            let arcs = DamageArcsView::new(&root.centre_layer);
            let crosshair = CrosshairView::new(&root.centre_layer);
            let hit = HitmarkersView::new(&root.centre_layer);
            let minimap = MinimapView::new(&root.chrome_layer);
            let compass = CompassView::new(&root.chrome_layer);
            let match_bar = MatchBarView::new(&root.chrome_layer);
            let killfeed = KillfeedView::new(&root.chrome_layer);
            let ammo = AmmoView::new(&root.chrome_layer);
            let prompt = PromptView::new(&root.chrome_layer);
            let banner = BannerView::new(&root.chrome_layer);
            let menu = MenuView::new(&root.root);
            HudViews {
                root,
                health,
                markers,
                arcs,
                crosshair,
                hit,
                minimap,
                compass,
                match_bar,
                killfeed,
                ammo,
                prompt,
                banner,
                menu,
            }
        }

        /// `index.js:588-591` — the scale the whole stylesheet is written
        /// against, plus the two widgets that cache it.
        pub fn resize(&mut self, k: f64, minimap: &Minimap) {
            self.root.set_scale(k);
            self.compass.set_scale(k);
            self.minimap.resize(minimap);
        }

        /// One frame: replay the journal, then paint.
        ///
        /// The journal comes first because every spawn in it happened *before*
        /// `late_update` ran (event handlers and API calls), so replaying it
        /// ahead of each self-driving view's `update(dt)` reproduces the
        /// facade's own pool ordering. See the module docs.
        pub fn apply(
            &mut self,
            core: &UiCore,
            frame: &UiFrame,
            dt: f64,
            camera: &CameraState,
            minimap: &Minimap,
        ) {
            frame.effects.iter().for_each(|e| self.replay(e));

            // `index.js:502-504`: the three layer opacities.
            self.root.apply(frame);

            self.crosshair.apply(&frame.crosshair);
            self.hit.update(dt);
            let b = frame.basis;
            self.arcs
                .update(dt, b.right_x, b.right_z, b.forward_x, b.forward_z);
            self.health.apply(&frame.health);
            self.ammo.apply(&core.ammo_input(), &frame.ammo);
            self.killfeed.update(dt);
            self.match_bar.apply(&frame.match_bar);
            self.prompt.apply(&frame.prompt);
            self.banner.apply(&frame.banner);

            // `_buildCompassObjectives(pos)` then `compass.update(...)`. The
            // view recomputes the tick from the bearing, so it takes bearings
            // — `UiFrame::objective_ticks` has already thrown them away.
            let pos = [frame.minimap.x, 0.0, frame.minimap.z];
            let bearings = core.compass_objectives(pos);
            let objectives: Vec<(f64, &str, &str)> = bearings
                .iter()
                .map(|(bearing, label, colour)| {
                    (
                        *bearing,
                        label.as_str(),
                        colour.as_deref().unwrap_or(OBJECTIVE_COLOUR),
                    )
                })
                .collect();
            self.compass.update(frame.heading_deg, &objectives);

            let positioned: Vec<Objective> = core
                .objectives()
                .iter()
                .filter_map(|o| {
                    o.position.map(|p| Objective {
                        position: [p[0] as f32, p[1] as f32, p[2] as f32],
                        label: o.label.clone(),
                        name: o.name.clone(),
                    })
                })
                .collect();
            let (vw, vh, k) = (core.vw, core.vh, core.k);
            self.markers
                .update_objectives(&positioned, camera, vw, vh, k);
            self.markers.update_grenades(dt, camera, vw, vh, k);
            self.markers.update_damage(dt, camera, vw, vh, k);

            // `minimap.draw(this._mmState)` — last, exactly as `index.js:545`.
            self.minimap.resize(minimap);
            let ops = minimap.draw(&frame.minimap);
            self.minimap.execute(&ops);
        }

        /// One journal entry. Only the arms that *spawn or clear* a pooled DOM
        /// node do anything: the rest are things the facade did to its own
        /// widgets, and the result of those already arrived in [`UiFrame`].
        fn replay(&mut self, effect: &UiEffect) {
            match effect {
                UiEffect::Hitmarker { kind, .. } => self.hit.spawn(*kind),
                UiEffect::Arc {
                    dir_x,
                    dir_z,
                    intensity,
                    ..
                } => self.arcs.spawn(*dir_x, *dir_z, *intensity),
                UiEffect::KillfeedRow { event, .. } => self.killfeed.push(event),
                UiEffect::DamageNumber {
                    position,
                    amount,
                    kind,
                    ..
                } => self.markers.spawn_damage(
                    [position[0] as f32, position[1] as f32, position[2] as f32],
                    *amount,
                    Self::damage_class(*kind),
                ),
                UiEffect::Grenade { position, fuse, .. } => self.markers.spawn_grenade(
                    [position[0] as f32, position[1] as f32, position[2] as f32],
                    *fuse,
                ),
                UiEffect::Banner { title, sub, .. } => self.banner.show(title, sub),
                UiEffect::PromptSet(spec) => self.prompt.set(spec),
                UiEffect::HitClear => self.hit.clear(),
                UiEffect::ArcsClear => self.arcs.clear(),
                UiEffect::KillfeedClear => self.killfeed.clear(),
                UiEffect::MarkersClear => self.markers.clear(),
                // Everything else is either a widget mutation whose result is
                // already in `UiFrame` (crosshair kicks, health flashes, the
                // menu's open flag), a call for the audio subsystem
                // (`Sfx`), or a minimap gate the rig itself answers.
                _ => {}
            }
        }

        /// `damageNumber`'s `kind` as the CSS class the view sets
        /// (`index.js:300`, `markers.js`).
        fn damage_class(kind: DamageKind) -> &'static str {
            match kind {
                DamageKind::Hit => "hit",
                DamageKind::Hs => "hs",
                DamageKind::Armour => "armour",
                DamageKind::Kill => "kill",
            }
        }

        /// `dispose()` (`index.js:594-612`), minus the unsubscription.
        pub fn dispose(self) {
            self.crosshair.dispose();
            self.hit.dispose();
            self.arcs.dispose();
            self.health.dispose();
            self.ammo.dispose();
            self.killfeed.dispose();
            self.compass.dispose();
            self.match_bar.dispose();
            self.minimap.dispose();
            self.markers.dispose();
            self.prompt.dispose();
            self.banner.dispose();
            self.menu.dispose();
            self.root.dispose();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::camera::Euler;
    use crate::ui::hitmarkers::HitKind;
    use crate::ui::system::{DamageKind, DebugState};

    fn pose() -> CameraPose {
        CameraPose {
            eye: [4.0, 1.66, -9.0],
            rotation: Euler {
                yaw: 0.4,
                pitch: -0.1,
                roll: 0.0,
            },
            fov_degrees: 80.0,
        }
    }

    fn pull<'a>(input: &'a Input, actors: &'a [ActorPose], t: f64) -> HudPull<'a> {
        HudPull {
            dt: 1.0 / 60.0,
            clock: UiClock { raw: t, elapsed: t },
            pose: pose(),
            aspect: 16.0 / 9.0,
            input,
            weapon: None,
            player: PlayerPull {
                health: Some(100.0),
                max_health: Some(100.0),
                ..PlayerPull::default()
            },
            player_position: [4.0, 0.0, -9.0],
            actors,
        }
    }

    fn rig() -> (HudRig, EventBus) {
        let events = EventBus::new();
        let mut rig = HudRig::new(Rng::new(0x51ce_0001));
        rig.wire(&events);
        rig.resize(1280.0, 720.0);
        (rig, events)
    }

    /// The whole point of the slice: `UiCore` runs, and it runs off the pose,
    /// the clock and the player state a real frame hands it.
    #[test]
    fn a_frame_drives_the_facade_from_the_pose_and_the_clock() {
        let (mut rig, events) = rig();
        let input = Input::new();
        let frame = rig.frame(pull(&input, &[], 1.0 / 60.0), &events);

        assert!(frame.hud_visible > 0.0, "the chrome layer is not faded out");
        // `state.time` is `ctx.time.elapsed`, written inside `late_update`.
        assert_eq!(rig.core().borrow().state.time, 1.0 / 60.0);
        // The camera basis is the pose's, not the identity default.
        assert!(
            frame.basis.forward_z.abs() < 1.0,
            "a yawed camera has a mixed forward, got {:?}",
            frame.basis
        );
        // `_mmState` tracks the player, not the camera default.
        assert_eq!((frame.minimap.x, frame.minimap.z), (4.0, -9.0));
        assert_eq!(frame.minimap.fov, 80.0);
    }

    /// `resize` reaches all three places `index.js:584-592` writes: the
    /// facade's `k`, the widgets that cache it, and the minimap's backing
    /// store. The last one is the host's job and used to have no host.
    #[test]
    fn resize_carries_the_scale_into_the_facade_and_the_minimap() {
        let (mut rig, _events) = rig();
        rig.resize(3840.0, 2160.0);
        let k = rig.core().borrow().k;
        assert!((k - 2.0).abs() < 1e-9, "4K height is k = 2");
        assert!((rig.core().borrow().compass.k - 2.0).abs() < 1e-9);
        // `round(178 * 2 * dpr)` with dpr = 1 natively.
        assert_eq!(rig.minimap.px, 356.0);
    }

    /// The soldiers reach the compass and the minimap as blips. Before this
    /// file the blip list was permanently empty — `set_blips` had no caller and
    /// `set_links` had no caller either.
    #[test]
    fn the_ai_actors_arrive_as_blips_and_the_dead_are_dropped() {
        let (mut rig, events) = rig();
        let input = Input::new();
        let actors = vec![
            ActorPose {
                id: 1,
                variant: "rifleman".to_string(),
                position: [10.0, 0.0, -20.0],
                yaw: std::f64::consts::PI,
                scale: 1.0,
                crouch: false,
                alive: true,
                lod_irrelevant: false,
                no_shadow: false,
            },
            ActorPose {
                id: 2,
                variant: "rifleman".to_string(),
                position: [12.0, 0.0, -22.0],
                yaw: 0.0,
                scale: 1.0,
                crouch: false,
                alive: false,
                lod_irrelevant: false,
                no_shadow: false,
            },
        ];
        let frame = rig.frame(pull(&input, &actors, 1.0 / 60.0), &events);
        assert_eq!(frame.minimap.blips.len(), 1, "the dead actor is skipped");
        let b = frame.minimap.blips[0];
        assert_eq!((b.x, b.z), (10.0, -20.0));
        assert!(!b.friendly);
        // `(yaw * 180) / PI`.
        assert!((b.heading_deg - 180.0).abs() < 1e-9);
    }

    /// The seven subscriptions are live, and the journal carries the outward
    /// calls a view and the audio graph consume.
    #[test]
    fn a_damage_event_reaches_the_hud_and_lands_in_the_journal() {
        let (mut rig, events) = rig();
        let input = Input::new();
        rig.frame(pull(&input, &[], 1.0 / 60.0), &events);

        let _ = events.emit(
            "damage:dealt",
            &crate::ui::system::DamageDealt {
                has_target: true,
                target_is_player: false,
                target_name: Some("BRAVO".to_string()),
                headshot: true,
                killed: true,
                amount: Some(120.0),
                point: Some([10.0, 1.5, -20.0]),
                ..Default::default()
            },
        );
        let frame = rig.frame(pull(&input, &[], 2.0 / 60.0), &events);

        let kinds: Vec<&UiEffect> = frame.effects.iter().collect();
        assert!(
            kinds.iter().any(|e| matches!(e, UiEffect::Hitmarker { kind: HitKind::Kill, .. })),
            "a kill draws a kill hitmarker"
        );
        assert!(
            kinds.iter().any(|e| matches!(
                e,
                UiEffect::DamageNumber { kind: DamageKind::Kill, .. }
            )),
            "and a kill-coloured damage number"
        );
        assert!(
            kinds.iter().any(|e| matches!(e, UiEffect::KillfeedRow { .. })),
            "and a killfeed row"
        );
        assert!(!rig.take_sfx().is_empty(), "and it made noise");
        assert!(rig.take_sfx().is_empty(), "which drains");
        assert_eq!(rig.core().borrow().state.score_us, 1);
    }

    /// Pooled spawns carry the slot the facade used, which is what lets the
    /// `wasm32` views replay the journal into their own identical pools. If
    /// this ever stops holding, the DOM and the model are painting different
    /// markers.
    #[test]
    fn journal_slots_walk_the_pool_in_acquire_order() {
        let (mut rig, events) = rig();
        let input = Input::new();
        rig.core().borrow_mut().hitmarker(HitKind::Hit);
        rig.core().borrow_mut().hitmarker(HitKind::Head);
        let frame = rig.frame(pull(&input, &[], 1.0 / 60.0), &events);
        let slots: Vec<usize> = frame
            .effects
            .iter()
            .filter_map(|e| match e {
                UiEffect::Hitmarker { slot, .. } => Some(*slot),
                _ => None,
            })
            .collect();
        assert_eq!(slots, vec![0, 1]);
        assert_eq!(frame.hit.len(), 2, "and both are live in the frame");
    }

    /// The menu has exactly one owner. `Game` forwards the Escape edge here
    /// rather than driving `PauseMenu` itself, so the two can never disagree.
    #[test]
    fn the_menu_toggles_once_per_forwarded_press() {
        let (mut rig, events) = rig();
        assert!(!rig.menu_open());
        rig.toggle_menu(&events);
        assert!(rig.menu_open());
        rig.toggle_menu(&events);
        assert!(!rig.menu_open());
    }

    /// The bake gate fires on the source's schedule and the answer comes back
    /// through `set_minimap_bake_done`. With no `LayoutSource` and no depth
    /// readback the bake cannot succeed, so the gate stays open — which is the
    /// source's behaviour too, and is why the map has no footprints.
    #[test]
    fn the_minimap_bake_gate_fires_and_never_completes() {
        let (mut rig, events) = rig();
        let input = Input::new();
        let requests = (1..=60)
            .filter(|i| {
                rig.frame(pull(&input, &[], f64::from(*i) / 60.0), &events)
                    .minimap_bake_requested
            })
            .count();
        // `++_bakeFrame > 6 && _bakeFrame % 20 == 0` — frames 20, 40 and 60.
        assert_eq!(requests, 3);
        assert!(!rig.minimap.bake_done, "no layout source, so no bake");
        assert!(rig.minimap.baked.is_none());
    }

    /// The facade's debug entry point is reachable through the rig, and its
    /// clears land in the journal where the view replays them.
    #[test]
    fn debug_state_clean_clears_every_pool_through_the_journal() {
        let (mut rig, events) = rig();
        let input = Input::new();
        rig.core().borrow_mut().hitmarker(HitKind::Kill);
        rig.core()
            .borrow_mut()
            .debug_state(DebugState::Clean, &events);
        let frame = rig.frame(pull(&input, &[], 1.0 / 60.0), &events);
        assert!(frame
            .effects
            .iter()
            .any(|e| matches!(e, UiEffect::HitClear)));
        assert!(frame.hit.is_empty(), "the pool really is empty");
    }
}
