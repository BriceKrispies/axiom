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
//!
//! The verdict is a **ranking heuristic, not an oracle**. It exists to sort
//! 128,000 lines into "look at these first", and the real decision is still the
//! doc's: can you name the vocabulary? `--vocab` prints it so you can try.

use std::collections::BTreeMap;

use syn::visit::Visit;

/// What the walk found in one file.
#[derive(Debug, Default, Clone)]
pub struct Shape {
    pub path: String,
    pub code_lines: usize,
    pub literals: usize,
    pub floats: usize,
    pub branches: usize,
    pub calls: usize,
    pub vocab: BTreeMap<String, usize>,
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

/// Walks a parsed file, counting the three things the verdict turns on.
struct Walk {
    shape: Shape,
}

impl Walk {
    fn note_call(&mut self, name: String) {
        self.shape.calls += 1;
        *self.shape.vocab.entry(name).or_insert(0) += 1;
    }
}

impl<'ast> Visit<'ast> for Walk {
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
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }
}

/// Count the lines that carry code, as opposed to comment or blank.
///
/// Deliberately textual rather than span-derived: a span covers the whole item
/// including its doc comment, and counting those would deflate every density in
/// a codebase whose files carry long explanatory headers — which is exactly
/// this one.
fn code_lines(source: &str) -> usize {
    let mut in_block = false;
    source
        .lines()
        .filter(|raw| {
            let line = raw.trim();
            let was_in_block = in_block;
            if in_block {
                if line.contains("*/") {
                    in_block = false;
                }
                return false;
            }
            if line.starts_with("/*") && !line.contains("*/") {
                in_block = true;
                return false;
            }
            !was_in_block
                && !line.is_empty()
                && !line.starts_with("//")
                && !line.starts_with("/*")
        })
        .count()
}

/// Parse one file and report its shape. Returns `None` for anything `syn`
/// cannot parse, which is reported to the caller as a skip rather than swallowed.
pub fn analyse(path: &str, source: &str) -> Option<Shape> {
    let parsed = syn::parse_file(source).ok()?;
    let mut walk = Walk {
        shape: Shape {
            path: path.to_owned(),
            code_lines: code_lines(source),
            ..Shape::default()
        },
    };
    walk.visit_file(&parsed);
    Some(walk.shape)
}

/// Merge a set of per-file vocabularies into one, for the "what is the closed
/// vocabulary of this subsystem" question.
pub fn merge_vocab(shapes: &[Shape]) -> BTreeMap<String, usize> {
    shapes.iter().fold(BTreeMap::new(), |mut all, shape| {
        shape.vocab.iter().for_each(|(name, n)| {
            *all.entry(name.clone()).or_insert(0) += n;
        });
        all
    })
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

    #[test]
    fn vocabularies_merge_across_files() {
        let a = analyse("a.rs", "fn f() { g(1.0); h(2.0); }").unwrap();
        let b = analyse("b.rs", "fn f() { g(3.0); }").unwrap();
        let all = merge_vocab(&[a, b]);
        assert_eq!(all.get("g"), Some(&2));
        assert_eq!(all.get("h"), Some(&1));
    }

    #[test]
    fn an_empty_file_has_no_densities_rather_than_a_division_by_zero() {
        let shape = analyse("e.rs", "").unwrap();
        assert_eq!(shape.literal_density(), 0.0);
        assert_eq!(shape.branch_density(), 0.0);
        assert_eq!(shape.reuse(), 0.0);
    }
}
