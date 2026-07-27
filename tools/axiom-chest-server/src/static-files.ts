/*
 * static-files.ts — serve `apps/casino-games/web/` from the SAME origin as the
 * API.
 *
 * One origin is the whole reason this server exists. A native
 * `<form method="POST">` navigation has no CORS story at all — there is no
 * preflight to grant, no header to add, and no way to opt in from the page. If
 * the page and the endpoint are not same-origin, the zero-JS tier is simply
 * impossible. So the static page and `/api/pick` are served by one process on
 * one port, and the enhanced tiers post to a relative URL against that same
 * origin. The locked-down browser we are targeting is not going to be persuaded
 * to do anything more exotic.
 *
 * Path handling is deliberately strict: the URL is decoded, resolved against
 * the root, and rejected unless it is still INSIDE the root. `..` never escapes.
 */

import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { extname, join, normalize, resolve, sep } from "node:path";
import type { ServerResponse } from "node:http";

const MIME: Readonly<Record<string, string>> = {
  ".css": "text/css; charset=utf-8",
  ".html": "text/html; charset=utf-8",
  ".ico": "image/x-icon",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".map": "application/json; charset=utf-8",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".webp": "image/webp",
  ".woff2": "font/woff2",
};

/** Resolve a URL path to a real file under `root`, or null if it escapes or
 * does not exist. */
export const resolveStaticPath = async (root: string, urlPath: string): Promise<string | null> => {
  const decoded = ((): string | null => {
    try {
      return decodeURIComponent(urlPath);
    } catch {
      return null;
    }
  })();
  if (decoded === null) return null;

  const requested = decoded === "/" ? "/resilient.html" : decoded;
  const candidate = resolve(join(root, normalize(requested)));
  const rootResolved = resolve(root);
  const inside = candidate === rootResolved || candidate.startsWith(rootResolved + sep);
  if (!inside) return null;

  try {
    const info = await stat(candidate);
    return info.isFile() ? candidate : null;
  } catch {
    return null;
  }
};

export const serveFile = (response: ServerResponse, path: string): void => {
  response.writeHead(200, {
    "cache-control": "no-store",
    "content-type": MIME[extname(path).toLowerCase()] ?? "application/octet-stream",
  });
  createReadStream(path).pipe(response);
};
