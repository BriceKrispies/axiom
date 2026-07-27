# axiom-chest-server

A stand-in origin for the Casino Games **resilient** page
(`apps/casino-games/web/resilient.html`). It serves the static site *and* the
POST endpoint from **one process on one port**, and decides every outcome with
the app's real chance engine.

It is repo **tooling**: no `Cargo.toml`, no `layer.toml`, invisible to cargo,
outside the engine dependency graph and the coverage gate.

## Why it exists, and why it is Node

The resilient page's baseline is a genuine `<form method="POST">` that must work
with **zero JavaScript and zero CSS** — the shape a locked-down enterprise
(Citrix) browser can be relied on to run. A native form navigation has no CORS
story at all: there is no preflight to grant and no header the page can add. So
the page and the endpoint must be same-origin, which means one server for both.
That is this.

It is Node rather than Rust because it imports the app's **real** chance engine:

```
apps/casino-games/web/src/chest-round/round.ts
  └─ chance-engine/probability/choice-population.ts   planChoicePopulation
     chance-engine/configuration/schema.ts            baseConfig
     chance-engine/configuration/validation.ts        validateConfig
     chance-engine/randomness/streams.ts              the seeded streams
```

That code is pure TypeScript with no renderer dependency — which is exactly why
a server can run it. A Rust server would mean writing the fairness logic a
second time, in a second language, and keeping two copies honest forever. There
is one copy.

No build step: Node runs the TypeScript directly by stripping types, the same
mechanism `node --test "**/*.test.ts"` already uses across this repo. Importing
the app as *source* is the point — a compiled copy could lag behind.

## Running it

Through the repo's process manager (preferred — detached, named, survives the
shell):

```sh
uv run scripts/localhost_servers.py start chest-resilient -- node tools/axiom-chest-server/src/main.ts --port 8090
uv run scripts/localhost_servers.py logs chest-resilient -n 20
uv run scripts/localhost_servers.py stop chest-resilient
```

> **Note the flag order.** `localhost_servers.py start` parses its command with
> `argparse.REMAINDER`, which swallows a `--port` written *before* the `--`. Pass
> the port to `node`, as above; the registry then lists the server with "no port
> declared", which is cosmetic.

Directly, when you want an attached process:

```sh
node tools/axiom-chest-server/src/main.ts --port 8090 [--root <dir>]
```

Then open <http://localhost:8090/> — `/` serves `resilient.html`, because that
is the page this server exists for.

## The endpoints

| Route       | Accepts                                             | Answers |
|-------------|-----------------------------------------------------|---------|
| `GET /*`    | —                                                    | `apps/casino-games/web/`, `/` → `resilient.html` |
| `POST /api/pick` | `x-www-form-urlencoded` **or** `application/json` | content-negotiated: a full HTML result page, or `PickResponse` JSON |
| `POST /api/new`  | either                                          | `303 → /resilient.html` for a browser; `RoundResponse` JSON otherwise |

**The decision path is identical either way.** Both encodings decode to the same
flat fields, run the same `resolvePick`, and differ only in the last step —
`renderResultPage` vs. `JSON.stringify`. There is no "no-JS branch" of the game
logic to rot, which is what makes testing the baseline meaningful.

Negotiation: `text/html` wins only when it is *strictly* preferred by the
`Accept` header's q-values. A tie — a bare `*/*` from curl or a default
`fetch` — resolves to JSON, on the grounds that an unopinionated client is a
machine. A real form navigation always names `text/html` explicitly.

Session state (one seed, a round number, and the pick already made) lives in
memory keyed by an `axiom_chest` cookie: `HttpOnly`, because the page never
reads it, and `SameSite=Lax`, so our own form POST is allowed and a cross-site
one is not. A repeat POST **replays** the recorded pick rather than opening a
second chest — the same commitment rule the engine build enforces, and what
makes a browser reload of a result page harmless.

## Trying it without a browser

```sh
# the baseline: a real urlencoded form POST, HTML back
curl -s -X POST http://localhost:8090/api/pick \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -H 'Accept: text/html' --data 'pick=4' -c /tmp/chest -b /tmp/chest

# the enhanced tier: same endpoint, same session, JSON back
curl -s -X POST http://localhost:8090/api/pick \
  -H 'Content-Type: application/json' -H 'Accept: application/json' \
  --data '{"pick":4}' -b /tmp/chest -c /tmp/chest
```

## Tests

```sh
node --test "tools/axiom-chest-server/src/**/*.test.ts"
```

`server.test.ts` boots the real server on an ephemeral port and speaks HTTP to
it, because the claim being made is about HTTP.
