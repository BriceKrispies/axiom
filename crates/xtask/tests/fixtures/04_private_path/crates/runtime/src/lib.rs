// ILLEGAL: reaches through the kernel's private `clock` module instead of using
// the public root export `kernel::KernelClock`.
use kernel::clock::KernelClock;

pub struct Runtime {
    clock: KernelClock,
}
