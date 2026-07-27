/*
 * transport.test.ts — the fallback ladder, exercised through its real failure
 * modes.
 *
 * The headline case is the one feature detection gets wrong: a `fetch` that is
 * PRESENT and REJECTS, which is what a CSP with `connect-src 'none'` produces.
 * A `typeof fetch === "function"` gate passes that check and then strands the
 * player. Every test below drives the transport through the failure rather than
 * asking it what it supports.
 */

import assert from "node:assert/strict";
import test from "node:test";

import type { PickResponse } from "./contract.ts";
import { postInPlace } from "./transport.ts";

const RESPONSE: PickResponse = {
  board: [{ index: 0, reward: null }],
  chestCount: 9,
  kind: "pick",
  picked: 0,
  replay: false,
  reward: null,
  round: 1,
  seed: 7,
  targetWinRate: 0.44,
  winnerCount: 4,
  won: false,
};

const okFetch = (): unknown => async (): Promise<unknown> => ({
  json: async (): Promise<unknown> => RESPONSE,
  ok: true,
  status: 200,
});

/** An XHR double whose behaviour is scripted by the caller. */
const fakeXhr = (behave: (request: Record<string, unknown>) => void): unknown =>
  class {
    public onload: (() => void) | null = null;
    public onerror: (() => void) | null = null;
    public ontimeout: (() => void) | null = null;
    public timeout = 0;
    public status = 0;
    public responseText = "";
    public open(): void {}
    public setRequestHeader(): void {}
    public send(): void {
      behave(this as unknown as Record<string, unknown>);
    }
  };

test("fetch carries the pick when it works", async () => {
  const outcome = await postInPlace("/api/pick", { pick: 0 }, { fetchImpl: okFetch() });
  assert.equal(outcome.kind, "ok");
  assert.equal(outcome.kind === "ok" ? outcome.via : "", "fetch");
  assert.deepEqual(outcome.kind === "ok" ? outcome.body : null, RESPONSE);
});

test("a PRESENT BUT REJECTING fetch falls through to XHR", async () => {
  // The enterprise-proxy case: `fetch` exists, is callable, and refuses.
  const hostileFetch = (): Promise<never> => Promise.reject(new TypeError("Failed to fetch"));
  const outcome = await postInPlace("/api/pick", { pick: 3 }, {
    fetchImpl: hostileFetch,
    xhrCtor: fakeXhr((request) => {
      request["status"] = 200;
      request["responseText"] = JSON.stringify(RESPONSE);
      (request["onload"] as () => void)();
    }),
  });
  assert.equal(outcome.kind, "ok");
  assert.equal(outcome.kind === "ok" ? outcome.via : "", "xhr");
});

test("an absent fetch is just another failure, not a special case", async () => {
  // No `typeof` guard exists in the transport, so `undefined` throws a
  // TypeError and is recorded exactly like a refusal.
  const outcome = await postInPlace("/api/pick", { pick: 1 }, {
    fetchImpl: undefined,
    xhrCtor: fakeXhr((request) => {
      request["status"] = 200;
      request["responseText"] = JSON.stringify(RESPONSE);
      (request["onload"] as () => void)();
    }),
  });
  assert.equal(outcome.kind === "ok" ? outcome.via : "", "xhr");
});

test("a non-2xx answer is a failure, however cheerfully it arrived", async () => {
  const outcome = await postInPlace("/api/pick", { pick: 1 }, {
    fetchImpl: async (): Promise<unknown> => ({ json: async (): Promise<unknown> => ({}), ok: false, status: 502 }),
    xhrCtor: fakeXhr((request) => {
      request["status"] = 500;
      (request["onload"] as () => void)();
    }),
  });
  assert.equal(outcome.kind, "unavailable");
  assert.equal(outcome.kind === "unavailable" ? outcome.reasons.length : 0, 2);
  assert.match(outcome.kind === "unavailable" ? outcome.reasons.join(" ") : "", /502[\s\S]*500/);
});

test("when both transports fail the caller is told to use the form", async () => {
  const outcome = await postInPlace("/api/pick", { pick: 8 }, {
    fetchImpl: (): Promise<never> => Promise.reject(new TypeError("blocked by CSP")),
    xhrCtor: fakeXhr((request) => (request["onerror"] as () => void)()),
  });
  assert.equal(outcome.kind, "unavailable");
  const reasons = outcome.kind === "unavailable" ? outcome.reasons : [];
  assert.match(reasons[0] ?? "", /^fetch — TypeError: blocked by CSP/);
  assert.match(reasons[1] ?? "", /^xhr — Error: XMLHttpRequest failed/);
});

test("a hung XHR times out instead of stranding the player", async () => {
  const outcome = await postInPlace("/api/pick", { pick: 2 }, {
    fetchImpl: (): Promise<never> => Promise.reject(new Error("no")),
    timeoutMs: 25,
    // Never calls back — a proxy that accepts the connection and says nothing.
    xhrCtor: fakeXhr((request) => setTimeout(() => (request["ontimeout"] as () => void)(), 5)),
  });
  assert.equal(outcome.kind, "unavailable");
  assert.match(outcome.kind === "unavailable" ? (outcome.reasons[1] ?? "") : "", /timed out after 25ms/);
});

test("unreadable JSON is reported, not thrown at the caller", async () => {
  const outcome = await postInPlace("/api/pick", { pick: 2 }, {
    fetchImpl: (): Promise<never> => Promise.reject(new Error("no")),
    xhrCtor: fakeXhr((request) => {
      request["status"] = 200;
      request["responseText"] = "<html>proxy login page</html>";
      (request["onload"] as () => void)();
    }),
  });
  assert.equal(outcome.kind, "unavailable");
  assert.match(outcome.kind === "unavailable" ? (outcome.reasons[1] ?? "") : "", /unreadable response/);
});

test("postInPlace never throws — the caller only ever chooses between two renders", async () => {
  const outcome = await postInPlace("/api/pick", { pick: 0 }, { fetchImpl: null, xhrCtor: null });
  assert.equal(outcome.kind, "unavailable");
});
