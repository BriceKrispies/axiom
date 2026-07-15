/*
 * registry.ts — the single source of truth for which games exist. The catalog
 * renders from it, the game screen mounts through it, and the workbench pulls
 * default configurations from it. Registration rejects duplicate ids and
 * default configurations that fail validation — a bad definition cannot be
 * registered quietly.
 */

import type { CasinoGameConfig } from "../configuration/schema.ts";
import { validateConfig } from "../configuration/validation.ts";
import type { CasinoGameDefinition } from "./definition.ts";

export class CasinoGameRegistry {
  readonly #definitions = new Map<string, CasinoGameDefinition<unknown>>();

  public register<TSpec>(definition: CasinoGameDefinition<TSpec>): void {
    if (this.#definitions.has(definition.id)) {
      throw new Error(`duplicate game id "${definition.id}"`);
    }
    const config = definition.defaultConfig() as CasinoGameConfig<unknown>;
    const issues = [...validateConfig(config), ...definition.validateSpec(config.gameSpecific as TSpec)];
    if (issues.length > 0) {
      const detail = issues.map((i) => `${i.path}: ${i.message}`).join("; ");
      throw new Error(`default config for "${definition.id}" is invalid — ${detail}`);
    }
    if (config.gameId !== definition.id) {
      throw new Error(`default config gameId "${config.gameId}" does not match definition id "${definition.id}"`);
    }
    this.#definitions.set(definition.id, definition as CasinoGameDefinition<unknown>);
  }

  public get(id: string): CasinoGameDefinition<unknown> {
    const definition = this.#definitions.get(id);
    if (definition === undefined) {
      throw new Error(`unknown game id "${id}"`);
    }
    return definition;
  }

  public has(id: string): boolean {
    return this.#definitions.has(id);
  }

  public all(): readonly CasinoGameDefinition<unknown>[] {
    return [...this.#definitions.values()];
  }
}
