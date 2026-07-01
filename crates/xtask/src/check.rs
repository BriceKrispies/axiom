//! The architecture checker core: turns layer manifests + source text into a
//! deterministic [`CheckReport`].
//!
//! Pure and deterministic: no clock, no randomness, every collection sorted
//! before iteration. The same repo always produces the same report.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::app_manifest::load_app_manifests;
use crate::cargo_metadata::load as load_cargo_metadata;
use crate::class_check::check as check_classes;
use crate::classification::ManifestIndex;
use crate::coverage_scope::check as check_coverage_scope;
use crate::hygiene::check as check_hygiene;
use crate::manifest::{load_manifests, LayerManifest};
use crate::module_manifest::load_module_manifests;
use crate::rust_source::{
    collect_rs_files, find_cfg_test_modules, find_cross_refs, find_public_export,
    reexport_module_path, references_symbol, strip_line_comments, strip_test_code,
};
use crate::violation::{CheckReport, Violation, ViolationKind};

/// Run the full architecture check rooted at `root` (a directory containing a
/// `crates/` folder). Returns a sorted, deterministic report.
pub fn check_architecture(root: &Path) -> CheckReport {
    let mut report = CheckReport::default();
    let (manifests, parse_errors) = load_manifests(root);

    parse_errors.iter().for_each(|err| {
        let rel = relativize(root, &err.path);
        report.push(
            Violation::new(
                ViolationKind::ManifestInvalid,
                rel.display().to_string(),
                format!("could not parse layer manifest: {}", err.message),
            )
            .at(rel, 1),
        );
    });

    let (module_manifests, module_errors) = load_module_manifests(root);
    module_errors.iter().for_each(|err| {
        let rel = relativize(root, &err.path);
        report.push(
            Violation::new(
                ViolationKind::ModuleManifestInvalid,
                rel.display().to_string(),
                format!("could not parse module manifest: {}", err.message),
            )
            .at(rel, 1),
        );
    });

    let (app_manifests, app_errors) = load_app_manifests(root);
    app_errors.iter().for_each(|err| {
        let rel = relativize(root, &err.path);
        report.push(
            Violation::new(
                ViolationKind::AppManifestInvalid,
                rel.display().to_string(),
                format!("could not parse app manifest: {}", err.message),
            )
            .at(rel, 1),
        );
    });

    let manifest_index = ManifestIndex::new(&manifests, &module_manifests, &app_manifests);

    let layer_dirs: Vec<(String, PathBuf)> = manifests
        .iter()
        .map(|m| (m.layer.name.clone(), m.src_dir()))
        .collect();
    let module_dirs: Vec<(String, PathBuf)> = module_manifests
        .iter()
        .map(|m| (m.module.name.clone(), m.src_dir()))
        .collect();

    check_hygiene(&layer_dirs, &module_dirs, &mut report);

    check_coverage_scope(root, &layer_dirs, &module_dirs, &mut report);

    // If `cargo metadata` is unavailable (e.g. a synthetic fixture with no
    // `Cargo.toml`), cross-class checks are skipped and every other check
    // still runs to completion.
    load_cargo_metadata(root).into_iter().for_each(|graph| {
        check_classes(root, &graph, &manifest_index, &mut report);
    });

    // With no manifests, `known_names`/`prefix_table` are empty and every step
    // below becomes a no-op, so no early return is needed.
    let known_names: BTreeSet<String> = manifests.iter().map(|m| m.layer.name.clone()).collect();
    let prefix_table: Vec<PrefixEntry> = manifests
        .iter()
        .map(|m| PrefixEntry {
            prefix: m.import_prefix(),
            name: m.layer.name.clone(),
        })
        .collect();

    check_dependency_graph(&manifests, &known_names, &mut report);

    // Sorted by name for a deterministic report.
    let mut ordered: Vec<&LayerManifest> = manifests.iter().collect();
    ordered.sort_by(|a, b| a.layer.name.cmp(&b.layer.name));
    report.layers_checked = ordered.iter().map(|m| m.layer.name.clone()).collect();

    ordered
        .iter()
        .for_each(|m| check_layer(root, m, &prefix_table, &mut report));

    report.finish()
}

struct PrefixEntry {
    prefix: String,
    name: String,
}

/// Validate the layer dependency graph: every `depends_on` names a real layer,
/// and the graph (edges = `depends_on`) is acyclic.
fn check_dependency_graph(
    manifests: &[LayerManifest],
    known: &BTreeSet<String>,
    report: &mut CheckReport,
) {
    manifests.iter().for_each(|m| {
        m.layer
            .depends_on
            .iter()
            .filter(|dep| !known.contains(*dep))
            .for_each(|dep| {
                report.push(Violation::new(
                    ViolationKind::UnknownDependency,
                    &m.layer.name,
                    format!(
                        "layer `{}` lists `depends_on = [.. \"{dep}\" ..]`, but no layer named \
                         `{dep}` exists",
                        m.layer.name
                    ),
                ));
            });
    });

    // Adjacency over known edges only (unknown edges are reported above).
    let adj: BTreeMap<&str, Vec<&str>> = manifests
        .iter()
        .map(|m| {
            let mut deps: Vec<&str> = m
                .layer
                .depends_on
                .iter()
                .filter(|d| known.contains(*d))
                .map(String::as_str)
                .collect();
            deps.sort_unstable();
            (m.layer.name.as_str(), deps)
        })
        .collect();

    // 3-colour DFS for a cycle, visiting nodes in sorted order for determinism.
    let mut color: BTreeMap<&str, u8> = adj.keys().map(|k| (*k, 0u8)).collect();
    let mut names: Vec<&str> = adj.keys().copied().collect();
    names.sort_unstable();
    // `find_map` stops at the first cycle found and threads `color` across
    // starts statefully.
    let cycle = names.into_iter().find_map(|start| {
        (color[start] == 0).then_some(()).and_then(|()| {
            let mut stack: Vec<&str> = Vec::new();
            dfs_cycle(start, &adj, &mut color, &mut stack)
        })
    });
    cycle.into_iter().for_each(|cycle| {
        report.push(Violation::new(
            ViolationKind::DependencyCycle,
            cycle.join(" -> "),
            format!(
                "the layer `depends_on` graph has a cycle: {}; layers must form a \
                 directed acyclic graph",
                cycle.join(" -> ")
            ),
        ));
    });
}

/// DFS that returns the first cycle (as a path of layer names) it finds, or
/// `None`. Colours: 0 = unvisited, 1 = on the current stack, 2 = done.
fn dfs_cycle<'a>(
    node: &'a str,
    adj: &BTreeMap<&'a str, Vec<&'a str>>,
    color: &mut BTreeMap<&'a str, u8>,
    stack: &mut Vec<&'a str>,
) -> Option<Vec<String>> {
    color.insert(node, 1);
    stack.push(node);
    // A `1`-coloured (on-stack) neighbour is a back-edge; `0` is recursed
    // into; `2` (done) contributes nothing.
    let neighbours: Vec<&str> = adj[node].clone();
    let found = neighbours.into_iter().find_map(|next| {
        let col = color.get(next).copied().unwrap_or(2);
        (col == 1)
            .then(|| {
                let start = stack.iter().position(|n| *n == next).unwrap_or(0);
                let mut cycle: Vec<String> = stack[start..].iter().map(|s| s.to_string()).collect();
                cycle.push(next.to_string());
                cycle
            })
            .or_else(|| {
                (col == 0)
                    .then(|| dfs_cycle(next, adj, color, stack))
                    .flatten()
            })
    });
    stack.pop();
    // Only mark this node done when no cycle was found through it, or a
    // real back-edge could be missed on a later, differently-ordered visit.
    found.is_none().then(|| color.insert(node, 2));
    found
}

fn check_layer(
    root: &Path,
    m: &LayerManifest,
    prefix_table: &[PrefixEntry],
    report: &mut CheckReport,
) {
    let name = &m.layer.name;

    // Comments stripped first (so a stray mention can't mask/invent a
    // violation), before test-code stripping, so `#[cfg(test)] mod NAME;`
    // declarations are still visible below.
    let commented: Vec<(PathBuf, String)> = collect_rs_files(&m.src_dir())
        .into_iter()
        .filter_map(|p| {
            std::fs::read_to_string(&p)
                .ok()
                .map(|t| (p, strip_line_comments(&t)))
        })
        .collect();

    // Files reachable only through a `#[cfg(test)] mod NAME;` declaration are
    // test-only, so drop them and strip remaining in-file test code — the
    // scan then sees only non-test architecture, consistent with the
    // `engine_genuine_dependency` dylint.
    let test_module_names: BTreeSet<String> = commented
        .iter()
        .flat_map(|(_, text)| find_cfg_test_modules(text))
        .collect();
    let files: Vec<(PathBuf, String)> = commented
        .into_iter()
        .filter(|(path, _)| !is_test_module_file(path, &test_module_names))
        .map(|(path, text)| (path, strip_test_code(&text)))
        .collect();

    // A layer referencing itself is intra-layer, not a cross-layer import.
    let own_prefix = m.import_prefix();
    let other_prefixes: Vec<String> = prefix_table
        .iter()
        .filter(|e| e.prefix != own_prefix)
        .map(|e| e.prefix.clone())
        .collect();
    let prefix_lookup: BTreeMap<&str, &PrefixEntry> = prefix_table
        .iter()
        .map(|e| (e.prefix.as_str(), e))
        .collect();

    files.iter().for_each(|(path, text)| {
        let rel = relativize(root, path);
        find_cross_refs(text, &other_prefixes)
            .into_iter()
            .filter_map(|cross| {
                prefix_lookup
                    .get(cross.prefix.as_str())
                    .map(|target| (cross, *target))
            })
            .for_each(|(cross, target)| {
                // This enforces only the "no undeclared import" direction;
                // "genuinely uses each declared dependency" is checked
                // separately by the `engine_genuine_dependency` dylint, which
                // has real type info.
                let declared = m.layer.depends_on.contains(&target.name);
                (!declared).then(|| {
                    report.push(
                        Violation::new(
                            ViolationKind::DisallowedLayerImport,
                            name,
                            format!(
                                "imports layer `{}` but it is not in `depends_on`; add it (and \
                                 genuinely use it) or remove the import",
                                target.name
                            ),
                        )
                        .at(rel.clone(), cross.line),
                    );
                });
                (declared & cross.private).then(|| {
                    report.push(
                        Violation::new(
                            ViolationKind::PrivatePathImport,
                            name,
                            format!(
                                "imports through a private module path of layer `{}` (`{}::...`); \
                                 import the layer's public export instead",
                                target.name, cross.prefix
                            ),
                        )
                        .at(rel.clone(), cross.line),
                    );
                });
            });
    });

    m.layer
        .introduced_capabilities
        .iter()
        .filter(|cap| locate_public_export(&files, cap).is_none())
        .for_each(|cap| {
            report.push(Violation::new(
                ViolationKind::CapabilityNotExported,
                name,
                format!(
                    "introduced capability `{cap}` is not publicly exported by layer `{name}`; \
                     declare it `pub` (or `pub use ... {cap}`)"
                ),
            ));
        });

    check_proof_exports(root, m, &files, report);
}

fn check_proof_exports(
    root: &Path,
    m: &LayerManifest,
    files: &[(PathBuf, String)],
    report: &mut CheckReport,
) {
    let name = &m.layer.name;

    // A root layer (empty `depends_on`) proves nothing by definition; when
    // this fires, `proof_exports` is empty, so the per-export loop below is
    // a no-op without an early return.
    let missing_all = !m.layer.depends_on.is_empty() & m.proof_exports.is_empty();
    missing_all.then(|| {
        report.push(Violation::new(
            ViolationKind::MissingProofExport,
            name,
            format!(
                "layer `{name}` declares no [[proof_exports]]; a non-root layer must expose at \
                 least one public capability whose implementation uses a layer it depends on"
            ),
        ));
    });

    m.proof_exports.iter().for_each(|pe| {
        // Build at most one `Violation` here, then push it once (so `report`
        // is borrowed only once).
        let violation = locate_public_export(files, &pe.export).map_or_else(
            || {
                Some(Violation::new(
                    ViolationKind::MissingProofExport,
                    name,
                    format!(
                        "proof export `{}` is not a public export of layer `{name}`; declare it `pub` \
                         or fix the name in layer.toml",
                        pe.export
                    ),
                ))
            },
            |(export_file, export_line)| {
                // An empty `must_reference` has nothing to prove against.
                (!pe.must_reference.is_empty()).then(|| {
                    // Candidate text: the file declaring the export, plus the
                    // module it re-exports from (the facade pattern), if any.
                    let export_text = text_of(files, &export_file);
                    let module_text = reexport_module_path(export_text, &pe.export)
                        .and_then(|segments| resolve_module_text(&m.src_dir(), &segments, files));
                    let candidate_texts: Vec<&str> =
                        std::iter::once(export_text).chain(module_text).collect();

                    let proven = pe.must_reference.iter().any(|sym| {
                        candidate_texts.iter().any(|t| references_symbol(t, sym))
                    });

                    (!proven).then(|| {
                        let rel = relativize(root, &export_file);
                        Violation::new(
                            ViolationKind::ProofReferenceMissing,
                            name,
                            format!(
                                "proof export `{}` does not reference any required depended-layer symbol \
                                 {:?}; its implementation must use at least one of them",
                                pe.export, pe.must_reference
                            ),
                        )
                        .at(rel, export_line)
                    })
                })
                .flatten()
            },
        );
        violation.into_iter().for_each(|v| report.push(v));
    });
}

fn locate_public_export(files: &[(PathBuf, String)], name: &str) -> Option<(PathBuf, usize)> {
    files
        .iter()
        .find_map(|(path, text)| find_public_export(text, name).map(|line| (path.clone(), line)))
}

fn text_of<'a>(files: &'a [(PathBuf, String)], path: &Path) -> &'a str {
    files
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t.as_str())
        .unwrap_or("")
}

/// Resolve a module path (e.g. `["facade"]`) under `src_dir` to that module's
/// source text, trying `seg/.../name.rs` and `seg/.../name/mod.rs`.
fn resolve_module_text<'a>(
    src_dir: &Path,
    segments: &[String],
    files: &'a [(PathBuf, String)],
) -> Option<&'a str> {
    let base = segments
        .iter()
        .fold(src_dir.to_path_buf(), |mut base, seg| {
            base.push(seg);
            base
        });
    let as_file = base.with_extension("rs");
    let as_mod = base.join("mod.rs");
    [as_file, as_mod].into_iter().find_map(|candidate| {
        files
            .iter()
            .find(|(p, _)| *p == candidate)
            .map(|(_, text)| text.as_str())
    })
}

/// Is `path` the source file of a `#[cfg(test)] mod NAME;` module? The module
/// name is the file stem, or — for a `NAME/mod.rs` directory module — the parent
/// directory name.
fn is_test_module_file(path: &Path, test_module_names: &BTreeSet<String>) -> bool {
    let is_mod_rs = path.file_name().and_then(|n| n.to_str()) == Some("mod.rs");
    // A `NAME/mod.rs` directory module is named by its parent dir; otherwise by
    // the file stem.
    let name =
        [path.file_stem(), path.parent().and_then(|p| p.file_name())][usize::from(is_mod_rs)];
    name.and_then(|n| n.to_str())
        .is_some_and(|n| test_module_names.contains(n))
}

fn relativize(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_root_is_ok() {
        let tmp = std::env::temp_dir().join("axiom_xtask_empty_check");
        let _ = std::fs::create_dir_all(&tmp);
        let report = check_architecture(&tmp);
        assert!(report.is_ok());
        assert!(report.layers_checked.is_empty());
    }
}
