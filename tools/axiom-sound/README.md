# `axiom-sound`

**A Strudel-based game-sound asset pipeline.** Point it at an Axiom app; it
discovers small [Strudel](https://strudel.cc) sound sources belonging to that
app, compiles and **semantically validates** them with the real pinned Strudel
implementation, renders them offline to lossless audio, validates the PCM,
encodes the final asset as **MP3**, and atomically publishes the MP3 plus a
generated audio manifest into the app's `assets/` directory.

This is a **Tool**. It lives under `tools/`, sits **outside the engine
dependency graph**, and is exempt from the coverage / branchless gates. It is a
self-contained Node/TypeScript npm package — **Strudel, FFmpeg, and Chromium
are isolated inside this tool and never link into or ship with the Axiom
runtime.** The only things an app ever consumes are the generated `.mp3` files
and the generated `manifest.json`.

## Why MP3 is the shipped format (and WAV is only intermediate)

Version one emits exactly one runtime format: **MP3** — small, universally
decodable in every browser via `<audio>` / WebAudio, and adequate for game SFX.
The tool renders internally to a **48 kHz 16-bit lossless WAV** purely as a
validation/cache intermediate (stored under a gitignored tool cache); it is
never published. Keeping WAV internal means PCM validation (silence, clipping,
non-finite, duration, channels) runs on exact samples before any lossy step.

## The source format

One source per sound: TOML front matter delimited by `+++`, then ordinary
Strudel code. Remove the front matter and the body pastes into the Strudel REPL
unchanged.

```
+++
id = "ui-perfect"
duration_ms = 900
tail_ms = 200
channels = 1
bitrate_kbps = 128
+++

note("c5 e5 g5")
  .s("triangle")
  .attack(0.005)
  .decay(0.12)
  .sustain(0)
  .release(0.18)
  .gain(0.7)
```

Fields (unknown fields are an error, so typos cannot slip through). All but
`render` are required:

| field          | meaning                                            | rule                                   |
|----------------|----------------------------------------------------|----------------------------------------|
| `id`           | stable kebab-case asset id                         | `^[a-z0-9]+(-[a-z0-9]+)*$`, **== filename stem** |
| `duration_ms`  | authored length before the effect tail             | integer, `0 < d ≤ 60000`               |
| `tail_ms`      | extra render time for release / echo / reverb      | integer, `0 ≤ t ≤ 30000`               |
| `channels`     | mono or stereo                                     | exactly `1` or `2`                     |
| `bitrate_kbps` | constant MP3 bitrate                               | one of `96 128 160 192 256 320` (default `128`) |
| `render`       | render pipeline (optional)                         | `offline` (default) or `realtime`      |

The render length is `duration_ms + tail_ms`.

**Effects.** AudioWorklet effects — `distort`, `shape`, `coarse`, `crush` — work:
their worklet modules are the packages' own inline `data:` URLs, loaded in-memory
with no network. Noise (`s("white"|"pink"|"brown")`), `room`/reverb, and `delay`
also render. `samples(...)` / sample-bank sounds still fail (they need the blocked
network).

**Determinism & the two render modes.** The default `offline` mode renders through
an `OfflineAudioContext` and is byte-deterministic for pure oscillators
(`triangle`/`sine`/`sawtooth`/`square`); noise sources make it audible but not
byte-reproducible. Some effect chains — notably a **distorted signal into
`.room()` reverb** — generate denormal floats in the offline render's decaying
tail that the offline audio thread does not flush to zero, stalling the render.
For those, set `render = "realtime"`: the pattern is played through a live
`AudioContext` in wall-clock time (whose audio thread *does* flush denormals) and
the master mix is captured. Realtime is **non-deterministic** (live noise, wall
clock) and takes at least the sound's own length to render; a peak guard
attenuates the captured mix to sit just under full scale. Opt in only when a sound
genuinely needs it.

## File layout

```
<app>/sounds/<id>.strudel          # authored source (never shipped as an asset)
<app>/assets/audio/<id>.mp3        # generated runtime asset
<app>/assets/audio/manifest.json   # generated manifest (see below)
```

## Commands

```sh
axiom-sound new     --app <app-path> --name <sound-id>
axiom-sound check   --app <app-path> [--name <sound-id>]
axiom-sound build   --app <app-path> [--name <sound-id>] [--force]
axiom-sound list    --app <app-path>
axiom-sound clean   --app <app-path>
axiom-sound preview --app <app-path> --name <sound-id>
```

Run via npm (this repo does not add a new package manager):

```sh
npm --prefix tools/axiom-sound install          # once (also downloads Chromium on first run)
npm --prefix tools/axiom-sound run check -- --app apps/my-app
npm --prefix tools/axiom-sound run build -- --app apps/my-app --name ui-perfect
# or via make:
make sound-build APP=apps/my-app
```

Global flags: `--json` (machine output on stdout, diagnostics on stderr),
`--verbose` (stacks / underlying causes), and `--force` (bypass the build
cache). Every failure exits nonzero.

- **`new`** — scaffold `<app>/sounds/<id>.strudel` from a template with a tiny
  placeholder tone. Never overwrites.
- **`check`** — validate one or every source with the real Strudel transpiler +
  evaluator: parse front matter → transpile → evaluate → assert a valid pattern
  → query the pattern across its finite range (surfacing lazy errors) →
  capture exceptions / rejections / `pageerror` / console errors, mapped to
  `file:line:column`. Writes nothing.
- **`build`** — runs the full `check` first (never renders invalid source), then
  renders → validates PCM → encodes MP3 → validates with FFprobe → atomically
  publishes → updates the manifest. Skips a sound whose source hash already
  matches its built output (`--force` to override).
- **`list`** — a stable, sorted inventory: id, source path, asset path, source
  status, build status, duration, channels, source hash.
- **`clean`** — remove generated MP3s, the generated manifest, and this app's
  cached WAV renders. **Never** touches `.strudel` sources.
- **`preview`** — build if stale, then open the MP3 with the OS default player.

## How Claude authors, checks, listens, revises, and builds a sound

1. `axiom-sound new --app <app> --name ui-perfect` — scaffold the source.
2. Edit `<app>/sounds/ui-perfect.strudel` — write Strudel in the body.
3. `axiom-sound check --app <app> --name ui-perfect` — fix any reported
   `file:line:col` errors until it passes.
4. `axiom-sound preview --app <app> --name ui-perfect` — build + listen.
5. Revise the body; repeat 3–4. The `.strudel` file is the editing surface.
6. `axiom-sound build --app <app>` — build every sound; commit the generated
   `.mp3` + `manifest.json`.

## How an app consumes the result

There is **no audio-file runtime in Axiom yet** — engine audio today is
procedural (`packages/axiom-web-engine` WebAudio, `modules/axiom-audio`). So the
tool stops at producing a correctly located MP3 + manifest, and documents the
lookup contract for an eventual audio loader:

- Assets live at `<app>/assets/audio/<id>.mp3`.
- The manifest at `<app>/assets/audio/manifest.json` maps each `id` to a `path`
  **relative to the app's `assets/` directory** (e.g. `audio/ui-perfect.mp3`),
  plus `mimeType`, `durationMs`, `channels`, `sampleRate`, `bitrateKbps`, and
  the `sha256` / `sourceSha256` hashes.
- A loader resolves an asset by `manifest.assets[id].path` joined to `assets/`,
  fetches the bytes, and decodes the MP3 with the platform decoder
  (`AudioContext.decodeAudioData` in the browser). Strudel is **not** involved
  at runtime.

### Manifest shape

```json
{
  "schemaVersion": 1,
  "generatedBy": "axiom-sound",
  "assets": {
    "ui-perfect": {
      "path": "audio/ui-perfect.mp3",
      "mimeType": "audio/mpeg",
      "durationMs": 1100,
      "channels": 1,
      "sampleRate": 48000,
      "bitrateKbps": 128,
      "sha256": "<encoded-file-sha256>",
      "sourceSha256": "<source+config+versions sha256>"
    }
  }
}
```

Keys are written in deterministic sorted order; paths are relative; there are no
absolute paths and no timestamps; unrelated entries are preserved; the file is
written atomically and is not rewritten when the serialized content is
unchanged. `generatedBy: "axiom-sound"` marks the manifest as tool-generated.

## Caching

A build is skipped when the sound's `sourceSha256` matches the existing manifest
entry and the MP3 exists. The source hash covers: the complete `.strudel`
contents, the parsed render config, the exact pinned Strudel package versions,
the tool version, the renderer version, and the encoder settings — so any change
that could alter output bytes invalidates the cache. Use `build --force` to
rebuild regardless.

## Inspecting a failed render

- Re-run with `--verbose` for stacks and the underlying cause.
- `check` maps Strudel errors to the source `file:line:column`.
- A `RENDER_SILENT` / `RENDER_CLIPPED` failure reports the measured RMS / peak.
- A `NETWORK_ACCESS_ATTEMPTED` failure names the first blocked URL — usually a
  `samples(...)` call or a sample-bank sound; switch to a synth voice.

## Error codes

Stable machine-readable codes (also emitted under `--json`):
`APP_NOT_FOUND`, `APP_MANIFEST_NOT_FOUND`, `INVALID_SOUND_ID`,
`INVALID_FRONT_MATTER`, `DUPLICATE_SOUND_ID`, `STRUDEL_TRANSPILE_FAILED`,
`STRUDEL_EVALUATION_FAILED`, `STRUDEL_PATTERN_INVALID`,
`NETWORK_ACCESS_ATTEMPTED`, `RENDER_TIMEOUT`, `RENDER_SILENT`, `RENDER_CLIPPED`,
`RENDER_INVALID_PCM`, `ENCODE_FAILED`, `ENCODE_VALIDATION_FAILED`,
`MANIFEST_WRITE_FAILED`.

## License boundaries

**Strudel is AGPL-3.0-or-later.** It is used here strictly as an isolated,
build-time renderer inside this tool. It is **not** linked into, bundled with,
or shipped as part of the Axiom engine or any Axiom app — only the generated
`.mp3`/`.json` outputs reach an app. **FFmpeg** (via the pinned `ffmpeg-static`
/ `ffprobe-static` binaries) is likewise a build-time tool, invoked as a
separate process. See [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) for the
full attribution and boundary statement. This project makes no legal claims
about the copyright status of audio you generate; it documents the boundary and
preserves the required upstream notices.

## Tests

```sh
npm --prefix tools/axiom-sound test
```

See [`TESTING.md`](TESTING.md). Architecture and internals are in
[`ARCHITECTURE.md`](ARCHITECTURE.md).
