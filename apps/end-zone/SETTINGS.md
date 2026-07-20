# End Zone — settings

The settings screen exposes exactly four settings, plus `BACK`. Changes apply
immediately — there is no working/committed copy, no APPLY or DISCARD, no reset
dialog, and no categories. The typed model is `EndZoneSettings`
(`src/frontend/settings.rs`); it is the entire persisted shape (`SETTINGS.md`'s
four keys, versioned — see `src/frontend/persistence.rs`).

| Setting | Values | Effect |
|---|---|---|
| `MASTER VOLUME` | `0`–`10` (normalized) | Scales all current audio output through the master gain the menu-tone path reads. |
| `MUSIC VOLUME` | `0`–`10` (normalized) | Scales the title-menu music beneath the master gain, so music can be lowered without muting UI sound effects. |
| `SCREEN SHAKE` | `OFF` / `LOW` / `FULL` | Scales the actual gameplay camera impulses (`OFF` is exactly zero shake). |
| `REDUCED MOTION` | `OFF` / `ON` | Suppresses large menu sweeps and nonessential presentation motion (flash juice, field wobble) while preserving gameplay clarity. |

Each setting drives real behavior:

- **Master volume** flows to the platform edge as the gain every procedural menu
  tone plays at (`FrontendApp::menu_tone_gain`).
- **Music volume** flows to the platform edge as the gain the title-menu music
  track plays at, beneath the master gain
  (`FrontendApp::menu_music_gain` = master × music). The music itself is an MP3
  loop played and faded by the wasm audio edge (`src/web/music.rs`).
- **Screen shake** flows into `RunConfig` and then `launch::camera_tuning`,
  which sets the camera director's `shake_scale`. `OFF` produces exactly zero
  impulse amplitude; `LOW` halves it.
- **Reduced motion** sets the theme's motion flag (the frontend background stops
  its continuous sweep and transitions collapse to short fades) and, in
  gameplay, zeroes the nonessential flash juice and damps field wobble
  (`launch::juice_tuning`).

## Audio capability note

Two audio paths are audible: the procedural menu tones (`AudioIntent` →
`ToneRecipe` → the edge's tone synth, gated by `MASTER VOLUME`) and a single
streamed **menu music** MP3 loop (gated by `MUSIC VOLUME` beneath the master
gain), played and cross-faded on title enter/leave by the wasm audio edge
(`src/web/music.rs`). There are still no separate crowd or effects volumes and no
per-category audio mixing — just the master gain and the one music trim.

## Removed

Everything the old categorized settings system carried is gone, not hidden:
gameplay/audio/video/controls/accessibility categories; difficulty, game speed,
camera style; render/effects quality; UI scale, text size; high contrast, flash
intensity, color distinction; separate crowd/effects/menu-category volumes (a
single `MUSIC VOLUME` for the menu track is retained); mute-when-unfocused; the
apply/discard/reset flow; and control-profile selection and rebinding.
