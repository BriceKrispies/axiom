# Zanzoban agent golden — pre-State-Engine baseline

The committed record of the real `axiom-agent` driver playing Zanzoban level 1,
captured **before** the State Engine (`crates/axiom-state`) existed, so that the
migration can be held to exactly this behaviour.

## Provenance

| | |
|---|---|
| Captured at commit | `c6cdca18` (`state-law: make hidden retained state mechanically visible`) |
| Branch | `feat/state-engine` |
| Level | `LEVEL_001_TOML`, embedded in the crate |
| Driver | `axiom_zanzoban::agent::play_first_level()` — every move goes through `axiom_agent::AgentApi::step`'s `observe → decide → emit` cycle and is lowered back into a grid `Direction` |
| Seed / random stream | **none — there is no RNG.** Ghost replay is driven by a recorded path and time advances in whole `Tick`s. Determinism comes from the level plus the transcript, so there is no seed to pin. |
| Capture + verify command | `cargo test -p axiom-zanzoban --features agent --test agent_golden` |
| Re-capture (intended change only) | `AXIOM_REGOLD=1 cargo test -p axiom-zanzoban --features agent --test agent_golden` |

The `agent` feature is native-only and off by default, so a plain
`cargo test --workspace` does not compile the driver. The feature flag above is
required.

## Artifacts

Three files, kept separate so a future mismatch localizes to a stage rather than
just reporting "the bytes moved".

| File | Bytes | What it pins |
|---|---:|---|
| `agent_transcript.bin` | 210 | **The recorded run.** Every command the driver applied to the game core, in order — including the `Tick`s and the `q` that the move list omits, and including moves the core rejected. `u32` count, then `[kind, payload]` per command. |
| `agent_trajectory.bin` | 4390 | The state after each command: player, every ghost, recording length, tick, solved flag. The deterministic consequence of the transcript. |
| `agent_outcome.bin` | 229 | The result: solved, the emitted move list, ghost count, final tick, and the milestone events. |

Every value is integer/enum data — not one `f32` — so the bytes are
platform-stable and exact equality is the right bar. There is no GPU artifact
here and therefore no tolerance policy to apply.

## The run this pins

```
walked to the button at (4, 5) in 3 moves
pressed q — ghost #1 will replay that path
the ghost reached the button — gate "main" is open (after 90 ticks)
reached the exit at (8, 5)

moves emitted through axiom-agent:
  [Right, Right, Right, Up, Right, Right, Right, Right, Down, Right, Right, Right]

outcome: solved=true  ghosts=1  final_tick=102
```

## How the regression comparison works

`replaying_the_committed_transcript_reproduces_the_trajectory` reads
`agent_transcript.bin` and replays it against a **fresh** `PuzzleGameState` with
the agent absent, then compares against `agent_trajectory.bin`. That is the
regression bar: a recorded run replayed against changed engine code, not the
agent improvising a new run. A re-run would only prove the agent is still
deterministic; a replay proves the *engine* still behaves.

Four properties guard the goldens themselves:

- `the_agent_run_is_repeatable` — two independent agent runs agree on all three
  artifacts. Verified byte-identical across three consecutive runs at capture.
- `a_perturbed_transcript_produces_different_bytes` — dropping the last command
  must move the trajectory bytes. A golden that cannot fail is worse than none.
  (The perturbation drops a command rather than editing one, because an edited
  early move can be swallowed by the core — a rejected move changes nothing.)
- `the_transcript_exercises_ghost_replay` — the run must create a ghost and tick
  it, so the golden covers the interesting path rather than a straight walk.
- `golden_agent_transcript` asserts the baseline run is a **winning** one.

`AXIOM_REGOLD` is compared against `"1"`, not merely tested for presence:
`AXIOM_REGOLD=0` silently reading as "re-bless everything" is exactly the footgun
that destroys a baseline without anyone noticing.

**Do not re-capture these to make a mismatch go away.** A mismatch after the
State Engine migration is a behavioural regression until proven otherwise.
