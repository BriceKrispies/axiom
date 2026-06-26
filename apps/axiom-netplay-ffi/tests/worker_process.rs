//! The out-of-process worker proves determinism ACROSS a real process boundary:
//! spawned as a child, driven over its local socket with the same scripted inputs
//! as an in-process `Session`, it must produce byte-identical snapshots and hashes
//! at every tick. This is the cross-check that the IPC marshalling and the
//! in-process path are the same authority.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStdout, Command, Stdio};

use axiom_netplay_ffi::ipc::{Request, Response};
use axiom_netplay_ffi::ruleset;
use axiom_netplay_ffi::session::Session;

/// Spawn the worker binary and return the child plus a connected socket, reading
/// the port off the worker's stdout handshake.
fn spawn_worker(seed: u64, max_players: u32, fixed_step: u64) -> (Child, TcpStream) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_axiom-netplay-worker"))
        .args([
            "--seed",
            &seed.to_string(),
            "--max-players",
            &max_players.to_string(),
            "--fixed-step",
            &fixed_step.to_string(),
        ])
        // The test `kill()`s this child, which under `cargo llvm-cov` would leave a
        // half-written, corrupt `.profraw` in the shared profile pool and break the
        // workspace-wide `llvm-profdata merge`. The worker is an app binary (outside
        // the coverage scope), so its profile is not wanted: clear the instrumented
        // output path for the child so it emits none. A no-op when not under coverage.
        .env_remove("LLVM_PROFILE_FILE")
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn worker");

    let stdout: ChildStdout = child.stdout.take().expect("worker stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read port handshake");
    let port: u16 = line.trim().parse().expect("port number");

    let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect worker");
    stream.set_nodelay(true).ok();
    (child, stream)
}

/// Send one request frame, read and decode the response frame.
fn call(stream: &mut TcpStream, request: &Request) -> Response {
    let body = request.encode();
    stream
        .write_all(&(body.len() as u32).to_le_bytes())
        .expect("write len");
    stream.write_all(&body).expect("write body");
    stream.flush().expect("flush");

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).expect("read len");
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).expect("read body");
    Response::decode(&buf).expect("decode response")
}

#[test]
fn out_of_process_worker_matches_in_process_determinism() {
    let (mut child, mut stream) = spawn_worker(7, 2, 16_666_667);
    let mut local = Session::new(7, 2, 16_666_667);

    // Drive 40 ticks; player 0 nudges right every third tick. Both the worker and
    // the in-process session must agree on tick count and state hash every step.
    for tick in 0..40u64 {
        if tick % 3 == 0 {
            let payload = ruleset::encode_move(0.1, 0.0);
            let seq = tick + 1;
            let reason = call(
                &mut stream,
                &Request::SubmitIntent {
                    player: 0,
                    sequence: seq,
                    predicted_tick: 0,
                    payload: payload.clone(),
                },
            );
            assert_eq!(reason, Response::Reason(0));
            local.submit_intent(0, seq, 0, &payload);
        }

        let advanced = call(&mut stream, &Request::AdvanceTick { target: tick });
        let (lt, lh) = local.advance();
        assert_eq!(advanced, Response::Tick { tick: lt, hash: lh });
    }

    // Full snapshot parity — byte-equality is the determinism proof.
    match call(&mut stream, &Request::Snapshot) {
        Response::Snapshot { hash, bytes } => {
            assert_eq!(hash, local.state_hash());
            assert_eq!(bytes, local.snapshot());
        }
        other => panic!("expected Snapshot, got {other:?}"),
    }

    drop(stream);
    child.kill().ok();
    child.wait().ok();
}

#[test]
fn a_restored_worker_resumes_identical_state() {
    // Run a worker, snapshot it mid-game, then prove a FRESH worker restored from
    // those bytes is byte-identical — the basis of the host's crash-recovery.
    let (mut a, mut sa) = spawn_worker(3, 2, 16_666_667);
    for tick in 0..10u64 {
        call(
            &mut sa,
            &Request::SubmitIntent {
                player: 0,
                sequence: tick + 1,
                predicted_tick: 0,
                payload: ruleset::encode_move(0.2, 0.0),
            },
        );
        call(&mut sa, &Request::AdvanceTick { target: tick });
    }
    let (snapshot, hash) = match call(&mut sa, &Request::Snapshot) {
        Response::Snapshot { hash, bytes } => (bytes, hash),
        other => panic!("expected Snapshot, got {other:?}"),
    };

    let (mut b, mut sb) = spawn_worker(3, 2, 16_666_667);
    assert_eq!(
        call(
            &mut sb,
            &Request::LoadState {
                bytes: snapshot.clone()
            }
        ),
        Response::Loaded(true)
    );
    assert_eq!(
        call(&mut sb, &Request::StateHash),
        Response::StateHash(hash)
    );
    match call(&mut sb, &Request::Snapshot) {
        Response::Snapshot { bytes, .. } => assert_eq!(bytes, snapshot),
        other => panic!("expected Snapshot, got {other:?}"),
    }

    drop(sa);
    drop(sb);
    a.kill().ok();
    a.wait().ok();
    b.kill().ok();
    b.wait().ok();
}
