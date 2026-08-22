//! Content search, using ripgrep's own walker and line searcher as libraries.
//!
//! Results are collected in full (up to a hard ceiling), then sorted by
//! `(path, line)` before truncation. Sorting rather than racing to a limit is
//! what makes two identical queries return byte-identical output — the same
//! determinism the engine itself is held to.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use grep_searcher::sinks;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::{WalkBuilder, WalkState};

use crate::repo::Repo;

/// Never collect more than this many hits, however broad the pattern.
const CEILING: usize = 50_000;

/// Directories that are never worth searching, regardless of gitignore state.
const ALWAYS_SKIP: &[&str] = &[".git", ".axiom-atlas", "target", "node_modules"];

#[derive(Debug, Clone)]
pub struct Hit {
    pub path: String,
    pub line: u64,
    pub text: String,
}

#[derive(Debug)]
pub struct Query {
    pub pattern: String,
    pub path_filter: Option<String>,
    pub lang: Option<String>,
    pub limit: usize,
    pub case_insensitive: bool,
    pub fixed: bool,
}

#[derive(Debug, Default)]
pub struct Outcome {
    /// Hits actually returned (already truncated to `limit`).
    pub hits: Vec<Hit>,
    /// Total hits found before truncation.
    pub total: usize,
    pub files_matched: usize,
    pub truncated: bool,
}

/// Maps a `--lang` token to the extensions it covers.
pub fn lang_extensions(lang: &str) -> Option<&'static [&'static str]> {
    match lang {
        "rs" | "rust" => Some(&["rs"]),
        "ts" | "typescript" => Some(&["ts", "tsx", "mts", "cts"]),
        "js" | "javascript" => Some(&["js", "jsx", "mjs", "cjs"]),
        "web" => Some(&["ts", "tsx", "js", "jsx", "mjs", "html", "css"]),
        "toml" => Some(&["toml"]),
        "md" | "markdown" => Some(&["md"]),
        "py" | "python" => Some(&["py"]),
        "shader" | "wgsl" => Some(&["wgsl", "glsl", "vert", "frag"]),
        "json" => Some(&["json"]),
        _ => None,
    }
}

/// Runs a search across the repo.
pub fn run(repo: &Repo, q: &Query) -> Result<Outcome, String> {
    let pattern = if q.fixed { regex::escape(&q.pattern) } else { q.pattern.clone() };

    let matcher = grep_regex::RegexMatcherBuilder::new()
        .case_insensitive(q.case_insensitive)
        .build(&pattern)
        .map_err(|e| format!("bad pattern `{}`: {e}", q.pattern))?;

    let path_re = q
        .path_filter
        .as_ref()
        .map(|f| regex::Regex::new(f).map_err(|e| format!("bad --path regex `{f}`: {e}")))
        .transpose()?;

    let exts: Option<&[&str]> = match q.lang.as_deref() {
        Some(l) => Some(lang_extensions(l).ok_or_else(|| {
            format!("unknown --lang `{l}` (try: rs, ts, js, web, toml, md, py, shader, json)")
        })?),
        None => None,
    };

    let hits: Mutex<Vec<Hit>> = Mutex::new(Vec::new());
    let files_matched = AtomicUsize::new(0);
    let total = AtomicUsize::new(0);

    let walker = WalkBuilder::new(&repo.root)
        .hidden(false) // .github/, .cargo/ and friends are real source
        .git_ignore(true)
        .git_global(false)
        .parents(false)
        .filter_entry(|e| !ALWAYS_SKIP.contains(&e.file_name().to_string_lossy().as_ref()))
        .threads(std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get))
        .build_parallel();

    // The per-thread closures borrow the shared sinks; `&Mutex`/`&Atomic` are
    // Copy, so each worker moves a reference rather than the value itself.
    let hits_ref = &hits;
    let files_ref = &files_matched;
    let total_ref = &total;

    walker.run(|| {
        let matcher = matcher.clone();
        let path_re = path_re.clone();
        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::quit(0))
            .line_number(true)
            .build();

        Box::new(move |entry| {
            let Ok(entry) = entry else { return WalkState::Continue };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                return WalkState::Continue;
            }
            let path = entry.path();

            if let Some(exts) = exts {
                let ok = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| exts.contains(&e));
                if !ok {
                    return WalkState::Continue;
                }
            }

            let rel = repo.rel(path);
            if let Some(re) = &path_re {
                if !re.is_match(&rel) {
                    return WalkState::Continue;
                }
            }

            if total_ref.load(Ordering::Relaxed) >= CEILING {
                return WalkState::Quit;
            }

            let mut local: Vec<Hit> = Vec::new();
            let sink = sinks::UTF8(|lnum, line| {
                local.push(Hit {
                    path: rel.clone(),
                    line: lnum,
                    text: line.trim_end().to_owned(),
                });
                Ok(local.len() < CEILING)
            });

            if searcher.search_path(&matcher, path, sink).is_ok() && !local.is_empty() {
                files_ref.fetch_add(1, Ordering::Relaxed);
                total_ref.fetch_add(local.len(), Ordering::Relaxed);
                if let Ok(mut guard) = hits_ref.lock() {
                    guard.extend(local);
                }
            }
            WalkState::Continue
        })
    });

    let mut hits = hits.into_inner().unwrap_or_default();
    hits.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));

    let total = hits.len();
    let truncated = total > q.limit;
    hits.truncate(q.limit);

    Ok(Outcome {
        hits,
        total,
        files_matched: files_matched.load(Ordering::Relaxed),
        truncated,
    })
}

/// Finds files whose repo-relative path matches `needle` (a regex).
pub fn find_files(repo: &Repo, needle: &str, limit: usize) -> Result<Vec<String>, String> {
    let re = regex::Regex::new(needle).map_err(|e| format!("bad pattern `{needle}`: {e}"))?;
    let found: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let found_ref = &found;

    WalkBuilder::new(&repo.root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .parents(false)
        .filter_entry(|e| !ALWAYS_SKIP.contains(&e.file_name().to_string_lossy().as_ref()))
        .build_parallel()
        .run(|| {
            let re = re.clone();
            Box::new(move |entry| {
                let Ok(entry) = entry else { return WalkState::Continue };
                if entry.file_type().is_some_and(|t| t.is_file()) {
                    let rel = repo.rel(entry.path());
                    if re.is_match(&rel) {
                        if let Ok(mut g) = found_ref.lock() {
                            g.push(rel);
                        }
                    }
                }
                WalkState::Continue
            })
        });

    let mut out = found.into_inner().unwrap_or_default();
    out.sort();
    out.truncate(limit);
    Ok(out)
}
