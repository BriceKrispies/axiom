# axiom-sound — testing

## Running

```sh
npm --prefix tools/axiom-sound install    # once (downloads Chromium on first run)
npm --prefix tools/axiom-sound test       # node --test over test/**/*.test.ts
npm --prefix tools/axiom-sound run typecheck
# or:
make sound-test
```

`test` runs the whole suite under Node's built-in test runner (`node --test`),
executing the TypeScript sources directly via Node ≥ 24 native type-stripping —
the same mechanism the repo's `packages/*` TS packages use. As a Tool,
`axiom-sound` is outside the engine's 100%-coverage / branchless gates, but it
ships a real suite that exercises every command and every documented behavior.

## Prerequisites

- **Node ≥ 24** (native `.ts` execution + `node:test`).
- **Chromium** via Playwright — installed on first `npm install`
  (`npx playwright install chromium`) or on first render.
- **FFmpeg / FFprobe** — provided by the pinned `ffmpeg-static` /
  `ffprobe-static` packages (no PATH dependency).

## Fixture strategy

The committed fixture app lives at `test/fixtures/app/` — an `app.toml` plus a
`sounds/` directory of authored `.strudel` fixtures, each engineered to exercise
one behavior:

| fixture              | proves                                             |
|----------------------|----------------------------------------------------|
| `tone-ok`            | a valid mono triangle tone (check + build + probe) |
| `tone-two`           | a second, stereo sound (multi-build isolation)     |
| `bad-syntax`         | invalid JavaScript → transpile failure             |
| `bad-mini`           | invalid mini-notation → Strudel failure            |
| `unknown-fn`         | unknown Strudel function → evaluation failure      |
| `silent`             | `gain(0)` → silence rejection                       |
| `clipping`           | `gain(9)` → clip rejection                          |
| `remote-sample`      | `samples('github:…')` → blocked network access     |

The committed fixture is **never built in place**. Each test copies it into a
fresh OS temp dir (`makeTempApp()` in `test/helpers.ts`), builds there, and
removes both the temp app and its tool cache on cleanup. Command output is
captured through a JSON-mode collecting `Reporter` (`Reporter.collecting`) — no
global `process.stdout` swap, so the test runner's own output stays intact.

## What the suite covers

**Unit** (fast, no browser):

- `frontmatter.test.ts` — front-matter parsing; missing/unknown fields; empty
  body; TOML error location.
- `config.test.ts` — duration/tail/channels/bitrate bounds and allowlists;
  encoder signature.
- `ids.test.ts` — kebab-case id validation; filename/id mismatch; path-traversal
  rejection.
- `hash.test.ts` — source-hash determinism and invalidation (body + config).
- `manifest.test.ts` — stable sorted ordering; relative paths; foreign-entry
  preservation; no-rewrite-when-unchanged.
- `atomicwrite.test.ts` — atomic write; symlink-destination refusal.
- `pcm-wav.test.ts` — WAV encoding; PCM validation (empty/silent/clip/non-finite/
  channels/duration).
- `commands.test.ts` — `new` scaffolding + no-overwrite; app / manifest not
  found; duplicate-id detection; `list` ordering; CLI usage/unknown-command.

**Exporter** (`exporter.test.ts`, browser) — pins the assembled offline exporter
against the pinned Strudel versions: proves it renders audible, non-clipping,
**byte-deterministic** PCM. This is the guard the task requires that the
minimal exporter still works with the pinned Strudel version.

**Integration** (`integration.test.ts`, browser + FFmpeg) — the 16 required
end-to-end behaviors:

1. a valid tone passes `check`
2. invalid JS fails `check`
3. invalid mini-notation fails `check`
4. an unknown function fails semantic evaluation
5. a failed check produces no MP3
6. a valid sound builds a WAV internally and a final MP3
7. the MP3 has the configured duration, channels, sample rate, bitrate
8. the manifest has the correct relative path + both hashes
9. rebuilding unchanged input is a cache hit and does not rewrite output
10. changing the source invalidates the cache
11. an all-silent pattern is rejected
12. a clipping pattern is rejected
13. an attempted remote sample fetch is blocked
14. building two sounds in one invocation does not leak Strudel state
15. `clean` removes generated output but preserves source
16. a destination write failure leaves the previous valid asset untouched

## Validation commands

The tool is validated by its own suite plus the repo gates it must not disturb
(it is not a Cargo member, so these must stay green):

```sh
npm --prefix tools/axiom-sound test          # the tool's gate
npm --prefix tools/axiom-sound run typecheck  # tsgo --noEmit
cargo xtask check-architecture                # unaffected: tool is off the graph
cargo test --workspace                        # unaffected
```
