//! **The one GPU the crate's offscreen test suite runs on.**
//!
//! Every CPU↔GPU parity proof in this crate needs a real adapter. Until this
//! module existed, each of them acquired its own: twenty files called
//! `wgpu::Instance::default()`, and roughly fifty `#[test]`s each took a fresh
//! instance + adapter + device from one. Cycling that many in a single process
//! makes this machine's driver fall over with a `STATUS_ACCESS_VIOLATION` —
//! intermittently, and **inside whichever GPU test happens to be running when it
//! gives way**, which is never the test at fault. `cargo test -p axiom-gpu-backend
//! --lib --features offscreen` was red four runs in five, for reasons unrelated to
//! whatever had just changed.
//!
//! What the mechanism is *not*, both ruled out by measurement (recorded in
//! `docs/work-manifests/shmup-port/notes/bloom.md` §9):
//!
//! - **Not a race.** `--test-threads=1` crashes at the same rate as the default
//!   parallelism.
//! - **Not hold time.** Acquiring per test — so each device drops immediately —
//!   is exactly as flaky as holding one for a whole module. It is the *cycling*
//!   of instances/adapters/devices, not the number alive at once.
//!
//! And measured here: collapsing all ~50 test harnesses onto this fixture was
//! **not** on its own enough. The residual twelve cycles came from the two
//! *production* capture entry points, which opened and destroyed a device per
//! call; the suite stayed red four runs in five until [`crate::native_gpu`] fixed
//! that at its root. With both, the suite is green every run and takes 4 s instead
//! of 90 s — the missing 86 s was device setup.
//!
//! ## One device for the whole process, test or not
//!
//! So this fixture does not open a device of its own either: it is a loud-failure
//! wrapper over [`crate::native_gpu`], the same instance + adapter + device the
//! native capture path uses. A parity proof therefore runs on exactly the device
//! `axiom-shot` renders on, which is better provenance than a private one.
//!
//! ## It fails loudly, and that is the point
//!
//! [`TestGpu::shared`] panics if the machine has no real adapter. It does not
//! skip, and no consumer may make it skip. A parity proof that silently passes
//! when nothing ran is worse than no parity proof: it reads as a green tick while
//! proving nothing at all. (`native_gpu` answers `Option` because a *capture* on a
//! headless box is an honest absence; a *proof* on one is not.)
//!
//! ## What a consumer gets
//!
//! `device` / `queue` are `Clone` — both are handles to the same underlying
//! device — so a module that wants to keep its own harness struct clones them out
//! of the fixture instead of opening a device. Nothing about a harness's
//! assertions, tolerances or rig changes; only where its device comes from.
//!
//! [`webgl2_limited`] is the one sanctioned second device: `gbuffer`'s proof is
//! specifically that the attachment set fits inside the **WebGL2 downlevel limits
//! the live browser arm requests**, and limits are a property of a device, not of
//! an adapter. It is created once, from this same instance and adapter, so it
//! costs one more device and zero more adapters.

/// The instance + adapter + device the crate's GPU tests share — the process's
/// one native GPU, named for what a test wants from it.
pub(crate) struct TestGpu {
    /// The shared device. A handle — clone it rather than opening another.
    pub(crate) device: wgpu::Device,
    /// The queue belonging to [`Self::device`]. Also a clonable handle.
    pub(crate) queue: wgpu::Queue,
    /// Which backend actually ran, so a proof can say so (and refuse
    /// `wgpu::Backend::Noop`) without re-querying the adapter.
    pub(crate) backend: wgpu::Backend,
}

impl TestGpu {
    /// The shared fixture, or a loud failure if this box has no real adapter.
    pub(crate) fn shared() -> &'static TestGpu {
        static SHARED: std::sync::OnceLock<TestGpu> = std::sync::OnceLock::new();
        SHARED.get_or_init(|| {
            let native = native();
            TestGpu {
                device: native.device.clone(),
                queue: native.queue.clone(),
                backend: native.adapter.get_info().backend,
            }
        })
    }
}

/// A second device on the **same** instance and adapter, holding the WebGL2
/// downlevel limits the live browser arm requests.
///
/// Limits are a property of a device, so `gbuffer`'s proof — that the G-buffer's
/// attachment set fits what the browser will actually grant — cannot run on the
/// default-limits device above without ceasing to prove that. One extra device,
/// created once however many tests want it; no extra instance and no extra
/// adapter, which is what the crash counts.
pub(crate) struct Webgl2LimitedGpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) limits: wgpu::Limits,
}

/// The shared WebGL2-limited device, created on first use and never again.
pub(crate) fn webgl2_limited() -> &'static Webgl2LimitedGpu {
    static SHARED: std::sync::OnceLock<Webgl2LimitedGpu> = std::sync::OnceLock::new();
    SHARED.get_or_init(|| {
        let limits = wgpu::Limits::downlevel_webgl2_defaults();
        let (device, queue) =
            pollster::block_on(native().adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("axiom-gpu-backend-test-device-webgl2-limits"),
                required_features: wgpu::Features::empty(),
                required_limits: limits.clone(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            }))
            .expect("the adapter must yield a device under the browser arm's downlevel limits");
        Webgl2LimitedGpu {
            device,
            queue,
            limits,
        }
    })
}

/// Run `work` inside a **validation error scope** on the shared device, and
/// return what it produced together with the first validation error it raised.
///
/// This exists because a `wgpu` error scope is a property of the **device**, not
/// of the thread that pushed it: `push_error_scope` / `pop_error_scope` drive one
/// stack per device. While every harness had a device of its own that was
/// invisible; the moment they share one, two tests compiling shaders in parallel
/// interleave on that single stack and steal each other's errors. It is not
/// hypothetical — it is what this suite did the first time it shared a device:
/// `surface_program::parity`'s deliberately-broken `fn broken( {` landed in
/// `noise_and_fbm_agree_with_the_cpu_evaluator_across_the_lattice`'s scope, so an
/// innocent test failed on a shader it never wrote while the test that *wanted*
/// the error popped an empty scope and failed for the opposite reason.
///
/// So the fixture that owns the shared device also owns serialized access to its
/// error-scope stack. The lock is held across push → `work` → pop, which is the
/// whole critical section; nothing else about a caller's assertions changes.
///
/// Every `push_error_scope` in this crate's tests must go through here. A raw
/// push on the shared device is the bug above, waiting.
pub(crate) fn validating<T>(device: &wgpu::Device, work: impl FnOnce() -> T) -> (T, Option<wgpu::Error>) {
    static SCOPE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A poisoned lock means some other test panicked mid-scope. That test's
    // failure is the report; this one still wants a truthful scope, so take the
    // guard rather than turning one failure into a cascade of unrelated ones.
    let guard = SCOPE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let produced = work();
    let failure = pollster::block_on(device.pop_error_scope());
    drop(guard);
    (produced, failure)
}

/// The process's native GPU, or the loud failure a proof is owed.
fn native() -> &'static crate::native_gpu::NativeGpu {
    crate::native_gpu::shared()
        .expect("a GPU parity test needs a real adapter; there is no honest fallback")
}
