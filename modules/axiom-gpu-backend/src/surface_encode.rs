//! **Who performs the linear → sRGB encode**, and the one WGSL definition of the
//! curve that does it.
//!
//! Every pass in this crate shades in **linear light** and expects the colour
//! attachment's sRGB store to encode the result for display. That expectation is
//! written down in [`crate::post_chain`]'s module docs, it is what
//! [`crate::offscreen`]'s `Rgba8UnormSrgb` capture target provides, and it is what
//! `axiom::Color::linear_rgb` promises the app author: *you pass linear, the
//! engine displays it correctly.*
//!
//! A swap-chain surface is the one attachment in the crate we do not get to
//! choose. `Surface::get_capabilities` returns whatever the browser offers, and
//! **whether that set contains an sRGB format is a per-browser, per-backend
//! accident**, not a property of the engine:
//!
//! | browser arm | offered surface formats | sRGB store |
//! |---|---|---|
//! | WebGL2 (wgpu GL) | `Rgba8UnormSrgb`, `Rgba8Unorm`, `Rgba16Float` | yes |
//! | WebGPU (Dawn/D3D12) | `Bgra8Unorm`, `Rgba8Unorm`, `Rgba16Float` | **no** |
//!
//! So the *same scene* presented through the two arms differed by exactly one
//! application of the sRGB transfer curve: the WebGL2 arm encoded and looked
//! right, the WebGPU arm stored raw linear bytes that the browser then displayed
//! as if they were sRGB — crushing every midtone toward black and oversaturating
//! the result. (A `0.05` linear clear became byte `13` instead of byte `63`.) The
//! un-encoded arm reads as the *punchier* of the two, which is why this survived:
//! a missing gamma encode always looks like "more contrast" until it is measured.
//!
//! This module is the fix's single decision point. Two rules, both pure and both
//! keyed on a **format**, never on a backend name — a WebGPU device that offered
//! an sRGB surface must not get a second encode, and a WebGL2 device that offered
//! none must get one:
//!
//! * [`present_encode_flag`] — the final pass to the swap chain encodes in the
//!   shader exactly when the swap-chain format will not encode for it.
//! * [`scene_target_format`] — the intermediate the scene renders into is *ours*
//!   to choose, so it is sRGB whenever the device can render+sample that format.
//!   This is not cosmetic: storing linear light in 8 bits spends its precision on
//!   highlights the eye cannot resolve and starves the shadows, so a linear
//!   intermediate bands visibly across a dark sky gradient even once the transfer
//!   is corrected. Making it sRGB is what leaves the two arms with the *same*
//!   pixels rather than merely the same average brightness.

/// The sRGB transfer curve as WGSL, prepended to every shader that needs it so
/// the crate has exactly one definition of the constants (`12.92`, `1.055`,
/// `2.4`, `0.0031308`, `0.04045`).
///
/// A `&str` concatenated at pipeline build rather than a copy pasted into each
/// shader: the encode in [`crate::upscale`], the encode in
/// [`crate::post_chain`]'s composite, and the encode/decode round trip its colour
/// grade runs must be the *same* curve, or an app that authors a grade would be
/// graded on one curve and displayed through another. Concatenation costs one
/// `String` per pipeline, once, at bind.
pub(crate) const SRGB_TRANSFER_WGSL: &str = r#"
// Linear <-> sRGB (IEC 61966-2-1), the piecewise curve a hardware `*Srgb`
// attachment applies on store and undoes on sample. Shared by every pass in the
// crate; see `surface_encode.rs` for why a pass ever has to run it by hand.
fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let v = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = v * 12.92;
    let hi = 1.055 * pow(v, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, v <= vec3<f32>(0.0031308));
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    let v = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = v / 12.92;
    let hi = pow((v + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, v <= vec3<f32>(0.04045));
}
"#;

/// Build one shader source: the shared transfer curve, then the pass's own WGSL.
pub(crate) fn shader_source(body: &str) -> String {
    [SRGB_TRANSFER_WGSL, body].concat()
}

/// `1.0` when a pass presenting to `target` must encode sRGB **itself**, `0.0`
/// when the attachment encodes on store.
///
/// A float rather than a bool because it is consumed as a uniform and the shaders
/// select with `mix(linear, encoded, flag)` — the branchless form, and the form
/// that keeps one pipeline for both cases so a device cannot stutter compiling a
/// second variant mid-session.
pub(crate) fn present_encode_flag(target: wgpu::TextureFormat) -> f32 {
    f32::from(!target.is_srgb())
}

/// The colour format the scene (and the bloom chain working targets) render into,
/// given the swap-chain `surface` format and what this adapter reports the sRGB
/// variant of that format can be used for.
///
/// Prefers the sRGB variant so the scene is *stored* display-encoded — see the
/// module docs on why a linear 8-bit intermediate bands. Falls back to the
/// surface format when the device cannot both render to and sample the sRGB
/// variant, because an intermediate that cannot be a render attachment or cannot
/// be sampled by the present pass is not a smaller quality compromise, it is a
/// dead backend. The fallback is still *correct*: the present pass's encode is
/// keyed on the swap-chain format independently, so a linear intermediate simply
/// carries linear values into a present that encodes them.
///
/// Compiled for the live arm and for tests, exactly as [`crate::surface_recovery`]
/// is: negotiating a format against a surface the engine did not choose is a
/// *swap-chain* problem, and the off-screen arm — which picks its own
/// `Rgba8UnormSrgb` target and has no surface to negotiate with — has no use for
/// it. The rule is still pure, so it is decided and tested here rather than
/// written out inside the wasm-only binding where no test can reach it.
///
/// # The float arm
///
/// `hdr_scene` overrides both answers with [`HDR_SCENE_FORMAT`]. It is *not* a
/// device fact and it is not read from the surface — it is
/// [`crate::hdr_target::hdr_scene_tonemap`]'s verdict, which is the app's
/// authored tone map **and** the granted capability, together. That is the whole
/// reason this parameter is a plain `bool` arriving from outside rather than
/// something this function could work out for itself: an intermediate that no
/// longer clamps at display white is a different picture, not a better one, and
/// the decision to render it belongs to whoever authored the frame.
///
/// Everything downstream keys off the returned format alone — the bloom working
/// targets are allocated in it (`crate::post_chain`), the scene pipeline's colour
/// target is it, and the present pass reads it. So this one value is the entire
/// switch, and an arm that does not opt in gets a format bit-identical to the one
/// it always got.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn scene_target_format(
    surface: wgpu::TextureFormat,
    srgb_usages: wgpu::TextureUsages,
    hdr_scene: bool,
) -> wgpu::TextureFormat {
    let needed = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let ldr = srgb_usages
        .contains(needed)
        .then(|| surface.add_srgb_suffix())
        .unwrap_or(surface);
    [ldr, HDR_SCENE_FORMAT][usize::from(hdr_scene)]
}

/// The float colour format the scene renders into when the HDR present path is
/// on: half-float RGBA, the format
/// [`axiom_host::HostAttachmentFormat::Rgba16Float`] names and the one
/// [`crate::hdr_target::device_hdr_targets`] asks the adapter about.
///
/// Half, not full. `Rgba32Float` doubles the bandwidth of every scene pixel and
/// every bloom tap to buy exponent range a frame does not have — a half carries
/// ~5 decimal digits and a range to 65 504, against a sun disc the source
/// authors at 4 000 and a metering tap clamped at 40. And it is the format both
/// browser arms can actually report as renderable *and* filterable; `Rgba32Float`
/// needs a separate feature to be sampled with a linear filter at all, which the
/// bloom chain requires of every target it touches.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const HDR_SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

#[cfg(test)]
mod tests {
    use super::*;

    /// Both formats a real browser surface offers, and the rule that separates
    /// them. This is the whole bug in one assertion: the arm whose surface is not
    /// sRGB is the arm that has to encode.
    #[test]
    fn encode_flag_is_set_only_for_a_non_srgb_target() {
        assert_eq!(
            present_encode_flag(wgpu::TextureFormat::Rgba8UnormSrgb),
            0.0,
            "an sRGB attachment encodes on store; a second encode would wash the frame out"
        );
        assert_eq!(
            present_encode_flag(wgpu::TextureFormat::Bgra8UnormSrgb),
            0.0,
            "the BGRA sRGB surface encodes on store too — the rule is the format, not the order"
        );
        assert_eq!(
            present_encode_flag(wgpu::TextureFormat::Bgra8Unorm),
            1.0,
            "the WebGPU arm's actual surface format: nothing encodes unless the shader does"
        );
        assert_eq!(
            present_encode_flag(wgpu::TextureFormat::Rgba8Unorm),
            1.0,
            "a linear RGBA surface needs the manual encode for the same reason"
        );
    }

    /// The flag is driven by the format alone, so the two browser arms converge:
    /// whichever surface a backend hands us, exactly one encode reaches the
    /// display.
    #[test]
    fn exactly_one_encode_happens_per_arm() {
        let arms = [
            // (surface offered, hardware encodes on store)
            (wgpu::TextureFormat::Rgba8UnormSrgb, 1.0),
            (wgpu::TextureFormat::Bgra8Unorm, 0.0),
        ];
        arms.iter().for_each(|&(format, hardware)| {
            assert_eq!(
                hardware + present_encode_flag(format),
                1.0,
                "{format:?} must be encoded exactly once, by hardware or by the shader"
            );
        });
    }

    #[test]
    fn scene_target_upgrades_to_srgb_when_the_device_can_hold_it() {
        let both = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        assert_eq!(
            scene_target_format(wgpu::TextureFormat::Bgra8Unorm, both, false),
            wgpu::TextureFormat::Bgra8UnormSrgb,
            "a linear surface still gets an sRGB intermediate, so the darks do not band"
        );
        assert_eq!(
            scene_target_format(wgpu::TextureFormat::Rgba8UnormSrgb, both, false),
            wgpu::TextureFormat::Rgba8UnormSrgb,
            "an already-sRGB surface is unchanged — the WebGL2 arm renders exactly as before"
        );
    }

    #[test]
    fn scene_target_falls_back_when_either_usage_is_missing() {
        let cases = [
            wgpu::TextureUsages::empty(),
            wgpu::TextureUsages::RENDER_ATTACHMENT,
            wgpu::TextureUsages::TEXTURE_BINDING,
        ];
        cases.iter().for_each(|&usages| {
            assert_eq!(
                scene_target_format(wgpu::TextureFormat::Bgra8Unorm, usages, false),
                wgpu::TextureFormat::Bgra8Unorm,
                "a target the device cannot both draw into and sample is not usable at all"
            );
        });
    }

    /// The float arm overrides every LDR answer — including the fallback one, so
    /// a device whose sRGB variant is unusable still gets a *float* intermediate
    /// rather than being quietly dropped back to 8 bits after the capability was
    /// already granted. The two questions are independent: "can this surface hold
    /// an sRGB view" says nothing about "can this device hold a half-float
    /// attachment", and conflating them would refuse HDR for the wrong reason.
    #[test]
    fn the_hdr_scene_target_is_half_float_whatever_the_surface_offered() {
        let both = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        [
            (wgpu::TextureFormat::Bgra8Unorm, both),
            (wgpu::TextureFormat::Rgba8UnormSrgb, both),
            (wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureUsages::empty()),
        ]
        .iter()
        .for_each(|&(surface, usages)| {
            assert_eq!(
                scene_target_format(surface, usages, true),
                HDR_SCENE_FORMAT,
                "{surface:?} did not take the float intermediate"
            );
            assert_ne!(
                scene_target_format(surface, usages, false),
                HDR_SCENE_FORMAT,
                "{surface:?} took the float intermediate without being asked"
            );
        });
        assert!(
            !HDR_SCENE_FORMAT.is_srgb(),
            "the float target stores linear radiance; an sRGB store would re-clamp it"
        );
    }

    /// The shared curve reaches the shader ahead of the body, so the body may
    /// call `srgb_encode` without declaring it.
    #[test]
    fn shader_source_prepends_the_shared_transfer_curve() {
        let source = shader_source("@fragment fn fs() {}");
        assert!(source.starts_with(SRGB_TRANSFER_WGSL));
        assert!(source.ends_with("@fragment fn fs() {}"));
        assert!(source.contains("fn srgb_encode"));
        assert!(source.contains("fn srgb_decode"));
    }
}
