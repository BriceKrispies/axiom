# Arena Forge — Multiplayer Protocol

Arena Forge's client/authority boundary is built to be transport-neutral from
day one, even though today only an in-process local host exists. This
document covers the wire-shaped envelopes, the `MatchApi` contract, sequence
handling, the reconnect seam, and why the boundary is designed to swap
transports without touching client code.

## The command envelope

`sim/commands.ts` defines the `Command` union (the actual game verbs: `buy`,
`sell`, `reroll`, `set_freeze`, `upgrade_forge_rank`, `play_card`,
`return_to_hand`, `reorder`) — but a bare `Command` carries no player identity
or ordering information. Crossing the authority boundary, a command is always
wrapped in a `CommandEnvelope` (`api/envelopes.ts`):

```ts
interface CommandEnvelope {
  readonly clientSeq: number;   // client-assigned, for client-side ordering/dedup
  readonly playerId: PlayerId;  // the authenticated player submitting it
  readonly command: Command;
}
```

`clientSeq` is assigned and owned by the client — it is not currently read or
validated by `LocalMatchHost`/`Match` (`Match.submit` takes `playerId` and
`command` directly, not the envelope), but the shape exists precisely so a
real networked client can tag every command it sends for its own
ordering/dedup/ack bookkeeping without the server needing to invent that
scheme later — it is part of the wire contract from the start, not
speculative.

## The event envelope

```ts
interface EventBatch {
  readonly events: readonly SimEvent[];  // a contiguous batch, seq-ordered
  readonly cursor: number;               // the next cursor value to request
}
```

Every `SimEvent` (`sim/events.ts`) carries a monotonic `seq`, stamped by the
single `EventSink` on the match (`EventSink.emit`). `MatchApi.eventsSince
(cursor)` returns every event with `seq >= cursor` plus the match's current
`eventSeq` as the batch's `cursor`, so a client's poll loop is simply: `batch
= eventsSince(myCursor); myCursor = batch.cursor;` — repeated indefinitely,
never missing or duplicating an event as long as it always saves the
returned cursor.

## The `MatchApi` interface

`api/match-api.ts` — the **only** surface any actor (human UI, bot, or a
future remote client) is allowed to talk to a match through:

```ts
interface MatchApi {
  submit(env: CommandEnvelope): CommandResult;   // accept/reject, transactional
  view(): MatchState;                             // full read-only current state
  eventsSince(cursor: number): EventBatch;         // incremental event drain
  isComplete(): boolean;                           // phase === "match_complete"
}
```

It is deliberately free of any networking, DOM, or renderer concept — no
`fetch`, no `WebSocket`, no `postMessage`. `LocalMatchHost`
(`api/local-host.ts`) is the one implementation today: an in-process host
that owns a `Match`, implements this interface directly (`submit` just calls
`this.match.submit(env.playerId, env.command)`), and additionally exposes
host-only controls the browser loop would need (`start`, `tick`,
`advancePhase`, `runToCompletion`, `getMatch`, `getDecisionLog`) that are
**not** part of the wire-shaped `MatchApi` — those are local-process
conveniences, not client-visible protocol.

## Sequence handling

Two independent sequence counters live on `MatchState` (`sim/model.ts`),
both mirrored from internal counters back onto state every time either
changes (`Match.syncSeqs`):

- **`commandSeq`** — incremented once per **accepted or rejected** submitted
  command (`Match.submit`), and recorded in the replayable command log
  (`LoggedCommand { seq, tick, playerId, command }`, `Match.getCommandLog()`).
  This is a total order over everything any player attempted, not just what
  succeeded.
- **`eventSeq`** — the next `seq` the `EventSink` will assign. Every
  emitted `SimEvent`, from every subsystem (economy, forging, combat,
  pairing, resolution, phase transitions), goes through the same one
  `EventSink` instance the `Match` owns, so `eventSeq` is a single global
  total order across the whole match — not per-player, not per-phase, not
  per-combat. A single combat's own stream is simply the subset of this one
  log with a matching `combatId` (`EventSink.combatStream`).

Because both are monotonic integers on serializable `MatchState`, a client
can always ask "what's changed since I last looked" (`eventsSince`) and
"what have I already tried" (`commandSeq` growth) without any additional
bookkeeping structure.

## Snapshot handling and the reconnect seam

A late-joining or reconnecting client's recovery path is exactly two calls:

```ts
const state = matchApi.view();          // full current MatchState — the snapshot
const { events, cursor } = matchApi.eventsSince(0);  // (or from a remembered cursor)
```

`view()` returns the complete authoritative `MatchState` — every player's
health/gold/hand/warband/shop, the pool, the current phase, pairings, round —
which is plain, JSON-serializable data (see `ARCHITECTURE.md`'s determinism
section). A client reconnecting after a drop does not need to replay the
entire event log from `seq` 0 to reconstruct state: `view()` alone gives it
the current authoritative snapshot, and `eventsSince(cursor)` from whatever
cursor it last acknowledged gives it exactly the events it missed to animate
catch-up (or `eventsSince(0)` for a client that never had a cursor, to
animate the whole match history if desired). This dual path — full snapshot
for correctness, event tail for animation — is why `SimEvent` never needs to
carry enough information to *reconstruct* state on its own: `view()` is
always the ground truth, and events are a reporting channel layered on top of
it (see `ARCHITECTURE.md`: "the renderer never decides a combat result; it
only replays one").

The event log itself is never truncated in the current implementation
(`EventSink.log` is append-only for the life of the `Match`), so
`eventsSince(0)` always works — a full history replay is possible for any
match at any point, which is also exactly the mechanism the determinism test
in `harness/headless.test.ts` relies on (`JSON.stringify({ state, events })`
equality between two independently-run hosts from the same seed).

## Why `LocalMatchHost` can be swapped for a remote host without client changes

Every actor in the system — including the seven bots — talks to the match
through the identical `MatchApi.submit`/`view`/`eventsSince`/`isComplete`
surface:

- `LocalMatchHost.runBotsIfNeeded` (`api/local-host.ts`) drives each bot via
  `deps.submit(playerId, command)`, which is `this.match.submit(...)` — the
  same call `LocalMatchHost.submit` makes for a human's `CommandEnvelope`.
  Bots never touch `Match` internals, `MatchState` mutation, or the event
  sink directly; they only enumerate legal commands (`BotPolicy.candidates`)
  and submit them through the public surface (`bots/driver.ts`
  `runBotTurn`). **This is the existing proof that the command surface is a
  real, sufficient contract** — an entirely separate actor (bot logic) drives
  a full game to completion through nothing but `submit`.
- Because `MatchApi` has zero networking/DOM/renderer concepts baked in, a
  remote authoritative host — running `Match` on a server, exposing the same
  four methods over a WebSocket (commands in as `CommandEnvelope` JSON,
  events out as `EventBatch` JSON) — is a drop-in replacement. Client code
  (UI or bot) that only ever calls `matchApi.submit(...)`,
  `matchApi.view()`, `matchApi.eventsSince(cursor)`, `matchApi.isComplete()`
  does not know or care whether `matchApi` is a `LocalMatchHost` wrapping an
  in-process `Match`, or a thin RPC stub forwarding to a server process
  running the identical `Match` class.
- **Authority stays server-side, commands are validated centrally, events are
  the only state channel** — this is already true even in the local
  single-process case: `applyShopCommand` (`sim/economy.ts`) is the *only*
  code path that mutates `PlayerState`, and it validates a command fully
  before any mutation (see `GAME_RULES.md` §2). A remote host changes
  *where* that validation runs (a server process instead of the browser
  tab), not *what* it does — the validation logic itself does not change,
  because it already lives entirely inside `Match`/`sim/economy.ts`, never
  in client code. There is no client-side prediction or client-side state
  mutation to strip out when moving to a remote host, because none exists
  today — the client (UI or bot) is already a pure `submit`/`view` consumer.

In short: `LocalMatchHost` is not a special case with hidden shortcuts that a
remote host would need to unwind — it is the `MatchApi` contract, honestly
implemented in-process, with zero client-visible behavior that a
network-backed implementation of the same four methods couldn't reproduce.
