// Workspace shell entry point.
//
// Boring and explicit: build the typed initial state, then mount the panels into
// the single app root. `mountWorkspace` owns the dispatch loop — every button in
// every panel dispatches a typed WorkspaceEvent, which the reducer applies to
// produce a new state, and the shell re-renders. No runtime is attached; this is
// a placeholder-only developer surface.

import { mountCartridgeHost } from "./cartridge_loader";
import { mountWorkspace } from "./dom_mount";
import { initialWorkspaceState } from "./workspace_state";

function boot(): void {
  const root = document.querySelector<HTMLElement>("#workspace-root");
  if (root === null) {
    return;
  }
  mountWorkspace(root, initialWorkspaceState());

  // The runtime console: mounted ONCE outside `#workspace-root` (which the
  // reducer clears on every dispatch), so a loaded cartridge keeps running. It
  // reads the games manifest and hosts the selected `games/` cartridge live.
  const console = document.querySelector<HTMLElement>("#cartridge-host");
  if (console !== null) {
    void mountCartridgeHost(console, "./games-manifest.json");
  }
}

boot();
