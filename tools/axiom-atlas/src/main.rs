//! `ax` - the Axiom repo's query-and-change gateway.
//!
//! Every search, symbol lookup, read and edit an agent performs is answered
//! here and appended to an NDJSON ledger. Two things fall out of that:
//!
//! 1. The repo becomes uniformly greppable, updateable and queryable through
//!    one surface, with one set of rules about what may be touched (nothing
//!    outside this checkout - see `repo.rs`).
//! 2. The ledger accumulates what agents *looked for*, which - via
//!    `ax miss` - is the only honest signal for what the repo is missing.
//!
//! CLI parsing is hand-rolled to match `xtask`'s style and to keep startup in
//! the low milliseconds: an agent will only prefer this tool if it is faster
//! than the habit it is replacing.

mod apply;
mod cite;
mod edit;
// `proc-macro2` is a dependency for its `span-locations` FEATURE, which is what
// gives `syn` real file:line for every AST node. Nothing here calls it directly.
use proc_macro2 as _;

mod graph;
mod index;
mod ledger;
mod observe;
mod repo;
mod search;
mod shape;
mod symbols;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read as _;
use std::process::ExitCode;
use std::time::Instant;

use ledger::{Record, Scope};
use repo::Repo;

/// Flags that never take a value.
const BOOL_FLAGS: &[&str] = &[
    "--all", "--json", "-i", "--ignore-case", "-F", "--fixed", "--apply", "--help", "-h",
    "--moved", "--vocab", "--rows",
];

/// Every flag that takes a value. A flag in neither list is a mistake, and
/// `Args::parse` now says so instead of guessing.
///
/// This registry is not bookkeeping — it is the fix for a real defect. An
/// unlisted flag used to be parsed as a key-value pair, silently swallowing the
/// next argument: `ax q 'pat' -A 30` made `-A` eat `30` and returned the match
/// with no context and no error, and `ax shape --help` took `--help` as a path
/// regex and ranked the whole repository. Three separate agents hit one of
/// those two in a single afternoon and each reported it as a wrong answer
/// rather than a usage error. That is the README's own "a zero that is a lie":
/// output with the shape of a true answer.
const VALUE_FLAGS: &[&str] = &[
    "--path", "--lang", "--limit", "--range", "--by", "--want", "--verdict", "--tool",
    "--replace", "--with", "--root", "--out", "--since", "--agent", "--session", "--kind",
];

// A boolean flag MUST be listed above. An unlisted `--flag` is parsed as a
// key-value pair and silently swallows the next argument, so `--vocab --limit
// 30` made `--vocab` eat `--limit` and the command printed the wrong report
// with no error. That is the "a zero that is a lie" failure the README names,
// wearing a different hat: the output had the shape of a true answer.

// A boolean flag MUST be listed above. An unlisted `--flag` is parsed as a
// key-value pair and silently swallows the next argument, so `--vocab --limit
// 30` made `--vocab` eat `--limit` and the command printed the wrong report
// with no error. That is the "a zero that is a lie" failure the README names,
// wearing a different hat: the output had the shape of a true answer.

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first().cloned() else {
        print_usage();
        return ExitCode::from(2);
    };

    if cmd == "help" || cmd == "--help" || cmd == "-h" {
        print_usage();
        return ExitCode::SUCCESS;
    }

    let repo = match Repo::discover() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ax: {e}");
            return ExitCode::from(2);
        }
    };

    // The detached observer. Runs, records what the worktree did since the last
    // look, and exits — no ledger row of its own, because it is bookkeeping
    // rather than something an agent asked for.
    if cmd == observe::OBSERVE_CMD {
        observe::observe(&repo);
        return ExitCode::SUCCESS;
    }

    let args = Args::parse(&argv[1..]);
    let started = Instant::now();
    let mut rec = Record::new(&cmd);

    // A flag the tool does not know is a usage error, not something to guess
    // at. `--help` after a subcommand lands here too, which is why
    // `ax shape --help` no longer scans the repository.
    if !args.unknown.is_empty() {
        let listed = args.unknown.join(", ");
        eprintln!("ax {cmd}: unknown flag(s): {listed}");
        eprintln!("ax {cmd}: run `ax help` for the command list and their flags");
        return ExitCode::from(2);
    }

    // `--help` AFTER a subcommand. It is a known bool flag, so the unknown-flag
    // guard above cannot catch it, and every command that takes a positional
    // pattern would otherwise read it as one: `ax shape --help` ranked the
    // whole repository, a slow and confidently wrong answer to a help request.
    if args.has("--help") || args.has("-h") {
        println!("{}", command_usage(&cmd));
        return ExitCode::SUCCESS;
    }

    let outcome = match cmd.as_str() {
        "q" | "search" => cmd_search(&repo, &args, &mut rec, None),
        "def" => cmd_symbol(&repo, &args, &mut rec, true),
        "refs" => cmd_symbol(&repo, &args, &mut rec, false),
        "impact" => cmd_impact(&repo, &args, &mut rec),
        "index" => cmd_index(&repo, &mut rec),
        "file" | "files" => cmd_files(&repo, &args, &mut rec),
        "cite" | "cites" => cmd_cite(&repo, &args, &mut rec),
        "read" => cmd_read(&repo, &args, &mut rec),
        "edit" => cmd_edit(&repo, &args, &mut rec),
        "apply" => cmd_apply(&repo, &mut rec),
        "write" => cmd_write(&repo, &args, &mut rec),
        "graph" => cmd_graph(&repo, &args, &mut rec),
        "owns" => cmd_owns(&repo, &args, &mut rec),
        "record" => cmd_record(&repo, &args, &mut rec),
        "friction" => cmd_friction(&args, &mut rec),
        "resolve" => cmd_resolve(&repo, &args, &mut rec),
        "miss" => cmd_miss(&repo, &args, &mut rec),
        "stats" => cmd_stats(&repo, &args, &mut rec),
        "compact" => cmd_compact(&repo, &mut rec),
        "sql" => cmd_sql(&repo, &args, &mut rec),
        "shape" => cmd_shape(&repo, &args, &mut rec),
        other => Err(Failure::Usage(format!("unknown command `{other}`"))),
    };

    rec.duration_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);

    let code = match outcome {
        Ok(Status::Found) => ExitCode::SUCCESS,
        Ok(Status::Empty) => {
            rec.zero_result = true;
            ExitCode::from(1)
        }
        Err(Failure::Usage(msg)) => {
            eprintln!("ax: {msg}");
            print_usage();
            rec.ok = false;
            rec.note = Some(msg);
            ExitCode::from(2)
        }
        Err(Failure::Refused(msg)) => {
            eprintln!("ax: {msg}");
            rec.ok = false;
            rec.note = Some(msg);
            ExitCode::from(3)
        }
        Err(Failure::Failed(msg)) => {
            eprintln!("ax: {msg}");
            rec.ok = false;
            rec.note = Some(msg);
            ExitCode::from(4)
        }
    };

    ledger::append(&repo, &rec);
    // Writes are OBSERVED, not mediated: this spawns a detached child at most
    // every few seconds to ask git what the worktree did. It never blocks and
    // never affects the exit code — see `observe` for why the mandate was
    // dropped.
    observe::maybe_spawn(&repo);
    code
}

// ---------------------------------------------------------------------------
// Command results
// ---------------------------------------------------------------------------

enum Status {
    /// The command produced results.
    Found,
    /// The command ran fine but found nothing - exit 1, grep-style, and the
    /// row is flagged `zero_result` for `ax miss`.
    Empty,
}

enum Failure {
    Usage(String),
    Refused(String),
    Failed(String),
}

type Outcome = Result<Status, Failure>;

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

struct Args {
    positional: Vec<String>,
    values: HashMap<String, String>,
    flags: HashSet<String>,
    /// Flags in neither registry. Reported before a command runs, so an
    /// unrecognised flag is a usage error rather than a quietly wrong answer.
    unknown: Vec<String>,
}

impl Args {
    fn parse(rest: &[String]) -> Self {
        let mut positional = Vec::new();
        let mut values = HashMap::new();
        let mut flags = HashSet::new();
        let mut unknown = Vec::new();

        let mut i = 0;
        while i < rest.len() {
            let a = &rest[i];
            if a.starts_with('-') {
                if BOOL_FLAGS.contains(&a.as_str()) {
                    flags.insert(a.clone());
                    i += 1;
                } else if VALUE_FLAGS.contains(&a.as_str()) {
                    match rest.get(i + 1) {
                        Some(v) => {
                            values.insert(a.clone(), v.clone());
                            i += 2;
                        }
                        None => {
                            unknown.push(format!("{a} needs a value"));
                            i += 1;
                        }
                    }
                } else {
                    // Neither list: refuse rather than guess. See VALUE_FLAGS.
                    unknown.push(a.clone());
                    i += 1;
                }
            } else {
                positional.push(a.clone());
                i += 1;
            }
        }
        Self { positional, values, flags, unknown }
    }

    fn arg(&self, n: usize) -> Option<&str> {
        self.positional.get(n).map(String::as_str)
    }

    fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    fn has(&self, key: &str) -> bool {
        self.flags.contains(key)
    }

    fn json(&self) -> bool {
        self.has("--json")
    }

    fn limit(&self, default: usize) -> usize {
        self.value("--limit")
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }
}

// ---------------------------------------------------------------------------
// Search-shaped commands
// ---------------------------------------------------------------------------

fn cmd_search(repo: &Repo, args: &Args, rec: &mut Record, pattern: Option<String>) -> Outcome {
    let pattern = match pattern {
        Some(p) => p,
        None => args
            .arg(0)
            .ok_or_else(|| Failure::Usage("`q` needs a pattern".to_owned()))?
            .to_owned(),
    };

    let q = search::Query {
        pattern: pattern.clone(),
        path_filter: args.value("--path").map(str::to_owned),
        lang: args.value("--lang").map(str::to_owned),
        limit: args.limit(80),
        case_insensitive: args.has("-i") || args.has("--ignore-case"),
        fixed: args.has("-F") || args.has("--fixed"),
    };

    rec.query = Some(args.arg(0).unwrap_or(&pattern).to_owned());
    rec.scope = Scope {
        path: q.path_filter.clone(),
        lang: q.lang.clone(),
    };

    let out = search::run(repo, &q).map_err(Failure::Usage)?;
    rec.hits = out.total;
    rec.files_matched = out.files_matched;
    rec.top_paths = distinct_paths(&out.hits, 10);

    emit_hits(args, &out);
    Ok(if out.total == 0 { Status::Empty } else { Status::Found })
}

fn cmd_symbol(repo: &Repo, args: &Args, rec: &mut Record, definition: bool) -> Outcome {
    let sym = args
        .arg(0)
        .ok_or_else(|| Failure::Usage("needs a symbol name".to_owned()))?;
    rec.query = Some(sym.to_owned());

    // The semantic index, not a regex. `--text` falls back to the old pattern
    // search for the cases the index cannot see — a name inside a macro body, a
    // TypeScript declaration, a mention in prose you actually want.
    if args.has("--text") {
        let pattern = match definition {
            true => symbols::definition_pattern(sym),
            false => symbols::reference_pattern(sym),
        };
        return cmd_search(repo, args, rec, Some(pattern));
    }

    let index = index::shard_for(repo, sym).map_err(Failure::Failed)?;
    let limit = args.limit(60);

    match definition {
        true => {
            let defs = index.defs_of(sym);
            rec.hits = defs.len();
            rec.files_matched = distinct(defs.iter().map(|d| d.file.as_str()));
            rec.top_paths = defs.iter().take(10).map(|d| d.file.clone()).collect();
            emit_defs(args, sym, &defs, limit);
            Ok(status(defs.len()))
        }
        false => {
            let refs = index.refs_of(sym);
            rec.hits = refs.len();
            rec.files_matched = distinct(refs.iter().map(|r| r.file.as_str()));
            rec.top_paths = refs.iter().take(10).map(|r| r.file.clone()).collect();
            emit_refs(args, sym, &refs, limit);
            Ok(status(refs.len()))
        }
    }
}

fn status(n: usize) -> Status {
    match n {
        0 => Status::Empty,
        _ => Status::Found,
    }
}

fn distinct<'a>(paths: impl Iterator<Item = &'a str>) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    paths.for_each(|p| {
        if !seen.contains(&p) {
            seen.push(p);
        }
    });
    seen.len()
}

fn emit_defs(args: &Args, sym: &str, defs: &[&index::Def], limit: usize) {
    if args.json() {
        println!(
            "{}",
            serde_json::json!({ "symbol": sym, "count": defs.len(), "defs": defs })
        );
        return;
    }
    defs.iter().take(limit).for_each(|d| {
        let q = match d.qualifier.is_empty() {
            true => String::new(),
            false => format!("{}::", d.qualifier),
        };
        let vis = match d.public {
            true => "pub ",
            false => "",
        };
        println!("{}:{}: {vis}{} {q}{}", d.file, d.line, d.kind.label(), d.name);
    });
    if defs.len() > limit {
        println!("... {} more (raise --limit)", defs.len() - limit);
    }
}

fn emit_refs(args: &Args, sym: &str, refs: &[&index::Ref], limit: usize) {
    if args.json() {
        println!(
            "{}",
            serde_json::json!({ "symbol": sym, "count": refs.len(), "refs": refs })
        );
        return;
    }
    refs.iter().take(limit).for_each(|r| {
        println!("{}:{}: {}", r.file, r.line, r.kind.label());
    });
    if refs.len() > limit {
        println!("... {} more (raise --limit)", refs.len() - limit);
    }
}

/// `ax impact <symbol>` — the blast radius of changing it.
///
/// Answers the question that decides how a change is scoped, and that this
/// session got wrong by not asking it: threading `FrameCamera` through the
/// engine turned out to reach public API consumed by tools and two other apps,
/// discovered halfway through, by which point the commit boundaries were
/// already decided.
///
/// Groups every reference by the package that owns it, and states each
/// package's class and the laws in force there — so "this is internal to one
/// module" and "this crosses into the app tier and is therefore public API" are
/// distinguishable before the first edit rather than after the first failed
/// build.
fn cmd_impact(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let sym = args
        .arg(0)
        .ok_or_else(|| Failure::Usage("`impact` needs a symbol name".to_owned()))?;
    rec.query = Some(sym.to_owned());

    let index = index::shard_for(repo, sym).map_err(Failure::Failed)?;
    let nodes = graph::load(repo);
    let defs = index.defs_of(sym);
    let refs = index.refs_of(sym);
    rec.hits = refs.len();
    rec.files_matched = distinct(refs.iter().map(|r| r.file.as_str()));

    // Package -> (references, files touched).
    let mut by_pkg: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    refs.iter().for_each(|r| {
        let owner = graph::owner(&nodes, &r.file).map(|n| n.name.clone());
        let key = owner.unwrap_or_else(|| "(unowned)".to_owned());
        let slot = by_pkg.entry(key).or_insert((0, Vec::new()));
        slot.0 += 1;
        if !slot.1.contains(&r.file) {
            slot.1.push(r.file.clone());
        }
    });

    let home: Vec<String> = defs
        .iter()
        .filter_map(|d| graph::owner(&nodes, &d.file).map(|n| n.name.clone()))
        .collect();
    let crosses: Vec<&String> = by_pkg.keys().filter(|k| !home.contains(k)).collect();

    if args.json() {
        let pkgs: Vec<_> = by_pkg
            .iter()
            .map(|(name, (n, files))| {
                serde_json::json!({ "package": name, "refs": n, "files": files })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "symbol": sym,
                "defined_in": home,
                "packages": pkgs,
                "crosses_package_boundary": !crosses.is_empty(),
            })
        );
        return Ok(status(refs.len()));
    }

    println!("{sym}");
    defs.iter().for_each(|d| {
        let owner = graph::owner(&nodes, &d.file)
            .map(|n| format!("  [{}]", n.name))
            .unwrap_or_default();
        println!("  defined  {} {}:{}{owner}", d.kind.label(), d.file, d.line);
    });
    println!(
        "  {} reference(s) across {} file(s) in {} package(s)",
        refs.len(),
        rec.files_matched,
        by_pkg.len()
    );
    by_pkg.iter().for_each(|(name, (n, files))| {
        let node = graph::find(&nodes, name);
        let class = node
            .map(|x| x.class.label())
            .unwrap_or("outside the workspace");
        println!("\n  {name}  ({class})");
        println!("    {n} reference(s) in {} file(s)", files.len());
        node.map(|x| {
            graph::laws(x.class)
                .iter()
                .for_each(|law| println!("    - {law}"));
        });
        files.iter().take(6).for_each(|f| println!("      {f}"));
        if files.len() > 6 {
            println!("      ... {} more", files.len() - 6);
        }
    });
    if !crosses.is_empty() {
        println!(
            "\n  CROSSES A PACKAGE BOUNDARY into: {}",
            crosses
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(
            "  Changing its shape is a change to published API, not an internal edit."
        );
    }
    Ok(status(refs.len()))
}

/// `ax index` — rebuild the semantic index and say what is in it.
fn cmd_index(repo: &Repo, rec: &mut Record) -> Outcome {
    let built = index::build(repo).map_err(Failure::Failed)?;
    rec.hits = built.defs + built.refs;
    println!(
        "indexed {} file(s): {} definitions, {} references",
        built.file_count,
        built.defs,
        built.refs
    );
    Ok(Status::Found)
}

fn cmd_files(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let needle = args
        .arg(0)
        .ok_or_else(|| Failure::Usage("`file` needs a path pattern".to_owned()))?;
    rec.query = Some(needle.to_owned());

    let found = search::find_files(repo, needle, args.limit(100)).map_err(Failure::Usage)?;
    rec.hits = found.len();
    rec.files_matched = found.len();
    rec.top_paths = found.iter().take(10).cloned().collect();

    if args.json() {
        println!(
            "{}",
            serde_json::json!({ "files": found, "count": found.len() })
        );
    } else {
        for f in &found {
            println!("{f}");
        }
    }
    Ok(if found.is_empty() { Status::Empty } else { Status::Found })
}

/// `ax cite <glob> [--baseline REV]` — resolve `foo.js:NNN` citations.
///
/// A port that ties each ported constant to the source line it came from is
/// only as trustworthy as those citations are, and nothing checked them: a
/// citation is prose, so it compiles, it passes every gate, and it rots (or is
/// wrong the day it is typed) in silence. Resolving one by hand costs a
/// `git show` and a read; the `apps/axiom-shmup` corpus holds ~3800 of them.
///
/// Two revisions are resolved, not one, because "wrong now" and "wrong when
/// written" are different findings with different fixes — the first is
/// documentation rot, the second means the port was read against the wrong
/// tree. See `cite.rs` for what the classifier can and cannot decide.
fn cmd_cite(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let pattern = args
        .arg(0)
        .ok_or_else(|| Failure::Usage("`cite` needs a path glob or regex".to_owned()))?;

    let req = cite::Request {
        pattern: pattern.to_owned(),
        baseline: args.value("--baseline").map(str::to_owned),
        source_root: args.value("--source-root").map(str::to_owned),
        limit: args.limit(60),
        only: args.value("--verdict").map(str::to_uppercase),
        moved: args.has("--moved"),
    };

    rec.query = Some(pattern.to_owned());
    rec.scope = Scope { path: Some(pattern.to_owned()), lang: None };

    let report = cite::run(repo, &req).map_err(|e| match e {
        cite::CiteError::Usage(m) => Failure::Usage(m),
        cite::CiteError::Refused(m) => Failure::Refused(m),
    })?;
    rec.hits = report.head.total;
    rec.files_matched = report.per_file.len();
    rec.top_paths = report.per_file.iter().take(10).map(|f| f.file.clone()).collect();
    rec.note = Some(format!(
        "{} citations, {:.0}% confirmed at HEAD",
        report.head.total,
        report.head.accuracy() * 100.0
    ));

    match args.json() {
        true => emit_cite_json(&report),
        false => emit_cite_text(&report, &req),
    }
    Ok(status(report.head.total))
}

fn cite_row_shown(row: &cite::Row, only: Option<&str>) -> bool {
    only.is_none_or(|want| {
        row.class() == want
            || row.history().is_some_and(|h| h == want)
            || (want == "EXTERNAL-BASE" && row.external_base.is_some())
    })
}

fn emit_cite_text(report: &cite::Report, req: &cite::Request) {
    let only = req.only.as_deref();
    let mut shown = 0usize;
    let mut current = String::new();
    let mut suppressed = 0usize;

    for row in &report.rows {
        if !cite_row_shown(row, only) {
            continue;
        }
        if shown >= req.limit {
            suppressed += 1;
            continue;
        }
        if row.file != current {
            println!("\n{}", row.file);
            current.clone_from(&row.file);
        }
        shown += 1;
        let target = row.target.as_deref().unwrap_or("(no such source file)");
        let amb = row.ambiguous.then_some("  [ambiguous basename]").unwrap_or("");
        println!(
            "  {}:{}  {}  ->  {}{}",
            row.file, row.line, row.raw, target, amb
        );
        let hist = row.history().map(|h| format!("  [{h}]")).unwrap_or_default();
        println!(
            "      HEAD {:<15} conf {:.2}{}",
            row.head.label(),
            row.head.confidence(),
            hist
        );
        row.head_text
            .as_deref()
            .map(|t| println!("        cited line reads: {t}"));
        if let Some(base) = &row.base {
            println!(
                "      {:<4} {:<15} conf {:.2}",
                report.baseline.as_deref().unwrap_or(""),
                base.label(),
                base.confidence()
            );
            row.base_text
                .as_deref()
                .filter(|t| Some(*t) != row.head_text.as_deref())
                .map(|t| println!("        read at baseline: {t}"));
        }
        row.suggest.map(|s| {
            let lo = row.ranges.first().map(|r| r.0).unwrap_or(0);
            println!(
                "      the quoted content is at line {s} ({:+})",
                i64::from(s) - i64::from(lo)
            )
        });
        row.moved_to
            .as_deref()
            .map(|m| println!("      and a better home exists in another file: {m}"));
        row.external_base
            .as_deref()
            .map(|b| println!("      cited against a base OUTSIDE this checkout: {b}"));
    }
    if suppressed > 0 {
        println!("\n... {suppressed} more row(s) (raise --limit)");
    }

    println!("\nper citing file");
    println!(
        "  {:<52} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>7}",
        "file", "cites", "ok", "part", "wrong", "eof", "nofil", "undec", "drift"
    );
    for f in &report.per_file {
        let d = f
            .drift
            .filter(|_| f.drift_samples >= 3)
            .map(|d| format!("{d:+}"))
            .unwrap_or_else(|| "-".to_owned());
        println!(
            "  {:<52} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>7}",
            f.file,
            f.head.total,
            f.head.ok,
            f.head.partial,
            f.head.wrong,
            f.head.out_of_range,
            f.head.unresolved,
            f.head.unverifiable,
            d
        );
    }

    println!(
        "\n{} citation(s) across {} file(s), {} file(s) scanned",
        report.head.total,
        report.per_file.len(),
        report.files_scanned
    );
    print_cite_offsets(report);
    print_cite_external(report);
    print_cite_tally("HEAD", &report.head);
    report
        .baseline
        .as_deref()
        .filter(|_| report.base.total > 0)
        .map(|rev| print_cite_tally(rev, &report.base));

    let failures = report.rotted + report.wrong_when_written;
    (failures > 0).then(|| {
        println!(
            "\n  rotted                {:>6}   was right at the baseline, wrong now",
            report.rotted
        );
        println!(
            "  wrong when written    {:>6}   wrong at both revisions ({:.0}% of failures)",
            report.wrong_when_written,
            100.0 * report.wrong_when_written as f64 / failures as f64
        );
    });
    println!(
        "\n  A citation is CONFIRMED only when everything its doc quotes is inside the\n  \
         cited range. UNVERIFIABLE means the doc quotes nothing findable — those rows\n  \
         are excluded from the rate rather than counted as passes."
    );
}

/// Files whose drifted citations share one or two offsets.
///
/// This is the difference between a corpus that one shift repairs and one that
/// needs reading. `physics/system.rs` is the first kind — two clean offsets, a
/// mechanical fix for every citation in it. `physics/character.rs` is the
/// second: its offsets scatter, so no shift is the answer and a human has to
/// look. Collapsing both into a median would hide exactly that distinction.
fn print_cite_offsets(report: &cite::Report) {
    // The question is not "do most citations agree" but "does ONE offset
    // explain a useful number of them" — a file can be half noise and still
    // have twenty citations repaired by a single shift. Requiring the top modes
    // to dominate hid exactly the two files this was built to find
    // (`physics/system.rs` +15 x23 of 72, `player/system.rs` +14 x19 of 51)
    // while surfacing three-sample coincidences.
    let mut interesting: Vec<&cite::FileStat> = report
        .per_file
        .iter()
        .filter(|f| f.drift_modes.first().is_some_and(|(_, n)| *n >= 5))
        .collect();
    interesting.sort_by_key(|f| std::cmp::Reverse(f.drift_modes[0].1));
    if interesting.is_empty() {
        return;
    }
    println!("
mechanical offsets — a consistent shift is a one-command repair");
    for f in interesting {
        let shifts: Vec<String> = f
            .drift_modes
            .iter()
            .take(2)
            .filter(|(_, n)| *n >= 3)
            .map(|(d, n)| format!("{d:+} x{n}"))
            .collect();
        println!(
            "  {:<52} {:<18} explains {}/{} drifted citation(s)",
            f.file,
            shifts.join(", "),
            f.drift_modes.iter().take(2).filter(|(_, n)| *n >= 3).map(|(_, n)| n).sum::<usize>(),
            f.drift_samples
        );
    }
}

/// Citations written against a source tree that is not this checkout.
///
/// A corpus answering to two citation bases at once cannot be repaired by any
/// single shift, because half of it is measured from a file the repo does not
/// have. That is a finding about the corpus, not about any one citation, so it
/// is reported on its own rather than folded into a verdict.
fn print_cite_external(report: &cite::Report) {
    if report.external == 0 {
        return;
    }
    println!(
        "
{} citation(s) name a base OUTSIDE this checkout:",
        report.external
    );
    report
        .external_bases
        .iter()
        .take(8)
        .for_each(|(b, n)| println!("  {n:>5}  {b}"));
}

fn print_cite_tally(label: &str, t: &cite::Tally) {
    println!(
        "\n  {label}: ok {} | partial {} | wrong {} | past-eof {} | no-such-file {} | unverifiable {}",
        t.ok, t.partial, t.wrong, t.out_of_range, t.unresolved, t.unverifiable
    );
    println!(
        "  {label}: confirmed {:.0}% of {} decidable citation(s)   (upper bound {:.0}% counting partials)",
        t.accuracy() * 100.0,
        t.decidable(),
        t.accuracy_upper() * 100.0
    );
}

fn emit_cite_json(report: &cite::Report) {
    let rows: Vec<_> = report
        .rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "file": r.file,
                "line": r.line,
                "citation": r.raw,
                "target": r.target,
                "ambiguous": r.ambiguous,
                "ranges": r.ranges.iter().map(|(a, b)| [a, b]).collect::<Vec<_>>(),
                "class": r.class(),
                "confidence": r.head.confidence(),
                "anchors": r.anchors,
                "head_text": r.head_text,
                "baseline_class": r.base.as_ref().map(cite::Judgement::label),
                "baseline_text": r.base_text,
                "history": r.history(),
                "suggested_line": r.suggest,
                "moved_to": r.moved_to,
                "external_base": r.external_base,
            })
        })
        .collect();
    let files: Vec<_> = report
        .per_file
        .iter()
        .map(|f| {
            serde_json::json!({
                "file": f.file,
                "head": cite_tally_json(&f.head),
                "baseline": cite_tally_json(&f.base),
                "drift_median": f.drift,
                "drift_samples": f.drift_samples,
                "drift_modes": f.drift_modes.iter().map(|(d, n)| serde_json::json!({"offset": d, "count": n})).collect::<Vec<_>>(),
                "external_base_citations": f.external,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({
            "citations": rows,
            "per_file": files,
            "head": cite_tally_json(&report.head),
            "baseline_rev": report.baseline,
            "baseline": cite_tally_json(&report.base),
            "rotted": report.rotted,
            "wrong_when_written": report.wrong_when_written,
            "files_scanned": report.files_scanned,
            "external_base_citations": report.external,
            "external_bases": report.external_bases.iter().map(|(b, n)| serde_json::json!({"base": b, "count": n})).collect::<Vec<_>>(),
        })
    );
}

fn cite_tally_json(t: &cite::Tally) -> serde_json::Value {
    serde_json::json!({
        "total": t.total,
        "ok": t.ok,
        "partial": t.partial,
        "wrong": t.wrong,
        "out_of_range": t.out_of_range,
        "unresolved_file": t.unresolved,
        "unverifiable": t.unverifiable,
        "decidable": t.decidable(),
        "accuracy": t.accuracy(),
        "accuracy_upper": t.accuracy_upper(),
    })
}

fn distinct_paths(hits: &[search::Hit], n: usize) -> Vec<String> {
    let mut seen = Vec::new();
    for h in hits {
        if !seen.contains(&h.path) {
            seen.push(h.path.clone());
        }
        if seen.len() >= n {
            break;
        }
    }
    seen
}

fn emit_hits(args: &Args, out: &search::Outcome) {
    if args.json() {
        let rows: Vec<_> = out
            .hits
            .iter()
            .map(|h| serde_json::json!({ "path": h.path, "line": h.line, "text": h.text }))
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "hits": rows,
                "total": out.total,
                "files_matched": out.files_matched,
                "truncated": out.truncated,
            })
        );
        return;
    }

    for h in &out.hits {
        println!("{}:{}:{}", h.path, h.line, h.text);
    }
    if out.truncated {
        println!(
            "... {} more hits in {} files (raise --limit)",
            out.total - out.hits.len(),
            out.files_matched
        );
    }
}

// ---------------------------------------------------------------------------
// Read and change
// ---------------------------------------------------------------------------

fn resolve(repo: &Repo, raw: &str) -> Result<std::path::PathBuf, Failure> {
    repo.resolve(raw).map_err(|e| Failure::Refused(e.to_string()))
}

fn cmd_read(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let raw = args
        .arg(0)
        .ok_or_else(|| Failure::Usage("`read` needs a path".to_owned()))?;
    // `resolve_read`, not `resolve`: reading may reach a configured reference
    // root. Every command below this one that WRITES still calls `resolve`, so
    // the readable set being wider than the writable set is a property of which
    // function each command calls rather than of a flag someone can forget.
    let path = repo
        .resolve_read(raw)
        .map_err(|e| Failure::Refused(e.to_string()))?;
    rec.query = Some(repo.rel(&path));
    rec.top_paths = vec![repo.rel(&path)];

    let range = args.value("--range").and_then(|r| {
        let (a, b) = r.split_once(':')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    });

    let text = edit::read_lines(&path, &repo.rel(&path), range).map_err(Failure::Failed)?;
    rec.hits = text.lines().count();
    println!("{text}");
    Ok(Status::Found)
}

fn cmd_edit(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let raw = args
        .arg(0)
        .ok_or_else(|| Failure::Usage("`edit` needs a path".to_owned()))?;
    let path = resolve(repo, raw)?;
    rec.query = Some(repo.rel(&path));
    rec.top_paths = vec![repo.rel(&path)];

    let old = args.value("--replace").ok_or_else(|| {
        Failure::Usage("`edit` needs --replace <text> --with <text>".to_owned())
    })?;
    let new = args
        .value("--with")
        .ok_or_else(|| Failure::Usage("`edit` needs --with <text>".to_owned()))?;

    let out = edit::replace(&path, &repo.rel(&path), old, new, args.has("--all"))
        .map_err(Failure::Failed)?;
    rec.hits = out.replacements;
    rec.files_matched = 1;
    rec.bytes_changed = out.delta();

    println!(
        "{}: {} replacement(s), {:+} bytes",
        repo.rel(&path),
        out.replacements,
        out.delta()
    );
    Ok(Status::Found)
}

fn cmd_write(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let raw = args
        .arg(0)
        .ok_or_else(|| Failure::Usage("`write` needs a path".to_owned()))?;
    let path = resolve(repo, raw)?;
    rec.query = Some(repo.rel(&path));
    rec.top_paths = vec![repo.rel(&path)];

    let mut content = String::new();
    std::io::stdin()
        .read_to_string(&mut content)
        .map_err(|e| Failure::Failed(format!("cannot read stdin: {e}")))?;

    let out = edit::write(&path, &repo.rel(&path), &content).map_err(Failure::Failed)?;
    rec.files_matched = 1;
    rec.bytes_changed = out.delta();

    println!("{}: wrote {} bytes", repo.rel(&path), out.bytes_after);
    Ok(Status::Found)
}

/// Applies a batch of edits read as JSON from stdin.
///
/// Every anchor is resolved against in-memory content before anything is
/// written, so a batch that would half-apply is rejected whole. This is the
/// command to reach for instead of writing a script.
fn cmd_apply(repo: &Repo, rec: &mut Record) -> Outcome {
    let mut raw = String::new();
    std::io::stdin()
        .read_to_string(&mut raw)
        .map_err(|e| Failure::Failed(format!("cannot read stdin: {e}")))?;

    let ops: Vec<apply::EditOp> = serde_json::from_str(&raw).map_err(|e| {
        Failure::Usage(format!(
            "stdin must be a JSON array of edits, e.g.\n  \
             [{{\"path\":\"a.md\",\"replace\":\"old\",\"with\":\"new\"}}]\n{e}"
        ))
    })?;

    // Scope every path — the edit target and any payload file — before a
    // single edit is planned.
    let mut resolved: Vec<apply::Resolved<'_>> = Vec::new();
    for op in &ops {
        let path = resolve(repo, &op.path)?;
        let label = repo.rel(&path);
        let payload = match op.payload_path() {
            Some(p) => {
                let src = resolve(repo, p)?;
                Some(std::fs::read_to_string(&src).map_err(|e| {
                    Failure::Failed(format!("cannot read text_file `{}`: {e}", repo.rel(&src)))
                })?)
            }
            None => None,
        };
        resolved.push((path, label, op, payload));
    }

    let planned = apply::plan(&resolved).map_err(|errs| {
        Failure::Failed(format!(
            "{} of {} edit(s) could not be applied - nothing was written:\n  {}",
            errs.len(),
            ops.len(),
            errs.join("\n  ")
        ))
    })?;

    let mut changed = 0usize;
    for p in &planned {
        let on_disk = std::fs::read_to_string(&p.path).unwrap_or_default();
        if on_disk == p.content {
            continue;
        }
        edit::write(&p.path, &p.label, &p.content).map_err(Failure::Failed)?;
        rec.bytes_changed += p.after - p.before;
        rec.top_paths.push(p.label.clone());
        changed += 1;
        println!("{}: {:+} bytes", p.label, p.after - p.before);
    }

    rec.hits = ops.len();
    rec.files_matched = changed;
    println!("{} edit(s) applied across {changed} file(s)", ops.len());
    Ok(Status::Found)
}

// ---------------------------------------------------------------------------
// Architecture
// ---------------------------------------------------------------------------

fn cmd_graph(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let nodes = graph::load(repo);

    let Some(query) = args.arg(0) else {
        rec.hits = nodes.len();
        for n in &nodes {
            println!("{:<40} {:<28} {}", n.dir, n.name, n.class.label());
        }
        return Ok(Status::Found);
    };

    rec.query = Some(query.to_owned());
    let Some(node) = graph::find(&nodes, query) else {
        println!("no layer, module, app or tool matches `{query}`");
        return Ok(Status::Empty);
    };

    rec.hits = 1;
    rec.top_paths = vec![node.dir.clone()];

    if args.json() {
        println!(
            "{}",
            serde_json::json!({
                "name": node.name,
                "crate": node.crate_name,
                "dir": node.dir,
                "class": node.class.label(),
                "layers": node.layers,
                "modules": node.modules,
                "capabilities": node.capabilities,
                "dependents": graph::dependents(&nodes, node)
                    .iter().map(|d| d.name.clone()).collect::<Vec<_>>(),
                "laws": graph::laws(node.class),
            })
        );
    } else {
        print!("{}", graph::describe(&nodes, node));
    }
    Ok(Status::Found)
}

fn cmd_owns(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let raw = args
        .arg(0)
        .ok_or_else(|| Failure::Usage("`owns` needs a path".to_owned()))?;
    let path = resolve(repo, raw)?;
    let rel = repo.rel(&path);
    rec.query = Some(rel.clone());

    let nodes = graph::load(repo);
    let Some(node) = graph::owner(&nodes, &rel) else {
        println!("{rel}: not owned by any layer, module, app or tool");
        return Ok(Status::Empty);
    };

    rec.hits = 1;
    rec.top_paths = vec![node.dir.clone()];

    if args.json() {
        println!(
            "{}",
            serde_json::json!({
                "path": rel,
                "owner": node.name,
                "class": node.class.label(),
                "spine": node.class.is_spine(),
                "laws": graph::laws(node.class),
            })
        );
    } else {
        println!("{rel}");
        print!("{}", graph::describe(&nodes, node));
    }
    Ok(Status::Found)
}

/// Records a change made *outside* `ax` - by an editor, a script, or an agent
/// tool that edits files directly - so the ledger stays a complete account of
/// what changed, and reminds the caller which laws govern what they just
/// touched.
fn cmd_record(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let raw = args
        .arg(0)
        .ok_or_else(|| Failure::Usage("`record` needs a path".to_owned()))?;
    let path = resolve(repo, raw)?;
    let rel = repo.rel(&path);

    rec.query = Some(rel.clone());
    rec.top_paths = vec![rel.clone()];
    rec.files_matched = 1;
    rec.hits = 1;
    rec.bytes_changed = args
        .value("--bytes")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    rec.note = args.value("--tool").map(str::to_owned);

    // Surfacing the governing laws right after a change is the whole reason an
    // agent benefits from routing through here rather than editing silently.
    let nodes = graph::load(repo);
    let Some(node) = graph::owner(&nodes, &rel) else {
        return Ok(Status::Found);
    };
    if node.class.is_spine() {
        eprintln!("ax: {rel} is in `{}` ({}).", node.name, node.class.label());
        eprintln!(
            "ax: the Branchless Law and the 100% Coverage Law apply here - no if/match/for in non-test code, and this change ships with the tests that cover it."
        );
    }
    Ok(Status::Found)
}

/// Records friction: `ax` could not answer a question, or could not make a
/// change, that an agent legitimately needed.
///
/// This is the other half of `ax miss`. A zero-result search says *the repo*
/// could not answer something; a friction row says *the tool* could not. Both
/// are backlog. Under the Atlas Friction Law an agent that hits a wall logs it
/// here and then fixes the cause - it does not quietly fall back to raw grep.
fn cmd_friction(args: &Args, rec: &mut Record) -> Outcome {
    let what = args.arg(0).ok_or_else(|| {
        Failure::Usage(
            "`friction` needs a description, e.g.\n  ax friction <what-you-tried> --want <what-you-needed> --verdict tool"
                .to_owned(),
        )
    })?;

    let verdict = args.value("--verdict").unwrap_or("unknown");
    matches!(verdict, "tool" | "repo" | "unknown")
        .then_some(())
        .ok_or_else(|| {
            Failure::Usage(format!(
                "--verdict must be `tool`, `repo` or `unknown` (got `{verdict}`)"
            ))
        })?;

    let id = ledger::friction_id(what);
    rec.id = Some(id.clone());
    rec.query = Some(what.to_owned());
    rec.note = Some(format!(
        "verdict={verdict}; want={}",
        args.value("--want").unwrap_or("-")
    ));
    rec.hits = 1;

    eprintln!("ax: logged friction {} [{verdict}] {what}", short_id(&id));
    eprintln!(
        "ax: the Atlas Friction Law applies - do not route around this. Fix the tool \
(`tools/axiom-atlas`) unless the repo is genuinely the thing that is mis-shaped."
    );
    eprintln!("ax: when it is fixed, close it with `ax resolve {}`", short_id(&id));
    Ok(Status::Found)
}

/// Closes a friction row once its cause is fixed.
///
/// The ledger is append-only - many agents write it concurrently and nothing
/// may rewrite history - so a resolution is *another row* that names the id it
/// closes, not an edit to the original. `ax miss` then subtracts the closed
/// ids. The complaint stays on the record; only the backlog shrinks.
fn cmd_resolve(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let prefix = args.arg(0).ok_or_else(|| {
        Failure::Usage("`resolve` needs a friction id - run `ax miss` to list them".to_owned())
    })?;

    let rows = ledger::read_all(repo);
    let mut found: Vec<(String, String)> = rows
        .iter()
        .filter(|r| r.cmd == "friction")
        .filter_map(|r| {
            r.id.clone()
                .map(|id| (id, r.query.clone().unwrap_or_default()))
        })
        .filter(|(id, _)| id.starts_with(prefix))
        .collect();
    found.sort();
    found.dedup();

    let (id, what) = match found.as_slice() {
        [] => {
            return Err(Failure::Failed(format!(
                "no logged friction has an id starting with `{prefix}` - run `ax miss` to list them"
            )))
        }
        [one] => one,
        many => {
            return Err(Failure::Failed(format!(
                "`{prefix}` matches {} friction ids; use more characters",
                many.len()
            )))
        }
    };

    rec.id = Some(id.clone());
    rec.query = Some(id.clone());
    rec.note = Some(format!(
        "resolved={what}; by={}",
        args.value("--by").unwrap_or("-")
    ));
    rec.hits = 1;

    println!("resolved {} {what}", short_id(id));
    Ok(Status::Found)
}

/// The 7-character form shown in reports and accepted by `ax resolve`.
fn short_id(id: &str) -> &str {
    id.get(..7).unwrap_or(id)
}

// ---------------------------------------------------------------------------
// The ledger's own queries
// ---------------------------------------------------------------------------

/// Commands that represent a change to the tree.
fn is_change(cmd: &str) -> bool {
    matches!(cmd, "edit" | "write" | "apply" | "record")
}

fn cmd_miss(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let rows = ledger::read_all(repo);
    let show_all = args.has("--all");

    let ranked = ledger::rank(
        rows.iter()
            .filter(|r| r.zero_result && r.ok)
            .filter_map(|r| r.query.clone().map(|q| (r.cmd.clone(), q))),
    );

    let resolved: HashSet<String> = rows
        .iter()
        // `r.ok` matters: a failed `ax resolve` must never close anything.
        .filter(|r| r.cmd == "resolve" && r.ok)
        .filter_map(|r| r.query.clone())
        .collect();

    let frictions = ledger::rank(rows.iter().filter(|r| r.cmd == "friction").filter_map(|r| {
        let id = r.id.clone().unwrap_or_default();
        (show_all || !resolved.contains(&id)).then(|| {
            (
                id,
                r.query.clone().unwrap_or_default(),
                r.note.clone().unwrap_or_default(),
            )
        })
    }));

    rec.hits = ranked.len() + frictions.len();
    let limit = args.limit(30);

    if args.json() {
        let misses: Vec<_> = ranked
            .iter()
            .take(limit)
            .map(|((cmd, q), n)| serde_json::json!({ "cmd": cmd, "query": q, "misses": n }))
            .collect();
        let friction: Vec<_> = frictions
            .iter()
            .take(limit)
            .map(|((id, what, note), n)| {
                serde_json::json!({
                    "id": id,
                    "what": what,
                    "note": note,
                    "n": n,
                    "resolved": resolved.contains(id),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "misses": misses,
                "friction": friction,
                "resolved_count": resolved.len(),
            })
        );
        return Ok(if rec.hits == 0 { Status::Empty } else { Status::Found });
    }

    if rec.hits == 0 {
        println!("nothing outstanding - no zero-result searches, no open friction");
        (resolved.is_empty())
            .then_some(())
            .map_or_else(|| println!("({} friction row(s) resolved)", resolved.len()), |()| ());
        return Ok(Status::Empty);
    }

    if !ranked.is_empty() {
        println!("What this REPO could not answer (zero-result searches):");
        println!("{:>6}  {:<6} {}", "misses", "cmd", "query");
        for ((cmd, q), n) in ranked.iter().take(limit) {
            println!("{n:>6}  {cmd:<6} {q}");
        }
    }

    if !frictions.is_empty() {
        println!("\nWhat this TOOL could not do (open friction):");
        for ((id, what, note), n) in frictions.iter().take(limit) {
            let mark = resolved.contains(id).then_some(" (resolved)").unwrap_or("");
            println!("{n:>6}  {}  {what}{mark}", short_id(id));
            println!("        {note}");
        }
        println!("\nClose one with: ax resolve <id> --by <what fixed it>");
    }
    Ok(Status::Found)
}

fn cmd_stats(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let rows = ledger::read_all(repo);
    rec.hits = rows.len();

    if rows.is_empty() {
        println!("ledger is empty - nothing recorded yet");
        return Ok(Status::Empty);
    }

    let limit = args.limit(15);
    let by_cmd = ledger::rank(rows.iter().map(|r| r.cmd.clone()));
    let queries = ledger::rank(rows.iter().filter_map(|r| r.query.clone()));
    let touched = ledger::rank(
        rows.iter()
            .filter(|r| is_change(&r.cmd) && r.ok)
            .flat_map(|r| r.top_paths.clone()),
    );
    let zero = rows.iter().filter(|r| r.zero_result).count();
    let median_us = {
        let mut d: Vec<u64> = rows.iter().map(|r| r.duration_us).collect();
        d.sort_unstable();
        d.get(d.len() / 2).copied().unwrap_or(0)
    };

    if args.json() {
        println!(
            "{}",
            serde_json::json!({
                "invocations": rows.len(),
                "zero_result": zero,
                "median_duration_us": median_us,
                "by_command": by_cmd.iter().map(|(k, n)| serde_json::json!({"cmd": k, "n": n})).collect::<Vec<_>>(),
                "top_queries": queries.iter().take(limit).map(|(k, n)| serde_json::json!({"query": k, "n": n})).collect::<Vec<_>>(),
                "most_edited": touched.iter().take(limit).map(|(k, n)| serde_json::json!({"path": k, "n": n})).collect::<Vec<_>>(),
            })
        );
        return Ok(Status::Found);
    }

    let pct = (zero as f64) * 100.0 / (rows.len() as f64);
    println!("invocations      {}", rows.len());
    println!("zero-result      {zero} ({pct:.1}%)");
    println!("median latency   {median_us} us");
    println!("\nby command");
    for (cmd, n) in &by_cmd {
        println!("  {n:>6}  {cmd}");
    }
    println!("\ntop queries");
    for (q, n) in queries.iter().take(limit) {
        println!("  {n:>6}  {q}");
    }
    if !touched.is_empty() {
        println!("\nmost-edited files");
        for (p, n) in touched.iter().take(limit) {
            println!("  {n:>6}  {p}");
        }
    }
    Ok(Status::Found)
}

fn cmd_compact(repo: &Repo, rec: &mut Record) -> Outcome {
    let raw = ledger::raw_dir(repo);
    let parquet = ledger::parquet_dir(repo);
    let today = Record::new("probe").day;

    let mut days: Vec<std::path::PathBuf> = std::fs::read_dir(&raw)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "ndjson"))
                .filter(|p| {
                    p.file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| s < today.as_str())
                })
                .collect()
        })
        .unwrap_or_default();
    days.sort();

    if days.is_empty() {
        println!("nothing to compact (today's ledger stays raw until the day closes)");
        return Ok(Status::Empty);
    }

    let has_duckdb = std::process::Command::new("duckdb")
        .arg("--version")
        .output()
        .is_ok();

    if !has_duckdb {
        println!("duckdb is not on PATH. Run this yourself to compact:\n");
        println!("{}", compact_sql(&raw, &parquet));
        return Ok(Status::Found);
    }

    std::fs::create_dir_all(&parquet)
        .map_err(|e| Failure::Failed(format!("cannot create parquet dir: {e}")))?;

    let mut compacted = 0usize;
    for day in &days {
        let stem = day.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let target = parquet.join(format!("{stem}.parquet"));
        let sql = format!(
            "COPY (SELECT * FROM read_json_auto('{}', union_by_name=true)) TO '{}' (FORMAT PARQUET);",
            day.display().to_string().replace('\\', "/"),
            target.display().to_string().replace('\\', "/")
        );
        let status = std::process::Command::new("duckdb")
            .arg("-c")
            .arg(&sql)
            .status()
            .map_err(|e| Failure::Failed(format!("duckdb failed: {e}")))?;
        if status.success() {
            let _ = std::fs::remove_file(day);
            compacted += 1;
            println!("compacted {stem} -> {}", repo.rel(&target));
        }
    }

    rec.hits = compacted;
    Ok(Status::Found)
}

fn compact_sql(raw: &std::path::Path, parquet: &std::path::Path) -> String {
    format!(
        "COPY (SELECT * FROM read_json_auto('{}/*.ndjson', union_by_name=true))\n  TO '{}/ledger.parquet' (FORMAT PARQUET);",
        raw.display().to_string().replace('\\', "/"),
        parquet.display().to_string().replace('\\', "/")
    )
}

/// A directory as DuckDB will accept it: forward slashes, and without the
/// Windows extended-length prefix.
///
/// `Repo` canonicalises, which on Windows yields a `\?\`-prefixed path. That
/// prefix is a Win32 API instruction, not part of the name, and DuckD\ub cannot
/// glob through it — the first real query this command ever ran failed on it.
fn duck_dir(path: &std::path::Path) -> String {
    path.display()
        .to_string()
        .replace(std::path::MAIN_SEPARATOR, "/")
        .trim_start_matches("//?/")
        .to_owned()
}

/// `ax sql [query]` — run a query over the whole ledger, or print the cuts
/// worth starting from.
///
/// **It executes.** This used to print a preamble and a few example queries for
/// a human to paste into a DuckDB somebody else had installed, which meant the
/// ledger's richest surface was unavailable on any machine that had not been
/// set up — and on the machine this was written on it simply was not, so the
/// questions the ledger exists to answer went unasked for as long as it has
/// existed.
///
/// The engine is the `duckdb` CLI on `PATH`, not a compiled-in one. Embedding it
/// was tried first and is the better ergonomics — a fresh checkout could query
/// with nothing installed — but `libduckdb-sys`'s `bundled` feature compiles
/// DuckDB's C++ amalgamation, which is minutes of `cc1plus` and gigabytes of
/// RAM on every cold target directory. `tools/` is meant to stay cheap to
/// build, and a 12 MB CLI one `scoop install duckdb` away is the smaller trade.
/// A machine without it gets the queries printed, exactly as before, so nothing
/// this command could do yesterday stops working.
/// `ax shape <path RE> [--vocab] [--limit N] [--json]`
///
/// Is this code data wearing Rust, or a genuine algorithm? See `shape.rs` for
/// what each column means and why the walk parses rather than greps.
/// Per-subcommand usage. Reached by `ax <cmd> --help`.
fn command_usage(cmd: &str) -> &'static str {
    match cmd {
        "q" | "search" => "ax q <regex> [--path P] [--lang L] [--limit N] [-i] [-F] [--json]
                             Search file contents. Zero results are recorded for `ax miss`.",
        "def" => "ax def <symbol> [--json]
  Where a symbol is defined (semantic index).",
        "refs" => "ax refs <symbol> [--limit N] [--json]
  Every real reference, with its kind.",
        "impact" => "ax impact <symbol>
  Blast radius: packages touched, and the laws in force there.",
        "file" | "files" => "ax file <regex|glob> [--limit N] [--json]
  Find files by path.",
        "read" => "ax read <path> [--range A:B]
  Read a file, or a line range of one.",
        "edit" => "ax edit <path> --replace <old> --with <new> [--all]
  Anchored single edit.",
        "apply" => "ax apply [<script>] [--dry-run]
  Batch edits, escape-free, all-or-nothing.
  A `from`/`to` span runs from the START of `from` to the START of `to`, so the
  `to` anchor is NOT consumed -- it stays in the file after the replacement.
  Include it at the end of your `with` payload if you meant to replace it.",
        "graph" => "ax graph [<layer|module|app>]
  Deps, dependents, and the laws in force.",
        "owns" => "ax owns <path>
  Which package owns a file, its class, and its rules.",
        "shape" => "ax shape <path regex> [--vocab] [--rows] [--limit N] [--json]
                      Is this code data wearing Rust, or an algorithm?
                      rows    = how many rows the biggest repeated record has (the N in
                                the Datafication Law's (N-1) x per-variant-code).
                      frm/row = what share of that record's fields vary in SHAPE rather
                                than just in value. 0.00 = every field one shape.
                      READ THOSE TWO TOGETHER. Low frm/row says a table is POSSIBLE;
                      high rows says it is WORTH IT. frm/row 0.00 at rows=3 saves
                      nothing; rows=30 at frm/row 0.84 is an algorithm.
                      lit/ln and reuse say only that content is PRESENT, not that it
                      is addressable -- do not screen on them alone.
                      --vocab names the callee vocabulary; --rows breaks frm/row down
                      per target. Tests are excluded from every count.",
        "eol" => "ax eol [<path regex>] [--fix]
  Line endings: what is, and what should be.",
        "wgsl" => "ax wgsl [<path regex>] [--apply]
  Inlined shader strings -> .wgsl files.",
        "friction" => "ax friction <what> --want <what you needed> --verdict tool|repo|unknown",
        "resolve" => "ax resolve <id> --by <what fixed it>",
        "miss" => "ax miss [--all]
  What the repo, and the tool, could not answer.",
        "stats" => "ax stats
  What agents look for and change.",
        "sql" => "ax sql <query>
  DuckDB over the whole ledger.",
        _ => "ax help
  Run `ax help` for the full command list.",
    }
}

fn cmd_shape(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    let filter = args.arg(0).unwrap_or(".");
    rec.query = Some(filter.to_owned());

    // `--limit` governs how many ROWS are printed, not how many files are
    // scanned: the summary counts and the merged vocabulary are only true if
    // the whole matched set was walked. Throttling the search here made the
    // first run of this command report on 22 files and call it the app.
    const SCAN_CAP: usize = 100_000;
    let files = search::find_files(repo, filter, SCAN_CAP).map_err(Failure::Usage)?;

    let mut skipped = 0usize;
    let mut shapes: Vec<shape::Shape> = files
        .iter()
        .filter_map(|rel| {
            (rel.ends_with(".rs")).then_some(())?;
            let abs = repo.resolve_read(rel).ok()?;
            let source = std::fs::read_to_string(&abs).ok()?;
            let parsed = shape::analyse(rel, &source);
            if parsed.is_none() {
                skipped += 1;
            }
            parsed
        })
        .collect();

    // Densest-in-constants first: this command exists to rank candidates, and
    // the top of the list is where a datafication pass should start.
    shapes.sort_by(|a, b| {
        b.literal_density()
            .partial_cmp(&a.literal_density())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    rec.hits = shapes.len();
    rec.files_matched = shapes.len();

    if args.json() {
        let items: Vec<serde_json::Value> = shapes
            .iter()
            .map(|s| {
                serde_json::json!({
                    "path": s.path,
                    "code_lines": s.code_lines,
                    "literals": s.literals,
                    "floats": s.floats,
                    "branches": s.branches,
                    "calls": s.calls,
                    "distinct_calls": s.distinct_calls(),
                    "literal_density": s.literal_density(),
                    "branch_density": s.branch_density(),
                    "reuse": s.reuse(),
                    "verdict": s.verdict().label(),
                })
            })
            .collect();
        let vocab: Vec<serde_json::Value> = shape::merge_vocab(&shapes)
            .into_iter()
            .map(|e| serde_json::json!({ "name": e.name, "count": e.count, "local": e.local }))
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "files": items,
                "skipped_unparseable": skipped,
                "vocabulary": vocab,
            })
        );
        return Ok(match shapes.is_empty() {
            true => Status::Empty,
            false => Status::Found,
        });
    }

    if shapes.is_empty() {
        println!("ax shape: `{filter}` matched no parseable Rust file");
        return Ok(Status::Empty);
    }

    let totals = shapes
        .iter()
        .fold((0usize, 0usize, 0usize, 0usize), |(l, b, c, t), s| {
            (
                l + s.literals,
                b + s.branches,
                c + s.code_lines,
                t + s.test_lines,
            )
        });

    if args.has("--rows") {
        let mut rows: Vec<(&String, &shape::Slot)> = shapes
            .iter()
            .flat_map(|s| s.slots.iter())
            .filter(|(_, sl)| sl.writes > 1)
            .collect();
        // Worst first: the target that varies most is the one that decides
        // whether this file can be a table at all.
        rows.sort_by(|a, b| {
            (b.1.forms.len(), b.1.writes)
                .cmp(&(a.1.forms.len(), a.1.writes))
                .then_with(|| a.0.cmp(b.0))
        });
        println!(
            "{} recurring target(s); `forms` distinct right-hand-side shapes over `writes`:",
            rows.len()
        );
        println!("{:>6} {:>6}  {}", "forms", "writes", "target");
        rows.iter().take(args.limit(40)).for_each(|(name, sl)| {
            println!("{:>6} {:>6}  {}", sl.forms.len(), sl.writes, name);
        });
        return Ok(Status::Found);
    }

    if args.has("--vocab") {
        let vocab = shape::merge_vocab(&shapes);
        let sites: usize = vocab.iter().map(|e| e.count).sum();
        let local = vocab.iter().filter(|e| e.local).count();
        println!(
            "vocabulary of {} file(s): {} distinct callee(s) over {} call site(s); \
             {local} defined in the scanned set",
            shapes.len(),
            vocab.len(),
            sites,
        );
        vocab.iter().take(args.limit(60)).for_each(|e| {
            // `local` is the column that separates a domain verb from an
            // `Option` combinator. Without it, reading a vocabulary means
            // classifying every name by hand.
            let scope = ["", "local"][usize::from(e.local)];
            println!("  {:6}  {:<6} {}", e.count, scope, e.name);
        });
        return Ok(Status::Found);
    }

    println!(
        "{:>5} {:>7} {:>7} {:>6} {:>5} {:>7}  {:<9} {}",
        "lines", "lit/ln", "br/ln", "reuse", "rows", "frm/row", "verdict", "path"
    );
    shapes.iter().take(args.limit(40)).for_each(|s| {
        println!(
            "{:>5} {:>7.2} {:>7.3} {:>6.1} {:>5} {:>7}  {:<9} {}",
            s.code_lines,
            s.literal_density(),
            s.branch_density(),
            s.reuse(),
            s.row_count()
                .map_or_else(|| "-".to_owned(), |n| n.to_string()),
            s.form_ratio()
                .map_or_else(|| "  -".to_owned(), |r| format!("{r:.2}")),
            s.verdict().label(),
            s.path
        );
    });

    let data = shapes
        .iter()
        .filter(|s| s.verdict() == shape::Verdict::Data)
        .count();
    let algo = shapes
        .iter()
        .filter(|s| s.verdict() == shape::Verdict::Algorithm)
        .count();
    println!(
        "\n{} file(s), {} code lines: {} data-shaped, {} algorithm, {} mixed",
        shapes.len(),
        totals.2,
        data,
        algo,
        shapes.len() - data - algo
    );
    println!(
        "overall {:.2} literals/line, {:.3} branches/line ({} test line(s) excluded)",
        totals.0 as f64 / totals.2.max(1) as f64,
        totals.1 as f64 / totals.2.max(1) as f64,
        totals.3
    );
    if skipped > 0 {
        println!("{skipped} file(s) skipped: could not be parsed as Rust");
    }
    println!(
        "`--vocab` names the closed vocabulary; `--rows` breaks frm/row down per target."
    );
    println!(
        "rows    = how many rows the biggest repeated record has -- the N in the \
         Datafication Law's (N-1) x per-variant-code.\n\
         frm/row = what share of that record's fields vary in SHAPE, not just value. \
         0.00 means every field always has the same shape, whatever the row count; \
         1.00 means every row is its own shape.\n\
         Read them together: low frm/row says a table is POSSIBLE, high rows says it \
         is WORTH IT. Either alone misleads -- frm/row 0.00 at rows=3 saves nothing, \
         and a high row count at frm/row 0.84 is an algorithm. lit/ln and reuse say \
         only that content is PRESENT, not that it is addressable."
    );

    Ok(Status::Found)
}

fn cmd_sql(repo: &Repo, args: &Args, rec: &mut Record) -> Outcome {
    // The parquet half may hold nothing: `ax compact` may never have run, and a
    // ledger of raw NDJSON alone is the normal state early on. `read_parquet`
    // over an empty glob is an error rather than an empty relation, so the view
    let raw = duck_dir(&ledger::raw_dir(repo));
    let parquet_dir = ledger::parquet_dir(repo);
    let has_parquet = std::fs::read_dir(&parquet_dir)
        .map(|d| {
            d.flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "parquet"))
        })
        .unwrap_or(false);
    let parquet = duck_dir(&parquet_dir);
    let halves = match has_parquet {
        true => format!(
            "SELECT * FROM read_json_auto('{raw}/*.ndjson', union_by_name=true)\n  \
             UNION ALL BY NAME\n  SELECT * FROM read_parquet('{parquet}/*.parquet')"
        ),
        false => format!("SELECT * FROM read_json_auto('{raw}/*.ndjson', union_by_name=true)"),
    };
    let view = format!("CREATE OR REPLACE VIEW ledger AS {halves};");

    let Some(sql) = args.arg(0) else {
        print_sql_starters(&halves);
        return Ok(Status::Found);
    };
    rec.query = Some(sql.to_owned());

    // `-noheader` is deliberately NOT passed: a column name is most of what
    // makes a result readable, and every other `ax` command labels its output.
    let out = std::process::Command::new("duckdb")
        .args(["-box", "-c", &format!("{view}\n{sql}")])
        .output();
    let Ok(out) = out else {
        eprintln!("ax: `duckdb` is not on PATH — install it (scoop install duckdb) to run queries.");
        eprintln!("ax: the view and the starting cuts, to paste into one you have:");
        eprintln!();
        print_sql_starters(&halves);
        return Err(Failure::Failed("no duckdb on PATH".to_owned()));
    };
    print!("{}", String::from_utf8_lossy(&out.stdout));
    let stderr = String::from_utf8_lossy(&out.stderr);
    stderr.is_empty().then_some(()).map_or_else(
        || eprint!("{stderr}"),
        |()| (),
    );
    // Rows, not lines: `-box` frames the table, so the payload is what sits
    // between the header rule and the footer rule.
    let rows = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with('│'))
        .count()
        .saturating_sub(1);
    rec.hits = rows;
    Ok(status(rows))
}

/// The cuts worth starting from, printed when `ax sql` is given no query.
///
/// Printed rather than run because which one a caller wants is the whole
/// question; running all four would bury the answer in the other three.
fn print_sql_starters(halves: &str) {
    println!("-- Pass a query to run one:");
    println!("--   ax sql \"SELECT cmd, count(*) FROM ledger GROUP BY 1 ORDER BY 2 DESC\"");
    println!("--");
    println!("-- The `ledger` view is created for you:");
    println!("CREATE OR REPLACE VIEW ledger AS {halves};");
    println!();
    println!("-- What the repo could not answer, most-wanted first.");
    println!("SELECT query, count(*) AS misses, count(DISTINCT session) AS sessions");
    println!("  FROM ledger WHERE zero_result GROUP BY 1 ORDER BY 2 DESC LIMIT 40;");
    println!();
    println!("-- Open friction: what the TOOL could not do, minus what has been closed.");
    println!("SELECT f.id, any_value(f.query) AS what, count(*) AS hits");
    println!("  FROM ledger f WHERE f.cmd = 'friction' AND f.id NOT IN (");
    println!("    SELECT query FROM ledger WHERE cmd = 'resolve' AND ok");
    println!("  ) GROUP BY f.id ORDER BY 3 DESC;");
    println!();
    println!("-- Where the work actually lands.");
    println!("SELECT unnest(top_paths) AS path, count(*) AS touches");
    println!("  FROM ledger WHERE cmd IN ('edit', 'write', 'record') AND ok");
    println!("  GROUP BY 1 ORDER BY 2 DESC LIMIT 40;");
}

// ---------------------------------------------------------------------------

fn print_usage() {
    eprintln!(
        r"ax - the Axiom repo's query-and-change gateway

  Search
    ax q <regex> [--path RE] [--lang L] [--limit N] [-i] [-F] [--json]
    ax def <symbol> [--lang L]        where a symbol is defined
    ax refs <symbol> [--lang L]       every mention of a symbol
    ax file <regex> [--limit N]       find files by path
    ax cite <glob> [--baseline REV] [--source-root P] [--verdict K]
                   [--moved] [--limit N] [--json]
                                      resolve `foo.js:NNN` citations against
                                      HEAD and a baseline revision, and report
                                      how many still point where they claim.
                                      --verdict OK|PARTIAL|WRONG|OUT-OF-RANGE|
                                      UNRESOLVED-FILE|UNVERIFIABLE|ROTTED|
                                      WRONG-WHEN-WRITTEN|EXTERNAL-BASE
                                      --moved hunts the other source files for
                                      where a lost citation's content went

  Read and change (scoped to this repo, always)
    ax read <path> [--range A:B]
    ax edit <path> --replace <old> --with <new> [--all]
    ax write <path>                   content on stdin
    ax apply                          batch edits as JSON on stdin; all-or-nothing.
                                      Reach for this instead of writing a script.
                                      [{{path, replace, with, all}}]
                                      [{{path, from, to, with}}]  span replace
                                      [{{path, insert_before|insert_after, text}}]
                                      [{{path, append}}] [{{path, content}}]
                                      any op: text_file <path> supplies the
                                      payload from a file (no escaping needed)
    ax record <path> [--bytes N] [--tool T]   log a change made outside ax

  Architecture
    ax graph [<layer|module|app>]     deps, dependents and the laws in force
    ax owns <path>                    which package owns a file, and its rules

  The ledger
    ax friction <what-you-tried> [--want X] [--verdict tool|repo|unknown]
                                      log that ax itself fell short
    ax resolve <friction-id> [--by <what fixed it>]
                                      close a friction once its cause is fixed
    ax miss [--limit N] [--all]       what the repo, and the tool, could not do
    ax stats [--limit N]              what agents look for and change
    ax compact                        roll closed days into Parquet
    ax sql [query]                    run DuckDB over the whole ledger (engine
                                      is compiled in; no query prints the cuts
                                      worth starting from)

  --lang: rs ts js web toml md py shader json
  Exit: 0 found | 1 nothing found | 2 usage | 3 out-of-repo path | 4 failed
  Env:  AXIOM_ATLAS_AGENT, AXIOM_ATLAS_SESSION, AXIOM_ATLAS_NO_LEDGER"
    );
}
