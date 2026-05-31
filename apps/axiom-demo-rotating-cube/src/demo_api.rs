//! The single public facade of the demo app: [`DemoRotatingCubeApi`].

use axiom_frame::{FrameApi, FrameBuilder};
use axiom_host::{HostApi, HostBoundaryConfig, HostLifecycleSignal, HostStepDriver, HostViewport};
use axiom_introspect::{FrameReport, IntrospectApi};
use axiom_kernel::{HandleId, MetricValue, Reflect, TelemetryMetric, TypeSchema};
use axiom_math::{MathApi, Transform};
use axiom_render::RenderApi;
use axiom_resources::ResourcesApi;
use axiom_runtime::{Runtime, RuntimeConfig, RuntimeContext, RuntimeResult, RuntimeSystem};
use axiom_webgpu::WebGpuApi;

use crate::scene_to_render_input::{VIEWPORT_HEIGHT, VIEWPORT_WIDTH};
use crate::vertical_slice::{run_vertical_slice, VerticalSliceArtifact};

/// One runtime step per tick — the demo runs at a fixed 1 ms step.
pub(crate) const FIXED_STEP_NANOS: u64 = 1_000_000;

/// How many recent frame reports the introspection facade retains.
const INTROSPECT_HISTORY: usize = 256;

/// The cube's spin: one full revolution every 360 simulation ticks, around
/// +Y, as radians. This is the *only* place the rotation is computed.
pub(crate) fn cube_spin_radians(sim_tick: u64) -> f32 {
    ((sim_tick % 360) as f32) * std::f32::consts::PI / 180.0
}

/// The runtime system that owns the cube's rotation. Each simulation step it
/// computes the spin angle from the deterministic simulation tick and emits it
/// as `cube.angle_rad`. The app reads that value back from the frame to build
/// the cube, so the value rendered and the value introspection reports are the
/// same — one source of truth, no duplicated formula, no tick fudging.
#[derive(Debug)]
struct CubeSpinSystem;

impl RuntimeSystem for CubeSpinSystem {
    fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
        let tick = ctx.step().tick();
        ctx.metric(TelemetryMetric::gauge(
            "cube.angle_rad",
            MetricValue::float(cube_spin_radians(tick.raw())),
            Some(tick),
        ));
        Ok(())
    }
}

/// The deterministic headless rotating-cube vertical-slice facade.
///
/// This is the **only** public facade of the app crate, and the only
/// place in the workspace where the `scene`, `resources`, `render`, and
/// `webgpu` modules are composed. [`Self::run_tick`] runs the entire
/// headless pipeline for one deterministic tick and returns a single
/// inspectable [`VerticalSliceArtifact`] describing every boundary.
///
/// The runtime, host driver, and frame builder are persistent across
/// ticks so engine frame indices, host frame sequences, and runtime tick
/// counts increase monotonically. The scene and resource table are
/// rebuilt deterministically each tick from the tick number alone.
#[derive(Debug)]
pub struct DemoRotatingCubeApi {
    pub(crate) math: MathApi,
    pub(crate) frame_api: FrameApi,
    pub(crate) resources_api: ResourcesApi,
    pub(crate) render_api: RenderApi,
    pub(crate) webgpu_api: WebGpuApi,

    pub(crate) runtime: Runtime,
    pub(crate) driver: HostStepDriver,
    pub(crate) frame_builder: FrameBuilder,
    pub(crate) viewport: HostViewport,

    /// The engine introspection surface, fed one report per tick so the demo
    /// is interrogable end-to-end (see [`Self::describe_frame`]).
    pub(crate) introspect: IntrospectApi,
}

impl DemoRotatingCubeApi {
    /// Construct a fresh demo with a started runtime, a host driver in the
    /// `Visible` lifecycle, and a frame builder at the demo's fixed step.
    pub fn new() -> Self {
        let math = MathApi::new();
        let host_api = HostApi::new();
        let frame_api = FrameApi::new();
        let resources_api = ResourcesApi::new();
        let render_api = RenderApi::new();
        // The headless slice stays on the deterministic recording backend.
        let webgpu_api = WebGpuApi::new_recording();

        let mut runtime = Runtime::new(
            RuntimeConfig::new(FIXED_STEP_NANOS).with_diagnostics_enabled(false),
        )
        .expect("runtime config is valid for the demo fixed step");
        runtime
            .initialize()
            .expect("demo runtime initialize cannot fail");
        runtime.start().expect("demo runtime start cannot fail");
        runtime
            .scheduler_mut()
            .register(
                HandleId::from_raw(1),
                "cube-spin",
                1,
                Box::new(CubeSpinSystem),
            )
            .expect("registering the cube spin system cannot fail");

        let boundary_config = HostBoundaryConfig::new(FIXED_STEP_NANOS, 1)
            .expect("max-steps-per-frame = 1 is valid");
        let mut driver = host_api.step_driver(boundary_config);
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);

        let frame_builder = frame_api.frame_builder(FIXED_STEP_NANOS);

        let viewport = host_api
            .viewport(&math, VIEWPORT_WIDTH, VIEWPORT_HEIGHT, 1.0)
            .expect("demo viewport dimensions are valid");

        DemoRotatingCubeApi {
            math,
            frame_api,
            resources_api,
            render_api,
            webgpu_api,
            runtime,
            driver,
            frame_builder,
            viewport,
            introspect: IntrospectApi::new(INTROSPECT_HISTORY),
        }
    }

    /// Run the full headless vertical slice for one deterministic tick.
    ///
    /// Two runs of the same tick (from a fresh `DemoRotatingCubeApi`)
    /// produce equal artifacts; tick N and tick N+60 produce different cube
    /// world transforms.
    pub fn run_tick(&mut self, tick: u64) -> VerticalSliceArtifact {
        run_vertical_slice(self, tick)
    }

    /// The recorded introspection report for a given engine frame index, if
    /// still retained in the bounded history.
    pub fn describe_frame(&self, engine_frame_index: u64) -> Option<&FrameReport> {
        self.introspect.describe_frame(engine_frame_index)
    }

    /// The most recent `n` introspection reports, in tick order.
    pub fn recent_frames(&self, n: usize) -> &[FrameReport] {
        self.introspect.recent(n)
    }

    /// A serialized snapshot of the most recent frame — the bytes an external
    /// agent would read. `None` until at least one tick has run.
    pub fn introspection_snapshot(&self) -> Option<Vec<u8>> {
        self.introspect.snapshot_bytes()
    }

    /// The reflected schemas of the component types the cube world is built
    /// from — the world describing its own shape as data an agent can read.
    pub fn component_schemas(&self) -> Vec<TypeSchema> {
        use crate::cube_world::{CameraData, LightData, RenderableData};
        vec![
            <Transform as Reflect>::SCHEMA,
            <CameraData as Reflect>::SCHEMA,
            <LightData as Reflect>::SCHEMA,
            <RenderableData as Reflect>::SCHEMA,
        ]
    }
}

impl Default for DemoRotatingCubeApi {
    fn default() -> Self {
        DemoRotatingCubeApi::new()
    }
}
