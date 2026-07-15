/*
 * main.ts — the browser boot for Casino Games. Builds the registry and hands
 * the DOM to the shell. All entropy, URL parameters, DOM, and storage live in
 * the shell (the app's impure edge); everything below it is deterministic.
 */

import { bootShell } from "./application/shell.ts";
import { buildRegistry } from "./games/index.ts";

bootShell(buildRegistry());
