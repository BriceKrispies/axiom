/*
 * The covered reconciliation actions the hot runtime enacts once `diff.ts` has
 * decided WHAT changed. Kept out of the exempt `hot-runtime.ts` wiring so the actual
 * world mutation is unit-tested against a fake `NativeBridge` (100% covered), not
 * left to the browser-only path.
 *
 * `migrateComponents` is the byte-level component migration `soft_app_reload` runs:
 * because the engine stays alive, a component version bump rewrites the LIVE world in
 * place — for each migrated component, read every carrier's RAW prior-layout bytes,
 * run the author's migrator, and write the new bytes back. It never touches the
 * snapshot; the snapshot is only the hot runtime's transactional rollback checkpoint.
 * Branchless: an `each` over the components, `whenPresent` for the optional migrator
 * (a component with no migrator is `unmigratable` and never reaches here), and an
 * `each` over the carriers `worldQuery` returns.
 */

import { each, whenPresent } from "./control-flow.ts";
import type { ComponentDef } from "./manifest.ts";
import type { NativeBridge } from "./native-bridge.ts";

/** Rewrite the live component bytes for each migrated component: read raw → migrate → write raw. */
export const migrateComponents = (bridge: NativeBridge, migrated: readonly ComponentDef[]): void => {
  each(migrated, (component): void => {
    whenPresent(component.migrate, (migrate): void => {
      each(bridge.worldQuery([component.id]), (entity): void => {
        bridge.worldRawSet(entity, component.id, migrate(bridge.worldRawGet(entity, component.id)));
      });
    });
  });
};
