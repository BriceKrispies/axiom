//! **The native headless GPU, acquired once for the process.**
//!
//! The `offscreen` arm has two capture entry points — [`crate::offscreen`] for a
//! 3D frame and [`crate::draw2d_offscreen`] for a 2D one — and both used to open
//! their own `wgpu::Instance`, enumerate their own adapter and request their own
//! device **on every call**, then destroy all three when the call returned. That
//! is wrong twice over.
//!
//! **It is wrong for the tool.** `axiom-shot` and the capture APIs pay a full
//! backend enumeration and device creation per screenshot; a caller taking a
//! series of stills pays it per still. The `repeat` parameter on
//! [`crate::offscreen::render_to_rgba`] exists precisely because that setup cost
//! had to be differenced away by any caller trying to *measure* a frame — which
//! is a workaround for this, not a feature.
//!
//! **It is wrong for the driver, and that one is measured.** The crate's offscreen
//! test suite was intermittently dying with a `STATUS_ACCESS_VIOLATION` inside
//! whichever GPU test happened to be running — roughly four runs in five. It was
//! never that test's fault: it is the create/destroy cycling of instances,
//! adapters and devices in one process that this machine's driver cannot take. On
//! this box, with every *test* harness sharing one device and only these two
//! capture entry points still cycling, twelve cycles was still enough. With them
//! sharing too, the same suite is green every run — and drops from 90 s to 4 s,
//! most of which was device setup.
//!
//! An adapter is a property of the machine, and a headless native capture has
//! exactly one machine to run on. So there is exactly one instance, one adapter
//! and one device here, created at first capture and held for the process. This
//! is not simulation state: no frame data lives here, nothing a capture returns
//! depends on how many captures preceded it, and the live browser arm
//! ([`crate::live_gpu_binding`]) already holds its device for the lifetime of the
//! page. The only thing that changed is that the *native* arm stopped throwing
//! its device away between frames.
//!
//! Absence stays honest: if the box has no adapter, [`shared`] answers `None`
//! once and both capture paths keep returning `None` exactly as they always did.

/// The one instance, adapter, device and queue every native headless capture in
/// this process shares.
pub(crate) struct NativeGpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    /// The adapter the device came from — the capture path reads its downlevel
    /// capabilities to decide the anisotropy budget.
    pub(crate) adapter: wgpu::Adapter,
    /// Held for the process's lifetime so the backend instance outlives the
    /// device taken from it.
    _instance: wgpu::Instance,
}

/// The shared native GPU, or `None` on a box with no adapter (a headless CI
/// runner without one). Acquired at most once, whichever answer it gives.
pub(crate) fn shared() -> Option<&'static NativeGpu> {
    static SHARED: std::sync::OnceLock<Option<NativeGpu>> = std::sync::OnceLock::new();
    SHARED.get_or_init(acquire).as_ref()
}

fn acquire() -> Option<NativeGpu> {
    let instance = wgpu::Instance::default();
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .ok()
    .and_then(|adapter| {
        // `TIMESTAMP_QUERY` is asked for only when this adapter already
        // advertises it, so the intersection is empty — and the request
        // bit-identical to the one the capture path has always made — on an
        // adapter without it.
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("axiom-native-headless-device"),
            required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .ok()
        .map(|(device, queue)| NativeGpu {
            device,
            queue,
            adapter,
            _instance: instance,
        })
    })
}
