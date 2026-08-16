//! Why a surface produced no runnable program — as a value, never a panic and
//! never a silently black draw.
//!
//! Two things can go wrong between an authored `axiom_surface::Surface` and a
//! shader module this backend can bind, and both are reported the same way:
//!
//! * the surface's layer tree will not **flatten** into one set of channel
//!   bindings (its composed graphs are over the field node budget), so there is
//!   nothing to emit;
//! * the emitted WGSL will not **compile** on the device.
//!
//! Every error names the surface's digest — the same number a draw carries in
//! `axiom_host::FrameDrawItem::surface_program`, and the number a program cache
//! keys on — and the channels the failing program covered. The digest is what
//! makes the report actionable: it identifies the *material*, not a line of
//! generated text that no author ever wrote.
//!
//! A compile failure cannot be attributed to one channel by the compiler, which
//! reports a position in a program the author never wrote. So the error names
//! every channel that program carried; a flatten failure names the channel set
//! it was asked for. Both are honest about what is known.

use core::fmt;

use axiom_surface::SurfaceChannel;

/// The channel names, indexed by `SurfaceChannel`'s discriminant. Spelled as the
/// generated `SurfaceOut` spells them, so a report and the WGSL agree.
const CHANNEL_NAMES: [&str; 7] = [
    "base_color",
    "roughness",
    "metallic",
    "normal",
    "emission",
    "opacity",
    "displacement",
];

/// What went wrong. The discriminant indexes [`FAULT_MESSAGES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceProgramFault {
    /// The layer tree would not compose into one set of channel bindings.
    Flatten = 0,
    /// The generated WGSL was rejected by the device's shader compiler.
    Compilation = 1,
}

/// One sentence per fault, indexed by its discriminant.
const FAULT_MESSAGES: [&str; 2] = [
    "the layer tree would not flatten into one program",
    "the generated shader would not compile",
];

/// A surface that produced no runnable program, and everything known about why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceProgramError {
    program_id: u64,
    channels: u16,
    fault: SurfaceProgramFault,
    detail: String,
}

impl SurfaceProgramError {
    /// The surface named by `program_id`, covering `channels`, failed with
    /// `fault`; `detail` is whatever the failing stage said.
    pub(crate) fn new(
        program_id: u64,
        channels: u16,
        fault: SurfaceProgramFault,
        detail: String,
    ) -> SurfaceProgramError {
        SurfaceProgramError {
            program_id,
            channels,
            fault,
            detail,
        }
    }

    /// The surface's digest — the program-cache key and the draw's
    /// `surface_program`.
    pub(crate) const fn program_id(&self) -> u64 {
        self.program_id
    }

    /// The channels the failing program covered.
    pub(crate) const fn channels(&self) -> u16 {
        self.channels
    }

    /// Which stage failed.
    pub(crate) const fn fault(&self) -> SurfaceProgramFault {
        self.fault
    }

    /// The failing stage's own message.
    pub(crate) fn detail(&self) -> &str {
        &self.detail
    }

    /// The covered channels, named, in channel order.
    pub(crate) fn channel_names(&self) -> Vec<&'static str> {
        SurfaceChannel::ALL
            .iter()
            .filter(|channel| (self.channels & channel.bit()) != 0)
            .map(|channel| CHANNEL_NAMES[channel.index()])
            .collect()
    }
}

impl fmt::Display for SurfaceProgramError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "surface program {:#018x} [{}]: {} ({})",
            self.program_id,
            self.channel_names().join(", "),
            FAULT_MESSAGES[self.fault as usize],
            self.detail
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(channels: u16, fault: SurfaceProgramFault) -> SurfaceProgramError {
        SurfaceProgramError::new(0x0123_4567_89AB_CDEF, channels, fault, String::from("why"))
    }

    #[test]
    fn an_error_reports_the_digest_the_channels_and_the_stage() {
        let failure = error(
            SurfaceChannel::BaseColor.bit() | SurfaceChannel::Opacity.bit(),
            SurfaceProgramFault::Flatten,
        );
        assert_eq!(failure.program_id(), 0x0123_4567_89AB_CDEF);
        assert_eq!(
            failure.channels(),
            SurfaceChannel::BaseColor.bit() | SurfaceChannel::Opacity.bit()
        );
        assert_eq!(failure.fault(), SurfaceProgramFault::Flatten);
        assert_eq!(failure.detail(), "why");
        assert_eq!(failure.channel_names(), vec!["base_color", "opacity"]);
        assert_eq!(
            failure.to_string(),
            "surface program 0x0123456789abcdef [base_color, opacity]: \
             the layer tree would not flatten into one program (why)"
        );
    }

    #[test]
    fn a_compilation_failure_names_every_channel_its_program_carried() {
        let all = SurfaceChannel::ALL
            .iter()
            .fold(0_u16, |bits, channel| bits | channel.bit());
        let failure = error(all, SurfaceProgramFault::Compilation);
        assert_eq!(failure.channel_names().len(), 7);
        assert!(failure
            .to_string()
            .contains("the generated shader would not compile"));
        assert_eq!(failure, error(all, SurfaceProgramFault::Compilation));
        assert_ne!(failure, error(all, SurfaceProgramFault::Flatten));
        assert!(format!("{failure:?}").contains("SurfaceProgramError"));
    }

    #[test]
    fn a_program_covering_no_channel_names_none() {
        let failure = error(0, SurfaceProgramFault::Flatten);
        assert!(failure.channel_names().is_empty());
        assert!(failure.to_string().contains("[]"));
        // The two fault rows are distinct sentences, so a report cannot be
        // ambiguous about which stage failed.
        assert_ne!(FAULT_MESSAGES[0], FAULT_MESSAGES[1]);
        assert_eq!(CHANNEL_NAMES.len(), SurfaceChannel::ALL.len());
    }
}
