//! The HUD subsystem facade and its event wiring.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/index.js:1-613` — the whole file.
//!
//! This is the integration layer: what turns the eleven ported widgets in
//! [`super`] into a running HUD. It owns no drawing of its own. It owns the
//! single source of truth ([`super::HudState`]), the seven event
//! subscriptions, the per-frame order the widgets are stepped in, and the
//! public API the rest of the game calls.
//!
//! ```text
//! PUBLIC API   const ui = ctx.get('ui')
//!   ui.hitmarker(kind)                  Hit | Armour | Head | Kill
//!   ui.damage_number(world_pos, n, kind)  Hit | Hs | Armour | Kill
//!   ui.hurt(amount, dir_x, dir_z)       directional arc + flash + flinch
//!   ui.killfeed_push(KillEvent)
//!   ui.banner_show(title, sub, life)    kill / objective confirmation
//!   ui.set_prompt(&PromptSpec) / ui.clear_prompt()
//!   ui.set_objectives(&[UiObjective]) / add_objective / remove_objective
//!   ui.set_blips(&[BlipSpec])
//!   ui.spawn_grenade(world_pos, fuse)
//!   ui.set_match(&MatchUpdate)
//!   ui.set_hud_visible(bool)            hide everything (cinematics)
//!   ui.pause() / ui.resume() / ui.menu.toggle()
//!   ui.debug_state(DebugState)
//! ```
//!
//! Events consumed: `weapon:fire`, `weapon:reload`, `damage:dealt`,
//! `damage:taken`, `actor:death`, `player:state`, `explosion`.
//! Events emitted (through [`PauseMenu`]): `ui:pause`, `ui:quality`,
//! `ui:sensitivity`, `ui:fov`, `ui:setting`.
//!
//! # The four seams this port had to name
//!
//! Modelled on [`crate::audio::system`], the established shape for a ported
//! `index.js` facade in this crate.
//!
//! 1. **`ctx.peek('weapons' | 'player' | 'ai')`.** The source pulls duck-typed
//!    state off three optional peers every frame (`index.js:48-58`,
//!    `249-274`). None of them exists in this port yet. They arrive through
//!    [`UiCore::set_links`] instead — every field optional, exactly as the
//!    source's `?? `-chains, so "no weapons subsystem" still means "the HUD
//!    counts its own rounds".
//! 2. **`ctx.camera` and `ctx.input`.** `Ctx` carries neither. They arrive
//!    through [`UiCore::set_camera`] / [`UiCore::set_input`], the same way
//!    [`crate::audio::system::AudioCore::set_listener_basis`] takes the
//!    listener basis — same numbers, same frame position.
//! 3. **`ctx.peek('audio')`.** `sfx(id, gain)` (`index.js:277-287`) is
//!    fire-and-forget into an optional audio subsystem, with the id strings
//!    the audio side's `UI_ALIAS` table resolves (`audio/index.js:50-54`).
//!    The port records them as [`UiEffect::Sfx`] in the frame's effect
//!    journal; the caller forwards them to [`crate::audio`]. Keeping them as
//!    data rather than a direct call is what makes the wiring assertable, and
//!    it is the honest shape anyway — the source's `try {} catch {}` exists
//!    precisely because the HUD must not care whether anyone is listening.
//! 4. **Shared mutable state.** JavaScript's `this` is reachable from the
//!    frame loop and from every event handler at once. [`EventBus`] handlers
//!    are `Fn`, so the state they mutate lives behind an `Rc<RefCell<UiCore>>`
//!    that [`UiSystem`] also holds — which *is* what `this` is, spelled out.
//!    `HealthFx::on_beat` needs the same trick from inside a widget update, so
//!    the effect journal is an `Rc<RefCell<Vec<UiEffect>>>` the callback
//!    captures.
//!
//! # What is deferred, and why
//!
//! * **The DOM.** `init()` builds the overlay and four stacking layers, and
//!   `lateUpdate` writes three `opacity` values onto them. That is the
//!   browser edge: everything above is pure state, and the `wasm32` [`view`]
//!   below is a transcriber with no decisions of its own — it creates the
//!   root and the four layers, and writes the opacity [`UiFrame`] already
//!   computed. The line is drawn exactly where the widgets themselves draw
//!   it (`crosshair::view`, `ammo::view`, …).
//! * **`minimap.js`** is ported, as [`crate::ui::minimap`]. This facade owns
//!   the two halves that were always here: the bake-gate arithmetic
//!   (`index.js:524-526`) as [`UiFrame::minimap_bake_requested`], and the
//!   `_mmState` it assembles (`index.js:527-545`) as [`UiFrame::minimap`].
//!   The host answers the gate with [`UiCore::set_minimap_bake_done`].
//! * **`demo.js`'s `CombatDemo`** is a screenshot/critic harness, not part of
//!   the HUD. [`UiCore::debug_state`] therefore implements the `Clean` and
//!   `Menu` arms in full and reports [`DebugReport::CombatUnavailable`] for
//!   the third. `HudState::simulate` stays public so a future port of the
//!   timeline can take the numbers over exactly as the source does.
//!
//! # Divergences, each deliberate
//!
//! * `_playerPos()` (`index.js:269-274`) and `ctx.time.elapsed`
//!   (`index.js:193`, `220`) are read live inside event handlers, which here
//!   have no `ctx`. Both are therefore cached: [`UiCore::set_camera`] /
//!   [`UiCore::set_links`] recompute the player position the moment either
//!   input changes, and [`UiCore::set_clock`] takes the clock. Call all four
//!   setters at the top of the frame — where `core/engine.js` advances
//!   `ctx.time` and where the camera reaches its final transform — and the
//!   handlers see exactly what the source sees. Note that `state.time` is
//!   deliberately NOT that clock: the source only writes it inside
//!   `lateUpdate`, so during a frame's `update` phase it still holds the
//!   previous frame's `elapsed`, and `ammo.js` reads it in that state.
//! * **Widths.** Every position the facade does arithmetic on is `f64`,
//!   because `THREE.Vector3` stores plain JS numbers — `_pos`, `_prevPos`,
//!   `_dir` and `_tmp` are all `f64`, and narrowing them costs ~1e-8 in the
//!   movement bloom, the arc direction and `_mmState`. The narrowing to
//!   `f32` happens only at the [`super::markers`] boundary, which stores
//!   `[f32; 3]` and is pinned at that width by its own golden, and at
//!   [`super::Blip`], whose `x`/`z` are `f32` and which the facade only ever
//!   copies into, never computes from.
//! * `ps.armour ?? ps.armor` (`index.js:443-444`) has one field here;
//!   [`super::PlayerPull`] spells it `armour` and the emitter picks.
//! * The `o._cmp` / `o._mm` caches (`index.js:539`, `575`) attach a scratch
//!   object to each objective and push *that* object into the output list. If
//!   the same objective appears twice in `_objectives`, both output entries
//!   are the same object and both read back the last write. The port stores
//!   objectives by value, so the aliasing cannot arise; the observable
//!   behaviour for distinct objectives is identical.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use axiom_kernel::Seconds;
use axiom_math::Mat4;

use crate::audio::foley::ReloadPhase;
use crate::engine::Ctx;
use crate::events::{EventBus, SubscriptionId};
use crate::registry::{Phase, Subsystem};
use crate::rng::Rng;

use super::ammo::{AmmoFrame, AmmoInput, AmmoPanel};
use super::compass::{match_frame, Compass, MatchFrame, MatchInput, ObjectiveTick};
use super::crosshair::{Crosshair, CrosshairFrame, CrosshairInput};
use super::damage::{ArcFrame, DamageArcs};
use super::health::{HealthFrame, HealthFx, HealthInput};
use super::hitmarkers::{HitKind, Hitmarkers, MarkerFrame};
use super::killfeed::{KillEvent, Killfeed, RowFrame};
use super::markers::{
    DamageNumberFrame, GrenadeFrame, Objective, ObjectiveFrame, ScreenProjector, WorldMarkers,
};
use super::menu::{MenuFrame, MenuHost, PauseMenu};
use super::prompts::{Banner, BannerFrame, Prompt, PromptFrame, PromptSpec};
use super::style;
use super::util::{clamp, clamp01, damp};
use super::{Blip, HudState, PlayerPull, WeaponPull};

/// `index.js:17`.
pub const MAX_BLIPS: usize = 48;

/// `spawnGrenade(worldPos, fuse = 2.4)` (`index.js:350`).
pub const DEFAULT_FUSE: f64 = 2.4;

/// `banner.show(title, sub, life = 2.1)` (`prompts.js:68`) — the default the
/// kill banner at `index.js:200` relies on.
pub const DEFAULT_BANNER_LIFE: f64 = 2.1;

/// `hurt(amount = 10, dirX = 0, dirZ = 1)` (`index.js:305`).
pub const DEFAULT_HURT: (f64, f64, f64) = (10.0, 0.0, 1.0);

/* ================================================================ */
/* Vocabulary                                                       */
/* ================================================================ */

/// `damageNumber`'s `kind` — `'hit' | 'hs' | 'armour' | 'kill'`
/// (`index.js:300`). [`super::markers::WorldMarkers::spawn_damage`] only needs
/// to know whether it is a kill (that picks the 1.25s vs 0.95s dwell); the
/// other three differ only in the CSS class the view sets, so the full
/// four-way kind lives here, alongside the number itself, and travels to the
/// view in [`UiEffect::DamageNumber`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Hit,
    Hs,
    Armour,
    Kill,
}

impl DamageKind {
    pub fn is_kill(self) -> bool {
        matches!(self, DamageKind::Kill)
    }
}

/// `debugState(name)`'s argument (`index.js:377`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugState {
    Combat,
    Menu,
    Clean,
}

/// What `debugState` returns (`index.js:387`, `392`, `396`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugReport {
    Clean,
    Menu,
    /// The `'combat'` arm needs `demo.js`'s `CombatDemo`, a screenshot
    /// harness that is not part of this port — see the module docs.
    CombatUnavailable,
}

/// One AI actor as `ai.getHudActors()` publishes it (`index.js:551-562`).
#[derive(Debug, Clone, Copy, Default)]
pub struct HudActor {
    /// `a?.position ?? a?.pos`.
    pub position: Option<[f64; 3]>,
    /// `a.alive === false` skips the actor. `None` is "not published".
    pub alive: Option<bool>,
    /// `a.dead === true` skips the actor.
    pub dead: Option<bool>,
    pub friendly: bool,
    pub heading: Option<f64>,
    /// Radians; used as `(yaw * 180) / PI` when `heading` is absent.
    pub yaw: Option<f64>,
}

/// One `setBlips` element (`index.js:337-348`).
#[derive(Debug, Clone, Copy, Default)]
pub struct BlipSpec {
    /// `src.x ?? src.position?.x ?? 0`, already resolved by the caller.
    pub x: f64,
    pub z: f64,
    /// `src.kind ?? (src.friendly ? 'friend' : 'enemy')`.
    pub friendly: bool,
    pub heading: Option<f64>,
}

/// `ctx.peek('player')` itself, as distinct from its `getHudState()`.
///
/// `_playerState()` and `_playerPos()` read *different* things off the same
/// object (`index.js:262-274`), and `lateUpdate:451-453` reads a third — a
/// bare numeric `player.health` when there is no hud state at all.
#[derive(Debug, Clone, Default)]
pub struct PlayerLink {
    /// `player.getHudState()` / `player.hudState`.
    pub hud: Option<PlayerPull>,
    /// `typeof player.health === 'number'`, the `else if` arm.
    pub health: Option<f64>,
    /// `player.position` (the source additionally requires `pos.isVector3`);
    /// `None` falls back to the camera position.
    pub position: Option<[f64; 3]>,
}

/// Everything the source reaches for through `ctx.peek` in one frame.
#[derive(Debug, Clone, Default)]
pub struct FrameLinks {
    /// `weapons.getHudState()`.
    pub weapon: Option<WeaponPull>,
    pub player: Option<PlayerLink>,
    /// `ai.getHudActors()` / `ai.actors`. `None` means "no ai subsystem, or
    /// it published something that is not an array" — which the source treats
    /// identically, and which notably **leaves the previous blip list
    /// standing** rather than clearing it (`index.js:552`).
    pub ai: Option<Vec<HudActor>>,
}

/// `ctx.input`, as much of it as the HUD reads (`index.js:409-416`, `462`).
#[derive(Debug, Clone, Copy)]
pub struct UiInput {
    pub enabled: bool,
    pub frozen: bool,
    pub pointer_locked: bool,
    /// `ctx.input.actionPressed('pause')`.
    pub pause_pressed: bool,
    pub ads: bool,
}

impl Default for UiInput {
    /// `enabled` starts true, matching [`crate::input::Input::new`] and the
    /// source's `core/input.js` constructor; everything else starts false.
    fn default() -> Self {
        UiInput {
            enabled: true,
            frozen: false,
            pointer_locked: false,
            pause_pressed: false,
            ads: false,
        }
    }
}

/// `ctx.camera`, as much of it as the HUD reads.
///
/// `matrix_world` is column-major, the order `THREE.Matrix4.elements` uses —
/// the facade reads elements 0, 2, 8 and 10 out of it directly
/// (`index.js:486-490`), so the storage order is part of the contract.
#[derive(Debug, Clone, Copy)]
pub struct CameraState {
    pub matrix_world: [f64; 16],
    /// `camera.position` — the `_playerPos()` fallback and the eye
    /// [`super::markers::project`] measures distance from.
    pub position: [f64; 3],
    pub fov: f64,
    /// `projectionMatrix * matrixWorldInverse`, what `Vector3.project` applies.
    pub view_projection: Mat4,
}

impl Default for CameraState {
    fn default() -> Self {
        CameraState {
            matrix_world: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            position: [0.0; 3],
            fov: 80.0,
            view_projection: Mat4::IDENTITY,
        }
    }
}

impl ScreenProjector for CameraState {
    /// Narrowed here and only here: `markers` stores and projects at `f32`,
    /// and its own golden is pinned at that width.
    fn eye(&self) -> [f32; 3] {
        narrow(self.position)
    }

    fn view_projection(&self) -> Mat4 {
        self.view_projection
    }
}

/// Owns the optional [`MenuHost`] as a **concrete** type.
///
/// This exists for a borrow-checking reason worth stating, because the obvious
/// shape does not compile. `Box<dyn MenuHost>` defaults its trait-object
/// lifetime to `'static`, so reborrowing the box out of the field yields
/// `&mut (dyn MenuHost + 'static)` — and `&mut` is *invariant* in its pointee,
/// so that reborrow cannot be shortened to fit [`PauseMenu::show`]'s
/// `Option<&mut dyn MenuHost>`, whose elided object lifetime is the
/// reference's own. The compiler resolves the mismatch the only way it can: by
/// requiring the `&mut self` borrow to outlive `'static`, which poisons every
/// other borrow in the same method (E0521, then a cascade of E0502/E0499).
///
/// Unsizing a **concrete** `&mut T` to `&mut dyn Trait` picks the object
/// lifetime freely, so the slot is a concrete type and the call sites read
/// `is_installed().then(|| &mut self.menu_host as &mut dyn MenuHost)`. `Some`
/// is still produced only when a host is genuinely installed, so the menu sees
/// exactly the `Option` the source's `ctx` reaches would give it.
#[derive(Default)]
pub struct MenuHostSlot {
    inner: Option<Box<dyn MenuHost>>,
}

impl MenuHostSlot {
    pub fn install(&mut self, host: Box<dyn MenuHost>) {
        self.inner = Some(host);
    }

    pub fn is_installed(&self) -> bool {
        self.inner.is_some()
    }
}

/// Forwarding impl. Every arm is unreachable while nothing is installed — the
/// call sites only hand the slot over once [`MenuHostSlot::is_installed`] is
/// true — so the empty answers are neutral values, never behaviour.
impl MenuHost for MenuHostSlot {
    fn freeze_time(&mut self) -> f64 {
        self.inner.as_mut().map_or(1.0, |h| h.freeze_time())
    }

    fn set_time_scale(&mut self, scale: f64) {
        if let Some(h) = self.inner.as_mut() {
            h.set_time_scale(scale);
        }
    }

    fn set_player_control_enabled(&mut self, enabled: bool) {
        if let Some(h) = self.inner.as_mut() {
            h.set_player_control_enabled(enabled);
        }
    }

    fn exit_pointer_lock(&mut self) {
        if let Some(h) = self.inner.as_mut() {
            h.exit_pointer_lock();
        }
    }

    fn request_pointer_lock(&mut self) {
        if let Some(h) = self.inner.as_mut() {
            h.request_pointer_lock();
        }
    }

    fn set_camera_fov(&mut self, fov_degrees: f32) {
        if let Some(h) = self.inner.as_mut() {
            h.set_camera_fov(fov_degrees);
        }
    }
}

/// `ctx.time`, as much of it as the HUD reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct UiClock {
    /// Unscaled wall-clock seconds since start — the `rawDt` source.
    pub raw: f64,
    /// Scaled seconds since start; becomes `state.time`.
    pub elapsed: f64,
}

/// One objective as the HUD stores it (`index.js:323-334`, `537-544`,
/// `570-580`). The source's objects are ad-hoc literals; the three consumers
/// read `position`, `label`, `name`, `color` and `id`.
#[derive(Debug, Clone, Default)]
pub struct UiObjective {
    /// `removeObjective(id)` matches on this.
    pub id: Option<String>,
    /// `if (!o.position) continue` in all three consumers.
    pub position: Option<[f64; 3]>,
    pub label: String,
    pub name: String,
    pub color: Option<String>,
}

/// `setMatch(m)` — `Object.assign(this.state, m)` (`index.js:355-357`),
/// narrowed to the four fields `index.js:42` documents.
#[derive(Debug, Clone, Default)]
pub struct MatchUpdate {
    pub score_us: Option<i64>,
    pub score_them: Option<i64>,
    pub time_left: Option<f64>,
    pub mode: Option<String>,
}

/* ================================================================ */
/* Event payloads                                                   */
/* ================================================================ */

// NOTE FOR THE INTEGRATION PASS. `EventBus` payloads cross as `&dyn Any` and a
// handler downcasts to one concrete type, so there must be exactly ONE payload
// type per event name across the whole game. `crate::audio::system` already
// declares `WeaponFire`, `WeaponReload`, `DamageDealt`, `DamageTaken`,
// `ActorDeath`, `ExplosionEvent` and `PlayerState` for the same six event
// names, and none of them carries the fields the HUD needs (`recoil`, the
// killfeed names, `armour`, `amount`, `from`, `sprinting`). The types below
// are the HUD's requirements; converging the two into one superset per event
// is a whole-game decision and belongs in the integration pass, not in this
// slice. Until it happens, only one of the two subsystems will see any given
// emit.

/// `weapon:fire`.
#[derive(Debug, Clone, Copy, Default)]
pub struct WeaponFire {
    /// `e?.recoil ?? 1`.
    pub recoil: Option<f64>,
}

/// `weapon:reload`. Only `Start` and `End` are acted on; the mid phases fall
/// through both arms untouched, exactly as the source's `if`/`else if` does.
#[derive(Debug, Clone, Copy, Default)]
pub struct WeaponReload {
    pub phase: Option<ReloadPhase>,
}

/// `damage:dealt`. The payload means "damage dealt TO `target`".
#[derive(Debug, Clone, Default)]
pub struct DamageDealt {
    /// `!!e.target` — `_isPlayerTarget(undefined)` is `false`, but
    /// `e.target?.name` needs to know whether there was a target at all.
    pub has_target: bool,
    /// The source's `t === 'player' || t === ctx.peek('player') ||
    /// t.isPlayer === true`, decided by the emitter.
    pub target_is_player: bool,
    /// `e.target?.name`.
    pub target_name: Option<String>,
    /// `e.name`, the fallback when the target has no name.
    pub name: Option<String>,
    pub headshot: bool,
    pub armour: bool,
    pub killed: bool,
    /// `e.amount ?? 0`.
    pub amount: Option<f64>,
    pub point: Option<[f64; 3]>,
}

/// `damage:taken`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DamageTaken {
    /// `e?.amount ?? 10`.
    pub amount: Option<f64>,
    /// An absolute health value wins over subtracting `amount`.
    pub health: Option<f64>,
    /// Where it came from, in world space.
    pub from: Option<[f64; 3]>,
}

/// `actor:death`.
#[derive(Debug, Clone, Default)]
pub struct ActorDeath {
    /// `e?.by?.name ?? 'ENEMY'`.
    pub by_name: Option<String>,
    /// `e?.actor?.name ?? 'OPERATOR'`.
    pub actor_name: Option<String>,
}

/// `explosion`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExplosionEvent {
    /// `if (!e?.position) return`.
    pub position: Option<[f64; 3]>,
    /// `e.radius ?? 6`.
    pub radius: Option<f64>,
}

/// `player:state`.
#[derive(Debug, Clone, Default)]
pub struct PlayerStateEvent {
    pub ads: Option<bool>,
    pub sprinting: Option<bool>,
    /// `crouch` becomes true for `'crouch'` and `'prone'`.
    pub stance: Option<String>,
}

/* ================================================================ */
/* The effect journal                                               */
/* ================================================================ */

/// One outward call the facade made, in the order it made it.
///
/// A facade *is* what it calls and when, so this is the port's primary
/// observable. It also carries the three things a numeric widget frame
/// cannot: the killfeed row's names, the banner's strings, and the damage
/// number's value and kind — all of which the `wasm32` view writes as text.
#[derive(Debug, Clone)]
pub enum UiEffect {
    /// `sfx(id, gain)` — `index.js:277-287`. The ids are the source's
    /// literals; `audio/index.js`'s `UI_ALIAS` resolves them.
    Sfx { id: &'static str, gain: f64 },
    CrosshairFire { amount: f64 },
    CrosshairFlinch { amount: f64 },
    CrosshairHit,
    HealthDamage { intensity: f64 },
    HealthRegenStart,
    Arc { dir_x: f64, dir_z: f64, intensity: f64, slot: usize },
    Hitmarker { kind: HitKind, slot: usize },
    DamageNumber { position: [f64; 3], amount: f64, kind: DamageKind, slot: usize },
    Grenade { position: [f64; 3], fuse: f64, slot: usize },
    KillfeedRow { event: KillEvent, slot: usize },
    Banner { title: String, sub: String, life: f64 },
    PromptSet(PromptSpec),
    PromptClear,
    HitClear,
    ArcsClear,
    KillfeedClear,
    MarkersClear,
    MenuShow,
    MenuClose,
    MenuToggle,
    /// `minimap.tryBake(ctx)` — the host runs the bake and reports the result
    /// back through [`UiCore::set_minimap_bake_done`].
    MinimapTryBake,
    /// `minimap.draw(this._mmState)` — hand the carried [`MinimapState`] to
    /// [`crate::ui::minimap`].
    MinimapDraw,
}

/// The `_mmState` the facade assembles for the minimap (`index.js:529-545`).
#[derive(Debug, Clone, Default)]
pub struct MinimapState {
    pub x: f64,
    pub z: f64,
    pub heading: f64,
    pub fov: f64,
    pub blips: Vec<Blip>,
    pub objectives: Vec<MinimapObjective>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MinimapObjective {
    pub x: f64,
    pub z: f64,
    pub label: String,
}

/// The camera's right/forward basis, projected to XZ and normalised
/// (`index.js:486-496`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CameraBasis {
    pub right_x: f64,
    pub right_z: f64,
    pub forward_x: f64,
    pub forward_z: f64,
}

/// Everything one `lateUpdate` computed: every widget's render state, the
/// derived scalars, and the effect journal.
#[derive(Debug, Clone)]
pub struct UiFrame {
    pub crosshair: CrosshairFrame,
    pub hit: Vec<(usize, MarkerFrame)>,
    pub arcs: Vec<(usize, ArcFrame)>,
    pub health: HealthFrame,
    pub ammo: AmmoFrame,
    pub killfeed: Vec<(usize, RowFrame)>,
    pub match_bar: MatchFrame,
    pub prompt: PromptFrame,
    pub banner: BannerFrame,
    pub menu: MenuFrame,
    pub compass_strip_x: f64,
    /// `(tick, label, colour)` per positioned objective, in list order.
    pub objective_ticks: Vec<(ObjectiveTick, String, Option<String>)>,
    pub objectives: Vec<(usize, ObjectiveFrame)>,
    pub grenades: Vec<(usize, GrenadeFrame)>,
    pub damage_numbers: Vec<(usize, DamageNumberFrame)>,
    /// The opacity written onto the chrome / world / centre layers.
    pub hud_visible: f64,
    pub heading_deg: f64,
    pub basis: CameraBasis,
    pub minimap: MinimapState,
    /// `!minimap.bakeDone && ++_bakeFrame > 6 && _bakeFrame % 20 === 0`.
    pub minimap_bake_requested: bool,
    pub effects: Vec<UiEffect>,
}

/* ================================================================ */
/* Pure helpers                                                     */
/* ================================================================ */

/// `(Math.atan2(y, x) * 180) / Math.PI`.
///
/// Deliberately **not** `f64::to_degrees`, which is `self * (180 / PI)` — a
/// different grouping, and float arithmetic is not associative. The source
/// multiplies first and divides second at all three of its bearing sites
/// (`index.js:497`, `562`, `574`), so the port does too.
fn atan2_degrees(y: f64, x: f64) -> f64 {
    (y.atan2(x) * 180.0) / std::f64::consts::PI
}

/// `(rad * 180) / Math.PI` — `index.js:562`'s `yaw` conversion.
fn radians_to_degrees(rad: f64) -> f64 {
    (rad * 180.0) / std::f64::consts::PI
}

/// JavaScript's `x || 1`: zero **and** NaN both fall through to `1`.
fn or_one(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        1.0
    } else {
        x
    }
}

/// The one place a world position crosses into `f32`. [`super::markers`]
/// stores `[f32; 3]` in its pools and projects from that, so a position
/// handed to it is rounded exactly as it would be on arrival; nothing the
/// facade itself computes goes through here.
fn narrow(p: [f64; 3]) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

/// `THREE.Vector3.prototype.length()` — `sqrt(x*x + y*y + z*z)`, in that
/// grouping. Not `f64::hypot`, which scales by the largest magnitude first
/// and rounds differently; `index.js:459-460` and `231` both go through
/// `Vector3.length()`, while `index.js:491-492` genuinely calls `Math.hypot`.
fn vec3_length(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/* ================================================================ */
/* The core                                                         */
/* ================================================================ */

/// `index.js:63-613`'s `UiSystem`, minus its `Subsystem` binding and its DOM.
pub struct UiCore {
    /// Single source of truth for everything the HUD draws.
    pub state: HudState,

    pub crosshair: Crosshair,
    pub hit: Hitmarkers<()>,
    pub arcs: DamageArcs<()>,
    pub health: HealthFx,
    pub ammo: AmmoPanel,
    pub killfeed: Killfeed<()>,
    pub compass: Compass,
    pub markers: WorldMarkers<()>,
    pub prompt: Prompt,
    pub banner: Banner,
    pub menu: PauseMenu,

    /// `this.rng` — kept because the source keeps it; both of its forks are
    /// spent in `init` (see [`UiCore::new`]).
    pub rng: Rng,

    pub k: f64,
    pub vw: f64,
    pub vh: f64,
    hud_visible: f64,
    hud_target: f64,

    clock: UiClock,
    last_raw: f64,
    last_kill_at: f64,
    regen_timer: f64,
    had_pointer_lock: bool,
    bake_frame: i64,
    /// `minimap.bakeDone`. Set through [`UiCore::set_minimap_bake_done`] by
    /// whoever runs the bake — until it is set, the gate keeps firing, which is
    /// what the source does until the bake succeeds.
    minimap_bake_done: bool,
    /// `ctx.rng.fork()`'s second draw (`index.js:86`), held until
    /// [`UiCore::take_minimap_rng`] hands it to [`crate::ui::minimap::Minimap`].
    minimap_rng: Option<Rng>,

    prev_pos: [f64; 3],
    /// `_playerPos()`, recomputed whenever the camera or the player link
    /// changes, so an event handler with no `ctx` still aims a damage arc at
    /// the live position. See the module docs.
    player_pos: [f64; 3],

    objectives: Vec<UiObjective>,
    blips: [Blip; MAX_BLIPS],
    blip_count: usize,

    /// Set each frame from [`UiCore::set_links`]; read by the event handlers
    /// the way the source re-reads `ctx.peek` inside them.
    links: FrameLinks,
    input: UiInput,
    camera: CameraState,
    menu_host: MenuHostSlot,

    /// Text the view needs but no numeric frame carries, indexed by pool slot.
    pub killfeed_rows: Vec<KillEvent>,
    pub damage_texts: Vec<(f64, DamageKind)>,
    pub banner_text: (String, String),
    pub prompt_spec: Option<PromptSpec>,

    effects: Rc<RefCell<Vec<UiEffect>>>,
}

impl UiCore {
    /// `init(ctx)` (`index.js:67-245`), minus the DOM and the event wiring
    /// ([`UiSystem::wire_events`] owns the latter).
    ///
    /// `rng` is `ctx.rng.fork()`. **Both** of the source's subsequent forks
    /// are taken here, in order: the first goes to [`WorldMarkers`]
    /// (`index.js:82`), the second to the minimap (`index.js:86`). The second
    /// is handed out by [`UiCore::take_minimap_rng`] rather than dropped: the
    /// widget lives outside this facade (the facade emits
    /// [`UiEvent::MinimapTryBake`]/[`UiEvent::MinimapDraw`] and the host
    /// realises them), but the fork has to be *this* stream's second, in this
    /// order, or every later value shifts.
    pub fn new(mut rng: Rng) -> Self {
        let markers_rng = rng.fork();
        let minimap_rng = rng.fork();
        let effects: Rc<RefCell<Vec<UiEffect>>> = Rc::new(RefCell::new(Vec::new()));

        let mut health = HealthFx::new();
        // `this.health.onBeat = (i) => this.sfx('heartbeat', 0.35 + i * 0.5)`
        // (`index.js:95`).
        let beat_sink = Rc::clone(&effects);
        health.on_beat = Some(Box::new(move |i| {
            beat_sink.borrow_mut().push(UiEffect::Sfx {
                id: "heartbeat",
                gain: 0.35 + i * 0.5,
            });
        }));

        UiCore {
            state: HudState::default(),
            crosshair: Crosshair::new(),
            hit: Hitmarkers::new(vec![(); 10]),
            arcs: DamageArcs::new(vec![(); 6]),
            health,
            ammo: AmmoPanel::new(),
            killfeed: Killfeed::new(vec![(); 6]),
            compass: Compass::new(),
            markers: WorldMarkers::new(vec![(); 6], 4, 16, markers_rng),
            prompt: Prompt::new(),
            banner: Banner::new(),
            menu: PauseMenu::new(),
            rng,
            k: 1.0,
            vw: 1920.0,
            vh: 1080.0,
            hud_visible: 1.0,
            hud_target: 1.0,
            clock: UiClock::default(),
            last_raw: 0.0,
            last_kill_at: -10.0,
            regen_timer: 0.0,
            had_pointer_lock: false,
            bake_frame: 0,
            minimap_bake_done: false,
            minimap_rng: Some(minimap_rng),
            prev_pos: [0.0; 3],
            player_pos: [0.0; 3],
            objectives: Vec::new(),
            blips: [Blip::default(); MAX_BLIPS],
            blip_count: 0,
            links: FrameLinks::default(),
            input: UiInput::default(),
            camera: CameraState::default(),
            menu_host: MenuHostSlot::default(),
            killfeed_rows: vec![KillEvent::default(); 6],
            damage_texts: vec![(0.0, DamageKind::Hit); 16],
            banner_text: (String::new(), String::new()),
            prompt_spec: None,
            effects,
        }
    }

    /// The tail of `init` (`index.js:243-244`): size the HUD to the canvas and
    /// seed `_prevPos` from the current player position, so the first frame's
    /// movement bloom does not see a teleport from the origin.
    pub fn init(&mut self, viewport_w: f64, viewport_h: f64, player_pos: [f64; 3]) {
        self.resize(viewport_w, viewport_h);
        self.prev_pos = player_pos;
        self.player_pos = player_pos;
    }

    /// `_prevPos` — what the movement bloom measures against.
    pub fn prev_pos(&self) -> [f64; 3] {
        self.prev_pos
    }

    /// `_regenTimer`, `_lastKillAt`, `_hadPointerLock`, `_bakeFrame` — the
    /// four scalars the frame does not carry but a test (and a future demo
    /// timeline) needs to see.
    pub fn regen_timer(&self) -> f64 {
        self.regen_timer
    }

    pub fn last_kill_at(&self) -> f64 {
        self.last_kill_at
    }

    /// `_lastRaw` — the unscaled timestamp the next frame's `rawDt` is
    /// measured from.
    pub fn last_raw(&self) -> f64 {
        self.last_raw
    }

    pub fn had_pointer_lock(&self) -> bool {
        self.had_pointer_lock
    }

    pub fn bake_frame(&self) -> i64 {
        self.bake_frame
    }

    /// `ctx.time`, as of the top of this frame — see the module docs for why
    /// this is not the same value as `state.time`.
    pub fn set_clock(&mut self, clock: UiClock) {
        self.clock = clock;
    }

    /// `ctx.camera` — see seam 2 in the module docs.
    pub fn set_camera(&mut self, camera: CameraState) {
        self.camera = camera;
        self.player_pos = self.resolve_player_pos();
    }

    /// `ctx.input` — see seam 2.
    pub fn set_input(&mut self, input: UiInput) {
        self.input = input;
    }

    /// What `ctx.peek('weapons' | 'player' | 'ai')` would have returned this
    /// frame — see seam 1.
    pub fn set_links(&mut self, links: FrameLinks) {
        self.links = links;
        self.player_pos = self.resolve_player_pos();
    }

    pub fn clock(&self) -> UiClock {
        self.clock
    }

    /// Install the `ctx.camera` / `ctx.time` / `ctx.peek('player')` /
    /// pointer-lock effects [`PauseMenu`] reaches for.
    pub fn set_menu_host(&mut self, host: Box<dyn MenuHost>) {
        self.menu_host.install(host);
    }

    /* ----------------------------------------------------- helpers -- */

    /// `_weaponState()` (`index.js:249-254`).
    fn weapon_state(&self) -> Option<&WeaponPull> {
        self.links.weapon.as_ref()
    }

    /// `_playerState()` (`index.js:262-267`).
    fn player_state(&self) -> Option<&PlayerPull> {
        self.links.player.as_ref().and_then(|p| p.hud.as_ref())
    }

    /// `_playerPos()` (`index.js:269-274`).
    fn resolve_player_pos(&self) -> [f64; 3] {
        self.links
            .player
            .as_ref()
            .and_then(|p| p.position)
            .unwrap_or(self.camera.position)
    }

    /// `sfx(id, gain)` (`index.js:277-287`).
    fn sfx(&self, id: &'static str, gain: f64) {
        self.effects.borrow_mut().push(UiEffect::Sfx { id, gain });
    }

    fn push(&self, effect: UiEffect) {
        self.effects.borrow_mut().push(effect);
    }

    /* --------------------------------------------------------- api -- */

    /// `hitmarker(kind)` (`index.js:291-298`).
    pub fn hitmarker(&mut self, kind: HitKind) {
        let (slot, _) = self.hit.spawn(kind);
        self.push(UiEffect::Hitmarker { kind, slot });
        self.crosshair.on_hit();
        self.push(UiEffect::CrosshairHit);
        self.sfx(
            match kind {
                HitKind::Kill => "hit_kill",
                HitKind::Head => "hit_head",
                HitKind::Armour => "hit_armour",
                HitKind::Hit => "hit_flesh",
            },
            match kind {
                HitKind::Kill => 1.0,
                _ => 0.7,
            },
        );
    }

    /// `damageNumber(worldPos, amount, kind)` (`index.js:300-302`).
    pub fn damage_number(&mut self, world_pos: [f64; 3], amount: f64, kind: DamageKind) {
        let slot = self.markers.spawn_damage(narrow(world_pos), kind.is_kill());
        self.damage_texts[slot] = (amount, kind);
        self.push(UiEffect::DamageNumber {
            position: world_pos,
            amount,
            kind,
            slot,
        });
    }

    /// Incoming damage: arc toward the source, screen flash, reticle flinch
    /// (`index.js:305-313`).
    pub fn hurt(&mut self, amount: f64, dir_x: f64, dir_z: f64) {
        let i = clamp01(amount / 40.0);
        let intensity = 0.45 + i * 0.55;
        let slot = self.arcs.spawn(dir_x, dir_z, intensity);
        self.push(UiEffect::Arc {
            dir_x,
            dir_z,
            intensity,
            slot,
        });
        self.health.on_damage(i);
        self.push(UiEffect::HealthDamage { intensity: i });
        self.crosshair.on_flinch(0.5 + i);
        self.push(UiEffect::CrosshairFlinch { amount: 0.5 + i });
        self.regen_timer = 0.0;
        self.state.regen = false;
        self.sfx("player_hurt", 0.6 + i * 0.4);
    }

    pub fn set_prompt(&mut self, p: &PromptSpec) {
        self.prompt.set(p);
        self.prompt_spec = Some(p.clone());
        self.push(UiEffect::PromptSet(p.clone()));
    }

    pub fn clear_prompt(&mut self) {
        self.prompt.clear();
        self.prompt_spec = None;
        self.push(UiEffect::PromptClear);
    }

    /// `banner.show(title, sub, life)` — reached through the facade so the
    /// strings land in the journal for the view (`index.js:38`, `200`).
    pub fn banner_show(&mut self, title: &str, sub: &str, life: f64) {
        self.banner.show(life);
        self.banner_text = (title.to_string(), sub.to_string());
        self.push(UiEffect::Banner {
            title: title.to_string(),
            sub: sub.to_string(),
            life,
        });
    }

    /// `killfeed.push(row)` — likewise (`index.js:37`, `194-199`).
    pub fn killfeed_push(&mut self, event: KillEvent) {
        let slot = self.killfeed.push();
        self.killfeed_rows[slot] = event.clone();
        self.push(UiEffect::KillfeedRow { event, slot });
    }

    /// `setObjectives(list)` (`index.js:323-325`) — `list ?? []`.
    pub fn set_objectives(&mut self, list: &[UiObjective]) {
        self.objectives = list.to_vec();
    }

    pub fn add_objective(&mut self, o: UiObjective) {
        self.objectives.push(o);
    }

    /// `removeObjective(id)` (`index.js:331-334`) — a miss is a no-op.
    pub fn remove_objective(&mut self, id: &str) {
        let found = self
            .objectives
            .iter()
            .position(|o| o.id.as_deref() == Some(id));
        if let Some(i) = found {
            self.objectives.remove(i);
        }
    }

    pub fn objectives(&self) -> &[UiObjective] {
        &self.objectives
    }

    /// Copies into the preallocated array — the caller's slice is not
    /// retained (`index.js:337-348`).
    pub fn set_blips(&mut self, list: &[BlipSpec]) {
        let n = list.len().min(MAX_BLIPS);
        for (dst, src) in self.blips.iter_mut().zip(list.iter()).take(n) {
            // `super::Blip` stores `x`/`z` as `f32`; the facade only ever
            // copies into it, so this is the carrier's width, not a compute.
            dst.x = src.x as f32;
            dst.z = src.z as f32;
            dst.friendly = src.friendly;
            dst.heading_deg = src.heading.unwrap_or(0.0);
        }
        self.blip_count = n;
    }

    pub fn blips(&self) -> &[Blip] {
        &self.blips[..self.blip_count]
    }

    /// `spawnGrenade(worldPos, fuse)` (`index.js:350-353`).
    pub fn spawn_grenade(&mut self, world_pos: [f64; 3], fuse: f64) {
        let slot = self.markers.spawn_grenade(narrow(world_pos), fuse);
        self.push(UiEffect::Grenade {
            position: world_pos,
            fuse,
            slot,
        });
        self.sfx("grenade_warn", 0.6);
    }

    /// `setMatch(m)` (`index.js:355-357`).
    pub fn set_match(&mut self, m: &MatchUpdate) {
        if let Some(v) = m.score_us {
            self.state.score_us = v;
        }
        if let Some(v) = m.score_them {
            self.state.score_them = v;
        }
        if let Some(v) = m.time_left {
            self.state.time_left = v;
        }
        if let Some(v) = &m.mode {
            self.state.mode = v.clone();
        }
    }

    pub fn set_hud_visible(&mut self, visible: bool) {
        self.hud_target = if visible { 1.0 } else { 0.0 };
    }

    pub fn hud_target(&self) -> f64 {
        self.hud_target
    }

    pub fn hud_visible(&self) -> f64 {
        self.hud_visible
    }

    /// `pause()` (`index.js:363-365`).
    pub fn pause(&mut self, events: &EventBus) {
        self.push(UiEffect::MenuShow);
        let installed = self.menu_host.is_installed();
        let host = installed.then(|| &mut self.menu_host as &mut dyn MenuHost);
        self.menu.show(host, events);
    }

    /// `resume()` (`index.js:367-369`).
    pub fn resume(&mut self, events: &EventBus) {
        self.push(UiEffect::MenuClose);
        let installed = self.menu_host.is_installed();
        let host = installed.then(|| &mut self.menu_host as &mut dyn MenuHost);
        self.menu.close(host, events);
    }

    /* ------------------------------------------------------- debug -- */

    /// `debugState(name)` (`index.js:377-397`).
    /// The minimap's RNG fork (`index.js:86`) — `None` after the first call.
    ///
    /// [`crate::ui::minimap::Minimap::new`] takes it. The fork is spent inside
    /// [`UiCore::new`] whether or not anyone asks for it, because its *position*
    /// in the stream is what matters.
    pub fn take_minimap_rng(&mut self) -> Option<Rng> {
        self.minimap_rng.take()
    }

    /// `minimap.bakeDone = …` — the host's answer to [`UiEvent::MinimapTryBake`].
    ///
    /// The source reads the flag straight off the minimap object; here the
    /// minimap widget is a separate module, so the bake result comes back
    /// through this. Until it is set the gate re-fires every twentieth frame
    /// after the sixth, exactly as `index.js:524-526` does — so leaving it unset
    /// is a live bake request, not an idle one.
    pub fn set_minimap_bake_done(&mut self, done: bool) {
        self.minimap_bake_done = done;
    }

    pub fn debug_state(&mut self, name: DebugState, events: &EventBus) -> DebugReport {
        match name {
            DebugState::Clean => {
                // `this.demo?.stop(this); this.demo = null;` — no demo exists.
                self.state.simulate = false;
                self.killfeed.clear();
                self.push(UiEffect::KillfeedClear);
                self.arcs.clear();
                self.push(UiEffect::ArcsClear);
                self.hit.clear();
                self.push(UiEffect::HitClear);
                self.markers.clear();
                self.push(UiEffect::MarkersClear);
                self.clear_prompt();
                DebugReport::Clean
            }
            DebugState::Menu => {
                self.debug_state(DebugState::Combat, events);
                self.pause(events);
                DebugReport::Menu
            }
            DebugState::Combat => DebugReport::CombatUnavailable,
        }
    }

    /* ------------------------------------------------------- frame -- */

    /// `lateUpdate(dt, ctx)` (`index.js:401-546`).
    pub fn late_update(&mut self, dt: f64, events: &EventBus) -> UiFrame {
        let raw_dt = clamp(self.clock.raw - self.last_raw, 0.0, 0.1);
        self.last_raw = self.clock.raw;
        self.state.time = self.clock.elapsed;

        // ---- pause -------------------------------------------------------
        if self.input.enabled && !self.input.frozen {
            if self.input.pause_pressed {
                self.push(UiEffect::MenuToggle);
                let was_open = self.menu.open;
                self.push(if was_open {
                    UiEffect::MenuClose
                } else {
                    UiEffect::MenuShow
                });
                let installed = self.menu_host.is_installed();
                let host = installed.then(|| &mut self.menu_host as &mut dyn MenuHost);
                self.menu.toggle(host, events);
            }
            // Losing pointer lock mid-match is the same intent as Escape.
            if self.input.pointer_locked {
                self.had_pointer_lock = true;
            } else if self.had_pointer_lock && !self.menu.open {
                self.had_pointer_lock = false;
                self.push(UiEffect::MenuShow);
                let installed = self.menu_host.is_installed();
                let host = installed.then(|| &mut self.menu_host as &mut dyn MenuHost);
                self.menu.show(host, events);
            }
        }
        let menu_frame = self.menu.update(raw_dt);

        // ---- external state ----------------------------------------------
        // `simulate` means a scripted debug timeline owns the HUD numbers;
        // letting the live weapon/player state through would fight it.
        let simulate = self.state.simulate;
        let ws = (!simulate).then(|| self.weapon_state().cloned()).flatten();
        if let Some(w) = &ws {
            if let Some(v) = &w.name {
                self.state.weapon_name = v.clone();
            }
            if let Some(v) = &w.mode {
                self.state.fire_mode = v.clone();
            }
            if let Some(v) = w.ammo {
                self.state.ammo = v;
            }
            if let Some(v) = w.reserve {
                self.state.reserve = v;
            }
            if let Some(v) = w.mag_size {
                self.state.mag_size = v;
            }
            if let Some(v) = w.reloading {
                self.state.reloading = v;
            }
            if let Some(v) = w.reload_progress {
                self.state.reload_progress = v;
            }
            if let Some(v) = w.ads {
                self.state.ads = v;
            }
            if let Some(v) = w.spread {
                self.state.base_spread = 4.0 + v * 40.0;
            }
            if let Some(v) = w.lethal_count {
                self.state.lethal_count = v;
            }
            if let Some(v) = w.tactical_count {
                self.state.tactical_count = v;
            }
        }

        let ps = (!simulate).then(|| self.player_state().cloned()).flatten();
        if let Some(p) = &ps {
            if let Some(v) = p.health {
                self.state.health = v;
            }
            if let Some(v) = p.max_health {
                self.state.max_health = v;
            }
            if let Some(v) = p.armour {
                self.state.armour = v;
            }
            if let Some(v) = p.regen {
                self.state.regen = v;
            }
            if let Some(v) = p.move_amount {
                self.state.move_amount = v;
            }
            if let Some(v) = p.sprint {
                self.state.sprint = v;
            }
            if let Some(v) = p.crouch {
                self.state.crouch = v;
            }
            if let Some(v) = p.ads {
                self.state.ads = v;
            }
            if let Some(v) = p.airborne {
                self.state.airborne = v;
            }
        } else if let Some(h) = self.links.player.as_ref().and_then(|p| p.health) {
            self.state.health = h;
        }

        // ---- movement-derived reticle bloom -------------------------------
        let pos = self.resolve_player_pos();
        self.player_pos = pos;
        if ps.is_none() && !simulate {
            // `_dir = pos - prevPos; _dir.y = 0; speed = _dir.length() / dt`.
            let dir = [pos[0] - self.prev_pos[0], 0.0, pos[2] - self.prev_pos[2]];
            let speed = if dt > 0.0 { vec3_length(dir) / dt } else { 0.0 };
            self.state.move_amount = damp(
                self.state.move_amount,
                clamp01(speed / 6.2),
                12.0,
                raw_dt.max(1e-3),
            );
            if self.weapon_state().is_none() {
                self.state.ads = self.input.ads && self.input.enabled;
            }
        }
        self.prev_pos = pos;

        // ---- health regeneration when nobody else owns health -------------
        if ps.is_none() && !simulate && self.state.health < self.state.max_health {
            self.regen_timer += dt;
            if self.regen_timer > 4.5 {
                if !self.state.regen {
                    self.state.regen = true;
                    self.health.on_regen_start();
                    self.push(UiEffect::HealthRegenStart);
                    self.sfx("regen", 0.4);
                }
                self.state.health = self.state.max_health.min(self.state.health + dt * 24.0);
            }
        }

        // ---- demo timeline ------------------------------------------------
        // `if (this.demo?.active) this.demo.update(this, dt)` — `demo.js` is
        // a screenshot harness and is not ported (see the module docs).

        // ---- ai blips ------------------------------------------------------
        self.collect_blips();

        // ---- camera basis ---------------------------------------------------
        let m = self.camera.matrix_world;
        let mut rx = m[0];
        let mut rz = m[2];
        let mut fx = -m[8];
        let mut fz = -m[10];
        let rl = or_one(rx.hypot(rz));
        let fl = or_one(fx.hypot(fz));
        rx /= rl;
        rz /= rl;
        fx /= fl;
        fz /= fl;
        let heading = atan2_degrees(fx, -fz);
        let basis = CameraBasis {
            right_x: rx,
            right_z: rz,
            forward_x: fx,
            forward_z: fz,
        };

        // ---- widgets ---------------------------------------------------------
        let hud_goal = self.hud_target * if self.menu.open { 0.15 } else { 1.0 };
        self.hud_visible = damp(self.hud_visible, hud_goal, 10.0, raw_dt);

        let crosshair = self.crosshair.update(
            dt,
            CrosshairInput {
                move_amount: self.state.move_amount,
                sprint: self.state.sprint,
                crouch: self.state.crouch,
                airborne: self.state.airborne,
                ads: self.state.ads,
                base_spread: Some(self.state.base_spread),
                // `s.hidden` is never set anywhere in the source's state.
                hidden: false,
            },
        );
        let hit = self.hit.update(dt);
        let arcs = self.arcs.update(dt, rx, rz, fx, fz);
        let health = self.health.update(
            dt,
            HealthInput {
                health: self.state.health,
                max_health: self.state.max_health,
                armour: self.state.armour,
                max_armour: self.state.max_armour,
                regen: self.state.regen,
            },
        );
        let ammo = self.ammo.update(
            dt,
            &AmmoInput {
                ammo: self.state.ammo,
                reserve: self.state.reserve,
                mag_size: self.state.mag_size,
                weapon_name: self.state.weapon_name.clone(),
                fire_mode: self.state.fire_mode.clone(),
                reloading: self.state.reloading,
                reload_progress: self.state.reload_progress,
                lethal_count: self.state.lethal_count,
                tactical_count: self.state.tactical_count,
                time: self.state.time,
            },
        );
        let killfeed = self.killfeed.update(dt);
        let match_bar = match_frame(
            &MatchInput {
                score_us: self.state.score_us,
                score_them: self.state.score_them,
                time_left: self.state.time_left,
            },
            &self.state.mode,
        );
        let prompt = self.prompt.update(dt);
        let banner = self.banner.update(dt);

        // `_buildCompassObjectives(pos)` then `compass.update(heading, objs)`.
        let compass_objs = self.compass_objectives(pos);
        let compass_strip_x = self.compass.strip_offset(heading);
        let objective_ticks: Vec<(ObjectiveTick, String, Option<String>)> = compass_objs
            .into_iter()
            .map(|(bearing, label, colour)| (self.compass.objective_tick(bearing), label, colour))
            .collect();

        // ---- world markers ---------------------------------------------------
        let camera = self.camera;
        let (vw, vh, k) = (self.vw, self.vh, self.k);
        let positioned: Vec<Objective> = self
            .objectives
            .iter()
            .filter_map(|o| {
                o.position.map(|position| Objective {
                    position: narrow(position),
                    label: o.label.clone(),
                    name: o.name.clone(),
                })
            })
            .collect();
        let objectives = self
            .markers
            .update_objectives(&positioned, &camera, vw, vh, k);
        let grenades = self.markers.update_grenades(dt, &camera, vw, vh, k);
        let damage_numbers = self.markers.update_damage(dt, &camera, vw, vh, k);

        // ---- minimap ---------------------------------------------------------
        // `!this.minimap.bakeDone && ++this._bakeFrame > 6 && this._bakeFrame
        // % 20 === 0` — the increment is INSIDE the `&&` chain, so it only
        // happens while the bake is still outstanding.
        let bake_requested = !self.minimap_bake_done && {
            self.bake_frame += 1;
            self.bake_frame > 6 && self.bake_frame % 20 == 0
        };
        if bake_requested {
            self.push(UiEffect::MinimapTryBake);
        }
        let minimap = MinimapState {
            x: pos[0],
            z: pos[2],
            heading,
            fov: self.camera.fov,
            blips: self.blips[..self.blip_count].to_vec(),
            objectives: self
                .objectives
                .iter()
                .filter_map(|o| {
                    o.position.map(|p| MinimapObjective {
                        x: p[0],
                        z: p[2],
                        label: o.label.clone(),
                    })
                })
                .collect(),
        };
        self.push(UiEffect::MinimapDraw);

        UiFrame {
            crosshair,
            hit,
            arcs,
            health,
            ammo,
            killfeed,
            match_bar,
            prompt,
            banner,
            menu: menu_frame,
            compass_strip_x,
            objective_ticks,
            objectives,
            grenades,
            damage_numbers,
            hud_visible: self.hud_visible,
            heading_deg: heading,
            basis,
            minimap,
            minimap_bake_requested: bake_requested,
            effects: self.effects.borrow_mut().drain(..).collect(),
        }
    }

    /// `_collectBlips()` (`index.js:548-565`).
    fn collect_blips(&mut self) {
        // `if (this.demo?.active) return;` — no demo (see the module docs).
        let Some(list) = self.links.ai.as_ref() else {
            // Not an array / no subsystem: the source returns *without*
            // touching `_blipCount`, so whatever `setBlips` last wrote stands.
            return;
        };
        let mut n = 0usize;
        for a in list.iter() {
            if n >= MAX_BLIPS {
                break;
            }
            let Some(p) = a.position else {
                continue;
            };
            if a.alive == Some(false) || a.dead == Some(true) {
                continue;
            }
            let b = &mut self.blips[n];
            n += 1;
            b.x = p[0] as f32;
            b.z = p[2] as f32;
            b.friendly = a.friendly;
            b.heading_deg = a
                .heading
                .unwrap_or_else(|| a.yaw.map_or(0.0, radians_to_degrees));
        }
        self.blip_count = n;
    }

    /// `_buildCompassObjectives(pos)` (`index.js:567-582`) — `(bearing,
    /// label, colour)` per positioned objective, in list order. Public
    /// because the bearing is the facade's own arithmetic, and the compass
    /// tick it feeds throws it away.
    pub fn compass_objectives(&self, pos: [f64; 3]) -> Vec<(f64, String, Option<String>)> {
        self.objectives
            .iter()
            .filter_map(|o| {
                let p = o.position?;
                let dx = p[0] - pos[0];
                let dz = p[2] - pos[2];
                Some((
                    atan2_degrees(dx, -dz),
                    o.label.clone(),
                    o.color.clone(),
                ))
            })
            .collect()
    }

    /// `resize(w, h, ctx)` (`index.js:584-592`).
    pub fn resize(&mut self, w: f64, h: f64) {
        self.vw = w;
        self.vh = h;
        self.k = style::scale_factor(h);
        self.crosshair.set_scale(self.k);
        self.compass.set_scale(self.k);
        // `this.minimap.resize(this.k)` — the widget lives outside this facade,
        // so the host forwards `self.k` to `Minimap::resize`. See the module doc.
    }

    /* ------------------------------------------------------ events -- */

    /// `weapon:fire` (`index.js:155-160`).
    pub fn on_weapon_fire(&mut self, e: &WeaponFire) {
        let recoil = e.recoil.unwrap_or(1.0);
        self.crosshair.on_fire(recoil);
        self.push(UiEffect::CrosshairFire { amount: recoil });
        if self.state.simulate {
            return;
        }
        if self.weapon_state().is_none() {
            self.state.ammo = (self.state.ammo - 1).max(0); // `Math.max(0, ammo - 1)`
        }
    }

    /// `weapon:reload` (`index.js:162-175`).
    pub fn on_weapon_reload(&mut self, e: &WeaponReload) {
        match e.phase {
            Some(ReloadPhase::Start) => {
                self.state.reloading = true;
                self.state.reload_progress = 0.0;
            }
            Some(ReloadPhase::End) => {
                self.state.reloading = false;
                if self.weapon_state().is_none() {
                    let take = (self.state.mag_size - self.state.ammo).min(self.state.reserve);
                    self.state.ammo += take;
                    self.state.reserve -= take;
                }
            }
            _ => {}
        }
    }

    /// `damage:dealt` (`index.js:177-203`).
    pub fn on_damage_dealt(&mut self, e: &DamageDealt) {
        // The payload means "damage dealt TO e.target". `ai` uses it for enemy
        // rounds that connect with the player, which must not draw a hitmarker
        // or a "YOU killed" killfeed row — that arrives as `damage:taken`.
        if e.has_target && e.target_is_player {
            return;
        }
        let kind = if e.killed {
            HitKind::Kill
        } else if e.headshot {
            HitKind::Head
        } else if e.armour {
            HitKind::Armour
        } else {
            HitKind::Hit
        };
        self.hitmarker(kind);
        if let Some(point) = e.point {
            let dk = if e.killed {
                DamageKind::Kill
            } else if e.headshot {
                DamageKind::Hs
            } else if e.armour {
                DamageKind::Armour
            } else {
                DamageKind::Hit
            };
            self.damage_number(point, e.amount.unwrap_or(0.0), dk);
        }
        if e.killed {
            self.last_kill_at = self.clock.elapsed;
            let victim = e
                .target_name
                .clone()
                .or_else(|| e.name.clone())
                .unwrap_or_else(|| "ENEMY".to_string());
            self.killfeed_push(KillEvent {
                attacker: "YOU".to_string(),
                victim,
                headshot: e.headshot,
                mine: true,
                attacker_friendly: None,
            });
            let sub = if e.headshot {
                "+150 XP · HEADSHOT"
            } else {
                "+100 XP"
            };
            self.banner_show("Enemy Eliminated", sub, DEFAULT_BANNER_LIFE);
            self.state.score_us += 1;
        }
    }

    /// `damage:taken` (`index.js:205-217`).
    pub fn on_damage_taken(&mut self, e: &DamageTaken) {
        let amount = e.amount.unwrap_or(DEFAULT_HURT.0);
        match e.health {
            Some(h) => self.state.health = h,
            None => self.state.health = 0.0f64.max(self.state.health - amount),
        }
        let mut dx = DEFAULT_HURT.1;
        let mut dz = DEFAULT_HURT.2;
        if let Some(from) = e.from {
            let here = self.player_pos;
            dx = from[0] - here[0];
            dz = from[2] - here[2];
        }
        self.hurt(amount, dx, dz);
    }

    /// `actor:death` (`index.js:219-226`).
    pub fn on_actor_death(&mut self, e: &ActorDeath) {
        if self.clock.elapsed - self.last_kill_at < 0.3 {
            return; // already credited by `damage:dealt`
        }
        self.killfeed_push(KillEvent {
            attacker: e.by_name.clone().unwrap_or_else(|| "ENEMY".to_string()),
            victim: e
                .actor_name
                .clone()
                .unwrap_or_else(|| "OPERATOR".to_string()),
            headshot: false,
            mine: false,
            attacker_friendly: Some(false),
        });
    }

    /// `explosion` (`index.js:228-233`).
    pub fn on_explosion(&mut self, e: &ExplosionEvent) {
        let Some(position) = e.position else {
            return;
        };
        let here = self.player_pos;
        let d = vec3_length([
            position[0] - here[0],
            position[1] - here[1],
            position[2] - here[2],
        ]);
        if d < e.radius.unwrap_or(6.0) * 2.5 {
            self.crosshair.on_flinch(0.6);
            self.push(UiEffect::CrosshairFlinch { amount: 0.6 });
        }
    }

    /// `player:state` (`index.js:235-241`).
    pub fn on_player_state(&mut self, e: &PlayerStateEvent) {
        if let Some(v) = e.ads {
            self.state.ads = v;
        }
        if let Some(v) = e.sprinting {
            self.state.sprint = v;
        }
        if let Some(v) = &e.stance {
            self.state.crouch = v.as_str() == "crouch" || v.as_str() == "prone";
        }
    }

    /// `dispose()` (`index.js:594-612`), minus the DOM teardown and the
    /// unsubscription ([`UiSystem::dispose`] owns the latter). The widgets'
    /// `dispose` is DOM-only, so on the native side there is nothing to free —
    /// what remains is resetting the pooled state so a re-`init` starts clean.
    pub fn dispose(&mut self) {
        self.hit.clear();
        self.arcs.clear();
        self.killfeed.clear();
        self.markers.clear();
        self.effects.borrow_mut().clear();
    }
}

/* ================================================================ */
/* The Subsystem wrapper                                            */
/* ================================================================ */

/// The registered subsystem. `static id = 'ui'`, `static deps = ['render']`.
pub struct UiSystem {
    core: Rc<RefCell<UiCore>>,
    offs: Vec<(&'static str, SubscriptionId)>,
    frame: Option<UiFrame>,
}

impl UiSystem {
    pub fn new(rng: Rng) -> Self {
        UiSystem {
            core: Rc::new(RefCell::new(UiCore::new(rng))),
            offs: Vec::new(),
            frame: None,
        }
    }

    /// The shared guts, so the app can push the camera / input / peer state in
    /// and read the frame back out — see the seams in the module docs.
    pub fn core(&self) -> Rc<RefCell<UiCore>> {
        Rc::clone(&self.core)
    }

    /// The seven `ctx.events.on(...)` calls in `init` (`index.js:152-241`).
    pub fn wire_events(&mut self, ctx: &Ctx<'_>) {
        macro_rules! on {
            ($name:literal, $payload:ty, $method:ident) => {{
                let core = Rc::clone(&self.core);
                let id = ctx.events.on($name, move |p: &dyn Any| {
                    if let Some(p) = p.downcast_ref::<$payload>() {
                        core.borrow_mut().$method(p);
                    }
                    Ok(())
                });
                self.offs.push(($name, id));
            }};
        }
        on!("weapon:fire", WeaponFire, on_weapon_fire);
        on!("weapon:reload", WeaponReload, on_weapon_reload);
        on!("damage:dealt", DamageDealt, on_damage_dealt);
        on!("damage:taken", DamageTaken, on_damage_taken);
        on!("actor:death", ActorDeath, on_actor_death);
        on!("explosion", ExplosionEvent, on_explosion);
        on!("player:state", PlayerStateEvent, on_player_state);
    }

    /// The last frame this system produced, for whoever paints it.
    pub fn take_frame(&mut self) -> Option<UiFrame> {
        self.frame.take()
    }

    fn step(&mut self, dt: f64, ctx: &Ctx<'_>) {
        let mut core = self.core.borrow_mut();
        // Re-sync in case the app did not; identical values either way.
        core.set_clock(UiClock {
            raw: ctx.time.raw,
            elapsed: ctx.time.elapsed,
        });
        let frame = core.late_update(dt, ctx.events);
        drop(core);
        self.frame = Some(frame);
    }
}

impl Subsystem for UiSystem {
    fn id(&self) -> &'static str {
        "ui"
    }

    fn deps(&self) -> &'static [&'static str] {
        &["render"]
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::LateUpdate, Phase::Resize]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), crate::error::CoreError> {
        self.wire_events(ctx);
        Ok(())
    }

    /// Driven after every `update`, so the HUD observes the camera's final
    /// transform for the frame — the whole reason the source uses
    /// `lateUpdate` (`index.js:24-25`).
    fn late_update(&mut self, dt: Seconds, ctx: &Ctx<'_>) {
        self.step(f64::from(dt.get()), ctx);
    }

    fn resize(&mut self, width: u32, height: u32, _ctx: &Ctx<'_>) {
        self.core
            .borrow_mut()
            .resize(f64::from(width), f64::from(height));
    }

    fn dispose(&mut self) {
        self.offs.clear();
        self.core.borrow_mut().dispose();
    }
}

/* ================================================================ */
/* The browser edge                                                 */
/* ================================================================ */

/// The DOM half of `init`/`lateUpdate`/`dispose` — the root overlay, the four
/// stacking layers, and the three `opacity` writes `lateUpdate` makes onto
/// them (`index.js:71-79`, `502-504`, `588`, `610-611`).
///
/// Nothing here decides anything: every number it writes was computed by
/// [`UiCore::late_update`] on the native side and arrives in [`UiFrame`].
#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use super::super::style::install::{install_styles, remove_styles};
    use super::super::util::dom;
    use super::UiFrame;

    /// The overlay root and its four layers, in the source's stacking order:
    /// hurt overlays under the HUD, the menu over everything.
    pub struct HudRoot {
        pub root: Element,
        pub hurt_layer: Element,
        pub world_layer: Element,
        pub centre_layer: Element,
        pub chrome_layer: Element,
    }

    impl HudRoot {
        /// `installStyles()` + `index.js:72-79`.
        pub fn install(host: Option<&Element>) -> HudRoot {
            install_styles();
            let root = dom::el("div", Some("ow-hud"), host);
            let hurt_layer = dom::el("div", Some("ow-layer"), Some(&root));
            let world_layer = dom::el("div", Some("ow-layer"), Some(&root));
            let centre_layer = dom::el("div", Some("ow-layer"), Some(&root));
            let chrome_layer = dom::el("div", Some("ow-layer"), Some(&root));
            HudRoot {
                root,
                hurt_layer,
                world_layer,
                centre_layer,
                chrome_layer,
            }
        }

        /// `index.js:588` — `--k` carries the HUD scale into every CSS rule.
        pub fn set_scale(&self, k: f64) {
            dom::set_style(&self.root, "--k", &format!("{k:.4}"));
        }

        /// `index.js:502-504`.
        pub fn apply(&self, frame: &UiFrame) {
            let opacity = format!("{:.3}", frame.hud_visible);
            dom::set_style(&self.chrome_layer, "opacity", &opacity);
            dom::set_style(&self.world_layer, "opacity", &opacity);
            dom::set_style(&self.centre_layer, "opacity", &opacity);
        }

        /// `index.js:610-611`.
        pub fn dispose(self) {
            dom::remove(&self.root);
            remove_styles();
        }
    }
}
