/*
 * catalog.ts — the arcade floor: one authored machine cabinet per registered
 * game (marquee, screen, control plate), arranged in a scrollable row where
 * the focused machine steps forward and its neighbors dim. Renders entirely
 * from the registry — the registry is the single source of truth for what
 * appears here. Supports mouse (click to focus/click Play), keyboard (arrow
 * keys move focus, Enter/Space plays the focused machine on `#cards`), and
 * touch (native scroll-snap swipe, synced back into focus state).
 */

import type { CasinoGameDefinition, GameCategory } from "../chance-engine/registry/definition.ts";
import type { CasinoGameRegistry } from "../chance-engine/registry/registry.ts";
import { paintGlyphBadge, paintThumbnail } from "./thumbnails.ts";

export type CatalogFilter = "all" | "2d" | "3d" | GameCategory;

const FILTERS: readonly { readonly id: CatalogFilter; readonly label: string }[] = [
  { id: "all", label: "All games" },
  { id: "2d", label: "2D" },
  { id: "3d", label: "3D" },
  { id: "choice", label: "Choice" },
  { id: "machine", label: "Machine" },
  { id: "physical", label: "Physical" },
  { id: "reveal", label: "Reveal" },
];

const matches = (definition: CasinoGameDefinition<unknown>, filter: CatalogFilter): boolean => {
  switch (filter) {
    case "all":
      return true;
    case "2d":
    case "3d":
      return definition.renderMode === filter;
    default:
      return definition.categories.includes(filter);
  }
};

export interface CatalogHandlers {
  readonly onPlay: (gameId: string) => void;
  readonly onConfigure: (gameId: string) => void;
}

const machineOf = (
  definition: CasinoGameDefinition<unknown>,
  index: number,
  handlers: CatalogHandlers,
  onFocusRequest: (index: number) => void,
): HTMLElement => {
  const machine = document.createElement("article");
  machine.className = "cab-machine";
  machine.setAttribute("role", "option");
  machine.dataset["gameId"] = definition.id;

  const marquee = document.createElement("div");
  marquee.className = "cab-machine-marquee";
  const glyph = document.createElement("canvas");
  glyph.className = "cab-machine-glyph";
  paintGlyphBadge(glyph, definition.thumbnail);
  const name = document.createElement("h3");
  name.className = "cab-bitmap";
  name.textContent = definition.displayName;
  marquee.append(glyph, name);

  const screen = document.createElement("div");
  screen.className = "cab-machine-screen";
  const thumb = document.createElement("canvas");
  paintThumbnail(thumb, definition.thumbnail);
  const scanband = document.createElement("div");
  scanband.className = "cab-machine-scanband";
  screen.append(thumb, scanband);

  const plate = document.createElement("div");
  plate.className = "cab-machine-plate";

  const description = document.createElement("p");
  description.textContent = definition.shortDescription;

  const badges = document.createElement("div");
  badges.className = "cab-machine-badges";
  const mode = document.createElement("span");
  mode.className = "cab-badge cab-badge--mode";
  mode.textContent = definition.renderMode.toUpperCase();
  const interaction = document.createElement("span");
  interaction.className = "cab-badge";
  interaction.textContent = definition.interaction;
  badges.append(mode, interaction);
  if (definition.machineInterior) {
    const machineBadge = document.createElement("span");
    machineBadge.className = "cab-badge";
    machineBadge.textContent = "machine";
    badges.append(machineBadge);
  }

  const actions = document.createElement("div");
  actions.className = "cab-machine-actions";
  const play = document.createElement("button");
  play.className = "cab-btn cab-btn--start";
  play.textContent = "Play";
  play.addEventListener("click", (event) => {
    event.stopPropagation();
    handlers.onPlay(definition.id);
  });
  const configure = document.createElement("button");
  configure.className = "cab-btn cab-btn--service";
  configure.textContent = "Set";
  configure.title = "Configure this machine";
  configure.addEventListener("click", (event) => {
    event.stopPropagation();
    handlers.onConfigure(definition.id);
  });
  actions.append(play, configure);

  plate.append(description, badges, actions);
  machine.append(marquee, screen, plate);

  machine.addEventListener("click", () => onFocusRequest(index));

  return machine;
};

export const buildCatalog = (
  registry: CasinoGameRegistry,
  filtersHost: HTMLElement,
  cardsHost: HTMLElement,
  handlers: CatalogHandlers,
): void => {
  let active: CatalogFilter = "all";
  let filtered: readonly CasinoGameDefinition<unknown>[] = [];
  let focusedIndex = 0;
  let scrollRaf = 0;

  const prevBtn = document.getElementById("floor-prev");
  const nextBtn = document.getElementById("floor-next");

  const applyFocusVisual = (): void => {
    Array.from(cardsHost.children).forEach((child, i) => {
      const el = child as HTMLElement;
      const isFocused = i === focusedIndex;
      el.dataset["focused"] = String(isFocused);
      el.setAttribute("aria-selected", String(isFocused));
    });
  };

  const setFocused = (index: number, scroll: boolean): void => {
    if (filtered.length === 0) {
      return;
    }
    focusedIndex = Math.min(Math.max(index, 0), filtered.length - 1);
    applyFocusVisual();
    if (scroll) {
      const el = cardsHost.children[focusedIndex] as HTMLElement | undefined;
      el?.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "center" });
    }
  };

  const nearestToScrollCenter = (): number => {
    const center = cardsHost.scrollLeft + cardsHost.clientWidth / 2;
    let best = 0;
    let bestDist = Number.POSITIVE_INFINITY;
    Array.from(cardsHost.children).forEach((child, i) => {
      const el = child as HTMLElement;
      const mid = el.offsetLeft + el.clientWidth / 2;
      const dist = Math.abs(mid - center);
      if (dist < bestDist) {
        bestDist = dist;
        best = i;
      }
    });
    return best;
  };

  cardsHost.addEventListener("scroll", () => {
    if (scrollRaf !== 0) {
      return;
    }
    scrollRaf = requestAnimationFrame(() => {
      scrollRaf = 0;
      setFocused(nearestToScrollCenter(), false);
    });
  });

  cardsHost.addEventListener("keydown", (event) => {
    if (event.key === "ArrowRight") {
      event.preventDefault();
      setFocused(focusedIndex + 1, true);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      setFocused(focusedIndex - 1, true);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      const definition = filtered[focusedIndex];
      if (definition !== undefined) {
        handlers.onPlay(definition.id);
      }
    }
  });

  prevBtn?.addEventListener("click", () => setFocused(focusedIndex - 1, true));
  nextBtn?.addEventListener("click", () => setFocused(focusedIndex + 1, true));

  const renderCards = (): void => {
    filtered = registry.all().filter((definition) => matches(definition, active));
    cardsHost.replaceChildren(
      ...filtered.map((definition, i) => machineOf(definition, i, handlers, (index) => setFocused(index, true))),
    );
    focusedIndex = 0;
    applyFocusVisual();
  };

  filtersHost.replaceChildren(
    ...FILTERS.map(({ id, label }) => {
      const button = document.createElement("button");
      button.textContent = label;
      button.setAttribute("aria-pressed", String(id === active));
      button.addEventListener("click", () => {
        active = id;
        for (const sibling of filtersHost.querySelectorAll("button")) {
          sibling.setAttribute("aria-pressed", String(sibling === button));
        }
        renderCards();
      });
      return button;
    }),
  );
  renderCards();
};
