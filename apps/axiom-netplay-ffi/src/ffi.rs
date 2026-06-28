//! The Tier-B worker-control C ABI: the server-only surface the .NET host drives.
//!
//! Every entry point is a thin, **panic-guarded** wrapper over [`Session`]. No
//! Rust panic may unwind across the C ABI — each body runs inside
//! [`catch_unwind`] and a caught panic becomes [`STATUS_ERR_PANIC`] (recorded in
//! the sim's last-error slot when a handle is available). Results are explicit
//! `i32` status codes; values come back through caller-provided out pointers.
//!
//! This is **not** the browser wire protocol. The browser-facing Tier-A codec is
//! re-exported separately in [`crate::codec`]; nothing here is reachable from a
//! socket.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::session::{Session, MAX_PLAYERS_CAP};
use crate::{replay, status::*};

/// The outcome of a guarded body: success, or a status code to return.
type Op = Result<(), i32>;

/// Run a sim-bound body under panic protection, mapping the result to a status
/// code and recording a caught panic on the handle when possible.
fn run(sim: *mut Session, body: impl FnOnce() -> Op) -> i32 {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => STATUS_OK,
        Ok(Err(code)) => code,
        Err(_) => {
            // SAFETY: sequential re-acquire; any borrow from the body unwound.
            unsafe {
                if let Some(s) = sim.as_mut() {
                    s.record_error(
                        STATUS_ERR_PANIC as u32,
                        "panic caught at the C ABI boundary",
                    );
                }
            }
            STATUS_ERR_PANIC
        }
    }
}

/// Run a handle-free body under panic protection.
fn run_stateless(body: impl FnOnce() -> Op) -> i32 {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => STATUS_OK,
        Ok(Err(code)) => code,
        Err(_) => STATUS_ERR_PANIC,
    }
}

/// The shared empty input slice for a null/zero-length buffer.
const EMPTY: &[u8] = &[];

/// Borrow a sim handle, or fail with [`STATUS_ERR_NULL_HANDLE`].
///
/// # Safety
/// `ptr` is either null or a valid `Session` pointer from [`axiom_sim_create`].
unsafe fn sess<'a>(ptr: *mut Session) -> Result<&'a mut Session, i32> {
    ptr.as_mut().ok_or(STATUS_ERR_NULL_HANDLE)
}

/// View a caller input buffer as a slice. A null pointer with length `0` is the
/// empty slice; a null pointer with a non-zero length is invalid.
///
/// # Safety
/// `ptr`/`len` describe a readable buffer (or `ptr` is null).
unsafe fn in_slice<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], i32> {
    if ptr.is_null() {
        return if len == 0 {
            Ok(EMPTY)
        } else {
            Err(STATUS_ERR_INVALID_ARG)
        };
    }
    Ok(std::slice::from_raw_parts(ptr, len))
}

/// Copy `bytes` into a caller output buffer, writing the count to `out_written`.
/// Fails with [`STATUS_ERR_BUFFER_TOO_SMALL`] if `capacity` is short (no write).
///
/// # Safety
/// `out_ptr`/`capacity` describe a writable buffer; `out_written` is valid.
unsafe fn copy_out(bytes: &[u8], out_ptr: *mut u8, capacity: usize, out_written: *mut usize) -> Op {
    let written = out_written.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
    if bytes.len() > capacity {
        return Err(STATUS_ERR_BUFFER_TOO_SMALL);
    }
    if out_ptr.is_null() && !bytes.is_empty() {
        return Err(STATUS_ERR_INVALID_ARG);
    }
    if !bytes.is_empty() {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, bytes.len());
    }
    *written = bytes.len();
    Ok(())
}

// --- version handshake ---

/// Worker semantic version, major.
#[no_mangle]
pub extern "C" fn axiom_worker_version_major() -> u32 {
    catch_unwind(|| WORKER_VERSION_MAJOR).unwrap_or(0)
}

/// Worker semantic version, minor.
#[no_mangle]
pub extern "C" fn axiom_worker_version_minor() -> u32 {
    catch_unwind(|| WORKER_VERSION_MINOR).unwrap_or(0)
}

/// Worker semantic version, patch.
#[no_mangle]
pub extern "C" fn axiom_worker_version_patch() -> u32 {
    catch_unwind(|| WORKER_VERSION_PATCH).unwrap_or(0)
}

/// The Tier-B worker-control protocol version the host must match.
#[no_mangle]
pub extern "C" fn axiom_worker_protocol_version() -> u32 {
    catch_unwind(|| WORKER_PROTOCOL_VERSION).unwrap_or(0)
}

// --- lifecycle ---

/// Create a sim instance. Returns an opaque handle, or null on invalid arguments
/// (`max_players` zero or above the cap, `fixed_step_ns` zero) or a caught panic.
/// Free it with [`axiom_sim_destroy`].
#[no_mangle]
pub extern "C" fn axiom_sim_create(
    seed: u64,
    max_players: u32,
    fixed_step_ns: u64,
) -> *mut Session {
    catch_unwind(|| {
        if !(1..=MAX_PLAYERS_CAP).contains(&max_players) || fixed_step_ns == 0 {
            return std::ptr::null_mut();
        }
        Box::into_raw(Box::new(Session::new(seed, max_players, fixed_step_ns)))
    })
    .unwrap_or(std::ptr::null_mut())
}

/// Destroy a sim instance created by [`axiom_sim_create`]. A null handle is a
/// no-op.
///
/// # Safety
/// `sim` is null or a handle from [`axiom_sim_create`], not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_destroy(sim: *mut Session) {
    (!sim.is_null()).then(|| {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(Box::from_raw(sim))));
    });
}

// --- state ---

/// Restore authoritative state from snapshot bytes (the .NET "load room state").
///
/// # Safety
/// `sim` is a valid handle; `ptr`/`len` describe a readable buffer (or null/0).
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_load_state(
    sim: *mut Session,
    ptr: *const u8,
    len: usize,
) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let bytes = in_slice(ptr, len)?;
        s.restore(bytes).then_some(()).ok_or(STATUS_ERR_DESERIALIZE)
    })
}

/// Submit a player intent. `player_id` is the slot the **host assigned** — it is
/// never trusted from the client. Returns [`STATUS_OK`] when accepted (reason
/// `REASON_NONE`), [`STATUS_REJECTED`] when validation rejects it (the specific
/// `REASON_*` is written to `out_reason_code`), or an error status for ABI misuse.
///
/// # Safety
/// `sim` is a valid handle; `payload_ptr`/`payload_len` describe a readable
/// buffer (or null/0); `out_reason_code` is valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_submit_intent(
    sim: *mut Session,
    player_id: u32,
    client_sequence: u64,
    predicted_client_tick: u64,
    payload_ptr: *const u8,
    payload_len: usize,
    out_reason_code: *mut u32,
) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let payload = in_slice(payload_ptr, payload_len)?;
        let reason_out = out_reason_code.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        let reason = s.submit_intent(player_id, client_sequence, predicted_client_tick, payload);
        *reason_out = reason;
        (reason == REASON_NONE).then_some(()).ok_or(STATUS_REJECTED)
    })
}

/// Advance one fixed tick, applying queued intents in deterministic order.
/// Writes the new authoritative tick count and state hash. `target_tick` is
/// advisory in v1 (the worker steps exactly once per call); it is reserved for
/// future multi-step catch-up.
///
/// # Safety
/// `sim` is a valid handle; `out_tick` and `out_state_hash` are valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_advance_tick(
    sim: *mut Session,
    target_tick: u64,
    out_tick: *mut u64,
    out_state_hash: *mut u64,
) -> i32 {
    run(sim, || {
        let _ = target_tick; // reserved; v1 advances exactly one step per call.
        let s = sess(sim)?;
        let tick_out = out_tick.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        let hash_out = out_state_hash.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        let (tick, hash) = s.advance();
        *tick_out = tick;
        *hash_out = hash;
        Ok(())
    })
}

/// Write the authoritative snapshot length (in bytes) to `out_len`.
///
/// # Safety
/// `sim` is a valid handle; `out_len` is valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_snapshot_len(sim: *mut Session, out_len: *mut usize) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let len_out = out_len.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        *len_out = s.snapshot().len();
        Ok(())
    })
}

/// Write the authoritative snapshot bytes (and its state hash) into a caller
/// buffer. Query [`axiom_sim_snapshot_len`] first; a short buffer fails with
/// [`STATUS_ERR_BUFFER_TOO_SMALL`] without writing.
///
/// # Safety
/// `sim` is a valid handle; `out_ptr`/`out_capacity` describe a writable buffer;
/// `out_written` and `out_state_hash` are valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_snapshot_write(
    sim: *mut Session,
    out_ptr: *mut u8,
    out_capacity: usize,
    out_written: *mut usize,
    out_state_hash: *mut u64,
) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let hash_out = out_state_hash.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        let bytes = s.snapshot();
        *hash_out = s.state_hash();
        copy_out(&bytes, out_ptr, out_capacity, out_written)
    })
}

// --- full session snapshot (sim + rng): the persistence / recovery aggregate ---
//
// Distinct from the scene-only `axiom_sim_snapshot_*` pair above (which the
// per-tick replay/hash machinery uses): this carries the durable sim state AND
// the host RNG in one opaque, versioned blob the embedding host stores verbatim
// and hands back on restore — so a recovered worker continues the identical
// random sequence rather than diverging.

/// Write the full session-snapshot length (in bytes) to `out_len`.
///
/// # Safety
/// `sim` is a valid handle; `out_len` is valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_session_snapshot_len(sim: *mut Session, out_len: *mut usize) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let len_out = out_len.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        *len_out = s.snapshot_session().len();
        Ok(())
    })
}

/// Write the full session-snapshot bytes into a caller buffer. Query
/// [`axiom_session_snapshot_len`] first; a short buffer fails with
/// [`STATUS_ERR_BUFFER_TOO_SMALL`] without writing.
///
/// # Safety
/// `sim` is a valid handle; `out_ptr`/`out_capacity` describe a writable buffer;
/// `out_written` is valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_session_snapshot_write(
    sim: *mut Session,
    out_ptr: *mut u8,
    out_capacity: usize,
    out_written: *mut usize,
) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let bytes = s.snapshot_session();
        copy_out(&bytes, out_ptr, out_capacity, out_written)
    })
}

/// Restore a full session (sim + rng) from a buffer produced by
/// [`axiom_session_snapshot_write`]. Fails with [`STATUS_ERR_DESERIALIZE`] on a
/// truncated / incompatible buffer.
///
/// # Safety
/// `sim` is a valid handle; `ptr`/`len` describe a readable buffer (or null/0).
#[no_mangle]
pub unsafe extern "C" fn axiom_session_restore(
    sim: *mut Session,
    ptr: *const u8,
    len: usize,
) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let bytes = in_slice(ptr, len)?;
        s.restore_session(bytes)
            .then_some(())
            .ok_or(STATUS_ERR_DESERIALIZE)
    })
}

/// Write the current authoritative state hash to `out_hash`.
///
/// # Safety
/// `sim` is a valid handle; `out_hash` is valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_state_hash(sim: *mut Session, out_hash: *mut u64) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let hash_out = out_hash.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        *hash_out = s.state_hash();
        Ok(())
    })
}

/// Write the authoritative render view — each player's `(x, y)` world position,
/// `2 * max_players` little-endian `f32` — into a caller buffer, setting the
/// count written. A short buffer fails with [`STATUS_ERR_BUFFER_TOO_SMALL`]. This
/// is a read-only projection of authoritative state (no mirror), the value the
/// host broadcasts so a browser can render/reconcile against authoritative positions.
///
/// # Safety
/// `sim` is a valid handle; `out_floats`/`cap` describe a writable `f32` buffer;
/// `out_count` is valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_render_view_write(
    sim: *mut Session,
    out_floats: *mut f32,
    cap: usize,
    out_count: *mut usize,
) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let count_out = out_count.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        let view = s.render_view();
        if view.len() > cap {
            return Err(STATUS_ERR_BUFFER_TOO_SMALL);
        }
        if out_floats.is_null() && !view.is_empty() {
            return Err(STATUS_ERR_INVALID_ARG);
        }
        if !view.is_empty() {
            std::ptr::copy_nonoverlapping(view.as_ptr(), out_floats, view.len());
        }
        *count_out = view.len();
        Ok(())
    })
}

// --- replay ---

/// Write the exported replay-record length (in bytes) to `out_len`.
///
/// # Safety
/// `sim` is a valid handle; `out_len` is valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_export_replay_len(
    sim: *mut Session,
    out_len: *mut usize,
) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let len_out = out_len.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        *len_out = s.export_replay().len();
        Ok(())
    })
}

/// Write the exported replay-record bytes into a caller buffer. Query
/// [`axiom_sim_export_replay_len`] first.
///
/// # Safety
/// `sim` is a valid handle; `out_ptr`/`out_capacity` describe a writable buffer;
/// `out_written` is valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_export_replay_write(
    sim: *mut Session,
    out_ptr: *mut u8,
    out_capacity: usize,
    out_written: *mut usize,
) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let bytes = s.export_replay();
        copy_out(&bytes, out_ptr, out_capacity, out_written)
    })
}

/// Verify a replay record from tick zero. Stateless (no sim handle): it builds a
/// fresh worker from the record's seed and compares per-tick hashes. Writes
/// `out_matched` (1/0), `out_first_divergence_tick` (meaningful when not matched),
/// and `out_final_hash`. Malformed bytes fail with [`STATUS_ERR_DESERIALIZE`].
///
/// # Safety
/// `ptr`/`len` describe a readable buffer (or null/0); the out pointers are valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_verify_replay(
    ptr: *const u8,
    len: usize,
    out_matched: *mut u32,
    out_first_divergence_tick: *mut u64,
    out_final_hash: *mut u64,
) -> i32 {
    run_stateless(|| {
        let bytes = in_slice(ptr, len)?;
        let matched_out = out_matched.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        let first_out = out_first_divergence_tick
            .as_mut()
            .ok_or(STATUS_ERR_INVALID_ARG)?;
        let final_out = out_final_hash.as_mut().ok_or(STATUS_ERR_INVALID_ARG)?;
        let record = replay::ReplayRecord::decode(bytes).map_err(|_| STATUS_ERR_DESERIALIZE)?;
        let outcome = replay::verify(&record);
        *matched_out = outcome.matched as u32;
        *first_out = outcome.first_divergence_tick;
        *final_out = outcome.final_hash;
        Ok(())
    })
}

// --- last error ---

/// The last error code recorded on the handle (`0` if none or null).
///
/// # Safety
/// `sim` is null or a valid handle.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_last_error_code(sim: *mut Session) -> u32 {
    catch_unwind(AssertUnwindSafe(|| {
        sim.as_ref().map(Session::last_error_code).unwrap_or(0)
    }))
    .unwrap_or(0)
}

/// Write the last error message (UTF-8, no NUL terminator) into a caller buffer.
///
/// # Safety
/// `sim` is a valid handle; `out_ptr`/`out_capacity` describe a writable buffer;
/// `out_written` is valid.
#[no_mangle]
pub unsafe extern "C" fn axiom_sim_last_error_message_write(
    sim: *mut Session,
    out_ptr: *mut u8,
    out_capacity: usize,
    out_written: *mut usize,
) -> i32 {
    run(sim, || {
        let s = sess(sim)?;
        let bytes = s.last_error_message().as_bytes().to_vec();
        copy_out(&bytes, out_ptr, out_capacity, out_written)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(dx: f32, dy: f32) -> Vec<u8> {
        crate::ruleset::encode_move(dx, dy)
    }

    #[test]
    fn version_exports_are_stable() {
        assert_eq!(axiom_worker_version_major(), WORKER_VERSION_MAJOR);
        assert_eq!(axiom_worker_version_minor(), WORKER_VERSION_MINOR);
        assert_eq!(axiom_worker_version_patch(), WORKER_VERSION_PATCH);
        assert_eq!(axiom_worker_protocol_version(), WORKER_PROTOCOL_VERSION);
    }

    #[test]
    fn create_and_destroy_is_safe() {
        let sim = axiom_sim_create(1, 2, 16_666_667);
        assert!(!sim.is_null());
        unsafe { axiom_sim_destroy(sim) };
    }

    #[test]
    fn create_rejects_invalid_arguments() {
        assert!(axiom_sim_create(1, 0, 16_666_667).is_null());
        assert!(axiom_sim_create(1, 2, 0).is_null());
        assert!(axiom_sim_create(1, MAX_PLAYERS_CAP + 1, 1).is_null());
    }

    #[test]
    fn c_abi_never_panics_on_null_sim() {
        let mut out: u64 = 0;
        let mut reason: u32 = 0;
        unsafe {
            assert_eq!(
                axiom_sim_load_state(std::ptr::null_mut(), std::ptr::null(), 0),
                STATUS_ERR_NULL_HANDLE
            );
            assert_eq!(
                axiom_sim_submit_intent(
                    std::ptr::null_mut(),
                    0,
                    1,
                    0,
                    std::ptr::null(),
                    0,
                    &mut reason
                ),
                STATUS_ERR_NULL_HANDLE
            );
            assert_eq!(
                axiom_sim_advance_tick(std::ptr::null_mut(), 0, &mut out, &mut out),
                STATUS_ERR_NULL_HANDLE
            );
            assert_eq!(
                axiom_sim_state_hash(std::ptr::null_mut(), &mut out),
                STATUS_ERR_NULL_HANDLE
            );
            assert_eq!(axiom_sim_last_error_code(std::ptr::null_mut()), 0);
            // Destroying null is a no-op.
            axiom_sim_destroy(std::ptr::null_mut());
        }
    }

    #[test]
    fn c_abi_never_panics_on_garbage_payload() {
        let sim = axiom_sim_create(1, 2, 16_666_667);
        let mut reason: u32 = 0;
        let garbage = [0xFFu8; 3];
        unsafe {
            // Malformed payload → rejected (not a panic, not OK).
            let status =
                axiom_sim_submit_intent(sim, 0, 1, 0, garbage.as_ptr(), garbage.len(), &mut reason);
            assert_eq!(status, STATUS_REJECTED);
            assert_eq!(reason, REASON_MALFORMED);
            // Garbage replay bytes → deserialize error, not a panic.
            let mut matched: u32 = 9;
            let mut first: u64 = 0;
            let mut final_hash: u64 = 0;
            assert_eq!(
                axiom_sim_verify_replay(
                    garbage.as_ptr(),
                    garbage.len(),
                    &mut matched,
                    &mut first,
                    &mut final_hash
                ),
                STATUS_ERR_DESERIALIZE
            );
            axiom_sim_destroy(sim);
        }
    }

    #[test]
    fn snapshot_len_write_round_trips_and_rejects_small_buffers() {
        let sim = axiom_sim_create(1, 2, 16_666_667);
        unsafe {
            let mut len: usize = 0;
            assert_eq!(axiom_sim_snapshot_len(sim, &mut len), STATUS_OK);
            assert!(len > 0);

            // Too-small buffer is rejected, not a panic.
            let mut tiny = vec![0u8; len - 1];
            let (mut written, mut hash) = (0usize, 0u64);
            assert_eq!(
                axiom_sim_snapshot_write(
                    sim,
                    tiny.as_mut_ptr(),
                    tiny.len(),
                    &mut written,
                    &mut hash
                ),
                STATUS_ERR_BUFFER_TOO_SMALL
            );

            // Exact buffer writes the snapshot and its hash.
            let mut buf = vec![0u8; len];
            assert_eq!(
                axiom_sim_snapshot_write(sim, buf.as_mut_ptr(), buf.len(), &mut written, &mut hash),
                STATUS_OK
            );
            assert_eq!(written, len);
            let mut direct_hash: u64 = 0;
            assert_eq!(axiom_sim_state_hash(sim, &mut direct_hash), STATUS_OK);
            assert_eq!(hash, direct_hash);
            axiom_sim_destroy(sim);
        }
    }

    #[test]
    fn session_snapshot_round_trips_through_the_c_abi() {
        let sim = axiom_sim_create(1, 2, 16_666_667);
        unsafe {
            // Drive a tick so the sim carries real state.
            let p = payload(0.4, 0.0);
            let mut reason: u32 = 0;
            let (mut tick, mut hash) = (0u64, 0u64);
            axiom_sim_submit_intent(sim, 0, 1, 0, p.as_ptr(), p.len(), &mut reason);
            axiom_sim_advance_tick(sim, 1, &mut tick, &mut hash);

            // Size-probe, reject a short buffer, then fill a host-owned buffer.
            let mut len: usize = 0;
            assert_eq!(axiom_session_snapshot_len(sim, &mut len), STATUS_OK);
            assert!(len > 0);
            let mut tiny = vec![0u8; len - 1];
            let mut written: usize = 0;
            assert_eq!(
                axiom_session_snapshot_write(sim, tiny.as_mut_ptr(), tiny.len(), &mut written),
                STATUS_ERR_BUFFER_TOO_SMALL
            );
            let mut buf = vec![0u8; len];
            assert_eq!(
                axiom_session_snapshot_write(sim, buf.as_mut_ptr(), buf.len(), &mut written),
                STATUS_OK
            );
            assert_eq!(written, len);

            // Restore into a fresh sim (different seed) and re-snapshot: the blob
            // carries the full session, so the re-snapshot is byte-identical.
            let fresh = axiom_sim_create(9, 2, 16_666_667);
            assert_eq!(
                axiom_session_restore(fresh, buf.as_ptr(), buf.len()),
                STATUS_OK
            );
            let mut len2: usize = 0;
            assert_eq!(axiom_session_snapshot_len(fresh, &mut len2), STATUS_OK);
            let mut buf2 = vec![0u8; len2];
            let mut written2: usize = 0;
            assert_eq!(
                axiom_session_snapshot_write(fresh, buf2.as_mut_ptr(), buf2.len(), &mut written2),
                STATUS_OK
            );
            assert_eq!(buf2, buf, "restored session re-snapshots byte-identically");

            // Garbage is a clean deserialize error, never a panic.
            let garbage = [1u8, 2, 3, 4, 5];
            assert_eq!(
                axiom_session_restore(fresh, garbage.as_ptr(), garbage.len()),
                STATUS_ERR_DESERIALIZE
            );
            axiom_sim_destroy(sim);
            axiom_sim_destroy(fresh);
        }
    }

    #[test]
    fn advance_writes_tick_and_hash() {
        let sim = axiom_sim_create(1, 2, 16_666_667);
        let mut reason: u32 = 0;
        let p = payload(0.5, 0.0);
        unsafe {
            assert_eq!(
                axiom_sim_submit_intent(sim, 0, 1, 0, p.as_ptr(), p.len(), &mut reason),
                STATUS_OK
            );
            let (mut tick, mut hash) = (0u64, 0u64);
            assert_eq!(
                axiom_sim_advance_tick(sim, 1, &mut tick, &mut hash),
                STATUS_OK
            );
            assert_eq!(tick, 1);
            assert_ne!(hash, 0);
            axiom_sim_destroy(sim);
        }
    }

    #[test]
    fn render_view_reports_authoritative_positions_and_rejects_small_buffers() {
        let sim = axiom_sim_create(1, 2, 16_666_667);
        unsafe {
            // Too-small buffer is rejected, not a panic.
            let mut tiny = [0.0f32; 2];
            let mut count: usize = 0;
            assert_eq!(
                axiom_sim_render_view_write(sim, tiny.as_mut_ptr(), tiny.len(), &mut count),
                STATUS_ERR_BUFFER_TOO_SMALL
            );

            let mut buf = [0.0f32; 4];
            assert_eq!(
                axiom_sim_render_view_write(sim, buf.as_mut_ptr(), buf.len(), &mut count),
                STATUS_OK
            );
            assert_eq!(count, 4);
            assert_eq!(buf, [-1.5, 0.0, 1.5, 0.0]);

            // A move updates the authoritative view.
            let p = payload(0.5, 0.0);
            let mut reason = 0u32;
            axiom_sim_submit_intent(sim, 0, 1, 0, p.as_ptr(), p.len(), &mut reason);
            let (mut tick, mut hash) = (0u64, 0u64);
            axiom_sim_advance_tick(sim, 1, &mut tick, &mut hash);
            axiom_sim_render_view_write(sim, buf.as_mut_ptr(), buf.len(), &mut count);
            assert_eq!(buf[0], -1.0);
            axiom_sim_destroy(sim);
        }
    }

    #[test]
    fn export_then_verify_round_trips_through_the_c_abi() {
        let sim = axiom_sim_create(3, 2, 16_666_667);
        let p = payload(0.4, 0.0);
        unsafe {
            let mut reason: u32 = 0;
            let (mut tick, mut hash) = (0u64, 0u64);
            axiom_sim_submit_intent(sim, 0, 1, 0, p.as_ptr(), p.len(), &mut reason);
            axiom_sim_advance_tick(sim, 1, &mut tick, &mut hash);

            let mut len: usize = 0;
            assert_eq!(axiom_sim_export_replay_len(sim, &mut len), STATUS_OK);
            let mut buf = vec![0u8; len];
            let mut written: usize = 0;
            assert_eq!(
                axiom_sim_export_replay_write(sim, buf.as_mut_ptr(), buf.len(), &mut written),
                STATUS_OK
            );

            let (mut matched, mut first, mut final_hash) = (0u32, 0u64, 0u64);
            assert_eq!(
                axiom_sim_verify_replay(
                    buf.as_ptr(),
                    written,
                    &mut matched,
                    &mut first,
                    &mut final_hash
                ),
                STATUS_OK
            );
            assert_eq!(matched, 1);
            assert_eq!(final_hash, hash);
            axiom_sim_destroy(sim);
        }
    }
}
