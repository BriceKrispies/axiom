use kernel::{KernelClock, KernelResult};

pub struct Runtime {
    clock: KernelClock,
}

impl Runtime {
    pub fn step(&self) -> KernelResult {
        KernelResult
    }
}

pub struct RuntimeScheduler {
    clock: KernelClock,
}
