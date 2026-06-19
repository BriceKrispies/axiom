//! C-ABI exports of the canonical `axiom-net-protocol` wire codec.
//!
//! These let the .NET host encode/decode protocol frames through the **one**
//! Rust codec instead of a hand-written C# twin. They are stateless (no session)
//! and operate over caller-provided byte buffers: decoders read a frame, encoders
//! write a frame into an `out` buffer and return the byte count (or `-1` on
//! error / too-small buffer). The opaque intent payload (a 2×`f32` move delta) is
//! unpacked here since that is app-level, not protocol.

use axiom_net_protocol::NetProtocolApi;

/// View a caller buffer as a slice, or `None` for a null pointer.
unsafe fn view<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    (!ptr.is_null()).then(|| std::slice::from_raw_parts(ptr, len))
}

/// Copy `bytes` into `out` (capacity `cap`), returning the count, or `-1` if
/// absent or the buffer is too small.
unsafe fn copy_out(bytes: Option<Vec<u8>>, out: *mut u8, cap: usize) -> isize {
    match bytes {
        Some(v) if !out.is_null() && v.len() <= cap => {
            std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
            v.len() as isize
        }
        _ => -1,
    }
}

/// Unpack a `(dx, dy)` move delta from an intent payload (two little-endian
/// `f32`s); a short payload is no movement.
fn unpack_delta(payload: &[u8]) -> (f32, f32) {
    if payload.len() < 8 {
        return (0.0, 0.0);
    }
    let dx = f32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let dy = f32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    (dx, dy)
}

/// The message kind of an encoded frame (`0..=6`), or `-1` if malformed.
///
/// # Safety
/// `ptr`/`len` must describe a readable buffer (or `ptr` null).
#[no_mangle]
pub unsafe extern "C" fn axiom_msg_kind(ptr: *const u8, len: usize) -> i32 {
    view(ptr, len)
        .and_then(|b| NetProtocolApi::message_kind(b).ok())
        .map(|k| k as i32)
        .unwrap_or(-1)
}

/// The protocol version of a `JoinRoom` frame, or `0` if it is not a valid
/// `JoinRoom` (a valid version is always nonzero).
///
/// # Safety
/// `ptr`/`len` must describe a readable buffer (or `ptr` null).
#[no_mangle]
pub unsafe extern "C" fn axiom_decode_join_version(ptr: *const u8, len: usize) -> u32 {
    view(ptr, len)
        .and_then(|b| NetProtocolApi::decode_join_room(b).ok())
        .map(|(version, _room, _token)| version)
        .unwrap_or(0)
}

/// Decode a `ClientIntent`, writing its sequence and unpacked `(dx, dy)` delta.
/// Returns `1` on success, `0` on failure.
///
/// # Safety
/// `ptr`/`len` must describe a readable buffer; the out pointers must be valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_decode_client_intent(
    ptr: *const u8,
    len: usize,
    out_seq: *mut u64,
    out_dx: *mut f32,
    out_dy: *mut f32,
) -> i32 {
    match view(ptr, len).and_then(|b| NetProtocolApi::decode_client_intent(b).ok()) {
        Some((seq, _predicted, _last_seen, payload)) => {
            let (dx, dy) = unpack_delta(&payload);
            out_seq.as_mut().map(|p| *p = seq);
            out_dx.as_mut().map(|p| *p = dx);
            out_dy.as_mut().map(|p| *p = dy);
            1
        }
        None => 0,
    }
}

/// Encode a `Welcome` into `out`; returns the byte count or `-1`.
///
/// # Safety
/// `out`/`cap` must describe a writable buffer.
#[no_mangle]
pub unsafe extern "C" fn axiom_encode_welcome(
    protocol_version: u32,
    client_id: u64,
    server_tick: u64,
    fixed_step_ns: u64,
    out: *mut u8,
    cap: usize,
) -> isize {
    copy_out(
        NetProtocolApi::encode_welcome(protocol_version, client_id, server_tick, fixed_step_ns).ok(),
        out,
        cap,
    )
}

/// Encode a `ServerSnapshot` wrapping `payload` into `out`; returns the byte
/// count or `-1`.
///
/// # Safety
/// `payload`/`payload_len` and `out`/`cap` must describe valid buffers.
#[no_mangle]
pub unsafe extern "C" fn axiom_encode_snapshot(
    server_tick: u64,
    last_accepted: u64,
    payload: *const u8,
    payload_len: usize,
    out: *mut u8,
    cap: usize,
) -> isize {
    let payload = view(payload, payload_len).unwrap_or(&[]);
    copy_out(
        NetProtocolApi::encode_server_snapshot(server_tick, last_accepted, payload).ok(),
        out,
        cap,
    )
}

/// Encode a `RejectedIntent` into `out`; returns the byte count or `-1`.
///
/// # Safety
/// `out`/`cap` must describe a writable buffer.
#[no_mangle]
pub unsafe extern "C" fn axiom_encode_rejected(
    client_sequence: u64,
    reason_code: u32,
    out: *mut u8,
    cap: usize,
) -> isize {
    copy_out(
        Some(NetProtocolApi::encode_rejected_intent(client_sequence, reason_code)),
        out,
        cap,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_encodes_and_peeks_back_as_a_welcome() {
        let mut buf = [0u8; 64];
        let n = unsafe { axiom_encode_welcome(1, 7, 42, 16_666_667, buf.as_mut_ptr(), buf.len()) };
        assert!(n > 0);
        let kind = unsafe { axiom_msg_kind(buf.as_ptr(), n as usize) };
        assert_eq!(kind, NetProtocolApi::KIND_WELCOME as i32);
    }

    #[test]
    fn client_intent_round_trips_through_the_c_abi() {
        // Build a real ClientIntent (payload = a (0.25, -0.5) delta) with the
        // canonical encoder, then decode it through the C ABI.
        let mut payload = Vec::new();
        payload.extend_from_slice(&0.25f32.to_le_bytes());
        payload.extend_from_slice(&(-0.5f32).to_le_bytes());
        let frame = NetProtocolApi::encode_client_intent(9, 0, 0, &payload).unwrap();

        let (mut seq, mut dx, mut dy) = (0u64, 0.0f32, 0.0f32);
        let ok = unsafe {
            axiom_decode_client_intent(
                frame.as_ptr(),
                frame.len(),
                &mut seq,
                &mut dx,
                &mut dy,
            )
        };
        assert_eq!(ok, 1);
        assert_eq!(seq, 9);
        assert_eq!((dx, dy), (0.25, -0.5));
    }

    #[test]
    fn join_version_and_snapshot_and_rejected() {
        let join = NetProtocolApi::encode_join_room(1, b"lobby", b"").unwrap();
        assert_eq!(unsafe { axiom_decode_join_version(join.as_ptr(), join.len()) }, 1);

        let mut buf = [0u8; 256];
        let n = unsafe {
            axiom_encode_snapshot(5, 3, [1u8, 2, 3].as_ptr(), 3, buf.as_mut_ptr(), buf.len())
        };
        assert!(n > 0);
        assert_eq!(
            unsafe { axiom_msg_kind(buf.as_ptr(), n as usize) },
            NetProtocolApi::KIND_SERVER_SNAPSHOT as i32
        );

        let n = unsafe { axiom_encode_rejected(5, 2, buf.as_mut_ptr(), buf.len()) };
        assert!(n > 0);
        assert_eq!(
            unsafe { axiom_msg_kind(buf.as_ptr(), n as usize) },
            NetProtocolApi::KIND_REJECTED_INTENT as i32
        );
    }

    #[test]
    fn bad_input_is_reported_not_panicked() {
        assert_eq!(unsafe { axiom_msg_kind(std::ptr::null(), 0) }, -1);
        assert_eq!(unsafe { axiom_msg_kind([0xFFu8, 0xFF].as_ptr(), 2) }, -1);
        assert_eq!(unsafe { axiom_decode_join_version([0u8].as_ptr(), 1) }, 0);
        // Too-small output buffer is reported as -1.
        let mut tiny = [0u8; 2];
        assert_eq!(
            unsafe { axiom_encode_welcome(1, 1, 0, 1, tiny.as_mut_ptr(), tiny.len()) },
            -1
        );
    }
}
