//! Persistent state the demo app carries across ticks.

use axiom_frame::{FrameApi, FrameBuilder};
use axiom_host::{HostApi, HostBoundaryConfig, HostStepDriver, HostViewport};
use axiom_kernel::KernelApi;
use axiom_math::MathApi;
use axiom_render::RenderApi;
use axiom_resources::ResourcesApi;
use axiom_runtime::{Runtime, RuntimeConfig};
use axiom_scene::SceneApi;
use axiom_webgpu::WebGpuApi;

/// The single nanosecond-per-step the demo runs at — 1 ms ticks.
pub const FIXED_STEP_NANOS: u64 = 1_000_000;
/// Window width (logical pixels).
pub const VIEWPORT_WIDTH: u32 = 800;
/// Window height (logical pixels).
pub const VIEWPORT_HEIGHT: u32 = 600;

/// Persistent state. Per-tick scenes / resource tables are built fresh
/// inside `run_tick`; only the runtime/driver/builder need to live across
/// frames for the engine frame index, host frame sequence, and runtime
/// tick to keep monotonically increasing.
#[derive(Debug)]
pub struct AppState {
    pub kernel: KernelApi,
    pub math: MathApi,
    pub host_api: HostApi,
    pub frame_api: FrameApi,
    pub scene_api: SceneApi,
    pub resources_api: ResourcesApi,
    pub render_api: RenderApi,
    pub webgpu_api: WebGpuApi,

    pub runtime: Runtime,
    pub driver: HostStepDriver,
    pub frame_builder: FrameBuilder,
    pub viewport: HostViewport,
}

impl AppState {
    /// Construct a brand-new app state with a started runtime, a host
    /// driver in the `Visible` lifecycle, and a fresh frame builder.
    pub fn new() -> Self {
        let kernel = KernelApi::new();
        let math = MathApi::new();
        let host_api = HostApi::new();
        let frame_api = FrameApi::new();
        let scene_api = SceneApi::new();
        let resources_api = ResourcesApi::new();
        let render_api = RenderApi::new();
        let webgpu_api = WebGpuApi::new();

        let mut runtime = Runtime::new(
            RuntimeConfig::new(FIXED_STEP_NANOS).with_diagnostics_enabled(false),
        )
        .expect("runtime config is valid for the demo fixed step");
        runtime
            .initialize()
            .expect("demo runtime initialize cannot fail");
        runtime
            .start()
            .expect("demo runtime start cannot fail");

        let boundary_config = HostBoundaryConfig::new(FIXED_STEP_NANOS, 1)
            .expect("max-steps-per-frame = 1 is valid");
        let mut driver = host_api.step_driver(boundary_config);
        driver.apply_lifecycle_signal(axiom_host::HostLifecycleSignal::Started);

        let frame_builder = frame_api.frame_builder(FIXED_STEP_NANOS);

        let viewport = host_api
            .viewport(&math, VIEWPORT_WIDTH, VIEWPORT_HEIGHT, 1.0)
            .expect("demo viewport dimensions are valid");

        AppState {
            kernel,
            math,
            host_api,
            frame_api,
            scene_api,
            resources_api,
            render_api,
            webgpu_api,
            runtime,
            driver,
            frame_builder,
            viewport,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        AppState::new()
    }
}
