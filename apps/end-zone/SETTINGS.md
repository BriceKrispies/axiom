# End Zone — settings

Every exposed setting maps to a real subsystem. The model is
`frontend/settings.rs` (`EndZoneSettings`): bounded enums and clamped steps,
valid by construction. The editor works on an explicit WORKING copy; only
APPLY commits (and requests persistence). BACK with unapplied changes raises
the app-styled discard dialog — never a browser alert.

## GAMEPLAY

| Setting | Values | What it really does |
|---|---|---|
| DIFFICULTY | ROOKIE / PRO / ALL-STAR | Default for new matches. A named `DifficultyProfile` scales the OPPONENT's defensive data: reaction delay, pursuit aggressiveness, tackle range (`launch.rs`). PRO is exactly the showcase default. No difficulty branches exist in AI code. |
| GAME SPEED | NORMAL / FAST / TURBO | Default for new matches. Whole sim steps per animation frame (1, 2/1 alternating, 2) — the fixed step itself never changes, so replays stay valid. |
| CAMERA | ARCADE / WIDE / CLOSE | Named `CameraTuning` profiles: follow distance/height, FOV, formation framing. |

## AUDIO

| Setting | Values | What it really does |
|---|---|---|
| MASTER VOLUME | 0–10 | Scales every menu tone (and all future audio) — the master gain factor. |
| MENU VOLUME | 0–10 | Interface tone level; menu gain = master × menu. |
| MUSIC VOLUME | 0–10 | **Reserved.** Typed + persisted; the engine's music playback arm is a stub, so nothing is audible yet. Labelled as such in the UI. |
| EFFECTS VOLUME | 0–10 | **Reserved** (same engine limitation — sample playback). |
| CROWD VOLUME | 0–10 | **Reserved** (same engine limitation). |
| MUTE WHEN UNFOCUSED | ON / OFF | Silences all tones while the tab is hidden (checked at the edge each frame). |

## VIDEO

| Setting | Values | What it really does |
|---|---|---|
| RENDER QUALITY | LOW / MEDIUM / HIGH | `RenderQuality::detail()` → contact-shadow caster tier + whether the fine marking mesh (one-yard ticks, hashes) renders. |
| EFFECTS INTENSITY | LOW / MEDIUM / HIGH | Named `JuiceTuning` profiles: dust particle counts, streaks, trail points, ring radii, squash amplitude, field wobble. |
| UI SCALE | SMALL / NORMAL / LARGE | Multiplies the whole interface layout (0.85 / 1.0 / 1.18); live preview while editing. |

## CONTROLS

Rebindable actions (keyboard + gamepad tokens, up to 3 per action):
navigation (up/down/left/right), CONFIRM, CANCEL, PAUSE, SNAP/THROW, plus
two RESERVED gameplay slots (secondary, switch player) carried for the
future game. Rebinding captures the next pressed key (Escape cancels, ~8 s
timeout); same-group conflicts are flagged in the row. RESTORE DEFAULT
CONTROLS resets the working copy. The emergency menu path (Enter / Escape /
arrows) always works, so no binding state can make the interface unusable.

## ACCESSIBILITY

| Setting | Values | What it really does |
|---|---|---|
| SCREEN SHAKE | OFF / LOW / FULL | Scales every camera impulse amplitude + FOV kick (`CameraTuning::shake_scale`, applied in the camera director). |
| REDUCED MOTION | ON / OFF | Replaces wipes/slides/zoom transitions with short fades, stills the background sweep/blink motion, and is honored by every screen's background. |
| HIGH CONTRAST | ON / OFF | Switches the computed theme palette (brighter text, solid panels, thicker borders, high-visibility focus ring). Proven by theme fingerprint test. |
| FLASH INTENSITY | OFF / LOW / FULL | Scales (or fully disables) the throw/catch screen flashes (`JuiceTuning::flash_scale`). |
| TEXT SIZE | NORMAL / LARGE | Scales interface text on top of the UI scale (1.25×). |
| COLOR DISTINCTION | STANDARD / ENHANCED | ENHANCED adds non-color team cues wherever team color is meaningful: abbreviations, emblem silhouettes, HOME/AWAY tags, patterned card edges. |

## Persistence

`FrontendProfile` (settings + bindings + last teams + last category +
control profile) is encoded as versioned `key=value` text
(`frontend/profile_codec.rs`, `v=1`) and stored behind the app-local
`ProfileStore` trait (`load / save / clear`). The wasm edge adapts it onto
`localStorage` (`web/storage.rs` — the only browser-storage touchpoint);
tests use `MemoryStore`. Decoding validates every field against explicit
keyword tables with per-field fallback; corrupt input can never panic or
produce an out-of-range value, and an equal team pair falls back to a legal
one. A missing/failed store logs through the kernel `LogSink` and the
frontend continues on defaults.
