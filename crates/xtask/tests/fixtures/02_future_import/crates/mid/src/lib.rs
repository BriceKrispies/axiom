use kernel::KernelClock;
use top::TopThing; // ILLEGAL: `top` is layer index 2, above this layer (index 1).

pub struct MidThing {
    clock: KernelClock,
    top: TopThing,
}
