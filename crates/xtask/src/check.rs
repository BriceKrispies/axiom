//! The architecture checker core: turns layer manifests + source text into a
//! deterministic [`CheckReport`].
//!
//! Pure and deterministic: no clock, no randomness, every collection sorted
//! before iteration. The same repo always produces the same report.

use std::collections::BTreeMap;
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
    collect_rs_files, find_cross_refs, find_public_export, reexport_module_path, references_symbol,
    strip_line_comments,
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

    // Index -> first manifest at that index (duplicates flagged separately).
    let mut by_index: BTreeMap<u32, Vec<&LayerManifest>> = BTreeMap::new();
    for m in &manifests {
        by_index.entry(m.layer.index).or_default().push(m);
    }

    check_indexing(&by_index, &mut report);

    // index -> layer name, and the full prefix table for cross-ref resolution.
    let index_to_name: BTreeMap<u32, String> = by_index
        .iter()
        .map(|(idx, ms)| (*idx, ms[0].layer.name.clone()))
        .collect();
    let prefix_table: Vec<PrefixEntry> = manifests
        .iter()
        .map(|m| PrefixEntry {
            prefix: m.import_prefix(),
            name: m.layer.name.clone(),
            index: m.layer.index,
        })
        .collect();

    // Layers in index order for a deterministic report.
    let mut ordered: Vec<&LayerManifest> = manifests.iter().collect();
    ordered.sort_by_key(|m| (m.layer.index, m.layer.name.clone()));
    report.layers_checked = ordered.iter().map(|m| m.layer.name.clone()).collect();

    for m in &ordered {
        check_layer(root, m, &index_to_name, &prefix_table, &mut report);
    }

    report.finish()
}

struct PrefixEntry {
    prefix: String,
    name: String,
    index: u32,
}

fn check_indexing(by_index: &BTreeMap<u32, Vec<&LayerManifest>>, report: &mut CheckReport) {
    for (idx, ms) in by_index {
        if ms.len() > 1 {
            let names: Vec<&str> = ms.iter().map(|m| m.layer.name.as_str()).collect();
            report.push(Violation::new(
                ViolationKind::DuplicateIndex,
                names.join(", "),
                format!(
                    "layers {names:?} all declare index {idx}; each layer index must be unique"
                ),
            ));
        }
    }

    let found: Vec<u32> = by_index.keys().copied().collect();
    let expected: Vec<u32> = (0..found.len() as u32).collect();
    if found != expected {
        report.push(Violation::new(
            ViolationKind::IndexNotContinuous,
            "<workspace>",
            format!(
                "layer indexes must be the continuous sequence starting at 0; \
                 found {found:?} but expected {expected:?}"
            ),
        ));
    }
}

fn check_layer(
    root: &Path,
    m: &LayerManifest,
    index_to_name: &BTreeMap<u32, String>,
    prefix_table: &[PrefixEntry],
    report: &mut CheckReport,
) {
    let name = &m.layer.name;
    let index = m.layer.index;

    // --- Previous-layer link (rule 2) ---
    if index > 0 {
        let prev_index = index - 1;
        match index_to_name.get(&prev_index) {
            None => report.push(Violation::new(
                ViolationKind::MissingPreviousLayer,
                name,
                format!(
                    "layer `{name}` has index {index} but no layer declares index {prev_index}; \
                     indexes must form a continuous chain"
                ),
            )),
            Some(expected_prev) => match &m.layer.previous {
                None => report.push(Violation::new(
                    ViolationKind::MissingPreviousLayer,
                    name,
                    format!(
                        "layer `{name}` (index {index}) must set `previous = \"{expected_prev}\"` \
                         in its [layer] table"
                    ),
                )),
                Some(declared) if declared != expected_prev => report.push(Violation::new(
                    ViolationKind::PreviousNameMismatch,
                    name,
                    format!(
                        "layer `{name}` declares previous = \"{declared}\" but the layer at index \
                         {prev_index} is `{expected_prev}`; set previous = \"{expected_prev}\""
                    ),
                )),
                Some(_) => {}
            },
        }
    }

    // --- Read this layer's source once (comments stripped so a stray mention
    // in a comment cannot mask or invent a violation) ---
    let files: Vec<(PathBuf, String)> = collect_rs_files(&m.src_dir())
        .into_iter()
        .filter_map(|p| {
            std::fs::read_to_string(&p)
                .ok()
                .map(|t| (p, strip_line_comments(&t)))
        })
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

    let mut referenced_previous = false;

    for (path, text) in &files {
        let rel = relativize(root, path);
        for cross in find_cross_refs(text, &other_prefixes) {
            let Some(target) = prefix_lookup.get(cross.prefix.as_str()) else {
                continue;
            };

            if target.index >= index {
                // Rule 4: never import from a future (or same-level) layer.
                report.push(
                    Violation::new(
                        ViolationKind::FutureImport,
                        name,
                        format!(
                            "imports `{}` (layer `{}`, index {}) which is not below layer `{name}` \
                             (index {index}); a layer may import only layers with a lower index",
                            cross.prefix, target.name, target.index
                        ),
                    )
                    .at(rel.clone(), cross.line),
                );
                continue;
            }

            // Lower layer: must be explicitly allowed and not forbidden.
            let allowed = m.layer.allowed_dependencies.contains(&target.name);
            let forbidden = m.layer.forbidden_dependencies.contains(&target.name);
            if !allowed || forbidden {
                let reason = if forbidden {
                    "it is listed in `forbidden_dependencies`"
                } else {
                    "it is not listed in `allowed_dependencies`"
                };
                report.push(
                    Violation::new(
                        ViolationKind::DisallowedLayerImport,
                        name,
                        format!(
                            "imports layer `{}` but {reason}; add it to \
                             `allowed_dependencies` or remove the import",
                            target.name
                        ),
                    )
                    .at(rel.clone(), cross.line),
                );
            }

            if cross.private {
                // Rule 5: reach only public exports, never private module paths.
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

            if target.index == index - 1 {
                referenced_previous = true;
            }
        }
    }

    // --- Must meaningfully use the previous layer (rule 2) ---
    if index > 0 && index_to_name.contains_key(&(index - 1)) && !referenced_previous {
        let prev = &index_to_name[&(index - 1)];
        report.push(Violation::new(
            ViolationKind::MissingPreviousImport,
            name,
            format!(
                "layer `{name}` never references its previous layer `{prev}`; a layer must adapt \
                 the layer directly beneath it (import at least one `{}::` symbol)",
                prefix_for(prefix_table, prev)
            ),
        ));
    }

    // --- Introduced capabilities must be publicly exported (rule 6 + 3) ---
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

    // --- Proof exports (rule 3) ---
    check_proof_exports(root, m, &files, report);
}

fn check_proof_exports(
    root: &Path,
    m: &LayerManifest,
    files: &[(PathBuf, String)],
    report: &mut CheckReport,
) {
    let name = &m.layer.name;
    let index = m.layer.index;

    if index > 0 && m.proof_exports.is_empty() {
        report.push(Violation::new(
            ViolationKind::MissingProofExport,
            name,
            format!(
                "layer `{name}` declares no [[proof_exports]]; a non-kernel layer must expose at \
                 least one public capability whose implementation uses the previous layer"
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

fn prefix_for(prefix_table: &[PrefixEntry], layer_name: &str) -> String {
    prefix_table
        .iter()
        .find(|e| e.name == layer_name)
        .map(|e| e.prefix.clone())
        .unwrap_or_else(|| layer_name.to_string())
}

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
