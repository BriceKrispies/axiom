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

    for err in &parse_errors {
        let rel = relativize(root, &err.path);
        report.push(
            Violation::new(
                ViolationKind::ManifestInvalid,
                rel.display().to_string(),
                format!("could not parse layer manifest: {}", err.message),
            )
            .at(rel, 1),
        );
    }

    let (module_manifests, module_errors) = load_module_manifests(root);
    for err in &module_errors {
        let rel = relativize(root, &err.path);
        report.push(
            Violation::new(
                ViolationKind::ModuleManifestInvalid,
                rel.display().to_string(),
                format!("could not parse module manifest: {}", err.message),
            )
            .at(rel, 1),
        );
    }

    let (app_manifests, app_errors) = load_app_manifests(root);
    for err in &app_errors {
        let rel = relativize(root, &err.path);
        report.push(
            Violation::new(
                ViolationKind::AppManifestInvalid,
                rel.display().to_string(),
                format!("could not parse app manifest: {}", err.message),
            )
            .at(rel, 1),
        );
    }

    let manifest_index = ManifestIndex::new(&manifests, &module_manifests, &app_manifests);

    // Pure filesystem scans below run for every discovered layer and module
    // regardless of whether `cargo metadata` is available.
    let layer_dirs: Vec<(String, PathBuf)> = manifests
        .iter()
        .map(|m| (m.layer.name.clone(), m.src_dir()))
        .collect();
    let module_dirs: Vec<(String, PathBuf)> = module_manifests
        .iter()
        .map(|m| (m.module.name.clone(), m.src_dir()))
        .collect();

    // Centralized source hygiene scan.
    check_hygiene(&layer_dirs, &module_dirs, &mut report);

    // The coverage gate's ignore list may exclude only apps and tooling; prove
    // it never excludes a layer or module (the Axiom Coverage Law's scope).
    check_coverage_scope(root, &layer_dirs, &module_dirs, &mut report);

    // Cargo metadata is required for cross-class dependency-graph checks.
    // If it is unavailable (e.g. on a synthetic fixture that has no
    // `Cargo.toml`), we still emit every check above and finish.
    if let Ok(graph) = load_cargo_metadata(root) {
        check_classes(root, &graph, &manifest_index, &mut report);
    }

    if manifests.is_empty() {
        return report.finish();
    }

    // The set of declared layer names, and the prefix table for resolving
    // cross-layer references found in source.
    let known_names: BTreeSet<String> = manifests.iter().map(|m| m.layer.name.clone()).collect();
    let prefix_table: Vec<PrefixEntry> = manifests
        .iter()
        .map(|m| PrefixEntry {
            prefix: m.import_prefix(),
            name: m.layer.name.clone(),
        })
        .collect();

    // Layers form a directed acyclic graph: every `depends_on` edge must name a
    // real layer, and the graph must contain no cycle.
    check_dependency_graph(&manifests, &known_names, &mut report);

    // Layers in name order for a deterministic report.
    let mut ordered: Vec<&LayerManifest> = manifests.iter().collect();
    ordered.sort_by(|a, b| a.layer.name.cmp(&b.layer.name));
    report.layers_checked = ordered.iter().map(|m| m.layer.name.clone()).collect();

    for m in &ordered {
        check_layer(root, m, &prefix_table, &mut report);
    }

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
    for m in manifests {
        for dep in &m.layer.depends_on {
            if !known.contains(dep) {
                report.push(Violation::new(
                    ViolationKind::UnknownDependency,
                    &m.layer.name,
                    format!(
                        "layer `{}` lists `depends_on = [.. \"{dep}\" ..]`, but no layer named \
                         `{dep}` exists",
                        m.layer.name
                    ),
                ));
            }
        }
    }

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
    for start in names {
        if color[start] == 0 {
            let mut stack: Vec<&str> = Vec::new();
            if let Some(cycle) = dfs_cycle(start, &adj, &mut color, &mut stack) {
                report.push(Violation::new(
                    ViolationKind::DependencyCycle,
                    cycle.join(" -> "),
                    format!(
                        "the layer `depends_on` graph has a cycle: {}; layers must form a \
                         directed acyclic graph",
                        cycle.join(" -> ")
                    ),
                ));
                return; // one reported cycle is enough to fail the build.
            }
        }
    }
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
    for &next in &adj[node] {
        match color.get(next).copied().unwrap_or(2) {
            1 => {
                // Back-edge: build the cycle path from where `next` sits on the stack.
                let start = stack.iter().position(|n| *n == next).unwrap_or(0);
                let mut cycle: Vec<String> = stack[start..].iter().map(|s| s.to_string()).collect();
                cycle.push(next.to_string());
                return Some(cycle);
            }
            0 => {
                if let Some(c) = dfs_cycle(next, adj, color, stack) {
                    return Some(c);
                }
            }
            _ => {}
        }
    }
    stack.pop();
    color.insert(node, 2);
    None
}

fn check_layer(
    root: &Path,
    m: &LayerManifest,
    prefix_table: &[PrefixEntry],
    report: &mut CheckReport,
) {
    let name = &m.layer.name;

    // --- Read this layer's source once (comments stripped so a stray mention
    // in a comment cannot mask or invent a violation) ---
    // Read each file with `//` comments stripped (so a mention can't mask or
    // invent a violation). This is BEFORE test-code stripping, so the
    // `#[cfg(test)] mod NAME;` declarations are still visible.
    let commented: Vec<(PathBuf, String)> = collect_rs_files(&m.src_dir())
        .into_iter()
        .filter_map(|p| {
            std::fs::read_to_string(&p)
                .ok()
                .map(|t| (p, strip_line_comments(&t)))
        })
        .collect();

    // Files reachable only through a `#[cfg(test)] mod NAME;` declaration are
    // test-only; the gate lives in the declaring file, not in them. Collect
    // those names first, then drop the corresponding files and strip remaining
    // in-file test code — so the scan sees only non-test architecture,
    // consistent with the `engine_genuine_dependency` dylint.
    let test_module_names: BTreeSet<String> = commented
        .iter()
        .flat_map(|(_, text)| find_cfg_test_modules(text))
        .collect();
    let files: Vec<(PathBuf, String)> = commented
        .into_iter()
        .filter(|(path, _)| !is_test_module_file(path, &test_module_names))
        .map(|(path, text)| (path, strip_test_code(&text)))
        .collect();

    // Prefixes of OTHER layers (a layer referencing itself is intra-layer).
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

    for (path, text) in &files {
        let rel = relativize(root, path);
        for cross in find_cross_refs(text, &other_prefixes) {
            let Some(target) = prefix_lookup.get(cross.prefix.as_str()) else {
                continue;
            };

            // DAG rule: a layer may import only the layers in its `depends_on`.
            // "Genuinely uses each declared dependency" is enforced separately by
            // the `engine_genuine_dependency` dylint (which has real type info);
            // here we enforce the converse — no import of an undeclared layer.
            if !m.layer.depends_on.contains(&target.name) {
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
                continue;
            }

            if cross.private {
                // Reach only public exports, never private module paths.
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
            }
        }
    }

    // --- Introduced capabilities must be publicly exported ---
    for cap in &m.layer.introduced_capabilities {
        if locate_public_export(&files, cap).is_none() {
            report.push(Violation::new(
                ViolationKind::CapabilityNotExported,
                name,
                format!(
                    "introduced capability `{cap}` is not publicly exported by layer `{name}`; \
                     declare it `pub` (or `pub use ... {cap}`)"
                ),
            ));
        }
    }

    // --- Proof exports ---
    check_proof_exports(root, m, &files, report);
}

fn check_proof_exports(
    root: &Path,
    m: &LayerManifest,
    files: &[(PathBuf, String)],
    report: &mut CheckReport,
) {
    let name = &m.layer.name;

    // A root layer (empty `depends_on`, e.g. the kernel) adapts nothing, so it
    // proves nothing. Every non-root layer must expose at least one public
    // capability whose implementation uses a layer it depends on.
    if !m.layer.depends_on.is_empty() && m.proof_exports.is_empty() {
        report.push(Violation::new(
            ViolationKind::MissingProofExport,
            name,
            format!(
                "layer `{name}` declares no [[proof_exports]]; a non-root layer must expose at \
                 least one public capability whose implementation uses a layer it depends on"
            ),
        ));
        return;
    }

    for pe in &m.proof_exports {
        let Some((export_file, export_line)) = locate_public_export(files, &pe.export) else {
            report.push(Violation::new(
                ViolationKind::MissingProofExport,
                name,
                format!(
                    "proof export `{}` is not a public export of layer `{name}`; declare it `pub` \
                     or fix the name in layer.toml",
                    pe.export
                ),
            ));
            continue;
        };

        if pe.must_reference.is_empty() {
            continue; // Nothing to prove against.
        }

        // Candidate text: the file declaring the export, plus the module it
        // re-exports from (the facade pattern), if any.
        let mut candidate_texts: Vec<&str> = vec![text_of(files, &export_file)];
        if let Some(segments) = reexport_module_path(text_of(files, &export_file), &pe.export) {
            if let Some(module_text) = resolve_module_text(&m.src_dir(), &segments, files) {
                candidate_texts.push(module_text);
            }
        }

        let proven = pe
            .must_reference
            .iter()
            .any(|sym| candidate_texts.iter().any(|t| references_symbol(t, sym)));

        if !proven {
            let rel = relativize(root, &export_file);
            report.push(
                Violation::new(
                    ViolationKind::ProofReferenceMissing,
                    name,
                    format!(
                        "proof export `{}` does not reference any required previous-layer symbol \
                         {:?}; its implementation must use at least one of them",
                        pe.export, pe.must_reference
                    ),
                )
                .at(rel, export_line),
            );
        }
    }
}

// --- helpers ---

fn locate_public_export(files: &[(PathBuf, String)], name: &str) -> Option<(PathBuf, usize)> {
    for (path, text) in files {
        if let Some(line) = find_public_export(text, name) {
            return Some((path.clone(), line));
        }
    }
    None
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
    let mut base = src_dir.to_path_buf();
    for seg in segments {
        base.push(seg);
    }
    let as_file = base.with_extension("rs");
    let as_mod = base.join("mod.rs");
    for candidate in [as_file, as_mod] {
        if let Some((_, text)) = files.iter().find(|(p, _)| *p == candidate) {
            return Some(text.as_str());
        }
    }
    None
}

/// Is `path` the source file of a `#[cfg(test)] mod NAME;` module? The module
/// name is the file stem, or — for a `NAME/mod.rs` directory module — the parent
/// directory name.
fn is_test_module_file(path: &Path, test_module_names: &BTreeSet<String>) -> bool {
    let name = if path.file_name().and_then(|n| n.to_str()) == Some("mod.rs") {
        path.parent().and_then(|p| p.file_name())
    } else {
        path.file_stem()
    };
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
        // A directory with no `crates/` produces an empty, passing report.
        let tmp = std::env::temp_dir().join("axiom_xtask_empty_check");
        let _ = std::fs::create_dir_all(&tmp);
        let report = check_architecture(&tmp);
        assert!(report.is_ok());
        assert!(report.layers_checked.is_empty());
    }
}
