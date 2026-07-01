use kernel::KernelClock;

pub struct RuntimeApi {
    clock: KernelClock,
}

// Declared as a proof export in layer.toml, but NOT public here.
struct Runtime {
    clock: KernelClock,
}
