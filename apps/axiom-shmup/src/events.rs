//! Minimal typed event bus. Handlers are called synchronously.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/core/registry.js:85-122` (the
//! `EventBus` class; the `Registry` in the same source file lives in
//! [`crate::registry`]).
//!
//! The source is a `Map<string, Set<fn>>` with four properties worth preserving,
//! because the frame loop and every subsystem depend on them:
//!
//! 1. **Synchronous dispatch.** `emit` runs every handler before it returns —
//!    no queue, no microtask, no ordering surprise inside a frame.
//! 2. **Unsubscribe during dispatch is safe.** The source copies the handler set
//!    before iterating, so a handler may `off()` itself or anyone else without
//!    invalidating the iteration. Note the exact consequence, which the port
//!    keeps: a handler unsubscribed mid-dispatch that was *already in the copy*
//!    still runs for this one emit.
//! 3. **One bad handler cannot kill the frame.** The source `try`/`catch`es each
//!    handler and logs.
//! 4. **Subscribing returns a way to unsubscribe.**
//!
//! ## The design, and why it is not a direct transcription
//!
//! **Handlers are `Rc<dyn Fn>` in a `Vec` keyed by id, not a `Set`.** A JS `Set`
//! dedups by function identity; Rust closures have no identity to dedup on, and
//! two closures with identical code are different values. So each subscription
//! gets a [`SubscriptionId`] at `on()` time and [`EventBus::off`] takes that id.
//! For the same reason `on()` returns the id rather than the source's `off`
//! closure: a closure that captured the bus in order to unsubscribe itself would
//! be an `Rc` cycle stored inside the very bus it points at — a leak by
//! construction. ([`EventBus::once`] does need exactly that self-reference, and
//! holds a `Weak` to avoid the cycle.)
//!
//! **The bus is `Rc<RefCell<…>>` inside and `Clone`.** Property 2 requires a
//! handler to mutate the bus while the bus is dispatching. Interior mutability is
//! what lets `emit` take `&self`, snapshot, *release the borrow*, and only then
//! call out — so a handler holding its own clone of the bus can subscribe or
//! unsubscribe freely mid-dispatch.
//!
//! **A failing handler returns `Err`; it does not panic.** This is the one place
//! the port deliberately does not transcribe the source's control flow. A JS
//! `throw` is recoverable control flow and `try`/`catch` is the right tool for
//! it. A Rust panic is a bug, and `catch_unwind` cannot catch it at all under
//! this workspace's `panic = "abort"` release profile — so a `catch_unwind`
//! transcription would be isolation that silently evaporates in the shipping
//! build. Instead a handler returns `Result<(), CoreError>` and [`EventBus::emit`]
//! **collects every failure and keeps going**, returning them to the caller. That
//! is property 3, honestly: dispatch always completes, and the failure is a value
//! the frame loop can log rather than a message swallowed into `console.error`.
//!
//! **Payloads cross as `&dyn Any`.** The source's payloads are ad-hoc object
//! literals per event type. `&dyn Any` is the closest honest Rust equivalent: any
//! `'static` payload type may be emitted, and a handler downcasts to the one it
//! expects. When the port's event vocabulary is complete this can tighten into an
//! enum; inventing that enum before the events exist would be guesswork.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use crate::error::CoreError;

/// A handler: takes the payload, reports failure rather than throwing.
pub type Handler = dyn Fn(&dyn Any) -> Result<(), CoreError>;

/// Handle to one subscription, returned by [`EventBus::on`] and consumed by
/// [`EventBus::off`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubscriptionId(u64);

/// A handler that returned `Err` during dispatch, with the event that reached
/// it. The source's `console.error("[events] handler for \"type\" threw:", err)`,
/// as a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchFailure {
    pub event: String,
    pub subscription: SubscriptionId,
    pub error: CoreError,
}

#[derive(Default)]
struct Inner {
    map: HashMap<String, Vec<(SubscriptionId, Rc<Handler>)>>,
    next_id: u64,
}

impl Inner {
    fn remove(&mut self, event: &str, id: SubscriptionId) {
        if let Some(list) = self.map.get_mut(event) {
            list.retain(|(existing, _)| *existing != id);
        }
    }
}

/// The bus. Cheap to clone — every clone is the same bus.
#[derive(Clone, Default)]
pub struct EventBus {
    inner: Rc<RefCell<Inner>>,
}

impl EventBus {
    pub fn new() -> Self {
        EventBus::default()
    }

    /// Subscribe. Returns the id to pass to [`EventBus::off`].
    pub fn on(
        &self,
        event: &str,
        handler: impl Fn(&dyn Any) -> Result<(), CoreError> + 'static,
    ) -> SubscriptionId {
        self.insert(event, Rc::new(handler))
    }

    /// Subscribe for exactly one dispatch, then unsubscribe.
    ///
    /// The wrapper has to reach back into the bus it is stored in, so it holds a
    /// `Weak` — an `Rc` here would be a reference cycle that never frees. It
    /// unsubscribes *before* calling the wrapped handler, matching the source, so
    /// a handler that re-emits its own event does not re-enter itself.
    pub fn once(
        &self,
        event: &str,
        handler: impl Fn(&dyn Any) -> Result<(), CoreError> + 'static,
    ) -> SubscriptionId {
        let id = SubscriptionId(self.reserve_id());
        let weak: Weak<RefCell<Inner>> = Rc::downgrade(&self.inner);
        let owned_event = event.to_string();
        let wrapper: Rc<Handler> = Rc::new(move |payload: &dyn Any| {
            if let Some(inner) = weak.upgrade() {
                inner.borrow_mut().remove(&owned_event, id);
            }
            handler(payload)
        });
        self.inner
            .borrow_mut()
            .map
            .entry(event.to_string())
            .or_default()
            .push((id, wrapper));
        id
    }

    /// Unsubscribe. A stale id is a no-op, exactly as `Set.delete` is.
    pub fn off(&self, event: &str, id: SubscriptionId) {
        self.inner.borrow_mut().remove(event, id);
    }

    /// Dispatch synchronously to every current handler, in subscription order.
    ///
    /// Returns the failures collected along the way — always after running every
    /// handler, never short-circuiting on the first one.
    pub fn emit(&self, event: &str, payload: &dyn Any) -> Vec<DispatchFailure> {
        // Snapshot and drop the borrow before calling out, so a handler may
        // subscribe or unsubscribe during dispatch. A handler removed by an
        // earlier handler in this same dispatch is still in this snapshot and
        // still runs — the source's `[...set]` copy behaves identically.
        let snapshot = match self.inner.borrow().map.get(event) {
            Some(list) => list.clone(),
            None => return Vec::new(),
        };
        snapshot
            .into_iter()
            .filter_map(|(id, handler)| {
                handler(payload).err().map(|error| DispatchFailure {
                    event: event.to_string(),
                    subscription: id,
                    error,
                })
            })
            .collect()
    }

    /// Drop every subscription.
    pub fn clear(&self) {
        self.inner.borrow_mut().map.clear();
    }

    /// How many handlers are subscribed to `event`. Not in the source; the port
    /// needs it to prove unsubscription actually happened.
    pub fn handler_count(&self, event: &str) -> usize {
        self.inner
            .borrow()
            .map
            .get(event)
            .map_or(0, |list| list.len())
    }

    fn reserve_id(&self) -> u64 {
        let mut inner = self.inner.borrow_mut();
        inner.next_id += 1;
        inner.next_id
    }

    fn insert(&self, event: &str, handler: Rc<Handler>) -> SubscriptionId {
        let id = SubscriptionId(self.reserve_id());
        self.inner
            .borrow_mut()
            .map
            .entry(event.to_string())
            .or_default()
            .push((id, handler));
        id
    }
}
