//! `ax wgsl` — shader text that lives inside Rust string literals, lifted out
//! into the `.wgsl` files it should have been all along.
//!
//! A shader written as `pub const FOO: &str = r#"…"#;` is invisible to every
//! tool that understands shaders: no syntax highlighting, no WGSL formatter, no
//! `naga` validation, no diff that reads as shader code. It is also invisible
//! to `ax --lang wgsl`, because as far as the walker is concerned the repo
//! contains no shaders at all. The fix is mechanical and it is the same one
//! every time: move the literal's text into a sibling `.wgsl` file and point
//! the constant at it with `include_str!`.
//!
//! # Why `include_str!` and not `wgpu::include_wgsl!`
//!
//! `include_wgsl!` is `include_str!` wrapped in a `wgpu::ShaderModuleDescriptor`
//! literal — it performs no compile-time validation, and it changes the
//! constant's **type**. That is the right macro when the constant *is* a whole
//! shader module handed straight to `create_shader_module`. It is the wrong one
//! for a **fragment** — a bare `owSurface` body, a helper library, a snippet
//! spliced into a generated program — because a fragment has to stay a
//! `&'static str` its call sites can `concat`, compare and hash. This command
//! therefore emits `include_str!`, which is byte-for-byte the same compile-time
//! load with the type left alone. [`Inlined::is_module`] reports which of the
//! two a given constant is, so the distinction is a fact the tool states rather
//! than a judgement the caller has to make.
//!
//! # The line-ending trap this command exists to not fall into
//!
//! Rust's lexer normalises `\r\n` to `\n` **inside** a string literal, and
//! `include_str!` does **not** normalise anything. So on a CRLF checkout a
//! naive extraction silently changes the constant's value from LF to CRLF —
//! nothing fails, nothing is logged, and the shader the GPU compiles is no
//! longer the shader the constant held.
//!
//! Line endings are [`crate::eol`]'s job, not this module's: source arrives
//! here already LF, and every file written goes back out through the one
//! renderer. What this module owns is the *check* — [`plan`] refuses to run at
//! all unless every `.wgsl` target resolves to LF, so the `.gitattributes`
//! entry that guarantees it cannot quietly go missing.
//!
//! # All-or-nothing
//!
//! Every literal is located, every target path is resolved and every collision
//! is settled *before* a byte is written — the same guarantee `ax apply` gives,
//! for the same reason: a half-applied extraction leaves a tree nobody designed.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;

use syn::visit::Visit;

use crate::eol::{self, Attributes};
use crate::repo::Repo;

/// Substrings that are WGSL and essentially nothing else. Two of these is
/// already decisive.
const STRONG: &[&str] = &[
    "vec2<f32>",
    "vec3<f32>",
    "vec4<f32>",
    "vec2<u32>",
    "vec3<u32>",
    "vec2<i32>",
    "mat3x3<",
    "mat4x4<",
    "array<",
    "ptr<function",
    "var<uniform>",
    "var<storage",
    "var<workgroup>",
    "@group(",
    "@binding(",
    "@location(",
    "@builtin(",
    "@vertex",
    "@fragment",
    "@compute",
    "@workgroup_size(",
    "textureSample",
    "textureLoad",
    "textureStore",
    ": f32",
    "-> f32",
];

/// Substrings that are shader-shaped but also ordinary prose or Rust. Worth a
/// point each, never decisive on their own.
const WEAK: &[&str] = &["fn ", "let ", "return ", "-> ", "();"];

/// A constant is treated as shader text at or above this score unless
/// `--min-score` says otherwise.
pub const DEFAULT_MIN_SCORE: u32 = 4;

/// Markers that make a fragment a whole, standalone shader **module** — the
/// only shape `wgpu::include_wgsl!` is the right macro for.
const MODULE_MARKERS: &[&str] = &["@vertex", "@fragment", "@compute"];

/// One shader constant found inlined in a Rust source file.
#[derive(Debug, Clone)]
pub struct Inlined {
    /// Repo-relative path of the `.rs` file holding the literal.
    pub path: String,
    /// 1-based line the constant is declared on.
    pub line: usize,
    /// The constant's name.
    pub name: String,
    /// The literal's **value** — what the compiler sees, LF-normalised already.
    pub value: String,
    /// Byte span of the literal token inside the `.rs` source.
    pub span: Range<usize>,
    /// How WGSL-like the value scored.
    pub score: u32,
}

impl Inlined {
    /// True when the text is a whole shader module (it declares a pipeline
    /// entry point) rather than a fragment spliced into one elsewhere.
    pub fn is_module(&self) -> bool {
        MODULE_MARKERS.iter().any(|m| self.value.contains(m))
    }

    /// Lines of shader text.
    pub fn lines(&self) -> usize {
        self.value.lines().count()
    }
}

/// How WGSL-like a string is. Strong markers count double.
pub fn score(text: &str) -> u32 {
    let strong: u32 = STRONG
        .iter()
        .filter(|m| text.contains(**m))
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let weak: u32 = WEAK
        .iter()
        .filter(|m| text.contains(**m))
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    strong.saturating_mul(2).saturating_add(weak)
}

/// `SCREAMING_SNAKE` / `CamelCase` / `snake_case` → the `.wgsl` file's stem.
pub fn file_stem(const_name: &str) -> String {
    let all_upper = const_name
        .chars()
        .all(|c| !c.is_alphabetic() || c.is_uppercase());
    if all_upper {
        return const_name.to_lowercase();
    }
    const_name.chars().enumerate().fold(String::new(), |mut acc, (i, c)| {
        if c.is_uppercase() && i > 0 {
            acc.push('_');
        }
        acc.extend(c.to_lowercase());
        acc
    })
}

/// The value **rustc** compiles this literal to.
///
/// This is not `LitStr::value()`, and the difference is the whole reason this
/// command is careful. Rust's lexer normalises a literal CRLF inside a string
/// to a single LF (the Reference: "the CR character is not allowed unless
/// followed by LF, in which case the pair is treated as a single LF"). `syn`
/// does not do that — `value()` on a raw string hands back the source bytes,
/// CRs and all. On a CRLF checkout, writing `value()` into a `.wgsl` and
/// reading it back with `include_str!` — which normalises nothing — would
/// therefore hand the compiler a *different string* than the one it replaced,
/// silently.
///
/// For a raw string the answer is exact: the body is the source between the
/// delimiters, and CRLF -> LF is the only transformation the lexer applies.
/// For a normal string the unescaping has already happened, so a literal CRLF
/// and an escaped `\r\n` are no longer distinguishable in the value; the caller
/// is told to leave that case alone rather than guess.
fn rustc_value(lit: &syn::LitStr, source: &str) -> Result<String, String> {
    let span = lit.span().byte_range();
    let raw_src = source.get(span).unwrap_or_default();
    let is_raw = raw_src.starts_with('r');
    let hashes = raw_src.chars().skip(1).take_while(|c| *c == '#').count();

    is_raw
        .then(|| {
            let open = 1 + hashes + 1;
            let close = raw_src.len().saturating_sub(hashes + 1);
            raw_src
                .get(open..close)
                .map(|body| body.replace("\r\n", "\n"))
                .ok_or_else(|| "raw string literal is malformed".to_owned())
        })
        .unwrap_or_else(|| {
            raw_src
                .contains("\\r")
                .then(|| {
                    Err("string literal mixes a `\\r` escape with real newlines; \
                         make it a raw string before extracting it"
                        .to_owned())
                })
                .unwrap_or_else(|| Ok(lit.value().replace("\r\n", "\n")))
        })
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

struct Collector<'a> {
    path: &'a str,
    source: &'a str,
    min_score: u32,
    found: Vec<Inlined>,
    warnings: Vec<String>,
}

impl Collector<'_> {
    /// Records `name = <literal>` when the type is `&str`/`&'static str`, the
    /// initialiser is a plain string literal, and the text scores as shader.
    fn consider(&mut self, name: &syn::Ident, ty: &syn::Type, expr: &syn::Expr) {
        if !is_str_ref(ty) {
            return;
        }
        let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = expr else {
            return;
        };
        let value = match rustc_value(s, self.source) {
            Ok(v) => v,
            Err(why) => {
                self.warnings.push(format!(
                    "{}:{} {}: skipped — {why}",
                    self.path,
                    name.span().start().line,
                    name
                ));
                return;
            }
        };
        let score = score(&value);
        if score < self.min_score {
            return;
        }
        self.found.push(Inlined {
            path: self.path.to_owned(),
            line: name.span().start().line,
            name: name.to_string(),
            value,
            span: s.span().byte_range(),
            score,
        });
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        self.consider(&node.ident, &node.ty, &node.expr);
        syn::visit::visit_item_const(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        self.consider(&node.ident, &node.ty, &node.expr);
        syn::visit::visit_item_static(self, node);
    }
}

/// `&str` or `&'static str` — the only types this command will move.
fn is_str_ref(ty: &syn::Type) -> bool {
    let syn::Type::Reference(r) = ty else { return false };
    let syn::Type::Path(p) = r.elem.as_ref() else { return false };
    p.qself.is_none() && p.path.is_ident("str")
}

/// What one file's scan turned up, including what it deliberately declined.
#[derive(Debug, Default)]
pub struct Scan {
    pub found: Vec<Inlined>,
    /// Constants that look like shaders but that this command will not move,
    /// each with the reason. Silence here would be the dangerous outcome.
    pub warnings: Vec<String>,
}

/// What each shader constant in a file points at *now*:
/// `NAME -> "file.wgsl"`, for every `const NAME: &str = include_str!("…")`.
///
/// The other half of [`scan`], and the reason an extraction can be checked
/// rather than trusted. `scan` reads the literals a revision held; this reads
/// the includes the worktree has; comparing the two proves the move preserved
/// every byte. Nothing else in the repo can prove that — the app's own tests
/// compare each constant against itself, so they move with the change.
pub fn includes(source: &str) -> BTreeMap<String, String> {
    let Ok(ast) = syn::parse_file(source) else {
        return BTreeMap::new();
    };
    let mut c = IncludeCollector { found: BTreeMap::new() };
    c.visit_file(&ast);
    c.found
}

struct IncludeCollector {
    found: BTreeMap<String, String>,
}

impl IncludeCollector {
    fn consider(&mut self, name: &syn::Ident, ty: &syn::Type, expr: &syn::Expr) {
        let syn::Expr::Macro(m) = expr else { return };
        let is_include = is_str_ref(ty) && m.mac.path.is_ident("include_str");
        let arg = is_include.then(|| m.mac.parse_body::<syn::LitStr>().ok()).flatten();
        arg.map(|lit| self.found.insert(name.to_string(), lit.value()));
    }
}

impl<'ast> Visit<'ast> for IncludeCollector {
    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        self.consider(&node.ident, &node.ty, &node.expr);
        syn::visit::visit_item_const(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        self.consider(&node.ident, &node.ty, &node.expr);
        syn::visit::visit_item_static(self, node);
    }
}

/// Finds every inlined shader constant in one already-read Rust source file.
pub fn scan(rel_path: &str, source: &str, min_score: u32) -> Scan {
    let Ok(ast) = syn::parse_file(source) else {
        return Scan::default();
    };
    let mut c = Collector {
        path: rel_path,
        source,
        min_score,
        found: Vec::new(),
        warnings: Vec::new(),
    };
    c.visit_file(&ast);
    c.found.sort_by_key(|f| f.span.start);
    Scan { found: c.found, warnings: c.warnings }
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// One extraction, fully decided: where the text goes and what replaces it.
#[derive(Debug, Clone)]
pub struct Extraction {
    pub rs_path: String,
    pub line: usize,
    pub name: String,
    /// Repo-relative path of the `.wgsl` file to write.
    pub wgsl_path: String,
    /// The `include_str!` argument — a bare file name, since the target is a
    /// sibling of the `.rs`.
    pub include_arg: String,
    pub bytes: usize,
    pub lines: usize,
    pub score: u32,
    pub is_module: bool,
}

/// A whole batch, resolved but not yet written.
#[derive(Debug, Default)]
pub struct Plan {
    pub extractions: Vec<Extraction>,
    /// Repo-relative `.wgsl` path → exact content to write (LF).
    pub writes: BTreeMap<String, String>,
    /// Repo-relative `.rs` path → its full rewritten source.
    pub rewrites: BTreeMap<String, String>,
    /// Shader-shaped constants this command declined to move, with reasons.
    pub warnings: Vec<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.extractions.is_empty()
    }

    /// Every extracted body is LF, always — see [`rustc_value`].
    pub fn is_lf_only(&self) -> bool {
        !self.writes.values().any(|c| c.contains('\r'))
    }

    pub fn bytes(&self) -> usize {
        self.writes.values().map(String::len).sum()
    }
}

/// Builds the whole plan in memory, or fails without touching anything.
///
/// `sources` is every candidate `.rs` file as `(repo-relative path, source)`.
/// Collisions are settled here: two constants that want the same file name get
/// the module stem prefixed onto both, and a name that still collides — or that
/// would clobber an unrelated file already on disk — is a hard error.
pub fn plan(
    repo: &Repo,
    attrs: &Attributes,
    sources: &[(String, String)],
    min_score: u32,
) -> Result<Plan, String> {
    let scans: Vec<Scan> = sources
        .iter()
        .map(|(path, src)| scan(path, src, min_score))
        .collect();
    let warnings: Vec<String> = scans.iter().flat_map(|s| s.warnings.clone()).collect();
    let found: Vec<Inlined> = scans.into_iter().flat_map(|s| s.found).collect();

    // Provisional target for each finding, then a second pass to disambiguate.
    let stems: Vec<String> = found.iter().map(|f| file_stem(&f.name)).collect();
    let mut claims: BTreeMap<String, usize> = BTreeMap::new();
    for (f, stem) in found.iter().zip(&stems) {
        *claims.entry(target_of(&f.path, stem)).or_insert(0) += 1;
    }

    let mut targets: Vec<String> = Vec::with_capacity(found.len());
    for (f, stem) in found.iter().zip(&stems) {
        let plain = target_of(&f.path, stem);
        let unique = claims.get(&plain).copied().unwrap_or(0) == 1;
        let stem = if unique {
            stem.clone()
        } else {
            format!("{}_{stem}", module_stem(&f.path))
        };
        targets.push(target_of(&f.path, &stem));
    }

    let mut plan = Plan { warnings, ..Plan::default() };
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for (f, target) in found.iter().zip(&targets) {
        if let Some(prior) = seen.get(target) {
            return Err(format!(
                "`{}` and `{prior}` both want `{target}` — rename one of the constants",
                f.name
            ));
        }
        seen.insert(target.clone(), f.name.clone());

        // Containment check, on the derived path as much as on the given one.
        let abs = repo
            .resolve(target)
            .map_err(|e| format!("refusing target `{target}`: {e}"))?;
        if Path::new(&abs).exists() {
            return Err(format!("`{target}` already exists — move it aside first"));
        }

        // `include_str!` normalises nothing, so a target written CRLF would
        // hand the compiler a different string than the literal it replaced.
        // That is guaranteed by `.gitattributes`, and checked here so the
        // guarantee cannot quietly lapse.
        let mode = eol::mode_for(attrs, target, eol::Shape::None);
        (mode != eol::Mode::Text(eol::Eol::Lf))
            .then(|| {
                Err::<(), String>(format!(
                    "`{target}` would be written as {}, not lf — add `*.wgsl text eol=lf` to \
                     .gitattributes first, or the extracted shader will not equal the string \
                     literal it replaces",
                    mode.label()
                ))
            })
            .transpose()?;

        let include_arg = file_name(target).to_owned();
        plan.extractions.push(Extraction {
            rs_path: f.path.clone(),
            line: f.line,
            name: f.name.clone(),
            wgsl_path: target.clone(),
            include_arg,
            bytes: f.value.len(),
            lines: f.lines(),
            score: f.score,
            is_module: f.is_module(),
        });
        plan.writes.insert(target.clone(), f.value.clone());
    }

    // One rewritten source per `.rs`, patched back-to-front so earlier spans
    // keep their offsets.
    for (path, src) in sources {
        let mut mine: Vec<(&Inlined, &String)> = found
            .iter()
            .zip(&targets)
            .filter(|(f, _)| &f.path == path)
            .collect();
        if mine.is_empty() {
            continue;
        }
        mine.sort_by_key(|(f, _)| std::cmp::Reverse(f.span.start));
        let mut out = src.clone();
        for (f, target) in mine {
            let replacement = format!("include_str!(\"{}\")", file_name(target));
            out.replace_range(f.span.clone(), &replacement);
        }
        plan.rewrites.insert(path.clone(), out);
    }

    Ok(plan)
}

/// Writes the plan. Every path was resolved during planning, so this step only
/// fails on I/O.
///
/// Both halves go out through [`crate::eol::store`] — the same atomic,
/// convention-aware write every other `ax` command uses. Nothing here calls
/// `fs::write`, which is what stops this command from growing a private answer
/// to a question the tool already answers once.
pub fn apply(repo: &Repo, attrs: &Attributes, plan: &Plan) -> Result<(), String> {
    plan.writes
        .iter()
        .chain(plan.rewrites.iter())
        .try_for_each(|(rel, content)| {
            let abs = repo.resolve(rel).map_err(|e| e.to_string())?;
            let loaded = eol::load(attrs, &abs, rel)?;
            eol::store(&abs, rel, &loaded, content).map(|_| ())
        })
}

/// The sibling `.wgsl` path for a constant found in `rs_path`.
fn target_of(rs_path: &str, stem: &str) -> String {
    match rs_path.rfind('/') {
        Some(i) => format!("{}/{stem}.wgsl", &rs_path[..i]),
        None => format!("{stem}.wgsl"),
    }
}

/// `a/b/arch.rs` → `arch`.
fn module_stem(rs_path: &str) -> &str {
    let base = file_name(rs_path);
    base.strip_suffix(".rs").unwrap_or(base)
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
/// doc
pub const BODY: &str = "
fn owSurface(uv: vec2<f32>) -> f32 {
  let p = uv * 2.0;
  return p.x;
}
";
const NOT_SHADER: &str = "just prose about a fn and a let";
"#;

    #[test]
    fn scan_finds_shader_constants_and_skips_prose() {
        let found = scan("a/b.rs", SAMPLE, DEFAULT_MIN_SCORE).found;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "BODY");
        assert!(found[0].value.contains("owSurface"));
        assert!(!found[0].is_module());
    }

    #[test]
    fn the_span_covers_exactly_the_literal() {
        let found = scan("a/b.rs", SAMPLE, DEFAULT_MIN_SCORE).found;
        let lit = &SAMPLE[found[0].span.clone()];
        assert!(lit.starts_with('"') && lit.ends_with('"'));
    }

    #[test]
    fn a_module_is_recognised_as_one() {
        let src = "pub const S: &str = \"@vertex fn vs() -> vec4<f32> { return vec4<f32>(0.0); }\";";
        let found = scan("a/b.rs", src, DEFAULT_MIN_SCORE).found;
        assert!(found[0].is_module());
    }

    /// The regression this command was built around: a CRLF checkout must not
    /// change the constant's value. `rustc` normalises CRLF to LF inside a
    /// literal and `syn` does not, so anything writing `LitStr::value()`
    /// straight out would hand `include_str!` — which normalises nothing — a
    /// different string than the one it replaced.
    #[test]
    fn a_crlf_source_still_yields_an_lf_body() {
        let src = "pub const S: &str = r#\"\r\nfn f(uv: vec2<f32>) -> f32 {\r\n  let p = uv;\r\n  return p.x;\r\n}\r\n\"#;\r\n";
        assert!(src.contains('\r'), "the fixture is CRLF");
        let scan = scan("a/b.rs", src, DEFAULT_MIN_SCORE);
        assert_eq!(scan.found.len(), 1);
        assert!(
            !scan.found[0].value.contains('\r'),
            "extracted body kept a CR: {:?}",
            scan.found[0].value
        );
        assert_eq!(scan.found[0].value, "\nfn f(uv: vec2<f32>) -> f32 {\n  let p = uv;\n  return p.x;\n}\n");
    }

    /// A `\r` escape and a real newline are indistinguishable after unescaping,
    /// so the command declines rather than guessing.
    #[test]
    fn an_escaped_cr_is_declined_with_a_reason() {
        let src = "pub const S: &str = \"fn f(uv: vec2<f32>) -> f32 {\\r\\n  let p = uv;\\r\\n  return p.x;\\r\\n}\";";
        let scan = scan("a/b.rs", src, DEFAULT_MIN_SCORE);
        assert!(scan.found.is_empty());
        assert_eq!(scan.warnings.len(), 1);
        assert!(scan.warnings[0].contains("raw string"), "{}", scan.warnings[0]);
    }

    /// Extract, then read back: the constant now points at a file, and the
    /// file holds exactly what the literal held.
    #[test]
    fn an_extraction_round_trips_through_includes() {
        let before = "pub const BODY: &str = r#\"\nfn f(uv: vec2<f32>) -> f32 { return uv.x; }\n\"#;\n";
        let found = scan("a/b.rs", before, DEFAULT_MIN_SCORE).found;
        let after = format!(
            "{}include_str!(\"body.wgsl\"){}",
            &before[..found[0].span.start],
            &before[found[0].span.end..]
        );
        let now = includes(&after);
        assert_eq!(now.get("BODY").map(String::as_str), Some("body.wgsl"));
        // What the .wgsl file would hold, versus what the literal held.
        assert_eq!(found[0].value, "\nfn f(uv: vec2<f32>) -> f32 { return uv.x; }\n");
    }

    #[test]
    fn stems_are_snake_case() {
        assert_eq!(file_stem("GL_SEMANTICS"), "gl_semantics");
        assert_eq!(file_stem("MetalRust"), "metal_rust");
        assert_eq!(file_stem("noise"), "noise");
    }

    #[test]
    fn targets_are_siblings_of_the_source() {
        assert_eq!(target_of("apps/x/src/wgsl/arch.rs", "concrete"), "apps/x/src/wgsl/concrete.wgsl");
        assert_eq!(module_stem("apps/x/src/wgsl/arch.rs"), "arch");
    }
}
