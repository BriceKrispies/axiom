//! `ax cite` — resolving `foo.js:NNN` citations against HEAD and a baseline.
//!
//! A port that documents each ported constant with the source line it came from
//! (`apps/axiom-shmup` carries ~3800 of them) is only as trustworthy as those
//! citations are. Checking one by hand costs a `git show` and a read; checking a
//! subsystem costs a day. This module does the whole corpus in one pass.
//!
//! # What it can decide, and what it cannot
//!
//! Two different questions hide inside "is this citation correct?", and they
//! have very different confidence:
//!
//! * **Does the target resolve?** — mechanical and certain. The file exists or
//!   it does not; the line is within EOF or it is not. `UNRESOLVED-FILE` and
//!   `OUT-OF-RANGE` are facts.
//! * **Does the cited line say what the citing code claims?** — a heuristic.
//!   The only evidence available is what the Rust doc comment quotes: backticked
//!   identifiers, numeric literals, and the literals in the item it documents.
//!   Those are searched for in the target; a citation is *corroborated* when
//!   they land inside the cited range.
//!
//! The second question is never answered with a bare boolean. A citation whose
//! doc quotes nothing findable is reported `UNVERIFIABLE` and kept **out of the
//! accuracy denominator** rather than counted as a pass — an over-confident
//! classifier here would launder exactly the problem this exists to expose.
//!
//! # Why the test scales with the citation's width
//!
//! `shells.js:20` and `impacts.js:332-410` make different claims. The first is
//! precise: everything the doc quotes must be on that line. The second delimits
//! a section, and a section legitimately mentions helpers defined elsewhere. So
//! a narrow citation is held to strict containment, and a wide one is allowed a
//! two-line pad above its start — which is where a JSDoc header sits.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::repo::Repo;

/// An evidence token appearing on more lines than this cannot localize
/// anything, so it is dropped rather than allowed to corroborate by accident.
const MAX_ANCHOR_LINES: usize = 10;

/// A citation wider than this is read as delimiting a section rather than
/// naming a line, and gets [`WIDE_PAD`] lines of slack above its start.
const NARROW_SPAN: u32 = 8;

/// Lines of slack above a wide citation's start — the JSDoc header above the
/// function it names.
const WIDE_PAD: u32 = 2;

/// Directories never worth walking, matching `search.rs`.
const ALWAYS_SKIP: &[&str] = &[".git", ".axiom-atlas", "target", "node_modules"];

/// Extensions a citation may name.
const SOURCE_EXTS: &[&str] = &["js", "mjs", "cjs", "jsx", "ts"];

/// Words that carry no evidence — Rust keywords, primitives, and the filler
/// that shows up inside backticks in prose.
const STOP: &[&str] = &[
    "self", "none", "some", "true", "false", "the", "and", "for", "this", "that", "with", "from",
    "into", "new", "let", "const", "var", "function", "return", "type", "impl", "pub", "fn",
    "struct", "enum", "crate", "super", "mut", "usize", "isize", "f32", "f64", "u8", "u16", "u32",
    "u64", "i8", "i16", "i32", "i64", "bool", "str", "string", "vec", "option", "result", "dyn",
    "ref", "where", "match", "else", "loop", "while", "await", "async", "move", "box",
];

// ---------------------------------------------------------------------------
// Request and results
// ---------------------------------------------------------------------------

pub struct Request {
    /// Glob (`apps/x/src/fx/**`) or regex over repo-relative paths.
    pub pattern: String,
    /// Revision to also resolve against, e.g. the commit the port was taken at.
    pub baseline: Option<String>,
    /// Explicit root the cited paths resolve under. Derived per-file when absent.
    pub source_root: Option<String>,
    /// Print at most this many citation rows (the summary is always complete).
    pub limit: usize,
    /// Show only rows of this class.
    pub only: Option<String>,
    /// Hunt other source files for where a contradicted citation's content went.
    pub moved: bool,
}

/// What a single revision has to say about one citation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// Every discriminating token the citing doc quotes is inside the range.
    Confirmed,
    /// Some are inside, some elsewhere in the file.
    Partial { covered: usize, total: usize },
    /// None are inside, but they exist elsewhere in the file.
    Contradicted { covered: usize, total: usize },
    /// The cited line is past end-of-file.
    OutOfRange { eof: u32 },
    /// No such file at this revision.
    Unresolved,
    /// The citing doc quotes nothing this file contains — undecidable.
    Uncorroborated,
}

impl Judgement {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Confirmed => "OK",
            Self::Partial { .. } => "PARTIAL",
            Self::Contradicted { .. } => "WRONG",
            Self::OutOfRange { .. } => "OUT-OF-RANGE",
            Self::Unresolved => "UNRESOLVED-FILE",
            Self::Uncorroborated => "UNVERIFIABLE",
        }
    }

    /// Whether this judgement counts toward the accuracy rate's denominator.
    ///
    /// Two kinds of citation are undecidable rather than wrong, and counting
    /// either as a failure would overstate the corpus's rot: one whose doc
    /// quotes nothing findable, and one whose target this repo does not contain
    /// at all (the port cites `three/src/core/BufferGeometry.js`, a vendored
    /// library that was never checked in). Both are reported in full; neither
    /// is scored.
    pub fn decidable(&self) -> bool {
        !matches!(self, Self::Uncorroborated | Self::Unresolved)
    }

    pub fn ok(&self) -> bool {
        matches!(self, Self::Confirmed)
    }

    /// Confidence in the judgement, 0..1. Mechanical facts are 1.0; a
    /// corroboration verdict is the fraction of evidence that agreed with it.
    pub fn confidence(&self) -> f64 {
        match self {
            Self::Confirmed | Self::OutOfRange { .. } | Self::Unresolved => 1.0,
            Self::Uncorroborated => 0.0,
            Self::Partial { covered, total } => *covered as f64 / *total as f64,
            Self::Contradicted { covered: _, total } => 1.0 - 1.0 / (*total as f64 + 1.0),
        }
    }
}

/// One citation, judged.
pub struct Row {
    /// Citing file, repo-relative.
    pub file: String,
    /// Line the citation is written on, 1-based.
    pub line: u32,
    /// The citation exactly as written, e.g. `shells.js:20`.
    pub raw: String,
    /// Where the file part resolved to, repo-relative — reported rather than
    /// assumed, because some citations name a different file than they mean.
    pub target: Option<String>,
    /// True when the basename matched several source files.
    pub ambiguous: bool,
    /// The out-of-checkout directory this citation resolves against, if any.
    pub external_base: Option<String>,
    pub ranges: Vec<(u32, u32)>,
    pub head: Judgement,
    pub base: Option<Judgement>,
    /// The cited line's text at HEAD (the first cited line).
    pub head_text: Option<String>,
    /// The cited line's text at the baseline.
    pub base_text: Option<String>,
    /// Where the quoted evidence actually sits at HEAD, when it is not in range.
    pub suggest: Option<u32>,
    /// `file:line` in a *different* source file that is a better home (`--moved`).
    pub moved_to: Option<String>,
    /// How many discriminating tokens the citing doc offered.
    pub anchors: usize,
}

impl Row {
    /// The headline class: the mechanical facts win, then corroboration.
    pub fn class(&self) -> &'static str {
        self.head.label()
    }

    /// Did it rot, or was it wrong the day it was typed? `None` without a
    /// baseline, and for a citation that is fine at HEAD.
    pub fn history(&self) -> Option<&'static str> {
        let base = self.base.as_ref()?;
        match (self.head.ok(), base.ok(), self.head.decidable()) {
            (true, _, _) => None,
            (false, _, false) => None,
            (false, true, _) => Some("ROTTED"),
            (false, false, _) => Some("WRONG-WHEN-WRITTEN"),
        }
    }
}

#[derive(Default, Clone)]
pub struct Tally {
    pub total: usize,
    pub ok: usize,
    pub partial: usize,
    pub wrong: usize,
    pub out_of_range: usize,
    pub unresolved: usize,
    pub unverifiable: usize,
}

impl Tally {
    fn add(&mut self, j: &Judgement) {
        self.total += 1;
        match j {
            Judgement::Confirmed => self.ok += 1,
            Judgement::Partial { .. } => self.partial += 1,
            Judgement::Contradicted { .. } => self.wrong += 1,
            Judgement::OutOfRange { .. } => self.out_of_range += 1,
            Judgement::Unresolved => self.unresolved += 1,
            Judgement::Uncorroborated => self.unverifiable += 1,
        }
    }

    pub fn decidable(&self) -> usize {
        self.total - self.unverifiable - self.unresolved
    }

    /// Confirmed over decidable — the lower bound on accuracy.
    pub fn accuracy(&self) -> f64 {
        let d = self.decidable();
        (d > 0).then(|| self.ok as f64 / d as f64).unwrap_or(0.0)
    }

    /// Confirmed-or-partial over decidable — the upper bound.
    pub fn accuracy_upper(&self) -> f64 {
        let d = self.decidable();
        (d > 0)
            .then(|| (self.ok + self.partial) as f64 / d as f64)
            .unwrap_or(0.0)
    }
}

pub struct FileStat {
    pub file: String,
    pub head: Tally,
    pub base: Tally,
    /// Median signed offset between where the evidence actually is and where
    /// the citation points. A consistent nonzero drift across a whole file is
    /// the signature of citing an out-of-tree source root.
    pub drift: Option<i64>,
    pub drift_samples: usize,
    /// The most common offsets, largest first. A file repaired by one shift
    /// shows one mode; `physics/system.rs` shows two (`+1` below line 242,
    /// `+15` above it) and is still mechanically repairable; a file whose
    /// offsets scatter has no shift that fixes it and is not.
    pub drift_modes: Vec<(i64, usize)>,
    /// Citations here written against a base outside this checkout.
    pub external: usize,
}

pub struct Report {
    pub rows: Vec<Row>,
    pub per_file: Vec<FileStat>,
    pub head: Tally,
    pub base: Tally,
    pub baseline: Option<String>,
    pub rotted: usize,
    pub wrong_when_written: usize,
    pub files_scanned: usize,
    /// Citations written against a base outside this checkout.
    pub external: usize,
    /// Every distinct out-of-checkout base, with how many citations use it.
    pub external_bases: Vec<(String, usize)>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Why a run could not start. The distinction is the exit code: a bad pattern
/// is usage (2), a `--source-root` outside the checkout is a refusal (3) —
/// the same contract every other `ax` command keeps, and the one guarantee the
/// whole tool rests on.
pub enum CiteError {
    Usage(String),
    Refused(String),
}

pub fn run(repo: &Repo, req: &Request) -> Result<Report, CiteError> {
    // One pattern language for the whole tool. `cite` used to decide glob vs
    // regex by looking for a `*`, which silently mangled every regex that had
    // one into a glob matching nothing. See `crate::pattern`.
    let selector = crate::pattern::PathPattern::parse(&req.pattern).map_err(CiteError::Usage)?;
    let (files, kind) = selector.select(walk_all(repo));
    selector.note(kind).map(|n| eprintln!("ax cite: {n}"));
    files.is_empty().then(|| eprintln!("ax cite: {}", selector.empty_note()));

    let explicit_root = req
        .source_root
        .as_ref()
        .map(|raw| {
            repo.resolve_read(raw)
                .map_err(|e| CiteError::Refused(e.to_string()))
                .and_then(|p| {
                    p.is_dir().then_some(p).ok_or_else(|| {
                        CiteError::Usage(format!("--source-root `{raw}` is not a directory"))
                    })
                })
        })
        .transpose()?;

    let mut sources = SourceIndex::new(repo, explicit_root);
    let mut rows: Vec<Row> = Vec::new();

    for file in &files {
        let abs = repo.root.join(file);
        let Ok(text) = read_text(&abs) else { continue };
        let lines: Vec<&str> = text.lines().collect();
        for cite in parse_file(&lines) {
            rows.push(judge(&mut sources, file, &lines, &cite, req));
        }
    }

    Ok(summarize(rows, req, files.len()))
}

// ---------------------------------------------------------------------------
// The grammar
// ---------------------------------------------------------------------------

/// One citation lifted out of a source file, before anything is resolved.
#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    /// 0-based index of the line it was written on.
    pub idx: usize,
    /// Byte offset the citation starts at within that line.
    pub start: usize,
    /// The directory an absolute citation names, when the base sits outside
    /// this checkout — `C:/dev/Claude-of-Duty/src/ui`. A corpus that answers to
    /// two citation bases at once cannot be mechanically repaired, so this is
    /// reported in its own right rather than folded into the verdict.
    pub external_base: Option<String>,
    /// The file part exactly as written (`shells.js`, `src/fx/shells.js`).
    pub name: String,
    /// The citation as written, file part included.
    pub raw: String,
    /// Every `N` or `N-M` in the spec, in order.
    pub ranges: Vec<(u32, u32)>,
}

/// Finds every `name.js:SPEC` in a file.
///
/// The corpus uses one grammar with several shapes:
/// `foo.js:123`, `foo.js:123-145`, `path/to/foo.js:123`,
/// `foo.js:71,73,135` and `foo.js:64, 89-90` (comma lists, spaced or not), and
/// — rarely — a list that wraps onto the next comment line, either after a
/// trailing comma or with the file part left dangling at end of line.
pub fn parse_file(lines: &[&str]) -> Vec<Parsed> {
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        // A citation may be split by a line wrap in two ways: the numbers may
        // continue after a trailing comma, or the whole spec may sit on the
        // next line with only `foo.js:` left behind. Both are handled by
        // splicing the next line's comment body on before scanning.
        let joined = lines
            .get(idx + 1)
            .filter(|_| wraps(line))
            .map(|next| format!("{line}{}", strip_comment_marker(next)));
        let scan = joined.as_deref().unwrap_or(line);
        // A citation is attributed to the line it STARTS on. Without this a
        // spliced line would harvest the next line's own citations a second
        // time, and every wrapped comment would inflate the corpus.
        out.extend(parse_line(idx, scan).into_iter().filter(|p| p.start < line.len()));
    }
    out
}

/// Does this line end mid-citation?
fn wraps(line: &str) -> bool {
    let t = line.trim_end();
    // `... foo.js:` with the spec on the next line.
    let dangling = t.rfind(':').is_some_and(|c| {
        let head = &t[..c];
        c + 1 == t.len() && SOURCE_EXTS.iter().any(|e| head.ends_with(&format!(".{e}")))
    });
    // `... foo.js:104-125,` continuing after the comma.
    let trailing_comma = t.ends_with(',') && has_citation(t);
    dangling || trailing_comma
}

fn has_citation(s: &str) -> bool {
    !parse_line(0, s).is_empty()
}

fn strip_comment_marker(line: &str) -> &str {
    let t = line.trim_start();
    ["///", "//!", "//", "*/", "*"]
        .iter()
        .find_map(|m| t.strip_prefix(m))
        .unwrap_or(t)
        .trim_start()
}

/// Scans one (possibly spliced) line for citations.
fn parse_line(idx: usize, line: &str) -> Vec<Parsed> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != b':' {
            i += 1;
            continue;
        }
        // Walk back over the file part: `[A-Za-z0-9_./-]*\.<ext>`.
        let mut s = i;
        while s > 0 && is_path_byte(b[s - 1]) {
            s -= 1;
        }
        let name = &line[s..i];
        let known_ext = name
            .rsplit_once('.')
            .is_some_and(|(_, e)| SOURCE_EXTS.contains(&e));
        if !known_ext {
            i += 1;
            continue;
        }
        // Then forward over the spec.
        let (ranges, end) = parse_spec(line, i + 1);
        if ranges.is_empty() {
            i += 1;
            continue;
        }
        out.push(Parsed {
            idx,
            start: s,
            external_base: external_base(line, s, name),
            name: name.to_owned(),
            raw: line[s..end].to_owned(),
            ranges,
        });
        i = end;
    }
    out
}

/// The out-of-checkout directory an absolute citation names, if it is one.
///
/// The walk-back over the file part stops at the drive letter's colon, so
/// `C:/dev/Claude-of-Duty/src/ui/hud.js:12` arrives here as `/dev/.../hud.js`
/// with `C:` left behind. Recovering it is what makes the second citation base
/// visible instead of silently resolving to the in-repo twin.
fn external_base(line: &str, s: usize, name: &str) -> Option<String> {
    let b = line.as_bytes();
    let drive = (s >= 2 && b[s - 1] == b':' && b[s - 2].is_ascii_alphabetic())
        .then(|| format!("{}:", b[s - 2] as char));
    let absolute = drive.is_some() || name.starts_with('/');
    absolute
        .then(|| {
            let full = format!("{}{name}", drive.unwrap_or_default());
            full.rsplit_once('/').map(|(dir, _)| dir.to_owned())
        })
        .flatten()
}

fn is_path_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'/' | b'-')
}

/// Parses `N`, `N-M`, and comma-separated lists of either, returning where the
/// spec ended so the scan can resume past it.
fn parse_spec(line: &str, from: usize) -> (Vec<(u32, u32)>, usize) {
    let b = line.as_bytes();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    let mut i = from;
    loop {
        let (lo, after) = read_num(b, i);
        let Some(lo) = lo else { break };
        let (hi, after) = match b.get(after) {
            Some(b'-') => match read_num(b, after + 1) {
                (Some(h), a) => (h, a),
                (None, _) => (lo, after),
            },
            _ => (lo, after),
        };
        ranges.push((lo, hi.max(lo)));
        i = after;
        // A comma (optionally spaced) continues the list, but only if a digit
        // follows — otherwise the comma belongs to the surrounding prose.
        let mut j = i;
        if b.get(j) == Some(&b',') {
            j += 1;
            while b.get(j) == Some(&b' ') {
                j += 1;
            }
            if b.get(j).is_some_and(u8::is_ascii_digit) {
                i = j;
                continue;
            }
        }
        break;
    }
    (ranges, i)
}

fn read_num(b: &[u8], from: usize) -> (Option<u32>, usize) {
    let mut i = from;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    match i > from {
        true => (std::str::from_utf8(&b[from..i]).ok().and_then(|s| s.parse().ok()), i),
        false => (None, from),
    }
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

/// What the citing code quotes that could also appear in the source.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Evidence {
    /// Identifiers, normalized (lowercased, `_` removed) so `CASE_LEN` in the
    /// source matches `case_len` in the port.
    pub idents: BTreeSet<String>,
    /// Numeric literals, compared verbatim.
    pub numbers: BTreeSet<String>,
}

/// The context a citation makes its claim in: the contiguous comment block it
/// sits in, plus — for a `///` doc comment — the item it documents.
///
/// A fixed line window would be wrong here: four consecutive one-line `///`
/// blocks each documenting a different constant would pool their evidence and
/// corroborate each other's citations by accident. That is not a hypothetical;
/// it is what made a first cut of this classifier score a demonstrably drifted
/// file at 68%.
pub fn context<'a>(lines: &[&'a str], idx: usize) -> Vec<&'a str> {
    let marker = ["///", "//!", "//"]
        .into_iter()
        .find(|m| lines[idx].trim_start().starts_with(m));

    let Some(marker) = marker else {
        // Not a line comment (a `/* */` banner, or a citation on a code line):
        // take the line itself and what immediately follows.
        return lines[idx..(idx + 3).min(lines.len())].to_vec();
    };

    let same = |l: &str| l.trim_start().starts_with(marker);
    let mut a = idx;
    while a > 0 && same(lines[a - 1]) {
        a -= 1;
    }
    let mut b = idx;
    while b + 1 < lines.len() && same(lines[b + 1]) {
        b += 1;
    }
    let mut out: Vec<&str> = lines[a..=b].to_vec();

    if marker == "///" {
        // The documented item. Attributes are skipped; the run stops at the
        // first blank line or after the signature's `{`/`;`.
        let mut j = b + 1;
        let mut taken = 0;
        while j < lines.len() && taken < 4 {
            let s = lines[j].trim();
            if s.starts_with("#[") {
                j += 1;
                continue;
            }
            if s.is_empty() {
                break;
            }
            out.push(lines[j]);
            taken += 1;
            j += 1;
            if s.ends_with('{') || s.ends_with(';') {
                break;
            }
        }
    }
    out
}

/// Pulls the checkable tokens out of a citation's context.
///
/// Backticked spans are the strongest signal — in this corpus a backtick is
/// nearly always a verbatim quote of the source. Prose words are deliberately
/// *not* used: they inflate the anchor set with words that appear all over a
/// file and make the verdict less honest, not more.
pub fn evidence(ctx: &[&str]) -> Evidence {
    let mut ev = Evidence::default();
    for raw in ctx {
        let is_comment = ["///", "//!", "//"]
            .iter()
            .any(|m| raw.trim_start().starts_with(m));
        let body = strip_comment_marker(raw);
        let mut rest = String::new();
        let mut in_tick = false;
        let mut span = String::new();
        for ch in body.chars() {
            match (ch == '`', in_tick) {
                (true, false) => in_tick = true,
                (true, true) => {
                    harvest_tick(&span, &mut ev);
                    span.clear();
                    in_tick = false;
                }
                (false, true) => span.push(ch),
                (false, false) => rest.push(ch),
            }
        }
        // An unterminated span (the tick pair wrapped across lines) still counts.
        harvest_tick(&span, &mut ev);

        // Outside backticks: numbers always; identifiers only from real code,
        // where a bare name is a name rather than a word in a sentence.
        harvest_numbers(&rest, &mut ev);
        if !is_comment {
            harvest_idents(&rest, &mut ev);
        }
    }
    ev
}

fn harvest_tick(span: &str, ev: &mut Evidence) {
    // A span that is itself a citation, or a Rust path, quotes nothing about
    // the source.
    let is_cite = SOURCE_EXTS
        .iter()
        .any(|e| span.contains(&format!(".{e}:")));
    if is_cite || span.contains("crate::") {
        return;
    }
    harvest_idents(span, ev);
    harvest_numbers(span, ev);
}

fn harvest_idents(s: &str, ev: &mut Evidence) {
    let mut cur = String::new();
    let flush = |cur: &mut String, ev: &mut Evidence| {
        let starts_ok = cur.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if cur.len() >= 3 && starts_ok {
            let n = normalize(cur);
            if !STOP.contains(&n.as_str()) {
                ev.idents.insert(n);
            }
        }
        cur.clear();
    };
    for ch in s.chars() {
        match ch.is_ascii_alphanumeric() || ch == '_' {
            true => cur.push(ch),
            false => flush(&mut cur, ev),
        }
    }
    flush(&mut cur, ev);
}

/// Decimals, hex literals, and integers of two digits or more. A bare `0`, `1`
/// or `2` corroborates nothing — it appears on most lines of most files.
fn harvest_numbers(s: &str, ev: &mut Evidence) {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let hex = i + 1 < b.len() && b[i] == '0' && (b[i + 1] == 'x' || b[i + 1] == 'X');
        if hex {
            i += 2;
            while i < b.len() && b[i].is_ascii_hexdigit() {
                i += 1;
            }
        } else {
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i < b.len() && b[i] == '.' && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        let tok: String = b[start..i].iter().collect();
        if tok.contains('.') || tok.starts_with("0x") || tok.starts_with("0X") || tok.len() >= 2 {
            ev.numbers.insert(tok);
        }
    }
}

pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

// ---------------------------------------------------------------------------
// Corroboration
// ---------------------------------------------------------------------------

/// A source file, with each line pre-normalized for identifier matching.
struct SourceText {
    raw: Vec<String>,
    lower: Vec<String>,
}

impl SourceText {
    fn new(text: &str) -> Self {
        let raw: Vec<String> = text.lines().map(str::to_owned).collect();
        let lower = raw.iter().map(|l| normalize(l)).collect();
        Self { raw, lower }
    }

    fn len(&self) -> u32 {
        self.raw.len() as u32
    }

    /// Lines (1-based) each evidence token appears on, dropping tokens that are
    /// absent and tokens too common to localize anything.
    fn anchors(&self, ev: &Evidence) -> Vec<Vec<u32>> {
        let mut out = Vec::new();
        let mut push = |lines: Vec<u32>| {
            if !lines.is_empty() && lines.len() <= MAX_ANCHOR_LINES {
                out.push(lines);
            }
        };
        for id in &ev.idents {
            push(
                self.lower
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.contains(id.as_str()))
                    .map(|(i, _)| i as u32 + 1)
                    .collect(),
            );
        }
        for n in &ev.numbers {
            push(
                self.raw
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.contains(n.as_str()))
                    .map(|(i, _)| i as u32 + 1)
                    .collect(),
            );
        }
        out
    }
}

/// Judges one citation against one revision of its target.
pub fn corroborate(src: &SourceTextView<'_>, ranges: &[(u32, u32)], ev: &Evidence) -> (Judgement, Option<u32>) {
    let lo = ranges.iter().map(|r| r.0).min().unwrap_or(1);
    let hi = ranges.iter().map(|r| r.1).max().unwrap_or(1);
    if hi > src.len() {
        return (Judgement::OutOfRange { eof: src.len() }, None);
    }
    let anchors = src.anchors(ev);
    if anchors.is_empty() {
        return (Judgement::Uncorroborated, None);
    }

    let pad = ((hi - lo + 1) > NARROW_SPAN).then_some(WIDE_PAD).unwrap_or(0);
    let covered: BTreeSet<u32> = ranges
        .iter()
        .flat_map(|(a, b)| (a.saturating_sub(pad).max(1))..=*b)
        .collect();

    let inside = anchors
        .iter()
        .filter(|lines| lines.iter().any(|l| covered.contains(l)))
        .count();
    let total = anchors.len();

    if inside == total {
        return (Judgement::Confirmed, None);
    }

    // Where the evidence actually clusters, outside the cited range. Ties break
    // toward the cited line, so a one-line drift reports as a one-line drift
    // rather than as a call site four hundred lines away.
    let mut score: BTreeMap<u32, usize> = BTreeMap::new();
    for lines in &anchors {
        for l in lines {
            if !covered.contains(l) {
                *score.entry(*l).or_default() += 1;
            }
        }
    }
    let best = score.values().copied().max().unwrap_or(0);
    let suggest = score
        .iter()
        .filter(|(_, v)| **v == best)
        .map(|(k, _)| *k)
        .min_by_key(|l| l.abs_diff(lo));

    let j = match inside {
        0 => Judgement::Contradicted { covered: 0, total },
        n => Judgement::Partial { covered: n, total },
    };
    (j, suggest)
}

/// A borrowed view so `corroborate` can be called on either revision's text.
pub struct SourceTextView<'a> {
    inner: &'a SourceText,
}

impl SourceTextView<'_> {
    fn len(&self) -> u32 {
        self.inner.len()
    }
    fn anchors(&self, ev: &Evidence) -> Vec<Vec<u32>> {
        self.inner.anchors(ev)
    }
    fn line(&self, n: u32) -> Option<String> {
        self.inner
            .raw
            .get(n as usize - 1)
            .map(|l| l.trim().to_owned())
    }
}

// ---------------------------------------------------------------------------
// Resolving the file part
// ---------------------------------------------------------------------------

/// Every source file reachable under a root, indexed by basename, plus the
/// text of each at HEAD and at the baseline.
struct SourceIndex<'a> {
    repo: &'a Repo,
    explicit_root: Option<PathBuf>,
    /// root -> basename -> repo-relative paths.
    by_root: HashMap<PathBuf, HashMap<String, Vec<String>>>,
    /// (repo-relative path, revision) -> text. `None` revision is HEAD.
    text: HashMap<(String, Option<String>), Option<SourceText>>,
}

impl<'a> SourceIndex<'a> {
    fn new(repo: &'a Repo, explicit_root: Option<PathBuf>) -> Self {
        Self {
            repo,
            explicit_root,
            by_root: HashMap::new(),
            text: HashMap::new(),
        }
    }

    /// The root a citation in `citing` resolves under.
    ///
    /// `--source-root` wins. Otherwise it is derived from the port's own path:
    /// `apps/axiom-shmup/src/fx/shells.rs` reads its source from
    /// `apps/shmup/src` — the same app name without the `axiom-` prefix. That
    /// convention is what a port in this repo already follows, so deriving it
    /// beats making every caller pass it.
    fn root_for(&self, citing: &str) -> Option<PathBuf> {
        if let Some(r) = &self.explicit_root {
            return Some(r.clone());
        }
        let parts: Vec<&str> = citing.split('/').collect();
        let app = parts
            .iter()
            .position(|p| *p == "apps")
            .and_then(|i| parts.get(i + 1))?;
        let bare = app.strip_prefix("axiom-").unwrap_or(app);
        let base = self.repo.root.join("apps").join(bare);
        let src = base.join("src");
        src.is_dir()
            .then_some(src)
            .or_else(|| base.is_dir().then_some(base))
    }

    fn index(&mut self, root: &Path) -> &HashMap<String, Vec<String>> {
        self.by_root.entry(root.to_path_buf()).or_insert_with(|| {
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            // Sources live under the root, but a citation may also name a
            // sibling of it (`tools/profile.mjs` next to `src/`), so the walk
            // starts one level up when that stays inside the repo.
            let start = root
                .parent()
                .filter(|p| p.starts_with(&self.repo.root) && *p != self.repo.root)
                .unwrap_or(root);
            for entry in WalkBuilder::new(start)
                .hidden(false)
                .git_ignore(true)
                .parents(false)
                .filter_entry(|e| !ALWAYS_SKIP.contains(&e.file_name().to_string_lossy().as_ref()))
                .build()
                .flatten()
            {
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                let ok = entry
                    .path()
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| SOURCE_EXTS.contains(&e));
                if !ok {
                    continue;
                }
                let rel = self.repo.rel(entry.path());
                let base = rel.rsplit('/').next().unwrap_or(&rel).to_owned();
                map.entry(base).or_default().push(rel);
            }
            for v in map.values_mut() {
                v.sort();
            }
            map
        })
    }

    /// Resolves a citation's file part to a repo-relative path.
    ///
    /// Returns `(path, ambiguous)`. A bare basename with several candidates is
    /// resolved by the citing file's own directory — `fx/shells.rs` citing
    /// `index.js` means `fx/index.js`, not one of the other eight — and flagged
    /// when even that leaves a choice.
    fn resolve(&mut self, citing: &str, name: &str) -> Option<(String, bool)> {
        let root = self.root_for(citing)?;
        let root_rel = self.repo.rel(&root);
        let cleaned = name.trim_start_matches("./");
        let cleaned = cleaned.strip_prefix("src/").unwrap_or(cleaned);

        if cleaned.contains('/') {
            let direct = format!("{root_rel}/{cleaned}");
            if self.repo.root.join(&direct).is_file() {
                return Some((direct, false));
            }
            // A path relative to the app root rather than its `src/`.
            if let Some(parent) = root_rel.rsplit_once('/').map(|(a, _)| a) {
                let sibling = format!("{parent}/{cleaned}");
                if self.repo.root.join(&sibling).is_file() {
                    return Some((sibling, false));
                }
            }
        }

        let base = cleaned.rsplit('/').next()?.to_owned();
        let index = self.index(&root);
        let cands = index.get(&base)?.clone();
        match cands.len() {
            0 => None,
            1 => Some((cands[0].clone(), false)),
            _ => Some(prefer_candidate(&root_rel, citing, &base, &cands)),
        }
    }

    /// The text of a source file at a revision.    /// The text of a source file at a revision. `None` revision is the worktree.
    fn text(&mut self, path: &str, rev: Option<&str>) -> Option<&SourceText> {
        let key = (path.to_owned(), rev.map(str::to_owned));
        if !self.text.contains_key(&key) {
            let loaded = match rev {
                None => read_text(&self.repo.root.join(path)).ok().map(|t| SourceText::new(&t)),
                Some(r) => crate::repo::git_show(&self.repo.root, r, path).map(|t| SourceText::new(&t)),
            };
            self.text.insert(key.clone(), loaded);
        }
        self.text.get(&key).and_then(Option::as_ref)
    }
}

/// Chooses between several source files sharing a basename, by the citing
/// file's own directory and then every parent of it.
///
/// Returns `(path, ambiguous)`. The exact-directory test alone is not enough,
/// and picking the alphabetically-first candidate when it fails is not a
/// tie-break, it is a coin flip: `weapons/parts/barrel.rs` cites `parts.js`
/// meaning `weapons/parts.js` and got `ai/parts.js` — 85 citations in one
/// subsystem judged against the wrong file, which is worse than not judging
/// them. The port nests deeper than its source, so the chain has to be walked.
pub fn prefer_candidate(
    root_rel: &str,
    citing: &str,
    base: &str,
    cands: &[String],
) -> (String, bool) {
    let sub = citing
        .rsplit_once('/')
        .map(|(d, _)| d)
        .and_then(|d| d.strip_prefix(&format!("{root_rel}/")).map(str::to_owned))
        .or_else(|| {
            // The port and the source share a layout below `src/`.
            let cd = citing.rsplit_once('/')?.0;
            let i = cd.find("/src/")?;
            Some(cd[i + 5..].to_owned())
        })
        .unwrap_or_default();

    let mut probe = Some(sub);
    while let Some(p) = probe {
        let want = match p.is_empty() {
            true => format!("{root_rel}/{base}"),
            false => format!("{root_rel}/{p}/{base}"),
        };
        if let Some(hit) = cands.iter().find(|c| **c == want) {
            return (hit.clone(), false);
        }
        probe = match p.rsplit_once('/') {
            Some((parent, _)) => Some(parent.to_owned()),
            None if p.is_empty() => None,
            None => Some(String::new()),
        };
    }
    (cands.first().cloned().unwrap_or_default(), true)
}

// ---------------------------------------------------------------------------
// Judging one citation
// ---------------------------------------------------------------------------

fn judge(
    sources: &mut SourceIndex<'_>,
    citing: &str,
    lines: &[&str],
    cite: &Parsed,
    req: &Request,
) -> Row {
    let ctx = context(lines, cite.idx);
    let ev = evidence(&ctx);
    let resolved = sources.resolve(citing, &cite.name);

    let mut row = Row {
        file: citing.to_owned(),
        line: cite.idx as u32 + 1,
        raw: cite.raw.clone(),
        target: resolved.as_ref().map(|(p, _)| p.clone()),
        ambiguous: resolved.as_ref().is_some_and(|(_, a)| *a),
        external_base: cite.external_base.clone(),
        ranges: cite.ranges.clone(),
        head: Judgement::Unresolved,
        base: None,
        head_text: None,
        base_text: None,
        suggest: None,
        moved_to: None,
        anchors: 0,
    };

    let Some((target, _)) = resolved else {
        row.base = req.baseline.as_ref().map(|_| Judgement::Unresolved);
        row.moved_to = req
            .moved
            .then(|| find_moved(sources, citing, "", &ev))
            .flatten();
        return row;
    };
    let first = cite.ranges.first().map(|r| r.0).unwrap_or(1);

    // HEAD.
    match sources.text(&target, None) {
        None => row.head = Judgement::Unresolved,
        Some(src) => {
            let view = SourceTextView { inner: src };
            row.anchors = view.anchors(&ev).len();
            row.head_text = view.line(first);
            let (j, s) = corroborate(&view, &cite.ranges, &ev);
            row.head = j;
            row.suggest = s;
        }
    }

    // Baseline.
    if let Some(rev) = &req.baseline {
        match sources.text(&target, Some(rev)) {
            None => row.base = Some(Judgement::Unresolved),
            Some(src) => {
                let view = SourceTextView { inner: src };
                row.base_text = view.line(first);
                row.base = Some(corroborate(&view, &cite.ranges, &ev).0);
            }
        }
    }

    // Where did it go? For a citation its own target contradicts OR cannot
    // hold at all — a line past EOF is the strongest case of all, because it
    // usually means the source file was split and the content is now in the
    // half that kept it (`atlas.js` -> `atlasbake.js`). Only on request: this
    // scans every source file under the root.
    let hunt = matches!(
        row.head,
        Judgement::Contradicted { .. } | Judgement::OutOfRange { .. } | Judgement::Unresolved
    );
    if req.moved && hunt {
        row.moved_to = find_moved(sources, citing, &target, &ev);
    }
    row
}

/// Hunts the other source files for a better home than the cited one.
///
/// This is what catches a citation that names the wrong file entirely — two
/// citations in this corpus name `atlas.js:220-236` for a symbol that lives in
/// `decals.js`. Reported only when exactly one other file covers *every* piece
/// of quoted evidence on a single line, so a guess is never dressed up as a
/// finding.
fn find_moved(
    sources: &mut SourceIndex<'_>,
    citing: &str,
    exclude: &str,
    ev: &Evidence,
) -> Option<String> {
    let root = sources.root_for(citing)?;
    let paths: Vec<String> = sources
        .index(&root)
        .values()
        .flatten()
        .filter(|p| *p != exclude)
        .cloned()
        .collect();

    let mut best: Option<(String, u32, usize)> = None;
    let mut ties = 0usize;
    for p in paths {
        let Some(src) = sources.text(&p, None) else { continue };
        let anchors = src.anchors(ev);
        if anchors.len() < ev.idents.len().max(1) {
            continue;
        }
        let mut score: BTreeMap<u32, usize> = BTreeMap::new();
        for lines in &anchors {
            for l in lines {
                *score.entry(*l).or_default() += 1;
            }
        }
        let Some((line, hits)) = score.into_iter().max_by_key(|(l, v)| (*v, std::cmp::Reverse(*l)))
        else {
            continue;
        };
        if hits < anchors.len() {
            continue;
        }
        match &best {
            Some((_, _, b)) if *b > hits => {}
            Some((_, _, b)) if *b == hits => ties += 1,
            _ => {
                best = Some((p, line, hits));
                ties = 0;
            }
        }
    }
    (ties == 0)
        .then_some(best)
        .flatten()
        .map(|(p, l, _)| format!("{p}:{l}"))
}

// ---------------------------------------------------------------------------
// Walking and summarising
// ---------------------------------------------------------------------------

/// Every file in the checkout, repo-relative and sorted — the candidate set the
/// pattern chooses from. Selection is `crate::pattern`'s job, not the walker's,
/// so one grammar governs every command.
fn walk_all(repo: &Repo) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in WalkBuilder::new(&repo.root)
        .hidden(false)
        .git_ignore(true)
        .parents(false)
        .filter_entry(|e| !ALWAYS_SKIP.contains(&e.file_name().to_string_lossy().as_ref()))
        .build()
        .flatten()
    {
        entry
            .file_type()
            .is_some_and(|t| t.is_file())
            .then(|| out.push(repo.rel(entry.path())));
    }
    out.sort();
    out
}

fn read_text(path: &Path) -> std::io::Result<String> {
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn summarize(rows: Vec<Row>, req: &Request, files_scanned: usize) -> Report {
    let mut head = Tally::default();
    let mut base = Tally::default();
    let mut per: BTreeMap<String, (Tally, Tally, Vec<i64>, usize)> = BTreeMap::new();
    let mut bases: BTreeMap<String, usize> = BTreeMap::new();
    let mut rotted = 0usize;
    let mut wrong_when_written = 0usize;

    for r in &rows {
        head.add(&r.head);
        if let Some(b) = &r.base {
            base.add(b);
        }
        let slot = per.entry(r.file.clone()).or_default();
        slot.0.add(&r.head);
        if let Some(b) = &r.base {
            slot.1.add(b);
        }
        if let Some(s) = r.suggest {
            let lo = r.ranges.first().map(|x| x.0).unwrap_or(0);
            slot.2.push(i64::from(s) - i64::from(lo));
        }
        if let Some(b) = &r.external_base {
            slot.3 += 1;
            *bases.entry(b.clone()).or_default() += 1;
        }
        match r.history() {
            Some("ROTTED") => rotted += 1,
            Some(_) => wrong_when_written += 1,
            None => {}
        }
    }

    let per_file = per
        .into_iter()
        .map(|(file, (h, b, mut d, external))| {
            d.sort_unstable();
            let drift = (!d.is_empty()).then(|| d[d.len() / 2]);
            FileStat {
                file,
                head: h,
                base: b,
                drift,
                drift_samples: d.len(),
                drift_modes: modes(&d),
                external,
            }
        })
        .collect();

    let external = bases.values().sum();
    let mut external_bases: Vec<(String, usize)> = bases.into_iter().collect();
    external_bases.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    Report {
        rows,
        per_file,
        head,
        base,
        baseline: req.baseline.clone(),
        rotted,
        wrong_when_written,
        files_scanned,
        external,
        external_bases,
    }
}

/// The most common offsets in a file's drift, largest first.
///
/// The median alone cannot tell a file repaired by one shift from a file whose
/// citations are wrong by a different amount each time — and those need
/// completely different fixes, so the difference has to survive into the report.
pub fn modes(offsets: &[i64]) -> Vec<(i64, usize)> {
    let mut count: BTreeMap<i64, usize> = BTreeMap::new();
    for o in offsets {
        *count.entry(*o).or_default() += 1;
    }
    let mut v: Vec<(i64, usize)> = count.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.abs().cmp(&b.0.abs())));
    v.truncate(3);
    v
}

// ---------------------------------------------------------------------------
// Tests// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Parsed> {
        let lines: Vec<&str> = text.lines().collect();
        parse_file(&lines)
    }

    /// **Every shape in the real corpus parses.**
    ///
    /// These are not invented: each line is the shape of a citation that
    /// actually appears in `apps/axiom-shmup`. A grammar fixed against a guess
    /// would silently drop whole files from the audit.
    #[test]
    fn every_citation_shape_in_the_corpus_parses() {
        let got = parse(concat!(
            "//! Ported from `src/fx/shells.js:1-245`.\n",
            "/// `CAPACITY`, `shells.js:20`.\n",
            "/// (`weapon.js:71,73,135`)\n",
            "/// (`grounding.js:64, 89-90`)\n",
            "// ---- scheduling, `index.js:202-203, 820` -----\n",
        ));
        let shapes: Vec<(&str, Vec<(u32, u32)>)> = got
            .iter()
            .map(|p| (p.name.as_str(), p.ranges.clone()))
            .collect();
        assert_eq!(
            shapes,
            vec![
                ("src/fx/shells.js", vec![(1, 245)]),
                ("shells.js", vec![(20, 20)]),
                ("weapon.js", vec![(71, 71), (73, 73), (135, 135)]),
                ("grounding.js", vec![(64, 64), (89, 90)]),
                ("index.js", vec![(202, 203), (820, 820)]),
            ]
        );
    }

    /// **A citation that wraps onto the next line is one citation, not none.**
    ///
    /// Both wrap shapes occur: a trailing comma continuing the list, and the
    /// file part left dangling at end of line. Missing them loses the citation
    /// entirely, which reads as "this constant was never cited" — the opposite
    /// of the truth.
    #[test]
    fn a_citation_that_wraps_across_lines_is_still_one_citation() {
        let comma = parse("//! (`bvh.js:104-125,\n//! 836-933`). Both flatten\n");
        assert_eq!(comma.len(), 1, "{comma:?}");
        assert_eq!(comma[0].ranges, vec![(104, 125), (836, 933)]);

        let dangling = parse("/// overrides, `shells.js:\n/// 103-107` (the `opts`).\n");
        assert_eq!(dangling.len(), 1, "{dangling:?}");
        assert_eq!(dangling[0].ranges, vec![(103, 107)]);
    }

    /// A comma that belongs to the prose does not extend the citation.
    #[test]
    fn a_prose_comma_does_not_swallow_the_next_number() {
        let got = parse("/// `a.js:12`, and then 34 other things\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].ranges, vec![(12, 12)]);
        assert_eq!(got[0].raw, "a.js:12");
    }

    /// Things that merely look like citations are not citations.
    #[test]
    fn near_misses_are_not_citations() {
        assert!(parse("// see http://example.com:8080/x\n").is_empty());
        assert!(parse("// foo.rs:12 is a rust file\n").is_empty());
        assert!(parse("// bare.js has no line\n").is_empty());
    }

    /// **Two adjacent one-line doc blocks do not pool their evidence.**
    ///
    /// This is the defect that made a first cut of the classifier score a
    /// demonstrably drifted file at 68%: a fixed line window let the constant
    /// below corroborate the constant above's citation.
    #[test]
    fn context_is_the_doc_block_not_a_line_window() {
        let text = concat!(
            "/// `CAPACITY`, `shells.js:20`.\n",
            "pub const CAPACITY: usize = 14;\n",
            "/// `LIFETIME`, `shells.js:21`.\n",
            "pub const LIFETIME: f64 = 9.0;\n",
        );
        let lines: Vec<&str> = text.lines().collect();
        let ctx = context(&lines, 0);
        assert_eq!(ctx.len(), 2, "the block and the item it documents: {ctx:?}");
        let ev = evidence(&ctx);
        assert!(ev.idents.contains("capacity"));
        assert!(!ev.idents.contains("lifetime"), "the next block leaked in: {ev:?}");
        assert!(ev.numbers.contains("14"));
        assert!(!ev.numbers.contains("9.0"), "the next block's literal leaked in");
    }

    /// A `//!` module doc block carries no item, and stops at its own end.
    #[test]
    fn a_module_doc_block_is_bounded_by_itself() {
        let text = "//! one `A`\n//! two `foo.js:3`\n\nuse x::Y;\n";
        let lines: Vec<&str> = text.lines().collect();
        let ctx = context(&lines, 1);
        assert_eq!(ctx, vec!["//! one `A`", "//! two `foo.js:3`"]);
    }

    /// Evidence is what the doc *quotes*, not what it says. Prose words are
    /// excluded on purpose: they corroborate by accident.
    #[test]
    fn evidence_is_backticked_quotes_and_literals_only() {
        let ev = evidence(&["/// the plaster puff uses `HOLE_PLASTER` at 0.045 metres"]);
        assert!(ev.idents.contains("holeplaster"));
        assert!(!ev.idents.contains("plaster"), "prose leaked in: {ev:?}");
        assert!(ev.numbers.contains("0.045"));
    }

    /// A citation inside a backtick is not evidence about itself, and neither
    /// is a Rust path.
    #[test]
    fn a_citation_and_a_rust_path_are_not_evidence() {
        let ev = evidence(&["/// see `shells.js:20` and `crate::fx::Shells`"]);
        assert!(ev.idents.is_empty() && ev.numbers.is_empty(), "{ev:?}");
    }

    /// Numbers too small to discriminate are dropped; decimals and hex are kept.
    #[test]
    fn only_discriminating_numbers_are_evidence() {
        let ev = evidence(&["/// `x = 1` `y = 14` `z = 0.7` `m = 0xff`"]);
        assert!(!ev.numbers.contains("1"), "{ev:?}");
        assert_eq!(
            ev.numbers.iter().cloned().collect::<Vec<_>>(),
            vec!["0.7".to_owned(), "0xff".to_owned(), "14".to_owned()]
        );
    }

    fn src(text: &str) -> SourceText {
        SourceText::new(text)
    }

    /// **The off-by-one that the hand audit found is caught, and named.**
    ///
    /// `shells.rs` cites `shells.js:20` for `CAPACITY`, which is on line 19.
    /// The line exists, so an existence check passes it. Corroboration does not,
    /// and it says where the constant actually is.
    #[test]
    fn a_one_line_drift_is_contradicted_and_the_real_line_is_suggested() {
        let s = src("\nconst CAPACITY = 14;\nconst LIFETIME = 9.0;\n");
        let view = SourceTextView { inner: &s };
        let ev = evidence(&["/// `CAPACITY`, `shells.js:3`.", "pub const CAPACITY: usize = 14;"]);
        let (j, sug) = corroborate(&view, &[(3, 3)], &ev);
        assert!(matches!(j, Judgement::Contradicted { .. }), "{j:?}");
        assert_eq!(sug, Some(2), "the constant is on line 2");

        let (j, sug) = corroborate(&view, &[(2, 2)], &ev);
        assert_eq!(j, Judgement::Confirmed);
        assert_eq!(sug, None);
    }

    /// A line past end-of-file is a mechanical fact, decided before any
    /// heuristic runs. This is the whole of `atlas.rs` at HEAD.
    #[test]
    fn a_line_past_eof_is_out_of_range_whatever_the_evidence_says() {
        let s = src("a\nb\n");
        let view = SourceTextView { inner: &s };
        let ev = evidence(&["/// `nothing`"]);
        assert_eq!(
            corroborate(&view, &[(220, 236)], &ev).0,
            Judgement::OutOfRange { eof: 2 }
        );
    }

    /// **A doc that quotes nothing findable is undecidable, not correct.**
    /// It stays out of the accuracy denominator.
    #[test]
    fn a_citation_with_no_checkable_evidence_is_declined() {
        let s = src("const A = 1;\nconst B = 2;\n");
        let view = SourceTextView { inner: &s };
        let ev = evidence(&["/// ported as the source has it"]);
        let (j, _) = corroborate(&view, &[(1, 1)], &ev);
        assert_eq!(j, Judgement::Uncorroborated);
        assert!(!j.decidable());
        let mut t = Tally::default();
        t.add(&j);
        assert_eq!(t.decidable(), 0);
    }

    /// **A wide section citation gets slack above its start; a narrow one does
    /// not.** The JSDoc header sits one line above the function it documents,
    /// and a section citation means the function — but a single-line citation
    /// means that line.
    #[test]
    fn only_a_wide_citation_gets_slack_for_its_jsdoc_header() {
        let body: String = (0..30).map(|i| format!("  line{i};
")).collect();
        // The only discriminating token sits in the JSDoc header, one line
        // ABOVE the function the citation names — which is the real shape in
        // `impacts.js`, where each surface function is introduced by a `/** */`.
        let s = src(&format!(
            "/** Plaster: HOLE_PLASTER. */
function plaster() {{
{body}}}
"
        ));
        let view = SourceTextView { inner: &s };
        let ev = evidence(&["/// Plaster, `HOLE_PLASTER`. `impacts.js:2-33`."]);
        assert_eq!(view.anchors(&ev).len(), 1, "one anchor, on line 1");

        // The wide citation starts at the function, one past its header.
        assert_eq!(corroborate(&view, &[(2, 33)], &ev).0, Judgement::Confirmed);

        // The same evidence cited as a single line one past its home is not:
        // a narrow citation gets no slack, and the drift is named.
        let (j, sug) = corroborate(&view, &[(2, 2)], &ev);
        assert!(matches!(j, Judgement::Contradicted { .. }), "{j:?}");
        assert_eq!(sug, Some(1));
    }

    /// A token appearing all over the file    /// A token appearing all over the file cannot localize anything, so it is
    /// dropped rather than allowed to corroborate by accident.
    #[test]
    fn an_undiscriminating_token_is_not_an_anchor() {
        let body: String = (0..20).map(|_| "  const rng = fx.rng;\n".to_owned()).collect();
        let s = src(&body);
        let ev = evidence(&["/// `rng`"]);
        assert!(s.anchors(&ev).is_empty(), "20 occurrences localize nothing");
    }

    /// Confidence is reported, never hidden: a partial verdict says how much of
    /// the evidence agreed with it.
    #[test]
    fn a_partial_verdict_carries_its_confidence() {
        let j = Judgement::Partial { covered: 1, total: 4 };
        assert_eq!(j.label(), "PARTIAL");
        assert!((j.confidence() - 0.25).abs() < 1e-9);
        assert!(j.decidable() && !j.ok());
        assert_eq!(Judgement::Confirmed.confidence(), 1.0);
    }

    /// The accuracy rate is a bracket, not a point: confirmed is the floor,
    /// confirmed-plus-partial the ceiling, and undecidable rows are in neither.
    #[test]
    fn accuracy_is_a_bracket_over_decidable_rows_only() {
        let mut t = Tally::default();
        t.add(&Judgement::Confirmed);
        t.add(&Judgement::Partial { covered: 1, total: 2 });
        t.add(&Judgement::Contradicted { covered: 0, total: 2 });
        t.add(&Judgement::Uncorroborated);
        assert_eq!(t.total, 4);
        assert_eq!(t.decidable(), 3);
        assert!((t.accuracy() - 1.0 / 3.0).abs() < 1e-9);
        assert!((t.accuracy_upper() - 2.0 / 3.0).abs() < 1e-9);
    }

    /// Rot and wrong-when-written are different findings, and the difference is
    /// the baseline. A citation wrong at both revisions was never right.
    #[test]
    fn rot_and_wrong_when_written_are_told_apart_by_the_baseline() {
        let row = |head: Judgement, base: Option<Judgement>| Row {
            file: "a.rs".into(),
            line: 1,
            raw: "a.js:1".into(),
            target: None,
            ambiguous: false,
            external_base: None,
            ranges: vec![(1, 1)],
            head,
            base,
            head_text: None,
            base_text: None,
            suggest: None,
            moved_to: None,
            anchors: 0,
        };
        let bad = || Judgement::Contradicted { covered: 0, total: 1 };
        assert_eq!(row(bad(), Some(Judgement::Confirmed)).history(), Some("ROTTED"));
        assert_eq!(row(bad(), Some(bad())).history(), Some("WRONG-WHEN-WRITTEN"));
        assert_eq!(row(Judgement::Confirmed, Some(bad())).history(), None);
        assert_eq!(row(bad(), None).history(), None, "no baseline, no history");
        assert_eq!(
            row(Judgement::Uncorroborated, Some(bad())).history(),
            None,
            "an undecidable row is not evidence of anything"
        );
    }

    /// **A citation written against a tree outside the checkout is named as
    /// such**, rather than silently resolving to the in-repo twin. A corpus
    /// answering to two bases at once is why some rot is not mechanically
    /// repairable, so the second base has to be visible.
    #[test]
    fn an_absolute_out_of_checkout_base_is_reported() {
        let got = parse("//! Ported from `C:/dev/Claude-of-Duty/src/ui/hud.js:1-370`
");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].external_base.as_deref(), Some("C:/dev/Claude-of-Duty/src/ui"));
        assert_eq!(got[0].ranges, vec![(1, 370)]);
        // The basename still resolves, so the citation is still judged.
        assert!(got[0].name.ends_with("ui/hud.js"));

        let inside = parse("//! Ported from `src/ui/hud.js:1-370`
");
        assert_eq!(inside[0].external_base, None, "an in-repo base is not flagged");
    }

    /// **One shift and many shifts are different findings.**
    ///
    /// A file whose citations are all off by `+14` is repaired by one command.
    /// A file whose offsets scatter is not, and a median would make the two
    /// look identical — which is precisely the over-fitting this must avoid.
    #[test]
    fn modes_separate_a_mechanical_shift_from_a_scattered_one() {
        let clean = modes(&[14, 14, 14, 14, 1]);
        assert_eq!(clean[0], (14, 4));
        let piecewise = modes(&[1, 1, 1, 15, 15, 15, 15]);
        assert_eq!(piecewise[0], (15, 4));
        assert_eq!(piecewise[1], (1, 3));
        let scattered = modes(&[-1, -3, -10, -7, -1, -4]);
        assert_eq!(scattered[0].1, 2, "no offset explains the file: {scattered:?}");
        assert!(modes(&[]).is_empty());
    }

    /// **A bare basename resolves by walking UP the citing file's directory.**
    ///
    /// Found by validation, not by design: `weapons/parts/barrel.rs` cites
    /// `parts.js` meaning `weapons/parts.js`, and the first cut answered
    /// `ai/parts.js` because it took the alphabetically-first candidate when
    /// the exact directory missed. 85 citations in one subsystem were judged
    /// against the wrong file — a silently wrong answer, which is worse than a
    /// refusal.
    #[test]
    fn a_bare_basename_resolves_by_walking_up_the_citing_directory() {
        let cands = vec![
            "apps/shmup/src/ai/parts.js".to_owned(),
            "apps/shmup/src/weapons/parts.js".to_owned(),
        ];
        let (hit, amb) = prefer_candidate(
            "apps/shmup/src",
            "apps/axiom-shmup/src/weapons/parts/barrel.rs",
            "parts.js",
            &cands,
        );
        assert_eq!(hit, "apps/shmup/src/weapons/parts.js");
        assert!(!amb);

        // The exact directory still wins when it exists.
        let idx = vec![
            "apps/shmup/src/ai/index.js".to_owned(),
            "apps/shmup/src/fx/index.js".to_owned(),
        ];
        assert_eq!(
            prefer_candidate("apps/shmup/src", "apps/axiom-shmup/src/fx/system.rs", "index.js", &idx).0,
            "apps/shmup/src/fx/index.js"
        );

        // And a genuinely undecidable one is FLAGGED, not silently guessed.
        let (_, amb) = prefer_candidate(
            "apps/shmup/src",
            "apps/axiom-shmup/src/render/draw.rs",
            "index.js",
            &idx,
        );
        assert!(amb, "no directory in the chain matches, so the pick is a guess");
    }

    /// Identifier matching is case- and separator-insensitive, so the port's
    /// `case_len` corroborates the source's `CASE_LEN`.
    #[test]
    fn identifiers_match_across_the_naming_convention_change() {
        assert_eq!(normalize("CASE_LEN"), normalize("caseLen"));
        let s = src("const CASE_LEN = 0.045;\n");
        let ev = evidence(&["/// `case_len`", "pub const CASE_LEN: f64 = 0.045;"]);
        let view = SourceTextView { inner: &s };
        assert_eq!(corroborate(&view, &[(1, 1)], &ev).0, Judgement::Confirmed);
    }
}
