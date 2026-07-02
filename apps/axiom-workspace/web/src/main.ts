// Workspace shell entry point.
//
// Boring and explicit: build the typed initial state, then mount the panels into
// the single app root. `mountWorkspace` owns the dispatch loop — every button in
// every panel dispatches a typed WorkspaceEvent, which the reducer applies to
// produce a new state, and the shell re-renders. No runtime is attached; this is
// a placeholder-only developer surface.

import { mountWorkspace } from "./dom_mount";
import { initialWorkspaceState } from "./workspace_state";

function boot(): void {
  const root = document.querySelector<HTMLElement>("#workspace-root");
  if (root === null) {
    return;
  }
  mountWorkspace(root, initialWorkspaceState());
}

boot();
