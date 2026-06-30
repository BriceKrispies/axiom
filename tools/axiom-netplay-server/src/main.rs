//! `axiom-netplay-server` — the **authored-callback authority** for the live
//! server-authoritative netplay demo (SPEC-13 §16, §3.5).
//!
//! Unlike the dumb lockstep relay (`tools/axiom-netcode-relay`), this server is
//! the single source of truth. It no longer hard-codes movement: it runs the
//! **authored game** headlessly through the engine's deterministic fixed-step
//! [`RunningApp::tick_with`](axiom::prelude::RunningApp) path (owned by
//! [`authority::Authority`]). Each browser's `JoinRoom` claims a seat (replying
//! `Welcome`); its per-player `ClientIntentFor` frames are folded into the authored
//! world; every fixed tick the authority steps the game and broadcasts a per-player
//! `ServerSnapshotFor` carrying the participant block and the per-player ack
//! cursors. Clients send *intents*, never state; the authority decides the
//! outcome; predicted clients reconcile.
//!
//! ## Why a single authority task + channels (not a shared `Mutex<Authority>`)
//! The authored world `RunningApp` is intentionally **not `Send`** (it holds engine
//! `dyn` system objects that never leave their owning thread). So the authority is
//! owned by exactly one task ([`authority_loop`], driven inline by [`run_server`],
//! never `tokio::spawn`ed) and the per-peer socket tasks — which *are* spawned and
//! must be `Send` — reach it only through `Send` channels: an `mpsc` of
//! [`Command`]s in, and a `broadcast` of snapshot frames out. This is also the
//! cleaner shape: a single owner of the deterministic world, exactly one writer.
//!
//! Repo tooling: a Tool by its `tools/` location — outside the engine dependency
//! graph and the coverage gate — but, like `axiom-netcode-relay`, written
//! **branchless** (the Branchless Law's `engine_no_branching` gate fires in tools
//! too): every dispatch decision is a data transform over `Option`/stream
//! combinators, not `if`/`match`/`for`/`while`.

mod admission;
mod authority;

use std::sync::Arc;
use std::time::Duration;

use admission::JwtPolicy;
use authority::{Authority, FIXED_STEP_NS, MAX_PLAYERS, PROTOCOL_VERSION};
use axiom_net_protocol::NetProtocolApi;
use futures_util::future::OptionFuture;
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_stream::wrappers::{
    BroadcastStream, IntervalStream, TcpListenerStream, UnboundedReceiverStream,
};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// The default address the server listens on (distinct from the relay's 9001).
const DEFAULT_ADDR: &str = "127.0.0.1:9002";

/// This peer's outbound socket half.
type Sink = futures_util::stream::SplitSink<WebSocketStream<TcpStream>, Message>;
/// This peer's inbound socket half.
type Source = futures_util::stream::SplitStream<WebSocketStream<TcpStream>>;
/// The channel a peer task uses to drive the single authority task.
type CmdTx = mpsc::UnboundedSender<Command>;
/// The bus a snapshot frame is broadcast to every connected peer on.
type SnapTx = broadcast::Sender<Arc<Vec<u8>>>;

#[tokio::main]
async fn main() {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_ADDR.to_string());
    let listener = TcpListener::bind(&addr)
        .await
        .expect("netplay-server: failed to bind the listen address");
    println!("axiom-netplay-server (authored-callback authority) listening on ws://{addr}");
    println!("up to {MAX_PLAYERS} browsers join as players; the authority runs the authored game and broadcasts ServerSnapshotFor.");
    run_server(listener).await;
}

/// Stand up the authority task and the accept loop and run them concurrently. The
/// authority task owns the (non-`Send`) authored world and is driven *inline* here
/// (never spawned); the accept loop spawns one `Send` peer task per socket. They
/// communicate only through the `mpsc`/`broadcast` channels created here.
async fn run_server(listener: TcpListener) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Command>();
    let (snap_tx, _snap_rx) = broadcast::channel::<Arc<Vec<u8>>>(1024);
    tokio::join!(
        authority_loop(Authority::new(), JwtPolicy::from_env(), cmd_rx, snap_tx.clone()),
        accept_loop(listener, cmd_tx, snap_tx),
    );
}

/// The single owner of the authored world. It folds a merged stream of fixed-step
/// ticks (`IntervalStream`) and peer commands (`UnboundedReceiverStream`) into the
/// [`Authority`]: a command mutates the world's seating/intents, a tick runs one
/// deterministic authored fixed update and broadcasts the resulting
/// `ServerSnapshotFor`. The wall clock decides only *when* a step runs, never
/// *what* it computes.
async fn authority_loop(
    authority: Authority,
    policy: JwtPolicy,
    cmd_rx: mpsc::UnboundedReceiver<Command>,
    snap_tx: SnapTx,
) {
    let ticks = IntervalStream::new(tokio::time::interval(Duration::from_nanos(FIXED_STEP_NS)))
        .map(|_instant| AuthEvent::tick());
    let commands = UnboundedReceiverStream::new(cmd_rx).map(AuthEvent::command);
    tokio_stream::StreamExt::merge(ticks, commands)
        .fold(authority, |authority, event| fold_event(authority, event, &policy, &snap_tx))
        .await;
}

/// Apply one merged event to the authority and (on a tick) step + broadcast.
/// Threaded by value through the fold so the world has a single owner.
async fn fold_event(
    mut authority: Authority,
    event: AuthEvent,
    policy: &JwtPolicy,
    snap_tx: &SnapTx,
) -> Authority {
    apply_command(&mut authority, policy, event.command);
    event.step.then(|| {
        let frame = authority.step();
        let _ = snap_tx.send(Arc::new(frame));
    });
    authority
}

/// Apply one optional peer command to the authority. A `Command` carries exactly
/// one of join/intent/leave; applying all three `Option`s is branchless and the
/// absent ones are no-ops. A join is gated by the [`JwtPolicy`]: a token the policy
/// rejects claims no seat, so the reply is `None` and the peer is never welcomed.
fn apply_command(authority: &mut Authority, policy: &JwtPolicy, command: Option<Command>) {
    command
        .map(|cmd| {
            cmd.join.map(|(token, reply)| {
                let seated = policy
                    .admits(&token)
                    .then(|| authority.claim())
                    .flatten()
                    .map(|seat| (seat, authority.tick()));
                let _ = reply.send(seated);
            });
            cmd.intent
                .map(|(player, sequence, payload)| authority.apply_intent(player, sequence, payload));
            cmd.leave.map(|seat| authority.leave(seat));
        })
        .unwrap_or(());
}

/// Accept connections forever, serving each on its own spawned (`Send`) task that
/// reaches the authority through the channels. A failed `accept()` is an `Err` item
/// that `conn.ok()` drops — no `loop` keyword.
async fn accept_loop(listener: TcpListener, cmd_tx: CmdTx, snap_tx: SnapTx) {
    TcpListenerStream::new(listener)
        .for_each_concurrent(None, |conn| {
            let cmd_tx = cmd_tx.clone();
            let snap_tx = snap_tx.clone();
            async move {
                conn.ok()
                    .map(|stream| tokio::spawn(serve(stream, cmd_tx, snap_tx)))
                    .map(|_| ())
                    .unwrap_or(());
            }
        })
        .await;
}

/// Serve one connection: complete the WebSocket handshake, then bridge it. A failed
/// handshake is logged and dropped (`.ok()` → the `OptionFuture` skips the bridge).
async fn serve(stream: TcpStream, cmd_tx: CmdTx, snap_tx: SnapTx) {
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .inspect_err(|_| eprintln!("netplay-server: a peer failed the WebSocket handshake"))
        .ok();
    OptionFuture::from(ws.map(|ws| serve_ws(ws, cmd_tx, snap_tx))).await;
}

/// Bridge one peer's socket to the authority until either side ends.
///
/// Two streams are merged into one and consumed by `try_fold`, the
/// `axiom-netcode-relay` shape: the **inbound** socket (mapped to [`Action`]s —
/// claim a seat, apply an intent, or stop) and the **bus** (mapped to [`Action`]s
/// carrying a snapshot to forward). Each is `.chain`ed with a terminal
/// `Action::stop()`; `try_fold` threads the [`Peer`] by value and short-circuits on
/// the first `Err(Peer)` (a stop or a socket-write failure), reproducing the old
/// `break` with no `loop`/`match`.
async fn serve_ws(ws: WebSocketStream<TcpStream>, cmd_tx: CmdTx, snap_tx: SnapTx) {
    let (sink, source): (Sink, Source) = ws.split();
    let rx = snap_tx.subscribe();

    let inbound = source
        .map(inbound_action)
        .chain(futures_util::stream::once(async { Action::stop() }));
    let outbound = BroadcastStream::new(rx)
        .map(bus_action)
        .chain(futures_util::stream::once(async { Action::stop() }));

    let folded = tokio_stream::StreamExt::merge(inbound, outbound)
        .map(Ok::<Action, Peer>)
        .try_fold(Peer::new(sink), |peer, action| peer.apply(action, &cmd_tx))
        .await;
    folded.unwrap_or_else(|stopped| stopped).cleanup(&cmd_tx).await;
}

/// A command from a peer task to the single authority task. Modelled as a struct of
/// optional fields (no enum `match`): exactly one is `Some` per command.
struct Command {
    /// Claim a seat: the join `token` (opaque JWT bytes, verified by the policy) and
    /// the reply, which carries `Some((seat, current_tick))` or `None` (rejected/full).
    join: Option<(Vec<u8>, oneshot::Sender<Option<(u64, u64)>>)>,
    /// Fold a player's intent into the authority: `(player, sequence, payload)`.
    intent: Option<(u64, u64, Vec<u8>)>,
    /// Release a seat (the peer disconnected).
    leave: Option<u64>,
}

impl Command {
    /// A seat-claim request carrying the join token and the reply channel.
    fn join(token: Vec<u8>, reply: oneshot::Sender<Option<(u64, u64)>>) -> Self {
        Command {
            join: Some((token, reply)),
            intent: None,
            leave: None,
        }
    }

    /// An intent to fold into the authority.
    fn intent(player: u64, sequence: u64, payload: Vec<u8>) -> Self {
        Command {
            join: None,
            intent: Some((player, sequence, payload)),
            leave: None,
        }
    }

    /// A seat release.
    fn leave(seat: u64) -> Self {
        Command {
            join: None,
            intent: None,
            leave: Some(seat),
        }
    }
}

/// One merged event the authority task folds: a fixed-step tick, or a peer command.
struct AuthEvent {
    /// `true` for an interval tick (step the authored update + broadcast).
    step: bool,
    /// `Some` for a peer command to apply before stepping.
    command: Option<Command>,
}

impl AuthEvent {
    /// A fixed-step tick event.
    fn tick() -> Self {
        AuthEvent {
            step: true,
            command: None,
        }
    }

    /// A peer-command event.
    fn command(command: Command) -> Self {
        AuthEvent {
            step: false,
            command: Some(command),
        }
    }
}

/// A decoded directive for the per-peer bridge, built from one socket frame or one
/// bus item. A struct of optional fields (no enum `match`): the empty action is a
/// no-op, and a stop ends the bridge.
struct Action {
    /// A compatible `JoinRoom` arrived, carrying its opaque join token: claim a seat
    /// (subject to the admission policy) and reply `Welcome`. `None` = not a join.
    join: Option<Vec<u8>>,
    /// Raw `ClientIntentFor` bytes to fold into the authority (this peer's seat).
    intent: Option<Vec<u8>>,
    /// A broadcast `ServerSnapshotFor` to forward to this peer's socket.
    snapshot: Option<Arc<Vec<u8>>>,
    /// A source stream ended (socket closed/errored, or a `LeaveRoom`): stop.
    stop: bool,
}

impl Action {
    /// A do-nothing action (an ignored frame, or a lagged bus tick).
    fn noop() -> Self {
        Action {
            join: None,
            intent: None,
            snapshot: None,
            stop: false,
        }
    }

    /// Stop the bridge: a source stream ended.
    fn stop() -> Self {
        Action {
            stop: true,
            ..Action::noop()
        }
    }
}

/// Map one inbound socket item to an [`Action`]: stop on a close/error, classify a
/// binary frame, ignore text/ping/pong. `Option`/boolean combinators only.
fn inbound_action(item: Result<Message, tokio_tungstenite::tungstenite::Error>) -> Action {
    item.ok()
        .map(|msg| {
            msg.is_close()
                .then(Action::stop)
                .unwrap_or_else(|| classify_binary(msg))
        })
        .unwrap_or_else(Action::stop)
}

/// Classify a non-close inbound message: a binary frame is decoded; text/ping/pong
/// is a no-op.
fn classify_binary(msg: Message) -> Action {
    msg.is_binary()
        .then(|| msg.into_data().to_vec())
        .map(classify_bytes)
        .unwrap_or_else(Action::noop)
}

/// Classify a binary frame by its message kind: a compatible `JoinRoom` claims a
/// seat, a `ClientIntentFor` is applied, a `LeaveRoom` stops; anything else (and any
/// decode failure) is a no-op (rejection is a value, never a panic).
fn classify_bytes(bytes: Vec<u8>) -> Action {
    NetProtocolApi::message_kind(&bytes)
        .ok()
        .map(|kind| Action {
            join: compatible_join_token(kind, &bytes),
            intent: (kind == NetProtocolApi::KIND_CLIENT_INTENT_FOR).then(|| bytes.clone()),
            snapshot: None,
            stop: kind == NetProtocolApi::KIND_LEAVE_ROOM,
        })
        .unwrap_or_else(Action::noop)
}

/// The opaque join token of a compatible `JoinRoom`, or `None` if `bytes` is not a
/// `JoinRoom` on a compatible protocol version. The token is verified later by the
/// admission policy (the authority, not the socket, decides membership).
fn compatible_join_token(kind: u8, bytes: &[u8]) -> Option<Vec<u8>> {
    (kind == NetProtocolApi::KIND_JOIN_ROOM)
        .then(|| NetProtocolApi::decode_join_room(bytes).ok())
        .flatten()
        .filter(|(version, _room, _token)| *version == PROTOCOL_VERSION)
        .map(|(_version, _room, token)| token)
}

/// Map one bus item to an [`Action`]: a published snapshot to forward, or a no-op
/// for a lagged receiver (we keep going). The bus *closing* ends the stream, not an
/// item here.
fn bus_action(
    item: Result<Arc<Vec<u8>>, tokio_stream::wrappers::errors::BroadcastStreamRecvError>,
) -> Action {
    item.ok()
        .map(|bytes| Action {
            snapshot: Some(bytes),
            ..Action::noop()
        })
        .unwrap_or_else(Action::noop)
}

/// The result of a join attempt: the seat claimed (if any) and whether the bridge
/// should keep going (a welcome that could not be delivered is a stop).
#[derive(Clone, Copy)]
struct JoinResult {
    seat: Option<u64>,
    ok: bool,
}

/// The single-owner per-peer bridge state threaded through `try_fold`: this peer's
/// socket and the seat it occupies (`None` until its `JoinRoom` is admitted).
struct Peer {
    sink: Sink,
    seat: Option<u64>,
}

impl Peer {
    /// A fresh, unseated peer over `sink`.
    fn new(sink: Sink) -> Self {
        Peer { sink, seat: None }
    }

    /// Carry out one [`Action`], returning `Ok(self)` to keep bridging or
    /// `Err(self)` to stop. Each effect is gated by `Option`/boolean combinators —
    /// no `if`/`match` — and `self` is moved exactly once into the chosen arm.
    async fn apply(mut self, action: Action, cmd_tx: &CmdTx) -> Result<Self, Self> {
        // JOIN: claim a seat + reply Welcome, iff a compatible join arrived (carrying
        // its token) and the peer is not already seated.
        let joined = OptionFuture::from(
            action
                .join
                .filter(|_| self.seat.is_none())
                .map(|token| claim_and_welcome(cmd_tx, &mut self.sink, token)),
        )
        .await;
        self.seat = self.seat.or(joined.and_then(|j| j.seat));
        let join_failed = joined.map(|j| !j.ok).unwrap_or(false);

        // INTENT: fold this peer's own intent into the authority, iff seated.
        OptionFuture::from(
            self.seat
                .and_then(|seat| action.intent.map(|bytes| (seat, bytes)))
                .map(|(seat, bytes)| apply_intent(cmd_tx, seat, bytes)),
        )
        .await;

        // SNAPSHOT: forward a broadcast snapshot to this peer's socket, iff seated.
        let send_failed = forward_snapshot(self.seat, action.snapshot, &mut self.sink).await;

        let keep = !(action.stop | join_failed | send_failed);
        const ARMS: [fn(Peer) -> Result<Peer, Peer>; 2] = [Err, Ok];
        ARMS[keep as usize](self)
    }

    /// Release this peer's seat when the bridge ends (the vacated seat is reported
    /// in the next snapshot's `leftThisTick`). A never-seated peer is a no-op.
    async fn cleanup(self, cmd_tx: &CmdTx) {
        OptionFuture::from(self.seat.map(|seat| release_and_log(cmd_tx, seat))).await;
    }
}

/// Ask the authority to claim the lowest free seat, then reply `Welcome`. On a
/// welcome that cannot be delivered, the freshly-claimed seat is released so it does
/// not leak. Returns the seat genuinely occupied plus whether to keep going (a full
/// room is *not* a stop — the peer stays connected, just unseated).
async fn claim_and_welcome(cmd_tx: &CmdTx, sink: &mut Sink, token: Vec<u8>) -> JoinResult {
    let (reply_tx, reply_rx) = oneshot::channel();
    let _ = cmd_tx.send(Command::join(token, reply_tx));
    let claimed = reply_rx.await.ok().flatten();
    let send_ok = OptionFuture::from(claimed.map(|(id, tick)| send_welcome(sink, id, tick)))
        .await
        .unwrap_or(true);
    OptionFuture::from(
        claimed
            .filter(|_| !send_ok)
            .map(|(id, _tick)| release(cmd_tx, id)),
    )
    .await;
    let seat = claimed.map(|(id, _tick)| id);
    log_join(seat, send_ok);
    JoinResult {
        seat: seat.filter(|_| send_ok),
        ok: send_ok,
    }
}

/// Encode and send the `Welcome` (protocol version, seat id, current tick, fixed
/// step), returning whether the socket write succeeded.
async fn send_welcome(sink: &mut Sink, id: u64, tick: u64) -> bool {
    let bytes = NetProtocolApi::encode_welcome(PROTOCOL_VERSION, id, tick, FIXED_STEP_NS)
        .expect("welcome fields are valid");
    sink.send(Message::binary(bytes)).await.is_ok()
}

/// Ask the authority to release a seat (used when a welcome could not be sent).
async fn release(cmd_tx: &CmdTx, id: u64) {
    let _ = cmd_tx.send(Command::leave(id));
}

/// Release a seat and log the departure (the disconnect path).
async fn release_and_log(cmd_tx: &CmdTx, seat: u64) {
    let _ = cmd_tx.send(Command::leave(seat));
    println!("netplay-server: player {seat} left");
}

/// Log the outcome of a join attempt, branchlessly over the claimed-seat / send-ok
/// combination.
fn log_join(claimed: Option<u64>, send_ok: bool) {
    claimed
        .filter(|_| send_ok)
        .map(|id| println!("netplay-server: player {id} joined"))
        .unwrap_or_else(|| {
            claimed
                .map(|_| println!("netplay-server: a joining player dropped before welcome"))
                .unwrap_or_else(|| println!("netplay-server: rejected an extra player (room full)"));
        });
}

/// Decode a `ClientIntentFor` and forward it to the authority for `seat`, iff it is
/// well-formed and addressed to this peer's own seat. An intent for another seat or
/// a malformed frame is dropped (a value, never a panic).
async fn apply_intent(cmd_tx: &CmdTx, seat: u64, bytes: Vec<u8>) {
    NetProtocolApi::decode_client_intent_for(&bytes)
        .ok()
        .filter(|(player, ..)| *player == seat)
        .map(|(player, sequence, _predicted, _last_seen, payload)| {
            let _ = cmd_tx.send(Command::intent(player, sequence, payload));
        })
        .unwrap_or(());
}

/// Forward a broadcast snapshot to this peer's socket, iff the peer is seated and
/// there is a snapshot this item. Returns whether the socket write failed (which
/// ends the bridge). `seat.and(snapshot)` gates on "seated AND a snapshot present".
async fn forward_snapshot(
    seat: Option<u64>,
    snapshot: Option<Arc<Vec<u8>>>,
    sink: &mut Sink,
) -> bool {
    let to_send = seat.and(snapshot);
    OptionFuture::from(to_send.map(|bytes| sink.send(Message::binary((*bytes).clone()))))
        .await
        .map(|res| res.is_err())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::connect_async;

    type ClientWs = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    async fn next_binary(ws: &mut ClientWs) -> Vec<u8> {
        loop {
            let msg = ws.next().await.unwrap().unwrap();
            if msg.is_binary() {
                return msg.into_data().to_vec();
            }
        }
    }

    /// A `dx, dy` intent payload (two little-endian f32s).
    fn move_payload(dx: f32, dy: f32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&dx.to_le_bytes());
        bytes.extend_from_slice(&dy.to_le_bytes());
        bytes
    }

    /// The client half of the slice test: join, read the Welcome, send a per-player
    /// intent, and confirm an authoritative `ServerSnapshotFor` acks it.
    async fn client_session(addr: std::net::SocketAddr) {
        let url = format!("ws://{addr}");
        let (mut ws, _) = connect_async(&url).await.unwrap();

        // Join, then read the Welcome (which assigns our seat id).
        let join = NetProtocolApi::encode_join_room(PROTOCOL_VERSION, b"lobby", b"").unwrap();
        ws.send(Message::binary(join)).await.unwrap();
        let welcome = next_binary(&mut ws).await;
        let (version, client_id, _tick, fixed_step_ns) =
            NetProtocolApi::decode_welcome(&welcome).unwrap();
        assert_eq!(version, PROTOCOL_VERSION);
        assert_eq!(client_id, 1); // first seat
        assert_eq!(fixed_step_ns, FIXED_STEP_NS);

        // Send a per-player intent for our seat, sequence 1.
        let intent =
            NetProtocolApi::encode_client_intent_for(client_id, 1, 0, 0, &move_payload(0.1, 0.0))
                .unwrap();
        ws.send(Message::binary(intent)).await.unwrap();

        // Read ServerSnapshotFor frames until our sequence is acked for our seat.
        let mut acked = false;
        for _ in 0..600 {
            let snap = next_binary(&mut ws).await;
            if let Ok((_tick, acks, payload)) = NetProtocolApi::decode_server_snapshot_for(&snap) {
                if acks.iter().any(|&(p, s)| p == client_id && s == 1) {
                    // The payload is the participant block + authoritative state.
                    assert!(!payload.is_empty());
                    acked = true;
                    break;
                }
            }
        }
        assert!(
            acked,
            "an authoritative ServerSnapshotFor acknowledged the client's intent"
        );
    }

    /// End-to-end slice (SPEC-13 §7 slice test): a client joins over a real socket,
    /// is welcomed with a seat, sends a per-player `ClientIntentFor`, and sees the
    /// authoritative `ServerSnapshotFor` acknowledge its sequence — proving the
    /// authored-callback authority runs over the wire, not hard-coded movement.
    #[tokio::test]
    async fn join_welcome_intent_then_authoritative_snapshot_for() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // The server runs forever; race it against the client session and let the
        // client's completion end the test.
        tokio::select! {
            _ = run_server(listener) => unreachable!("the server runs forever"),
            () = client_session(addr) => {}
        }
    }

    #[test]
    fn inbound_classifies_join_intent_leave_and_junk() {
        // A JoinRoom frame → a join action carrying the (here empty) token.
        let join = NetProtocolApi::encode_join_room(PROTOCOL_VERSION, b"r", b"tok").unwrap();
        assert_eq!(inbound_action(Ok(Message::binary(join))).join, Some(b"tok".to_vec()));

        // A ClientIntentFor frame → an intent action carrying the bytes.
        let intent = NetProtocolApi::encode_client_intent_for(1, 1, 0, 0, b"x").unwrap();
        assert!(inbound_action(Ok(Message::binary(intent))).intent.is_some());

        // A LeaveRoom frame → a stop.
        let leave = NetProtocolApi::encode_leave_room(b"r").unwrap();
        assert!(inbound_action(Ok(Message::binary(leave))).stop);

        // A close frame → a stop; a text frame → a no-op; garbage → a no-op.
        assert!(inbound_action(Ok(Message::Close(None))).stop);
        let text = inbound_action(Ok(Message::text("hi")));
        assert!(text.join.is_none() && text.intent.is_none() && !text.stop);
        let junk = inbound_action(Ok(Message::binary(vec![0xFF, 0xFF])));
        assert!(junk.join.is_none() && junk.intent.is_none() && !junk.stop);
    }

    #[test]
    fn an_incompatible_join_version_is_not_admitted() {
        let join = NetProtocolApi::encode_join_room(PROTOCOL_VERSION + 1, b"r", b"").unwrap();
        assert!(inbound_action(Ok(Message::binary(join))).join.is_none());
    }

    #[test]
    fn a_bus_snapshot_becomes_a_forward_action() {
        let bytes = Arc::new(vec![1u8, 2, 3]);
        let action = bus_action(Ok(bytes.clone()));
        assert_eq!(action.snapshot, Some(bytes));
        assert!(!action.stop);
    }
}
