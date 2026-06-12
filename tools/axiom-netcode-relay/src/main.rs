//! `axiom-netcode-relay` — a dumb WebSocket relay for the live
//! deterministic-lockstep demo.
//!
//! It assigns each connecting browser a **stable slot id** (1 or 2 — freed when
//! that peer leaves, so two live browsers are always peers `1` and `2`, matching
//! the client's fixed peer set), and once **both** slots are filled it broadcasts
//! a `go` so the two clients start their simulations together (no one submits
//! input before both are present, so there is no lost-warmup problem). It then
//! forwards every *binary* frame from one peer to the other. It never decodes
//! netcode bytes and runs no game logic — ordering and the lockstep gate are the
//! clients' job (`axiom-netcode`). Repo tooling: a Tool by its `tools/` location,
//! outside the engine dependency graph and the coverage gate.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::tungstenite::Message;

/// The default address the relay listens on.
const DEFAULT_ADDR: &str = "127.0.0.1:9001";

/// The number of player slots (this demo is two-player).
const MAX_PEERS: usize = 2;

/// What travels on the broadcast bus: a "start the game" signal, or one peer's
/// input frame tagged with its id.
#[derive(Clone)]
enum Bus {
    Go,
    Data(u64, Vec<u8>),
}

/// Shared relay state: the live player slots and the broadcast bus.
struct Relay {
    slots: Mutex<[bool; MAX_PEERS]>,
    tx: broadcast::Sender<Bus>,
}

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

/// Accept connections forever, serving each on the shared relay state.
async fn run_relay(listener: TcpListener) {
    let (tx, _rx) = broadcast::channel::<Bus>(1024);
    let relay = Arc::new(Relay {
        slots: Mutex::new([false; MAX_PEERS]),
        tx,
    });
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let relay = relay.clone();
        let rx = relay.tx.subscribe();
        tokio::spawn(serve_peer(stream, relay, rx));
    }
}

/// Claim the lowest free slot, returning `(id, all_slots_now_filled)`, or `None`
/// if the game is already full.
async fn claim_slot(relay: &Relay) -> Option<(u64, bool)> {
    let mut slots = relay.slots.lock().await;
    let free = slots.iter().position(|taken| !*taken)?;
    slots[free] = true;
    let all_filled = slots.iter().all(|taken| *taken);
    Some(((free + 1) as u64, all_filled))
}

/// Release a slot when its peer leaves.
async fn free_slot(relay: &Relay, id: u64) {
    if let Some(slot) = relay.slots.lock().await.get_mut((id - 1) as usize) {
        *slot = false;
    }
}

/// Serve one peer: handshake, claim a slot id, broadcast `go` if it completes the
/// pair, then bridge its socket to the bus until it leaves.
async fn serve_peer(stream: TcpStream, relay: Arc<Relay>, mut rx: broadcast::Receiver<Bus>) {
    let Ok(ws) = tokio_tungstenite::accept_async(stream).await else {
        println!("relay: a peer failed the WebSocket handshake");
        return;
    };
    let (mut sink, mut source) = ws.split();

    let Some((id, all_filled)) = claim_slot(&relay).await else {
        let _ = sink.send(Message::text("full")).await;
        println!("relay: rejected an extra peer (game is full, max {MAX_PEERS})");
        return;
    };

    // The peer id arrives as a text frame; all netcode payloads are binary.
    if sink.send(Message::text(id.to_string())).await.is_err() {
        free_slot(&relay, id).await;
        return;
    }
    println!("relay: peer {id} connected");
    // Completing the pair starts both clients together (no early-input loss).
    if all_filled {
        println!("relay: both players present — sending go");
        let _ = relay.tx.send(Bus::Go);
    }

    let mut sent_in = 0u64;
    let mut sent_out = 0u64;
    loop {
        tokio::select! {
            incoming = source.next() => match incoming {
                Some(Ok(msg)) if msg.is_binary() => {
                    sent_in += 1;
                    if sent_in == 1 || sent_in % 300 == 0 {
                        println!("relay: peer {id} -> {sent_in} input frames forwarded");
                    }
                    let _ = relay.tx.send(Bus::Data(id, msg.into_data().to_vec()));
                }
                Some(Ok(msg)) if msg.is_close() => break,
                Some(Ok(_)) => {}                // ignore text / ping / pong
                Some(Err(_)) | None => break,    // errored or closed
            },
            relayed = rx.recv() => match relayed {
                Ok(Bus::Go) => {
                    if sink.send(Message::text("go")).await.is_err() {
                        break;
                    }
                }
                Ok(Bus::Data(from, data)) if from != id => {
                    sent_out += 1;
                    if sent_out == 1 {
                        println!("relay: peer {id} <- first frame from peer {from}");
                    }
                    if sink.send(Message::binary(data)).await.is_err() {
                        break;
                    }
                }
                Ok(Bus::Data(..)) => {}           // our own echo — skip
                Err(broadcast::error::RecvError::Lagged(_)) => {} // fell behind; keep going
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
    free_slot(&relay, id).await;
    println!("relay: peer {id} disconnected (in={sent_in}, out={sent_out})");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::connect_async;

    async fn next_text(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> String {
        ws.next().await.unwrap().unwrap().into_text().unwrap()
    }

    #[tokio::test]
    async fn pairs_two_peers_then_relays_input_without_echo() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(run_relay(listener));
        let url = format!("ws://{addr}");

        let (mut a, _) = connect_async(&url).await.unwrap();
        let id_a = next_text(&mut a).await; // "1" (alone — no go yet)
        let (mut b, _) = connect_async(&url).await.unwrap();
        let id_b = next_text(&mut b).await; // "2" (completes the pair)

        let mut ids = [id_a.as_str(), id_b.as_str()];
        ids.sort_unstable();
        assert_eq!(ids, ["1", "2"]);

        // Both peers now receive the `go` to start together.
        assert_eq!(next_text(&mut a).await, "go");
        assert_eq!(next_text(&mut b).await, "go");

        // A binary frame from A reaches B unchanged, and A gets no echo of it.
        a.send(Message::binary(vec![7, 8, 9])).await.unwrap();
        let got = b.next().await.unwrap().unwrap();
        assert!(got.is_binary());
        assert_eq!(got.into_data().to_vec(), vec![7, 8, 9]);
        b.send(Message::binary(vec![1])).await.unwrap();
        let got_a = a.next().await.unwrap().unwrap();
        assert_eq!(got_a.into_data().to_vec(), vec![1]);
    }

    #[tokio::test]
    async fn a_freed_slot_is_reused_so_ids_do_not_climb() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(run_relay(listener));
        let url = format!("ws://{addr}");

        let (mut a, _) = connect_async(&url).await.unwrap();
        assert_eq!(next_text(&mut a).await, "1");
        a.close(None).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // The next peer reuses slot 1 rather than climbing to 2 — the fix for
        // "both browsers black": a reconnect must not push ids past the client's
        // fixed [1, 2] peer set.
        let (mut c, _) = connect_async(&url).await.unwrap();
        assert_eq!(next_text(&mut c).await, "1");
    }

    #[tokio::test]
    async fn a_third_peer_is_rejected_as_full() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(run_relay(listener));
        let url = format!("ws://{addr}");

        let (mut a, _) = connect_async(&url).await.unwrap();
        let _ = next_text(&mut a).await;
        let (mut b, _) = connect_async(&url).await.unwrap();
        let _ = next_text(&mut b).await;
        // The third peer is told the game is full.
        let (mut c, _) = connect_async(&url).await.unwrap();
        assert_eq!(next_text(&mut c).await, "full");
    }
}
