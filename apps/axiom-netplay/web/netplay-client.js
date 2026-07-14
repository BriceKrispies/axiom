// Client-side netcode for the netplay demo: client-side PREDICTION (your own
// cube responds instantly) and entity INTERPOLATION (the other cube moves
// smoothly despite laggy, jittery snapshots). This is game-specific glue — it
// knows the payload is positions/deltas, and it knows the authority's
// participant-block layout — so it lives in the demo, not the generic
// @axiom/client SDK.
//
// It speaks the engine's PER-PLAYER wire frames against the authored-callback
// authority (tools/axiom-netplay-server): it sends `ClientIntentFor` addressed to
// this browser's own seat, and decodes `ServerSnapshotFor` — a per-player-acked
// snapshot whose opaque payload is the authority's participant block. The SDK's
// high-level `AxiomClient` only speaks the older anonymous frames, so this glue
// drops down to the SDK's exported wire codec + byte-frame transports and drives
// the join → welcome → intent → snapshot lifecycle itself. The codec is still the
// real `@axiom/client` codec (byte-for-byte the Rust `axiom-net-protocol`); only
// the lifecycle wiring and the participant-block decode live here.
//
// Pass ?naive in the URL to DISABLE both (render the raw latest snapshot) so you
// can see the difference: naive = stuttery + input lag; default = smooth + instant.

import {
  WebSocketTransport,
  WebTransportTransport,
  WebRtcTransport,
  encodeJoinRoom,
  encodeClientIntentFor,
  decodeFrame,
  KIND_WELCOME,
  KIND_SERVER_SNAPSHOT_FOR,
} from "./vendor/axiom-client/index.js";

const MOVE_SPEED = 0.06;                 // world units per intent while a key is held
const INITIAL = [-1.5, 0.0, 1.5, 0.0];   // [p0x,p0y,p1x,p1y] spawn (matches the authority's initial_x)
const LIMIT = 3.5;                       // field bound (mirrors the server clamp)
const INTERP_MS = 200;                   // render the OTHER cube this far in the past
const PROTOCOL_VERSION = 1;              // matches the authority's Welcome
const JOIN_RESEND_MS = 250;              // resend JoinRoom this often over an unreliable transport

const STATUS_DISCONNECTED = "disconnected";
const STATUS_CONNECTING = "connecting";
const STATUS_CONNECTED = "connected";

const clamp = (v) => Math.max(-LIMIT, Math.min(LIMIT, v));
const utf8 = (s) => new TextEncoder().encode(s);

// Build the byte-frame transport the page selected. This mirrors the SDK's own
// (un-exported) buildTransport selection over the exported transport classes.
function makeTransport({ transport, serverUrl, serverCertificateHash, signalingUrl }) {
  if (transport === "webrtc") return new WebRtcTransport(signalingUrl || serverUrl);
  if (transport === "webtransport") return new WebTransportTransport(serverUrl, serverCertificateHash);
  return new WebSocketTransport(() => new WebSocket(serverUrl));
}

/**
 * Start the netcode. `setPositions(p0x,p0y,p1x,p1y)` is the wasm renderer entry.
 * Returns a small client handle so the page can attach a status handler and read
 * debug state — the same surface the old AxiomClient exposed to this demo.
 */
export function startNetplay({ setPositions, serverUrl, naive, transport, serverCertificateHash, signalingUrl, roomId }) {
  const room = roomId || "lobby";   // which authoritative room to join (matchmaker-assigned)

  let status = STATUS_DISCONNECTED;
  let clientId = 0;                        // server-assigned seat (1-based); 0 until Welcome
  let myPlayer = -1;                       // 0-based seat index once Welcome arrives
  let serverTick = 0;                      // newest authoritative tick applied
  let lastAckedSeq = 0;                    // newest of OUR sequences the authority acked
  let nextSeq = 1;                         // monotonic client sequence

  let predicted = { x: 0, y: 0 };          // our own cube, predicted locally
  let unacked = [];                        // {seq, dx, dy} inputs not yet acked
  let authoritative = INITIAL.slice();     // latest server state [p0x,p0y,p1x,p1y]
  const interp = [];                       // {t,x,y} history of the OTHER cube
  const statusHandlers = [];

  const keys = { left: false, right: false, up: false, down: false };
  const KEY = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" };
  addEventListener("keydown", (e) => { const k = KEY[e.key]; if (k) { keys[k] = true; e.preventDefault(); } });
  addEventListener("keyup", (e) => { const k = KEY[e.key]; if (k) { keys[k] = false; e.preventDefault(); } });

  const setStatus = (s) => { status = s; for (const h of statusHandlers) h(s); };

  const conn = makeTransport({ transport, serverUrl, serverCertificateHash, signalingUrl });
  const joinFrame = encodeJoinRoom(PROTOCOL_VERSION, utf8(room), new Uint8Array());

  let joinTimer = 0;
  const stopJoinResend = () => { if (joinTimer) { clearInterval(joinTimer); joinTimer = 0; } };

  conn.open({
    onOpen() {
      conn.send(joinFrame);
      // Resend JoinRoom over an unreliable transport until welcomed.
      if (!conn.reliable) joinTimer = setInterval(() => conn.send(joinFrame), JOIN_RESEND_MS);
    },
    onMessage(bytes) { handleInbound(bytes); },
    onClose() { stopJoinResend(); setStatus(STATUS_DISCONNECTED); },
  });
  setStatus(STATUS_CONNECTING);

  function handleInbound(bytes) {
    let msg;
    try { msg = decodeFrame(bytes); } catch { return; }   // ignore malformed/unknown frames
    if (msg.kind === KIND_WELCOME) { onWelcome(msg); return; }
    if (msg.kind === KIND_SERVER_SNAPSHOT_FOR) { onSnapshot(msg); }
    // Events / rejections are not needed by this demo.
  }

  function onWelcome(msg) {
    if (status === STATUS_CONNECTED) return;   // ignore a duplicate Welcome (unreliable path)
    clientId = msg.clientId;                   // our 1-based seat on the wire
    myPlayer = clientId - 1;                   // 0-based index into INITIAL / positions
    serverTick = msg.serverTick;
    predicted = { x: INITIAL[myPlayer * 2], y: INITIAL[myPlayer * 2 + 1] };
    unacked = [];
    stopJoinResend();
    setStatus(STATUS_CONNECTED);
  }

  function onSnapshot(snap) {
    if (snap.serverTick < serverTick) return;  // newer-wins (unreliable transports may reorder)
    serverTick = snap.serverTick;

    // Per-player ack: pick THIS seat's accepted sequence out of the ack list.
    const own = snap.acks.find((a) => a.player === clientId);
    if (own) lastAckedSeq = own.sequence;

    // The snapshot payload is the authority's participant block: integrate each
    // seat's last-applied intent delta into the authoritative positions (the
    // authority itself derives positions by folding these same deltas through the
    // engine's per-player tick path from the shared initial spawn).
    const block = decodeParticipants(snap.payload);
    if (!block) return;
    for (const p of block.participants) {
      const idx = p.player - 1;
      if (idx < 0 || idx > 1) continue;
      const d = readDelta(p.intent);
      authoritative[idx * 2] = clamp(authoritative[idx * 2] + d.dx);
      authoritative[idx * 2 + 1] = clamp(authoritative[idx * 2 + 1] + d.dy);
    }

    if (myPlayer < 0) return;
    const other = 1 - myPlayer;

    // Interpolation: record the OTHER cube's authoritative position with a timestamp.
    interp.push({ t: performance.now(), x: authoritative[other * 2], y: authoritative[other * 2 + 1] });
    while (interp.length > 120) interp.shift();

    // Reconciliation: drop acked inputs, then snap our cube to the authoritative
    // position and re-apply every input the server hasn't processed yet.
    unacked = unacked.filter((u) => u.seq > lastAckedSeq);
    let x = authoritative[myPlayer * 2];
    let y = authoritative[myPlayer * 2 + 1];
    for (const u of unacked) { x = clamp(x + u.dx); y = clamp(y + u.dy); }
    predicted = { x, y };
  }

  // Input + prediction at ~60 Hz: send the held-key delta as a per-player intent
  // addressed to OUR seat, and immediately apply it locally so our own cube
  // responds with zero round-trip delay.
  setInterval(() => {
    if (status !== STATUS_CONNECTED || myPlayer < 0) return;
    const dx = ((keys.right ? 1 : 0) - (keys.left ? 1 : 0)) * MOVE_SPEED;
    const dy = ((keys.up ? 1 : 0) - (keys.down ? 1 : 0)) * MOVE_SPEED;
    const buf = new Uint8Array(8);
    const w = new DataView(buf.buffer);
    w.setFloat32(0, dx, true);
    w.setFloat32(4, dy, true);
    const seq = nextSeq++;
    conn.send(encodeClientIntentFor({
      player: clientId,
      clientSequence: seq,
      predictedClientTick: serverTick,
      lastSeenServerTick: serverTick,
      payload: buf,
    }));
    unacked.push({ seq, dx, dy });
    predicted = { x: clamp(predicted.x + dx), y: clamp(predicted.y + dy) };
  }, 16);

  // Render: every animation frame, compute where to draw both cubes and push to
  // the wasm renderer. Own cube = predicted (instant); other cube = interpolated.
  function frame() {
    const pos = authoritative.slice();
    if (myPlayer >= 0 && !naive) {
      const other = 1 - myPlayer;
      pos[myPlayer * 2] = predicted.x;
      pos[myPlayer * 2 + 1] = predicted.y;
      const o = interpolateAt(interp, performance.now() - INTERP_MS);
      if (o) { pos[other * 2] = o.x; pos[other * 2 + 1] = o.y; }
    }
    setPositions(pos[0], pos[1], pos[2], pos[3]);
    // Debug state (handy for inspecting prediction vs authoritative).
    window.__net = {
      myPlayer, naive: !!naive, predicted, authoritative, room,
      pending: unacked.length,
      serverTick,
      acked: lastAckedSeq,
      status,
    };
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);

  // The page attaches a status handler and reads debug getters off this handle.
  return {
    onStatus(handler) { statusHandlers.push(handler); },
    getStatus: () => status,
    getServerTick: () => serverTick,
    getLastAckedSequence: () => lastAckedSeq,
    getClientId: () => clientId,
  };
}

// Decode the authority's participant block (the ServerSnapshotFor payload), per
// the schema in tools/axiom-netplay-server/src/authority.rs (all little-endian):
//   u32 participant_count;
//   participant_count × { u64 player_id; u8 flags; u32 intent_len; u8*intent_len intent };
//   u32 left_count; left_count × u64 player_id;
//   u32 state_len; u8*state_len state.
function decodeParticipants(payload) {
  try {
    const dv = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
    let at = 0;
    const u32 = () => { const v = dv.getUint32(at, true); at += 4; return v; };
    const u64 = () => { const v = Number(dv.getBigUint64(at, true)); at += 8; return v; };
    const count = u32();
    const participants = [];
    for (let i = 0; i < count; i++) {
      const player = u64();
      const flags = dv.getUint8(at); at += 1;
      const len = u32();
      const intent = payload.subarray(at, at + len); at += len;
      participants.push({ player, flags, intent });
    }
    const leftCount = u32();
    const left = [];
    for (let i = 0; i < leftCount; i++) left.push(u64());
    const stateLen = u32();
    const state = payload.subarray(at, at + stateLen); at += stateLen;
    return { participants, left, state };
  } catch {
    return null;   // a malformed block is ignored, never thrown
  }
}

// Read a {dx, dy} move from an 8-byte (two little-endian f32) intent payload; a
// short or absent payload (a seat that sent no intent this tick) is no movement.
function readDelta(intent) {
  if (intent.byteLength < 8) return { dx: 0, dy: 0 };
  const dv = new DataView(intent.buffer, intent.byteOffset, intent.byteLength);
  return { dx: dv.getFloat32(0, true), dy: dv.getFloat32(4, true) };
}

// Linear interpolation of the buffered samples at time `t`.
function interpolateAt(buf, t) {
  if (buf.length === 0) return null;
  if (t <= buf[0].t) return buf[0];
  if (t >= buf[buf.length - 1].t) return buf[buf.length - 1];
  for (let i = 1; i < buf.length; i++) {
    if (buf[i].t >= t) {
      const a = buf[i - 1];
      const b = buf[i];
      const f = (t - a.t) / (b.t - a.t);
      return { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f };
    }
  }
  return buf[buf.length - 1];
}
