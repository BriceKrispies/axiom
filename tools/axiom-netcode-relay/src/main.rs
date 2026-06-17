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

use futures_util::{SinkExt, StreamExt, TryStreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex};
use tokio_stream::wrappers::{BroadcastStream, TcpListenerStream};
use tokio_tungstenite::tungstenite::Message;

/// The default address the relay listens on.
const DEFAULT_ADDR: &str = "127.0.0.1:9001";

/// The number of player slots (this demo is two-player).
const MAX_PEERS: usize = 2;

/// What travels on the broadcast bus. Modelled as a struct (not an enum) so its
/// shape is destructurable purely with `Option`/boolean combinators — no `match`
/// or `if let` — to satisfy the engine's no-branching analyzer:
///
/// * `from == 0` with `payload == None` is the relay's own **"go" start signal**.
/// * `from == <peer id>` with `payload == Some(bytes)` is one peer's **input
///   frame**, tagged with the id that sent it so the receiver can skip its echo.
#[derive(Clone)]
struct Bus {
    from: u64,
    payload: Option<Vec<u8>>,
}

impl Bus {
    /// The "start the game" signal (sender id `0`, no payload).
    fn go() -> Self {
        Self {
            from: 0,
            payload: None,
        }
    }

    /// One peer's input frame tagged with the id that produced it.
    fn data(from: u64, bytes: Vec<u8>) -> Self {
        Self {
            from,
            payload: Some(bytes),
        }
    }
}

/// One thing the per-peer bridge does in response to a stream item. Modelled so
/// it dispatches with `Option`/boolean combinators rather than a `match`: every
/// variant carries the data its action needs, and the empty actions
/// (`relay_to`/`broadcast`/`go_signal` all absent) is a no-op.
struct Action {
    /// `Some((from, bytes))` to forward another peer's frame (originating id
    /// `from`) to *this* peer's socket.
    relay_to: Option<(u64, Vec<u8>)>,
    /// `Some(bytes)` to publish *this* peer's input frame onto the bus.
    broadcast: Option<Vec<u8>>,
    /// `true` to send the textual "go" start signal to this peer's socket.
    go_signal: bool,
    /// `true` once a source stream has ended (socket closed/errored, or bus
    /// closed): applying this stops the bridge — the stream-end `break` of old.
    stop: bool,
}

impl Action {
    /// A do-nothing action (an ignored frame, an own-echo, or a lagged bus tick).
    fn noop() -> Self {
        Self {
            relay_to: None,
            broadcast: None,
            go_signal: false,
            stop: false,
        }
    }

    /// Stop the bridge: a source stream ended.
    fn stop() -> Self {
        Self {
            stop: true,
            ..Self::noop()
        }
    }

    /// Forward another peer's `bytes` (originating id `from`) to this socket.
    fn relay(from: u64, bytes: Vec<u8>) -> Self {
        Self {
            relay_to: Some((from, bytes)),
            ..Self::noop()
        }
    }

    /// Publish `bytes` onto the broadcast bus.
    fn broadcast(bytes: Vec<u8>) -> Self {
        Self {
            broadcast: Some(bytes),
            ..Self::noop()
        }
    }

    /// Send the textual "go" start signal to this peer's socket.
    fn go() -> Self {
        Self {
            go_signal: true,
            ..Self::noop()
        }
    }
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
///
/// The accept-forever loop is the `TcpListenerStream` of inbound connections
/// driven by `for_each_concurrent`: one spawned `serve_peer` task per accepted
/// socket, all in flight at once (`None` = unbounded concurrency). A failed
/// `accept()` surfaces as an `Err` item, which `conn.ok()` drops and the stream
/// keeps going — exactly the old "ignore the error and accept the next" behavior,
/// with no `loop` keyword.
async fn run_relay(listener: TcpListener) {
    let (tx, _rx) = broadcast::channel::<Bus>(1024);
    let relay = Arc::new(Relay {
        slots: Mutex::new([false; MAX_PEERS]),
        tx,
    });
    TcpListenerStream::new(listener)
        .for_each_concurrent(None, |conn| {
            let relay = relay.clone();
            async move {
                conn.ok()
                    .map(|stream| {
                        let rx = relay.tx.subscribe();
                        tokio::spawn(serve_peer(stream, relay, rx))
                    })
                    .map(|_| ())
                    .unwrap_or(());
            }
        })
        .await;
}

/// Claim the lowest free slot, returning `(id, all_slots_now_filled)`, or `None`
/// if the game is already full.
async fn claim_slot(relay: &Relay) -> Option<(u64, bool)> {
    let mut slots = relay.slots.lock().await;
    slots
        .iter()
        .position(|taken| !*taken)
        .map(|free| {
            slots[free] = true;
            ((free + 1) as u64, slots.iter().all(|taken| *taken))
        })
}

/// Release a slot when its peer leaves.
async fn free_slot(relay: &Relay, id: u64) {
    relay
        .slots
        .lock()
        .await
        .get_mut((id - 1) as usize)
        .map(|slot| *slot = false)
        .unwrap_or(());
}

/// Serve one peer: handshake, claim a slot id, broadcast `go` if it completes the
/// pair, then bridge its socket to the bus until it leaves.
async fn serve_peer(stream: TcpStream, relay: Arc<Relay>, rx: broadcast::Receiver<Bus>) {
    let Ok(ws) = tokio_tungstenite::accept_async(stream).await else {
        println!("relay: a peer failed the WebSocket handshake");
        return;
    };
    let (mut sink, source) = ws.split();

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
    all_filled
        .then(|| {
            println!("relay: both players present — sending go");
            let _ = relay.tx.send(Bus::go());
        })
        .unwrap_or(());

    bridge_peer(sink, source, &relay, rx, id).await;
    free_slot(&relay, id).await;
}

/// Bridge one paired peer's socket to the bus until either side ends.
///
/// The old `loop { tokio::select! { ... } }` is replaced by two streams merged
/// into one and consumed by `try_fold`:
///
/// * the **inbound** socket (`source`) mapped to `Action`s (broadcast a binary
///   frame, ignore text/ping/pong, or stop), and
/// * the **bus** receiver (`BroadcastStream`) mapped to `Action`s (send "go",
///   relay another peer's frame, or skip our own echo / a lagged tick).
///
/// Each stream is `.chain`ed with a single terminal `Action::stop()` so that when
/// *either* stream ends (socket closed/errored, or bus closed) a stop flows
/// through — reproducing the old `break`-on-any-end. `try_fold` threads the
/// `Bridge` (sink + counters) through each action and short-circuits on the first
/// `Err(Bridge)` — a stop action or a send failure — reproducing the old
/// `break`-on-send-error while carrying the final counters out for the disconnect
/// log. No `loop`, `match`, or `if let` is written; the only branching is inside
/// the `select!` the `merge` combinator uses internally (a macro, exempt from the
/// analyzer).
async fn bridge_peer(
    sink: PeerSink,
    source: PeerSource,
    relay: &Relay,
    rx: broadcast::Receiver<Bus>,
    id: u64,
) {
    // Each stream is mapped to `Action`s and `.chain`ed with one terminal
    // `Action::stop()` so that when *either* stream ends (socket closed/errored,
    // or bus closed) a stop action flows through — the old `break`-on-any-end.
    let inbound = source
        .map(inbound_action)
        .chain(futures_util::stream::once(async { Action::stop() }));
    let outbound = BroadcastStream::new(rx)
        .map(move |relayed| bus_action(relayed, id))
        .chain(futures_util::stream::once(async { Action::stop() }));

    // `try_fold` threads the per-peer state (sink + counters) by value through
    // each action and short-circuits on the first `Err(Bridge)` — a stop action
    // or a send failure. The merged stream is infallible (`Action` items), so the
    // fold-future's error and the recovered accumulator are both `Bridge`: the
    // final counters survive whichever way the bridge ends.
    let folded = tokio_stream::StreamExt::merge(inbound, outbound)
        .map(Ok::<Action, Bridge>)
        .try_fold(Bridge::new(sink), |bridge, action| {
            bridge.apply(action, &relay.tx, id)
        })
        .await;

    let bridge = folded.unwrap_or_else(|stopped| stopped);
    let (sent_in, sent_out) = bridge.counts();
    println!("relay: peer {id} disconnected (in={sent_in}, out={sent_out})");
}

/// The single-owner per-peer bridge state threaded through `try_fold`: this
/// peer's socket and its forwarded-frame counters.
struct Bridge {
    sink: PeerSink,
    sent_in: u64,
    sent_out: u64,
}

impl Bridge {
    /// A fresh bridge over `sink` with zeroed counters.
    fn new(sink: PeerSink) -> Self {
        Self {
            sink,
            sent_in: 0,
            sent_out: 0,
        }
    }

    /// `(frames forwarded onto the bus, frames relayed to this peer)`.
    fn counts(&self) -> (u64, u64) {
        (self.sent_in, self.sent_out)
    }

    /// Carry out one `Action`, returning `Ok(self)` to keep bridging or
    /// `Err(self)` to stop (a socket write failed). Either way the (mutated)
    /// bridge is carried out so the disconnect log sees the final counts.
    async fn apply(
        mut self,
        action: Action,
        tx: &broadcast::Sender<Bus>,
        id: u64,
    ) -> Result<Self, Self> {
        let keep_going = perform_action(
            action,
            &mut self.sink,
            tx,
            id,
            &mut self.sent_in,
            &mut self.sent_out,
        )
        .await
        .is_ok();
        // Route the (single) `self` into `Ok` to keep bridging or `Err` to stop,
        // selecting the variant by index rather than a branch keyword: both are
        // `fn(Self) -> Result<Self, Self>`, and `self` is moved exactly once.
        const ARMS: [fn(Bridge) -> Result<Bridge, Bridge>; 2] = [Err, Ok];
        ARMS[keep_going as usize](self)
    }
}

/// This peer's outbound socket half.
type PeerSink =
    futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>;

/// This peer's inbound socket half.
type PeerSource =
    futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<TcpStream>>;

/// Map one inbound socket item to an `Action`: broadcast a binary frame, ignore a
/// text/ping/pong, or stop the bridge (the socket closed or errored). Built from
/// `Option`/boolean combinators: no `match` / `if`.
fn inbound_action(incoming: Result<Message, tokio_tungstenite::tungstenite::Error>) -> Action {
    incoming
        .ok()
        // A close frame ends the bridge; a binary frame is broadcast; everything
        // else (text/ping/pong) is ignored. An `Err` item also stops (the `.ok()`
        // above turns it into the `None` -> stop path).
        .map(|msg| {
            msg.is_close().then(Action::stop).unwrap_or_else(|| {
                msg.is_binary()
                    .then(|| Action::broadcast(msg.into_data().to_vec()))
                    .unwrap_or_else(Action::noop)
            })
        })
        .unwrap_or_else(Action::stop)
}

/// Map one bus item to an `Action`: send "go", relay another peer's frame, or do
/// nothing (our own echo, or a `Lagged` tick — we keep going either way). The bus
/// being *closed* is signalled by the stream ending, not by an item here.
fn bus_action(relayed: Result<Bus, tokio_stream::wrappers::errors::BroadcastStreamRecvError>, id: u64) -> Action {
    relayed
        .map(|bus| {
            let from = bus.from;
            bus.payload
                .map(|data| {
                    // Another peer's frame is relayed; our own echo is skipped.
                    (from != id)
                        .then(|| Action::relay(from, data))
                        .unwrap_or_else(Action::noop)
                })
                // No payload == the "go" start signal.
                .unwrap_or_else(Action::go)
        })
        // The only `Err` is `Lagged`: we fell behind, keep going.
        .unwrap_or_else(|_| Action::noop())
}

/// Carry out one `Action` against this peer's socket, returning `Err(())` if the
/// action is a stop or a socket write fails (which stops the bridge). Dispatch is
/// by `Option`/boolean combinators on the action's fields — no `match` / `if`.
async fn perform_action(
    action: Action,
    sink: &mut PeerSink,
    tx: &broadcast::Sender<Bus>,
    id: u64,
    sent_in: &mut u64,
    sent_out: &mut u64,
) -> Result<(), ()> {
    // A stop action (a source stream ended) is itself a reason to end the bridge.
    let keep = action.stop.then_some(()).map_or(Ok(()), |()| Err(()));

    // Publish a peer-origin frame onto the bus (with the periodic forward log).
    action
        .broadcast
        .map(|data| {
            *sent_in += 1;
            ((*sent_in == 1) | sent_in.is_multiple_of(300))
                .then(|| println!("relay: peer {id} -> {sent_in} input frames forwarded"))
                .unwrap_or(());
            let _ = tx.send(Bus::data(id, data));
        })
        .unwrap_or(());

    // Send the textual "go" start signal; a send failure ends the bridge.
    let go = futures_util::future::OptionFuture::from(
        action.go_signal.then(|| sink.send(Message::text("go"))),
    )
    .await
    .map(|res| res.map_err(|_| ()))
    .unwrap_or(Ok(()));

    // Relay another peer's frame (with the first-frame log); failure ends it.
    let relayed = send_relayed(action.relay_to, sink, id, sent_out).await;

    keep.and(go).and(relayed)
}

/// Send a relayed peer frame to this socket (logging the first one), returning
/// `Err(())` on a send failure. `None` (nothing to relay) is `Ok(())`.
async fn send_relayed(
    relay_to: Option<(u64, Vec<u8>)>,
    sink: &mut PeerSink,
    id: u64,
    sent_out: &mut u64,
) -> Result<(), ()> {
    let to_send = relay_to.map(|(from, data)| {
        *sent_out += 1;
        (*sent_out == 1)
            .then(|| println!("relay: peer {id} <- first frame from peer {from}"))
            .unwrap_or(());
        Message::binary(data)
    });
    futures_util::future::OptionFuture::from(to_send.map(|msg| sink.send(msg)))
        .await
        .map(|res| res.map_err(|_| ()))
        .unwrap_or(Ok(()))
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
