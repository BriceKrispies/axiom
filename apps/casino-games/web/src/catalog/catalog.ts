/*
 * catalog.ts — the game catalog: one card per registered game (name, short
 * description, 2D/3D badge, interaction type, procedural thumbnail, Play and
 * Configure), plus the filter row. Renders entirely from the registry — the
 * registry is the single source of truth for what appears here.
 */

import type { CasinoGameDefinition, GameCategory } from "../chance-engine/registry/definition.ts";
import type { CasinoGameRegistry } from "../chance-engine/registry/registry.ts";
import { paintThumbnail } from "./thumbnails.ts";

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

export const buildCatalog = (
  registry: CasinoGameRegistry,
  filtersHost: HTMLElement,
  cardsHost: HTMLElement,
  handlers: CatalogHandlers,
): void => {
  let active: CatalogFilter = "all";

  const renderCards = (): void => {
    cardsHost.replaceChildren(
      ...registry
        .all()
        .filter((definition) => matches(definition, active))
        .map((definition) => cardOf(definition, handlers)),
    );
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

const cardOf = (definition: CasinoGameDefinition<unknown>, handlers: CatalogHandlers): HTMLElement => {
  const card = document.createElement("article");
  card.className = "game-card";

  const thumb = document.createElement("canvas");
  paintThumbnail(thumb, definition.thumbnail);
  card.append(thumb);

  const body = document.createElement("div");
  body.className = "body";

  const title = document.createElement("h3");
  title.textContent = definition.displayName;

  const badges = document.createElement("div");
  badges.className = "badges";
  const mode = document.createElement("span");
  mode.className = "badge mode";
  mode.textContent = definition.renderMode.toUpperCase();
  badges.append(mode);
  if (definition.machineInterior) {
    const machine = document.createElement("span");
    machine.className = "badge machine";
    machine.textContent = "machine";
    badges.append(machine);
  }
  const interaction = document.createElement("span");
  interaction.className = "badge";
  interaction.textContent = definition.interaction;
  badges.append(interaction);

  const description = document.createElement("p");
  description.textContent = definition.shortDescription;

  const actions = document.createElement("div");
  actions.className = "card-actions";
  const play = document.createElement("button");
  play.className = "play";
  play.textContent = "Play";
  play.addEventListener("click", () => handlers.onPlay(definition.id));
  const configure = document.createElement("button");
  configure.className = "configure";
  configure.textContent = "Configure";
  configure.addEventListener("click", () => handlers.onConfigure(definition.id));
  actions.append(play, configure);

  body.append(title, badges, description, actions);
  card.append(body);
  return card;
};
