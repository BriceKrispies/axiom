use kernel::KernelClock;

// The publicly exported capability.
pub struct RuntimeApi {
    clock: KernelClock,
}

// Declared as a proof export in layer.toml, but NOT public here.
struct Runtime {
    clock: KernelClock,
}
