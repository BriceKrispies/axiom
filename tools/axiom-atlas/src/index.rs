//! **The semantic index** — Rust source as data, rather than as text.
//!
//! `ax def` and `ax refs` used to be regexes. A regex over source cannot tell a
//! call from a doc comment that happens to name the same word, and on this repo
//! that is not a corner case: searching `camera_view_proj` returned twenty-three
//! hits in one module, of which several were prose. An agent then eyeballs the
//! list and decides — which is the step this index removes.
//!
//! # What "semantic" means here, and what it does not
//!
//! This parses with `syn`, so it works from the **abstract syntax tree**:
//! comments and string literals are not in it, and every identifier arrives with
//! its syntactic role attached — a call, a type position, an import, a macro
//! invocation. That is the whole win, and it is honest to say where it stops:
//!
//! * **Not type-resolved.** Two inherent `new` methods on different types are
//!   one name here. Resolving that needs `rustc`'s `TyCtxt`, which this repo
//!   already has a platform for in `tools/lints` — that is the next rung, and
//!   [`Def::qualifier`] is the hook it will hang on.
//! * **Not macro-expanded.** A name assembled by `concat!`/`paste!` is invisible,
//!   exactly as it is to a human reader. The Atlas Friction Law already calls
//!   that a *repo* defect rather than a tool one, and it is right to.
//!
//! # Why it is cached rather than rebuilt
//!
//! Parsing ~2,000 files costs seconds; a query must cost milliseconds. So the
//! index is built once into `.axiom-atlas/index.json` and reused until a source
//! file's modification time moves past the stamp it was built at. A stale index
//! is worse than none — it answers confidently and wrongly — so the staleness
//! check is a whole-tree scan of mtimes, which is cheap next to parsing.

use std::fs;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use syn::visit::Visit;

use crate::repo::Repo;

/// What kind of thing a definition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefKind {
    Fn,
    Method,
    Struct,
    Enum,
    Trait,
    TraitItem,
    Union,
    TypeAlias,
    Const,
    Static,
    Mod,
    Macro,
    Field,
    Variant,
}

impl DefKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fn => "fn",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::TraitItem => "trait item",
            Self::Union => "union",
            Self::TypeAlias => "type",
            Self::Const => "const",
            Self::Static => "static",
            Self::Mod => "mod",
            Self::Macro => "macro",
            Self::Field => "field",
            Self::Variant => "variant",
        }
    }
}

/// How a reference uses the name — the distinction a regex cannot make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefKind {
    /// `foo(..)` or `x.foo(..)`.
    Call,
    /// A name in a path expression or type: `Foo`, `a::Foo`.
    Path,
    /// A `use` item.
    Import,
    /// `foo!(..)`.
    Macro,
}

impl RefKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Path => "path",
            Self::Import => "import",
            Self::Macro => "macro",
        }
    }
}

/// One definition site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Def {
    pub name: String,
    pub kind: DefKind,
    pub file: String,
    pub line: usize,
    /// The `impl` self-type or enclosing trait a method belongs to, so
    /// `SceneRenderer::record` is distinguishable from another `record`. Empty
    /// for free items. This is the field a type-resolved index would fill
    /// properly; today it is syntactic.
    pub qualifier: String,
    /// Whether the item is `pub` in any form. Not visibility *resolution* — a
    /// `pub` item inside a private module is still `pub` here — but enough to
    /// separate "meant to be used elsewhere" from "internal".
    pub public: bool,
}

//// How specific a reference kind is. A `Path` is the fallback every name
/// matches; anything else says something a path does not.
fn rank(kind: RefKind) -> u8 {
    match kind {
        RefKind::Call => 0,
        RefKind::Import => 1,
        RefKind::Macro => 2,
        RefKind::Path => 3,
    }
}

/// One reference site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ref {
    pub name: String,
    pub kind: RefKind,
    pub file: String,
    pub line: usize,
}

//// The current source fingerprint: newest mtime and file count. Both, because
/// a deletion moves the count without moving the newest mtime.
fn fingerprint(repo: &Repo) -> (u64, usize) {
    // Reads the metadata the WALKER already has rather than re-`stat`ing each
    // path. On Windows that second syscall per file cost ~150 ms across this
    // tree — more than the whole query it was guarding.
    walker(repo)
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
        .fold((0, 0), |(newest, count), e| {
            let secs = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
                .unwrap_or(0);
            (newest.max(secs), count + 1)
        })
}

/// The one gitignore-aware walk both the fingerprint and the parse use.
fn walker(repo: &Repo) -> ignore::Walk {
    ignore::WalkBuilder::new(&repo.root)
        .hidden(false)
        .git_ignore(true)
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != "target" && name != "node_modules" && name != ".git"
        })
        .build()
}

/// Every `.rs` file the walker sees, gitignore-aware.
fn rust_files(repo: &Repo) -> Vec<std::path::PathBuf> {
    walker(repo)
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(ignore::DirEntry::into_path)
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect()
}

/// How many shards the index is split across.
///
/// A query asks about ONE name, so loading a whole-tree index to answer it is
/// the wrong shape: as a single file this was 86 MB and 580 ms a query, against
/// `ax q`'s 94 ms. A tool that loses to the habit it replaces does not get used,
/// and the README makes that a stated feature rather than a nicety.
///
/// Sharding on a hash of the name means a query touches exactly one file. 256
/// puts a few hundred names and a few thousand references in each — small
/// enough to parse in single-digit milliseconds, few enough that the directory
/// stays legible.
const SHARDS: usize = 256;

/// Which shard a name lives in. FNV-1a, written out: the hash must be stable
/// across runs and across platforms, and `DefaultHasher` promises neither.
fn shard_of(name: &str) -> usize {
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    (hash % SHARDS as u64) as usize
}

/// The index directory. Beside the ledger, and gitignored with it.
pub fn index_dir(repo: &Repo) -> std::path::PathBuf {
    repo.root.join(".axiom-atlas").join("index")
}

fn meta_path(repo: &Repo) -> std::path::PathBuf {
    index_dir(repo).join("meta.json")
}

fn shard_path(repo: &Repo, shard: usize) -> std::path::PathBuf {
    index_dir(repo).join(format!("{shard:03}.json"))
}

/// What was true when the index was built.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Meta {
    pub stamp_secs: u64,
    pub file_count: usize,
    pub defs: usize,
    pub refs: usize,
}

/// One shard: every definition and reference whose name hashes here.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Shard {
    pub defs: Vec<Def>,
    pub refs: Vec<Ref>,
}

impl Shard {
    /// Definitions of `name`.
    pub fn defs_of<'a>(&'a self, name: &str) -> Vec<&'a Def> {
        self.defs.iter().filter(|d| d.name == name).collect()
    }

    /// References to `name`, excluding the lines its own definitions sit on — a
    /// definition is not a use of itself, and listing it every time is the noise
    /// that made the regex version tiring to read.
    pub fn refs_of<'a>(&'a self, name: &str) -> Vec<&'a Ref> {
        let sites: Vec<(&str, usize)> = self
            .defs_of(name)
            .iter()
            .map(|d| (d.file.as_str(), d.line))
            .collect();
        let mut found: Vec<&Ref> = self
            .refs
            .iter()
            .filter(|r| r.name == name)
            .filter(|r| !sites.contains(&(r.file.as_str(), r.line)))
            .collect();
        // **One site, one row.** A call is recorded twice — once by the call
        // visitor and once by the path visitor that sees its final segment —
        // and an import inside a `use` tree likewise. Reporting both would
        // double every count and put two lines on the screen for one edit an
        // agent has to make. The more specific kind wins.
        found.sort_by(|a, b| {
            (a.file.as_str(), a.line, rank(a.kind)).cmp(&(b.file.as_str(), b.line, rank(b.kind)))
        });
        found.dedup_by(|a, b| a.file == b.file && a.line == b.line);
        found
    }
}

/// Loads the one shard `name` lives in, rebuilding the index first when the
/// tree has moved under it.
///
/// A stale index is worse than none — it answers confidently and wrongly — so
/// the check is a whole-tree mtime scan, which is cheap next to parsing.
pub fn shard_for(repo: &Repo, name: &str) -> Result<Shard, String> {
    let (secs, count) = fingerprint(repo);
    let fresh = fs::read_to_string(meta_path(repo))
        .ok()
        .and_then(|t| serde_json::from_str::<Meta>(&t).ok())
        .is_some_and(|m| m.stamp_secs == secs && m.file_count == count);
    if !fresh {
        build(repo)?;
    }
    let path = shard_path(repo, shard_of(name));
    let text = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_owned());
    serde_json::from_str(&text).map_err(|e| format!("index shard is unreadable: {e}"))
}

/// Parses the whole tree and writes every shard.
pub fn build(repo: &Repo) -> Result<Meta, String> {
    let (stamp_secs, file_count) = fingerprint(repo);
    let mut shards: Vec<Shard> = (0..SHARDS).map(|_| Shard::default()).collect();
    let mut defs_total = 0_usize;
    let mut refs_total = 0_usize;

    for path in rust_files(repo) {
        let rel = repo.rel(&path);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        // A file that does not parse is skipped rather than fatal: the index is
        // an aid, and one syntactically broken file mid-edit must not take the
        // whole tool down.
        let Ok(ast) = syn::parse_file(&text) else {
            continue;
        };
        let mut defs = Vec::new();
        let mut refs = Vec::new();
        let mut v = Collector {
            file: rel,
            qualifier: String::new(),
            defs: &mut defs,
            refs: &mut refs,
        };
        v.visit_file(&ast);
        defs_total += defs.len();
        refs_total += refs.len();
        defs.into_iter().for_each(|d| shards[shard_of(&d.name)].defs.push(d));
        refs.into_iter().for_each(|r| shards[shard_of(&r.name)].refs.push(r));
    }

    let dir = index_dir(repo);
    let _ = fs::create_dir_all(&dir);
    for (i, shard) in shards.iter().enumerate() {
        let json = serde_json::to_string(shard).map_err(|e| e.to_string())?;
        fs::write(shard_path(repo, i), json).map_err(|e| e.to_string())?;
    }
    let meta = Meta {
        stamp_secs,
        file_count,
        defs: defs_total,
        refs: refs_total,
    };
    fs::write(
        meta_path(repo),
        serde_json::to_string(&meta).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(meta)
}

/// Walks one file's AST, collecting definitions and references.
struct Collector<'a> {
    file: String,
    /// The enclosing `impl` self-type or trait name, for methods.
    qualifier: String,
    defs: &'a mut Vec<Def>,
    refs: &'a mut Vec<Ref>,
}

impl Collector<'_> {
    fn def(&mut self, name: &syn::Ident, kind: DefKind, public: bool) {
        self.defs.push(Def {
            name: name.to_string(),
            kind,
            file: self.file.clone(),
            line: name.span().start().line,
            qualifier: self.qualifier.clone(),
            public,
        });
    }

    fn reference(&mut self, name: &syn::Ident, kind: RefKind) {
        // `self`, `Self`, `crate` and `super` are grammar, not names anyone can
        // navigate to. They were 5% of the index and 100% noise in an answer.
        if matches!(name.to_string().as_str(), "self" | "Self" | "crate" | "super") {
            return;
        }
        self.refs.push(Ref {
            name: name.to_string(),
            kind,
            file: self.file.clone(),
            line: name.span().start().line,
        });
    }
}

fn is_pub(vis: &syn::Visibility) -> bool {
    !matches!(vis, syn::Visibility::Inherited)
}

/// The last segment of a type, as a qualifier string: `Foo<T>` -> `Foo`.
fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default(),
        syn::Type::Reference(r) => type_name(&r.elem),
        _ => String::new(),
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.def(&node.sig.ident, DefKind::Fn, is_pub(&node.vis));
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        self.def(&node.ident, DefKind::Struct, is_pub(&node.vis));
        node.fields.iter().for_each(|f| {
            let outer = std::mem::replace(&mut self.qualifier, node.ident.to_string());
            f.ident
                .as_ref()
                .map(|i| self.def(i, DefKind::Field, is_pub(&f.vis)));
            self.qualifier = outer;
        });
        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        self.def(&node.ident, DefKind::Enum, is_pub(&node.vis));
        let outer = std::mem::replace(&mut self.qualifier, node.ident.to_string());
        node.variants.iter().for_each(|v| {
            let ident = v.ident.clone();
            self.def(&ident, DefKind::Variant, is_pub(&node.vis));
        });
        self.qualifier = outer;
        syn::visit::visit_item_enum(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        self.def(&node.ident, DefKind::Trait, is_pub(&node.vis));
        let outer = std::mem::replace(&mut self.qualifier, node.ident.to_string());
        node.items.iter().for_each(|item| {
            if let syn::TraitItem::Fn(f) = item {
                self.def(&f.sig.ident, DefKind::TraitItem, true);
            }
        });
        self.qualifier = outer;
        syn::visit::visit_item_trait(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let outer = std::mem::replace(&mut self.qualifier, type_name(&node.self_ty));
        node.items.iter().for_each(|item| {
            if let syn::ImplItem::Fn(f) = item {
                let public = is_pub(&f.vis) || node.trait_.is_some();
                self.def(&f.sig.ident, DefKind::Method, public);
            }
        });
        syn::visit::visit_item_impl(self, node);
        self.qualifier = outer;
    }

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        self.def(&node.ident, DefKind::TypeAlias, is_pub(&node.vis));
        syn::visit::visit_item_type(self, node);
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        self.def(&node.ident, DefKind::Const, is_pub(&node.vis));
        syn::visit::visit_item_const(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        self.def(&node.ident, DefKind::Static, is_pub(&node.vis));
        syn::visit::visit_item_static(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        self.def(&node.ident, DefKind::Mod, is_pub(&node.vis));
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_item_union(&mut self, node: &'ast syn::ItemUnion) {
        self.def(&node.ident, DefKind::Union, is_pub(&node.vis));
        syn::visit::visit_item_union(self, node);
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        node.ident
            .as_ref()
            .map(|i| self.def(i, DefKind::Macro, true));
        syn::visit::visit_item_macro(self, node);
    }

    // ---- references --------------------------------------------------------

    fn visit_use_tree(&mut self, node: &'ast syn::UseTree) {
        match node {
            syn::UseTree::Path(p) => self.reference(&p.ident, RefKind::Import),
            syn::UseTree::Name(n) => self.reference(&n.ident, RefKind::Import),
            syn::UseTree::Rename(r) => self.reference(&r.ident, RefKind::Import),
            syn::UseTree::Glob(_) | syn::UseTree::Group(_) => {}
        }
        syn::visit::visit_use_tree(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        self.reference(&node.method, RefKind::Call);
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        // `foo(..)` and `a::b::foo(..)` — the callee's last segment is the name
        // being called; the earlier segments are recorded as paths by the path
        // visitor below, so a module name still shows up as a reference.
        if let syn::Expr::Path(p) = &*node.func {
            if let Some(seg) = p.path.segments.last() {
                self.reference(&seg.ident, RefKind::Call);
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        // Every segment, so `axiom_host::FrameCamera::IDENTITY` records the
        // crate, the type and the constant. A call's final segment is already
        // recorded as a Call; recording it again as a Path would double-count,
        // so the call visitor and this one are reconciled at query time by
        // preferring the more specific kind on the same (file, line, name).
        node.segments
            .iter()
            .for_each(|s| self.reference(&s.ident, RefKind::Path));
        syn::visit::visit_path(self, node);
    }

    /// **Attributes are not code, and are not indexed.**
    ///
    /// `syn` models a doc comment as `#[doc = "..."]`, so descending into
    /// attributes made `doc` the single most-referenced name in this repo —
    /// 107,422 hits, 12% of the whole index. That is the regex false-positive
    /// class this index exists to remove, reintroduced through the back door.
    /// `#[cfg(feature = "x")]` and `#[derive(Debug)]` are the same story: real
    /// metadata, not references to a `cfg` or a `Debug` the agent can go and
    /// change.
    fn visit_attribute(&mut self, _node: &'ast syn::Attribute) {}

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        node.path
            .segments
            .last()
            .map(|s| self.reference(&s.ident, RefKind::Macro));
        // Deliberately NOT descending into the token stream: it is unparsed
        // tokens, and guessing at their meaning is how a "semantic" index
        // quietly becomes a regex again.
        syn::visit::visit_macro(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(src: &str) -> (Vec<Def>, Vec<Ref>) {
        let ast = syn::parse_file(src).expect("the fixture parses");
        let (mut defs, mut refs) = (Vec::new(), Vec::new());
        let mut v = Collector {
            file: "fixture.rs".to_owned(),
            qualifier: String::new(),
            defs: &mut defs,
            refs: &mut refs,
        };
        v.visit_file(&ast);
        (defs, refs)
    }

    /// **A name inside a doc comment, a line comment or a string is not a
    /// reference.** This is the entire reason the index exists.
    ///
    /// The regex it replaces could not tell the difference, and on this repo
    /// that was not academic: searching one type returned 123 hits where 105
    /// were real, and the difference was prose — including a comment naming an
    /// API that no longer exists.
    #[test]
    fn prose_is_not_a_reference() {
        let (_, refs) = collect(
            r#"
            /// Mentions Target in a doc comment.
            // Mentions Target in a line comment.
            fn f() {
                let s = "Target in a string";
                let _ = s;
            }
            "#,
        );
        assert!(
            !refs.iter().any(|r| r.name == "Target"),
            "prose leaked into the index: {refs:?}"
        );
    }

    /// **Attributes are not references either.**
    ///
    /// `syn` models a doc comment as `#[doc = "..."]`, so descending into
    /// attributes made `doc` the most-referenced name in the repo — 107,422
    /// hits, 12% of the index — which is the regex false-positive class walking
    /// back in through a different door.
    #[test]
    fn attributes_are_not_indexed() {
        let (_, refs) = collect(
            r#"
            #[derive(Debug)]
            #[cfg(feature = "x")]
            /// doc
            pub struct S;
            "#,
        );
        ["doc", "derive", "cfg", "feature"].iter().for_each(|n| {
            assert!(
                !refs.iter().any(|r| &r.name == n),
                "attribute `{n}` was indexed as a reference"
            );
        });
    }

    /// A reference carries **how** the name was used, which is the thing a text
    /// search cannot say and the thing that makes a result list scannable.
    #[test]
    fn a_reference_records_its_syntactic_role() {
        let (_, refs) = collect(
            r#"
            use other::Thing;
            fn f(t: Thing) {
                helper(t);
                t.method();
                shout!();
            }
            "#,
        );
        let kind = |n: &str| refs.iter().find(|r| r.name == n).map(|r| r.kind);
        assert_eq!(kind("Thing"), Some(RefKind::Import), "the use item comes first");
        assert_eq!(kind("helper"), Some(RefKind::Call));
        assert_eq!(kind("method"), Some(RefKind::Call), "a method call is a call");
        assert_eq!(kind("shout"), Some(RefKind::Macro));
    }

    /// **A method is qualified by the type it hangs on**, so two `new`s are
    /// distinguishable in the output even though this index does not resolve
    /// types.
    #[test]
    fn a_method_carries_its_impl_type() {
        let (defs, _) = collect(
            r#"
            struct A;
            struct B;
            impl A { pub fn new() -> A { A } }
            impl B { pub fn new() -> B { B } }
            "#,
        );
        let news: Vec<&Def> = defs.iter().filter(|d| d.name == "new").collect();
        assert_eq!(news.len(), 2);
        let mut quals: Vec<&str> = news.iter().map(|d| d.qualifier.as_str()).collect();
        quals.sort_unstable();
        assert_eq!(quals, vec!["A", "B"]);
        assert!(news.iter().all(|d| d.kind == DefKind::Method));
    }

    /// Every item form this repo actually uses is found, with the right kind
    /// and the right visibility.
    #[test]
    fn the_item_forms_are_all_recognised() {
        let (defs, _) = collect(
            r#"
            pub fn free() {}
            fn private() {}
            pub struct S { pub field: u8 }
            pub enum E { Variant }
            pub trait T { fn required(&self); }
            pub type Alias = u8;
            pub const C: u8 = 0;
            pub static ST: u8 = 0;
            pub mod m {}
            pub union U { a: u8 }
            "#,
        );
        let kind = |n: &str| defs.iter().find(|d| d.name == n).map(|d| d.kind);
        assert_eq!(kind("free"), Some(DefKind::Fn));
        assert_eq!(kind("S"), Some(DefKind::Struct));
        assert_eq!(kind("field"), Some(DefKind::Field));
        assert_eq!(kind("E"), Some(DefKind::Enum));
        assert_eq!(kind("Variant"), Some(DefKind::Variant));
        assert_eq!(kind("T"), Some(DefKind::Trait));
        assert_eq!(kind("required"), Some(DefKind::TraitItem));
        assert_eq!(kind("Alias"), Some(DefKind::TypeAlias));
        assert_eq!(kind("C"), Some(DefKind::Const));
        assert_eq!(kind("ST"), Some(DefKind::Static));
        assert_eq!(kind("m"), Some(DefKind::Mod));
        assert_eq!(kind("U"), Some(DefKind::Union));

        let public = |n: &str| defs.iter().find(|d| d.name == n).map(|d| d.public);
        assert_eq!(public("free"), Some(true));
        assert_eq!(public("private"), Some(false));
    }

    /// Grammar words are not navigable names and were 5% of the index.
    #[test]
    fn grammar_keywords_are_not_references() {
        let (_, refs) = collect(
            r#"
            impl S {
                fn f(&self) -> Self { let _ = crate::x::y; Self }
            }
            "#,
        );
        ["self", "Self", "crate", "super"].iter().for_each(|n| {
            assert!(!refs.iter().any(|r| &r.name == n), "`{n}` was indexed");
        });
    }

    /// **A definition is not a use of itself.** Listing the definition line
    /// among the references is the noise that made the regex version tiring.
    #[test]
    fn refs_of_excludes_the_definition_line() {
        let (defs, refs) = collect("fn thing() {}\nfn other() { thing(); }\n");
        let shard = Shard { defs, refs };
        let found = shard.refs_of("thing");
        assert_eq!(found.len(), 1, "only the call site: {found:?}");
        assert_eq!(found[0].line, 2);
    }

    /// The shard hash is **stable** — it decides which file a name's data lives
    /// in, so a hash that varied per run or per platform would silently return
    /// an empty answer instead of a wrong one, which is worse.
    #[test]
    fn the_shard_hash_is_stable_and_bounded() {
        assert_eq!(shard_of("FrameCamera"), shard_of("FrameCamera"));
        ["a", "", "FrameCamera", "a_very_long_symbol_name_indeed"]
            .iter()
            .for_each(|n| assert!(shard_of(n) < SHARDS, "{n} escaped the shard range"));
        // Two different names should not all land in one bucket.
        let spread: std::collections::BTreeSet<usize> =
            (0..64).map(|i| shard_of(&format!("sym{i}"))).collect();
        assert!(spread.len() > 20, "the hash barely spreads: {} buckets", spread.len());
    }
}
