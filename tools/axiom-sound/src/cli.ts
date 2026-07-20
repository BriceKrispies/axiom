// CLI entry: parse args, resolve the app, dispatch to a command. All failures
// funnel through Reporter.error → a nonzero exit + a concise message (human) or
// a JSON error envelope (--json).

import { parseArgs } from 'node:util';
import { resolve } from 'node:path';
import { resolveApp } from './appdir.ts';
import { SoundError } from './errors.ts';
import { Reporter, type Sinks } from './output.ts';
import { runNew } from './commands/new.ts';
import { runCheck } from './commands/check.ts';
import { runBuild } from './commands/build.ts';
import { runList } from './commands/list.ts';
import { runClean } from './commands/clean.ts';
import { runPreview } from './commands/preview.ts';

const USAGE = `axiom-sound — Strudel game-sound asset pipeline

Usage:
  axiom-sound new     --app <app> --name <id>
  axiom-sound check   --app <app> [--name <id>]
  axiom-sound build   --app <app> [--name <id>] [--force]
  axiom-sound list    --app <app>
  axiom-sound clean   --app <app>
  axiom-sound preview --app <app> --name <id>

Global flags:
  --json      emit machine-readable JSON on stdout (diagnostics go to stderr)
  --verbose   include stacks / underlying causes in error output
  --help      show this help`;

export async function main(argv: readonly string[], sinks?: Sinks): Promise<number> {
  const parsed = parse(argv);
  const reporter = new Reporter({ json: parsed.json, verbose: parsed.verbose }, sinks);

  try {
    if (parsed.help || parsed.command === undefined) {
      reporter.human(USAGE);
      reporter.result({ ok: true, usage: true });
      return 0;
    }
    return await dispatch(parsed, reporter);
  } catch (err) {
    return reporter.error(err);
  }
}

interface ParsedArgs {
  readonly command?: string;
  readonly app?: string;
  readonly name?: string;
  readonly json: boolean;
  readonly verbose: boolean;
  readonly force: boolean;
  readonly help: boolean;
}

function parse(argv: readonly string[]): ParsedArgs {
  const { values, positionals } = parseArgs({
    args: [...argv],
    allowPositionals: true,
    options: {
      app: { type: 'string' },
      name: { type: 'string' },
      json: { type: 'boolean', default: false },
      verbose: { type: 'boolean', default: false },
      force: { type: 'boolean', default: false },
      help: { type: 'boolean', default: false },
    },
  });
  return {
    command: positionals[0],
    app: values.app,
    name: values.name,
    json: values.json ?? false,
    verbose: values.verbose ?? false,
    force: values.force ?? false,
    help: values.help ?? false,
  };
}

const COMMANDS = new Set(['new', 'check', 'build', 'list', 'clean', 'preview']);

async function dispatch(args: ParsedArgs, reporter: Reporter): Promise<number> {
  if (!COMMANDS.has(args.command ?? '')) {
    throw new SoundError('USAGE', `unknown command: ${args.command}`);
  }
  if (!args.app) {
    throw new SoundError('USAGE', `command \`${args.command}\` requires --app <app-path>`);
  }
  // `npm --prefix <tool> run …` runs with cwd = the tool dir. npm sets INIT_CWD
  // to the directory the user actually invoked npm from, so a relative --app
  // (e.g. `apps/my-app` from the repo root, via `make sound-build`) resolves
  // correctly. Falls back to cwd for a direct `node bin/axiom-sound.mjs` run.
  const base = process.env.INIT_CWD ?? process.cwd();
  const app = resolveApp(resolve(base, args.app));
  const common = { json: args.json, verbose: args.verbose };

  switch (args.command) {
    case 'new':
      return runNew(app, args.name, reporter, common);
    case 'check':
      return await runCheck(app, reporter, { ...common, name: args.name });
    case 'build':
      return await runBuild(app, reporter, { ...common, name: args.name, force: args.force });
    case 'list':
      return runList(app, reporter, common);
    case 'clean':
      return runClean(app, reporter, common);
    case 'preview':
      return await runPreview(app, reporter, { ...common, name: args.name });
    default:
      throw new SoundError('USAGE', `unknown command: ${args.command}`);
  }
}
