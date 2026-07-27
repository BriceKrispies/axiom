/*
 * main.ts — the CLI entry point.
 *
 *   node tools/axiom-chest-server/src/main.ts [--port 8090] [--root <dir>]
 *
 * No build step: Node runs the TypeScript directly by stripping types, which is
 * the same mechanism `node --test "**\/*.test.ts"` already relies on across this
 * repo. That is not a convenience — it is what lets the server import the app's
 * real chance engine as source instead of a compiled copy that could lag behind.
 */

import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

import { createChestServer } from "./server.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = resolve(HERE, "..", "..", "..", "apps", "casino-games", "web");
const DEFAULT_PORT = 8090;

const flag = (argv: readonly string[], name: string): string | undefined => {
  const index = argv.indexOf(`--${name}`);
  return index >= 0 ? argv[index + 1] : undefined;
};

const argv = process.argv.slice(2);
const port = Number(flag(argv, "port") ?? process.env["PORT"] ?? DEFAULT_PORT);
const webRoot = resolve(flag(argv, "root") ?? DEFAULT_ROOT);

const server = createChestServer({ webRoot });
server.listen(port, () => {
  process.stdout.write(`axiom-chest-server: http://localhost:${port}/  (root ${webRoot})\n`);
});

const shutdown = (): void => {
  server.close(() => process.exit(0));
};
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
