//! `Welcome` — the server's acceptance of a joining client (server → client).

use axiom_kernel::{BinaryReader, BinaryWriter, FixedStep, KernelResult};

use crate::client_id::ClientId;
use crate::frame;
use crate::protocol_version::ProtocolVersion;

/// The server's acceptance of a `JoinRoom`. It hands the client everything it
/// needs to begin: the confirmed application [`ProtocolVersion`], the
/// server-assigned [`ClientId`], the current authoritative `server_tick`, and
/// the simulation's [`FixedStep`] (`fixed_step_ns`, the nanoseconds per tick).
///
/// `fixed_step_ns` reuses the kernel's [`FixedStep`], which is validated nonzero
/// at construction — a zero step would let a clock "advance" without
/// progressing, so it is rejected here just as the kernel rejects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Welcome {
    protocol_version: ProtocolVersion,
    client_id: ClientId,
    server_tick: u64,
    fixed_step: FixedStep,
}

impl Welcome {
    /// Validate and construct a `Welcome`. Fails if the protocol version is
    /// zero, the client id is zero, or `fixed_step_ns` is zero.
    pub(crate) fn new(
        protocol_version: u32,
        client_id: u64,
        server_tick: u64,
        fixed_step_ns: u64,
    ) -> KernelResult<Self> {
        ProtocolVersion::new(protocol_version).and_then(|protocol_version| {
            ClientId::new(client_id).and_then(|client_id| {
                FixedStep::new(fixed_step_ns).map(|fixed_step| Welcome {
                    protocol_version,
                    client_id,
                    server_tick,
                    fixed_step,
                })
            })
        })
    }

    /// The confirmed application protocol version.
    pub(crate) fn protocol_version(&self) -> u32 {
        self.protocol_version.raw()
    }

    /// The server-assigned client id.
    pub(crate) fn client_id(&self) -> u64 {
        self.client_id.raw()
    }

    /// The current authoritative server tick.
    pub(crate) fn server_tick(&self) -> u64 {
        self.server_tick
    }

    /// The simulation step, in nanoseconds (always nonzero).
    pub(crate) fn fixed_step_ns(&self) -> u64 {
        self.fixed_step.nanos()
    }

    /// Encode to a complete wire frame (header + body).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_WELCOME);
        self.protocol_version.write_to(&mut w);
        self.client_id.write_to(&mut w);
        w.write_u64(self.server_tick);
        w.write_u64(self.fixed_step.nanos());
        w.into_bytes()
    }

    /// Decode a complete wire frame previously produced by [`Self::encode`].
    pub(crate) fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        frame::read_expected_kind(&mut r, frame::KIND_WELCOME)
            .and_then(|()| ProtocolVersion::read_from(&mut r))
            .and_then(|protocol_version| {
                ClientId::read_from(&mut r).and_then(|client_id| {
                    r.read_u64().and_then(|server_tick| {
                        r.read_u64().and_then(|fixed_step_ns| {
                            FixedStep::new(fixed_step_ns).map(|fixed_step| Welcome {
                                protocol_version,
                                client_id,
                                server_tick,
                                fixed_step,
                            })
                        })
                    })
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{KernelErrorCode, KernelErrorScope};

    fn sample() -> Welcome {
        Welcome::new(1, 77, 42, 16_666_667).unwrap()
    }

    #[test]
    fn accessors_return_the_fields() {
        let m = sample();
        assert_eq!(m.protocol_version(), 1);
        assert_eq!(m.client_id(), 77);
        assert_eq!(m.server_tick(), 42);
        assert_eq!(m.fixed_step_ns(), 16_666_667);
    }

    #[test]
    fn round_trips() {
        assert_eq!(Welcome::decode(&sample().encode()).unwrap(), sample());
    }

    #[test]
    fn construction_rejects_invalid_fields() {
        assert_eq!(
            Welcome::new(0, 1, 0, 1).unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
        assert_eq!(
            Welcome::new(1, 0, 0, 1).unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
        // A zero fixed step is rejected with the kernel's own Time-scope error.
        let err = Welcome::new(1, 1, 0, 0).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Time);
        assert_eq!(err.code(), KernelErrorCode::InvalidFixedStep);
    }

    #[test]
    fn decode_rejects_a_zero_fixed_step() {
        // A frame whose fixed-step field is zero must fail to decode.
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_WELCOME);
        ProtocolVersion::new(1).unwrap().write_to(&mut w);
        ClientId::new(1).unwrap().write_to(&mut w);
        w.write_u64(0); // server_tick
        w.write_u64(0); // fixed_step_ns = 0 → invalid
        assert_eq!(
            Welcome::decode(w.as_bytes()).unwrap_err().code(),
            KernelErrorCode::InvalidFixedStep
        );
    }

    #[test]
    fn decode_rejects_a_wrong_kind() {
        let other = crate::join_room::JoinRoom::new(1, b"r", b"")
            .unwrap()
            .encode();
        assert_eq!(
            Welcome::decode(&other).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = sample().encode();
        (0..bytes.len()).for_each(|k| {
            assert!(
                Welcome::decode(&bytes[..k]).is_err(),
                "prefix {k} must fail"
            );
        });
        assert!(Welcome::decode(&bytes).is_ok());
    }
}
