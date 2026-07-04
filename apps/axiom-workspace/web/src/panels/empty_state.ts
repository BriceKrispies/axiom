// Shared empty-state row for panels with no data yet.
//
// The workspace shell attaches no real runtime, so every data panel starts empty
// (no fake/placeholder rows). When a slice has no items, panels show this instead
// of a blank container, so "nothing here yet" reads as intentional.

export function renderEmpty(label: string): HTMLElement {
  const p = document.createElement("p");
  p.className = "ws-empty-state";
  p.textContent = label;
  return p;
}
