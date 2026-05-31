//! The browser-app-local table mapping a `HostSurfaceHandle` (kernel
//! `HandleId`) to its real browser/GPU binding.
//!
//! This registry lives **only** in the browser app. `axiom-host` owns the
//! abstract `HostSurfaceHandle` identity; the real surface/device/queue are
//! bound here, keyed by that identity. The key/slot bookkeeping is
//! deterministic and browser-free (and unit-tested on native); the real GPU
//! binding values are attached only on wasm32.

use crate::live_gpu_binding::LiveBindingState;

/// One registered surface: the stable `HostSurfaceHandle` raw id, the target
/// dimensions, and the deterministic binding lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceSlot {
    pub handle_id: u64,
    pub width: u32,
    pub height: u32,
    pub state: LiveBindingState,
}

/// The browser app's surface table. Keyed by `HostSurfaceHandle` raw id.
#[derive(Debug, Default)]
pub struct BrowserSurfaceRegistry {
    slots: Vec<SurfaceSlot>,
    /// The real GPU bindings, parallel to `slots` by `handle_id`. Present only
    /// on wasm32 so native builds never reference any GPU object.
    #[cfg(target_arch = "wasm32")]
    bindings: Vec<(u64, crate::live_gpu_binding::LiveGpuBinding)>,
}

impl BrowserSurfaceRegistry {
    pub fn new() -> Self {
        BrowserSurfaceRegistry::default()
    }

    /// Register a surface handle with its target dimensions. Idempotent on the
    /// handle id; the initial state is `SurfaceRegistered`.
    pub fn register(&mut self, handle_id: u64, width: u32, height: u32) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.handle_id == handle_id) {
            slot.width = width;
            slot.height = height;
            return;
        }
        self.slots.push(SurfaceSlot {
            handle_id,
            width,
            height,
            state: LiveBindingState::SurfaceRegistered,
        });
    }

    /// The slot for a handle id, if registered.
    pub fn slot(&self, handle_id: u64) -> Option<SurfaceSlot> {
        self.slots.iter().copied().find(|s| s.handle_id == handle_id)
    }

    /// The deterministic binding state for a handle id (`Unbound` if unknown).
    pub fn state(&self, handle_id: u64) -> LiveBindingState {
        self.slot(handle_id)
            .map(|s| s.state)
            .unwrap_or(LiveBindingState::Unbound)
    }

    /// Advance a registered handle's state on a successful init step.
    pub fn advance(&mut self, handle_id: u64) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.handle_id == handle_id) {
            slot.state = slot.state.next_on_success();
        }
    }

    /// Mark a registered handle's binding as failed.
    pub fn fail(&mut self, handle_id: u64) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.handle_id == handle_id) {
            slot.state = LiveBindingState::Failed;
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Attach a real GPU binding for an already-registered handle (wasm32).
    #[cfg(target_arch = "wasm32")]
    pub fn attach_binding(
        &mut self,
        handle_id: u64,
        binding: crate::live_gpu_binding::LiveGpuBinding,
    ) {
        self.bindings.retain(|(id, _)| *id != handle_id);
        self.bindings.push((handle_id, binding));
        if let Some(slot) = self.slots.iter_mut().find(|s| s.handle_id == handle_id) {
            slot.state = LiveBindingState::SurfaceConfigured;
        }
    }

    /// The real GPU binding for a handle, if attached (wasm32).
    #[cfg(target_arch = "wasm32")]
    pub fn binding(&self, handle_id: u64) -> Option<&crate::live_gpu_binding::LiveGpuBinding> {
        self.bindings
            .iter()
            .find(|(id, _)| *id == handle_id)
            .map(|(_, b)| b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_lookup_is_deterministic() {
        let mut reg = BrowserSurfaceRegistry::new();
        reg.register(2, 800, 600);
        let slot = reg.slot(2).expect("registered");
        assert_eq!(slot.handle_id, 2);
        assert_eq!(slot.width, 800);
        assert_eq!(slot.height, 600);
        assert_eq!(slot.state, LiveBindingState::SurfaceRegistered);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn unknown_handle_is_unbound() {
        let reg = BrowserSurfaceRegistry::new();
        assert_eq!(reg.state(99), LiveBindingState::Unbound);
        assert!(reg.is_empty());
    }

    #[test]
    fn advance_walks_the_success_sequence() {
        let mut reg = BrowserSurfaceRegistry::new();
        reg.register(2, 1, 1);
        assert_eq!(reg.state(2), LiveBindingState::SurfaceRegistered);
        reg.advance(2);
        assert_eq!(reg.state(2), LiveBindingState::AdapterRequested);
        reg.advance(2);
        assert_eq!(reg.state(2), LiveBindingState::DeviceAcquired);
        reg.advance(2);
        assert_eq!(reg.state(2), LiveBindingState::SurfaceConfigured);
    }

    #[test]
    fn register_is_idempotent_on_handle_id() {
        let mut reg = BrowserSurfaceRegistry::new();
        reg.register(2, 800, 600);
        reg.register(2, 1024, 768);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.slot(2).unwrap().width, 1024);
    }

    #[test]
    fn fail_marks_failed() {
        let mut reg = BrowserSurfaceRegistry::new();
        reg.register(2, 1, 1);
        reg.fail(2);
        assert_eq!(reg.state(2), LiveBindingState::Failed);
    }
}
