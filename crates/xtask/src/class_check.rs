//! Cross-class dependency-graph enforcement: layers, modules, apps, and
//! tools each have their own dependency rules. This module runs on top of
//! the workspace dep graph from `cargo metadata` and the classification
//! produced by [`crate::classification`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::cargo_metadata::WorkspaceGraph;
use crate::classification::{classify, Classified, ManifestIndex, PackageClass};
use crate::violation::{CheckReport, Violation, ViolationKind};

/// Run every cross-class rule. Pushes violations into `report`.
pub fn check(
    root: &Path,
    graph: &WorkspaceGraph,
    index: &ManifestIndex,
    report: &mut CheckReport,
) -> Vec<Classified> {
    let classified = classify_all(root, graph, index, report);
    if classified.is_empty() {
        return classified;
    }

    let class_by_name: BTreeMap<&str, PackageClass> = classified
        .iter()
        .map(|c| (c.package.name.as_str(), c.class))
        .collect();

    check_duplicate_module_names(index, report);
    check_duplicate_module_capabilities(index, report);
    check_module_manifest_local_rules(index, report);
    check_feature_module_allowed_module_references(index, report);
    check_layer_manifest_crate_name_matches(index, &class_by_name, report);
    check_module_manifest_crate_name_matches(index, report);
    check_app_manifest_crate_name_matches(index, report);
    check_module_allowed_layer_references(index, report);
    check_app_allowed_layer_references(index, report);
    check_app_allowed_module_references(index, report);
    check_forward_dependencies(&classified, &class_by_name, index, report);
    check_apps_are_leaves(&classified, &class_by_name, report);
    check_tools_not_used_by_engine(&classified, &class_by_name, report);
    check_module_facades_export_one(index, report);

    classified
}

fn check_module_facades_export_one(index: &ManifestIndex, report: &mut CheckReport) {
    for m in index.module_by_dir.values() {
        let lib_rs = m.src_dir().join("lib.rs");
        let text = match std::fs::read_to_string(&lib_rs) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let stripped = crate::rust_source::strip_line_comments(&text);
        let public_exports: Vec<&str> = stripped
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
            .collect();
        if public_exports.len() != 1 {
            report.push(
                Violation::new(
                    ViolationKind::ModuleFacadeMustExportOne,
                    m.module.name.clone(),
                    format!(
                        "module `{}` must publicly export exactly one facade from lib.rs, found {}: {:?}",
                        m.module.name,
                        public_exports.len(),
                        public_exports
                    ),
                )
                .at(lib_rs, 1),
            );
        }
    }
}

fn classify_all(
    root: &Path,
    graph: &WorkspaceGraph,
    index: &ManifestIndex,
    report: &mut CheckReport,
) -> Vec<Classified> {
    let mut out = Vec::new();
    for pkg in &graph.packages {
        match classify(root, pkg, index) {
            Some(class) => out.push(Classified {
                package: pkg.clone(),
                class,
            }),
            None => {
                report.push(Violation::new(
                    ViolationKind::UnknownPackageClass,
                    pkg.name.clone(),
                    format!(
                        "workspace package `{}` could not be classified as a layer, module, app, or tool; \
                         place it under `crates/<name>/` with a `layer.toml`, `modules/<name>/` with a \
                         `module.toml`, `apps/<name>/` with an `app.toml`, or `tools/<name>/`",
                        pkg.name
                    ),
                ));
            }
        }
    }
    out
}

fn check_duplicate_module_names(index: &ManifestIndex, report: &mut CheckReport) {
    let mut seen: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for m in index.module_by_dir.values() {
        seen.entry(m.module.name.as_str())
            .or_default()
            .push(m.module.crate_name.as_str());
    }
    for (name, crates) in seen {
        if crates.len() > 1 {
            report.push(Violation::new(
                ViolationKind::DuplicateModuleName,
                name.to_string(),
                format!(
                    "module name `{name}` is declared by multiple crates: {crates:?}; \
                     module names must be unique"
                ),
            ));
        }
    }
}

fn check_duplicate_module_capabilities(index: &ManifestIndex, report: &mut CheckReport) {
    let mut owner_of: BTreeMap<&str, &str> = BTreeMap::new();
    for m in index.module_by_dir.values() {
        for cap in &m.module.introduced_capabilities {
            if let Some(prev) = owner_of.insert(cap.as_str(), m.module.name.as_str()) {
                report.push(Violation::new(
                    ViolationKind::DuplicateModuleCapability,
                    m.module.name.clone(),
                    format!(
                        "capability `{cap}` is introduced by both module `{prev}` and module `{}`; \
                         capabilities must be unique across modules",
                        m.module.name
                    ),
                ));
            }
        }
    }
}

fn check_module_manifest_local_rules(index: &ManifestIndex, report: &mut CheckReport) {
    for m in index.module_by_dir.values() {
        // Only *engine* modules are barred from composing other modules.
        // Feature modules (`kind = "feature-module"`) may list modules they
        // compose; those references are validated separately.
        if !m.module.is_feature_module() && !m.module.allowed_modules.is_empty() {
            report.push(Violation::new(
                ViolationKind::ModuleHasNonEmptyAllowedModules,
                m.module.name.clone(),
                format!(
                    "engine module `{}` declares non-empty `allowed_modules` = {:?}; \
                     only feature modules (kind = \"feature-module\") may compose other \
                     modules — leave `allowed_modules = []` or mark this a feature module",
                    m.module.name, m.module.allowed_modules
                ),
            ));
        }
    }
}

/// A feature module's `allowed_modules` must name real modules.
fn check_feature_module_allowed_module_references(index: &ManifestIndex, report: &mut CheckReport) {
    let known_modules: BTreeSet<&str> = index
        .module_by_dir
        .values()
        .map(|m| m.module.name.as_str())
        .collect();
    for m in index.module_by_dir.values() {
        if !m.module.is_feature_module() {
            continue;
        }
        for module_name in &m.module.allowed_modules {
            if !known_modules.contains(module_name.as_str()) {
                report.push(Violation::new(
                    ViolationKind::ModuleAllowedModuleUnknown,
                    m.module.name.clone(),
                    format!(
                        "feature module `{}` allows module `{module_name}`, but no such module exists; \
                         valid module names are: {known_modules:?}",
                        m.module.name
                    ),
                ));
            }
        }
    }
}

fn check_layer_manifest_crate_name_matches(
    index: &ManifestIndex,
    class_by_name: &BTreeMap<&str, PackageClass>,
    report: &mut CheckReport,
) {
    // Layer manifests may default the crate name. We only flag a mismatch
    // when the cargo package at the same dir has a different name from the
    // explicit `crate_name` field.
    for m in index.layer_by_dir.values() {
        let explicit = match &m.layer.crate_name {
            Some(n) => n.clone(),
            None => continue,
        };
        if !class_by_name.contains_key(explicit.as_str()) {
            report.push(Violation::new(
                ViolationKind::ManifestCrateNameMismatch,
                m.layer.name.clone(),
                format!(
                    "layer `{}` declares crate_name = `{}` but no workspace package by that name exists",
                    m.layer.name, explicit
                ),
            ));
        }
    }
}

fn check_module_manifest_crate_name_matches(index: &ManifestIndex, report: &mut CheckReport) {
    for m in index.module_by_dir.values() {
        // The package at this dir must have the declared crate_name.
        // We don't have the package here; the check is enforced indirectly:
        // if `crate_name` doesn't match any classified package, the dir
        // classification will catch it via `UnknownPackageClass`. For
        // explicit ergonomics we also validate the name shape here.
        let cn = &m.module.crate_name;
        if cn.trim().is_empty() {
            report.push(Violation::new(
                ViolationKind::ModuleManifestInvalid,
                m.module.name.clone(),
                "module manifest has empty `crate_name`",
            ));
        }
    }
}

fn check_app_manifest_crate_name_matches(index: &ManifestIndex, report: &mut CheckReport) {
    for a in index.app_by_dir.values() {
        if a.app.crate_name.trim().is_empty() {
            report.push(Violation::new(
                ViolationKind::AppManifestInvalid,
                a.app.name.clone(),
                "app manifest has empty `crate_name`",
            ));
        }
    }
}

fn check_module_allowed_layer_references(index: &ManifestIndex, report: &mut CheckReport) {
    let known_layers: BTreeSet<&str> = index
        .layer_by_dir
        .values()
        .map(|l| l.layer.name.as_str())
        .collect();
    for m in index.module_by_dir.values() {
        for layer_name in &m.module.allowed_layers {
            if !known_layers.contains(layer_name.as_str()) {
                report.push(Violation::new(
                    ViolationKind::ModuleAllowedLayerUnknown,
                    m.module.name.clone(),
                    format!(
                        "module `{}` allows layer `{layer_name}`, but no such layer exists; \
                         valid layer names are: {known_layers:?}",
                        m.module.name
                    ),
                ));
            }
        }
    }
}

fn check_app_allowed_layer_references(index: &ManifestIndex, report: &mut CheckReport) {
    let known_layers: BTreeSet<&str> = index
        .layer_by_dir
        .values()
        .map(|l| l.layer.name.as_str())
        .collect();
    for a in index.app_by_dir.values() {
        for layer_name in &a.app.allowed_layers {
            if !known_layers.contains(layer_name.as_str()) {
                report.push(Violation::new(
                    ViolationKind::AppAllowedLayerUnknown,
                    a.app.name.clone(),
                    format!(
                        "app `{}` allows layer `{layer_name}`, but no such layer exists; \
                         valid layer names are: {known_layers:?}",
                        a.app.name
                    ),
                ));
            }
        }
    }
}

fn check_app_allowed_module_references(index: &ManifestIndex, report: &mut CheckReport) {
    let known_modules: BTreeSet<&str> = index
        .module_by_dir
        .values()
        .map(|m| m.module.name.as_str())
        .collect();
    for a in index.app_by_dir.values() {
        for module_name in &a.app.allowed_modules {
            if !known_modules.contains(module_name.as_str()) {
                report.push(Violation::new(
                    ViolationKind::AppAllowedModuleUnknown,
                    a.app.name.clone(),
                    format!(
                        "app `{}` allows module `{module_name}`, but no such module exists; \
                         valid module names are: {known_modules:?}",
                        a.app.name
                    ),
                ));
            }
        }
    }
}

fn check_forward_dependencies(
    classified: &[Classified],
    class_by_name: &BTreeMap<&str, PackageClass>,
    index: &ManifestIndex,
    report: &mut CheckReport,
) {
    // Lookups by cargo package name → module/app manifest (for allowlists).
    let module_by_crate: BTreeMap<&str, &crate::module_manifest::ModuleManifest> = index
        .module_by_dir
        .values()
        .map(|m| (m.module.crate_name.as_str(), m))
        .collect();
    let app_by_crate: BTreeMap<&str, &crate::app_manifest::AppManifest> = index
        .app_by_dir
        .values()
        .map(|a| (a.app.crate_name.as_str(), a))
        .collect();
    let layer_by_crate: BTreeMap<String, &crate::manifest::LayerManifest> = index
        .layer_by_dir
        .values()
        .map(|l| {
            let cn = l
                .layer
                .crate_name
                .clone()
                .unwrap_or_else(|| format!("axiom-{}", l.layer.name));
            (cn, l)
        })
        .collect();

    for c in classified {
        for dep_name in &c.package.workspace_deps {
            let dep_class = match class_by_name.get(dep_name.as_str()) {
                Some(c) => *c,
                None => continue, // dep is itself an unclassified package; flagged separately
            };

            match c.class {
                PackageClass::Layer => match dep_class {
                    PackageClass::Module => report.push(Violation::new(
                        ViolationKind::LayerDependsOnModule,
                        c.package.name.clone(),
                        format!(
                            "layer crate `{}` depends on module crate `{dep_name}`; \
                             layers must never depend on modules",
                            c.package.name
                        ),
                    )),
                    PackageClass::App => report.push(Violation::new(
                        ViolationKind::LayerDependsOnApp,
                        c.package.name.clone(),
                        format!(
                            "layer crate `{}` depends on app crate `{dep_name}`; \
                             layers must never depend on apps",
                            c.package.name
                        ),
                    )),
                    PackageClass::Tool => report.push(Violation::new(
                        ViolationKind::LayerDependsOnTool,
                        c.package.name.clone(),
                        format!(
                            "layer crate `{}` depends on tool crate `{dep_name}`; \
                             layers must never depend on tools",
                            c.package.name
                        ),
                    )),
                    PackageClass::Layer => { /* allowed: the existing layer law handles index ordering */
                    }
                    PackageClass::Support => { /* allowed: any engine code may depend on the support crate */
                    }
                },
                PackageClass::Module => match dep_class {
                    PackageClass::Module => {
                        // Engine modules may never depend on a module. A feature
                        // module may depend on the modules it lists in
                        // `allowed_modules`; anything else is a violation.
                        let src = module_by_crate.get(c.package.name.as_str());
                        let is_feature = src.is_some_and(|s| s.module.is_feature_module());
                        if is_feature {
                            let target_name = module_by_crate
                                .get(dep_name.as_str())
                                .map(|t| t.module.name.clone());
                            let allowed = match &target_name {
                                Some(name) => src
                                    .expect("is_feature implies src is Some")
                                    .module
                                    .allowed_modules
                                    .contains(name),
                                None => false,
                            };
                            if !allowed {
                                report.push(Violation::new(
                                    ViolationKind::ModuleDependsOnModuleNotAllowed,
                                    c.package.name.clone(),
                                    format!(
                                        "feature module `{}` depends on module crate `{dep_name}` but its \
                                         `allowed_modules` is {:?}; add `{}` to `allowed_modules` or drop the dependency",
                                        c.package.name,
                                        src.expect("is_feature implies src is Some").module.allowed_modules,
                                        target_name.unwrap_or_else(|| dep_name.clone())
                                    ),
                                ));
                            }
                        } else {
                            report.push(Violation::new(
                                ViolationKind::ModuleDependsOnModule,
                                c.package.name.clone(),
                                format!(
                                    "engine module crate `{}` depends on module crate `{dep_name}`; \
                                     only feature modules may compose other modules",
                                    c.package.name
                                ),
                            ));
                        }
                    }
                    PackageClass::App => report.push(Violation::new(
                        ViolationKind::ModuleDependsOnApp,
                        c.package.name.clone(),
                        format!(
                            "module crate `{}` depends on app crate `{dep_name}`; \
                             modules must never depend on apps",
                            c.package.name
                        ),
                    )),
                    PackageClass::Tool => report.push(Violation::new(
                        ViolationKind::ModuleDependsOnTool,
                        c.package.name.clone(),
                        format!(
                            "module crate `{}` depends on tool crate `{dep_name}`; \
                             modules must never depend on tools",
                            c.package.name
                        ),
                    )),
                    PackageClass::Layer => {
                        if let Some(module) = module_by_crate.get(c.package.name.as_str()) {
                            let layer_name = layer_by_crate
                                .iter()
                                .find_map(|(crate_name, l)| {
                                    (crate_name == dep_name).then(|| l.layer.name.clone())
                                });
                            let allowed = match &layer_name {
                                Some(name) => module.module.allowed_layers.contains(name),
                                None => false,
                            };
                            if !allowed {
                                report.push(Violation::new(
                                    ViolationKind::ModuleDependsOnLayerNotAllowed,
                                    c.package.name.clone(),
                                    format!(
                                        "module `{}` depends on layer crate `{dep_name}` but its `allowed_layers` is {:?}; \
                                         add `{}` to `allowed_layers` or drop the dependency",
                                        module.module.name,
                                        module.module.allowed_layers,
                                        layer_name.unwrap_or_else(|| dep_name.clone())
                                    ),
                                ));
                            }
                        }
                    }
                    PackageClass::Support => { /* allowed: any engine code may depend on the support crate */
                    }
                },
                PackageClass::App => match dep_class {
                    PackageClass::App => report.push(Violation::new(
                        ViolationKind::AppDependsOnApp,
                        c.package.name.clone(),
                        format!(
                            "app crate `{}` depends on app crate `{dep_name}`; \
                             apps must not depend on other apps",
                            c.package.name
                        ),
                    )),
                    PackageClass::Tool => report.push(Violation::new(
                        ViolationKind::AppDependsOnTool,
                        c.package.name.clone(),
                        format!(
                            "app crate `{}` depends on tool crate `{dep_name}`; \
                             apps must not depend on tools",
                            c.package.name
                        ),
                    )),
                    PackageClass::Layer => {
                        if let Some(app) = app_by_crate.get(c.package.name.as_str()) {
                            let layer_name = layer_by_crate
                                .iter()
                                .find_map(|(crate_name, l)| {
                                    (crate_name == dep_name).then(|| l.layer.name.clone())
                                });
                            let allowed = match &layer_name {
                                Some(name) => app.app.allowed_layers.contains(name),
                                None => false,
                            };
                            if !allowed {
                                report.push(Violation::new(
                                    ViolationKind::AppDependsOnLayerNotAllowed,
                                    c.package.name.clone(),
                                    format!(
                                        "app `{}` depends on layer crate `{dep_name}` but its `allowed_layers` is {:?}; \
                                         add `{}` to `allowed_layers` or drop the dependency",
                                        app.app.name,
                                        app.app.allowed_layers,
                                        layer_name.unwrap_or_else(|| dep_name.clone())
                                    ),
                                ));
                            }
                        }
                    }
                    PackageClass::Module => {
                        if let Some(app) = app_by_crate.get(c.package.name.as_str()) {
                            let module_name = module_by_crate
                                .get(dep_name.as_str())
                                .map(|m| m.module.name.clone());
                            let allowed = match &module_name {
                                Some(name) => app.app.allowed_modules.contains(name),
                                None => false,
                            };
                            if !allowed {
                                report.push(Violation::new(
                                    ViolationKind::AppDependsOnModuleNotAllowed,
                                    c.package.name.clone(),
                                    format!(
                                        "app `{}` depends on module crate `{dep_name}` but its `allowed_modules` is {:?}; \
                                         add `{}` to `allowed_modules` or drop the dependency",
                                        app.app.name,
                                        app.app.allowed_modules,
                                        module_name.unwrap_or_else(|| dep_name.clone())
                                    ),
                                ));
                            }
                        }
                    }
                    PackageClass::Support => { /* allowed: any engine code may depend on the support crate */
                    }
                },
                PackageClass::Tool => { /* tools are free to depend on whatever they need */ }
                PackageClass::Support => { /* the support crate depends only on external proc-macro crates */
                }
            }
        }
    }
}

fn check_apps_are_leaves(
    classified: &[Classified],
    class_by_name: &BTreeMap<&str, PackageClass>,
    report: &mut CheckReport,
) {
    let app_names: BTreeSet<&str> = classified
        .iter()
        .filter(|c| c.class == PackageClass::App)
        .map(|c| c.package.name.as_str())
        .collect();
    if app_names.is_empty() {
        return;
    }
    for c in classified {
        for dep_name in &c.package.workspace_deps {
            if app_names.contains(dep_name.as_str())
                && class_by_name.get(c.package.name.as_str()) != Some(&PackageClass::App)
            {
                // The dep-class branches above already flag layer/module/tool
                // -> app individually. This loop catches any other importer
                // shape (today only `Tool`, but stays robust if classes grow).
                if class_by_name.get(c.package.name.as_str()) == Some(&PackageClass::Tool) {
                    report.push(Violation::new(
                        ViolationKind::AppImportedBySomething,
                        c.package.name.clone(),
                        format!(
                            "tool crate `{}` depends on app crate `{dep_name}`; \
                             apps must not be depended on by anything",
                            c.package.name
                        ),
                    ));
                }
            }
        }
    }
}

fn check_tools_not_used_by_engine(
    classified: &[Classified],
    class_by_name: &BTreeMap<&str, PackageClass>,
    report: &mut CheckReport,
) {
    let tool_names: BTreeSet<&str> = classified
        .iter()
        .filter(|c| c.class == PackageClass::Tool)
        .map(|c| c.package.name.as_str())
        .collect();
    if tool_names.is_empty() {
        return;
    }
    for c in classified {
        if c.class == PackageClass::Tool {
            continue;
        }
        for dep_name in &c.package.workspace_deps {
            if !tool_names.contains(dep_name.as_str()) {
                continue;
            }
            // Layer/module/app -> tool: already flagged by the per-class
            // dispatch above; this is a defensive duplicate-safe check for
            // any future class. Skip if already-covered cases.
            match class_by_name.get(c.package.name.as_str()) {
                Some(PackageClass::Layer)
                | Some(PackageClass::Module)
                | Some(PackageClass::App) => {
                    // Already flagged with the more specific code; do not
                    // emit `ToolImportedByEngine` to avoid double-counting.
                }
                _ => report.push(Violation::new(
                    ViolationKind::ToolImportedByEngine,
                    c.package.name.clone(),
                    format!(
                        "non-tool crate `{}` depends on tool crate `{dep_name}`; \
                         tooling must not be part of the runtime dependency graph",
                        c.package.name
                    ),
                )),
            }
        }
    }
}
