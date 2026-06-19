# Axiom Client Core — Testing

All tests are inline `#[cfg(test)]` modules next to the code they cover. Run
them with:

```sh
cargo test -p axiom-client-core
```

The module is part of the engine spine, so it is held to the Coverage Law (100%
regions/lines/functions) and the Branchless Law (zero control flow in non-test
code). The workspace coverage gate covers it:

```sh
scripts/coverage.ps1        # Windows
bash scripts/coverage.sh    # Linux / CI
```

## What the tests prove

State machine (`client_core_api`):

- initial state is `Disconnected`, sequence `1`, tick `0`, no pending, last-ack `0`;
- `connect()` transitions `Disconnected → Connecting`, and is rejected from any
  other state;
- `accept_welcome()` transitions `Connecting → Connected`, and is ignored from
  `Disconnected` or from `Connected` (a second welcome);
- `next_intent` **fails (`None`) while `Disconnected`** and **while
  `Connecting`**, and **succeeds (`Some`) while `Connected`**;
- `client_sequence` **starts at 1** and **increments deterministically** (1, 2,
  3, …);
- `pending_intent_count` increments as intents are produced;
- a snapshot ack **drains pending intents up to `last_accepted_client_sequence`**
  and updates `last_acked_client_sequence`;
- pending **insertion order is preserved** across acks and rejections;
- `RejectedIntent` **removes exactly that pending sequence** (and an absent
  sequence is a harmless no-op);
- a snapshot **before `Welcome` is rejected**; a **rejection before `Welcome` is
  rejected**;
- **older snapshots are rejected, an equal tick is allowed**;
- `latest_server_tick` **updates from snapshots**;
- the **whole flow is deterministic**: two clients fed identical inputs reach
  byte-identical observable state.

Connection-state enum (`connection_state`):

- the `#[repr(u8)]` discriminants are stable (`0`/`1`/`2`) and the states are
  distinct.

## Determinism

Every method is pure over the client's own fields — no clock, no randomness, no
global state. The replay test (`the_whole_flow_is_deterministic_when_replayed`)
asserts that identical call sequences produce identical state.
