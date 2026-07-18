# End Zone — settings

The settings screen exposes exactly three settings, plus `BACK`. Changes apply
immediately — there is no working/committed copy, no APPLY or DISCARD, no reset
dialog, and no categories. The typed model is `EndZoneSettings`
(`src/frontend/settings.rs`); it is the entire persisted shape (`SETTINGS.md`'s
three keys, versioned — see `src/frontend/persistence.rs`).

| Setting | Values | Effect |
|---|---|---|
| `MASTER VOLUME` | `0`–`10` (normalized) | Scales all current audio output through the master gain the menu-tone path reads. |
| `SCREEN SHAKE` | `OFF` / `LOW` / `FULL` | Scales the actual gameplay camera impulses (`OFF` is exactly zero shake). |
| `REDUCED MOTION` | `OFF` / `ON` | Suppresses large menu sweeps and nonessential presentation motion (flash juice, field wobble) while preserving gameplay clarity. |

Each setting drives real behavior:

- **Master volume** flows to the platform edge as the gain every procedural menu
  tone plays at (`FrontendApp::menu_tone_gain`).
- **Screen shake** flows into `RunConfig` and then `launch::camera_tuning`,
  which sets the camera director's `shake_scale`. `OFF` produces exactly zero
  impulse amplitude; `LOW` halves it.
- **Reduced motion** sets the theme's motion flag (the frontend background stops
  its continuous sweep and transitions collapse to short fades) and, in
  gameplay, zeroes the nonessential flash juice and damps field wobble
  (`launch::juice_tuning`).

## Audio capability note

Only the master gain and the procedural menu tones are audible today. The
engine's sample/music playback arm is not wired for this app, so there is a
single typed `MASTER VOLUME` and the existing audio-intent boundary
(`AudioIntent` → `ToneRecipe` → the edge's tone synth) — no separate music,
crowd, or effects volumes, and no app-local browser audio workaround.

## Removed

Everything the old categorized settings system carried is gone, not hidden:
gameplay/audio/video/controls/accessibility categories; difficulty, game speed,
camera style; render/effects quality; UI scale, text size; high contrast, flash
intensity, color distinction; separate music/crowd/effects/menu volumes;
mute-when-unfocused; the apply/discard/reset flow; and control-profile
selection and rebinding.
