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
//! system              index.js (`UiSystem`)
//! ```
//!
//! ## The facade lives in [`system`], and there is only one of it
//!
//! This file used to carry a second port of `index.js` called `Hud` — the
//! source's `UiSystem` minus the `Subsystem` impl and the `ctx.get`/`ctx.peek`
//! reaches, written while no camera/input/weapons/player/ai subsystem had
//! landed. [`system::UiCore`] is the same file ported again, and a strict
//! superset: it adds the seven event subscriptions, the effect journal, the
//! killfeed/banner/objective/match/blip API, the minimap gate, the menu host
//! and the `wasm32` DOM view, and it closes exactly the `ctx` reaches `Hud`
//! existed to avoid ([`system::UiCore::set_links`], `set_camera`, `set_input`,
//! `set_clock`).
//!
//! Two ports of one file, sharing [`HudState`], [`Blip`], [`PlayerPull`] and
//! [`WeaponPull`] but owning **separate copies of all eleven widgets** and
//! **separate frame drives**, is one HUD too many. `Hud` is deleted;
//! [`system::UiCore`] is what runs, mounted by
//! [`crate::scene::wiring::hud::HudRig`].
//!
//! What is left here is the shared *vocabulary* — the four value types both
//! ports named and the facade still traffics in. They stay in this file rather
//! than moving into [`system`] because [`markers`], [`minimap`] and the
//! wiring tier all name them, and a vocabulary type that lives inside the
//! facade it feeds is a cycle waiting to be drawn.
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

/// One AI actor's compass/minimap blip — `ui.setBlips`'s element shape
/// (`index.js:337-348`). The facade stores [`system::MAX_BLIPS`] of them and
/// hands the live prefix out through [`system::UiCore::blips`]; the only
/// consumer of the list is the minimap (`minimap.js`'s `_mmState.blips`).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_matches_the_source_defaults() {
        let s = HudState::default();
        assert_eq!(s.health, 100.0);
        assert_eq!(s.ammo, 30);
        assert_eq!(s.weapon_name, "M4A1");
        assert!(!s.simulate);
    }

    /// The vocabulary types are what the facade and the widgets agree on, so
    /// the thing worth pinning here is that they still default to the source's
    /// neutral values — a blip at the origin facing north, and "no state
    /// published" for both duck-typed pulls.
    #[test]
    fn the_shared_vocabulary_defaults_to_nothing_published() {
        let b = Blip::default();
        assert_eq!((b.x, b.z, b.heading_deg), (0.0, 0.0, 0.0));
        assert!(!b.friendly);

        let w = WeaponPull::default();
        assert!(w.ammo.is_none() && w.name.is_none());

        let p = PlayerPull::default();
        assert!(p.health.is_none() && p.position.is_none());
    }
}
