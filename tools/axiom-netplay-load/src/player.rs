//! One virtual player: a real WebSocket client that follows the authentic client
//! lifecycle (join → welcome → stream intents → apply snapshots) driven by the
//! engine's own [`ClientCoreApi`] and the canonical [`NetProtocolApi`] codec.

use std::collections::HashMap;

use axiom_client_core::ClientCoreApi;
use axiom_net_protocol::NetProtocolApi;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

/// The application protocol version the real client speaks (matches the worker's
/// `WORKER_PROTOCOL_VERSION` and the browser SDK's `DEFAULT_PROTOCOL_VERSION`).
pub const PROTOCOL_VERSION: u32 = 1;

/// The per-axis magnitude of each synthetic move. Well inside the ruleset's
/// `MAX_INTENT_DELTA` (1.0) so every intent is a *legal* move the server accepts;
/// the sign flips periodically so the cube bounces across the field.
const MOVE_MAGNITUDE: f32 = 0.5;

/// How many intents to send in one direction before reversing.
const FLIP_EVERY: u64 = 30;

/// What one virtual player observed over its session.
#[derive(Debug, Clone)]
pub struct PlayerReport {
    pub connected: bool,
    /// Whether the server admitted this player to a room (sent a `Welcome`). A
    /// connected-but-not-welcomed player is over-subscription: it reached the
    /// node but every room it could join was full.
    pub welcomed: bool,
    pub intents_sent: u64,
    pub snapshots: u64,
    pub rejects: u64,
    pub first_tick_seen: u64,
    pub max_server_tick: u64,
    pub latencies_ms: Vec<f64>,
    pub error: Option<String>,
}

impl PlayerReport {
    fn failed(error: String) -> Self {
        PlayerReport {
            connected: false,
            welcomed: false,
            intents_sent: 0,
            snapshots: 0,
            rejects: 0,
            first_tick_seen: 0,
            max_server_tick: 0,
            latencies_ms: Vec::new(),
            error: Some(error),
        }
    }

    /// How many authoritative ticks elapsed while this player watched.
    pub fn tick_advance(&self) -> u64 {
        self.max_server_tick.saturating_sub(self.first_tick_seen)
    }
}

/// The canonical 8-byte move payload: two little-endian `f32` `(dx, dy)`.
pub fn move_payload(dx: f32, dy: f32) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&dx.to_le_bytes());
    out.extend_from_slice(&dy.to_le_bytes());
    out
}

/// The signed move for intent number `n` (bounces every [`FLIP_EVERY`] intents).
fn step_payload(n: u64) -> Vec<u8> {
    let sign = match (n / FLIP_EVERY) % 2 {
        0 => 1.0,
        _ => -1.0,
    };
    move_payload(MOVE_MAGNITUDE * sign, 0.0)
}

/// Tracks outstanding intents to derive intent→ack round-trip latency.
struct Latency {
    sent_at: HashMap<u64, Instant>,
    last_acked: u64,
    samples: Vec<f64>,
}

impl Latency {
    fn new() -> Self {
        Latency {
            sent_at: HashMap::new(),
            last_acked: 0,
            samples: Vec::new(),
        }
    }

    fn on_send(&mut self, sequence: u64, now: Instant) {
        self.sent_at.insert(sequence, now);
    }

    /// A snapshot acknowledged everything up to `acked`. Record the RTT of the
    /// *oldest* newly-acked intent we still hold a send time for — the worst-case
    /// round trip for this batch — then drop all acknowledged send times. (Using
    /// the oldest, not the newest, avoids biasing the latency tail optimistically
    /// when one snapshot acks a whole batch of intents.)
    fn on_ack(&mut self, acked: u64, now: Instant) {
        let oldest = self
            .sent_at
            .iter()
            .filter(|(&seq, _)| seq > self.last_acked && seq <= acked)
            .min_by_key(|(&seq, _)| seq)
            .map(|(_, &t)| t);
        oldest
            .into_iter()
            .for_each(|t| self.samples.push((now - t).as_secs_f64() * 1000.0));
        self.sent_at.retain(|&seq, _| seq > acked);
        self.last_acked = self.last_acked.max(acked);
    }
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Run one player against `ws_url` in `room_id` for `duration`, sending intents
/// at `intent_hz`. Never panics — a connection failure becomes an error report.
pub async fn run_player(
    ws_url: String,
    room_id: String,
    duration: Duration,
    intent_hz: f64,
) -> PlayerReport {
    let connected = match connect_async(&ws_url).await {
        Ok((ws, _)) => ws,
        Err(e) => return PlayerReport::failed(format!("connect {ws_url} failed: {e}")),
    };
    drive(connected, room_id, duration, intent_hz).await
}

async fn drive(ws: Ws, room_id: String, duration: Duration, intent_hz: f64) -> PlayerReport {
    let (mut write, mut read) = ws.split();

    let join = match NetProtocolApi::encode_join_room(PROTOCOL_VERSION, room_id.as_bytes(), b"") {
        Ok(bytes) => bytes,
        Err(e) => return PlayerReport::failed(format!("encode join: {e:?}")),
    };
    if let Err(e) = write.send(Message::binary(join)).await {
        return PlayerReport::failed(format!("send join: {e}"));
    }

    let mut client = ClientCoreApi::new();
    client.connect();
    let mut latency = Latency::new();
    let mut report = PlayerReport {
        connected: true,
        welcomed: false,
        intents_sent: 0,
        snapshots: 0,
        rejects: 0,
        first_tick_seen: 0,
        max_server_tick: 0,
        latencies_ms: Vec::new(),
        error: None,
    };

    let deadline = Instant::now() + duration;
    let mut ticker = tokio::time::interval(Duration::from_secs_f64(1.0 / intent_hz));
    let mut local_tick: u64 = 0;
    // This player's authoritative seat (the `Welcome` client id). Every
    // `ClientIntentFor` is addressed to it so the authority folds the intent into
    // the matching seat. Zero until welcomed — and no intents flow before then.
    let mut my_player: u64 = 0;

    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            item = read.next() => match item {
                None => break,
                Some(Err(_)) => break,
                Some(Ok(msg)) => handle_inbound(msg, &mut client, &mut latency, &mut report, &mut my_player),
            },
            _ = ticker.tick() => {
                local_tick += 1;
                let last_seen = client.latest_server_tick();
                let payload = step_payload(report.intents_sent);
                match client.next_intent(local_tick, last_seen, &payload) {
                    None => {} // not welcomed yet
                    Some((seq, predicted, seen, body)) => {
                        let frame = NetProtocolApi::encode_client_intent_for(my_player, seq, predicted, seen, &body)
                            .expect("a synthetic move is always within the protocol bound");
                        latency.on_send(seq, Instant::now());
                        match write.send(Message::binary(frame)).await {
                            Ok(()) => report.intents_sent += 1,
                            Err(_) => break,
                        }
                    }
                }
            }
        }
    }

    report.latencies_ms = latency.samples;
    report
}

/// Apply one inbound frame to the client state machine and the report. `my_player`
/// is this player's authoritative seat, learned from `Welcome` and used to pick
/// this seat's own acknowledgement out of a `ServerSnapshotFor`'s per-player acks.
fn handle_inbound(
    msg: Message,
    client: &mut ClientCoreApi,
    latency: &mut Latency,
    report: &mut PlayerReport,
    my_player: &mut u64,
) {
    if !msg.is_binary() {
        return;
    }
    let data = msg.into_data().to_vec();
    let kind = match NetProtocolApi::message_kind(&data) {
        Ok(k) => k,
        Err(_) => return,
    };
    match kind {
        k if k == NetProtocolApi::KIND_WELCOME => {
            if let Ok((_v, id, server_tick, _step)) = NetProtocolApi::decode_welcome(&data) {
                client.accept_welcome(server_tick);
                *my_player = id;
                report.welcomed = true;
            }
        }
        k if k == NetProtocolApi::KIND_SERVER_SNAPSHOT_FOR => {
            if let Ok((server_tick, acks, _payload)) =
                NetProtocolApi::decode_server_snapshot_for(&data)
            {
                // Pick this seat's own ack out of the per-player ack list (absent
                // when the authority carried no ack for us this tick → 0).
                let acked = acks
                    .iter()
                    .find(|&&(player, _)| player == *my_player)
                    .map(|&(_, sequence)| sequence)
                    .unwrap_or(0);
                client.accept_snapshot(server_tick, acked);
                latency.on_ack(acked, Instant::now());
                report.snapshots += 1;
                report.first_tick_seen = match report.first_tick_seen {
                    0 => server_tick,
                    seen => seen,
                };
                report.max_server_tick = report.max_server_tick.max(server_tick);
            }
        }
        k if k == NetProtocolApi::KIND_REJECTED_INTENT => {
            if let Ok((seq, _reason)) = NetProtocolApi::decode_rejected_intent(&data) {
                client.accept_rejected_intent(seq);
                report.rejects += 1;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_payload_is_two_le_floats() {
        let p = move_payload(0.5, -0.25);
        assert_eq!(p.len(), 8);
        assert_eq!(f32::from_le_bytes([p[0], p[1], p[2], p[3]]), 0.5);
        assert_eq!(f32::from_le_bytes([p[4], p[5], p[6], p[7]]), -0.25);
    }

    #[test]
    fn step_payload_bounces_direction() {
        let first = step_payload(0);
        let later = step_payload(FLIP_EVERY);
        let fx = f32::from_le_bytes([first[0], first[1], first[2], first[3]]);
        let lx = f32::from_le_bytes([later[0], later[1], later[2], later[3]]);
        assert!(fx > 0.0 && lx < 0.0, "fx={fx} lx={lx}");
    }

    #[test]
    fn synthetic_moves_are_within_the_legal_delta() {
        // Every synthetic payload must be a *legal* move (|delta| <= 1.0) so the
        // server accepts it — otherwise the load test would measure rejections.
        (0..200u64).for_each(|n| {
            let p = step_payload(n);
            let dx = f32::from_le_bytes([p[0], p[1], p[2], p[3]]).abs();
            assert!(dx <= 1.0, "n={n} dx={dx} exceeds the legal delta");
        });
    }

    #[test]
    fn latency_records_one_sample_per_advancing_ack() {
        let mut l = Latency::new();
        let t0 = Instant::now();
        l.on_send(1, t0);
        l.on_send(2, t0);
        l.on_send(3, t0);
        // Ack up to 2: one sample (oldest newly-acked = seq 1), seq 1 and 2
        // dropped, seq 3 remains.
        l.on_ack(2, t0 + Duration::from_millis(20));
        assert_eq!(l.samples.len(), 1);
        assert!(l.samples[0] >= 20.0);
        assert_eq!(l.sent_at.len(), 1);
        l.on_ack(3, t0 + Duration::from_millis(40));
        assert_eq!(l.samples.len(), 2);
        assert!(l.sent_at.is_empty());
    }

    #[test]
    fn latency_ignores_a_stale_ack_with_no_pending() {
        let mut l = Latency::new();
        let t0 = Instant::now();
        l.on_ack(5, t0);
        assert!(l.samples.is_empty());
    }

    #[test]
    fn failed_report_marks_disconnected() {
        let r = PlayerReport::failed("boom".to_string());
        assert!(!r.connected);
        assert_eq!(r.tick_advance(), 0);
        assert_eq!(r.error.as_deref(), Some("boom"));
    }

    #[test]
    fn handle_inbound_ignores_non_binary_and_garbage() {
        let mut client = ClientCoreApi::new();
        client.connect();
        let mut latency = Latency::new();
        let mut report = PlayerReport::failed("x".to_string());
        let mut my_player = 0u64;
        handle_inbound(
            Message::text("hi"),
            &mut client,
            &mut latency,
            &mut report,
            &mut my_player,
        );
        handle_inbound(
            Message::binary(vec![9, 9, 9]),
            &mut client,
            &mut latency,
            &mut report,
            &mut my_player,
        );
        assert_eq!(report.snapshots, 0);
    }

    #[test]
    fn handle_inbound_applies_welcome_then_snapshot() {
        let mut client = ClientCoreApi::new();
        client.connect();
        let mut latency = Latency::new();
        let mut report = PlayerReport {
            connected: true,
            welcomed: false,
            intents_sent: 0,
            snapshots: 0,
            rejects: 0,
            first_tick_seen: 0,
            max_server_tick: 0,
            latencies_ms: Vec::new(),
            error: None,
        };

        let mut my_player = 0u64;
        let welcome = NetProtocolApi::encode_welcome(PROTOCOL_VERSION, 1, 10, 16_666_667).unwrap();
        handle_inbound(
            Message::binary(welcome),
            &mut client,
            &mut latency,
            &mut report,
            &mut my_player,
        );
        assert!(client.is_connected());
        assert!(report.welcomed);
        assert_eq!(my_player, 1, "the welcome assigns this player its seat");

        // A per-player ServerSnapshotFor acking our seat (1) at sequence 0.
        let snap =
            NetProtocolApi::encode_server_snapshot_for(11, &[(1, 0)], b"\0\0\0\0\0\0\0\0").unwrap();
        handle_inbound(
            Message::binary(snap),
            &mut client,
            &mut latency,
            &mut report,
            &mut my_player,
        );
        assert_eq!(report.snapshots, 1);
        assert_eq!(report.first_tick_seen, 11);
        assert_eq!(report.max_server_tick, 11);

        let reject = NetProtocolApi::encode_rejected_intent(1, NetProtocolApi::REASON_OUT_OF_ORDER);
        handle_inbound(
            Message::binary(reject),
            &mut client,
            &mut latency,
            &mut report,
            &mut my_player,
        );
        assert_eq!(report.rejects, 1);
    }

    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    /// A minimal authoritative mock: await the client's `JoinRoom`, optionally send
    /// `Welcome` (seat 1), then broadcast per-player `ServerSnapshotFor`s every ~16ms
    /// acking the highest `ClientIntentFor` sequence seen for seat 1. `welcome =
    /// false` stays silent after the join (models a server that admits no one).
    async fn mock_serve(ws: WebSocketStream<TcpStream>, welcome: bool, run: std::time::Duration) {
        let (mut write, mut read) = ws.split();
        // Wait for the first binary frame (the JoinRoom).
        loop {
            match read.next().await {
                Some(Ok(m)) if m.is_binary() => break,
                Some(Ok(_)) => continue,
                _ => return,
            }
        }
        if !welcome {
            let _ = tokio::time::timeout(run, async { while read.next().await.is_some() {} }).await;
            return;
        }
        let hello = NetProtocolApi::encode_welcome(PROTOCOL_VERSION, 1, 0, 16_666_667).unwrap();
        write.send(Message::binary(hello)).await.unwrap();

        // Track the highest intent sequence the client has sent, to ack it.
        let last_seq = Arc::new(AtomicU64::new(0));
        let reader_seq = last_seq.clone();
        let reader = tokio::spawn(async move {
            while let Some(Ok(m)) = read.next().await {
                if m.is_binary() {
                    if let Ok((_player, seq, _, _, _)) =
                        NetProtocolApi::decode_client_intent_for(&m.into_data().to_vec())
                    {
                        reader_seq.fetch_max(seq, Ordering::Relaxed);
                    }
                }
            }
        });

        let deadline = Instant::now() + run;
        let mut tick = 0u64;
        let mut ticker = tokio::time::interval(Duration::from_millis(16));
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                _ = ticker.tick() => {
                    tick += 1;
                    let acked = last_seq.load(Ordering::Relaxed);
                    let snap = NetProtocolApi::encode_server_snapshot_for(tick, &[(1, acked)], &[0u8; 16]).unwrap();
                    if write.send(Message::binary(snap)).await.is_err() {
                        break;
                    }
                }
            }
        }
        reader.abort();
    }

    async fn spawn_mock(welcome: bool, run_ms: u64) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            mock_serve(ws, welcome, Duration::from_millis(run_ms)).await;
        });
        format!("ws://{addr}")
    }

    #[tokio::test]
    async fn run_player_against_a_welcoming_server_is_healthy() {
        let url = spawn_mock(true, 800).await;
        let report = run_player(url, "room".to_string(), Duration::from_millis(400), 120.0).await;
        assert!(report.connected, "{:?}", report.error);
        assert!(report.welcomed, "the server sent Welcome");
        assert!(report.intents_sent > 0, "intents flow after Welcome");
        assert!(report.snapshots > 0, "snapshots were received");
        assert!(
            !report.latencies_ms.is_empty(),
            "acks produced latency samples"
        );
        assert!(report.tick_advance() > 0, "authoritative ticks advanced");
    }

    #[tokio::test]
    async fn run_player_against_a_silent_server_fails_the_verdict() {
        // A server that accepts the socket but never welcomes: the client connects
        // but ClientCoreApi blocks every intent, so the run must FAIL the verdict
        // (welcomed != attempted, and no intent acked) rather than pass vacuously.
        let url = spawn_mock(false, 400).await;
        let report = run_player(url, "room".to_string(), Duration::from_millis(250), 120.0).await;
        assert!(report.connected, "the socket opened");
        assert!(!report.welcomed, "no Welcome was sent");
        assert_eq!(
            report.intents_sent, 0,
            "a non-welcomed client sends nothing"
        );
        assert!(report.latencies_ms.is_empty());

        let agg = crate::stats::aggregate(&[report]);
        let cfg = crate::args::Config::parse(&["soak".to_string()]).unwrap();
        assert!(
            !crate::stats::verdict(&agg, &cfg).0,
            "a never-welcoming server must FAIL the verdict"
        );
    }

    #[tokio::test]
    async fn run_player_against_a_dead_address_reports_disconnected() {
        // Nothing listening: the connection fails and the player reports an error,
        // which makes the run FAIL (connected != attempted) — never a silent pass.
        let report = run_player(
            "ws://127.0.0.1:1".to_string(),
            "room".to_string(),
            Duration::from_millis(200),
            60.0,
        )
        .await;
        assert!(!report.connected);
        assert!(report.error.is_some());
    }
}
