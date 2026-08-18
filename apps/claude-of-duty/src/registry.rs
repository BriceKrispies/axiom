//! Subsystem registry + shared context.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/core/registry.js:1-83` (the `Registry`
//! class; the `EventBus` in the same source file lives in [`crate::events`] —
//! two independent capabilities that happened to share a file).
//!
//! CONTRACT — every subsystem implements [`Subsystem`]:
//!   - `id()`            : unique. Other systems fetch it via `ctx.get(id)`.
//!   - `deps()`          : ids that must init first.
//!   - `phases()`        : which frame phases it takes part in.
//!   - `init(ctx)`       : build resources.
//!   - `fixed_update(h, ctx)` : fixed-rate (`PHYSICS_HZ`), 0..N times per frame.
//!   - `update(dt, ctx)` : variable-rate, once per frame, before render.
//!   - `late_update(dt, ctx)` : after all `update()`, before render.
//!   - `resize(w, h, ctx)`, `render(ctx)`, `dispose()`.
//!
//! Subsystems MUST NOT reach for each other directly — they go through
//! `ctx.get(id)`. That keeps the dependency graph explicit and lets agents own
//! files in isolation.
//!
//! ## Two deliberate shape changes
//!
//! **`phases()` replaces `typeof s[method] === 'function'`.** The source decides
//! at runtime which systems implement `fixedUpdate`/`update`/`lateUpdate`/
//! `resize` and caches the filtered arrays so the frame loop never re-filters.
//! Rust has no such reflection: a trait's default method is present on every
//! implementor whether or not the implementor wrote it. So each system *declares*
//! the phases it takes part in, and [`Registry::with`] filters and caches on that
//! declaration. Same cache, same per-frame cost, declaration instead of
//! introspection.
//!
//! **`Rc<RefCell<dyn Subsystem>>` is the storage.** `ctx.get(id)` hands one
//! system a live, mutable reference to another *while the frame loop is already
//! stepping systems* — trivially expressible in JS, and an aliasing violation if
//! the registry simply owned a `Vec<Box<dyn Subsystem>>`. Shared ownership with
//! a runtime borrow check is the honest translation: the frame loop borrows one
//! system at a time, and a re-entrant borrow (system A stepping system B which
//! reaches back into A) panics loudly instead of corrupting state, which is the
//! same bug JS would let you write silently.

use std::any::Any;
use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use axiom_kernel::Seconds;

use crate::engine::Ctx;
use crate::error::CoreError;

/// A registered subsystem, shared so `ctx.get(id)` can hand it out mid-frame.
pub type SystemRef = Rc<RefCell<dyn Subsystem>>;

/// The frame phases [`Registry::with`] caches an ordered list for.
///
/// `init` and `dispose` are deliberately absent: the source runs those over the
/// whole resolved order (forwards and reversed respectively), never over a
/// filtered list, so there is nothing to cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    FixedUpdate,
    Update,
    LateUpdate,
    Resize,
    Render,
}

/// A subsystem: one owned slice of the game, sequenced by the engine and blind
/// to every other subsystem except through `ctx`.
pub trait Subsystem {
    /// Unique id. `static id` in the source.
    fn id(&self) -> &'static str;

    /// Ids that must init first. `static deps` in the source.
    fn deps(&self) -> &'static [&'static str] {
        &[]
    }

    /// The phases this system takes part in. See the module docs for why this
    /// is declared rather than detected.
    fn phases(&self) -> &'static [Phase];

    /// Upcast for `ctx.get(id)` consumers that need the concrete type back —
    /// see [`downcast`]. Every implementor writes `fn as_any(&self) -> &dyn Any
    /// { self }`; Rust has no way to derive it.
    fn as_any(&self) -> &dyn Any;

    /// Build resources. The source's `init` is `async` and may await asset
    /// loads; the port's is synchronous, because Rust has no ambient event loop
    /// to await on and the loading path lands with the asset arm of the port.
    fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), CoreError> {
        let _ = ctx;
        Ok(())
    }

    /// Fixed-rate step, `0..MAX_SUBSTEPS` times per frame.
    fn fixed_update(&mut self, h: Seconds, ctx: &Ctx<'_>) {
        let _ = (h, ctx);
    }

    /// Variable-rate step, once per frame, before render.
    fn update(&mut self, dt: Seconds, ctx: &Ctx<'_>) {
        let _ = (dt, ctx);
    }

    /// After every `update`, before render.
    fn late_update(&mut self, dt: Seconds, ctx: &Ctx<'_>) {
        let _ = (dt, ctx);
    }

    /// Viewport changed.
    fn resize(&mut self, width: u32, height: u32, ctx: &Ctx<'_>) {
        let _ = (width, height, ctx);
    }

    /// Draw. Only the system registered as `"render"` is asked.
    fn render(&mut self, ctx: &Ctx<'_>) {
        let _ = ctx;
    }

    /// Free resources. Called in reverse dependency order.
    fn dispose(&mut self) {}
}

/// Borrow a registered subsystem as its concrete type.
///
/// The typed half of `ctx.get(id)`: JS gets the real object back and reads its
/// fields, so the port needs a way from a `SystemRef` to `&T`. Returns `None`
/// if `id` names a different type than the caller expected.
pub fn downcast<T: Subsystem + 'static>(system: &SystemRef) -> Option<Ref<'_, T>> {
    Ref::filter_map(system.borrow(), |s| s.as_any().downcast_ref::<T>()).ok()
}

/// The registry: insertion-ordered systems, a topological order over their
/// declared deps, and a per-phase cache of that order.
#[derive(Default)]
pub struct Registry {
    systems: Vec<SystemRef>,
    /// Parallel to `systems`. The source iterates `Map.keys()`, which is
    /// insertion order; the topological sort's tie-breaking — and therefore the
    /// init order of two independent systems — depends on it, so it is
    /// preserved rather than sorted.
    ids: Vec<&'static str>,
    index: HashMap<&'static str, usize>,
    order: RefCell<Vec<SystemRef>>,
    cache: RefCell<HashMap<Phase, Vec<SystemRef>>>,
}

impl Registry {
    pub fn new() -> Self {
        Registry::default()
    }

    /// Register a system.
    ///
    /// The source returns `this` for chaining and throws on a missing `static
    /// id` — impossible here, since [`Subsystem::id`] is a required method. The
    /// port returns the shared handle instead of `self`, so a caller that wants
    /// typed access later can keep one without going back through `get`.
    ///
    /// Diverges in one place: this clears the resolved order and the phase
    /// cache. The source does not, so a system added after a `resolve()` is
    /// silently never stepped. That never fires there (every `add` precedes
    /// `init`), but it is a defect, and reproducing a defect that nothing
    /// observes buys nothing.
    pub fn add(&mut self, system: impl Subsystem + 'static) -> Result<SystemRef, CoreError> {
        let id = system.id();
        if self.index.contains_key(id) {
            return Err(CoreError::new(format!("duplicate subsystem id \"{id}\"")));
        }
        let shared: SystemRef = Rc::new(RefCell::new(system));
        self.index.insert(id, self.systems.len());
        self.ids.push(id);
        self.systems.push(Rc::clone(&shared));
        self.order.borrow_mut().clear();
        self.invalidate();
        Ok(shared)
    }

    /// Throwing lookup — the one every subsystem uses for a hard dependency.
    pub fn get(&self, id: &str) -> Result<SystemRef, CoreError> {
        self.peek(id)
            .ok_or_else(|| CoreError::new(format!("subsystem \"{id}\" not registered")))
    }

    /// Non-throwing lookup for optional dependencies.
    pub fn peek(&self, id: &str) -> Option<SystemRef> {
        self.index.get(id).map(|&i| Rc::clone(&self.systems[i]))
    }

    pub fn has(&self, id: &str) -> bool {
        self.index.contains_key(id)
    }

    /// Topological sort over declared deps; fails on cycles or missing deps.
    ///
    /// Depth-first with a tri-state visit map, exactly as in the source:
    /// absent = unvisited, `0` = on the current path (so meeting it again is a
    /// cycle), `1` = emitted. The recursion is kept — the source's shape, and
    /// a subsystem graph is a dozen nodes deep at most.
    pub fn resolve(&self) -> Result<Vec<SystemRef>, CoreError> {
        let mut seen: HashMap<&'static str, u8> = HashMap::new();
        let mut out: Vec<SystemRef> = Vec::new();
        for &id in &self.ids {
            self.visit(id, "<root>", &mut seen, &mut out)?;
        }
        *self.order.borrow_mut() = out.clone();
        Ok(out)
    }

    fn visit(
        &self,
        id: &str,
        from: &str,
        seen: &mut HashMap<&'static str, u8>,
        out: &mut Vec<SystemRef>,
    ) -> Result<(), CoreError> {
        match seen.get(id) {
            Some(1) => return Ok(()),
            Some(_) => {
                return Err(CoreError::new(format!(
                    "dependency cycle at \"{id}\" (via {from})"
                )))
            }
            None => {}
        }
        let Some(&idx) = self.index.get(id) else {
            return Err(CoreError::new(format!(
                "\"{from}\" depends on unregistered subsystem \"{id}\""
            )));
        };
        let key = self.ids[idx];
        // Copy the dep list out before recursing: holding a `RefCell` borrow of
        // this system across the recursion would panic the moment the graph
        // touched it again, which is a different failure than the cycle error
        // the map is there to report.
        let deps = self.systems[idx].borrow().deps();
        seen.insert(key, 0);
        for d in deps {
            self.visit(d, key, seen, out)?;
        }
        seen.insert(key, 1);
        out.push(Rc::clone(&self.systems[idx]));
        Ok(())
    }

    /// The resolved order, resolving on first use. The source's `get ordered()`.
    pub fn ordered(&self) -> Result<Vec<SystemRef>, CoreError> {
        let cached = self.order.borrow().clone();
        if cached.is_empty() {
            return self.resolve();
        }
        Ok(cached)
    }

    /// Systems that take part in `phase`, in dependency order. Cached per phase,
    /// so the frame loop iterates a precomputed array rather than filtering
    /// every frame.
    pub fn with(&self, phase: Phase) -> Result<Vec<SystemRef>, CoreError> {
        if let Some(list) = self.cache.borrow().get(&phase) {
            return Ok(list.clone());
        }
        let list: Vec<SystemRef> = self
            .ordered()?
            .into_iter()
            .filter(|s| s.borrow().phases().contains(&phase))
            .collect();
        self.cache.borrow_mut().insert(phase, list.clone());
        Ok(list)
    }

    /// Drop the per-phase cache.
    pub fn invalidate(&self) {
        self.cache.borrow_mut().clear();
    }

    /// How many systems are registered. Not in the source (JS reads
    /// `Map.size`); the port needs a way to ask.
    pub fn len(&self) -> usize {
        self.systems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.systems.is_empty()
    }
}
