//! **What the bound device can do, as one value.**
//!
//! # Why this type exists
//!
//! The render path's *shape* depends on facts about the physical GPU: which
//! texture formats it can render into, which it can sample, which it can filter,
//! how many colour attachments a pass may bind. Those facts decided, among other
//! things, whether the G-buffer chain runs — and therefore whether the world is
//! lit at all.
//!
//! Before this type they were read **ad hoc, at the point of use**, straight off
//! the live adapter: a dozen separate `get_texture_format_features` /
//! `device.limits()` calls scattered across the bind. Each read was locally
//! correct and the whole was not, for one reason:
//!
//! > **A fact you read from the device is a fact you cannot inject.**
//!
//! [`axiom_host::BackendCapabilityProfile`] promises that a frame is a function
//! of *(data, capability profile)*. That promise is what makes a frame
//! reproducible — pin both and any machine renders the same picture. Scattered
//! adapter reads break it silently, because the render path then depends on a
//! third input nobody named and nobody can supply. The cost is not theoretical:
//! a device-class rendering fault becomes **unreproducible on any other
//! machine**, and the only way to investigate it is to guess, ship a guess, and
//! ask the person holding the device whether the guess worked. That is not
//! debugging.
//!
//! So the facts are resolved **once**, at the bind, into this record, and every
//! later decision reads the record rather than the adapter. Which makes them
//! *data* — and data can be substituted.
//!
//! # Impersonation, which is the point
//!
//! [`DeviceFacts::impersonating`] narrows a record to describe a *different*
//! device, so a workstation can render exactly what a phone renders. That is not
//! a debugging convenience bolted on the side; it is the property the type
//! exists to restore. A capability system whose inputs cannot be supplied is a
//! capability system that cannot be tested on anything but the hardware in the
//! room.
//!
//! It can only ever **narrow**. A machine can be asked to pretend it lacks a
//! format it has; it can never be told to pretend it has one it lacks, because
//! that would ask the driver for something it will refuse — and the refusal
//! would be a different failure from the one being reproduced.

/// Everything about the bound device that the render path branches on.
///
/// Deliberately free of `wgpu` types: this is the *answer*, not the query, so it
/// stays a plain value that a test or a URL can construct. The wgpu-shaped
/// resolution lives in `live_gpu_binding`, at the one place that holds an
/// adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeviceFacts {
    /// `Rgba16Float` is renderable AND samplable — the pair
    /// [`crate::hdr_target::device_hdr_targets`] requires, and the gate for the
    /// float scene target the tone map needs.
    pub(crate) hdr_renderable: bool,
    pub(crate) hdr_samplable: bool,
    /// `Rg16Float` is renderable: the G-buffer's velocity channel, and the
    /// target both the occlusion and contact chains render into.
    pub(crate) rg16float_renderable: bool,
    /// `R32Float` is renderable: the G-buffer's linear-depth channel.
    ///
    /// Separate from [`Self::hdr_renderable`] because on WebGL2 they are
    /// separate extensions — `EXT_color_buffer_half_float` grants the first and
    /// **not** this one, which needs `EXT_color_buffer_float`. Treating one as a
    /// proxy for the other is what let a phone be granted a G-buffer it could
    /// not hold.
    pub(crate) r32float_renderable: bool,
    /// `Rgba16Float` may be sampled with a LINEAR filter.
    ///
    /// Renderability and filterability are different device answers and on
    /// WebGL2 they come from different extensions: a colour-buffer extension
    /// makes the format RENDERABLE, and `OES_texture_float_linear` makes it
    /// FILTERABLE. A device can have the first and not the second — and mobile
    /// commonly does.
    pub(crate) rgba16float_filterable: bool,
    /// `Rg16Float` may be sampled with a LINEAR filter.
    ///
    /// The one that decides whether the world is lit. The occlusion and contact
    /// targets are `Rg16Float` and the main pass samples them through a
    /// FILTERING sampler, because the fetch upsamples a half-resolution signal.
    /// If the device cannot filter the format, GLES texture completeness makes
    /// that texture INCOMPLETE and every sample returns `0.0` — which these two
    /// terms read as "fully occluded", multiplying the ambient, the indirect
    /// fill and the sun away while the sky, which samples neither, is untouched.
    pub(crate) rg16float_filterable: bool,
    /// The depth format may be sampled with a LINEAR filter.
    ///
    /// A comparison sampler is exempt from WebGPU's filterable-format rule, so a
    /// `Linear` shadow sampler validates everywhere and reaches the driver — but
    /// GLES 3.0 texture completeness has no such exemption, and an incomplete
    /// sampler returns `0.0`, which a shadow compare reads as *fully shadowed*.
    pub(crate) depth_filterable: bool,
    /// `max_color_attachments` — how many targets one pass may bind.
    pub(crate) max_color_attachments: u32,
    /// `max_color_attachment_bytes_per_sample` — the total width of them.
    pub(crate) max_color_attachment_bytes_per_sample: u32,
}

impl DeviceFacts {
    /// The facts a device has when nothing has been measured yet: none of them.
    ///
    /// The same discipline as
    /// [`crate::hdr_target::unresolved_capability_profile`] — a backend that has
    /// resolved no adapter must not claim a device fact it cannot know.
    pub(crate) const UNRESOLVED: DeviceFacts = DeviceFacts {
        hdr_renderable: false,
        hdr_samplable: false,
        rg16float_renderable: false,
        r32float_renderable: false,
        rgba16float_filterable: false,
        rg16float_filterable: false,
        depth_filterable: false,
        max_color_attachments: 0,
        max_color_attachment_bytes_per_sample: 0,
    };

    /// Narrow these facts to describe a **less** capable device.
    ///
    /// `spec` is a comma-separated list of things to take away. Unknown tokens
    /// are ignored rather than rejected, so a stale or misspelled spec renders
    /// the real device instead of failing to boot — the caller is usually a
    /// query string typed on a phone.
    ///
    /// | token | takes away |
    /// |---|---|
    /// | `no-hdr` | `Rgba16Float` render **and** sample |
    /// | `no-rg16f` | `Rg16Float` render |
    /// | `no-r32f` | `R32Float` render |
    /// | `no-float-filter` | linear filtering of `Rgba16Float` **and** `Rg16Float` |
    /// | `no-depth-filter` | linear filtering of the depth format |
    /// | `no-mrt` | multiple colour attachments (drops the count to 1) |
    ///
    /// Narrowing only: every field is `AND`-ed with the absence of its token, so
    /// no spec can turn a fact on. See the module docs for why that direction is
    /// the only honest one.
    pub(crate) fn impersonating(self, spec: &str) -> DeviceFacts {
        spec.split(',').map(str::trim).fold(self, |facts, token| {
            let kept = |name: &str| token != name;
            let hdr = kept("no-hdr");
            DeviceFacts {
                hdr_renderable: facts.hdr_renderable & hdr,
                hdr_samplable: facts.hdr_samplable & hdr,
                rg16float_renderable: facts.rg16float_renderable & kept("no-rg16f"),
                r32float_renderable: facts.r32float_renderable & kept("no-r32f"),
                rgba16float_filterable: facts.rgba16float_filterable & kept("no-float-filter"),
                rg16float_filterable: facts.rg16float_filterable & kept("no-float-filter"),
                depth_filterable: facts.depth_filterable & kept("no-depth-filter"),
                max_color_attachments: facts
                    .max_color_attachments
                    .min([1, u32::MAX][usize::from(kept("no-mrt"))]),
                max_color_attachment_bytes_per_sample: facts
                    .max_color_attachment_bytes_per_sample,
            }
        })
    }

    /// Whether every format the G-buffer chain renders into is renderable.
    ///
    /// All four, asked individually. The prepass writes `Rgba16Float`,
    /// `Rg16Float` and `R32Float`; the occlusion and contact chains fed by it
    /// render `Rg16Float` again. A device that can hold some and not others has
    /// no G-buffer, and the honest moment to say so is the bind — not three
    /// passes later, in a frame whose occlusion target never resolved and whose
    /// zero clear multiplies the world's light away.
    pub(crate) const fn gbuffer_formats_renderable(&self) -> bool {
        self.hdr_renderable & self.rg16float_renderable & self.r32float_renderable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything a fully-capable desktop reports.
    const FULL: DeviceFacts = DeviceFacts {
        hdr_renderable: true,
        hdr_samplable: true,
        rg16float_renderable: true,
        r32float_renderable: true,
        rgba16float_filterable: true,
        rg16float_filterable: true,
        depth_filterable: true,
        max_color_attachments: 8,
        max_color_attachment_bytes_per_sample: 32,
    };

    /// An unresolved backend claims nothing — the same rule the capability
    /// profile follows, and for the same reason: a fact you have not measured is
    /// not a fact.
    #[test]
    fn an_unresolved_device_claims_no_capability_at_all() {
        assert!(!DeviceFacts::UNRESOLVED.hdr_renderable);
        assert!(!DeviceFacts::UNRESOLVED.gbuffer_formats_renderable());
        assert_eq!(DeviceFacts::UNRESOLVED.max_color_attachments, 0);
    }

    /// The G-buffer gate asks about EVERY format, not one representative.
    ///
    /// This is the defect the type was extracted to fix: the old gate used
    /// `Rgba16Float` as a proxy for the whole set, and on WebGL2 that is a
    /// different extension from the one `R32Float` needs. A device with the
    /// first and not the second was granted a G-buffer it could not hold.
    #[test]
    fn one_unrenderable_format_is_enough_to_deny_the_gbuffer() {
        assert!(FULL.gbuffer_formats_renderable());
        assert!(!DeviceFacts { r32float_renderable: false, ..FULL }.gbuffer_formats_renderable());
        assert!(!DeviceFacts { rg16float_renderable: false, ..FULL }.gbuffer_formats_renderable());
        assert!(!DeviceFacts { hdr_renderable: false, ..FULL }.gbuffer_formats_renderable());
    }

    /// The whole point of the type: a capable machine can be told to render as
    /// an incapable one, exactly.
    #[test]
    fn impersonating_narrows_each_fact_its_token_names() {
        assert!(!FULL.impersonating("no-r32f").r32float_renderable);
        assert!(!FULL.impersonating("no-rg16f").rg16float_renderable);
        assert!(!FULL.impersonating("no-depth-filter").depth_filterable);
        assert_eq!(FULL.impersonating("no-mrt").max_color_attachments, 1);
        // `no-hdr` takes both halves, because they are one device answer.
        let no_hdr = FULL.impersonating("no-hdr");
        assert!(!no_hdr.hdr_renderable);
        assert!(!no_hdr.hdr_samplable);
    }

    /// A phone, spelled out: half-float colour but no full-float colour, which
    /// is the real WebGL2 extension split. The G-buffer must be denied.
    #[test]
    fn a_half_float_only_device_is_denied_the_gbuffer_but_keeps_its_hdr_target() {
        let phone = FULL.impersonating("no-r32f");
        assert!(phone.hdr_renderable, "the tone map's float target survives");
        assert!(
            !phone.gbuffer_formats_renderable(),
            "the G-buffer does not, and that is the distinction the old gate could not draw"
        );
    }

    /// **Renderable and filterable are different questions.**
    ///
    /// The distinction this field pair exists for: a device can render into a
    /// float target and be unable to FILTER it, because on WebGL2 those are two
    /// different extensions. A record that collapsed them would report a phone
    /// as identical to a workstation — which is exactly what the first version
    /// of this record did, and why it could not reproduce one.
    #[test]
    fn a_format_can_be_renderable_without_being_filterable() {
        let phone = FULL.impersonating("no-float-filter");
        assert!(phone.rg16float_renderable, "still a valid render target");
        assert!(!phone.rg16float_filterable, "but it cannot be filtered");
        assert!(!phone.rgba16float_filterable);
        // And the G-buffer gate, which is about RENDERING, is unmoved by it.
        assert!(phone.gbuffer_formats_renderable());
    }

    /// Several at once, and the tokens are order-independent.
    #[test]
    fn a_spec_narrows_by_every_token_it_lists_in_any_order() {
        let a = FULL.impersonating("no-r32f,no-depth-filter");
        let b = FULL.impersonating("no-depth-filter,no-r32f");
        assert_eq!(a, b);
        assert!(!a.r32float_renderable & !a.depth_filterable);
        // Whitespace around a token is tolerated: this arrives from a URL.
        assert_eq!(FULL.impersonating(" no-r32f , no-rg16f "),
                   FULL.impersonating("no-r32f,no-rg16f"));
    }

    /// Unknown and empty specs render the real device rather than refusing to
    /// boot. A misspelled query string must not be a black screen.
    #[test]
    fn an_unknown_or_empty_spec_changes_nothing() {
        assert_eq!(FULL.impersonating(""), FULL);
        assert_eq!(FULL.impersonating("no-such-thing"), FULL);
        assert_eq!(FULL.impersonating("no-r32f,bogus").r32float_renderable, false);
    }

    /// Narrowing only. No spec can hand a device a capability it does not have,
    /// because the driver would refuse and the refusal would be a DIFFERENT
    /// failure from the one being reproduced.
    #[test]
    fn impersonation_can_never_grant_a_capability() {
        let bare = DeviceFacts::UNRESOLVED;
        [
            "no-hdr",
            "no-r32f",
            "no-rg16f",
            "no-float-filter",
            "no-depth-filter",
            "no-mrt",
            "",
            "anything",
        ]
        .iter()
        .for_each(|spec| {
            let after = bare.impersonating(spec);
            assert!(!after.hdr_renderable);
            assert!(!after.r32float_renderable);
            assert!(!after.rg16float_renderable);
            assert!(!after.rg16float_filterable);
            assert!(!after.rgba16float_filterable);
            assert!(!after.depth_filterable);
        });
    }
}
