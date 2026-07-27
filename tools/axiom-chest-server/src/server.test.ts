/*
 * server.test.ts — the whole thing, over a real socket.
 *
 * These tests boot the actual server on an ephemeral port and speak HTTP to it,
 * because the claim being made is about HTTP: that a urlencoded form POST and a
 * JSON POST reach the SAME decision and differ only in representation. A test
 * that called the handler directly would be testing a shape, not that claim.
 *
 * The urlencoded case is written the way a browser writes it — a `pick=N` body
 * and an `Accept` header that ranks `text/html` first — so the no-JavaScript
 * tier is exercised exactly as it ships, not approximated.
 */

import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import type { AddressInfo } from "node:net";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import type { PickResponse } from "../../../apps/casino-games/web/src/resilient/contract.ts";
import { createChestServer } from "./server.ts";
import { createSessionStore, resolvePick, describeRound, TARGET_WIN_RATE } from "./sessions.ts";
import { readCookie } from "./server.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB_ROOT = resolve(HERE, "..", "..", "..", "apps", "casino-games", "web");

const BROWSER_ACCEPT = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

let origin = "";
const server = createChestServer({ webRoot: WEB_ROOT });

before(async () => {
  await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
  const address = server.address() as AddressInfo;
  origin = `http://127.0.0.1:${address.port}`;
});

after(async () => {
  await new Promise<void>((done) => server.close(() => done()));
});

/** Post as a browser navigating a form: urlencoded body, HTML-first Accept. */
const postForm = (body: string, cookie?: string): Promise<Response> =>
  fetch(`${origin}/api/pick`, {
    body,
    headers: {
      accept: BROWSER_ACCEPT,
      "content-type": "application/x-www-form-urlencoded",
      ...(cookie === undefined ? {} : { cookie }),
    },
    method: "POST",
    redirect: "manual",
  });

/** Post as the enhanced tier does. */
const postJson = (body: unknown, cookie?: string): Promise<Response> =>
  fetch(`${origin}/api/pick`, {
    body: JSON.stringify(body),
    headers: {
      accept: "application/json",
      "content-type": "application/json",
      ...(cookie === undefined ? {} : { cookie }),
    },
    method: "POST",
  });

const sessionCookieOf = (response: Response): string => {
  const raw = response.headers.get("set-cookie") ?? "";
  const id = readCookie(raw.split(";")[0], "axiom_chest");
  assert.ok(id !== null, "the server must issue a session cookie");
  return `axiom_chest=${encodeURIComponent(id)}`;
};

test("the baseline: a urlencoded form POST returns a whole HTML result page", async () => {
  const response = await postForm("pick=4");
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type") ?? "", /^text\/html/);
  const html = await response.text();
  assert.match(html, /^<!doctype html>/);
  assert.match(html, /<h1>Treasure Chest Pick<\/h1>/);
  assert.match(html, /You (won!|opened)/);
  assert.match(html, /chest 5/);
  // The way back is a real form with a real submit control — no script needed.
  assert.match(html, /<form class="resilient-again" method="POST" action="\/api\/new">/);
  assert.match(html, /<button class="resilient-submit" type="submit">/);
  // ...and the whole board is disclosed as a table, readable with no CSS at all.
  assert.match(html, /<table class="resilient-board">/);
  assert.equal((html.match(/<th scope="row">Chest /g) ?? []).length, 9);
});

test("the enhanced tier: a JSON POST returns the same decision as JSON", async () => {
  const response = await postJson({ pick: 4 });
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type") ?? "", /^application\/json/);
  const body = (await response.json()) as PickResponse;
  assert.equal(body.kind, "pick");
  assert.equal(body.picked, 4);
  assert.equal(body.chestCount, 9);
  assert.equal(body.board.length, 9);
  assert.equal(body.targetWinRate, TARGET_WIN_RATE);
  assert.equal(body.won, body.reward !== null);
  assert.equal(body.board.filter((chest) => chest.reward !== null).length, body.winnerCount);
});

test("both encodings are ONE code path — same session, same answer", async () => {
  // Pick via the form, then re-post the same session as JSON. If the two paths
  // were separate implementations, this is where they would drift.
  const formResponse = await postForm("pick=6");
  const cookie = sessionCookieOf(formResponse);
  const html = await formResponse.text();

  const jsonResponse = await postJson({ pick: 6 }, cookie);
  const body = (await jsonResponse.json()) as PickResponse;

  assert.equal(body.picked, 6);
  assert.equal(body.replay, true, "the second POST must replay, not reroll");
  assert.equal(html.includes("You won!"), body.won);
  body.board.forEach((chest) => {
    const held = chest.reward === null ? "empty" : chest.reward.rewardLabel;
    assert.ok(html.includes(held), `the HTML page omitted chest ${chest.index + 1}'s "${held}"`);
  });
});

test("a repeat POST replays the recorded pick — a refresh cannot reroll", async () => {
  const first = await postJson({ pick: 1 });
  const cookie = sessionCookieOf(first);
  const one = (await first.json()) as PickResponse;

  // Even naming a DIFFERENT chest: the round is already committed.
  const second = await postJson({ pick: 7 }, cookie);
  const two = (await second.json()) as PickResponse;

  assert.equal(two.picked, 1);
  assert.equal(two.replay, true);
  assert.equal(two.won, one.won);
  assert.deepEqual(two.board, one.board);
});

test("a new round deals a fresh board and un-commits the pick", async () => {
  const first = await postJson({ pick: 0 });
  const cookie = sessionCookieOf(first);
  const before_ = (await first.json()) as PickResponse;

  const dealt = await fetch(`${origin}/api/new`, {
    headers: { accept: "application/json", cookie },
    method: "POST",
  });
  const round = (await dealt.json()) as { round: number; seed: number };
  assert.equal(round.round, before_.round + 1);
  assert.equal(round.seed, before_.seed, "the seed is drawn once per session, not per round");

  const next = (await (await postJson({ pick: 0 }, cookie)).json()) as PickResponse;
  assert.equal(next.replay, false);
  assert.equal(next.round, before_.round + 1);
});

test("a no-JS 'play another round' is a POST-redirect-GET back to the board", async () => {
  const response = await fetch(`${origin}/api/new`, {
    headers: { accept: BROWSER_ACCEPT },
    method: "POST",
    redirect: "manual",
  });
  assert.equal(response.status, 303);
  assert.equal(response.headers.get("location"), "/resilient.html");
});

test("a nonsense pick is refused in the caller's own language", async () => {
  const html = await postForm("pick=99");
  assert.equal(html.status, 400);
  assert.match(html.headers.get("content-type") ?? "", /^text\/html/);
  assert.match(await html.text(), /Pick a chest from 1 to 9/);

  const json = await postJson({ pick: "banana" });
  assert.equal(json.status, 400);
  assert.deepEqual(await json.json(), { kind: "error", message: "Pick a chest from 1 to 9." });
});

test("the page and the endpoint are the same origin, which is the point", async () => {
  const page = await fetch(`${origin}/`, { headers: { accept: BROWSER_ACCEPT } });
  assert.equal(page.status, 200);
  assert.match(page.headers.get("content-type") ?? "", /^text\/html/);
  const html = await page.text();
  // `/` serves the resilient page, and its form posts to a RELATIVE action —
  // so a native form navigation needs no CORS grant of any kind.
  assert.match(html, /<form id="pick-form" class="resilient-form" method="POST" action="\/api\/pick">/);
  assert.equal((html.match(/type="submit" name="pick"/g) ?? []).length, 9);

  const css = await fetch(`${origin}/styles/resilient.css`);
  assert.equal(css.status, 200);
  assert.match(css.headers.get("content-type") ?? "", /^text\/css/);
});

test("the static root cannot be escaped", async () => {
  const escaped = await fetch(`${origin}/../../../CLAUDE.md`, { redirect: "manual" });
  assert.notEqual(escaped.status, 200);
  const encoded = await fetch(`${origin}/%2e%2e%2f%2e%2e%2fCLAUDE.md`, { redirect: "manual" });
  assert.notEqual(encoded.status, 200);
});

test("the endpoints refuse the wrong method instead of guessing", async () => {
  const got = await fetch(`${origin}/api/pick`, { headers: { accept: "application/json" } });
  assert.equal(got.status, 405);
  const posted = await fetch(`${origin}/resilient.html`, { body: "", method: "POST" });
  assert.equal(posted.status, 405);
});

test("sessions are isolated: two players do not share a board", async () => {
  const a = await postJson({ pick: 2 });
  const b = await postJson({ pick: 2 });
  const cookieA = sessionCookieOf(a);
  const cookieB = sessionCookieOf(b);
  assert.notEqual(cookieA, cookieB);
  const replayA = (await (await postJson({ pick: 5 }, cookieA)).json()) as PickResponse;
  const replayB = (await (await postJson({ pick: 5 }, cookieB)).json()) as PickResponse;
  assert.equal(replayA.picked, 2);
  assert.equal(replayB.picked, 2);
});

// ── the store, without a socket in the way ─────────────────────────────────

test("the store hands the same session back for the same cookie, and a new one otherwise", () => {
  let id = 0;
  const store = createSessionStore({ idSource: (): string => `s${(id += 1)}`, seedSource: (): number => 4242 });
  const first = store.acquire(null);
  assert.equal(store.acquire(first.id).id, first.id);
  assert.notEqual(store.acquire(null).id, first.id);
  // An unknown cookie is not an error — it opens a session, exactly as a first
  // visit does. A locked-down browser that drops cookies must still be playable.
  assert.ok(store.acquire("not-a-session").id.startsWith("s"));
});

test("the outcome comes from the real chance engine, seed for seed", () => {
  const store = createSessionStore({ idSource: (): string => "fixed", seedSource: (): number => 4242 });
  const session = store.acquire(null);
  const result = resolvePick(session, 3);
  assert.equal(describeRound(session).seed, 4242);
  // The shared rules module decides this — the server adds no probability of
  // its own. Winners are the stochastic rounding of 9 · 0.44 = 3.96.
  assert.ok(result.winnerCount === 3 || result.winnerCount === 4, `unexpected winner count ${result.winnerCount}`);
  assert.equal(result.board.filter((chest) => chest.reward !== null).length, result.winnerCount);
  assert.equal(result.won, result.board[3]?.reward !== null);
});

test("idle sessions are swept so the map cannot grow forever", () => {
  let now = 0;
  let id = 0;
  const store = createSessionStore({
    idSource: (): string => `s${(id += 1)}`,
    nowMs: (): number => now,
    seedSource: (): number => 1,
  });
  const stale = store.acquire(null);
  assert.equal(store.size(), 1);
  now += 3 * 60 * 60 * 1000;
  const fresh = store.acquire(null);
  assert.equal(store.size(), 1);
  assert.notEqual(fresh.id, stale.id);
});
