/*
 * gallery-layout.ts — the pure geometry of the Figure Lab gallery: given the
 * gallery's screen rect, an icon size, and the ordered entries, it produces the
 * section headers and the per-entry cell rects of a wrapping grid, in CONTENT
 * space (y = 0 at the top of the content, before scrolling). The screen applies
 * the scroll offset and culls; the 3D stage turns each cell rect into a world
 * placement. Keeping this pure means the gallery's reflow — column count, section
 * breaks, content height, resizeable icons — is unit-testable with no DOM.
 */

import type { CatalogEntry, SortMode } from "./catalog.ts";
import { sectionOf } from "./catalog.ts";

/** One figure's slot in the grid. `index` indexes the entries array. */
export interface GalleryCell {
  readonly index: number;
  readonly x: number;
  readonly y: number;
  readonly size: number;
}

/** A tribe/section band spanning the grid width. */
export interface GalleryHeader {
  readonly label: string;
  readonly y: number;
  readonly count: number;
}

export interface GalleryLayout {
  readonly columns: number;
  readonly cells: readonly GalleryCell[];
  readonly headers: readonly GalleryHeader[];
  /** Total content height in pixels (what the scroll range is derived from). */
  readonly contentHeight: number;
}

export const HEADER_H = 22;
/** Room under each icon for its name + stat line. */
export const CAPTION_H = 26;
const PAD = 8;
const GAP = 6;

export const MIN_ICON = 44;
export const MAX_ICON = 260;

export const clampIcon = (px: number): number => Math.max(MIN_ICON, Math.min(MAX_ICON, px));

/**
 * Lay the entries out as a wrapping grid inside `width`, breaking into labelled
 * sections when the sort is sectioned. Cell y is content-relative (add scroll).
 */
export const layoutGallery = (entries: readonly CatalogEntry[], width: number, icon: number, sort: SortMode): GalleryLayout => {
  const cellW = icon + GAP;
  const cellH = icon + CAPTION_H + GAP;
  const columns = Math.max(1, Math.floor((width - PAD * 2 + GAP) / cellW));
  const cells: GalleryCell[] = [];
  const headers: GalleryHeader[] = [];
  let y = PAD;
  let section: string | null = null;
  let col = 0;
  let sectionStart = 0;

  entries.forEach((entry, index) => {
    const s = sectionOf(entry, sort);
    if (s !== null && s !== section) {
      // Close the previous section's row, then open a new labelled band.
      y += col > 0 ? cellH : 0;
      col = 0;
      section = s;
      sectionStart = headers.length;
      headers.push({ label: s, y, count: 0 });
      y += HEADER_H;
    }
    cells.push({ index, x: PAD + col * cellW, y, size: icon });
    col += 1;
    if (col >= columns) {
      col = 0;
      y += cellH;
    }
    const open = headers[sectionStart];
    if (s !== null && open !== undefined) {
      headers[sectionStart] = { label: open.label, y: open.y, count: open.count + 1 };
    }
  });

  return { columns, cells, headers, contentHeight: y + (col > 0 ? cellH : 0) + PAD };
};
