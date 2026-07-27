/*
 * main.ts — the CLI. One command brings up the whole hostile-environment host:
 *
 *   node tools/axiom-harness/src/main.ts [--port 8091]
 *
 * It starts the REAL `axiom-chest-server` on an ephemeral loopback port and
 * puts the harness in front of it, so there is one thing to run and no chance of
 * testing against a stale or differently-configured API. Nothing else in the
 * repo has to be running first.
 *
 *   --port <n>      the harness port (default 8091)
 *   --api-port <n>  force the upstream chest server's port instead of an
 *                   ephemeral one — useful when you want to curl it directly
 *   --api <url>     do NOT start a chest server; forward to one already running
 *
 * No build step: Node runs the TypeScript directly by stripping types, the same
 * mechanism `axiom-chest-server` already relies on. That is load-bearing rather
 * than convenient — it is what lets the harness import the chest server's real
 * source instead of a compiled copy that could lag behind it.
 *
 * Repo tooling: outside the engine dependency graph and its laws.
 */

import type { AddressInfo } from "node:net";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { createChestServer } from "../../axiom-chest-server/src/server.ts";
import { createHarnessServer } from "./server.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..", "..");
const GAME_ROOT = resolve(REPO, "apps", "casino-games", "web");
const ENGINE_DIST = resolve(REPO, "packages", "axiom-web-engine", "dist");
const HARNESS_ROOT = resolve(HERE, "..", "web");
const DEFAULT_PORT = 8091;

const flag = (argv: readonly string[], name: string): string | undefined => {
  const index = argv.indexOf(`--${name}`);
  return index >= 0 ? argv[index + 1] : undefined;
};

const argv = process.argv.slice(2);
const port = Number(flag(argv, "port") ?? process.env["AXIOM_HARNESS_PORT"] ?? DEFAULT_PORT);
const externalApi = flag(argv, "api");

/** Bring up the upstream: either the caller's, or our own on a port we pick. */
const upstream = async (): Promise<{ host: string; port: number; stop: () => void }> => {
  if (externalApi !== undefined) {
    const url = new URL(externalApi);
    return { host: url.hostname, port: Number(url.port || 80), stop: (): void => undefined };
  }
  const chest = createChestServer({ webRoot: GAME_ROOT });
  const wanted = Number(flag(argv, "api-port") ?? 0);
  await new Promise<void>((ready) => chest.listen(wanted, "127.0.0.1", ready));
  const address = chest.address() as AddressInfo;
  return { host: "127.0.0.1", port: address.port, stop: (): void => chest.close() };
};

const api = await upstream();
const harness = createHarnessServer({ engineDist: ENGINE_DIST, harnessRoot: HARNESS_ROOT, upstream: api });

harness.listen(port, () => {
  process.stdout.write(
    `axiom-harness: http://localhost:${port}/__harness/  (game + api proxied from 127.0.0.1:${api.port})\n` +
      `axiom-harness: the game itself is at http://localhost:${port}/resilient.html\n`,
  );
});

const shutdown = (): void => {
  api.stop();
  harness.close(() => process.exit(0));
};
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
