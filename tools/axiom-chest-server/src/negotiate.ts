/*
 * negotiate.ts — request-body parsing and response content negotiation.
 *
 * ONE endpoint serves two very different clients:
 *
 *   - a browser NAVIGATING a `<form method="POST">` with no JavaScript at all.
 *     It sends `application/x-www-form-urlencoded` and an `Accept` header that
 *     ranks `text/html` first, and it expects a whole new document back.
 *   - the enhanced tiers, which send `application/json` and ask for JSON.
 *
 * Both are the SAME request as far as the game is concerned. That is the point
 * of the resilient build: the decision path is identical and only the
 * representation differs, so what the no-JS tier exercises is what the enhanced
 * tier exercises. This module is the only place that difference lives.
 *
 * Negotiation rule: parse `Accept` into media ranges with their q-values, score
 * `text/html` and `application/json` (falling back through `type/*` and `* /*`),
 * and pick HTML only when it is STRICTLY preferred. A tie — which is what a bare
 * `Accept: * /*` from curl or a default `fetch` produces — resolves to JSON,
 * because an unopinionated client is a machine client. A real form navigation
 * always names `text/html` explicitly, so it always lands on HTML.
 */

export type Representation = "html" | "json";

interface MediaRange {
  readonly type: string;
  readonly subtype: string;
  readonly q: number;
}

const parseRange = (raw: string): MediaRange | null => {
  const parts = raw.split(";").map((part) => part.trim());
  const [media, ...params] = parts;
  const [type, subtype] = (media ?? "").split("/");
  if (type === undefined || subtype === undefined || type === "") return null;
  const qParam = params.find((param) => param.toLowerCase().startsWith("q="));
  const q = qParam === undefined ? 1 : Number(qParam.slice(2));
  return { q: Number.isFinite(q) ? Math.max(0, Math.min(1, q)) : 1, subtype, type: type.toLowerCase() };
};

/** The best q-value this Accept header assigns to a concrete media type. */
export const acceptScore = (accept: string, type: string, subtype: string): number => {
  const ranges = accept
    .split(",")
    .map((raw) => parseRange(raw))
    .filter((range): range is MediaRange => range !== null);
  const matches = ranges.filter(
    (range) =>
      (range.type === type || range.type === "*") &&
      (range.subtype === subtype || range.subtype === "*"),
  );
  return matches.reduce((best, range) => Math.max(best, range.q), 0);
};

/**
 * Which representation to answer with. An absent/empty `Accept` means the
 * client said nothing, which is JSON by the rule above.
 */
export const negotiate = (accept: string | undefined): Representation => {
  const header = (accept ?? "").trim();
  const html = acceptScore(header, "text", "html");
  const json = acceptScore(header, "application", "json");
  return html > json ? "html" : "json";
};

/** The fields a request body carried, whatever its encoding was. */
export type Fields = Readonly<Record<string, string>>;

const parseUrlEncoded = (body: string): Fields =>
  Object.fromEntries(new URLSearchParams(body).entries());

const parseJson = (body: string): Fields => {
  try {
    const value: unknown = JSON.parse(body === "" ? "{}" : body);
    const record = typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
    return Object.fromEntries(Object.entries(record).map(([key, raw]) => [key, String(raw)]));
  } catch {
    return {};
  }
};

/**
 * Decode a body into flat string fields. Both encodings collapse to the same
 * shape here, so everything downstream — validation, the decision, the result —
 * is written once and cannot diverge between the two tiers.
 */
export const parseFields = (contentType: string | undefined, body: string): Fields => {
  const type = (contentType ?? "").split(";")[0]?.trim().toLowerCase() ?? "";
  return type === "application/json" ? parseJson(body) : parseUrlEncoded(body);
};
