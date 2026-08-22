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
mod edit;
mod graph;
mod ledger;
mod repo;
mod search;
mod symbols;

use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::process::ExitCode;
use std::time::Instant;

use ledger::{Record, Scope};
use repo::Repo;

/// Flags that never take a value.
const BOOL_FLAGS: &[&str] = &[
    "--all", "--json", "-i", "--ignore-case", "-F", "--fixed", "--apply", "--help", "-h",
];

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

    let args = Args::parse(&argv[1..]);
    let started = Instant::now();
    let mut rec = Record::new(&cmd);

    let outcome = match cmd.as_str() {
        "q" | "search" => cmd_search(&repo, &args, &mut rec, None),
        "def" => cmd_symbol(&repo, &args, &mut rec, true),
        "refs" => cmd_symbol(&repo, &args, &mut rec, false),
        "file" | "files" => cmd_files(&repo, &args, &mut rec),
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
        "sql" => cmd_sql(&repo, &mut rec),
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
}

impl Args {
    fn parse(rest: &[String]) -> Self {
        let mut positional = Vec::new();
        let mut values = HashMap::new();
        let mut flags = HashSet::new();

        let mut i = 0;
        while i < rest.len() {
            let a = &rest[i];
            if a.starts_with('-') {
                if BOOL_FLAGS.contains(&a.as_str()) {
                    flags.insert(a.clone());
                    i += 1;
                } else if i + 1 < rest.len() {
                    values.insert(a.clone(), rest[i + 1].clone());
                    i += 2;
                } else {
                    flags.insert(a.clone());
                    i += 1;
                }
            } else {
                positional.push(a.clone());
                i += 1;
            }
        }
        Self { positional, values, flags }
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
    let pattern = if definition {
        symbols::definition_pattern(sym)
    } else {
        symbols::reference_pattern(sym)
    };
    cmd_search(repo, args, rec, Some(pattern))
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
    let path = resolve(repo, raw)?;
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

fn cmd_sql(repo: &Repo, _rec: &mut Record) -> Outcome {
    let raw = ledger::raw_dir(repo).display().to_string().replace('\\', "/");
    let parquet = ledger::parquet_dir(repo).display().to_string().replace('\\', "/");

    println!("-- Every ledger row, raw and compacted, as one relation.");
    println!("CREATE OR REPLACE VIEW ledger AS");
    println!("  SELECT * FROM read_json_auto('{raw}/*.ndjson', union_by_name=true)");
    println!("  UNION ALL BY NAME");
    println!("  SELECT * FROM read_parquet('{parquet}/*.parquet');");
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
    Ok(Status::Found)
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
    ax sql                            DuckDB queries over the whole ledger

  --lang: rs ts js web toml md py shader json
  Exit: 0 found | 1 nothing found | 2 usage | 3 out-of-repo path | 4 failed
  Env:  AXIOM_ATLAS_AGENT, AXIOM_ATLAS_SESSION, AXIOM_ATLAS_NO_LEDGER"
    );
}
