# axiom-atlas — the repo's query-and-change gateway (`ax`)

`ax` is the one surface through which agents search, read, and change this
repo. It exists for two reasons:

1. **One rulebook.** Every path passes a single scoping check, so the tool is
   structurally incapable of reading or writing anything outside the checkout
   it was invoked in. `../../`, absolute paths, and symlinks that point out are
   all resolved *before* the containment test, and `.git/` is never touched.
2. **A record.** Every invocation appends one NDJSON row to a ledger. That
   turns agent behaviour into data: what gets searched for repeatedly, where
   the work actually lands, and — the point of the whole exercise — which
   searches came back **empty**. A search that finds nothing is a question this
   repo could not answer, which is the only honest signal for what it is
   missing.

Repo tooling: outside the engine dependency graph, and outside the coverage and
branchless gates (Module Law §5).

## Why it is fast

Speed is a feature, not a nicety — an agent will only route through a tool that
beats the habit it replaces. `ax` links ripgrep's own `ignore` walker and
`grep-searcher` as *libraries* (no subprocess), skips `target/`, `node_modules/`
and `.git/` outright, and parses its CLI by hand.

Measured on this repo (3,815 tracked files, ~2,000 Rust sources), warm cache:

| | per query |
|---|---|
| `ax q` | **~94 ms** |
| `rg` (raw ripgrep) | ~232 ms |

## Commands

```text
Search
  ax q <regex> [--path RE] [--lang L] [--limit N] [-i] [-F] [--json]
  ax def <symbol>                  where a symbol is defined (Rust + TS)
  ax refs <symbol>                 every mention of a symbol
  ax file <regex>                  find files by path

Read and change (scoped to this repo, always)
  ax read <path> [--range A:B]
  ax edit <path> --replace <old> --with <new> [--all]
  ax write <path>                  content on stdin
  ax apply                         batch edits as JSON on stdin, all-or-nothing
  ax record <path> [--bytes N] [--tool T]   log a change made outside ax

Architecture
  ax graph [<layer|module|app>]    deps, dependents, and the laws in force
  ax owns <path>                   which package owns a file, and its rules

The ledger
  ax friction <what> [--want X] [--verdict tool|repo|unknown]
  ax resolve <friction-id> [--by <what fixed it>]
  ax miss [--limit N] [--all]      what the repo, and the tool, could not do
  ax stats [--limit N]             what agents look for and change
  ax compact                       roll closed days into Parquet
  ax sql                           DuckDB queries over the whole ledger
```

`--lang`: `rs ts js web toml md py shader json`

Exit codes: `0` found · `1` nothing found (grep convention) · `2` usage ·
`3` out-of-repo path refused · `4` failed.

Environment: `AXIOM_ATLAS_AGENT`, `AXIOM_ATLAS_SESSION`,
`AXIOM_ATLAS_NO_LEDGER`, `AXIOM_ATLAS_ROOT`, `AXIOM_ATLAS_DEBUG`,
`AXIOM_ATLAS_REF_ROOTS`.

## Reading a reference tree

`AXIOM_ATLAS_REF_ROOTS` (platform path separator, like `PATH`) names trees
`ax read` may look into and that **nothing** may ever write to.

It exists for porting: the source being ported lives outside the checkout, so
without it every source read goes around `ax` — which loses the scoping story
for the files an agent spends most of its time in, and makes them invisible to
the ledger. A ledger that cannot see what an agent actually read is answering
the wrong question.

Read-only is structural rather than a flag. Every mutating command resolves
through `Repo::resolve`, which consults the repo root alone; only `ax read`
calls `Repo::resolve_read`. Adding a readable tree therefore cannot widen what
`ax` can change. `.git/` stays refused inside a reference root too.

```sh
AXIOM_ATLAS_REF_ROOTS=/dev/Claude-of-Duty ax read /dev/Claude-of-Duty/src/render/probe.js
```

Paths outside the checkout print with a `ref:` marker, so nothing in the output
can be mistaken for a file inside it.

### `ax owns` — the command worth learning first

Before touching a file, ask who owns it and which of Axiom's laws bind you
there. This is the difference between writing a branchless `map`/`fold` and
having the dylint gate reject your `if`:

```console
$ ax owns modules/axiom-scene/src/lib.rs
modules/axiom-scene  [scene]
  class          Engine module (isolated capability)
  layers         kernel, runtime, math, frame, ecs
  depended on by rotating-cube-demo, engine, render-pipeline
  laws in force
    - Branchless Law - no if/else, match, for/while/loop, &&/||, ?, if let ...
    - Coverage Law - 100% regions/lines/functions; new code ships with its tests
    - Module Law - allowed_modules must be empty; never depend on another module
```

The graph is read through the architecture checker's own manifest loaders
(`xtask::manifest`, `::module_manifest`, `::app_manifest`), so what `ax` reports
and what `cargo xtask check-architecture` enforces come from one definition.

## The ledger

One NDJSON line per invocation, under `.axiom-atlas/ledger/` (gitignored):

```text
.axiom-atlas/ledger/raw/2026-08-22.ndjson     # hot, append-only
.axiom-atlas/ledger/parquet/2026-08-21.parquet # compacted by `ax compact`
```

NDJSON is deliberate. A single short `write_all` to a handle opened in append
mode is atomic enough that many agents can search concurrently without locking
or losing rows — a database file holding an exclusive write lock would make
parallel agents block or fail. Parquet arrives at compaction time, once a day is
closed, via DuckDB (no Arrow dependency in the hot path).

A ledger failure never fails the command the agent asked for.

### Querying it

`ax miss` and `ax stats` cover the common questions. For anything else,
`ax sql` prints a ready-to-paste DuckDB session that unions raw and compacted
rows into one relation:

```sql
CREATE OR REPLACE VIEW ledger AS
  SELECT * FROM read_json_auto('.../raw/*.ndjson', union_by_name=true)
  UNION ALL BY NAME
  SELECT * FROM read_parquet('.../parquet/*.parquet');

-- What the repo could not answer, most-wanted first.
SELECT query, count(*) AS misses, count(DISTINCT session) AS sessions
  FROM ledger WHERE zero_result GROUP BY 1 ORDER BY 2 DESC LIMIT 40;
```

Row shape: `ts`, `day`, `session`, `agent`, `cmd`, `id`, `query`,
`scope{path,lang}`, `hits`, `files_matched`, `zero_result`, `top_paths[]`,
`bytes_changed`, `duration_us`, `ok`, `note`.

### Friction, and closing it

`ax friction` records what the *tool* could not do; `ax miss` lists it beside
the zero-result searches that record what the *repo* could not answer. Each
friction carries a stable id — an FNV-1a hash with an fmix64 finalizer over the
normalised text — so the same complaint always lands on the same id, repeats
count rather than duplicate, and a 7-character prefix is safe to type.

Because the ledger is append-only, `ax resolve <id>` appends a row naming the id
it closes rather than editing the original. `ax miss` subtracts closed ids;
`ax miss --all` shows them again, marked. A failed resolve closes nothing.

Zero-result searches need no equivalent: they close themselves the moment the
thing exists and the search starts finding it.

## Determinism

Two identical queries return byte-identical output. Hits are collected in full
(to a 50,000 hit ceiling), sorted by `(path, line)`, and only then truncated to
`--limit` — rather than racing threads to a limit, which would make results
depend on scheduling. The engine is held to determinism; its tooling should be
too.

## Routing

`.claude/settings.json` wires two hooks: `Grep` is blocked and redirected to
`ax q`, and `Edit`/`Write`/`MultiEdit` are logged through `ax record` after the
fact. Edits are recorded rather than intercepted so that multi-line edits keep
their ergonomics while the ledger stays complete. See the "Atlas Rule" section
of the repo-root `CLAUDE.md`.
