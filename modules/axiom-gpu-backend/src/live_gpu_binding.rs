//! The real wgpu **swap-chain** presentation binding — wasm32 only.
//! This is the surface arm: it acquires a `wgpu` surface from the browser canvas,
//! configures it, and presents one frame per call. All the actual *rendering* —
//! pipeline, mesh/material caches, lighting uniform, instance packing, draw loop —
//! lives in the shared [`crate::scene_renderer::SceneRenderer`], which the native
//! off-screen arm ([`crate::offscreen`]) uses too, so there is a single
//! definition of how a frame is drawn (no second copy to drift from).

use wasm_bindgen::JsValue;
use web_sys::HtmlCanvasElement;

use crate::draw2d_geometry::{build_geometry, Draw2dTextureSizes};
use crate::draw2d_renderer::Draw2dRenderer;
use crate::scene_renderer::{create_depth_view, SceneRenderer};
use crate::surface_recovery::{RecoveryAction, SurfaceStatus};
use crate::upscale::UpscaleBlit;

/// The real, browser-owned GPU objects (surface + device + queue) plus the shared
/// [`SceneRenderer`]. Each frame the scene is recorded into an **intermediate
/// colour target** sized to the device tier's render resolution (with a matching
/// depth view), then the [`UpscaleBlit`] samples that target across the acquired
/// swap-chain texture — magnifying it (a capped tier) or box-filtering it back
/// down (a supersampling tier) on present.
/// The surface `config` is retained so the binding can **reconfigure and
/// re-acquire** the drawing context after a backgrounded mobile browser drops it
/// (the surface then reports `Lost`/`Outdated`) — see [`Self::render_frame`].
#[derive(Debug)]
pub struct LiveGpuBinding {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// The off-swapchain colour target the scene is rendered into (then resolved
    /// to the swapchain). Sized `render_width × render_height`, which is below
    /// the surface on a capped tier and above it on a supersampling one.
    intermediate_view: wgpu::TextureView,
    /// The depth buffer for the scene pass, sized to the intermediate target.
    depth_view: wgpu::TextureView,
    /// Presents `intermediate_view` to the swapchain with a linear upscale. Used
    /// on frames the app authored no bloom for; `post` replaces it otherwise.
    upscale: UpscaleBlit,
    /// The bloom chain, present only when the app's render look authored bloom.
    /// Its composite is itself a fullscreen triangle sampling `intermediate_view`
    /// with a linear filter, so it upscales for free and stands in for
    /// `upscale` rather than running after it.
    post: Option<crate::post_chain::PostChain>,
    /// The authored bloom parameters `post` is recorded with. Held beside the
    /// chain because the chain owns pipelines and targets (which are sized at
    /// bind) while these are the per-record tunables.
    bloom: Option<axiom_host::FrameBloom>,
    /// The authored colour grade the composite presents through, on the same
    /// terms as `bloom`. This is the live arm's half of
    /// `axiom_host::apply_frame_postprocess`: the off-screen and Canvas 2D arms
    /// run that loop over their read-back bytes, and a swap-chain frame — which
    /// never becomes bytes — folds the identical arithmetic into the composite.
    grade: Option<axiom_host::FramePostProcess>,
    renderer: SceneRenderer,
    /// The 2D quad renderer (SPEC-04), the same `Draw2dRenderer` the off-screen
    /// parity path uses. It is built for the **linear** (non-sRGB) view of the
    /// swapchain (`draw2d_format`) so a 2D present writes `linear → byte` exactly
    /// as the software Canvas 2D backend does — keeping the GPU and Canvas arms of
    /// the cascade byte-identical (the property the off-screen parity proof pins).
    draw2d: Draw2dRenderer,
    /// The non-sRGB view format the 2D pass renders through. A non-sRGB **view**
    /// of the (sRGB) swapchain texture: the bytes the 2D renderer writes are stored
    /// verbatim (no gamma encode) yet the browser still presents the texture as
    /// sRGB — so a 2D frame displays identically to the software path's
    /// `putImageData`.
    draw2d_format: wgpu::TextureFormat,
    /// The full surface (canvas display) size the 2D pass renders at — 2D is
    /// pixel-exact, so unlike the 3D scene it is not rendered at the reduced
    /// `render_*` resolution and upscaled.
    width: u32,
    height: u32,
    /// The swapchain colour format — what the *surface* was configured with, and
    /// so what decides whether the present pass encodes sRGB itself (see
    /// [`crate::surface_encode`]). Not the format the scene renders into: that is
    /// `scene_format`, derived from this one and generally sRGB even when this is
    /// not, so it is deliberately not reusable as a scene-target format.
    format: wgpu::TextureFormat,
    /// The device tier's render size at [`axiom_host::RenderScale::FULL`] — what
    /// the app asked for, after the device's own texture-dimension clamp. Every
    /// adaptive size is derived from this rather than from the previous one, so
    /// repeated scale changes cannot drift the target away from the tier.
    render_base: (u32, u32),
    /// The size the scene currently renders at.
    render_size: (u32, u32),
    /// The scale currently applied to [`Self::render_base`].
    render_scale: axiom_host::RenderScale,
    /// Whether the app's look wants a post chain, so a rebuilt target knows
    /// whether to rebuild one. Held rather than re-derived because the look's
    /// bloom/grade can be `None` per frame while the chain's existence is a
    /// bind-time property.
    wants_post: bool,
    /// **The GPU frame's own stopwatch**, present only on a device that really
    /// has `wgpu::Features::TIMESTAMP_QUERY`. The binding owns it because the
    /// measured frame spans three recorders — the scene renderer, the post
    /// chain / upscale blit, and the 2D pass — and only this type owns all of
    /// them. `None` on every WebGL2 browser and on any adapter without the
    /// feature, and then every pass records exactly what it recorded before this
    /// existed.
    clock: Option<crate::gpu_pass_clock::GpuPassClock>,
    /// Which graphics API the canvas actually committed to. Reported through
    /// [`Self::bound_backend`]: the choice used to reach an app only as a
    /// console line it had to intercept.
    backend: axiom_host::BackendKind,
    /// Whether this adapter reported the half-float colour format usable as
    /// **both** a render attachment and a sampled texture — i.e. whether this
    /// device can actually hold an HDR intermediate. Reported through
    /// [`Self::has_hdr_targets`], which is what grants
    /// `axiom_host::RenderCapability::HdrTargets` on the backend's profile.
    ///
    /// Held rather than re-derived because the adapter is dropped at the end of
    /// `initialize`, and because the answer is a bind-time property: nothing about
    /// a frame can change it.
    hdr_targets: bool,
    /// Whether this device can carry the G-buffer — held for the same reason as
    /// `hdr_targets`, and derived from the same bind-time facts: the adapter's
    /// `max_color_attachments` and `max_color_attachment_bytes_per_sample`, plus
    /// HDR itself (a `Rgba16Float` normal target is an HDR target).
    gbuffer: bool,
}

/// Translate a `wgpu` surface acquisition failure into the engine's
/// [`SurfaceStatus`], whose [`SurfaceStatus::recovery_action`] decides what to do.
/// **`?nocaps=shadows,normalmap,…` — switch a capability off from the URL.**
///
/// A frame that renders correctly on one GPU and wrongly on another is a
/// bisect, and until now there was no way to run one: the capability word is
/// resolved inside the binary from what the adapter reports, and the machine
/// showing the fault is usually a phone with no console and no debugger. This
/// is `?backend=`'s sibling — the same idea one level down. Instead of picking
/// which backend runs, it turns off one of the things that backend does, so the
/// subsystem responsible can be found in a couple of page loads by whoever is
/// holding the device.
///
/// It can only ever CLEAR bits. A capability the device did not grant cannot be
/// switched on from a query string, so this can make a frame simpler but never
/// asks for something the hardware cannot do.
///
/// wasm32 only — the URL is the platform edge, so ordinary control flow is fine
/// here, exactly as in `backend_preference`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn dropped_by_url(
    profile: axiom_host::BackendCapabilityProfile,
) -> axiom_host::BackendCapabilityProfile {
    use axiom_host::RenderCapability;
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let Some(list) = search
        .split("nocaps=")
        .nth(1)
        .map(|rest| rest.split('&').next().unwrap_or(rest))
    else {
        return profile;
    };
    let dropped = list.split(',').fold(profile, |p, name| match name {
        "shadows" => p.without(RenderCapability::Shadows),
        "normalmap" => p.without(RenderCapability::NormalMapping),
        "specular" => p.without(RenderCapability::Specular),
        "sky" => p.without(RenderCapability::Sky),
        "bloom" => p.without(RenderCapability::Bloom),
        "aerial" => p.without(RenderCapability::AerialPerspective),
        "textures" => p.without(RenderCapability::Textures),
        "hdr" => p.without(RenderCapability::HdrTargets),
        "gbuffer" => p.without(RenderCapability::GBuffer),
        "sdf" => p.without(RenderCapability::Sdf),
        _ => p,
    });
    web_sys::console::log_1(&JsValue::from_str(&format!(
        "axiom: nocaps={list:?} -> caps {:#010x} (was {:#010x})",
        dropped.bits(),
        profile.bits()
    )));
    dropped
}

/// **`?device=no-r32f,no-depth-filter,…` — render as a LESS capable device.**
///
/// The lever that makes a foreign device's frame reproducible here. See
/// [`crate::device_facts`] for the token list and for why it can only ever take
/// capability away.
///
/// wasm32 only — the URL is the platform edge, so ordinary control flow is fine
/// here, exactly as in `backend_preference`.
#[cfg(target_arch = "wasm32")]
fn impersonation_spec() -> String {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default()
        .split("device=")
        .nth(1)
        .map(|rest| rest.split('&').next().unwrap_or(rest).to_owned())
        .unwrap_or_default()
}

fn classify(error: &wgpu::SurfaceError) -> SurfaceStatus {
    match error {
        wgpu::SurfaceError::Timeout => SurfaceStatus::Timeout,
        wgpu::SurfaceError::Outdated => SurfaceStatus::Outdated,
        wgpu::SurfaceError::Lost => SurfaceStatus::Lost,
        wgpu::SurfaceError::OutOfMemory => SurfaceStatus::OutOfMemory,
        _ => SurfaceStatus::Other,
    }
}

impl LiveGpuBinding {
    /// Real GPU initialisation: pick a backend (WebGPU, else WebGL2) → surface
    /// from canvas → adapter → device/queue → configure surface → build the shared
    /// [`SceneRenderer`] (mesh + material caches, pipeline for the surface format)
    /// → depth buffer. `meshes` is `(mesh_id, 12-float vertices, indices)` and
    /// `materials` is `(material_id, width, height, RGBA8)`. Errors surface as
    /// `JsValue`.
    /// Backend selection (see docs/render-fallback.md): a browser canvas can host
    /// exactly one context type, so the backend must be chosen *before* the
    /// surface is created. `preference` decides which graphics API is bound:
    ///
    /// * `None` — **auto**: probe a WebGPU adapter+device via `navigator.gpu` (no
    ///   canvas context needed); if one is live we present through WebGPU, else we
    ///   fall back to wgpu's WebGL2 backend. This is the default the run loop uses.
    /// * `Some(BackendKind::GpuPrimary)` — **WebGPU only**: bind WebGPU, and if no
    ///   WebGPU device is available return `Err` rather than falling back — so a
    ///   caller comparing backends sees an honest failure instead of a silent
    ///   downgrade.
    /// * `Some(BackendKind::GpuFallback)` — **WebGL2 only**: skip WebGPU entirely
    ///   and bind wgpu's GL backend.
    /// * `Some(BackendKind::Canvas2d)` — never reaches here (the software arm is
    ///   selected in `axiom-windowing` before a GPU backend is built); treated as
    ///   auto for totality.
    ///
    /// The same shared [`SceneRenderer`], shaders, and instancing run unchanged on
    /// either GPU arm, since the renderer is held to `downlevel_webgl2_defaults`
    /// limits.
    #[allow(clippy::too_many_arguments)]
    pub async fn initialize(
        canvas: HtmlCanvasElement,
        width: u32,
        height: u32,
        render_width: u32,
        render_height: u32,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
        skinned_meshes: &[(u64, Vec<f32>, Vec<u32>)],
        materials: &[axiom_host::MaterialTexture],
        max_instances: u32,
        shadow_size: u32,
        tier_max_anisotropy: u16,
        look: axiom_host::FrameRenderLook,
        // The capability profile as the HOST left it, before any device answered.
        // Needed here — not just after the bind — because the scene target's
        // FORMAT is decided from it: a host that declined
        // `RenderCapability::HdrTargets` must not get a float intermediate
        // allocated behind its back, and the decision has to be made before the
        // texture exists. `GpuBackendApi::bind_canvas` folds the device's answer
        // into its own copy after this returns, with the same (idempotent) grant.
        profile: axiom_host::BackendCapabilityProfile,
        preference: Option<axiom_host::BackendKind>,
    ) -> Result<LiveGpuBinding, JsValue> {
        use axiom_host::BackendKind;
        // WebGL2-only skips the WebGPU probe; WebGPU-only forbids the GL fallback.
        let webgl2_only = matches!(preference, Some(BackendKind::GpuFallback));
        let webgpu_only = matches!(preference, Some(BackendKind::GpuPrimary));
        let width = width.max(1);
        let height = height.max(1);
        // The scene renders at the device tier's resolution (`render_size`),
        // which may be SMALLER than the swapchain (a capped high-DPR phone) or
        // LARGER (a supersampling tier — `HostDeviceProfile::render_supersample`).
        // Both directions resolve to `width × height` through the same present
        // filter, so the only ceiling that belongs here is what the device can
        // actually hold as a texture; that clamp is applied below, once the
        // device exists and its real limit is known.
        let render_width = render_width.max(1);
        let render_height = render_height.max(1);

        // Probe WebGPU *fully* — adapter AND device — on its own instance, with no
        // canvas (`navigator.gpu` needs none), so the probe never acquires the
        // canvas's one-and-only context slot. We require a working device, not
        // just an adapter: some browsers expose a WebGPU adapter whose device
        // creation then fails ("Device failed at creation"), and since a canvas
        // context type cannot be reclaimed once taken, committing the canvas on
        // adapter presence alone would strand us on a dead backend with no way
        // back to WebGL2. Only a live device commits to WebGPU.
        let webgpu = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });
        // Skip the WebGPU probe entirely when WebGL2 was explicitly requested.
        //
        // Every way this probe can fail is REPORTED. Silently discarding the reason
        // is what made a dead WebGPU device indistinguishable from a machine with no
        // WebGPU at all: the console showed only `render backend = Gl`, and the real
        // cause (e.g. Chrome failing to load `dxil.dll`, so `requestDevice` fails
        // even though an adapter is granted) was invisible without probing the page
        // by hand. The downgrade is legitimate; being unable to see WHY is not.
        let webgpu_ready = match webgl2_only {
            true => None,
            false => match webgpu
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .await
            {
                Ok(adapter) => match request_render_device(&adapter).await {
                    Ok((device, queue)) => Some((adapter, device, queue)),
                    Err(e) => {
                        web_sys::console::warn_1(&JsValue::from_str(&format!(
                            "axiom: WebGPU adapter found but device creation failed \
                             ({e}) — falling back to WebGL2. Note the WebGL2 path has \
                             no vertex-stage storage buffers, so skinned geometry is \
                             not drawn there."
                        )));
                        None
                    }
                },
                Err(e) => {
                    web_sys::console::warn_1(&JsValue::from_str(&format!(
                        "axiom: no WebGPU adapter ({e}) — falling back to WebGL2."
                    )));
                    None
                }
            },
        };

        // WebGPU if its device is live, else WebGL2 — unless WebGPU was demanded,
        // in which case a missing device is a hard error (no silent downgrade).
        // Each arm creates the surface on the instance whose backend it committed
        // to (the canvas context is acquired there), so the two never contend for
        // the one context slot.
        let (surface, adapter, device, queue) = match webgpu_ready {
            Some((adapter, device, queue)) => {
                let surface = webgpu
                    .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                    .map_err(|e| JsValue::from_str(&format!("create_surface failed: {e}")))?;
                (surface, adapter, device, queue)
            }
            None if webgpu_only => {
                return Err(JsValue::from_str(
                    "WebGPU backend requested but no WebGPU device is available",
                ));
            }
            None => {
                let gl = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                    backends: wgpu::Backends::GL,
                    ..Default::default()
                });
                let surface = gl
                    .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                    .map_err(|e| JsValue::from_str(&format!("create_surface failed: {e}")))?;
                let adapter = gl
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::HighPerformance,
                        force_fallback_adapter: false,
                        compatible_surface: Some(&surface),
                    })
                    .await
                    .map_err(|e| {
                        JsValue::from_str(&format!("no WebGPU and WebGL2 adapter failed: {e}"))
                    })?;
                let (device, queue) = request_render_device(&adapter)
                    .await
                    .map_err(|e| JsValue::from_str(&format!("request_device failed: {e}")))?;
                (surface, adapter, device, queue)
            }
        };

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        // The 2D pass renders through a NON-sRGB view of the swapchain texture so
        // its `linear → byte` writes match the software backend byte-for-byte. The
        // surface must be configured to permit that view format (an empty list
        // forbids any view whose format differs from the texture's). When the
        // surface format is already non-sRGB the view format equals it and the
        // extra entry is harmless.
        //
        // A distinct swapchain view format requires the `SURFACE_VIEW_FORMATS`
        // downlevel capability, which some WebGL2 devices lack (notably headless
        // swiftshader). Configuring a distinct view there is a hard error, so when
        // the device can't do it we drop back to the surface's own format for the
        // 2D view: the 2D pass then writes through the sRGB view (a minor colour
        // difference on that path only, on downlevel devices only) instead of
        // aborting the whole backend. This is exactly what lets the WebGL2
        // comparison pane bind on more devices rather than crash.
        let supports_view_formats = adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
        let draw2d_format = supports_view_formats
            .then(|| format.remove_srgb_suffix())
            .unwrap_or(format);
        let view_formats = (draw2d_format != format)
            .then(|| vec![draw2d_format])
            .unwrap_or_default();
        // Record which backend won AND the colour contract it committed to, so the
        // browser console (and Playwright) can confirm both at a glance. The
        // format is not cosmetic bookkeeping: the whole render chain writes
        // *linear* values and depends on the attachment's sRGB store to encode
        // them for display, so `srgb = false` means this surface needs the
        // composite's own encode (see [`crate::surface_encode`]) and `srgb = true`
        // means it must not get one. Printing the offered set beside the choice is
        // what makes a fallback legible rather than mysterious — the chosen format
        // alone cannot tell you whether an sRGB surface was available and passed
        // over, or never offered at all.
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "axiom: render backend = {:?}, surface = {:?} (srgb = {}), draw2d view = {:?}, offered = {:?}",
            adapter.get_info().backend,
            format,
            format.is_srgb(),
            draw2d_format,
            caps.formats,
        )));
        // The swapchain is a texture, and it has the same ceiling every other
        // texture does. Until the surface was *measured* this could not bite: the
        // size was a compile-time constant every app picked small enough. Now the
        // windowing layer reports the real canvas, so a large window or a high
        // device-pixel-ratio can ask for a surface the device cannot allocate —
        // and `Surface::configure` does not return an error for that, it raises a
        // wgpu validation error, which on wasm aborts the module. The whole game
        // dies at startup on exactly the devices that asked for the most pixels.
        //
        // Measured on this machine (`max_texture_dimension_2d` = 4096): a
        // 2700x3900 canvas binds, 2700x4800 panics.
        //
        // So the requested surface is a request, exactly as the render target
        // below already treats its own: clamp to what the device can hold, scale
        // both axes by the same ratio so the aspect the camera was solved for is
        // preserved, and let the browser scale the slightly smaller buffer up to
        // the element. A device with less headroom presents a little softer;
        // nothing crashes, and no app has to know its own limits.
        let surface_max = device.limits().max_texture_dimension_2d.max(1);
        let surface_longest = width.max(height).max(1);
        let surface_held = surface_longest.min(surface_max);
        let fit = |axis: u32| {
            (((axis as u64) * (surface_held as u64)) / (surface_longest as u64)).max(1) as u32
        };
        let (width, height) = (fit(width), fit(height));
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats,
        };
        surface.configure(&device, &config);

        // **Can this device hold a value above one between passes?**
        //
        // Asked of the adapter, not assumed from the arm. This engine requests
        // WebGL2 downlevel limits on *both* browser arms to hold them at
        // capability parity, and half-float render targets are not guaranteed
        // under those limits — which used to be the end of the argument, and is
        // why the post chain still tone-maps an 8-bit intermediate. But "not
        // guaranteed for the class" is not "absent on this device", and the two
        // were being conflated: an adapter that reports the format perfectly
        // usable was held to the ceiling of one that does not.
        //
        // Both usages are required. Every pass downstream of the scene samples
        // the previous target, so a render-attachment-only format would let the
        // scene pass succeed and the chain that consumes it fail — the same rule
        // `surface_encode::scene_target_format` applies to the sRGB upgrade.
        // **Every device fact this bind depends on, resolved once, here.**
        //
        // Each of these used to be read at its point of use, straight off the
        // adapter. That is why a device-class rendering fault could not be
        // reproduced anywhere but on the device: the render path depended on a
        // third input that was neither named nor suppliable. Resolving them into
        // one value makes them DATA -- see `crate::device_facts`.
        let usages = |format: wgpu::TextureFormat| {
            adapter.get_texture_format_features(format).allowed_usages
        };
        let renderable = |format: wgpu::TextureFormat| {
            usages(format).contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        };
        let depth_filterable = adapter
            .get_texture_format_features(crate::scene_renderer::DEPTH_FORMAT)
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE);
        let measured = crate::device_facts::DeviceFacts {
            hdr_renderable: renderable(wgpu::TextureFormat::Rgba16Float),
            hdr_samplable: usages(wgpu::TextureFormat::Rgba16Float)
                .contains(wgpu::TextureUsages::TEXTURE_BINDING),
            rg16float_renderable: renderable(wgpu::TextureFormat::Rg16Float),
            r32float_renderable: renderable(wgpu::TextureFormat::R32Float),
            depth_filterable,
            max_color_attachments: device.limits().max_color_attachments,
            max_color_attachment_bytes_per_sample: device
                .limits()
                .max_color_attachment_bytes_per_sample,
        };
        // `?device=` lets this machine render as a LESS capable one. The whole
        // reason the record exists — a fault that only appears on one class of
        // device has to be reproducible on the machine doing the fixing.
        let facts = measured.impersonating(&impersonation_spec());
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "axiom: device facts = {facts:?}{}",
            [" (measured)", " (IMPERSONATED via ?device=)"]
                [usize::from(facts != measured)]
        )));
        let hdr_targets =
            crate::hdr_target::device_hdr_targets(facts.hdr_renderable, facts.hdr_samplable);
        // **What this device actually resolved**, on one line, because the answer
        // decides how the frame is exposed and is otherwise invisible from
        // outside the binary. A phone reporting a dark frame and a desktop
        // reporting a correct one differ HERE or nowhere, and "the sky is right
        // and the world is black" is not a guess anyone should have to make from
        // a screenshot.
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "axiom: hdr_targets = {hdr_targets} (render_attachment = {}, texture_binding = {}), \
             caps_at_bind = {:#010x} (PRE-grant; the frame's own word is reported by \n             `scene_exposure` below), tonemap_authored = {}",
            facts.hdr_renderable,
            facts.hdr_samplable,
            profile.bits(),
            look.tonemap().is_some(),
        )));

        // **Does this frame render in high dynamic range?** Both halves, together:
        // the app authored a tone map, and the profile — the host's, with this
        // device's answer folded in by the same grant `bind_canvas` will apply —
        // carries the attachment. `None` is the 8-bit chain, and it is the answer
        // for an app that authored nothing *and* for a device that cannot, which
        // is what makes the degradation a single path rather than two.
        let tonemap = crate::hdr_target::hdr_scene_tonemap(
            look.tonemap(),
            crate::hdr_target::grant_hdr_targets(profile, hdr_targets),
        );

        // The colour format the SCENE renders into. Unlike the swap chain this is
        // our own texture, so it is sRGB whenever the device can render to and
        // sample that format — the scene is then *stored* display-encoded on every
        // arm, which is both what the shading chain assumes and what stops a dark
        // gradient banding under 8-bit linear storage. On a surface that already
        // offers sRGB (the WebGL2 arm) this is the surface format unchanged.
        //
        // Unless the tone map is on, in which case it is half-float linear
        // instead and nothing between the scene and the composite clamps.
        // Clamp the tier's requested render target to what this device can hold.
        // A supersampled tier can ask for more than the device's
        // `max_texture_dimension_2d`, and exceeding it is not a soft failure —
        // wgpu rejects the texture and the whole backend dies — so the request is
        // a request, and this is the ceiling. Clamping (rather than refusing)
        // means a device with less headroom simply supersamples less, which is
        // the same graceful shape the anisotropy clamp above has. The aspect is
        // preserved by scaling both axes by the same clamped ratio.
        let device_max = device.limits().max_texture_dimension_2d.max(1);
        let requested_longest = render_width.max(render_height).max(1);
        let held_longest = requested_longest.min(device_max);
        let hold = |axis: u32| {
            (((axis as u64) * (held_longest as u64)) / (requested_longest as u64)).max(1) as u32
        };
        let render_width = hold(render_width);
        let render_height = hold(render_height);

        // The G-buffer's own two limits, read from the device this binding got
        // rather than assumed from the arm. `device_gbuffer` also requires HDR,
        // because an `Rgba16Float` normal target IS an HDR target — asking twice
        // in two places is how the two answers drift apart.
        // **Every format the chain renders into, asked about individually.**
        //
        // `hdr_targets` alone is the wrong question here, and on exactly one
        // class of device it gives the wrong answer. It is measured from
        // `Rgba16Float`, but the prepass also writes `R32Float` and the occlusion
        // and contact chains render `Rg16Float`. On WebGL2 those are two
        // different extensions: `EXT_color_buffer_half_float` makes `Rgba16Float`
        // renderable and does NOT make `R32Float` renderable -- only
        // `EXT_color_buffer_float` does. A phone with the first and not the
        // second reports HDR, is granted a G-buffer it cannot actually hold, and
        // then renders one whose attachments never resolve. The occlusion target
        // stays at its zero clear, and zero occlusion is not "unoccluded" -- it
        // multiplies the ambient, the fill and the sun to nothing.
        //
        // So each format is asked about on its own terms. A device that can hold
        // some of them and not others has no G-buffer, and says so at the bind
        // rather than three passes later in a black frame.
        let gbuffer = crate::gbuffer::device_gbuffer(
            facts.max_color_attachments,
            facts.max_color_attachment_bytes_per_sample,
            facts.gbuffer_formats_renderable(),
        );
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "axiom: gbuffer = {gbuffer} (formats renderable = {})",
            facts.gbuffer_formats_renderable()
        )));

        let scene_format = crate::surface_encode::scene_target_format(
            format,
            adapter
                .get_texture_format_features(format.add_srgb_suffix())
                .allowed_usages,
            tonemap.is_some(),
        );

        let renderer = SceneRenderer::new(
            &device,
            &queue,
            scene_format,
            meshes,
            skinned_meshes,
            // Albedo AND the four optional maps, all on the one carrier. This
            // used to be followed by `&[]` for a parallel normal-map slice that
            // only the off-screen path ever filled — which is exactly why the
            // live browser arm had no normal maps at all. There is nothing to
            // forget to pass any more.
            materials,
            max_instances,
            shadow_size,
            // The app-authored render look — hemisphere ambient, depth fog, sky —
            // threaded from the run loop through bind, so the live render lights
            // unlit faces, recedes its horizon and paints its sky exactly as the
            // offscreen capture and the Canvas 2D fallback do.
            look,
            // Whether this device can hold the prepass's attachments AT ALL. The
            // capability word says whether the frame wants them; this says
            // whether the hardware can, and the two are different questions on
            // exactly the devices where it matters.
            facts.gbuffer_formats_renderable(),
            // Anisotropic filtering rides on an extension the WebGL2 arm may not
            // have; asking the adapter here is what lets `texture_sampling`
            // resolve a clamp that is already legal for this device rather than
            // one wgpu has to silently correct behind our back.
            //
            // The adapter answers *capability*; `tier_max_anisotropy` answers
            // *affordability*, and the sampler is held to the smaller. Capability
            // alone was not enough and the reason is specific: wgpu fills the
            // WebGPU backend's downlevel flags from `DownlevelCapabilities::default()`
            // on the stated assumption that "WebGPU is assumed to be fully
            // compliant", so `ANISOTROPIC_FILTERING` is `true` on every WebGPU
            // device including the weakest phone — it is never measured. The WebGL2
            // arm genuinely queries the extension and can answer `false` on that
            // same handset. One arm therefore took 16 taps per road pixel and the
            // other took one, on identical hardware, for a near-identical frame.
            crate::texture_sampling::device_max_anisotropy(
                adapter
                    .get_downlevel_capabilities()
                    .flags
                    .contains(wgpu::DownlevelFlags::ANISOTROPIC_FILTERING),
                tier_max_anisotropy,
            ),
        // The G-buffer and the half-resolution ambient-occlusion chain, at the
        // colour target's full allocated size. `Some` only when the device
        // granted the multiple-render-target and HDR bits `device_gbuffer`
        // measured above — a downlevel arm renders exactly the frame it did
        // before, with a 1x1 white AO bound so the shader's multiply is one.
        [None, Some((render_width, render_height))][usize::from(gbuffer)],
        );


        // **What this device actually resolved to**, beside the backend line above.
        //
        // The backend line reports the arm and its colour contract; it says nothing
        // about the two numbers that decide what the frame costs — how many pixels
        // are rendered, and how many taps each textured one takes. Both are settled
        // by negotiation (a tier, a device pixel ratio, an adapter limit, a
        // downlevel flag), so neither can be read off the source, and until now
        // neither was observable from the page at all. That gap turned "the WebGPU
        // arm is slow on this phone" into archaeology across wgpu's backends when it
        // should have been one glance at a console line.
        //
        // Printed for the same reason the offered-format set is: a negotiated
        // outcome nobody can see is a negotiated outcome nobody can check.
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "axiom: surface = {width}x{height}, render target = {render_width}x{render_height} \
             (device max {device_max}), anisotropy = {aniso} (tier cap {tier_max_anisotropy}), \
             hdr targets = {hdr_targets}, scene target = {scene_format:?}",
            aniso = crate::texture_sampling::device_max_anisotropy(
                adapter
                    .get_downlevel_capabilities()
                    .flags
                    .contains(wgpu::DownlevelFlags::ANISOTROPIC_FILTERING),
                tier_max_anisotropy,
            ),
        )));

        // The intermediate colour target the scene renders into (then resolved to
        // the swapchain), in the sRGB-preferring `scene_format`, plus
        // `TEXTURE_BINDING` so the blit can sample it. Its depth view matches it,
        // not the swapchain.
        let intermediate = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-render-target"),
            size: wgpu::Extent3d {
                width: render_width,
                height: render_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: scene_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let intermediate_view = intermediate.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = create_depth_view(&device, render_width, render_height);
        let upscale = UpscaleBlit::new(
            &device,
            format,
            &intermediate_view,
            wgpu::FilterMode::Nearest,
        );
        // The post chain, built only when the app authored something for it to do
        // — bloom **or** a colour grade. A look with neither keeps the plain blit:
        // the chain's composite is a fourth fullscreen pass that would cost every
        // plain app three extra passes a frame to produce a copy. Built at bind,
        // like the ambient and the fog, because the pipelines and the
        // half-resolution targets are sized from the surface — the same contract
        // the rest of the render look already has.
        //
        // The grade has to be able to build it on its own: an app that wants its
        // finished frame graded and nothing bloomed would otherwise fall through
        // to the blit and present ungraded, which is precisely the divergence
        // carrying the grade on the look exists to close.
        // A tone map joins bloom and the grade as a reason to build the chain, and
        // it is a *harder* reason than either: on the HDR arm the scene lives in a
        // float texture the swap chain cannot present, so the composite is the
        // only pass that brings it down to display bytes. Falling through to the
        // plain blit there would present raw radiance.
        let wants_post =
            look.bloom().is_some() | look.grade().is_some() | tonemap.is_some();
        let post = wants_post.then(|| {
            crate::post_chain::PostChain::new(
                &device,
                &queue,
                // Present target = the swap chain (which decides whether the
                // composite encodes); working targets = the scene format.
                format,
                scene_format,
                &intermediate_view,
                (render_width, render_height),
                tonemap.as_ref(),
            )
        });

        // The 2D quad renderer, built for the non-sRGB swapchain view and the full
        // canvas size. Its sprite/atlas textures are uploaded later, once the app
        // resolves them (see [`Self::set_draw2d_textures`]).
        let draw2d = Draw2dRenderer::new(&device, &queue, draw2d_format, width, height, &[]);

        // The per-pass stopwatch, built only when the device actually carries
        // `TIMESTAMP_QUERY` — which the request above asked for only because the
        // adapter already advertised it. A browser on the WebGL2 fallback gets
        // `None` here and records an untimed frame, which is what every frame in
        // this engine was before now.
        let clock = crate::gpu_pass_clock::GpuPassClock::try_new(&device, &queue);
        // The graphics API this canvas committed to, in the host's own
        // vocabulary: `GpuPrimary` is WebGPU, `GpuFallback` is WebGL2.
        let backend = [
            axiom_host::BackendKind::GpuFallback,
            axiom_host::BackendKind::GpuPrimary,
        ][usize::from(matches!(
            adapter.get_info().backend,
            wgpu::Backend::BrowserWebGpu
        ))];

        Ok(LiveGpuBinding {
            surface,
            device,
            queue,
            config,
            gbuffer,
            intermediate_view,
            depth_view,
            upscale,
            post,
            // The authored bloom parameters and colour grade the chain is
            // recorded with each frame.
            bloom: look.bloom(),
            grade: look.grade(),
            renderer,
            draw2d,
            draw2d_format,
            width,
            height,
            format,
            render_base: (render_width, render_height),
            render_size: (render_width, render_height),
            render_scale: axiom_host::RenderScale::FULL,
            wants_post,
            clock,
            backend,
            hdr_targets,
        })
    }

    /// Which graphics API this binding actually bound. Answered from
    /// `adapter.get_info().backend`, the same fact the bind-time console line
    /// prints — so an app no longer has to intercept that line to display it.
    pub fn bound_backend(&self) -> axiom_host::BackendKind {
        self.backend
    }

    /// Whether this device can hold a high-dynamic-range colour attachment, as
    /// the adapter reported it at bind — the same fact the bind-time console line
    /// prints, and what grants
    /// `axiom_host::RenderCapability::HdrTargets` on the owning backend's
    /// capability profile.
    pub fn has_hdr_targets(&self) -> bool {
        self.hdr_targets
    }

    /// Whether the bound device can carry the G-buffer
    /// ([`crate::gbuffer`]) — the depth prepass, the oct-encoded view normal,
    /// the velocity buffer and the linear depth the temporal passes read.
    pub fn has_gbuffer(&self) -> bool {
        self.gbuffer
    }

    /// The most recent **resolved** per-pass GPU timings, or the reason there
    /// are none. See [`crate::gpu_pass_timing`].
    pub fn pass_timing(&self) -> crate::gpu_pass_timing::GpuFrameTiming {
        self.clock.as_ref().map_or_else(
            || {
                crate::gpu_pass_timing::GpuFrameTiming::unavailable(
                    crate::gpu_pass_clock::ADAPTER_HAS_NO_TIMESTAMP_QUERY,
                )
            },
            crate::gpu_pass_clock::GpuPassClock::timing,
        )
    }

    /// Re-render the scene at `scale` of the device tier's render size.
    ///
    /// This is the live arm of [`axiom_host::RenderScaleController`]: the tier
    /// decides the resolution the frame would like, and this applies what the
    /// device can actually afford. Fragment cost is very nearly linear in pixels,
    /// so it is the one quality dial that trades smoothly against frame time.
    ///
    /// Rebuilding is not free — it reallocates the scene colour target, its depth
    /// buffer, the upscale blit's bind group and the whole bloom chain — which is
    /// exactly why the controller moves in a few held steps rather than on a
    /// continuous gradient. A scale that resolves to the size already in use is a
    /// no-op, so calling this every frame with an unchanged scale costs one
    /// comparison.
    ///
    /// The new size is derived from [`Self::render_base`], never from the current
    /// size: scaling the previous result would compound rounding on every change
    /// and walk the target away from the tier it is supposed to be a fraction of.
    pub fn set_render_scale(&mut self, scale: axiom_host::RenderScale) {
        let (want_w, want_h) = scale.apply(self.render_base.0, self.render_base.1);
        self.render_size = (want_w.min(self.render_base.0), want_h.min(self.render_base.1));
        self.render_scale = scale;
    }

    /// The fraction of the full-size render targets the frame currently occupies.
    fn live_fraction(&self) -> (f32, f32) {
        (
            (self.render_size.0 as f32) / (self.render_base.0.max(1) as f32),
            (self.render_size.1 as f32) / (self.render_base.1.max(1) as f32),
        )
    }

    /// The scale the scene is currently rendered at.
    pub const fn render_scale(&self) -> axiom_host::RenderScale {
        self.render_scale
    }

    /// The device pixels the scene currently renders into, before the present
    /// resolve to the swapchain.
    pub const fn render_size(&self) -> (u32, u32) {
        self.render_size
    }

    /// Acquire the next swap-chain texture, **recovering a dropped context** when
    /// the browser backgrounded the tab (a mobile-first necessity). On a
    /// `Lost`/`Outdated`/other failure the surface is reconfigured with its stored
    /// config — re-acquiring the WebGPU/WebGL context — and acquisition is retried
    /// once; a `Timeout` skips the frame; `OutOfMemory` signals a full rebuild. The
    /// returned `Ok(None)` means "skip this frame cleanly" (the context will be
    /// healthy again shortly), `Err` means the binding must be reinitialised.
    fn acquire_texture(&self) -> Result<Option<wgpu::SurfaceTexture>, JsValue> {
        match self.surface.get_current_texture() {
            Ok(frame) => Ok(Some(frame)),
            Err(error) => match classify(&error).recovery_action() {
                RecoveryAction::SkipFrame => Ok(None),
                RecoveryAction::Reconfigure => {
                    // Re-acquire the dropped drawing context, then retry once. A
                    // still-failing acquisition skips this frame; the next frame
                    // tries again from a freshly configured surface.
                    self.surface.configure(&self.device, &self.config);
                    Ok(self.surface.get_current_texture().ok())
                }
                RecoveryAction::Reinitialize => Err(JsValue::from_str(
                    "gpu surface unrecoverable (out of memory): binding needs reinitialize",
                )),
            },
        }
    }

    /// Draw + present one real frame from per-`(mesh, material)` instance batches
    /// and the frame's `lights`. The scene is recorded into the reduced-resolution
    /// intermediate target by the shared [`SceneRenderer`], then the
    /// [`UpscaleBlit`] samples it across the acquired swap-chain texture (upscaling
    /// on present). Real pixels. A frame skipped for surface recovery (see
    /// [`Self::acquire_texture`]) presents nothing and returns `Ok`.
    #[allow(clippy::too_many_arguments)]
    pub fn render_frame(
        &self,
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
        // The surface program each batch draws with, in `batches` order. Empty
        // for a frame that names no authored surface.
        programs: &[u64],
        skinned: &[crate::scene_renderer::SkinnedGpuDraw],
        clear: [f32; 4],
        sdf: Option<&axiom_host::SdfScene>,
        caps: u32,
        // The frame's camera — view, projection and their product. See
        // `SceneRenderer::record` for which pass reads which half.
        camera: axiom_host::FrameCamera,
        // The frame's surface time in seconds — explicitly supplied engine time,
        // zero for a frame whose surfaces read no clock.
        surface_time: f32,
    ) -> Result<(), JsValue> {
        let frame = match self.acquire_texture()? {
            Some(frame) => frame,
            None => return Ok(()),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // Open the measured frame. Every pass recorded below reports under this
        // index, and the index is what a caller compares against its own to see
        // how stale a (necessarily asynchronous) reading is.
        self.clock
            .as_ref()
            .map(crate::gpu_pass_clock::GpuPassClock::begin_frame);
        // Render the scene at tier resolution into the intermediate target
        // (renderer owns its own encoder + submit), gating each per-fragment feature
        // on the caller's capability mask.
        // The scene draws into the live sub-rect of a target that stays allocated
        // at full tier size — so a render-scale change is a viewport, not a
        // reallocation. Reallocating the colour target, its depth buffer and the
        // bloom chain mid-frame is tens of milliseconds on a mobile GPU, i.e. a
        // visible hitch every time the loop adapted; adapting must not itself be
        // the thing that makes the frame late.
        self.renderer.record(
            &self.device,
            &self.queue,
            &self.intermediate_view,
            &self.depth_view,
            self.render_size,
            lights,
            light_view_proj,
            batches,
            programs,
            skinned,
            clear,
            sdf,
            caps,
            camera,
            surface_time,
            self.clock.as_ref(),
        );
        // ... then upscale-blit it across the full swapchain view and present.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("axiom-upscale-encoder"),
            });
        // The post chain, when the app authored bloom or a grade: bright-pass →
        // separable blur → tonemapped, graded composite, straight into the
        // swapchain view. The composite is itself a fullscreen triangle sampling
        // the intermediate with a linear filter, so it upscales for free and
        // replaces the blit rather than following it. With neither the plain blit
        // runs, byte-for-byte as before.
        let bloomed = self.post.as_ref().map(|chain| {
            chain.record(
                &self.queue,
                &mut encoder,
                &view,
                self.bloom.as_ref(),
                self.grade.as_ref(),
                self.live_fraction(),
                (self.width, self.height),
                self.clock.as_ref(),
            )
        });
        bloomed.is_none().then(|| {
            self.upscale.record(
                &self.queue,
                &mut encoder,
                &view,
                self.live_fraction(),
                self.clock.as_ref(),
            )
        });
        // Every pass of this frame is now encoded (the scene renderer submitted
        // its own encoder first, and submissions are ordered), so the resolve
        // goes last, on this encoder.
        self.clock
            .as_ref()
            .map(|clock| clock.resolve(&mut encoder));
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        // Publish whatever finished resolving and ask for the next map. Never
        // blocks: a frame that finds nothing ready simply reports the previous
        // reading again.
        self.clock
            .as_ref()
            .map(|clock| clock.pump(&self.device));
        Ok(())
    }

    /// Upload (replacing) the CPU sprite/atlas textures the 2D sprite/text path
    /// samples, as `(texture_id, width, height, RGBA8)`. Resolved in the app
    /// (fetch/decode) and pushed here whenever the set changes — the 2D peer of the
    /// 3D material upload, kept off the per-frame path so a present uploads nothing.
    pub fn set_draw2d_textures(&mut self, textures: &[(u64, u32, u32, Vec<u8>)]) {
        self.draw2d
            .set_textures(&self.device, &self.queue, textures);
    }

    /// Present one 2D frame: walk the layer-sorted [`axiom_host::Draw2dList`] into
    /// backend-neutral quad geometry through the **covered core**
    /// ([`crate::draw2d_geometry`]) — the very geometry the off-screen parity proof
    /// validates — then draw it alpha-blended into a non-sRGB view of the acquired
    /// swap-chain texture and present. `clear` is the background colour. Recovers a
    /// dropped context exactly as [`Self::render_frame`] does. Real pixels; a frame
    /// skipped for surface recovery presents nothing and returns `Ok`.
    /// Gradient fills are the one degraded case here: their baked ramp textures
    /// (emitted by the covered core into `geometry.gradient_textures()`) are not
    /// uploaded per frame, so a gradient-filled quad samples the white fallback.
    /// Gradients are not reachable from the `@axiom/game` `Frame` surface (no
    /// gradient verb), so this never triggers for a game booted through it; a
    /// caller that authors gradients should present through the Canvas 2D arm,
    /// which reads the list's paint data directly.
    pub fn render_draw2d(
        &self,
        list: &axiom_host::Draw2dList,
        textures: &[(u64, u32, u32, Vec<u8>)],
        clear: [f32; 4],
    ) -> Result<(), JsValue> {
        let frame = match self.acquire_texture()? {
            Some(frame) => frame,
            None => return Ok(()),
        };
        // The non-sRGB view: its bytes are stored verbatim (no gamma encode), so
        // they match the software backend's linear→byte write, while the browser
        // still presents the underlying sRGB texture as sRGB.
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.draw2d_format),
            ..Default::default()
        });
        let sizes = Draw2dTextureSizes::from_textures(textures);
        let geometry = build_geometry(list, self.width, self.height, &sizes);
        // A 2D present is a whole frame of its own: it opens the measured frame,
        // records its one pass, and resolves inside `record` (that encoder is the
        // frame).
        self.clock
            .as_ref()
            .map(crate::gpu_pass_clock::GpuPassClock::begin_frame);
        self.draw2d.record(
            &self.device,
            &self.queue,
            &view,
            clear,
            &geometry,
            self.clock.as_ref(),
        );
        frame.present();
        self.clock
            .as_ref()
            .map(|clock| clock.pump(&self.device));
        Ok(())
    }

    /// Replace one cached mesh's geometry mid-loop (sliding terrain streaming).
    pub fn replace_geometry(&mut self, mesh_id: u64, vertices: &[f32], indices: &[u32]) {
        self.renderer
            .replace_geometry(&self.device, mesh_id, vertices, indices);
    }

    /// Re-upload the WHOLE mesh set, so a retained scene that registered meshes
    /// after bind has them all on the GPU (see [`crate::scene_renderer::SceneRenderer::load_meshes`]).
    pub fn load_meshes(&mut self, meshes: &[(u64, Vec<f32>, Vec<u32>)]) {
        self.renderer.load_meshes(&self.device, meshes);
    }

    /// Compile every prepared surface program onto this device — the **only**
    /// place this binding compiles a pipeline after `initialize`, and it is
    /// driven from the app's preparation task before the simulation is allowed to
    /// advance. See
    /// [`crate::scene_renderer::SceneRenderer::prepare_surfaces`].
    pub fn prepare_surfaces(
        &mut self,
        catalog: &crate::surface_program::cache::SurfaceProgramCatalog,
    ) {
        self.renderer
            .prepare_surfaces(&self.device, &self.queue, catalog);
    }
}

/// The largest render-target edge the live arm will ever ask a device for. The
/// supersampling tier multiplies the surface, and a supersample is worth nothing
/// past the point where the target stops fitting in memory on a phone; 4096 is
/// two doublings of a 1080p-class surface and is what every WebGL2-capable GPU
/// in practice reports.
const MAX_LIVE_TEXTURE_DIMENSION: u32 = 4096;

/// Request the render device + queue from an adapter, with the engine's shared
/// descriptor (`downlevel_webgl2_defaults` limits so WebGPU and WebGL2 agree).
/// Used both to *probe* WebGPU viability before committing the canvas and to
/// create the real device on the chosen backend.
///
/// One limit is deliberately raised above the downlevel defaults:
/// `max_texture_dimension_2d`, which those defaults pin to 2048 — the GLES 3.0
/// *floor*, not what any real device has. That number is the ceiling on the
/// intermediate render target, so leaving it at the floor would make a
/// supersampled tier (`HostDeviceProfile::render_supersample`) unrepresentable on
/// any surface above 1024 px. It is raised to what this adapter itself reports,
/// capped at [`MAX_LIVE_TEXTURE_DIMENSION`] and never lowered below the downlevel
/// value, so the request can never exceed what the adapter already advertises —
/// i.e. it cannot turn a working device into a failed one. Every other limit
/// stays at the downlevel value, so WebGPU and WebGL2 still agree on what the
/// renderer may use.
///
/// **One optional feature is requested, and only when the adapter already
/// advertises it**: `TIMESTAMP_QUERY`, which is what lets
/// [`crate::gpu_pass_clock`] measure per-pass GPU time. Intersecting the wanted
/// bit with `adapter.features()` is what makes that safe — on an adapter without
/// it (every WebGL2 device) the intersection is empty and this request is
/// bit-identical to the `Features::empty()` one it has always made, so asking can
/// never turn a working device into a failed one.
async fn request_render_device(
    adapter: &wgpu::Adapter,
) -> Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError> {
    let mut limits = wgpu::Limits::downlevel_webgl2_defaults();
    limits.max_texture_dimension_2d = adapter
        .limits()
        .max_texture_dimension_2d
        .min(MAX_LIVE_TEXTURE_DIMENSION)
        .max(limits.max_texture_dimension_2d);
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("axiom-live-device"),
            required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
            required_limits: limits,
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
}
