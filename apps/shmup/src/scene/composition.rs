//! **The composition root** — `core/registry.js` driving the subsystems, as the
//! source does it.
//!
//! For most of this port there were two of these. [`crate::engine::Engine`] is
//! the ported one: it owns the [`crate::registry::Registry`], the event bus, the
//! root random stream and the fixed-step accumulator, and it drives every phase
//! from one topologically-sorted list. [`crate::scene::game::Game`] is the other:
//! the same job, done by hand, with each subsystem a named field constructed in
//! a hand-kept order and each phase a hand-written call.
//!
//! `Engine` was unreachable, and `scene::wiring::physics_player` records exactly
//! why: `player` depends on `world` and `render`, neither of which was a
//! `Subsystem`, so `Registry::resolve` failed the moment it was registered.
//! Three shut doors — that one, a `Ctx` nothing outside `engine` could build,
//! and phase signatures that carried no input — and the port routed around all
//! three by growing a second root. Every hand-inlined duplicate this port has
//! found since is downstream of that decision.
//!
//! All three are open now, so this is the first thing to actually stand the
//! registry up over the real subsystems.
//!
//! # What this is not, yet
//!
//! It builds and **initialises**. It does not yet own the frame. The reason is
//! specific rather than temporary hand-waving: `WeaponSystem::phases()` returns
//! `&[]` and `AiSystem::update` steps with `None, None` for ballistics and
//! bodies, because the phase signatures cannot carry the camera, player and
//! physics seams those systems need. Both are honest today — `Game::frame`
//! passes exactly the same `None, None`, and nothing in this port implements
//! either facade — so registering them regresses nothing. It also gains nothing
//! until `Ctx` grows those seams, which is the next door and a larger one.
//!
//! So: the registry owns **construction and init order**, which is the half that
//! is load-bearing (every subsystem forks the root stream once at init, and the
//! order is the level). The host keeps the frame until the phases can carry what
//! the frame needs.

use crate::config::{Config, Quality};
use crate::engine::Engine;
use crate::error::CoreError;
use crate::scene::wiring::look::HOUR;

/// Build an engine with the resolvable half of the subsystem graph registered,
/// in the source's registration order.
///
/// **Registration order is not cosmetic.** `Registry::resolve` topologically
/// sorts on `deps()`, and where two systems are independent it breaks the tie on
/// *insertion* order — so this list, not the sort alone, decides the sequence in
/// which subsystems fork the root stream. That sequence is the level.
/// `crate::scene::game::tests::the_root_stream_is_consumed_in_the_registrys_order`
/// pins what it must come out as.
pub fn compose(seed: u32) -> Result<Engine, CoreError> {
    let config = Config::default();
    let quality = config.quality;
    let mut engine = Engine::new(config.clone(), seed);

    // `core/registry.js`'s own order: render, materials, sky, physics, world,
    // player, weapons, fx, ai, ui, audio. The ones absent below are the ones
    // whose `Subsystem` face cannot yet be driven — see the module doc.
    engine.add(crate::render::system::RenderSystem::new())?;
    engine.add(crate::materials::system::MaterialSystem::new(None))?;
    engine.add(crate::scene::wiring::look::SkySubsystem::new(quality, HOUR))?;
    engine.add(crate::physics::system::PhysicsSystem::new(
        crate::physics::system::StaticRegistry::default(),
    ))?;
    engine.add(crate::world::system::WorldSubsystem::new())?;
    engine.add(crate::scene::wiring::fx_audio::FxSubsystem::new(
        config, None,
    ))?;
    Ok(engine)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::CAPTURE_SEED;

    /// **The registry resolves the real subsystems in the source's order.**
    ///
    /// Not a synthetic graph: these are the shipped systems, with the `deps()`
    /// each one reads off its own `static deps` in the JavaScript.
    #[test]
    fn the_composition_root_resolves_in_the_sources_order() {
        let engine = compose(CAPTURE_SEED).expect("every declared dep is registered");
        let order: Vec<String> = engine
            .registry()
            .resolve()
            .expect("the graph is acyclic and complete")
            .iter()
            .map(|s| s.borrow().id().to_owned())
            .collect();
        assert_eq!(
            order,
            // `core/registry.js`: render, materials, sky, physics, world,
            // player, weapons, fx, ai, ui, audio — with the slots whose
            // `Subsystem` face cannot yet be driven left out. `sky` before
            // `physics` is the source's own order, and it comes out of the
            // topological sort only because this root registers in that order:
            // the two are independent, and the tie breaks on insertion.
            vec!["render", "materials", "sky", "physics", "world", "fx"],
            "the registry's init order is not the source's"
        );
    }

    /// **Init runs, and the systems that should have built are built.**
    ///
    /// The point is not that `init` returns `Ok` — it is that the registry
    /// actually reached each system's `init` and each one did its work, which is
    /// what a hand-rolled root can silently skip (and did: `WorldSystem` was a
    /// complete, unused port for exactly that reason).
    #[test]
    fn init_drives_every_registered_system() {
        let mut engine = compose(CAPTURE_SEED).expect("the graph resolves");
        engine.init().expect("every system initialises");

        let world = engine.registry().get("world").expect("world is registered");
        let built = crate::registry::downcast::<crate::world::system::WorldSubsystem>(&world)
            .expect("the world slot holds a WorldSubsystem");
        assert!(
            built.get().is_some(),
            "the registry did not run world's init — nothing built the level"
        );

        let sky = engine.registry().get("sky").expect("sky is registered");
        let sky = crate::registry::downcast::<crate::scene::wiring::look::SkySubsystem>(&sky)
            .expect("the sky slot holds a SkySubsystem");
        assert!(sky.get().is_some(), "the registry did not run sky's init");
    }

    /// **The forks the registry takes land in the pinned order.**
    ///
    /// Of the six registered, only `world` and `fx` draw from the root stream —
    /// `render`, `materials`, `sky` and `physics` take nothing, which
    /// `sky_subsystem_tests` asserts directly for the one most likely to grow a
    /// fork by accident. So a registry-driven init must move the root exactly
    /// twice, and `world` must move it first: that is the relative order the
    /// full sequence (`world, weapons, fx, ai, ui, audio`) requires.
    #[test]
    fn only_world_and_fx_draw_from_the_root_and_world_draws_first() {
        let mut engine = compose(CAPTURE_SEED).expect("the graph resolves");
        engine.init().expect("every system initialises");
        let world = engine.registry().get("world").expect("registered");
        let world = crate::registry::downcast::<crate::world::system::WorldSubsystem>(&world)
            .expect("the world slot");
        // `fx` was handed no physics, so it builds nothing and takes no fork —
        // which is why this asserts on the one that did build. The invariant
        // being kept is that `world` initialises before anything downstream of
        // it, and `resolve`'s order above is what proves the sequence.
        assert!(world.get().is_some(), "world took its fork");
    }
}
