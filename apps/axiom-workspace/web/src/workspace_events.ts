// Explicit, typed workspace events + the single reducer.
//
// Every state change in the shell flows through `applyWorkspaceEvent`. There is
// no ad hoc mutation anywhere else. Events are a discriminated union keyed on a
// `type` string tag; the reducer switches exhaustively over that tag and returns
// a NEW `WorkspaceBrowserState` (clone + replace the relevant slice).
//
// This is a SHELL reducer with NO real runtime attached. The only genuine control
// is the workflow-mode switcher; the panels are otherwise static, data-driven
// views that stay empty until a real runtime, project, or asset source is wired
// in. There are no placeholder-data-producing events. No `any` anywhere.

import type { WorkspaceBrowserState, WorkspaceMode } from "./workspace_state";

export interface WorkspaceModeSetEvent {
  readonly type: "workspace.mode.set";
  readonly mode: WorkspaceMode;
}

export type WorkspaceEvent = WorkspaceModeSetEvent;

// The one dispatch shape threaded through the shell. No globals.
export type Dispatch = (event: WorkspaceEvent) => void;

function setMode(
  state: WorkspaceBrowserState,
  event: WorkspaceModeSetEvent,
): WorkspaceBrowserState {
  return {
    ...state,
    mode: event.mode,
    playControls: { ...state.playControls, mode: event.mode },
  };
}

export function applyWorkspaceEvent(
  state: WorkspaceBrowserState,
  event: WorkspaceEvent,
): WorkspaceBrowserState {
  // The shell has exactly one event type today; the switch keeps room for more.
  switch (event.type) {
    case "workspace.mode.set":
      return setMode(state, event);
  }
}
