/*
 * inject.ts — the pure half of the harness server: where a script goes in an
 * HTML document, and what the mask preamble looks like.
 *
 * Separated from the server because the ONLY thing that makes the harness
 * trustworthy is timing — the mask must run before the page's first script — and
 * timing here is a string-position question. A string-position question is
 * testable under bare `node --test`; the same question tangled into a request
 * handler is testable only by hand, and would rot.
 *
 * Repo tooling: outside the engine dependency graph and its laws.
 */

/** Where the deny list travels between the harness UI and the harness server. */
export const DENY_PARAM = "deny";

/** The URL prefix the harness serves its own files from. Kept out of the way of
 * the game's paths so hosting the real app under the harness needs no rewriting
 * of the app. */
export const HARNESS_PREFIX = "/__harness";

/** The import map for pages that do not ship one, and the same one `axiom-serve`
 * injects — the harness serves `@axiom/web-engine` from the package's built
 * `dist/` at `/vendor/`. Without it a page with a bare specifier cannot resolve
 * it and the engine ladder could not be exercised through the real app at all. */
export const IMPORT_MAP =
  '<script type="importmap">{"imports":{"@axiom/web-engine":"/vendor/axiom-web-engine/index.js"}}</script>';

/**
 * Inject the map only into pages that do not already have one.
 *
 * `resilient.html` ships its own (pointing at the app's `node_modules` copy), and
 * a second map is not a harmless duplicate: it is a different answer to the same
 * question. Whichever one wins, the harness would be running the game against an
 * engine build the game did not choose — which is precisely the drift this
 * harness exists to detect, arriving from the harness itself. `axiom-serve`
 * applies the same rule for the same reason.
 */
export const importMapFor = (html: string): string => (/type=["']importmap["']/i.test(html) ? "" : IMPORT_MAP);

/**
 * The two tags that arm the mask, in order: load the shared file, then call it.
 * `deny` is embedded as JSON, and `/` is escaped so no value can close the
 * script element early.
 */
export const maskPreamble = (deny: readonly string[]): string =>
  `<script src="${HARNESS_PREFIX}/caps-mask.js"></script>` +
  `<script>window.__axiomMask(${JSON.stringify(deny).replaceAll("/", "\\/")});</script>`;

/** Case-insensitively find the end of the opening `<head …>` tag, or -1. */
const headOpenEnd = (html: string): number => {
  const match = /<head[^>]*>/i.exec(html);
  return match === null ? -1 : match.index + match[0].length;
};

/**
 * Insert `snippet` as the FIRST content of `<head>`.
 *
 * First, not last, and not before `</head>`: the page's own `<script>` tags,
 * its stylesheets and its `<meta>` all live in that head, and a mask that lands
 * after them has already lost. When there is no head at all (a fragment, or a
 * document the parser will synthesise one for) the snippet goes at the very
 * front, which is still ahead of everything the document contains.
 */
export const injectIntoHead = (html: string, snippet: string): string => {
  const at = headOpenEnd(html);
  return at < 0 ? snippet + html : html.slice(0, at) + snippet + html.slice(at);
};

/** Parse `?deny=a,b c` into tokens. The harness UI writes commas; a human typing
 * the URL by hand writes spaces, and both should work. */
export const parseDeny = (raw: string | null): readonly string[] =>
  (raw ?? "")
    .split(/[\s,]+/)
    .map((token) => token.trim().toLowerCase())
    .filter((token) => token.length > 0);
