// `axiom-sound check --app <app> [--name <id>]`: semantically validate one or
// every source with the real Strudel implementation. Writes nothing. Exits
// nonzero if any source fails.

import { relative } from 'node:path';
import type { AppDirs } from '../appdir.ts';
import { isSoundError } from '../errors.ts';
import { loadSound } from '../pipeline.ts';
import { assertNoDuplicateIds, listSources, resolveNamed, type SourceRef } from '../sources.ts';
import { RenderHarness, withHarness } from '../render/harness.ts';
import type { Reporter } from '../output.ts';
import type { CommonOptions, NameOption } from '../options.ts';

export interface CheckOutcome {
  readonly id: string;
  readonly source: string;
  readonly ok: boolean;
  readonly hapCount?: number;
  readonly error?: { code: string; message: string; line?: number; column?: number };
}

export async function runCheck(
  app: AppDirs,
  reporter: Reporter,
  opts: CommonOptions & NameOption,
): Promise<number> {
  const refs = selectRefs(app, opts.name);
  if (refs.length === 0) {
    reporter.human('no sounds to check');
    reporter.result({ ok: true, checked: [] });
    return 0;
  }

  assertNoDuplicateIds(listSources(app));

  const outcomes = await withHarness((harness) => checkAll(app, refs, harness, reporter));
  const failed = outcomes.filter((o) => !o.ok);

  reporter.result({ ok: failed.length === 0, checked: outcomes });
  reporter.human(
    failed.length === 0
      ? `ok: ${outcomes.length} sound(s) passed`
      : `FAILED: ${failed.length}/${outcomes.length} sound(s) failed`,
  );
  return failed.length === 0 ? 0 : 1;
}

/** Resolve either the one named ref or all sources. */
export function selectRefs(app: AppDirs, name: string | undefined): SourceRef[] {
  return name ? [resolveNamed(app, name)] : listSources(app);
}

async function checkAll(
  app: AppDirs,
  refs: readonly SourceRef[],
  harness: RenderHarness,
  reporter: Reporter,
): Promise<CheckOutcome[]> {
  const outcomes: CheckOutcome[] = [];
  for (const ref of refs) {
    const source = relative(app.root, ref.path);
    try {
      const loaded = loadSound(app, ref);
      const hapCount = await harness.check(
        loaded.parsed.body,
        loaded.config,
        loaded.parsed.bodyStartLine,
      );
      reporter.info(`  ok    ${ref.stem}`);
      outcomes.push({ id: loaded.config.id, source, ok: true, hapCount });
    } catch (err) {
      const e = isSoundError(err)
        ? { code: err.code, message: err.message, line: err.details.line, column: err.details.column }
        : { code: 'INTERNAL', message: String(err) };
      reporter.info(`  FAIL  ${ref.stem}  [${e.code}] ${e.message}`);
      outcomes.push({ id: ref.stem, source, ok: false, error: e });
    }
  }
  return outcomes;
}
