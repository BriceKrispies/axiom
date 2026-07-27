/*
 * server.ts — the harness host: ONE origin that serves the harness UI, the real
 * game, the real API, and the engine bundle.
 *
 * WHY ONE ORIGIN, AND WHY A PROXY. The zero-JS rung is a native
 * `<form method="POST">` navigation. That has no CORS story at all — no
 * preflight to grant, no header to add, nothing the page can opt into — so if
 * the page and `/api/pick` are not same-origin the baseline rung is simply
 * impossible, and the harness would be testing a game that cannot exist. The
 * harness therefore stands in front of the REAL `axiom-chest-server` and
 * forwards everything it does not own, instead of reimplementing static serving
 * or the pick endpoint. There is exactly one copy of the game and one copy of
 * the API, and the harness cannot drift from them because it does not contain
 * them.
 *
 * WHAT IT ADDS on the way through, and only to HTML:
 *
 *   1. an `@axiom/web-engine` import map, but ONLY for pages that do not ship
 *      one (the same rule `axiom-serve` applies). `resilient.html` ships its
 *      own; overriding it would silently run the game against an engine build
 *      the game did not choose.
 *   2. when `?deny=` is present, `caps-mask.js` plus the call that arms it, as
 *      the FIRST thing in `<head>`.
 *
 * WHY A QUERY AND NOT `srcdoc`. Both were considered; only one of them works.
 * `srcdoc` composition puts the game in an opaque origin, which breaks the
 * session cookie and the same-origin form POST — it would silently delete the
 * rung the harness exists to prove. Patching `iframe.contentWindow` before
 * assigning `.src` is not an option either: measured, the navigation installs a
 * fresh realm and the patch is gone (`injected:false`). A query the server
 * understands is the only mechanism that is guaranteed to run before the page's
 * first script AND leave the origin intact, which is why the toggles reboot the
 * iframe instead of applying live.
 *
 * Repo tooling: outside the engine dependency graph, the Coverage Law and the
 * Branchless Law.
 */

import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { extname, join, normalize, resolve, sep } from "node:path";
import { createServer, request as httpRequest, type IncomingMessage, type Server, type ServerResponse } from "node:http";

import { DENY_PARAM, HARNESS_PREFIX, importMapFor, injectIntoHead, maskPreamble, parseDeny } from "./inject.ts";

const MIME: Readonly<Record<string, string>> = {
  ".css": "text/css; charset=utf-8",
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".map": "application/json; charset=utf-8",
  ".png": "image/png",
  ".svg": "image/svg+xml",
};

const mimeFor = (path: string): string => MIME[extname(path).toLowerCase()] ?? "application/octet-stream";

/** Resolve `urlPath` under `root`, refusing anything that escapes it. */
const underRoot = async (root: string, urlPath: string): Promise<string | null> => {
  const decoded = ((): string | null => {
    try {
      return decodeURIComponent(urlPath);
    } catch {
      return null;
    }
  })();
  if (decoded === null) return null;
  const candidate = resolve(join(root, normalize(decoded)));
  const rootResolved = resolve(root);
  if (candidate !== rootResolved && !candidate.startsWith(rootResolved + sep)) return null;
  try {
    return (await stat(candidate)).isFile() ? candidate : null;
  } catch {
    return null;
  }
};

const sendFile = (response: ServerResponse, path: string): void => {
  response.writeHead(200, { "cache-control": "no-store", "content-type": mimeFor(path) });
  createReadStream(path).pipe(response);
};

const sendText = (response: ServerResponse, status: number, body: string, type = "text/plain; charset=utf-8"): void => {
  response.writeHead(status, { "cache-control": "no-store", "content-type": type });
  response.end(body);
};

export interface HarnessServerOptions {
  /** `tools/axiom-harness/web` — the harness's own UI. */
  readonly harnessRoot: string;
  /** `packages/axiom-web-engine/dist` — served at `/vendor/axiom-web-engine/`. */
  readonly engineDist: string;
  /** Where the real chest server is listening. Everything not owned above is
   * forwarded there, unmodified except for the HTML injection. */
  readonly upstream: { readonly host: string; readonly port: number };
}

/** Read a whole request body — small by construction (a nine-byte pick). */
const readBody = async (request: IncomingMessage): Promise<Buffer> => {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks);
};

/**
 * Hop-by-hop headers describe THIS connection, not the message, so forwarding
 * them is always wrong. It is also actively harmful here — the chest server streams files
 * with `transfer-encoding: chunked`, and copying that alongside the
 * `content-length` this proxy computes produces a response Chromium rejects
 * outright ("Content-Length can't be present with Transfer-Encoding"). That
 * surfaced as `page.route(...).fetch()` failing, i.e. as the CSP rung being
 * untestable — a harness bug masquerading as a browser limitation.
 */
const HOP_BY_HOP = ["connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade"];

const forwardable = (headers: Record<string, unknown>): Record<string, unknown> => {
  const out = { ...headers };
  HOP_BY_HOP.forEach((name) => delete out[name]);
  return out;
};

/**
 * Forward one request upstream. HTML comes back buffered so the preamble can be
 * inserted; everything else is piped, so images and the game's own modules cost
 * nothing extra.
 */
const proxy = async (
  options: HarnessServerOptions,
  request: IncomingMessage,
  response: ServerResponse,
  deny: readonly string[],
): Promise<void> => {
  const body = await readBody(request);
  const headers = forwardable(request.headers);
  delete headers.host;
  delete headers["content-length"];
  delete headers["accept-encoding"]; // buffered rewriting must not fight gzip

  await new Promise<void>((done) => {
    const upstream = httpRequest(
      {
        headers: { ...headers, "content-length": String(body.length), host: `${options.upstream.host}:${options.upstream.port}` },
        host: options.upstream.host,
        method: request.method,
        path: request.url,
        port: options.upstream.port,
      },
      (incoming) => {
        const type = String(incoming.headers["content-type"] ?? "");
        const outHeaders = forwardable(incoming.headers);
        if (!type.includes("text/html")) {
          response.writeHead(incoming.statusCode ?? 200, outHeaders);
          incoming.pipe(response).on("finish", done);
          return;
        }
        const parts: Buffer[] = [];
        incoming.on("data", (chunk: Buffer) => parts.push(chunk));
        incoming.on("end", () => {
          const source = Buffer.concat(parts).toString("utf8");
          const preamble = importMapFor(source) + (deny.length > 0 ? maskPreamble(deny) : "");
          const html = injectIntoHead(source, preamble);
          const out = Buffer.from(html, "utf8");
          delete outHeaders["content-length"];
          delete outHeaders["content-encoding"];
          response.writeHead(incoming.statusCode ?? 200, { ...outHeaders, "content-length": String(out.length) });
          response.end(out, () => done());
        });
      },
    );
    upstream.on("error", (error: Error) => {
      sendText(response, 502, `harness: upstream chest server unreachable — ${error.message}`);
      done();
    });
    upstream.end(body);
  });
};

/** Build (but do not listen on) the harness server. */
export const createHarnessServer = (options: HarnessServerOptions): Server =>
  createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://localhost");
    const deny = parseDeny(url.searchParams.get(DENY_PARAM));

    const serve = async (): Promise<void> => {
      if (url.pathname === HARNESS_PREFIX || url.pathname === `${HARNESS_PREFIX}/`) {
        const index = await underRoot(options.harnessRoot, "/index.html");
        if (index === null) {
          sendText(response, 500, "harness: web/index.html is missing");
          return;
        }
        sendFile(response, index);
        return;
      }
      if (url.pathname.startsWith(`${HARNESS_PREFIX}/`)) {
        const path = await underRoot(options.harnessRoot, url.pathname.slice(HARNESS_PREFIX.length));
        if (path === null) {
          sendText(response, 404, `harness: nothing at ${url.pathname}`);
          return;
        }
        sendFile(response, path);
        return;
      }
      const VENDOR = "/vendor/axiom-web-engine/";
      if (url.pathname.startsWith(VENDOR)) {
        const path = await underRoot(options.engineDist, url.pathname.slice(VENDOR.length - 1));
        if (path === null) {
          sendText(response, 404, `harness: the engine bundle has no ${url.pathname} — is packages/axiom-web-engine/dist built?`);
          return;
        }
        sendFile(response, path);
        return;
      }
      await proxy(options, request, response, deny);
    };

    serve().catch((error: unknown) => {
      sendText(response, 500, `harness: ${error instanceof Error ? error.message : String(error)}`);
    });
  });
