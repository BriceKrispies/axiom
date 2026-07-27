/*
 * transport.ts — post a pick without navigating, or admit that you cannot.
 *
 * THE BUG THIS FILE EXISTS TO AVOID. Feature detection asks "is `fetch` here?".
 * In the environment this build targets — a managed enterprise browser behind a
 * proxy, with a CSP that may carry `connect-src 'none'` — `fetch` is present,
 * callable, and REJECTS. `typeof fetch === "function"` is true right up until
 * the request dies. So nothing here is gated on a `typeof` check: every attempt
 * is made inside `try`/`catch`, and the answer to "can we post in place?" is
 * "we tried, and here is what happened", never "we looked, and it seemed fine".
 *
 * The ladder is fetch → XMLHttpRequest → give up. Giving up is a real, expected
 * outcome, not an error: the caller responds by letting the browser's own form
 * submission proceed, which is the tier-1 path and always works. The transport
 * layer's job is to fail FAST and HONESTLY so that fallback stays available;
 * hence the timeout on XHR, without which a hung proxy connection would leave
 * the player looking at a dead board forever.
 *
 * `XMLHttpRequest` is the second rung rather than a curiosity because it
 * predates `fetch` by a decade and is exempt from some proxy rules that target
 * modern APIs — and because when both fail we have genuinely learned something,
 * rather than having tested one API twice.
 */

import type { PickResponse } from "./contract.ts";

/** A successful in-place post, and which rung of the ladder carried it. */
export interface PostSucceeded {
  readonly kind: "ok";
  readonly via: "fetch" | "xhr";
  readonly body: PickResponse;
}

/** Every rung failed. `reasons` is kept for the diagnostics line — knowing WHY
 * both transports died is the difference between a five-minute diagnosis and an
 * afternoon of guessing, in an environment where nobody can open devtools. */
export interface PostUnavailable {
  readonly kind: "unavailable";
  readonly reasons: readonly string[];
}

export type PostOutcome = PostSucceeded | PostUnavailable;

/** The pieces of the world this module touches, injected so the failure modes
 * that matter can be provoked in a test instead of hoped about. */
export interface TransportDeps {
  readonly fetchImpl?: unknown;
  readonly xhrCtor?: unknown;
  /** How long XHR may hang before we call it dead, in ms. */
  readonly timeoutMs?: number;
}

const DEFAULT_TIMEOUT_MS = 6000;

const describe = (error: unknown): string =>
  error instanceof Error ? `${error.name}: ${error.message}` : String(error);

type FetchLike = (url: string, init: Record<string, unknown>) => Promise<{
  readonly ok: boolean;
  readonly status: number;
  readonly json: () => Promise<unknown>;
}>;

const viaFetch = async (url: string, payload: unknown, impl: unknown): Promise<PickResponse> => {
  // Deliberately unguarded: if `impl` is undefined this throws a TypeError,
  // which is caught below and recorded exactly like a network refusal. A
  // `typeof` guard here would be the very check this module refuses to make.
  const send = impl as FetchLike;
  const response = await send(url, {
    body: JSON.stringify(payload),
    // Same-origin is the default, but a managed browser may ship a different
    // one; say it out loud so the session cookie is always sent.
    credentials: "same-origin",
    headers: { accept: "application/json", "content-type": "application/json" },
    method: "POST",
  });
  if (!response.ok) throw new Error(`server answered ${response.status}`);
  return (await response.json()) as PickResponse;
};

interface XhrLike {
  open: (method: string, url: string) => void;
  setRequestHeader: (name: string, value: string) => void;
  send: (body: string) => void;
  onload: (() => void) | null;
  onerror: (() => void) | null;
  ontimeout: (() => void) | null;
  timeout: number;
  status: number;
  responseText: string;
  withCredentials?: boolean;
}

const viaXhr = (url: string, payload: unknown, ctor: unknown, timeoutMs: number): Promise<PickResponse> =>
  new Promise<PickResponse>((accept, reject) => {
    const Ctor = ctor as new () => XhrLike;
    const request = new Ctor();
    request.open("POST", url);
    request.setRequestHeader("content-type", "application/json");
    request.setRequestHeader("accept", "application/json");
    request.timeout = timeoutMs;
    request.onerror = (): void => reject(new Error("XMLHttpRequest failed"));
    request.ontimeout = (): void => reject(new Error(`XMLHttpRequest timed out after ${timeoutMs}ms`));
    request.onload = (): void => {
      const ok = request.status >= 200 && request.status < 300;
      if (!ok) {
        reject(new Error(`server answered ${request.status}`));
        return;
      }
      try {
        accept(JSON.parse(request.responseText) as PickResponse);
      } catch (error: unknown) {
        reject(new Error(`unreadable response — ${describe(error)}`));
      }
    };
    request.send(JSON.stringify(payload));
  });

/**
 * Try to post `payload` to `url` in place. Never throws: the caller's decision
 * is between "render this" and "let the form go", and an exception would only
 * be a third way of saying the second.
 */
export const postInPlace = async (url: string, payload: unknown, deps: TransportDeps = {}): Promise<PostOutcome> => {
  const timeoutMs = deps.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const reasons: string[] = [];

  try {
    return { body: await viaFetch(url, payload, deps.fetchImpl), kind: "ok", via: "fetch" };
  } catch (error: unknown) {
    reasons.push(`fetch — ${describe(error)}`);
  }

  try {
    return { body: await viaXhr(url, payload, deps.xhrCtor, timeoutMs), kind: "ok", via: "xhr" };
  } catch (error: unknown) {
    reasons.push(`xhr — ${describe(error)}`);
  }

  return { kind: "unavailable", reasons };
};
