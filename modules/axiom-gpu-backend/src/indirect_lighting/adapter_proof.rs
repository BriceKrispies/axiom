//! Real-adapter proof for [`crate::indirect_lighting`]: the WGSL in
//! [`INDIRECT_LIGHTING_WGSL`] computes what the CPU reference next door
//! computes, on the device this crate actually renders on.
//!
//! The instrument is [`crate::surface_program::parity`]'s, reproduced rather
//! than shared — that module's harness is `pub(super)` and the orchestrator has
//! ruled that extracting a common one happens at composition, not mid-fan-out
//! (`notes/_wiring-queue.md`, "Extract a shared GPU parity harness"). The device
//! is [`crate::test_gpu::TestGpu::shared`], the crate's **one** adapter; opening
//! another is what crashed the driver twenty sites ago.
//!
//! # What this proves, and what it cannot
//!
//! It proves the two transcriptions agree. It does **not** prove either one
//! reads the GLSL correctly — that is what
//! `crate::indirect_lighting::tests`'s longhand second transcription and its
//! property assertions are for, and this port has measured what happens when a
//! slice relies on parity alone (ten defects in `sky/`, where one reading
//! produced both sides).
//!
//! # THE TOLERANCES BELOW ARE UNVERIFIED
//!
//! This slice was written in a wave that does not build (see
//! `docs/work-manifests/shmup-port/12-final-wave-brief.md`), so no number here
//! has been measured on hardware. They are *expectations*, derived from what
//! this crate has already measured on the same device for the same shapes:
//!
//! * [`TOLERANCE`] — plain f32 arithmetic chains, where the crate's measured
//!   figures run 4e-7 (`material_shader::masks`, one ULP) to 7.6e-6
//!   (`material_shader::uv_mode`, an `fma` contraction). Set at the middle of
//!   that band because these functions contain several `a*b + c` shapes a
//!   driver may contract.
//! * [`POW_TOLERANCE`] — the one transcendental,
//!   `pow( max( ao, 0.0 ), 1.0 + r2 * 2.0 )`. Set to the figure
//!   `crate::surface_program::parity_transcendental` **measured** for `Pow` on
//!   this device.
//!
//! The orchestrator must run this and replace both with the measured worst
//! delta plus a margin. If the real delta is more than 10x under a constant
//! here, tighten it: a tolerance looser than the hardware needs is a tolerance
//! that hides the next regression.

use crate::indirect_lighting::{
    contact_shadow, direct_light, indirect, interior_gate, multi_bounce, sample_ao,
    specular_occlusion, ssr_blend, sun_bounce, IndirectIn, IndirectUniforms,
    INDIRECT_LIGHTING_WGSL, MAX_ROOMS,
};

/// How many probes one run compares. Also the probe target's width.
const PROBES: usize = 24;

/// `copy_texture_to_buffer` requires each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// Expected absolute tolerance for the non-transcendental lanes. **Unverified**
/// — see the module header.
const TOLERANCE: f32 = 2.0e-6;

/// Expected absolute tolerance for any lane downstream of `pow`. **Unverified**
/// — see the module header. The figure is
/// `crate::surface_program::parity_transcendental::POW_TOLERANCE`.
const POW_TOLERANCE: f32 = 3.0e-5;

/// The harness's own WGSL: a full-screen triangle, the probe block, and one
/// fragment entry point per group of four lanes. Appended to
/// [`INDIRECT_LIGHTING_WGSL`], which declares everything it calls.
const HARNESS_WGSL: &str = r#"
struct Probes { items: array<vec4<f32>, 192> };

@group(0) @binding(0) var<uniform> probes: Probes;
@group(0) @binding(1) var<uniform> ind_u: AxiomIndirectU;

@vertex
fn probe_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

struct Probe {
    world_pos: vec3<f32>,
    ao_texel: f32,
    world_normal: vec3<f32>,
    roughness: f32,
    albedo: vec3<f32>,
    contact_texel: f32,
    light_dot_sun: f32,
    sun_shadow: f32,
    receive_shadow: f32,
    irradiance: vec3<f32>,
    ibl: vec3<f32>,
    radiance: vec3<f32>,
    direct_color: vec3<f32>,
    ssr: vec4<f32>,
};

fn probe_at(index: u32) -> Probe {
    let base = index * 8u;
    let p0 = probes.items[base + 0u];
    let p1 = probes.items[base + 1u];
    let p2 = probes.items[base + 2u];
    let p3 = probes.items[base + 3u];
    let p4 = probes.items[base + 4u];
    let p5 = probes.items[base + 5u];
    let p6 = probes.items[base + 6u];
    let p7 = probes.items[base + 7u];
    var p: Probe;
    p.world_pos = p0.xyz;
    p.ao_texel = p0.w;
    p.world_normal = p1.xyz;
    p.roughness = p1.w;
    p.albedo = p2.xyz;
    p.contact_texel = p2.w;
    p.light_dot_sun = p3.x;
    p.sun_shadow = p3.y;
    p.receive_shadow = p3.w;
    p.irradiance = p4.xyz;
    p.ibl = p5.xyz;
    p.radiance = p6.xyz;
    p.direct_color = p7.xyz;
    p.ssr = vec4<f32>(p4.w, p5.w, p6.w, p3.z);
    return p;
}

// The AO every consumer sees. `owSampleAO()` is called three times per fragment
// in the source and returns the same value each time; computing it once here is
// what the CPU reference does too.
fn probe_ao(p: Probe) -> f32 {
    return axiom_indirect_sample_ao(ind_u, p.ao_texel);
}

@fragment
fn probe_scalars_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = probe_at(u32(position.x));
    let ao = probe_ao(p);
    return vec4<f32>(
        ao,
        axiom_indirect_contact_shadow(ind_u, p.light_dot_sun, p.contact_texel),
        axiom_indirect_specular_occlusion(ao, p.roughness),
        axiom_indirect_interior_gate(ind_u, p.world_pos, ao),
    );
}

@fragment
fn probe_bounce_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = probe_at(u32(position.x));
    let ao = probe_ao(p);
    let bounce = axiom_indirect_multi_bounce(ao, p.albedo);
    return vec4<f32>(bounce, axiom_indirect_sun_bounce(ind_u, p.world_normal));
}

@fragment
fn probe_irradiance_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = probe_at(u32(position.x));
    let out = axiom_indirect_apply(
        ind_u, p.irradiance, p.ibl, p.radiance, p.albedo, p.roughness,
        p.world_pos, p.world_normal, probe_ao(p));
    return vec4<f32>(out.irradiance, out.indoor);
}

@fragment
fn probe_ibl_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = probe_at(u32(position.x));
    let out = axiom_indirect_apply(
        ind_u, p.irradiance, p.ibl, p.radiance, p.albedo, p.roughness,
        p.world_pos, p.world_normal, probe_ao(p));
    return vec4<f32>(out.ibl_irradiance, 0.0);
}

@fragment
fn probe_radiance_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = probe_at(u32(position.x));
    let out = axiom_indirect_apply(
        ind_u, p.irradiance, p.ibl, p.radiance, p.albedo, p.roughness,
        p.world_pos, p.world_normal, probe_ao(p));
    return vec4<f32>(axiom_indirect_ssr_blend(ind_u, out.radiance, p.roughness, p.ssr), 0.0);
}

@fragment
fn probe_direct_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = probe_at(u32(position.x));
    let lit = axiom_indirect_direct_light(
        ind_u,
        p.direct_color,
        p.receive_shadow > 0.5,
        p.sun_shadow,
        axiom_indirect_contact_shadow(ind_u, p.light_dot_sun, p.contact_texel),
        probe_ao(p),
    );
    return vec4<f32>(lit, 0.0);
}
"#;

/// One probe's inputs, in the order the WGSL's `Probe` unpacks them.
#[derive(Debug, Clone, Copy)]
struct Probe {
    world_pos: [f32; 3],
    ao_texel: f32,
    world_normal: [f32; 3],
    roughness: f32,
    albedo: [f32; 3],
    contact_texel: f32,
    light_dot_sun: f32,
    sun_shadow: f32,
    receive_shadow: f32,
    irradiance: [f32; 3],
    ibl: [f32; 3],
    radiance: [f32; 3],
    direct_color: [f32; 3],
    ssr: [f32; 4],
}

/// [`PROBES`] fragments chosen to reach every arm on both sides: the AO floor,
/// full visibility (the guard's identity), a sun-facing and a non-sun light,
/// a mirror and a fully rough surface, normals across the whole sky/ground
/// gate, a fragment inside a room, one on a facade's outer skin, one on its
/// inner skin, and world coordinates that go negative (where a `fract`-shaped
/// misreading would show, and where the level transform's two rows can be
/// caught transposed).
fn probes() -> Vec<Probe> {
    let base = Probe {
        world_pos: [0.0, 1.5, 0.0],
        ao_texel: 0.7,
        world_normal: [1.0, 0.0, 0.0],
        roughness: 0.5,
        albedo: [0.62, 0.58, 0.5],
        contact_texel: 0.4,
        light_dot_sun: 1.0,
        sun_shadow: 0.6,
        receive_shadow: 1.0,
        irradiance: [0.05, 0.05, 0.06],
        ibl: [0.4, 0.45, 0.6],
        radiance: [0.2, 0.22, 0.3],
        direct_color: [0.9, 0.85, 0.7],
        ssr: [1.0, 0.5, 0.25, 1.0],
    };
    let normals: [[f32; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, -1.0],
        [0.573_462, 0.573_462, 0.585_0],
        [-0.707_106_77, -0.5, 0.5],
    ];
    let positions: [[f32; 3]; 6] = [
        [0.0, 1.5, 0.0],      // room centre
        [5.0, 1.5, 0.0],      // the facade's outer skin, depth 0
        [4.65, 1.5, 0.0],     // its inner skin, one wall thickness in
        [-4.82, 2.2, -3.1],   // negative world coordinates, mid-feather
        [40.0, 1.5, 12.0],    // far outside every volume
        [0.0, 6.5, 0.0],      // above the roof deck
    ];
    (0..PROBES)
        .map(|index| {
            let step = index as f32 / (PROBES as f32 - 1.0);
            Probe {
                world_pos: positions[index % 6],
                // Sweeps the whole buffer range, including 0 (below the 0.25
                // floor) and 1 (the guard's identity).
                ao_texel: step,
                world_normal: normals[(index / 2) % 6],
                // 0 -> a mirror, 1 -> fully rough; crosses SSR's 0.62 cutoff and
                // its 0.14 ramp foot.
                roughness: step,
                albedo: [
                    base.albedo[0] * step,
                    base.albedo[1],
                    1.0 - base.albedo[2] * step,
                ],
                contact_texel: 1.0 - step,
                // Half the probes are the sun, half are a second directional
                // just off it — the `0.999` test's two arms.
                light_dot_sun: [0.998, 1.0][index % 2],
                sun_shadow: 1.0 - step * 0.5,
                receive_shadow: [0.0, 1.0][index % 2],
                irradiance: base.irradiance,
                ibl: base.ibl,
                radiance: base.radiance,
                direct_color: base.direct_color,
                ssr: [1.0, 0.5, 0.25, step],
            }
        })
        .collect()
}

/// The two uniform blocks a run compares against: the constructor's own state
/// (every feature off, no rooms) and a fully-lit street with one live interior
/// volume. Both are real states of the original — the first is what a tier
/// without `gtao`/`ssr` runs every frame.
fn uniform_cases() -> [(&'static str, IndirectUniforms); 2] {
    let off = IndirectUniforms::shipped();
    let mut on = off;
    on.feat = [1.0, 1.0, 1.0, 1.0];
    on.sky_fill = [0.20, 0.31, 0.55];
    on.ground_fill = [0.33, 0.29, 0.225];
    on.fill_gain = [1.0, 0.5];
    on.indirect = [0.85, 0.15, 1.0, 0.0];
    // A level authored on a yaw, so the world -> level rotation is not the
    // identity: a transposed row shows up here and nowhere else.
    on.room_xf = [0.936_293_4, 0.351_283_1, -1.25, 0.4];
    on.rooms[0] = [0.0, 0.0, 5.0, 5.0];
    on.rooms_y[0] = [-0.8, 6.0, 0.0, 0.0];
    on.sun_dir_world = [0.6, 0.4, 0.69];
    [("features off", off), ("features on", on)]
}

/// Pack [`IndirectUniforms`] exactly as the WGSL `AxiomIndirectU` lays it out:
/// nine `vec4`s then two `array<vec4<f32>, 10>`, all 16-byte aligned.
fn pack_uniforms(u: &IndirectUniforms) -> Vec<u8> {
    let mut out: Vec<f32> = Vec::with_capacity(9 * 4 + MAX_ROOMS * 8);
    out.extend_from_slice(&u.feat);
    out.extend_from_slice(&[u.ao_strength[0], u.ao_strength[1], 0.0, 0.0]);
    out.extend_from_slice(&[u.sky_fill[0], u.sky_fill[1], u.sky_fill[2], 0.0]);
    out.extend_from_slice(&[u.ground_fill[0], u.ground_fill[1], u.ground_fill[2], 0.0]);
    out.extend_from_slice(&[u.fill_gain[0], u.fill_gain[1], 0.0, 0.0]);
    out.extend_from_slice(&u.fill_dir);
    out.extend_from_slice(&u.indirect);
    out.extend_from_slice(&u.room_xf);
    out.extend_from_slice(&[
        u.sun_dir_world[0],
        u.sun_dir_world[1],
        u.sun_dir_world[2],
        0.0,
    ]);
    u.rooms.iter().for_each(|r| out.extend_from_slice(r));
    u.rooms_y.iter().for_each(|r| out.extend_from_slice(r));
    out.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Pack the probe set as the WGSL's eight `vec4`s per probe.
fn pack_probes(list: &[Probe]) -> Vec<u8> {
    let mut out: Vec<f32> = Vec::with_capacity(PROBES * 32);
    list.iter().for_each(|p| {
        out.extend_from_slice(&[p.world_pos[0], p.world_pos[1], p.world_pos[2], p.ao_texel]);
        out.extend_from_slice(&[
            p.world_normal[0],
            p.world_normal[1],
            p.world_normal[2],
            p.roughness,
        ]);
        out.extend_from_slice(&[p.albedo[0], p.albedo[1], p.albedo[2], p.contact_texel]);
        out.extend_from_slice(&[
            p.light_dot_sun,
            p.sun_shadow,
            p.ssr[3],
            p.receive_shadow,
        ]);
        out.extend_from_slice(&[p.irradiance[0], p.irradiance[1], p.irradiance[2], p.ssr[0]]);
        out.extend_from_slice(&[p.ibl[0], p.ibl[1], p.ibl[2], p.ssr[1]]);
        out.extend_from_slice(&[p.radiance[0], p.radiance[1], p.radiance[2], p.ssr[2]]);
        out.extend_from_slice(&[
            p.direct_color[0],
            p.direct_color[1],
            p.direct_color[2],
            0.0,
        ]);
    });
    out.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Render one fragment entry point over a `PROBES x 1` `Rgba32Float` target and
/// read every pixel's four lanes back.
fn render(module: &wgpu::ShaderModule, entry_point: &str, probe_bytes: &[u8], uniform_bytes: &[u8]) -> Vec<[f32; 4]> {
    let gpu = crate::test_gpu::TestGpu::shared();
    let device = &gpu.device;
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("axiom-indirect-parity-bgl"),
        entries: &[0_u32, 1]
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .to_vec(),
    });
    let buffers = [probe_bytes, uniform_bytes].map(|bytes| {
        wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("axiom-indirect-parity-uniform"),
                contents: bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        )
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-indirect-parity-bg"),
        layout: &layout,
        entries: &[0_usize, 1]
            .map(|index| wgpu::BindGroupEntry {
                binding: index as u32,
                resource: buffers[index].as_entire_binding(),
            })
            .to_vec(),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axiom-indirect-parity-pl"),
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axiom-indirect-parity-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("probe_vs"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry_point),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-indirect-parity-target"),
        size: wgpu::Extent3d {
            width: PROBES as u32,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let row_bytes = (PROBES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-indirect-parity-readback"),
        size: u64::from(row_bytes),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("axiom-indirect-parity-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: PROBES as u32,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::Wait)
        .expect("the readback must complete");
    let mapped = slice.get_mapped_range();
    (0..PROBES)
        .map(|probe| {
            [0_usize, 1, 2, 3].map(|lane| {
                let at = probe * 16 + lane * 4;
                f32::from_le_bytes([
                    mapped[at],
                    mapped[at + 1],
                    mapped[at + 2],
                    mapped[at + 3],
                ])
            })
        })
        .collect()
}

/// Compile [`INDIRECT_LIGHTING_WGSL`] plus the harness, failing with the
/// validator's own message rather than a bare panic.
fn compile() -> wgpu::ShaderModule {
    let gpu = crate::test_gpu::TestGpu::shared();
    let source = format!("{}{}", INDIRECT_LIGHTING_WGSL, HARNESS_WGSL);
    let (module, failure) = crate::test_gpu::validating(&gpu.device, || {
        gpu.device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("axiom-indirect-parity-shader"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
    });
    assert!(
        failure.is_none(),
        "the indirect-lighting WGSL must compile: {}",
        failure.map_or(String::new(), |error| error.to_string())
    );
    module
}

/// The worst absolute disagreement across a lane set, and where it was.
fn worst(cpu: &[[f32; 4]], gpu: &[[f32; 4]], lanes: &[usize]) -> (f32, usize, usize) {
    cpu.iter()
        .zip(gpu.iter())
        .enumerate()
        .flat_map(|(probe, (a, b))| {
            lanes
                .iter()
                .map(move |lane| ((a[*lane] - b[*lane]).abs(), probe, *lane))
        })
        .fold((0.0_f32, 0, 0), |acc, next| {
            [acc, next][usize::from(next.0 > acc.0)]
        })
}

fn assert_within(
    label: &str,
    case: &str,
    cpu: &[[f32; 4]],
    gpu: &[[f32; 4]],
    lanes: &[usize],
    tolerance: f32,
) {
    let (delta, probe, lane) = worst(cpu, gpu, lanes);
    assert!(
        delta <= tolerance,
        "{case}/{label}: worst delta {delta} at probe {probe} lane {lane} \
         exceeds {tolerance} (cpu {:?}, gpu {:?})",
        cpu[probe],
        gpu[probe]
    );
}

#[test]
fn the_wgsl_agrees_with_the_cpu_reference_on_a_real_adapter() {
    let module = compile();
    let list = probes();
    let probe_bytes = pack_probes(&list);

    for (case, u) in uniform_cases() {
        let uniform_bytes = pack_uniforms(&u);

        // ---- scalars: sample_ao, contact_shadow, specular_occlusion, gate ----
        let cpu: Vec<[f32; 4]> = list
            .iter()
            .map(|p| {
                let ao = sample_ao(u.feat[0], p.ao_texel, u.ao_strength[0]);
                [
                    ao,
                    contact_shadow(u.feat[1], p.light_dot_sun, p.contact_texel),
                    specular_occlusion(ao, p.roughness),
                    interior_gate(p.world_pos, ao, &u),
                ]
            })
            .collect();
        let gpu = render(&module, "probe_scalars_fs", &probe_bytes, &uniform_bytes);
        assert_within("sample_ao", case, &cpu, &gpu, &[0], TOLERANCE);
        assert_within("contact_shadow", case, &cpu, &gpu, &[1], TOLERANCE);
        assert_within("specular_occlusion", case, &cpu, &gpu, &[2], POW_TOLERANCE);
        assert_within("interior_gate", case, &cpu, &gpu, &[3], TOLERANCE);

        // ---- multi_bounce (rgb) and sun_bounce (a) ---------------------------
        let cpu: Vec<[f32; 4]> = list
            .iter()
            .map(|p| {
                let ao = sample_ao(u.feat[0], p.ao_texel, u.ao_strength[0]);
                let bounce = multi_bounce(ao, p.albedo);
                [
                    bounce[0],
                    bounce[1],
                    bounce[2],
                    sun_bounce(p.world_normal, u.sun_dir_world),
                ]
            })
            .collect();
        let gpu = render(&module, "probe_bounce_fs", &probe_bytes, &uniform_bytes);
        assert_within("multi_bounce", case, &cpu, &gpu, &[0, 1, 2], TOLERANCE);
        assert_within("sun_bounce", case, &cpu, &gpu, &[3], TOLERANCE);

        // ---- the whole `lights_fragment_maps` body ---------------------------
        let composed: Vec<_> = list
            .iter()
            .map(|p| {
                let ao = sample_ao(u.feat[0], p.ao_texel, u.ao_strength[0]);
                indirect(
                    IndirectIn {
                        irradiance: p.irradiance,
                        ibl_irradiance: p.ibl,
                        radiance: p.radiance,
                        diffuse_color: p.albedo,
                        roughness: p.roughness,
                        world_pos: p.world_pos,
                        world_normal: p.world_normal,
                        ao,
                    },
                    &u,
                )
            })
            .collect();

        let cpu: Vec<[f32; 4]> = composed
            .iter()
            .map(|o| [o.irradiance[0], o.irradiance[1], o.irradiance[2], o.indoor])
            .collect();
        let gpu = render(&module, "probe_irradiance_fs", &probe_bytes, &uniform_bytes);
        assert_within("irradiance", case, &cpu, &gpu, &[0, 1, 2, 3], TOLERANCE);

        let cpu: Vec<[f32; 4]> = composed
            .iter()
            .map(|o| {
                [
                    o.ibl_irradiance[0],
                    o.ibl_irradiance[1],
                    o.ibl_irradiance[2],
                    0.0,
                ]
            })
            .collect();
        let gpu = render(&module, "probe_ibl_fs", &probe_bytes, &uniform_bytes);
        assert_within("ibl_irradiance", case, &cpu, &gpu, &[0, 1, 2], TOLERANCE);

        // ---- SSR over the specular-occluded radiance -------------------------
        let cpu: Vec<[f32; 4]> = composed
            .iter()
            .zip(list.iter())
            .map(|(o, p)| {
                let blended = ssr_blend(o.radiance, u.feat[2], p.roughness, p.ssr);
                [blended[0], blended[1], blended[2], 0.0]
            })
            .collect();
        let gpu = render(&module, "probe_radiance_fs", &probe_bytes, &uniform_bytes);
        assert_within("ssr_blend", case, &cpu, &gpu, &[0, 1, 2], POW_TOLERANCE);

        // ---- the directional light's two injected multiplies -----------------
        let cpu: Vec<[f32; 4]> = list
            .iter()
            .map(|p| {
                let ao = sample_ao(u.feat[0], p.ao_texel, u.ao_strength[0]);
                let lit = direct_light(
                    p.direct_color,
                    p.receive_shadow > 0.5,
                    p.sun_shadow,
                    contact_shadow(u.feat[1], p.light_dot_sun, p.contact_texel),
                    ao,
                    u.ao_strength[0],
                );
                [lit[0], lit[1], lit[2], 0.0]
            })
            .collect();
        let gpu = render(&module, "probe_direct_fs", &probe_bytes, &uniform_bytes);
        assert_within("direct_light", case, &cpu, &gpu, &[0, 1, 2], TOLERANCE);
    }
}

/// The probe set must actually reach both arms of every gate, or the parity
/// above is proving one arm twice. This is the check that keeps the sweep
/// honest as it is edited.
#[test]
fn the_probe_set_reaches_both_arms_of_every_gate() {
    let list = probes();
    let (_, on) = uniform_cases()[1];
    let sun = list.iter().filter(|p| p.light_dot_sun >= 0.999).count();
    assert!(sun > 0 && sun < list.len(), "both sun-test arms: {sun}");
    let shadowed = list.iter().filter(|p| p.receive_shadow > 0.5).count();
    assert!(
        shadowed > 0 && shadowed < list.len(),
        "both receiveShadow arms: {shadowed}"
    );
    let full = list.iter().filter(|p| p.ao_texel >= 1.0).count();
    assert!(full > 0, "the `owAo < 1.0` guard's identity arm is unreached");
    let mirrors = list.iter().filter(|p| p.roughness < 0.62).count();
    assert!(
        mirrors > 0 && mirrors < list.len(),
        "both SSR roughness arms: {mirrors}"
    );
    // And at least one probe is deep inside the live interior volume, and at
    // least one is outdoors.
    let indoor = list
        .iter()
        .filter(|p| interior_gate(p.world_pos, 1.0, &on) < 0.5)
        .count();
    assert!(
        indoor > 0 && indoor < list.len(),
        "both interior-gate arms: {indoor}"
    );
}
