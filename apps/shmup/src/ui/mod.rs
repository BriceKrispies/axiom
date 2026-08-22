//! **HUD / UI subsystem.**
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/` (all files except `demo.js` —
//! see below).
//!
//! ```text
//! this crate module   source
//! ------------------  ----------------
//! util                util.js
//! style               style.js
//! crosshair           crosshair.js
//! hitmarkers          hitmarkers.js
//! damage              damage.js
//! health              health.js
//! ammo                ammo.js
//! killfeed            killfeed.js
//! compass             compass.js
//! prompts             prompts.js
//! menu                menu.js
//! markers             markers.js
//! minimap             minimap.js
//! (this file)         index.js (`UiSystem`)
//! ```
//!
//! ## What is deferred, and why
//!
//! **`minimap.js` (603 lines) is ported**, as [`minimap`]. It was recorded for
//! months as blocked on an orthographic depth bake plus a Sobel pass; both
//! halves of that were false. There is no Sobel pass — `minimap.js:10-23`'s
//! *comment* describes one, the code uses a blurred-coverage rim (`:415`) — and
//! the depth bake is the **fallback** (`:74-76`), not the primary path. The
//! primary is `_buildVectorMap`, pure CPU, needing only
//! `world.{buildings, levelToWorld, isOpen}`, all three already public. So it
//! ported with no engine capability added and nothing invented.
//!
//! That deferral is the fifth this port has found to be a defect, and the first
//! that was **never true** rather than true-then-expired. A deferral is a claim,
//! and a claim has to be checked against the code, not against the comment above
//! it.
//!
//! **`demo.js` (198 lines)** drives a scripted combat timeline against this
//! same public API purely for screenshot/critic capture
//! (`UiSystem.debugState('combat')`). It is a test harness, not part of the
//! HUD itself, and is left for whichever slice ports the capture tooling.
//!
//! ## Why this is not yet a [`crate::registry::Subsystem`]
//!
//! `index.js`'s `UiSystem` is a real `Subsystem`: it reads `ctx.camera`,
//! `ctx.canvas`, `ctx.input`, and pulls duck-typed state off
//! `ctx.peek('weapons')`/`ctx.peek('player')`/`ctx.peek('ai')`. None of
//! those exist on [`crate::engine::Ctx`] yet — no camera, canvas, input,
//! weapons, player, or ai subsystem has landed in this port (see the
//! concurrency note in the port manifest: those are other agents' slices,
//! running in parallel with this one).
//!
//! Rather than block on that or invent placeholder subsystems here (which
//! would just be a second, throwaway design the real ones would replace),
//! [`Hud`] is the source's `UiSystem` minus the `Subsystem` impl and the
//! `ctx.get`/`ctx.peek` reaches: every value the source pulls from another
//! subsystem is instead an explicit, optional parameter to
//! [`Hud::late_update`] — the same shape [`markers::ScreenProjector`] and
//! [`menu::MenuHost`] already use for the camera and the pause menu's
//! host effects. When the camera/input/weapons/player/ai subsystems land,
//! wiring `Hud` behind a real `Subsystem` impl that reads `ctx` and calls
//! `late_update` with the pulled values is a thin adapter, not a redesign.

pub mod ammo;
pub mod compass;
pub mod crosshair;
pub mod damage;
pub mod health;
pub mod hitmarkers;
pub mod killfeed;
pub mod markers;
pub mod menu;
pub mod minimap;
pub mod prompts;
pub mod style;
pub mod system;
pub mod util;

use ammo::{AmmoFrame, AmmoInput, AmmoPanel};
use compass::{match_frame, Compass, MatchFrame, MatchInput};
use crosshair::{Crosshair, CrosshairFrame, CrosshairInput};
use damage::{ArcFrame, DamageArcs};
use health::{HealthFrame, HealthFx, HealthInput};
use hitmarkers::{HitKind, Hitmarkers, MarkerFrame};
use killfeed::{Killfeed, RowFrame};
use markers::{Objective, WorldMarkers};
use menu::{MenuFrame, PauseMenu};
use prompts::{Banner, BannerFrame, Prompt, PromptFrame, PromptSpec};
use util::{clamp01, damp};

/// One AI actor's compass/minimap blip — `ui.setBlips`'s element shape
/// (`index.js:337-348`). `MAX_BLIPS` (48 in the source) has no home yet: the
/// only consumer of the blip list is the minimap (`minimap.js`'s
/// `_mmState.blips`), and [`FramePull::blips`] carries the shape forward
/// uncapped.
#[derive(Debug, Clone, Copy, Default)]
pub struct Blip {
    pub x: f32,
    pub z: f32,
    pub friendly: bool,
    pub heading_deg: f64,
}

/// Duck-typed weapon state — `weapons.getHudState()` (`index.js:50-52`).
#[derive(Debug, Clone, Default)]
pub struct WeaponPull {
    pub name: Option<String>,
    pub mode: Option<String>,
    pub ammo: Option<i64>,
    pub reserve: Option<i64>,
    pub mag_size: Option<i64>,
    pub reloading: Option<bool>,
    pub reload_progress: Option<f64>,
    pub ads: Option<bool>,
    pub spread: Option<f64>,
    pub lethal_count: Option<i64>,
    pub tactical_count: Option<i64>,
}

/// Duck-typed player state — `player.getHudState()` (`index.js:53-54`).
#[derive(Debug, Clone, Default)]
pub struct PlayerPull {
    pub health: Option<f64>,
    pub max_health: Option<f64>,
    pub armour: Option<f64>,
    pub regen: Option<bool>,
    pub move_amount: Option<f64>,
    pub sprint: Option<bool>,
    pub crouch: Option<bool>,
    pub ads: Option<bool>,
    pub airborne: Option<bool>,
    pub position: Option<[f32; 3]>,
}

/// Single source of truth for everything the HUD draws — `index.js:98-126`'s
/// `this.state`.
#[derive(Debug, Clone)]
pub struct HudState {
    pub health: f64,
    pub max_health: f64,
    pub armour: f64,
    pub max_armour: f64,
    pub regen: bool,
    pub ammo: i64,
    pub reserve: i64,
    pub mag_size: i64,
    pub reloading: bool,
    pub reload_progress: f64,
    pub weapon_name: String,
    pub fire_mode: String,
    pub lethal_count: i64,
    pub tactical_count: i64,
    pub move_amount: f64,
    pub sprint: bool,
    pub crouch: bool,
    pub ads: bool,
    pub airborne: bool,
    pub base_spread: f64,
    pub score_us: i64,
    pub score_them: i64,
    pub time_left: f64,
    pub mode: String,
    /// `true` when no player/weapons subsystem is driving the HUD (a
    /// scripted debug timeline owns the numbers instead).
    pub simulate: bool,
    pub time: f64,
}

impl Default for HudState {
    fn default() -> Self {
        HudState {
            health: 100.0,
            max_health: 100.0,
            armour: 0.0,
            max_armour: 150.0,
            regen: false,
            ammo: 30,
            reserve: 210,
            mag_size: 30,
            reloading: false,
            reload_progress: 0.0,
            weapon_name: "M4A1".to_string(),
            fire_mode: "AUTO".to_string(),
            lethal_count: 2,
            tactical_count: 1,
            move_amount: 0.0,
            sprint: false,
            crouch: false,
            ads: false,
            airborne: false,
            base_spread: 5.5,
            score_us: 0,
            score_them: 0,
            time_left: 600.0,
            mode: "TDM".to_string(),
            simulate: false,
            time: 0.0,
        }
    }
}

/// Movement-derived reticle bloom, for when nothing else supplies `move`
/// directly — `index.js:456-463`.
pub fn movement_bloom(current: f64, prev_pos: [f32; 3], pos: [f32; 3], dt: f64, raw_dt: f64) -> f64 {
    let dx = (pos[0] - prev_pos[0]) as f64;
    let dz = (pos[2] - prev_pos[2]) as f64;
    let speed = if dt > 0.0 { dx.hypot(dz) / dt } else { 0.0 };
    damp(current, clamp01(speed / 6.2), 12.0, raw_dt.max(1e-3))
}

/// Camera heading in degrees, 0 = north, clockwise — `index.js:497`, from the
/// camera's world-space forward XZ (already unit-length).
pub fn camera_heading_deg(forward_x: f64, forward_z: f64) -> f64 {
    forward_x.atan2(-forward_z).to_degrees()
}

/// One objective's compass bearing from the player position —
/// `index.js:567-582`'s `_buildCompassObjectives`.
pub fn compass_bearing(objective_xz: (f32, f32), player_xz: (f32, f32)) -> f64 {
    let dx = (objective_xz.0 - player_xz.0) as f64;
    let dz = (objective_xz.1 - player_xz.1) as f64;
    dx.atan2(-dz).to_degrees()
}

/// The camera-space right/forward basis [`Hud::late_update`] needs for the
/// directional damage arcs and the compass heading — the XZ projection of
/// `camera.matrixWorld`'s columns 0 and 2, already normalised
/// (`index.js:486-496`).
#[derive(Debug, Clone, Copy, Default)]
pub struct CameraBasis {
    pub right_x: f64,
    pub right_z: f64,
    pub forward_x: f64,
    pub forward_z: f64,
}

/// Everything [`Hud::late_update`] would otherwise reach through `ctx.peek`
/// for — every field optional, exactly as the source's duck typing.
#[derive(Default)]
pub struct FramePull<'a> {
    pub weapon: Option<WeaponPull>,
    pub player: Option<PlayerPull>,
    pub blips: &'a [Blip],
    pub objectives: &'a [Objective],
}

/// `index.js:63-613`'s `UiSystem`, minus its `Subsystem`/`ctx` binding — see
/// the module docs.
pub struct Hud {
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

    pub k: f64,
    pub vw: f64,
    pub vh: f64,
    hud_visible: f64,
    hud_target: f64,
    prev_pos: [f32; 3],
    regen_timer: f64,
}

impl Hud {
    pub fn new(rng: crate::rng::Rng) -> Self {
        Hud {
            state: HudState::default(),
            crosshair: Crosshair::new(),
            hit: Hitmarkers::new((0..10).map(|_| ()).collect()),
            arcs: DamageArcs::new((0..6).map(|_| ()).collect()),
            health: HealthFx::new(),
            ammo: AmmoPanel::new(),
            killfeed: Killfeed::new((0..6).map(|_| ()).collect()),
            compass: Compass::new(),
            markers: WorldMarkers::new((0..6).map(|_| ()).collect(), 4, 16, rng),
            prompt: Prompt::new(),
            banner: Banner::new(),
            menu: PauseMenu::new(),
            k: 1.0,
            vw: 1920.0,
            vh: 1080.0,
            hud_visible: 1.0,
            hud_target: 1.0,
            prev_pos: [0.0; 3],
            regen_timer: 0.0,
        }
    }

    /// `resize(w, h, ctx)` — `index.js:584-592`.
    pub fn resize(&mut self, w: f64, h: f64) {
        self.vw = w;
        self.vh = h;
        self.k = style::scale_factor(h);
        self.crosshair.set_scale(self.k);
        self.compass.set_scale(self.k);
    }

    pub fn set_hud_visible(&mut self, visible: bool) {
        self.hud_target = if visible { 1.0 } else { 0.0 };
    }

    /// `hitmarker(kind)` — `index.js:291-298`. Audio (`sfx(...)`) is the
    /// source's fire-and-forget call into an optional `audio` subsystem;
    /// that subsystem is ported (see [`crate::audio`]) but not yet wired to
    /// a running `Hud`, so it is the caller's job for now — see the returned
    /// [`HitKind`] echoed back for exactly that purpose.
    pub fn hitmarker(&mut self, kind: HitKind) -> HitKind {
        self.hit.spawn(kind);
        self.crosshair.on_hit();
        kind
    }

    pub fn damage_number(&mut self, world_pos: [f32; 3], is_kill: bool) {
        self.markers.spawn_damage(world_pos, is_kill);
    }

    /// Incoming damage: arc toward the source, screen flash, reticle flinch
    /// — `index.js:305-313`.
    pub fn hurt(&mut self, amount: f64, dir_x: f64, dir_z: f64) {
        let i = clamp01(amount / 40.0);
        self.arcs.spawn(dir_x, dir_z, 0.45 + i * 0.55);
        self.health.on_damage(i);
        self.crosshair.on_flinch(0.5 + i);
        self.regen_timer = 0.0;
        self.state.regen = false;
    }

    pub fn set_prompt(&mut self, p: &PromptSpec) {
        self.prompt.set(p);
    }

    pub fn clear_prompt(&mut self) {
        self.prompt.clear();
    }

    pub fn spawn_grenade(&mut self, world_pos: [f32; 3], fuse: f64) {
        self.markers.spawn_grenade(world_pos, fuse);
    }

    /// `lateUpdate(dt, ctx)` — `index.js:401-546`, minus the minimap bake
    /// itself (requested through `UiFrame::minimap_bake_requested`, run by the
    /// host, answered with `UiCore::set_minimap_bake_done`) and the widgets'
    /// own DOM writes (a
    /// `wasm32` [`view::HudView`] applies the returned frames).
    #[allow(clippy::too_many_arguments)]
    pub fn late_update(&mut self, dt: f64, raw_dt: f64, camera: CameraBasis, pull: FramePull<'_>) -> HudFrame {
        let s = &mut self.state;
        s.time += dt;

        let menu = self.menu.update(raw_dt);

        // ---- external state ---------------------------------------------
        let ws = (!s.simulate).then_some(pull.weapon).flatten();
        if let Some(w) = &ws {
            w.name.as_ref().map(|v| s.weapon_name = v.clone());
            w.mode.as_ref().map(|v| s.fire_mode = v.clone());
            w.ammo.map(|v| s.ammo = v);
            w.reserve.map(|v| s.reserve = v);
            w.mag_size.map(|v| s.mag_size = v);
            w.reloading.map(|v| s.reloading = v);
            w.reload_progress.map(|v| s.reload_progress = v);
            w.ads.map(|v| s.ads = v);
            w.spread.map(|v| s.base_spread = 4.0 + v * 40.0);
            w.lethal_count.map(|v| s.lethal_count = v);
            w.tactical_count.map(|v| s.tactical_count = v);
        }

        let ps = (!s.simulate).then_some(pull.player).flatten();
        if let Some(p) = &ps {
            p.health.map(|v| s.health = v);
            p.max_health.map(|v| s.max_health = v);
            p.armour.map(|v| s.armour = v);
            p.regen.map(|v| s.regen = v);
            p.move_amount.map(|v| s.move_amount = v);
            p.sprint.map(|v| s.sprint = v);
            p.crouch.map(|v| s.crouch = v);
            p.ads.map(|v| s.ads = v);
            p.airborne.map(|v| s.airborne = v);
        }

        // ---- movement-derived reticle bloom -----------------------------
        let pos = ps.as_ref().and_then(|p| p.position).unwrap_or(self.prev_pos);
        if ps.is_none() && !s.simulate {
            s.move_amount = movement_bloom(s.move_amount, self.prev_pos, pos, dt, raw_dt);
        }
        self.prev_pos = pos;

        // ---- health regeneration when nobody else owns health -----------
        if ps.is_none() && !s.simulate && s.health < s.max_health {
            self.regen_timer += dt;
            if self.regen_timer > 4.5 {
                if !s.regen {
                    s.regen = true;
                    self.health.on_regen_start();
                }
                s.health = (s.health + dt * 24.0).min(s.max_health);
            }
        }

        let heading = camera_heading_deg(camera.forward_x, camera.forward_z);

        // ---- widgets -------------------------------------------------
        let hud_goal = self.hud_target * if self.menu.open { 0.15 } else { 1.0 };
        self.hud_visible = damp(self.hud_visible, hud_goal, 10.0, raw_dt);

        let crosshair = self.crosshair.update(
            dt,
            CrosshairInput {
                move_amount: s.move_amount,
                sprint: s.sprint,
                crouch: s.crouch,
                airborne: s.airborne,
                ads: s.ads,
                base_spread: Some(s.base_spread),
                hidden: false,
            },
        );
        let hit = self.hit.update(dt);
        let arcs = self.arcs.update(dt, camera.right_x, camera.right_z, camera.forward_x, camera.forward_z);
        let health = self.health.update(
            dt,
            HealthInput { health: s.health, max_health: s.max_health, armour: s.armour, max_armour: s.max_armour, regen: s.regen },
        );
        let ammo = self.ammo.update(
            dt,
            &AmmoInput {
                ammo: s.ammo,
                reserve: s.reserve,
                mag_size: s.mag_size,
                weapon_name: s.weapon_name.clone(),
                fire_mode: s.fire_mode.clone(),
                reloading: s.reloading,
                reload_progress: s.reload_progress,
                lethal_count: s.lethal_count,
                tactical_count: s.tactical_count,
                time: s.time,
            },
        );
        let killfeed = self.killfeed.update(dt);
        let match_bar = match_frame(&MatchInput { score_us: s.score_us, score_them: s.score_them, time_left: s.time_left }, &s.mode);
        let prompt = self.prompt.update(dt);
        let banner = self.banner.update(dt);

        let player_xz = (pos[0], pos[2]);
        let compass_objectives: Vec<(f64, String)> =
            pull.objectives.iter().map(|o| (compass_bearing((o.position[0], o.position[2]), player_xz), o.label.clone())).collect();
        let strip_x = self.compass.strip_offset(heading);
        let objective_ticks: Vec<(compass::ObjectiveTick, String)> =
            compass_objectives.into_iter().map(|(bearing, label)| (self.compass.objective_tick(bearing), label)).collect();

        HudFrame {
            crosshair,
            hit,
            arcs,
            health,
            ammo,
            killfeed,
            match_bar,
            prompt,
            banner,
            compass_strip_x: strip_x,
            objective_ticks,
            hud_visible: self.hud_visible,
            heading_deg: heading,
            menu,
        }
    }
}

/// Every widget's computed per-frame render state, bundled — the pure output
/// of one [`Hud::late_update`] call, which a `wasm32` view paints and a
/// native test asserts against directly.
pub struct HudFrame {
    pub crosshair: CrosshairFrame,
    pub hit: Vec<(usize, MarkerFrame)>,
    pub arcs: Vec<(usize, ArcFrame)>,
    pub health: HealthFrame,
    pub ammo: AmmoFrame,
    pub killfeed: Vec<(usize, RowFrame)>,
    pub match_bar: MatchFrame,
    pub prompt: PromptFrame,
    pub banner: BannerFrame,
    pub compass_strip_x: f64,
    pub objective_ticks: Vec<(compass::ObjectiveTick, String)>,
    pub hud_visible: f64,
    pub heading_deg: f64,
    pub menu: MenuFrame,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng() -> crate::rng::Rng {
        crate::rng::Rng::new(1)
    }

    fn basis() -> CameraBasis {
        CameraBasis { right_x: 1.0, right_z: 0.0, forward_x: 0.0, forward_z: 1.0 }
    }

    #[test]
    fn default_state_matches_the_source_defaults() {
        let s = HudState::default();
        assert_eq!(s.health, 100.0);
        assert_eq!(s.ammo, 30);
        assert_eq!(s.weapon_name, "M4A1");
        assert!(!s.simulate);
    }

    #[test]
    fn movement_bloom_ramps_up_with_speed_and_damps_toward_it() {
        let prev = [0.0, 0.0, 0.0];
        let pos = [0.0, 0.0, 3.0]; // 3m in one 1/60s tick = 180 m/s (extreme, saturates)
        let v = movement_bloom(0.0, prev, pos, 1.0 / 60.0, 1.0 / 60.0);
        assert!(v > 0.0);
    }

    #[test]
    fn camera_heading_matches_known_directions() {
        assert!((camera_heading_deg(0.0, -1.0) - 0.0).abs() < 1e-9); // facing north (-Z)
        assert!((camera_heading_deg(1.0, 0.0) - 90.0).abs() < 1e-9); // facing east (+X)
    }

    #[test]
    fn late_update_without_any_external_pull_drives_regen_after_a_delay() {
        let mut hud = Hud::new(rng());
        hud.state.health = 50.0;
        for _ in 0..(60 * 5) {
            hud.late_update(1.0 / 60.0, 1.0 / 60.0, basis(), FramePull::default());
        }
        assert!(hud.state.health > 50.0, "health should regenerate once the timer clears 4.5s");
    }

    #[test]
    fn hitmarker_also_pulses_the_crosshair() {
        let mut hud = Hud::new(rng());
        hud.hitmarker(HitKind::Kill);
        let frame = hud.late_update(1.0 / 600.0, 1.0 / 600.0, basis(), FramePull::default());
        assert!(frame.crosshair.dot_scale > 1.0);
    }

    #[test]
    fn hurt_spawns_a_directional_arc_and_resets_regen() {
        let mut hud = Hud::new(rng());
        hud.regen_timer = 10.0;
        hud.state.regen = true;
        hud.hurt(20.0, 1.0, 0.0);
        assert!(!hud.state.regen);
        assert_eq!(hud.regen_timer, 0.0);
        let frame = hud.late_update(0.01, 0.01, basis(), FramePull::default());
        assert_eq!(frame.arcs.len(), 1);
    }

    #[test]
    fn weapon_pull_overrides_ammo_state_when_not_simulating() {
        let mut hud = Hud::new(rng());
        let pull = FramePull {
            weapon: Some(WeaponPull { ammo: Some(12), reserve: Some(50), ..WeaponPull::default() }),
            ..FramePull::default()
        };
        hud.late_update(1.0 / 60.0, 1.0 / 60.0, basis(), pull);
        assert_eq!(hud.state.ammo, 12);
        assert_eq!(hud.state.reserve, 50);
    }

    #[test]
    fn simulate_flag_ignores_external_pulls() {
        let mut hud = Hud::new(rng());
        hud.state.simulate = true;
        let pull = FramePull { weapon: Some(WeaponPull { ammo: Some(1), ..WeaponPull::default() }), ..FramePull::default() };
        hud.late_update(1.0 / 60.0, 1.0 / 60.0, basis(), pull);
        assert_eq!(hud.state.ammo, 30); // untouched
    }

    #[test]
    fn resize_derives_k_from_viewport_height() {
        let mut hud = Hud::new(rng());
        hud.resize(1920.0, 2160.0);
        assert!((hud.k - 2.0).abs() < 1e-9);
        assert!((hud.crosshair.k - 2.0).abs() < 1e-9);
        assert!((hud.compass.k - 2.0).abs() < 1e-9);
    }

    #[test]
    fn compass_bearing_matches_camera_heading_convention() {
        // objective due north of the player should read bearing 0, matching
        // `camera_heading_deg`'s convention (0 = -Z).
        let b = compass_bearing((0.0, -10.0), (0.0, 0.0));
        assert!((b - 0.0).abs() < 1e-9);
    }
}
