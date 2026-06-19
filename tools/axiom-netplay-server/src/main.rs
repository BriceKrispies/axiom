//! `axiom-netplay-server` — a minimal **authoritative** game server for the live
//! netplay demo.
//!
//! Unlike the dumb lockstep relay (`tools/axiom-netcode-relay`), this server is
//! the source of truth. It holds the authoritative state — two player cubes —
//! accepts each browser's `JoinRoom` (replying `Welcome`), integrates the
//! `ClientIntent`s it receives, and broadcasts a `ServerSnapshot` of the
//! authoritative positions every fixed tick. Clients send *intents*, never
//! state; the server decides the outcome and the clients render its snapshots.
//!
//! It speaks the canonical wire format through the `axiom-net-protocol` module,
//! so the bytes are exactly what the TypeScript `@axiom/client` SDK encodes and
//! decodes. Repo tooling: a Tool by its `tools/` location, outside the engine
//! dependency graph and the coverage gate, so it uses ordinary control flow and
//! `std::net`/sockets that the engine spine may not.

use std::sync::Arc;
use std::time::Duration;

use axiom_net_protocol::NetProtocolApi;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// The default address the server listens on (distinct from the relay's 9001).
const DEFAULT_ADDR: &str = "127.0.0.1:9002";

/// The application protocol version this server speaks.
const PROTOCOL_VERSION: u32 = 1;

/// The authoritative simulation step, in nanoseconds (~60 Hz). Sent in `Welcome`
/// and used as the broadcast tick interval.
const FIXED_STEP_NS: u64 = 16_666_667;

/// The number of player slots (this demo is two-player).
const MAX_PLAYERS: u64 = 2;

/// The authoritative starting positions of player 0 and player 1. The browser
/// renderer seeds the same values, so absolute snapshots line up from tick 0.
const INITIAL_POSITIONS: [[f32; 2]; 2] = [[-1.5, 0.0], [1.5, 0.0]];

/// How far from the origin a cube may travel on each axis — the authoritative
/// bound the server enforces (a client cannot drive its cube off the field).
const POSITION_LIMIT: f32 = 3.5;

type Sink = futures_util::stream::SplitSink<WebSocketStream<TcpStream>, Message>;
type Source = futures_util::stream::SplitStream<WebSocketStream<TcpStream>>;
type SharedSink = Arc<Mutex<Sink>>;
type Shared = Arc<Mutex<Game>>;

/// One connected, welcomed player.
struct Client {
    /// The assigned client id (1 or 2); it controls player index `id - 1`.
    id: u64,
    /// The move delta from this client's most recent intent, applied next tick.
    pending: [f32; 2],
    /// The highest client sequence accepted from this client (echoed in its
    /// snapshots so it can drop acknowledged pending intents).
    last_accepted_seq: u64,
    /// This client's socket, shared so the tick loop can broadcast to it.
    sink: SharedSink,
}

/// The authoritative game state.
struct Game {
    tick: u64,
    pos: [[f32; 2]; 2],
    clients: Vec<Client>,
}

impl Game {
    fn new() -> Self {
        Game {
            tick: 0,
            pos: INITIAL_POSITIONS,
            clients: Vec::new(),
        }
    }

    /// The lowest free player id, or `None` when the game is full.
    fn free_id(&self) -> Option<u64> {
        (1..=MAX_PLAYERS).find(|id| !self.clients.iter().any(|c| c.id == *id))
    }
}

#[tokio::main]
async fn main() {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_ADDR.to_string());
    let listener = TcpListener::bind(&addr)
        .await
        .expect("netplay-server: failed to bind the listen address");
    println!("axiom-netplay-server (authoritative) listening on ws://{addr}");
    println!("open the netplay page in two WebGPU browsers — each joins as a player.");

    let game: Shared = Arc::new(Mutex::new(Game::new()));
    tokio::spawn(tick_loop(game.clone()));

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(serve(stream, game.clone()));
            }
            Err(e) => eprintln!("netplay-server: accept error: {e}"),
        }
    }
}

/// Advance the authoritative simulation and broadcast a snapshot every fixed
/// step: integrate each client's pending delta into its cube (clamped to the
/// field), then send each client a `ServerSnapshot` carrying the packed
/// positions and that client's own last-accepted sequence.
async fn tick_loop(game: Shared) {
    let mut interval = tokio::time::interval(Duration::from_nanos(FIXED_STEP_NS));
    loop {
        interval.tick().await;

        // Mutate state and build the per-client sends under the lock, then send
        // after releasing it (a socket write must not hold the game lock).
        let sends: Vec<(SharedSink, Vec<u8>)> = {
            let mut g = game.lock().await;

            // Integrate each player's pending delta (split the borrow: collect
            // the deltas, apply, then clear them).
            let updates: Vec<(usize, [f32; 2])> = g
                .clients
                .iter()
                .map(|c| ((c.id - 1) as usize, c.pending))
                .collect();
            for (player, delta) in updates {
                g.pos[player][0] = clamp(g.pos[player][0] + delta[0]);
                g.pos[player][1] = clamp(g.pos[player][1] + delta[1]);
            }
            for c in g.clients.iter_mut() {
                c.pending = [0.0, 0.0];
            }
            g.tick += 1;

            let tick = g.tick;
            let payload = pack_positions(&g.pos);
            g.clients
                .iter()
                .map(|c| {
                    let bytes = NetProtocolApi::encode_server_snapshot(
                        tick,
                        c.last_accepted_seq,
                        &payload,
                    )
                    .expect("snapshot payload is within the protocol bound");
                    (c.sink.clone(), bytes)
                })
                .collect()
        };

        for (sink, bytes) in sends {
            let _ = sink.lock().await.send(Message::binary(bytes)).await;
        }
    }
}

/// Serve one connection: await its `JoinRoom`, claim a player slot, reply
/// `Welcome`, then fold its `ClientIntent`s into the authoritative state until it
/// leaves.
async fn serve(stream: TcpStream, game: Shared) {
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(_) => {
            eprintln!("netplay-server: a peer failed the WebSocket handshake");
            return;
        }
    };
    let (sink, mut source) = ws.split();
    let sink: SharedSink = Arc::new(Mutex::new(sink));

    // 1. The first message must be a JoinRoom on a compatible protocol version.
    if !await_join_room(&mut source).await {
        return;
    }

    // 2. Claim a slot and register atomically, building the Welcome under lock.
    let (id, welcome) = {
        let mut g = game.lock().await;
        match g.free_id() {
            Some(id) => {
                g.clients.push(Client {
                    id,
                    pending: [0.0, 0.0],
                    last_accepted_seq: 0,
                    sink: sink.clone(),
                });
                let welcome =
                    NetProtocolApi::encode_welcome(PROTOCOL_VERSION, id, g.tick, FIXED_STEP_NS)
                        .expect("welcome fields are valid");
                (Some(id), welcome)
            }
            None => (None, Vec::new()),
        }
    };
    let id = match id {
        Some(id) => id,
        None => {
            eprintln!("netplay-server: rejected an extra player (game is full)");
            let _ = sink.lock().await.close().await;
            return;
        }
    };

    // 3. Send Welcome. If the socket is already gone, unregister and bail.
    if sink.lock().await.send(Message::binary(welcome)).await.is_err() {
        game.lock().await.clients.retain(|c| c.id != id);
        return;
    }
    println!("netplay-server: player {id} joined");

    // 4. Apply intents until the client leaves or disconnects.
    process_intents(&mut source, &game, id).await;

    // 5. Release the slot.
    {
        let mut g = game.lock().await;
        g.clients.retain(|c| c.id != id);
        println!(
            "netplay-server: player {id} left ({} remaining)",
            g.clients.len()
        );
    }
}

/// Read until the client's `JoinRoom` arrives. Returns `true` on a valid,
/// version-compatible join; `false` if the socket closes/errors first or the
/// first binary frame is not a compatible `JoinRoom`.
async fn await_join_room(source: &mut Source) -> bool {
    while let Some(item) = source.next().await {
        let msg = match item {
            Ok(m) => m,
            Err(_) => return false,
        };
        if msg.is_close() {
            return false;
        }
        if msg.is_binary() {
            return match NetProtocolApi::decode_join_room(&msg.into_data().to_vec()) {
                Ok((version, _room, _token)) if version == PROTOCOL_VERSION => true,
                Ok((version, _, _)) => {
                    eprintln!("netplay-server: client protocol version {version} unsupported");
                    false
                }
                Err(_) => {
                    eprintln!("netplay-server: first frame was not a JoinRoom");
                    false
                }
            };
        }
        // Ignore text/ping/pong; keep waiting for the JoinRoom.
    }
    false
}

/// Fold a client's inbound `ClientIntent`s into the authoritative state until it
/// sends `LeaveRoom`, closes, or errors.
async fn process_intents(source: &mut Source, game: &Shared, id: u64) {
    while let Some(item) = source.next().await {
        let msg = match item {
            Ok(m) => m,
            Err(_) => break,
        };
        if msg.is_close() {
            break;
        }
        if !msg.is_binary() {
            continue;
        }
        let bytes = msg.into_data().to_vec();
        let kind = match NetProtocolApi::message_kind(&bytes) {
            Ok(k) => k,
            Err(_) => continue,
        };
        if kind == NetProtocolApi::KIND_LEAVE_ROOM {
            break;
        }
        if kind == NetProtocolApi::KIND_CLIENT_INTENT {
            if let Ok((seq, _predicted, _last_seen, payload)) =
                NetProtocolApi::decode_client_intent(&bytes)
            {
                let delta = unpack_delta(&payload);
                let mut g = game.lock().await;
                if let Some(c) = g.clients.iter_mut().find(|c| c.id == id) {
                    // Latest intent wins for the upcoming tick.
                    c.pending = delta;
                    c.last_accepted_seq = c.last_accepted_seq.max(seq);
                }
            }
        }
    }
}

/// Keep a coordinate within the authoritative field bound.
fn clamp(v: f32) -> f32 {
    v.clamp(-POSITION_LIMIT, POSITION_LIMIT)
}

/// Pack the two players' positions as four little-endian `f32`s
/// (`p0x, p0y, p1x, p1y`) — the opaque snapshot payload the browser decodes.
fn pack_positions(pos: &[[f32; 2]; 2]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    for p in pos {
        out.extend_from_slice(&p[0].to_le_bytes());
        out.extend_from_slice(&p[1].to_le_bytes());
    }
    out
}

/// Decode an intent payload of two little-endian `f32`s (`dx, dy`); a short or
/// garbled payload is treated as no movement.
fn unpack_delta(payload: &[u8]) -> [f32; 2] {
    if payload.len() < 8 {
        return [0.0, 0.0];
    }
    let x = f32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let y = f32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    [x, y]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::connect_async;

    #[test]
    fn unpack_delta_handles_full_and_short_payloads() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0.5f32.to_le_bytes());
        bytes.extend_from_slice(&(-0.25f32).to_le_bytes());
        assert_eq!(unpack_delta(&bytes), [0.5, -0.25]);
        assert_eq!(unpack_delta(&[1, 2, 3]), [0.0, 0.0]);
    }

    #[test]
    fn clamp_enforces_the_field_bound() {
        assert_eq!(clamp(99.0), POSITION_LIMIT);
        assert_eq!(clamp(-99.0), -POSITION_LIMIT);
        assert_eq!(clamp(1.0), 1.0);
    }

    #[test]
    fn pack_positions_is_four_little_endian_floats() {
        let bytes = pack_positions(&[[1.0, 2.0], [3.0, 4.0]]);
        assert_eq!(bytes.len(), 16);
        assert_eq!(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]), 1.0);
        assert_eq!(f32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]), 4.0);
    }

    /// End-to-end: a client joins, is welcomed, sends an intent, and sees the
    /// authoritative snapshot reflect the move and acknowledge its sequence.
    #[tokio::test]
    async fn join_welcome_intent_then_authoritative_snapshot() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let game: Shared = Arc::new(Mutex::new(Game::new()));
        tokio::spawn(tick_loop(game.clone()));
        tokio::spawn({
            let game = game.clone();
            async move {
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    tokio::spawn(serve(stream, game.clone()));
                }
            }
        });

        let url = format!("ws://{addr}");
        let (mut ws, _) = connect_async(&url).await.unwrap();

        // Join, then read the Welcome.
        let join = NetProtocolApi::encode_join_room(PROTOCOL_VERSION, b"lobby", b"").unwrap();
        ws.send(Message::binary(join)).await.unwrap();
        let welcome = next_binary(&mut ws).await;
        let (version, client_id, _tick, fixed_step_ns) =
            NetProtocolApi::decode_welcome(&welcome).unwrap();
        assert_eq!(version, PROTOCOL_VERSION);
        assert_eq!(client_id, 1); // first player
        assert_eq!(fixed_step_ns, FIXED_STEP_NS);

        // Send an intent moving player 0 right by 0.1, with sequence 1.
        let mut payload = Vec::new();
        payload.extend_from_slice(&0.1f32.to_le_bytes());
        payload.extend_from_slice(&0.0f32.to_le_bytes());
        let intent = NetProtocolApi::encode_client_intent(1, 0, 0, &payload).unwrap();
        ws.send(Message::binary(intent)).await.unwrap();

        // Read snapshots until the move is reflected and our sequence is acked.
        let mut moved = false;
        for _ in 0..120 {
            let snap = next_binary(&mut ws).await;
            let (_tick, last_acked, state) =
                NetProtocolApi::decode_server_snapshot(&snap).unwrap();
            let p0x = f32::from_le_bytes([state[0], state[1], state[2], state[3]]);
            if last_acked == 1 && p0x > -1.5 {
                assert!((p0x - (-1.4)).abs() < 1e-4, "player 0 moved right by 0.1");
                moved = true;
                break;
            }
        }
        assert!(moved, "an authoritative snapshot reflected the client's intent");
    }

    async fn next_binary(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> Vec<u8> {
        loop {
            let msg = ws.next().await.unwrap().unwrap();
            if msg.is_binary() {
                return msg.into_data().to_vec();
            }
        }
    }
}
