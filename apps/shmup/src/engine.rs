//! The frame loop and the shared context handed to every subsystem.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/core/engine.js:1-158`, minus the
//! Three.js scene/camera construction (`engine.js:28-35`, plus the camera aspect
//! updates in `resize`) — those are the renderer's business and land with the
//! render arm of the port, not with the loop.
//!
//! The Engine owns the frame loop and the shared context handed to every
//! subsystem. It does NOT know what any subsystem does — it only sequences them.
//!
//! Frame order (`engine.js:11-17`, unchanged):
//!   1. `input.begin_frame()`
//!   2. `fixed_update(FIXED_DT)` xN   — physics, deterministic gameplay
//!   3. `update(dt)`                  — animation, cameras, AI decisions
//!   4. `late_update(dt)`             — anything that must observe final transforms
//!   5. render subsystem draws
//!   6. `input.end_frame()`
//!
//! Steps 1 and 6 are marked in [`Engine::step`] and do nothing yet:
//! `core/input.js` is not ported, and inventing a stand-in for it here would put
//! input policy in the loop that is supposed to be blind to it.
//!
//! ## What the browser arm keeps
//!
//! `start()`, `stop()` and the `requestAnimationFrame` trampoline
//! (`engine.js:100-116`) are not here. They are the platform edge — a browser
//! frame source — and the loop they drive is [`Engine::step`], which the source
//! already exposes for exactly this reason ("so the capture harness can pump
//! frames by hand"). The wasm bootstrap calls `step`; so does every test; so does
//! a headless capture. One loop, several clocks.
//!
//! The root seed is likewise explicit. The source picks
//! `config.deterministic ? 0x5eed1234 : Math.random()` — the only `Math.random()`
//! in the entire game. A constructor argument makes every run reproducible and
//! puts the choice of entropy at the platform edge where it belongs.

use std::cell::{Cell, RefCell};

use axiom_kernel::Seconds;

use crate::config::{Config, FIXED_DT, FIXED_STEP, MAX_SUBSTEPS};
use crate::error::CoreError;
use crate::events::{DispatchFailure, EventBus};
use crate::registry::{Phase, Registry, SystemRef, Subsystem};
use crate::rng::Rng;

/// The source's capture seed (`engine.js:26`) — what `config.deterministic`
/// selects so a screenshot is byte-identical run to run.
pub const CAPTURE_SEED: u32 = 0x5eed_1234;

/// The id the source reserves for the drawing subsystem (`engine.js:145`).
pub const RENDER_SYSTEM_ID: &str = "render";

/// The engine clock, as the source's `this.time` object.
///
/// The scalars are `f64` because the accumulator must not drift and because a JS
/// number *is* an `f64` — narrowing here would change the stepping. The kernel
/// `Seconds` accessors are the boundary: a subsystem is handed dimensioned time,
/// never a naked float.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Time {
    /// Seconds since start, scaled.
    pub elapsed: f64,
    /// Unscaled wall-clock seconds since start.
    pub raw: f64,
    /// Last frame delta, scaled and clamped.
    pub dt: f64,
    /// Fixed step.
    pub fixed: f64,
    /// Interpolation alpha between the last two physics steps, 0..1.
    pub alpha: f64,
    pub scale: f64,
    pub frame: u64,
}

impl Time {
    fn start() -> Self {
        Time {
            fixed: FIXED_DT,
            scale: 1.0,
            ..Time::default()
        }
    }

    /// The frame delta as a dimensioned duration.
    pub fn dt_seconds(self) -> Seconds {
        Seconds::finite_or_zero(self.dt as f32)
    }

    /// Time since start as a dimensioned duration.
    pub fn elapsed_seconds(self) -> Seconds {
        Seconds::finite_or_zero(self.elapsed as f32)
    }
}

/// The shared context every subsystem is stepped with — the source's `ctx`
/// object, minus the Three.js handles.
///
/// It is a borrowed view rather than a stored struct: the engine hands out one
/// per phase, so the fields it mutates between phases (`time.alpha`, notably)
/// cannot be observed half-updated.
pub struct Ctx<'a> {
    pub config: &'a Config,
    pub events: &'a EventBus,
    pub time: &'a Time,
    /// The root random stream. Every subsystem takes `ctx.rng.borrow_mut()
    /// .fork()` once at init and never touches this again — that discipline is
    /// what keeps one system's edits from reshuffling another's sequence.
    pub rng: &'a RefCell<Rng>,
    registry: &'a Registry,
}

impl Ctx<'_> {
    /// `ctx.get(id)` — throwing lookup.
    pub fn get(&self, id: &str) -> Result<SystemRef, CoreError> {
        self.registry.get(id)
    }

    /// `ctx.peek(id)` — non-throwing lookup.
    pub fn peek(&self, id: &str) -> Option<SystemRef> {
        self.registry.peek(id)
    }

    /// `ctx.has(id)`.
    pub fn has(&self, id: &str) -> bool {
        self.registry.has(id)
    }
}

/// The frame loop.
pub struct Engine {
    pub config: Config,
    registry: Registry,
    events: EventBus,
    rng: RefCell<Rng>,
    time: Time,
    /// The fixed-step accumulator.
    ///
    /// A `Cell` because the fixed loop drains it *while* a [`Ctx`] borrowing the
    /// registry is alive; JS has no such constraint, and splitting the drain out
    /// of the loop would change when `time.alpha` becomes visible to a
    /// `fixed_update`.
    accum: Cell<f64>,
    /// Timestamp of the previous [`Engine::step`], in milliseconds — the units
    /// `performance.now()` reports in, kept so the clamp below reads the same.
    last: f64,
}

impl Engine {
    /// Build an engine. `root_seed` is the seed of the one random stream every
    /// subsystem forks from; pass [`CAPTURE_SEED`] for a reproducible capture.
    pub fn new(config: Config, root_seed: u32) -> Self {
        Engine {
            config,
            registry: Registry::new(),
            events: EventBus::new(),
            rng: RefCell::new(Rng::new(root_seed)),
            time: Time::start(),
            accum: Cell::new(0.0),
            last: 0.0,
        }
    }

    /// `engine.add(SystemClass, opts)`. Returns the shared handle rather than
    /// `this`: `Result` does not chain, and the handle is what a caller needs to
    /// reach a system by its concrete type later.
    pub fn add(&mut self, system: impl Subsystem + 'static) -> Result<SystemRef, CoreError> {
        self.registry.add(system)
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn events(&self) -> &EventBus {
        &self.events
    }

    pub fn time(&self) -> Time {
        self.time
    }

    /// Set the simulation time scale — slow-motion, hit-stop, pause. The only
    /// field of `time` the source ever writes from outside the loop.
    pub fn set_time_scale(&mut self, scale: f64) {
        self.time.scale = scale;
    }

    /// Resolve the dependency order and initialise every system in it.
    ///
    /// The source is `async` (it awaits asset loads) and logs any system whose
    /// init exceeds 50ms. Neither survives: there is nothing to await yet, and
    /// per-system timing is telemetry that belongs behind the kernel's telemetry
    /// sink rather than a `console.info` in the loop.
    pub fn init(&mut self) -> Result<(), CoreError> {
        let order = self.registry.resolve()?;
        let ctx = Ctx {
            config: &self.config,
            events: &self.events,
            time: &self.time,
            rng: &self.rng,
            registry: &self.registry,
        };
        for system in order {
            system.borrow_mut().init(&ctx)?;
        }
        Ok(())
    }

    /// Viewport changed. The source reads the size off the canvas; here the
    /// platform edge that owns the canvas passes it in.
    pub fn resize(&self, width: u32, height: u32) -> Result<Vec<DispatchFailure>, CoreError> {
        let width = width.max(1);
        let height = height.max(1);
        let systems = self.registry.with(Phase::Resize)?;
        let ctx = Ctx {
            config: &self.config,
            events: &self.events,
            time: &self.time,
            rng: &self.rng,
            registry: &self.registry,
        };
        for system in systems {
            system.borrow_mut().resize(width, height, &ctx);
        }
        Ok(self.events.emit("resize", &(width, height)))
    }

    /// Re-anchor the frame clock, so the next [`Engine::step`] measures a delta
    /// from `now` rather than from zero. The source does this in `start()`
    /// (`this._last = performance.now()`); without it the first frame after a
    /// long pause eats the 0.1s clamp.
    pub fn reset_clock(&mut self, now: f64) {
        self.last = now;
    }

    /// Advance one frame. `now` is a monotonic timestamp in **milliseconds**,
    /// the unit `performance.now()` reports in.
    pub fn step(&mut self, now: f64) -> Result<(), CoreError> {
        // Clamp so a tab-switch or a breakpoint doesn't teleport the simulation.
        let raw_dt = 0.1f64.min(0.0f64.max((now - self.last) / 1000.0));
        self.last = now;
        self.time.raw += raw_dt;
        self.time.dt = raw_dt * self.time.scale;
        self.time.elapsed += self.time.dt;
        self.time.frame += 1;

        // 1. input.begin_frame() — `core/input.js` is not ported yet.

        let fixed_systems = self.registry.with(Phase::FixedUpdate)?;
        let update_systems = self.registry.with(Phase::Update)?;
        let late_systems = self.registry.with(Phase::LateUpdate)?;
        let render_system = self
            .registry
            .peek(RENDER_SYSTEM_ID)
            .filter(|s| s.borrow().phases().contains(&Phase::Render));

        // 2. The fixed loop. Scoped so `time.alpha` can be written the instant
        //    it ends — a `fixed_update` sees the *previous* frame's alpha, which
        //    is what the source does and what an interpolating physics system
        //    reads.
        let steps = {
            let ctx = Ctx {
                config: &self.config,
                events: &self.events,
                time: &self.time,
                rng: &self.rng,
                registry: &self.registry,
            };
            self.accum.set(self.accum.get() + self.time.dt);
            let mut steps = 0u32;
            while self.accum.get() >= FIXED_DT && steps < MAX_SUBSTEPS {
                for system in &fixed_systems {
                    system.borrow_mut().fixed_update(FIXED_STEP, &ctx);
                }
                self.accum.set(self.accum.get() - FIXED_DT);
                steps += 1;
            }
            steps
        };
        if steps == MAX_SUBSTEPS {
            // Shed the backlog rather than spiral: a frame that could not keep
            // up drops the un-simulated time instead of handing the next frame
            // an even bigger debt.
            self.accum.set(0.0);
        }
        self.time.alpha = self.accum.get() / FIXED_DT;

        {
            let ctx = Ctx {
                config: &self.config,
                events: &self.events,
                time: &self.time,
                rng: &self.rng,
                registry: &self.registry,
            };
            let dt = self.time.dt_seconds();
            // 3. update
            for system in &update_systems {
                system.borrow_mut().update(dt, &ctx);
            }
            // 4. late_update
            for system in &late_systems {
                system.borrow_mut().late_update(dt, &ctx);
            }
            // 5. the render subsystem draws
            if let Some(system) = render_system {
                system.borrow_mut().render(&ctx);
            }
        }

        // 6. input.end_frame() — `core/input.js` is not ported yet.

        Ok(())
    }

    /// Tear down: dispose every system in reverse dependency order, then drop
    /// every event subscription.
    pub fn dispose(&mut self) -> Result<(), CoreError> {
        let mut order = self.registry.ordered()?;
        order.reverse();
        for system in order {
            system.borrow_mut().dispose();
        }
        self.events.clear();
        Ok(())
    }
}
