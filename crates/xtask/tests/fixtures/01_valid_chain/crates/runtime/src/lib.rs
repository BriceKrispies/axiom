use kernel::KernelClock;

pub struct Runtime {
    clock: KernelClock,
}

impl Runtime {
    pub fn new() -> Self {
        Runtime { clock: KernelClock }
    }
}
