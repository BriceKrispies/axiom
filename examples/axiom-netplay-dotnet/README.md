# axiom-netplay-dotnet — a .NET 10 server that embeds the real Axiom engine

An example of "embedding Axiom multiplayer into a real server" using **.NET 10 /
ASP.NET Core**, where the server runs the **actual Axiom engine** as the
authority. One process does three things:

1. **Serves the client** — the built the packaged `dist/` directory
   (the wasm renderer + the vendored `@axiom/client` SDK + `index.html`).
2. **Runs the engine** — it embeds the real Axiom engine in-process via FFI (a
   native `cdylib`, `apps/axiom-netplay-ffi`), building the netplay scene,
   applying player intents, and ticking it headlessly.
3. **Is the authoritative game server** — a WebSocket at `/ws` that broadcasts the
   engine's *actual rendered frame* (per-cube `[mvp(16), colour(4)]` instance
   floats) as the `ServerSnapshot` every tick. The browser just rasterizes those
   floats — so the pixels are computed by the engine running here, in .NET.

Both are on the same origin, so the page connects back to `/ws` automatically.

## Does the engine's WASM run on the server? No.

This is the question the example answers. **WASM is a compile target, not a
runtime requirement.** The `.wasm` file exists only to ship the engine into a
*browser*. Here the engine takes the **native FFI route**:

- `apps/axiom-netplay-ffi` compiles the engine **and the wire codec** to a native
  shared library (`axiom_netplay_ffi.dll` / `.so` / `.dylib`) exposing a small C
  ABI: the engine session (`axiom_netplay_create` / `apply_intent` / `tick` /
  `copy_frame` / `destroy`) and the canonical `axiom-net-protocol` codec
  (`axiom_msg_kind` / `axiom_decode_*` / `axiom_encode_*`).
- `Ffi.cs` P/Invokes into it; `Game.cs` forwards each `ClientIntent` into the
  engine and broadcasts the frame the engine renders. The C# code contains **no
  game logic and no protocol codec** — both the simulation and the wire format
  come from the one Rust source of truth across the FFI boundary. (There is no
  hand-written C# codec; the earlier `NetProtocol.cs` twin was removed.)

The alternative — running the engine compiled to WASM inside a server-side WASM
runtime (e.g. Wasmtime for .NET) — is also valid, but it's a portable-library
choice, not "the browser's instance." This example takes the simpler, faster
native-FFI path.

## Run it

```sh
# 1. Build the client + the engine FFI library (and vendor the SDK):
make netplay-build
#   (this also runs `cargo build -p axiom-netplay-ffi --release`)

# 2. Run this server (serves the client AND runs the engine + game):
make netplay-dotnet
#   or: dotnet run --project examples/axiom-netplay-dotnet

# 3. Open http://localhost:8090 in TWO WebGPU browser windows and arrow-key
#    your cube. Red = player 1, blue = player 2.
```

Config (environment variables):

- `AXIOM_TRANSPORT` — `websocket` (default) or `webtransport`. See **Transports** below.
- `ASPNETCORE_URLS` — listen address (default `http://localhost:8090`).
- `AXIOM_WEB_ROOT` — path to the client `web/` dir (default: auto-discovered).
- `AXIOM_FFI_LIB` — explicit path to the engine `cdylib` (default: auto-discovered
  in `target/release` or `target/debug`).
- `AXIOM_LAG_MS` — simulated server lag: base latency before each snapshot
  delivery, in ms (default `90`). The engine still ticks at 60 Hz; only delivery
  is delayed, so the cubes update in laggy, jittery bursts.
- `AXIOM_JITTER_MS` — max extra random latency added per delivery (default `120`),
  so the effective snapshot interval is `AXIOM_LAG_MS .. AXIOM_LAG_MS+AXIOM_JITTER_MS`.
  Set both to `0` to disable lag.

## Transports

The transport is a **config choice** — the engine modules are transport-agnostic
(plain bytes), so this lives entirely at the edge (this server + the SDK), and
the Branchless Law over the modules is unaffected.

- **WebSocket** (default, over TCP) — reliable, ordered. Served at `/ws` on
  `:8090`. The page uses it by default.
- **WebTransport** (`AXIOM_TRANSPORT=webtransport`) — HTTP/3 over QUIC on `:8091`,
  using a **reliable bidirectional stream** (length-framed). .NET's public
  WebTransport API exposes streams, not datagrams, so this is reliable — not the
  unreliable "UDP-like" mode. The server mints a short-lived self-signed ECDSA
  cert and publishes its sha-256 at `GET /cert-hash`; the page passes that to the
  browser so it trusts the dev cert. Select it with
  `http://localhost:8090/?transport=webtransport`.

  > **Requires HTTP/3 / QUIC OS support: Windows 11 / Server 2022+, or Linux with
  > libmsquic.** On Windows 10 (and other unsupported platforms) the server
  > **logs a warning and degrades to WebSocket-only** instead of failing — the
  > WebTransport code is present and correct, it just can't bind an HTTP/3 socket
  > there.

## Files

- `Program.cs` — host wiring: static files, `/ws` (WebSocket), and (when supported)
  the HTTP/3 WebTransport endpoint + `/cert-hash`.
- `Game.cs` — transport-neutral game: one session loop drives the embedded engine
  + codec for both WebSocket and WebTransport; broadcasts snapshots.
- `Ffi.cs` — P/Invoke bindings (engine session + codec) + resolver for the `cdylib`.
- `WebTransportServer.cs` — the HTTP/3 WebTransport edge: dev cert + length-framed
  bidi stream bridged to the neutral session loop.

The native engine wrapper is `apps/axiom-netplay-ffi` (a Rust `cdylib`). This
.NET project is a standalone example: it is **not** part of the Cargo workspace
and is invisible to the Rust architecture checker.
