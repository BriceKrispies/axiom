// The headless-Chromium render harness. One Chromium process is reused across a
// multi-sound build; each sound runs in a FRESH browser context (disposed after)
// so Strudel/audio state cannot leak between assets. All network is intercepted:
// the page itself is served from memory and every other request is aborted and
// recorded, so an attempted remote-sample fetch becomes NETWORK_ACCESS_ATTEMPTED.

import { chromium, type Browser, type Page } from 'playwright';
import { SoundError, type SourceLocation } from '../errors.ts';
import { CPS, SAMPLE_RATE, totalSeconds, type RenderConfig } from '../config.ts';
import type { RenderedPcm } from '../wav.ts';
import { buildBundle, buildRealtimeBundle, PAGE_URL, pageHtml } from './page.ts';
import type { CheckResult, Diagnostic, Phase, RenderResult } from './protocol.ts';

/**
 * Per-command wall-clock ceiling for a single sound's check or render. Sized to
 * cover a heavy full-length render with per-sample worklet DSP (distort/shape/…),
 * which runs in JS and is slower than realtime, while still catching a truly hung
 * headless browser in reasonable time.
 */
export const COMMAND_TIMEOUT_MS = 120_000;
/** Grace period after evaluate/render for any fire-and-forget fetch to surface. */
const NETWORK_SETTLE_MS = 150;

const PHASE_TO_CODE: Record<Phase, 'STRUDEL_TRANSPILE_FAILED' | 'STRUDEL_EVALUATION_FAILED' | 'STRUDEL_PATTERN_INVALID'> =
  {
    transpile: 'STRUDEL_TRANSPILE_FAILED',
    evaluate: 'STRUDEL_EVALUATION_FAILED',
    pattern: 'STRUDEL_PATTERN_INVALID',
    query: 'STRUDEL_PATTERN_INVALID',
  };

interface RunOutcome {
  readonly check?: CheckResult;
  readonly render?: RenderResult;
  readonly unexpectedRequests: readonly string[];
  readonly pageErrors: readonly string[];
}

export class RenderHarness {
  readonly #browser: Browser;
  readonly #bundleJs: string;
  readonly #realtimeBundleJs: string;

  private constructor(browser: Browser, bundleJs: string, realtimeBundleJs: string) {
    this.#browser = browser;
    this.#bundleJs = bundleJs;
    this.#realtimeBundleJs = realtimeBundleJs;
  }

  /** Launch Chromium and build both (offline + realtime) browser bundles. */
  static async launch(): Promise<RenderHarness> {
    const bundleJs = await buildBundle();
    const realtimeBundleJs = await buildRealtimeBundle();
    // `--autoplay-policy=no-user-gesture-required` lets the realtime capture path
    // start its AudioContext without a user gesture; it is inert for offline renders.
    const browser = await chromium.launch({
      headless: true,
      args: ['--autoplay-policy=no-user-gesture-required'],
    });
    return new RenderHarness(browser, bundleJs, realtimeBundleJs);
  }

  /** Close the shared browser. Safe to call more than once. */
  async dispose(): Promise<void> {
    await this.#browser.close();
  }

  /**
   * Run the full check pipeline for one sound. Throws a SoundError (with a
   * source-mapped location) on any failure; returns the hap count on success.
   */
  async check(code: string, config: RenderConfig, bodyStartLine: number): Promise<number> {
    const outcome = await this.run(this.#bundleJs, async (page) => {
      return {
        check: await withTimeout(
          page.evaluate(
            ([c, cfg]) => window.__strudelCheck({ code: c, seconds: cfg.seconds, cps: cfg.cps }),
            [code, { seconds: totalSeconds(config), cps: CPS }] as const,
          ),
          config.id,
        ),
      };
    });
    this.assertClean(outcome, config, bodyStartLine, outcome.check?.diagnostic);
    return outcome.check?.hapCount ?? 0;
  }

  /**
   * Render one sound to PCM. Runs the same validation as `check` first (never
   * renders invalid source). Throws SoundError on failure.
   */
  async render(code: string, config: RenderConfig, bodyStartLine: number): Promise<RenderedPcm> {
    const outcome = await this.run(this.#bundleJs, async (page) => {
      return {
        render: await withTimeout(
          page.evaluate(
            ([c, cfg]) =>
              window.__strudelRender({
                code: c,
                seconds: cfg.seconds,
                cps: cfg.cps,
                channels: cfg.channels,
                sampleRate: cfg.sampleRate,
              }),
            [
              code,
              { seconds: totalSeconds(config), cps: CPS, channels: config.channels, sampleRate: SAMPLE_RATE },
            ] as const,
          ),
          config.id,
        ),
      };
    });
    return this.finishRender(outcome, config, bodyStartLine);
  }

  /**
   * Render one sound to PCM through the REALTIME capture path (a live
   * AudioContext played in wall-clock time). Used for sounds that declare
   * `render = "realtime"` — non-deterministic, but immune to the offline
   * render's denormal-tail stall (e.g. distorted signal into `.room()` reverb).
   * The command timeout is widened to exceed the sound's own wall-clock length.
   */
  async renderRealtime(
    code: string,
    config: RenderConfig,
    bodyStartLine: number,
  ): Promise<RenderedPcm> {
    const seconds = totalSeconds(config);
    const timeoutMs = Math.max(COMMAND_TIMEOUT_MS, Math.ceil(seconds * 1000) + 60_000);
    const outcome = await this.run(this.#realtimeBundleJs, async (page) => {
      return {
        render: await withTimeout(
          page.evaluate(
            ([c, cfg]) =>
              window.__realtimeRender({
                code: c,
                seconds: cfg.seconds,
                cps: cfg.cps,
                channels: cfg.channels,
                sampleRate: cfg.sampleRate,
              }),
            [code, { seconds, cps: CPS, channels: config.channels, sampleRate: SAMPLE_RATE }] as const,
          ),
          config.id,
          timeoutMs,
        ),
      };
    });
    return this.finishRender(outcome, config, bodyStartLine);
  }

  /** Shared tail of `render`/`renderRealtime`: assert clean, then decode PCM. */
  private finishRender(
    outcome: RunOutcome,
    config: RenderConfig,
    bodyStartLine: number,
  ): RenderedPcm {
    this.assertClean(outcome, config, bodyStartLine, outcome.render?.diagnostic);

    const render = outcome.render;
    if (!render?.channelsB64 || render.sampleRate === undefined) {
      throw new SoundError('RENDER_INVALID_PCM', 'render returned no PCM', { id: config.id });
    }
    return {
      channels: render.channelsB64.map(base64ToFloat32),
      sampleRate: render.sampleRate,
    };
  }

  /**
   * Create an isolated context + a page navigated to the in-memory render page,
   * with network interception, run `fn` against that page, then dispose the
   * whole context (so no Strudel/audio state leaks to the next sound).
   */
  private async run(
    bundleJs: string,
    fn: (page: Page) => Promise<Partial<RunOutcome>>,
  ): Promise<RunOutcome> {
    const context = await this.#browser.newContext();
    const unexpectedRequests: string[] = [];
    const pageErrors: string[] = [];

    await context.route('**/*', (route) => {
      const url = route.request().url();
      if (url === PAGE_URL) {
        return route.fulfill({ contentType: 'text/html; charset=utf-8', body: pageHtml(bundleJs) });
      }
      unexpectedRequests.push(url);
      return route.abort();
    });
    context.on('weberror', (e) => pageErrors.push(e.error().message));

    try {
      const page = await context.newPage();
      page.on('pageerror', (e) => pageErrors.push(e.message));
      page.on('console', (m) => {
        if (m.type() === 'error') {
          pageErrors.push(m.text());
        }
      });
      await page.goto(PAGE_URL, { timeout: COMMAND_TIMEOUT_MS });
      const partial = await fn(page);
      // Let any fire-and-forget fetch (e.g. samples()) reach the route handler.
      await page.waitForTimeout(NETWORK_SETTLE_MS);
      return { unexpectedRequests, pageErrors, ...partial };
    } finally {
      await context.close();
    }
  }

  private assertClean(
    outcome: RunOutcome,
    config: RenderConfig,
    bodyStartLine: number,
    diagnostic: Diagnostic | undefined,
  ): void {
    if (outcome.unexpectedRequests.length > 0) {
      throw new SoundError(
        'NETWORK_ACCESS_ATTEMPTED',
        `render attempted network access (blocked): ${outcome.unexpectedRequests[0]}`,
        { id: config.id, extra: { urls: outcome.unexpectedRequests } },
      );
    }
    const ok = outcome.check?.ok ?? outcome.render?.ok ?? false;
    if (ok) {
      return;
    }
    if (diagnostic) {
      throw diagnosticError(diagnostic, config.id, bodyStartLine);
    }
    throw new SoundError(
      'STRUDEL_EVALUATION_FAILED',
      outcome.pageErrors[0] ?? 'source failed to evaluate for an unknown reason',
      { id: config.id, extra: { pageErrors: outcome.pageErrors } },
    );
  }
}

/** Launch a harness, run `fn`, and always dispose the browser afterwards. */
export async function withHarness<T>(fn: (harness: RenderHarness) => Promise<T>): Promise<T> {
  const harness = await RenderHarness.launch();
  try {
    return await fn(harness);
  } finally {
    await harness.dispose();
  }
}

function diagnosticError(diagnostic: Diagnostic, id: string, bodyStartLine: number): SoundError {
  // Body-relative line -> source-file line: line 1 of the body sits on source
  // line `bodyStartLine`.
  const location: SourceLocation =
    diagnostic.line !== undefined
      ? { line: bodyStartLine + diagnostic.line - 1, column: diagnostic.column }
      : {};
  return new SoundError(PHASE_TO_CODE[diagnostic.phase], diagnostic.message, { id, ...location });
}

function base64ToFloat32(b64: string): Float32Array {
  const raw = Buffer.from(b64, 'base64');
  // Copy into an aligned ArrayBuffer (Buffer's offset need not be 4-aligned).
  const aligned = raw.buffer.slice(raw.byteOffset, raw.byteOffset + raw.byteLength);
  return new Float32Array(aligned);
}

async function withTimeout<T>(
  promise: Promise<T>,
  id: string,
  timeoutMs: number = COMMAND_TIMEOUT_MS,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new SoundError('RENDER_TIMEOUT', `render timed out after ${timeoutMs}ms`, { id })),
      timeoutMs,
    );
  });
  try {
    return await Promise.race([promise, timeout]);
  } finally {
    if (timer) {
      clearTimeout(timer);
    }
  }
}
