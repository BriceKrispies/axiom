//! The out-of-process Axiom sim worker.
//!
//! One `Session` (its parameters fixed by CLI args), served to a single host over
//! a local TCP socket using the [`axiom_netplay_ffi::ipc`] protocol. The host
//! spawns this process per room; running each authoritative sim in its own process
//! gives crash isolation — a worker fault takes down only its room, and the host's
//! supervisor respawns it and restores the last snapshot.
//!
//! Handshake: the worker binds an ephemeral loopback port and prints it (one line)
//! on stdout, so the host learns the port without a fixed-port race. It then
//! accepts exactly one connection and serves request frames until the host closes.
//!
//! This is app/tooling-tier (a server-side process), never compiled to wasm.

use std::io::{Read, Write};
use std::net::TcpListener;

use axiom_netplay_ffi::ipc;
use axiom_netplay_ffi::session::Session;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed = arg_u64(&args, "--seed").unwrap_or(1);
    let max_players = arg_u64(&args, "--max-players").unwrap_or(2) as u32;
    let fixed_step = arg_u64(&args, "--fixed-step").unwrap_or(16_666_667);

    let mut session = Session::new(seed, max_players, fixed_step);

    let listener = TcpListener::bind("127.0.0.1:0").expect("worker: bind loopback");
    let port = listener.local_addr().expect("worker: local_addr").port();
    println!("{port}");
    std::io::stdout().flush().ok();

    let (mut stream, _peer) = listener.accept().expect("worker: accept host");
    stream.set_nodelay(true).ok();
    while let Some(frame) = read_frame(&mut stream) {
        let response = ipc::handle(&mut session, &frame);
        if write_frame(&mut stream, &response).is_err() {
            break;
        }
    }
}

/// Read one u32-length-prefixed frame, or `None` at end-of-stream.
fn read_frame(stream: &mut impl Read) -> Option<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).ok()?;
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// Write one u32-length-prefixed frame.
fn write_frame(stream: &mut impl Write, data: &[u8]) -> std::io::Result<()> {
    stream.write_all(&(data.len() as u32).to_le_bytes())?;
    stream.write_all(data)?;
    stream.flush()
}

/// Parse `--key <u64>` from the argument list.
fn arg_u64(args: &[String], key: &str) -> Option<u64> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}
