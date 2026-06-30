/*
 * The presentation asset loaders (SPEC-04 §10): the free `loadTexture` / `loadFont`
 * authoring functions. Like the free `sound` surface, they are not scoped to a
 * `Sim` or `Scene`, so they read the installed `HostBridge` at call time
 * (`boundHost()`); before `bindNative` they return the inert host's neutral value.
 *
 * Determinism class — presentation/boundary. A texture/font is fetched and decoded
 * in the app (the host arm), never on the sim tick; the handle is stable for the
 * session. The author calls `loadTexture` once (e.g. in `preload`/`create`) and
 * draws the returned handle every frame through `Frame.sprite`. Until the pixels
 * resolve app-side, a draw naming the handle simply paints nothing — no error.
 */

import type { FontSpec, TextureId } from "./vocabulary.ts";
import { boundHost } from "./host-binding.ts";

/**
 * Register `url` as a texture and return its stable handle immediately (the pixels
 * resolve asynchronously in the app). The same url always returns the same handle.
 */
export const loadTexture = (url: string): TextureId => boundHost().loadTexture(url);

/**
 * Register `url` as a font and return its `FontSpec`. Tier-0 ships one built-in
 * monospace family, so the spec's `family` is the built-in and `size` is the
 * default the author overrides per `Frame.text` call.
 */
export const loadFont = (url: string): FontSpec => boundHost().loadFont(url);
