# Third-party notices — axiom-sound

`axiom-sound` is a **build-time repository tool**. The dependencies below are
used only while authoring/generating audio assets inside this tool. **None of
them is linked into, bundled with, or shipped as part of the Axiom engine or any
Axiom app.** The only artifacts that reach an app are the generated `.mp3` files
and the generated `manifest.json`.

## The boundary (read this first)

- **Strudel is AGPL-3.0-or-later.** It runs here strictly as an isolated,
  build-time renderer: this tool feeds it a `.strudel` source and reads back
  fixed PCM. Strudel is never imported by, compiled into, or distributed with
  the runtime engine or a game. Its AGPL obligations therefore attach to this
  tool (which lives in the Axiom repository under the repository's own license),
  not to any shipped Axiom binary or app.
- **FFmpeg** is invoked as a separate child process via the pinned
  `ffmpeg-static` / `ffprobe-static` binaries. It is a build-time encoder/prober,
  not a library linked into anything Axiom ships.
- **Chromium** (via Playwright) is a build-time headless render host, downloaded
  by Playwright into its own cache. It is not part of any Axiom deliverable.

This file documents the boundary and preserves the required upstream notices. It
makes **no** legal claim about the copyright status of audio you generate with
Strudel — that is between you and the tools/samples you use. Because the tool
renders only pure-oscillator synthesis (and blocks all network / sample loading
during rendering), no third-party sample content is embedded in generated
assets.

## Dependencies and licenses

Runtime dependencies of the tool (exact pinned versions; see `package-lock.json`
for the full transitive tree):

| package                | version | license                |
|------------------------|---------|------------------------|
| `@strudel/core`        | 1.2.6   | AGPL-3.0-or-later      |
| `@strudel/mini`        | 1.2.6   | AGPL-3.0-or-later      |
| `@strudel/tonal`       | 1.2.6   | AGPL-3.0-or-later      |
| `@strudel/transpiler`  | 1.2.6   | AGPL-3.0-or-later      |
| `@strudel/webaudio`    | 1.3.0   | AGPL-3.0-or-later      |
| `superdough`           | 1.3.0   | AGPL-3.0-or-later      |
| `playwright`           | 1.48.2  | Apache-2.0             |
| `ffmpeg-static`        | 5.2.0   | GPL-3.0-or-later (packaging); ships an FFmpeg binary (GPL/LGPL per build) |
| `ffprobe-static`       | 3.1.0   | MIT (packaging); ships an FFprobe binary (GPL/LGPL per build) |
| `smol-toml`            | 1.3.1   | BSD-3-Clause           |
| `esbuild`              | 0.25.5  | MIT                    |

## Attribution

- **Strudel** — © the Strudel authors (Tidal Cycles / uzu community).
  <https://strudel.cc> · <https://codeberg.org/uzu/strudel>. Licensed under the
  GNU Affero General Public License v3.0 or later. The full AGPL text is
  distributed with each `@strudel/*` and `superdough` package under
  `node_modules/<pkg>/` and at
  <https://www.gnu.org/licenses/agpl-3.0.html>.
- **FFmpeg** — © the FFmpeg developers. <https://ffmpeg.org>. Binaries provided
  by `ffmpeg-static` / `ffprobe-static`; their licenses (GPL/LGPL depending on
  the build) travel with those packages under `node_modules/`.
- **Playwright** — © Microsoft, Apache-2.0. **esbuild** — © Evan Wallace, MIT.
  **smol-toml** — © the smol-toml authors, BSD-3-Clause.

Each dependency's own `LICENSE` file remains present in `node_modules/<pkg>/`
after `npm install`; those files are the authoritative notices and are not
reproduced or altered here.
