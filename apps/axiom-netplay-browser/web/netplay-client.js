// Client-side netcode for the netplay demo: client-side PREDICTION (your own
// cube responds instantly) and entity INTERPOLATION (the other cube moves
// smoothly despite laggy, jittery snapshots). This is game-specific glue — it
// knows the payload is positions/deltas — so it lives in the demo, not the
// generic @axiom/client SDK. The SDK gives us transport + the sequence/ack
// bookkeeping; we add the prediction/interpolation on top.
//
// Pass ?naive in the URL to DISABLE both (render the raw latest snapshot) so you
// can see the difference: naive = stuttery + input lag; default = smooth + instant.

import { AxiomClient } from "./vendor/axiom-client/index.js";

const MOVE_SPEED = 0.06;                 // world units per intent while a key is held
const INITIAL = [-1.5, 0.0, 1.5, 0.0];   // [p0x,p0y,p1x,p1y] spawn (matches server)
const LIMIT = 3.5;                       // field bound (mirrors the server clamp)
const INTERP_MS = 200;                   // render the OTHER cube this far in the past

const clamp = (v) => Math.max(-LIMIT, Math.min(LIMIT, v));

/**
 * Start the netcode. `setPositions(p0x,p0y,p1x,p1y)` is the wasm renderer entry.
 * Returns the AxiomClient so the page can attach a status handler.
 */
export function startNetplay({ setPositions, serverUrl, naive, transport, serverCertificateHash, signalingUrl, roomId }) {
  const client = new AxiomClient();
  const room = roomId || "lobby";   // which authoritative room to join (matchmaker-assigned)

  let myPlayer = -1;                       // 0 or 1 once Welcome arrives
  let predicted = { x: 0, y: 0 };          // our own cube, predicted locally
  let unacked = [];                        // {seq, dx, dy} inputs not yet acked
  let authoritative = INITIAL.slice();     // latest server state [p0x,p0y,p1x,p1y]
  const interp = [];                       // {t,x,y} history of the OTHER cube

  const keys = { left: false, right: false, up: false, down: false };
  const KEY = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" };
  addEventListener("keydown", (e) => { const k = KEY[e.key]; if (k) { keys[k] = true; e.preventDefault(); } });
  addEventListener("keyup", (e) => { const k = KEY[e.key]; if (k) { keys[k] = false; e.preventDefault(); } });

  client.onStatus((s) => {
    if (s === "connected") {
      myPlayer = client.getClientId() - 1;
      predicted = { x: INITIAL[myPlayer * 2], y: INITIAL[myPlayer * 2 + 1] };
      unacked = [];
    }
  });

  client.onSnapshot((snap) => {
    const p = snap.payload;
    if (p.byteLength < 16) return;
    const dv = new DataView(p.buffer, p.byteOffset, p.byteLength);
    authoritative = [
      dv.getFloat32(0, true), dv.getFloat32(4, true),
      dv.getFloat32(8, true), dv.getFloat32(12, true),
    ];
    if (myPlayer < 0) return;
    const other = 1 - myPlayer;

    // Interpolation: record the OTHER cube's authoritative position with a timestamp.
    interp.push({ t: performance.now(), x: authoritative[other * 2], y: authoritative[other * 2 + 1] });
    while (interp.length > 120) interp.shift();

    // Reconciliation: drop acked inputs, then snap our cube to the authoritative
    // position and re-apply every input the server hasn't processed yet.
    unacked = unacked.filter((u) => u.seq > snap.lastAcceptedClientSequence);
    let x = authoritative[myPlayer * 2];
    let y = authoritative[myPlayer * 2 + 1];
    for (const u of unacked) { x = clamp(x + u.dx); y = clamp(y + u.dy); }
    predicted = { x, y };
  });

  client.connect({ url: serverUrl, roomId: room, protocolVersion: 1, transport, serverCertificateHash, signalingUrl });

  // Input + prediction at ~60 Hz: send the held-key delta, and immediately apply
  // it locally so our own cube responds with zero round-trip delay.
  setInterval(() => {
    if (myPlayer < 0) return;
    const dx = ((keys.right ? 1 : 0) - (keys.left ? 1 : 0)) * MOVE_SPEED;
    const dy = ((keys.up ? 1 : 0) - (keys.down ? 1 : 0)) * MOVE_SPEED;
    const buf = new Uint8Array(8);
    const w = new DataView(buf.buffer);
    w.setFloat32(0, dx, true);
    w.setFloat32(4, dy, true);
    const seq = client.sendIntent(buf);
    if (seq !== null) {
      unacked.push({ seq, dx, dy });
      predicted = { x: clamp(predicted.x + dx), y: clamp(predicted.y + dy) };
    }
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
      serverTick: client.getServerTick(),
      acked: client.getLastAckedSequence(),
      status: client.getStatus(),
    };
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);

  return client;
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
