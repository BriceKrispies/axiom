//! `ax shape` — is this file data wearing Rust, or a genuine algorithm?
//!
//! `docs/engine-datafication.md` sets one sharp test for whether a piece of
//! code can become data:
//!
//! > A method that is "select params + apply a formula over a closed
//! > vocabulary" wants to be data. […] If you cannot name the closed vocabulary
//! > a method selects over, it is an algorithm — leave it in Rust.
//!
//! That test is mechanical and nothing in the repo could run it. Answering it
//! by hand means `grep -oE` for literals, `grep -cE` for control flow, `wc -l`,
//! and dividing — which is slow, approximate, and wrong in the one way that
//! matters: a `grep` counts the numbers inside doc comments and string
//! literals, and a subsystem with a long explanatory header reads as "dense in
//! constants" when it is nothing of the kind.
//!
//! This walks the real AST instead. A number is a number only where the
//! compiler sees `syn::Lit`; a branch is one only where it sees `Expr::If` and
//! friends; a call is one only in call position. Comments and string contents
//! cannot contribute, because the parser has already thrown them away.
//!
//! ## What it reports, and what each column is for
//!
//! - **literal density** — numeric literals per line of code. High density is
//!   the signature of *content*: a kit piece, a weapon part, a texture recipe
//!   is mostly constants with a little glue.
//! - **branch density** — control-flow constructs per line of code. High
//!   density is the signature of an *algorithm*: real decisions over real
//!   state.
//! - **reuse** — call sites divided by distinct callees. This is the closed
//!   vocabulary test made numeric. A file that calls twelve distinct functions
//!   two hundred times is assembling something out of a small vocabulary, which
//!   is exactly what a recipe graph expresses. A file that calls two hundred
//!   distinct functions once each has no vocabulary to name.
//! - **nodes** — expression nodes in the AST. A *lower bound* on the size of
//!   the fully-inlined expression graph a field/recipe VM would have to hold,
//!   because loops and calls expand further. It is here because that number,
//!   not the densities, is what decides feasibility: `axiom-recipe`'s budget is
//!   256 nodes, and a file whose AST alone exceeds it certainly will not fit.
//!
//! Tests are excluded from every count. A `#[cfg(test)]` module is not part of
//! the subsystem being characterised, and counting it inflated the first run of
//! this command by ~23% on `world/props`.
//!
//! The verdict is a **ranking heuristic, not an oracle**. It exists to sort
//! 128,000 lines into "look at these first", and the real decision is still the
//! doc's: can you name the vocabulary? `--vocab` prints it, tagging each entry
//! `local` when it is defined inside the scanned set — which is what separates
//! a domain verb from an `Option` combinator.

use std::collections::{BTreeMap, BTreeSet};

use syn::spanned::Spanned as _;
use syn::visit::Visit;

/// What the walk found in one file.
#[derive(Debug, Default, Clone)]
pub struct Shape {
    pub path: String,
    pub code_lines: usize,
    pub test_lines: usize,
    pub literals: usize,
    pub floats: usize,
    pub branches: usize,
    pub calls: usize,
    pub nodes: usize,
    pub vocab: BTreeMap<String, usize>,
    /// Names of functions *defined* in this file, so `--vocab` can tell a
    /// domain verb from a language builtin.
    pub defined: BTreeSet<String>,
    /// Recurring assignment targets and the distinct right-hand-side forms each
    /// takes. See [`Slot`] — this is the measurement that decides
    /// table-versus-algorithm, and the one `reuse` cannot make.
    pub slots: BTreeMap<String, Slot>,
}

impl Shape {
    /// How many rows the biggest repeated record actually has — the `N` in the
    /// Datafication Law's `(N - 1) x per-variant-code`.
    ///
    /// [`Shape::form_ratio`] is not a screen on its own, and reading it as one
    /// is how this tool sent an agent at the wrong file. A file can score a
    /// perfect 0.00 — every field one shape — and still be worthless to convert,
    /// because it has three rows. `fx/tracers.rs` is exactly that: three sprites
    /// of one shape, and converting it took the file from 105 lines to 108.
    ///
    /// The two numbers are only meaningful together. Low variance says a table
    /// is *possible*; a high row count says it is *worth it*.
    pub fn row_count(&self) -> Option<usize> {
        self.slots
            .values()
            .map(|s| s.writes)
            .max()
            .filter(|n| *n >= 3)
    }

    /// What share of a record's *row family* varies in shape.
    ///
    /// **0.0 means every field of the repeated record always has the same
    /// shape** — a pure table, whatever its row count, differing only in
    /// numbers. **1.0 means every field takes a different shape from row to
    /// row** — an algorithm that merely shares a vocabulary.
    ///
    /// The row family is the fields written at least half as often as the
    /// most-written field, i.e. the ones that actually co-occur as rows. That
    /// restriction is load-bearing, and so is measuring a *share* rather than a
    /// max or a mean. All three of the obvious alternatives are wrong, and each
    /// was wrong in a way that looked fine until calibrated against files whose
    /// answer was already known:
    ///
    /// * **Mean forms over writes** lets a dozen constant fields
    ///   (`s.drag = 0.02`, one form, 27 writes) drown the one field that takes
    ///   ten. It scored `fx/impacts.rs` as *more* table-like than
    ///   `audio/foley.rs` — exactly inverted.
    /// * **Max over all fields** saturates on noise: any field written twice
    ///   with two shapes reads 1.00, so both files scored 1.00.
    /// * **Ignoring write counts** misses the actual signal, which is not that
    ///   some field varies but that *the frequently-written ones* do.
    ///   `foley.rs`'s row fields are one shape over 17 writes; `impacts.rs`'s
    ///   are nine to twelve shapes over 27.
    ///
    /// `None` when no field is written at least three times — there is no row
    /// set to speak of, so the question does not arise.
    pub fn form_ratio(&self) -> Option<f64> {
        let peak = self.slots.values().map(|s| s.writes).max()?;
        let family: Vec<&Slot> = self
            .slots
            .values()
            .filter(|s| s.writes * 2 >= peak)
            .collect();
        let varies = family.iter().filter(|s| s.forms.len() > 1).count();
        (peak >= 3).then(|| varies as f64 / family.len() as f64)
    }
}

/// Which side of the datafication line a file falls on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Dense in constants, sparse in decisions — content, expressible as data.
    Data,
    /// Neither shape dominates; read it before deciding.
    Mixed,
    /// Decisions over state, with no small vocabulary to name — leave it code.
    Algorithm,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Data => "data",
            Verdict::Mixed => "mixed",
            Verdict::Algorithm => "algorithm",
        }
    }
}

/// A file with no code lines has no shape to report; guarding here keeps every
/// density below a plain division.
const EMPTY: f64 = 0.0;

impl Shape {
    pub fn literal_density(&self) -> f64 {
        match self.code_lines {
            0 => EMPTY,
            n => self.literals as f64 / n as f64,
        }
    }

    pub fn branch_density(&self) -> f64 {
        match self.code_lines {
            0 => EMPTY,
            n => self.branches as f64 / n as f64,
        }
    }

    pub fn distinct_calls(&self) -> usize {
        self.vocab.len()
    }

    /// Call sites per distinct callee — the closed-vocabulary test, numerically.
    pub fn reuse(&self) -> f64 {
        match self.vocab.len() {
            0 => EMPTY,
            n => self.calls as f64 / n as f64,
        }
    }

    /// The thresholds are deliberately loose. They are tuned to separate the
    /// two ends of this repo's own distribution — `world/kit` at one extreme,
    /// `physics/bvh` at the other — and anything in between is reported as
    /// `mixed` rather than guessed at.
    pub fn verdict(&self) -> Verdict {
        let (literals, branches) = (self.literal_density(), self.branch_density());
        match (literals, branches) {
            (l, b) if b < 0.06 && l >= 0.30 => Verdict::Data,
            (l, b) if b >= 0.10 || l < 0.12 => Verdict::Algorithm,
            _ => Verdict::Mixed,
        }
    }
}

/// Walks a parsed file, counting the things the verdict turns on.
struct Walk {
    shape: Shape,
}

impl Walk {
    fn note_call(&mut self, name: String) {
        self.shape.calls += 1;
        *self.shape.vocab.entry(name).or_insert(0) += 1;
    }

    /// Record one write to `target`, and the form its value took.
    fn note_slot(&mut self, target: String, value: &syn::Expr) {
        let slot = self.shape.slots.entry(target).or_default();
        slot.writes += 1;
        slot.forms.insert(form(value));
    }
}

impl<'ast> Visit<'ast> for Walk {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.shape.defined.insert(item.sig.ident.to_string());
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.shape.defined.insert(item.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_lit(&mut self, lit: &'ast syn::Lit) {
        match lit {
            syn::Lit::Float(_) => {
                self.shape.literals += 1;
                self.shape.floats += 1;
            }
            syn::Lit::Int(_) => self.shape.literals += 1,
            _ => {}
        }
        syn::visit::visit_lit(self, lit);
    }

    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        self.shape.nodes += 1;
        match expr {
            // Every construct the Branchless Law names, which is the same set
            // that distinguishes a decision from a description.
            syn::Expr::If(_)
            | syn::Expr::Match(_)
            | syn::Expr::While(_)
            | syn::Expr::ForLoop(_)
            | syn::Expr::Loop(_)
            | syn::Expr::Try(_) => self.shape.branches += 1,
            syn::Expr::Binary(b) => {
                if matches!(b.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) {
                    self.shape.branches += 1;
                }
            }
            // A free call: name it by its last path segment, so `world::kit::box_geo`
            // and `box_geo` are one vocabulary entry rather than two.
            syn::Expr::Call(call) => {
                if let syn::Expr::Path(path) = &*call.func {
                    if let Some(seg) = path.path.segments.last() {
                        self.note_call(seg.ident.to_string());
                    }
                }
            }
            syn::Expr::MethodCall(call) => self.note_call(call.method.to_string()),
            // The two ways a row gets filled in: a statement that writes a
            // field, and a struct literal that names one. `impacts.rs` is the
            // first, `foley.rs` and `tracers.rs` are the second, and the point
            // of the measurement is to compare them on one scale.
            // A *field* write only. `s.size0 = ...` is a row being filled in;
            // `end = end.max(..)` is a driver accumulating, and counting the
            // second made `audio/foley.rs` -- a genuine 12-row table whose
            // driver happens to fold a local 27 times -- score worse than the
            // algorithm it is supposed to be distinguished from.
            syn::Expr::Assign(a) => {
                let target = form(&a.left);
                target
                    .contains('.')
                    .then(|| self.note_slot(target, &a.right));
            }
            // A struct literal's fields are the same thing said the other way,
            // and are qualified by the struct so two unrelated types with a
            // field of the same name do not pool.
            syn::Expr::Struct(st) => {
                let ty = st
                    .path
                    .segments
                    .last()
                    .map_or_else(|| "_".to_owned(), |g| g.ident.to_string());
                st.fields
                    .iter()
                    .for_each(|f| self.note_slot(format!("{ty}.{}", member(&f.member)), &f.expr));
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }
}

/// Whether an item is compiled only under `cfg(test)`, or is itself a test.
///
/// Both spellings appear in this repo — a `#[cfg(test)] mod tests` and a bare
/// `#[test] fn` — and neither is part of the subsystem being characterised.
fn is_test_item(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        if path.is_ident("test") {
            return true;
        }
        if !path.is_ident("cfg") {
            return false;
        }
        attr.parse_args::<syn::Meta>()
            .map(|meta| meta.path().is_ident("test"))
            .unwrap_or(false)
    })
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Mod(i) => &i.attrs,
        syn::Item::Fn(i) => &i.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Struct(i) => &i.attrs,
        syn::Item::Enum(i) => &i.attrs,
        syn::Item::Const(i) => &i.attrs,
        syn::Item::Static(i) => &i.attrs,
        syn::Item::Use(i) => &i.attrs,
        syn::Item::Type(i) => &i.attrs,
        syn::Item::Trait(i) => &i.attrs,
        _ => &[],
    }
}

/// Count the lines that carry code, as opposed to comment or blank, skipping
/// any line inside a test item.
///
/// Deliberately textual rather than span-derived for the comment part: a span
/// covers the whole item including its doc comment, and counting those would
/// deflate every density in a codebase whose files carry long explanatory
/// headers — which is exactly this one.
fn code_lines(source: &str, skip: &[(usize, usize)]) -> (usize, usize) {
    let in_skip = |n: usize| skip.iter().any(|(a, b)| n >= *a && n <= *b);
    let mut in_block = false;
    let mut code = 0usize;
    let mut tests = 0usize;
    source.lines().enumerate().for_each(|(i, raw)| {
        let number = i + 1;
        let line = raw.trim();
        let was_in_block = in_block;
        if in_block {
            if line.contains("*/") {
                in_block = false;
            }
            return;
        }
        if line.starts_with("/*") && !line.contains("*/") {
            in_block = true;
            return;
        }
        let is_code = !was_in_block
            && !line.is_empty()
            && !line.starts_with("//")
            && !line.starts_with("/*");
        if !is_code {
            return;
        }
        match in_skip(number) {
            true => tests += 1,
            false => code += 1,
        }
    });
    (code, tests)
}

/// Parse one file and report its shape. Returns `None` for anything `syn`
/// cannot parse, which is reported to the caller as a skip rather than swallowed.
/// A *slot*: one recurring assignment target, and the distinct shapes its
/// right-hand side takes across every place it is written.
///
/// This is the measurement that actually decides table-versus-algorithm, and it
/// exists because the ones next to it do not.
///
/// `reuse` (call sites over distinct callees) and literal density both answer
/// "is there content here?" Neither answers "is the content *addressable*?" —
/// and those come apart badly. `fx/impacts.rs` scores 0.76 literals/line and
/// reuse 10.6, which reads as a table with a driver; it is 27 bursts that share
/// a vocabulary and repeat nothing. `audio/foley.rs` scores reuse 14.0 and is a
/// genuine 12-row table. Dispatching an agent on the first pair of numbers sends
/// it to rewrite an algorithm as a table, which is the one outcome this
/// programme cannot afford, because a forced conversion ships a silent RNG
/// draw-order shift.
///
/// The test that does separate them: **for each target written more than once,
/// how many distinct right-hand-side *forms* does it take?** A form is the
/// expression with every literal replaced by `#`, so `px + n.0 * 0.012` and
/// `px + n.0 * 0.02` are one form — they differ only in a number, which is
/// exactly what a table row holds. `px + dvx + n.0 * off` is a different form,
/// and a table cannot hold the difference without growing a branch.
///
/// One form over many rows is a table. One form per row is an algorithm wearing
/// a table's vocabulary.
#[derive(Debug, Default, Clone)]
pub struct Slot {
    pub writes: usize,
    pub forms: BTreeSet<String>,
}

/// The structural form of an expression: its shape with every literal erased.
///
/// Names are kept and numbers are dropped, deliberately. A table row varies the
/// numbers; if it has to vary which *variable* is read, it is not a row, it is a
/// branch. That asymmetry is the whole measurement.
fn form(expr: &syn::Expr) -> String {
    match expr {
        syn::Expr::Lit(_) => "#".to_owned(),
        syn::Expr::Path(p) => p
            .path
            .segments
            .last()
            .map_or_else(|| "_".to_owned(), |s| s.ident.to_string()),
        syn::Expr::Binary(b) => format!("{}{}{}", form(&b.left), binop(&b.op), form(&b.right)),
        syn::Expr::Unary(u) => format!("{}{}", unop(&u.op), form(&u.expr)),
        syn::Expr::Field(f) => format!("{}.{}", form(&f.base), member(&f.member)),
        syn::Expr::Paren(p) => form(&p.expr),
        syn::Expr::Group(g) => form(&g.expr),
        syn::Expr::Reference(r) => format!("&{}", form(&r.expr)),
        syn::Expr::Cast(c) => format!("{} as _", form(&c.expr)),
        syn::Expr::Index(i) => format!("{}[{}]", form(&i.expr), form(&i.index)),
        syn::Expr::Call(c) => format!("{}({})", form(&c.func), forms(c.args.iter())),
        syn::Expr::MethodCall(m) => {
            format!("{}.{}({})", form(&m.receiver), m.method, forms(m.args.iter()))
        }
        syn::Expr::Tuple(t) => format!("({})", forms(t.elems.iter())),
        syn::Expr::Array(a) => format!("[{}]", forms(a.elems.iter())),
        // A branch on the right-hand side is itself the finding: a row that
        // needs one is not a row. Naming the kind rather than recursing keeps
        // every such right-hand side distinct from every other.
        syn::Expr::If(_) => "<if>".to_owned(),
        syn::Expr::Match(_) => "<match>".to_owned(),
        other => format!("<{}>", kind(other)),
    }
}

fn forms<'a>(args: impl Iterator<Item = &'a syn::Expr>) -> String {
    args.map(form).collect::<Vec<_>>().join(",")
}

fn member(m: &syn::Member) -> String {
    match m {
        syn::Member::Named(i) => i.to_string(),
        syn::Member::Unnamed(i) => i.index.to_string(),
    }
}

fn binop(op: &syn::BinOp) -> &'static str {
    match op {
        syn::BinOp::Add(_) => "+",
        syn::BinOp::Sub(_) => "-",
        syn::BinOp::Mul(_) => "*",
        syn::BinOp::Div(_) => "/",
        syn::BinOp::Rem(_) => "%",
        _ => "?",
    }
}

fn unop(op: &syn::UnOp) -> &'static str {
    match op {
        syn::UnOp::Neg(_) => "-",
        syn::UnOp::Not(_) => "!",
        _ => "*",
    }
}

fn kind(e: &syn::Expr) -> &'static str {
    match e {
        syn::Expr::Closure(_) => "closure",
        syn::Expr::Struct(_) => "struct",
        syn::Expr::Block(_) => "block",
        syn::Expr::Macro(_) => "macro",
        syn::Expr::Range(_) => "range",
        _ => "expr",
    }
}

pub fn analyse(path: &str, source: &str) -> Option<Shape> {
    let parsed = syn::parse_file(source).ok()?;

    let test_spans: Vec<(usize, usize)> = parsed
        .items
        .iter()
        .filter(|item| is_test_item(item_attrs(item)))
        .map(|item| {
            let span = item.span();
            (span.start().line, span.end().line)
        })
        .collect();

    let (code, tests) = code_lines(source, &test_spans);

    let mut walk = Walk {
        shape: Shape {
            path: path.to_owned(),
            code_lines: code,
            test_lines: tests,
            ..Shape::default()
        },
    };
    // Visit only the non-test items, so a test's literals and branches cannot
    // move a verdict about the code it tests.
    parsed
        .items
        .iter()
        .filter(|item| !is_test_item(item_attrs(item)))
        .for_each(|item| walk.visit_item(item));

    Some(walk.shape)
}

/// One row of the merged vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabEntry {
    pub name: String,
    pub count: usize,
    /// Defined inside the scanned set. The column that separates a domain verb
    /// from an `Option` combinator — the single thing that made reading a
    /// vocabulary by hand a manual classification job.
    pub local: bool,
}

/// Merge a set of per-file vocabularies into one, for the "what is the closed
/// vocabulary of this subsystem" question.
pub fn merge_vocab(shapes: &[Shape]) -> Vec<VocabEntry> {
    let defined: BTreeSet<&String> = shapes.iter().flat_map(|s| s.defined.iter()).collect();
    let counts = shapes.iter().fold(BTreeMap::new(), |mut all, shape| {
        shape.vocab.iter().for_each(|(name, n)| {
            *all.entry(name.clone()).or_insert(0usize) += n;
        });
        all
    });
    let mut rows: Vec<VocabEntry> = counts
        .into_iter()
        .map(|(name, count)| VocabEntry {
            local: defined.contains(&name),
            name,
            count,
        })
        .collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_constant_dense_builder_reads_as_data() {
        let src = r#"
            fn window(a: &mut A) {
                a.box_at(0.055, 0.075, 0.34);
                a.box_at(0.19, 0.62, 0.5);
                a.plane(1.0, 2.0, 3.0);
                a.plane(4.0, 5.0, 6.0);
                a.box_at(7.0, 8.0, 9.0);
            }
        "#;
        let shape = analyse("k.rs", src).unwrap();
        assert_eq!(shape.verdict(), Verdict::Data);
        assert_eq!(shape.distinct_calls(), 2);
        assert!(shape.reuse() > 2.0);
    }

    #[test]
    fn a_decision_dense_routine_reads_as_algorithm() {
        let src = r#"
            fn traverse(n: &N) -> i32 {
                let mut acc = 0;
                for c in n.children() {
                    if c.hit() {
                        acc += 1;
                    } else if c.near() {
                        acc -= 1;
                    }
                    while c.more() {
                        acc += 1;
                    }
                }
                acc
            }
        "#;
        let shape = analyse("b.rs", src).unwrap();
        assert_eq!(shape.verdict(), Verdict::Algorithm);
    }

    /// The reason this walks an AST instead of grepping: a long doc comment
    /// full of numbers must not make a file look like content.
    #[test]
    fn numbers_in_comments_and_strings_do_not_count() {
        let src = r#"
            /// Ported from source.js:12-345, with constants 1.5, 2.5, 3.5, 4.5.
            //  See also 6.5 and 7.5 and 8.5 and 9.5 and 10.5 and 11.5.
            fn label() -> &'static str {
                "values 1.5 2.5 3.5 4.5 5.5 6.5 7.5 8.5 9.5"
            }
        "#;
        let shape = analyse("d.rs", src).unwrap();
        assert_eq!(shape.literals, 0);
        assert_eq!(shape.floats, 0);
    }

    #[test]
    fn block_comments_do_not_count_as_code_lines() {
        let src = "/*\n  a\n  b\n*/\nfn f() {}\n";
        let shape = analyse("c.rs", src).unwrap();
        assert_eq!(shape.code_lines, 1);
    }

    #[test]
    fn a_call_is_named_by_its_last_path_segment() {
        let src = "fn f() { crate::world::kit::box_geo(1.0); box_geo(2.0); }";
        let shape = analyse("v.rs", src).unwrap();
        assert_eq!(shape.vocab.get("box_geo"), Some(&2));
    }

    #[test]
    fn short_circuit_operators_count_as_branches() {
        let src = "fn f(a: bool, b: bool) -> bool { a && b || a }";
        let shape = analyse("s.rs", src).unwrap();
        assert_eq!(shape.branches, 2);
    }

    #[test]
    fn unparseable_input_is_skipped_rather_than_guessed_at() {
        assert!(analyse("x.rs", "fn (((").is_none());
    }

    /// Tests are not part of the subsystem. Counting them inflated the first
    /// run of this command by ~23% on `world/props`, which moved a split
    /// estimate a fleet of agents was building on.
    #[test]
    fn a_cfg_test_module_is_excluded_from_every_count() {
        let src = "fn f() { g(1.0); }\n\
                   #[cfg(test)]\n\
                   mod tests {\n\
                       fn t() { h(2.0); i(3.0); j(4.0); }\n\
                   }\n";
        let shape = analyse("t.rs", src).unwrap();
        assert_eq!(shape.literals, 1, "test literals leaked into the count");
        assert_eq!(shape.vocab.get("h"), None, "test calls leaked into the vocab");
        assert_eq!(shape.code_lines, 1);
        assert_eq!(shape.test_lines, 4);
    }

    #[test]
    fn a_bare_test_function_is_excluded_too() {
        let src = "fn f() { g(1.0); }\n#[test]\nfn t() { h(2.0); }\n";
        let shape = analyse("t.rs", src).unwrap();
        assert_eq!(shape.literals, 1);
        assert_eq!(shape.vocab.get("h"), None);
    }

    /// The column that separates a domain verb from a language builtin.
    #[test]
    fn the_vocabulary_marks_callees_defined_in_the_scanned_set() {
        let a = analyse("a.rs", "fn ll(x: f64) -> f64 { x }\nfn f() { ll(1.0); }").unwrap();
        let b = analyse("b.rs", "fn g() { ll(2.0); opt.map(|v| v); }").unwrap();
        let rows = merge_vocab(&[a, b]);
        let ll = rows.iter().find(|r| r.name == "ll").unwrap();
        let map = rows.iter().find(|r| r.name == "map").unwrap();
        assert_eq!(ll.count, 2);
        assert!(ll.local, "a function defined in the set is local");
        assert!(!map.local, "an Option combinator is not domain vocabulary");
    }

    #[test]
    fn vocabularies_merge_and_rank_by_count() {
        let a = analyse("a.rs", "fn f() { g(1.0); h(2.0); }").unwrap();
        let b = analyse("b.rs", "fn f() { g(3.0); }").unwrap();
        let rows = merge_vocab(&[a, b]);
        assert_eq!(rows[0].name, "g");
        assert_eq!(rows[0].count, 2);
    }

    /// The number that decides field-graph feasibility: `axiom-recipe`'s budget
    /// is 256 nodes, and an AST already over it certainly will not fit.
    #[test]
    fn expression_nodes_are_counted_as_a_lower_bound_on_graph_size() {
        let flat = analyse("f.rs", "fn f() -> f64 { 1.0 }").unwrap();
        let nested = analyse("n.rs", "fn f(a: f64) -> f64 { a * 2.0 + a * 3.0 - a }").unwrap();
        assert!(nested.nodes > flat.nodes);
        assert!(flat.nodes >= 1);
    }

    #[test]
    fn an_empty_file_has_no_densities_rather_than_a_division_by_zero() {
        let shape = analyse("e.rs", "").unwrap();
        assert_eq!(shape.literal_density(), 0.0);
        assert_eq!(shape.branch_density(), 0.0);
        assert_eq!(shape.reuse(), 0.0);
    }
}
