use kernel::KernelClock;

// Uses KernelClock, satisfying the previous-layer-import rule, but the proof
// requires a reference to KernelResult, which never appears.
pub struct Runtime {
    clock: KernelClock,
}
