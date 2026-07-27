/*
 * negotiate.test.ts — the two shapes one endpoint has to speak.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { acceptScore, negotiate, parseFields } from "./negotiate.ts";

test("a browser form navigation gets HTML", () => {
  // Exactly what Chrome sends when it navigates a form POST.
  assert.equal(negotiate("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"), "html");
  assert.equal(negotiate("text/html"), "html");
});

test("a scripted client gets JSON", () => {
  assert.equal(negotiate("application/json"), "json");
  assert.equal(negotiate("application/json, text/plain, */*"), "json");
});

test("an unopinionated client gets JSON — a tie is a machine", () => {
  assert.equal(negotiate("*/*"), "json");
  assert.equal(negotiate(""), "json");
  assert.equal(negotiate(undefined), "json");
});

test("q-values decide, not header order", () => {
  assert.equal(negotiate("application/json;q=0.1, text/html;q=0.9"), "html");
  assert.equal(negotiate("text/html;q=0.2, application/json;q=0.8"), "json");
  // A client that explicitly refuses HTML must not be sent HTML.
  assert.equal(negotiate("text/html;q=0, */*"), "json");
});

test("wildcards score through their range", () => {
  assert.equal(acceptScore("text/*", "text", "html"), 1);
  assert.equal(acceptScore("*/*;q=0.5", "application", "json"), 0.5);
  assert.equal(acceptScore("text/plain", "text", "html"), 0);
  // A malformed range is ignored rather than crashing the request.
  assert.equal(acceptScore("garbage, text/html", "text", "html"), 1);
});

test("both encodings decode to the same fields", () => {
  const encoded = parseFields("application/x-www-form-urlencoded", "pick=4");
  const json = parseFields("application/json", '{"pick":4}');
  assert.deepEqual(encoded, { pick: "4" });
  assert.deepEqual(json, { pick: "4" });
  assert.deepEqual(encoded, json, "the two tiers must arrive at one identical request");
});

test("an unknown or absent content type is read as a form body", () => {
  // A form POST is the baseline, so it is what an unlabelled body is assumed to be.
  assert.deepEqual(parseFields(undefined, "pick=7"), { pick: "7" });
  assert.deepEqual(parseFields("application/x-www-form-urlencoded; charset=UTF-8", "pick=7"), { pick: "7" });
});

test("a broken JSON body yields no fields rather than an exception", () => {
  assert.deepEqual(parseFields("application/json", "{not json"), {});
  assert.deepEqual(parseFields("application/json", ""), {});
  assert.deepEqual(parseFields("application/json", "[1,2]"), { 0: "1", 1: "2" });
});
