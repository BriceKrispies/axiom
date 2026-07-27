/*
 * inject.test.ts — the harness's one testable claim: the mask lands BEFORE the
 * page's own scripts.
 *
 * That is the whole contract. A mask injected after the page's first `<script>`
 * is not a weaker mask, it is a mask that does nothing at all while still
 * reporting a tier — the harness would lie, quietly, in the direction of
 * "everything is fine". So the position is pinned here rather than eyeballed in
 * a browser.
 *
 * Repo tooling: outside the Coverage Law, but this file is cheap and the thing
 * it guards is not recoverable by inspection.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { IMPORT_MAP, importMapFor, injectIntoHead, maskPreamble, parseDeny } from "./inject.ts";

const PAGE = '<!doctype html>\n<html>\n  <head>\n    <meta charset="utf-8">\n    <script src="/app.js"></script>\n  </head>\n</html>';

test("the preamble lands inside <head>, ahead of everything the page ships", () => {
  const out = injectIntoHead(PAGE, maskPreamble(["webgpu"]));
  const mask = out.indexOf("caps-mask.js");
  assert.ok(mask > out.indexOf("<head>"), "the preamble must be inside <head>");
  assert.ok(mask < out.indexOf("/app.js"), "the preamble must precede the page's own scripts");
  assert.ok(mask < out.indexOf('charset="utf-8"'), "the preamble must precede even the page's <meta>");
});

test("a head with attributes is still matched", () => {
  const out = injectIntoHead('<html><head lang="en" data-x><script src="/a.js"></script></head></html>', "<!--M-->");
  assert.ok(out.indexOf("<!--M-->") < out.indexOf("/a.js"));
});

test("a document with no head still gets the preamble first", () => {
  const out = injectIntoHead('<script src="/a.js"></script>', "<!--M-->");
  assert.ok(out.startsWith("<!--M-->"));
});

test("the deny list is embedded as JSON with no way to close the script early", () => {
  const tag = maskPreamble(["webgl2", "</script><script>alert(1)</script>"]);
  assert.equal(tag.split("</script>").length - 1, 2, "exactly the two tags the preamble is made of");
});

test("an import map is added only when the page does not ship one", () => {
  assert.equal(importMapFor("<html><head></head></html>"), IMPORT_MAP);
  assert.equal(importMapFor('<html><head><script type="importmap">{}</script></head></html>'), "");
  assert.equal(importMapFor("<html><head><script type='importmap'>{}</script></head></html>"), "");
});

test("deny tokens are read from commas or spaces, lowercased, and blanks dropped", () => {
  assert.deepEqual(parseDeny("WebGPU, webgl2   fetch,,"), ["webgpu", "webgl2", "fetch"]);
  assert.deepEqual(parseDeny(null), []);
  assert.deepEqual(parseDeny(""), []);
});
