# Axiom retro FPS — agent bridge protocol

A small JSON-over-HTTP protocol that lets an external agent (or any client)
**drive the real retro FPS game and read back what it does** — locally headless, or
against a live browser. The engine is a pure function of `(tick, inputs)`, so the
bridge just feeds the same `Intent` the keyboard would and reports the resulting
frame; the agent's events are indistinguishable from a player's.

The bridge is a native binary in this app crate (`src/bin/agent.rs`), built
behind a feature so the wasm build and the default gates never compile it:

```sh
# A. headless — drive the real game with no browser (structured state)
cargo run -p axiom-retro-fps-browser --features agent --bin agent
# A+images — same, plus an offscreen-rendered PNG per `render` request
cargo run -p axiom-retro-fps-browser --features agent-render --bin agent
# B. bridge — relay to a live browser opened with ?agent=ws://127.0.0.1:7879
cargo run -p axiom-retro-fps-browser --features agent --bin agent -- --bridge
```

## Endpoints (HTTP, default `127.0.0.1:7878`)

- `POST /step` — apply an **Action**, advance, return an **Observation**.
- `POST /reset` — fresh game (headless mode only).
- `GET  /state` — the current Observation without stepping.

## Action (request body, all fields optional; `{}` = idle step)

```json
{ "steps": 1,
  "keys": ["forward","backward","left","right","strafe_left","strafe_right","fire"],
  "yaw": 0.03, "pitch": -0.02,
  "fire": false,
  "render": false }
```

- `keys` — held inputs this step (tank-style: `left`/`right` turn, `forward`/
  `backward` move). `yaw`/`pitch` are mouse-look deltas in radians (+left / +up).
- `steps` — ticks to advance (headless only; the browser runs in real time, so
  in bridge mode an action is *held* until the next one).
- `render` — attach an image to the observation (offscreen PNG in headless
  `agent-render`; canvas snapshot data-URL in bridge mode).

## Observation (response)

```json
{ "tick": 42,
  "hud": { "hp": 100, "score": 200, "ammo": 35, "enemies": 2 },
  "draw_count": 65,
  "state_hash": "46ef536be28ac064",
  "image": "frames/000042.png" }
```

- `state_hash` — FNV-1a fingerprint of the frame's packed instance floats; a
  fixed action script replays to the same hash (deterministic), so it doubles as
  a replay/diff key. `image` is present only when `render` was requested (a file
  path in headless mode, a `data:image/png;base64,…` URL in bridge mode).

## Examples

```sh
curl -s localhost:7878/state
curl -s -XPOST localhost:7878/step -d '{"keys":["forward"],"fire":true}'
curl -s -XPOST localhost:7878/step -d '{"steps":120,"keys":["left"],"fire":true}'
curl -s -XPOST localhost:7878/step -d '{"render":true}'   # -> image in the obs
```

Browser bridge: run `--bridge`, open `…/demo.html?id=retro_fps&agent=ws://127.0.0.1:7879`
in a WebGPU browser, then `POST /step` here — the browser applies the input and
streams its frames back. Images come from `canvas.toDataURL()`; if that is blank
on a given setup, screenshot the live canvas with the Playwright controller
instead.
