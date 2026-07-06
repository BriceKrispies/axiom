# IP / Trademark Hygiene

> **Structure note (2026-07-06):** the `games/` cartridge tier was later retired and
> `retro-fps` folded back into the gallery app as the in-crate `apps/axiom-gallery/src/retro_fps/`
> demo module. Historical `games/retro-fps` / `axiom-game-retro-fps` / `game.toml` references
> below describe the layout at scrub time; the brand-scrub findings themselves are unchanged.

A rename-and-scrub pass that removed third-party game/platform brand references from the
repo while preserving all gameplay, rendering, tests, demos, and architecture. Two brand
terms were present — a first-person-shooter title name and an early-console render-look
name. Both are used here only as generic, descriptive replacements.

## Findings

- **No copied third-party assets.** The first-person-shooter demo is 100% original Axiom
  code — a hand-authored `level.axiom` text grid, cube geometry, and original Rust/TS
  logic. There is no game-data file, map, sprite, texture, or sound from any third-party
  title. The retro low-poly render profile is an original low-resolution + ordered-dither
  + vertex-snap effect. **Nothing was deleted; everything was renamed.**
- **Neutral generic names now in use:** the shooter demo is the **retro-fps** cartridge
  (`games/retro-fps`, crate `axiom-game-retro-fps`); the render look is the
  **retro-32-bit** profile (`FrameRetro32BitProfile` / `frame_retro_32bit`).
- **No console-manufacturer or publisher names** appear anywhere in authored source,
  docs, tests, scripts, or manifests.
- **The `.ps1` PowerShell script extension is not a brand reference** and is preserved:
  `scripts/coverage.ps1`, `scripts/ts-gate.ps1`, `scripts/wasm-test.ps1`, and every
  mention of them (e.g. in `modules/*/TESTING.md`, `tools/wasm-runner/*`, and the
  coverage-scope constant in `crates/xtask/src/coverage_scope.rs`) are unchanged.

## Rust conventions applied
kebab-case for Cargo package/directory names; snake_case for modules/functions/features;
PascalCase for types; SCREAMING_SNAKE_CASE for constants; plain generic English for
user-facing text.

## Categories renamed
Cargo manifests (root + game + gallery + screenshot tool + agent-harness + dev-reload),
`game.toml`/`app.toml`, source (the cartridge, the `axiom-host` render profile + capability,
the `axiom-gpu-backend` consumers, the soccer-penalty app, `xtask` fixtures, and doc-comment
cross-references across several modules/apps), web pages + `gallery.js` + `games-manifest.json`,
the screenshot-tool registry, the Makefile, the e2e + packaging scripts, docs, visual-target
ledgers, tests + golden fixtures, and `CLAUDE.md`. A neutral trademark disclaimer was added
to the root docs.
