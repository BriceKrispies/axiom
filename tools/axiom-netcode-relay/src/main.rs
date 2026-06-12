//! `axiom-netcode-relay` — a dumb WebSocket broadcast relay for the live
//! deterministic-lockstep demo.
//!
//! It assigns each connecting browser a peer id (a text frame) and forwards
//! every *binary* frame from one peer to all the others. It never decodes
//! netcode bytes and runs no game logic — input ordering and the lockstep gate
//! are entirely the clients' job (`axiom-netcode`). This is repo tooling: a Tool
//! by its `tools/` location, outside the engine dependency graph and the
//! coverage gate.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;

/// The default address the relay listens on.
const DEFAULT_ADDR: &str = "127.0.0.1:9001";

#[tokio::main]
async fn main() {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_ADDR.to_string());
    let listener = TcpListener::bind(&addr)
        .await
        .expect("relay: failed to bind the listen address");
    println!("axiom-netcode-relay listening on ws://{addr}");
    println!("open the netplay page in two WebGPU browsers — each becomes a peer.");
    run_relay(listener).await;
}

/// Accept connections forever, assigning each a monotonic peer id and bridging
/// it to a shared broadcast bus.
async fn run_relay(listener: TcpListener) {
    let (tx, _rx) = broadcast::channel::<(u64, Vec<u8>)>(1024);
    let next_id = Arc::new(AtomicU64::new(1));
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let id = next_id.fetch_add(1, Ordering::SeqCst);
        let tx = tx.clone();
        let rx = tx.subscribe();
        tokio::spawn(serve_peer(stream, id, tx, rx));
    }
}

/// Serve one peer: complete the WebSocket handshake, hand it its id, then bridge
/// its socket to the bus — forwarding others' binary frames down to it, and its
/// own binary frames up to everyone else.
async fn serve_peer(
    stream: TcpStream,
    id: u64,
    tx: broadcast::Sender<(u64, Vec<u8>)>,
    mut rx: broadcast::Receiver<(u64, Vec<u8>)>,
) {
    let Ok(ws) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    let (mut sink, mut source) = ws.split();

    // The peer id arrives as a text frame; all netcode payloads are binary.
    if sink.send(Message::text(id.to_string())).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            incoming = source.next() => match incoming {
                Some(Ok(msg)) if msg.is_binary() => {
                    let _ = tx.send((id, msg.into_data().to_vec()));
                }
                Some(Ok(msg)) if msg.is_close() => break,
                Some(Ok(_)) => {}                // ignore text / ping / pong
                Some(Err(_)) | None => break,    // errored or closed
            },
            relayed = rx.recv() => match relayed {
                Ok((from, data)) if from != id => {
                    if sink.send(Message::binary(data)).await.is_err() {
                        break;
                    }
                }
                Ok(_) => {}                       // our own echo — skip
                Err(broadcast::error::RecvError::Lagged(_)) => {} // fell behind; keep going
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::connect_async;

    #[tokio::test]
    async fn assigns_ids_and_relays_binary_between_peers_without_echo() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(run_relay(listener));
        let url = format!("ws://{addr}");

        let (mut a, _) = connect_async(&url).await.unwrap();
        let (mut b, _) = connect_async(&url).await.unwrap();

        // Each peer first receives its id as a text frame.
        let id_a = a.next().await.unwrap().unwrap();
        let id_b = b.next().await.unwrap().unwrap();
        assert!(id_a.is_text() && id_b.is_text());
        assert_ne!(id_a.into_text().unwrap(), id_b.into_text().unwrap());

        // A binary message from A reaches B unchanged.
        a.send(Message::binary(vec![7, 8, 9])).await.unwrap();
        let got = b.next().await.unwrap().unwrap();
        assert!(got.is_binary());
        assert_eq!(got.into_data().to_vec(), vec![7, 8, 9]);

        // A does not receive its own echo: a fresh message from B arrives at A
        // (proving A's stream is live and only carries others' traffic).
        b.send(Message::binary(vec![1])).await.unwrap();
        let got_a = a.next().await.unwrap().unwrap();
        assert_eq!(got_a.into_data().to_vec(), vec![1]);
    }
}
