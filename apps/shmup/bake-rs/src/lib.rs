//! Rust/wasm texture baking for `apps/shmup` — AN EXPERIMENT THAT SAID NO.
//!
//! THE FINDING, so nobody has to run it again: porting the bake hot path to
//! wasm is **1.18-1.44x**, bit-exact, and not worth the toolchain.
//!
//!     node tools/wasmbench.mjs        # reproduces it in seconds
//!
//! The case for trying looked strong. `tools/bakeprofile.mjs` showed 54% of the
//! ~4 s of worker bake CPU in three functions — `fbm`, `ridge` and the loop
//! around them — all pure f64 arithmetic behind a narrow `seed -> typed arrays`
//! interface crossed once per bake, with no transcendentals in the hot path and
//! therefore a real prospect of byte-identical output. Every one of those
//! things turned out to be true. The speedup still is not there.
//!
//! WHY, and this is the part worth keeping: the noise is **gather-bound, not
//! ALU-bound**. Every `n2()` does four random-access lookups into a 4096-entry
//! table, and `fbm` at four octaves does sixteen — dependent loads at
//! unpredictable addresses. The bottleneck is memory latency, which wasm does
//! not change; and V8 already compiles monomorphic f64 loops over typed arrays
//! to near-identical machine code, so there is little left for a static
//! compiler to win. SIMD would not rescue it either: wasm128 has no gather
//! instruction, so the lookups stay scalar however the arithmetic is widened.
//!
//! What DID help, for a fraction of the effort, was splitting the bake to one
//! shard per texture set and widening the worker pool — the wall time of a
//! parallel bake is its largest shard, and the shards were far too coarse. That
//! removed the wait entirely (`fx:atlases.await` went from ~390 ms to nothing).
//!
//! This crate is kept as the evidence, not as a dependency. It is NOT in the
//! app's build, NOT in the Cargo workspace, and nothing imports it. If the
//! algorithm changes to something ALU-bound, or V8 regresses, re-run the bench
//! before believing this note.

mod noise;
pub use noise::{Rng, TileNoise};

/// A benchmark entry point, not a product one: build the noise table from a
/// seed and sum `n` fbm samples across it.
///
/// It exists to answer the question that decides whether porting the rest is
/// worth it — how much faster is this arithmetic in wasm than in JavaScript —
/// and the returned sum doubles as a bit-exactness check against the JS.
#[no_mangle]
pub extern "C" fn bench_fbm(seed: u32, n: u32, period: f64, oct: u32) -> f64 {
    let mut rng = Rng::new(seed);
    let nz = TileNoise::new(&mut rng);
    let mut acc = 0.0;
    let inv = 1.0 / n as f64;
    for i in 0..n {
        let u = i as f64 * inv;
        let v = (i as f64 * 0.61803398875) % 1.0;
        acc += nz.fbm(u, v, period, oct, 0.5);
    }
    acc
}

/// Same, for the ridged variant.
#[no_mangle]
pub extern "C" fn bench_ridge(seed: u32, n: u32, period: f64, oct: u32) -> f64 {
    let mut rng = Rng::new(seed);
    let nz = TileNoise::new(&mut rng);
    let mut acc = 0.0;
    let inv = 1.0 / n as f64;
    for i in 0..n {
        let u = i as f64 * inv;
        let v = (i as f64 * 0.61803398875) % 1.0;
        acc += nz.ridge(u, v, period, oct);
    }
    acc
}
