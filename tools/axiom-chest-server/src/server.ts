/*
 * server.ts — the router: one origin, two endpoints, four representations.
 *
 * The entire contract of this file is that `/api/pick` behaves IDENTICALLY
 * however it was reached. A urlencoded form navigation and a JSON `fetch` walk
 * the same code from `parseFields` through `resolvePick`; only the last step —
 * `renderResultPage` vs. `JSON.stringify` — differs. There is no "no-JS branch"
 * of the game logic to rot, which is what makes the baseline tier trustworthy:
 * testing it tests the thing that ships.
 *
 * COOKIE. One `axiom_chest` cookie carries the session id. It is `HttpOnly`
 * because the page never reads it — `fetch` and a native form navigation both
 * send it automatically on a same-origin request — and `SameSite=Lax` so a form
 * POST from our own page is allowed while a cross-site one is not.
 *
 * NO-CACHE. Every dynamic response is `no-store`. A result page that came back
 * from the bfcache after a "play again" would show a stale board, and the
 * baseline tier has no script to correct it.
 */

import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";

import { NEW_ROUND_ENDPOINT, PICK_ENDPOINT, PICK_FIELD, parsePick } from "../../../apps/casino-games/web/src/resilient/contract.ts";
import type { ErrorResponse } from "../../../apps/casino-games/web/src/resilient/contract.ts";
import { CHEST_COUNT } from "../../../apps/casino-games/web/src/chest-round/round.ts";
import { negotiate, parseFields, type Representation } from "./negotiate.ts";
import { renderErrorPage, renderResultPage } from "./result-page.ts";
import { createSessionStore, describeRound, resolvePick, type SessionStore } from "./sessions.ts";
import { resolveStaticPath, serveFile } from "./static-files.ts";

const COOKIE_NAME = "axiom_chest";
/** Bodies are nine bytes of "pick=4"; anything large is not a chest pick. */
const MAX_BODY_BYTES = 4096;

export const readCookie = (header: string | undefined, name: string): string | null => {
  const found = (header ?? "")
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith(`${name}=`));
  return found === undefined ? null : decodeURIComponent(found.slice(name.length + 1));
};

const readBody = async (request: IncomingMessage): Promise<string> => {
  const chunks: Buffer[] = [];
  let total = 0;
  for await (const chunk of request) {
    const buffer = chunk as Buffer;
    total += buffer.length;
    if (total > MAX_BODY_BYTES) break;
    chunks.push(buffer);
  }
  return Buffer.concat(chunks).toString("utf8");
};

const sendJson = (response: ServerResponse, status: number, payload: unknown, cookie: string | null): void => {
  response.writeHead(status, {
    "cache-control": "no-store",
    "content-type": "application/json; charset=utf-8",
    ...(cookie === null ? {} : { "set-cookie": cookie }),
  });
  response.end(JSON.stringify(payload));
};

const sendHtml = (response: ServerResponse, status: number, html: string, cookie: string | null): void => {
  response.writeHead(status, {
    "cache-control": "no-store",
    "content-type": "text/html; charset=utf-8",
    ...(cookie === null ? {} : { "set-cookie": cookie }),
  });
  response.end(html);
};

const sessionCookie = (id: string): string =>
  `${COOKIE_NAME}=${encodeURIComponent(id)}; Path=/; SameSite=Lax; HttpOnly; Max-Age=7200`;

const refuse = (response: ServerResponse, want: Representation, status: number, message: string, cookie: string | null): void => {
  if (want === "html") {
    sendHtml(response, status, renderErrorPage(message), cookie);
    return;
  }
  const payload: ErrorResponse = { kind: "error", message };
  sendJson(response, status, payload, cookie);
};

export interface ChestServerOptions {
  /** Directory served as the site root — `apps/casino-games/web`. */
  readonly webRoot: string;
  readonly store?: SessionStore;
}

/** Build (but do not listen on) the HTTP server. */
export const createChestServer = (options: ChestServerOptions): Server => {
  const store = options.store ?? createSessionStore();

  return createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://localhost");
    const want = negotiate(request.headers.accept);
    const method = (request.method ?? "GET").toUpperCase();

    const handlePost = async (): Promise<void> => {
      const session = store.acquire(readCookie(request.headers.cookie, COOKIE_NAME));
      const cookie = sessionCookie(session.id);
      const fields = parseFields(request.headers["content-type"], await readBody(request));

      if (url.pathname === NEW_ROUND_ENDPOINT) {
        store.nextRound(session);
        // The baseline has no script to re-render, so send it back to the board
        // as a GET. 303 is the POST-redirect-GET status: it turns the reload of
        // a result page into a harmless navigation instead of a re-POST.
        if (want === "html") {
          response.writeHead(303, { "cache-control": "no-store", location: "/resilient.html", "set-cookie": cookie });
          response.end();
          return;
        }
        sendJson(response, 200, describeRound(session), cookie);
        return;
      }

      const picked = parsePick(fields[PICK_FIELD], CHEST_COUNT);
      if (picked === null) {
        refuse(response, want, 400, `Pick a chest from 1 to ${CHEST_COUNT}.`, cookie);
        return;
      }
      const result = resolvePick(session, picked);
      if (want === "html") {
        sendHtml(response, 200, renderResultPage(result), cookie);
        return;
      }
      sendJson(response, 200, result, cookie);
    };

    const handleGet = async (): Promise<void> => {
      const path = await resolveStaticPath(options.webRoot, url.pathname);
      if (path === null) {
        refuse(response, want, 404, `Nothing is served at ${url.pathname}.`, null);
        return;
      }
      serveFile(response, path);
    };

    const api = url.pathname === PICK_ENDPOINT || url.pathname === NEW_ROUND_ENDPOINT;
    const readOnly = method === "GET" || method === "HEAD";
    if (api && method !== "POST") {
      refuse(response, want, 405, "That endpoint only accepts POST.", null);
      return;
    }
    if (!api && !readOnly) {
      refuse(response, want, 405, "The static site only answers GET.", null);
      return;
    }

    const work = api ? handlePost() : handleGet();
    work.catch((error: unknown) => {
      refuse(response, want, 500, error instanceof Error ? error.message : "Unknown failure.", null);
    });
  });
};
