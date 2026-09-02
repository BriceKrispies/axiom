//! **One path-pattern language, for every subcommand.**
//!
//! `ax` had two. `q --path`, `file`, `wgsl` and `eol` took regexes. `cite` took
//! a glob, and decided which was which by asking *"does the pattern contain a
//! `*` or a `?`"* — so any regex containing `*`, which is most regexes, was
//! silently translated as a glob: its `.`, `[`, `]` and `$` escaped, the whole
//! thing anchored, and the result matching nothing.
//!
//! ```text
//! $ ax cite 'materials/wgsl/.*[.]rs$'
//! 0 citation(s) across 0 file(s), 0 file(s) scanned
//! $ ax cite 'materials/wgsl/*.rs'
//! 32 citation(s) across 6 file(s)
//! ```
//!
//! **A zero that is a lie is the worst thing this tool can do.** Every other
//! failure announces itself; this one returns the same shape as a true answer,
//! and it poisons the ledger too — `ax miss` records a zero-result search as
//! *"a question the repo could not answer"* when the repo answered fine and the
//! tool mis-parsed the question.
//!
//! # The rule
//!
//! Stop guessing from the characters. Read the pattern **both** ways and use
//! whichever actually selects files:
//!
//! 1. as an **unanchored regex** — the language `ax q --path` and `ax file`
//!    document, and the one that wins when both match;
//! 2. failing that, as an **anchored glob** — `*`, `**`, `?`, the language
//!    `ax cite` documents.
//!
//! Both interpretations are always reported when the glob is the one that fired,
//! so the fallback is visible rather than magic. When neither matches, the
//! message says both were tried — which is an honest zero.
//!
//! # Why regex wins a tie
//!
//! A bare word like `axiom-shmup` is a valid regex (substring) and a valid glob
//! (exact match, hitting nothing). Preferring the regex keeps `ax cite foo`
//! behaving like `ax file foo`, which is the established meaning across the
//! tool.
//!
//! # What this is deliberately not
//!
//! It is **not** `.gitattributes` glob matching. That language is git's, with
//! its own anchoring rule (a pattern without a `/` matches a basename at any
//! depth) and it belongs to [`crate::eol`], which implements it separately and
//! must keep doing so. Two grammars that look alike are not one grammar.

/// Which reading of the pattern selected the files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Regex,
    Glob,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Regex => "regex",
            Self::Glob => "glob",
        }
    }
}

/// A path pattern, held as both readings until the candidates decide.
#[derive(Debug)]
pub struct PathPattern {
    raw: String,
    /// The pattern read as an unanchored regex, when it compiles as one.
    regex: Option<regex::Regex>,
    /// The pattern read as an anchored glob. Always compiles — every character
    /// that is not glob syntax is escaped.
    glob: regex::Regex,
}

impl PathPattern {
    /// Compiles both readings.
    ///
    /// Fails only when the glob reading cannot compile, which takes a pattern
    /// that is not expressible at all; a pattern that is merely not valid regex
    /// (`*.rs`) is fine and simply has no regex reading.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let glob = regex::Regex::new(&glob_to_regex(raw))
            .map_err(|e| format!("bad path pattern `{raw}`: {e}"))?;
        Ok(Self {
            raw: raw.to_owned(),
            regex: regex::Regex::new(raw).ok(),
            glob,
        })
    }

    /// Selects from `candidates`, regex first, glob second.
    ///
    /// Returns the matches and which reading produced them. An empty result
    /// means genuinely neither reading matched anything.
    pub fn select<I, S>(&self, candidates: I) -> (Vec<String>, Kind)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let all: Vec<String> = candidates
            .into_iter()
            .map(|c| c.as_ref().to_owned())
            .collect();

        let by_regex: Vec<String> = self
            .regex
            .as_ref()
            .map(|re| all.iter().filter(|p| re.is_match(p)).cloned().collect())
            .unwrap_or_default();
        if !by_regex.is_empty() {
            return (by_regex, Kind::Regex);
        }

        let by_glob: Vec<String> = all
            .into_iter()
            .filter(|p| self.glob.is_match(p))
            .collect();
        (by_glob, Kind::Glob)
    }

    /// True when this pattern has no regex reading at all, so the glob is the
    /// only thing that could have matched.
    pub fn glob_only(&self) -> bool {
        self.regex.is_none()
    }

    /// What to print when the glob reading is the one that fired, so a fallback
    /// is never invisible.
    pub fn note(&self, kind: Kind) -> Option<String> {
        (kind == Kind::Glob && !self.glob_only()).then(|| {
            format!(
                "`{}` matched no path as a regex; read as a {}",
                self.raw,
                kind.label()
            )
        })
    }

    /// The regex source of each reading, for a caller that filters a stream and
    /// so cannot hand the whole candidate set to [`Self::select`] — `ax q`'s
    /// `--path`, which decides per file inside the parallel walk.
    pub fn regex_source(&self) -> Option<&str> {
        self.regex.as_ref().map(regex::Regex::as_str)
    }

    /// The glob reading's regex source. Always present.
    pub fn glob_source(&self) -> &str {
        self.glob.as_str()
    }

    /// What to say when nothing matched, naming both attempts.
    pub fn empty_note(&self) -> String {
        match self.glob_only() {
            true => format!("`{}` matched nothing (read as a glob; it is not valid regex)", self.raw),
            false => format!("`{}` matched nothing, as a regex or as a glob", self.raw),
        }
    }

}

/// A glob as an anchored regex over repo-relative paths.
///
/// `*` stops at a path separator, `**` crosses them (and `**/` also matches
/// nothing at all, so `a/**/b.rs` covers `a/b.rs`), `?` is one character.
/// Everything else is literal.
fn glob_to_regex(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::from("^");
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                out.push_str(".*");
                i += 2;
                // `**/` also matches nothing at all.
                if chars.get(i) == Some(&'/') {
                    out.push_str("/?");
                    i += 1;
                }
            }
            '*' => {
                out.push_str("[^/]*");
                i += 1;
            }
            '?' => {
                out.push('.');
                i += 1;
            }
            c => {
                out.push_str(&regex::escape(&c.to_string()));
                i += 1;
            }
        }
    }
    out.push('$');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: &[&str] = &[
        "apps/axiom-shmup/src/materials/wgsl/arch.rs",
        "apps/axiom-shmup/src/materials/wgsl/ground.rs",
        "apps/axiom-shmup/src/materials/wgsl/concrete.wgsl",
        "apps/axiom-shmup/src/fx/noise.rs",
        "modules/axiom-gpu-backend/src/lib.rs",
        "data/blob.bin",
    ];

    fn select(p: &str) -> (Vec<String>, Kind) {
        PathPattern::parse(p).expect("compiles").select(FILES)
    }

    /// **The reported bug.** A regex containing `*` used to be mangled into a
    /// glob and match nothing.
    #[test]
    fn a_regex_containing_a_star_is_read_as_a_regex() {
        let (hits, kind) = select(r"materials/wgsl/.*[.]rs$");
        assert_eq!(kind, Kind::Regex);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!(hits.iter().all(|h| h.ends_with(".rs")));
    }

    /// And the glob that already worked keeps working.
    #[test]
    fn a_glob_still_selects_a_subtree() {
        let (hits, kind) = select("apps/axiom-shmup/src/materials/wgsl/*.rs");
        assert_eq!(kind, Kind::Glob, "{hits:?}");
        assert_eq!(hits.len(), 2);

        let (deep, kind) = select("apps/**/fx/*.rs");
        assert_eq!(kind, Kind::Glob);
        assert_eq!(deep, vec!["apps/axiom-shmup/src/fx/noise.rs"]);
    }

    /// `*.rs` is not valid regex at all, so the glob is the only reading — and
    /// the tool says so rather than reporting a bare zero.
    #[test]
    fn a_leading_star_has_no_regex_reading() {
        let p = PathPattern::parse("*.wgsl").expect("compiles as a glob");
        assert!(p.glob_only());
        let (hits, kind) = p.select(FILES);
        assert_eq!(kind, Kind::Glob);
        // Anchored: `*` does not cross a `/`, so a nested path needs `**`.
        assert!(hits.is_empty());
        assert!(p.empty_note().contains("not valid regex"));
    }

    /// A bare word is both readings; the regex wins, so `cite foo` means what
    /// `file foo` means.
    #[test]
    fn a_bare_word_is_a_regex_substring() {
        let (hits, kind) = select("axiom-shmup");
        assert_eq!(kind, Kind::Regex);
        assert_eq!(hits.len(), 4);
    }

    /// When the glob is what fired, that is announced — a silent reinterpretation
    /// would be the same class of surprise as the bug being fixed.
    #[test]
    fn a_glob_fallback_is_reported() {
        let p = PathPattern::parse("apps/axiom-shmup/src/materials/wgsl/*.rs").expect("compiles");
        let (_, kind) = p.select(FILES);
        assert!(p.note(kind).expect("a note").contains("read as a glob"));
    }

    #[test]
    fn a_true_zero_names_both_attempts() {
        let p = PathPattern::parse("nothing/here").expect("compiles");
        let (hits, _) = p.select(FILES);
        assert!(hits.is_empty());
        assert!(p.empty_note().contains("as a regex or as a glob"));
    }
}
