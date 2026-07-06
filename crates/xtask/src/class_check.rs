//! Cross-class dependency-graph enforcement: layers, modules, apps, and
//! tools each have their own dependency rules. This module runs on top of
//! the workspace dep graph from `cargo metadata` and the classification
//! produced by [`crate::classification`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::cargo_metadata::WorkspaceGraph;
use crate::classification::{classify, Classified, ManifestIndex, PackageClass};
use crate::violation::{CheckReport, Violation, ViolationKind};

/// Tool crates sanctioned as capture **hosts**: a screenshot/capture harness
/// legitimately depends on the apps it renders. This is a narrowly-scoped,
/// documented amendment — the same shape as the `PLATFORM_FACING_*` allowlists
/// and the `axiom-zones` Support class — not a general loosening of the tool→app
/// ban: every *other* tool is still forbidden from depending on an app.
const CAPTURE_HARNESS_TOOLS: &[&str] = &["axiom-shot"];

/// Whether `crate_name` is a sanctioned capture-harness host.
fn is_capture_harness(crate_name: &str) -> bool {
    CAPTURE_HARNESS_TOOLS.contains(&crate_name)
}

/// Run every cross-class rule. Pushes violations into `report`.
pub fn check(
    root: &Path,
    graph: &WorkspaceGraph,
    index: &ManifestIndex,
    report: &mut CheckReport,
) -> Vec<Classified> {
    let classified = classify_all(root, graph, index, report);

    (!classified.is_empty()).then(|| {
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
    });

    classified
}

fn check_module_facades_export_one(index: &ManifestIndex, report: &mut CheckReport) {
    index.module_by_dir.values().for_each(|m| {
        let lib_rs = m.src_dir().join("lib.rs");
        std::fs::read_to_string(&lib_rs)
            .ok()
            .into_iter()
            .for_each(|text| {
                let stripped = crate::rust_source::strip_line_comments(&text);
                let public_exports: Vec<&str> = stripped
                    .lines()
                    .map(str::trim)
                    .filter(|line| line.starts_with("pub ") & !line.starts_with("pub(crate)"))
                    .collect();
                let facade_exports: Vec<&str> = public_exports
                    .iter()
                    .copied()
                    .filter(|line| !is_identity_vocabulary_export(line))
                    .collect();
                (facade_exports.len() != 1).then(|| {
                    report.push(
                        Violation::new(
                            ViolationKind::ModuleFacadeMustExportOne,
                            m.module.name.clone(),
                            format!(
                                "module `{}` must publicly export exactly one facade from lib.rs (an `ids` identity-vocabulary re-export is also allowed), found {} facade item(s): {:?}",
                                m.module.name,
                                facade_exports.len(),
                                facade_exports
                            ),
                        )
                        .at(lib_rs.clone(), 1),
                    );
                });
            });
    });
}

/// Whether a top-level `pub` line re-exports a module's identity vocabulary
/// (a `pub use` path containing a whole `ids` segment, e.g. `pub use
/// ids::{FactId, …};`). Matching on a whole segment avoids misclassifying
/// unrelated paths like `fluids::`.
fn is_identity_vocabulary_export(line: &str) -> bool {
    line.trim()
        .strip_prefix("pub use ")
        .map(|rest| {
            rest.split(|c: char| !(c.is_alphanumeric() | (c == '_')))
                .any(|segment| segment == "ids")
        })
        .unwrap_or(false)
}

fn classify_all(
    root: &Path,
    graph: &WorkspaceGraph,
    index: &ManifestIndex,
    report: &mut CheckReport,
) -> Vec<Classified> {
    let mut out = Vec::new();
    graph.packages.iter().for_each(|pkg| {
        classify(root, pkg, index).map_or_else(
            || {
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
            },
            |class| {
                out.push(Classified {
                    package: pkg.clone(),
                    class,
                });
            },
        );
    });
    out
}

fn check_duplicate_module_names(index: &ManifestIndex, report: &mut CheckReport) {
    let mut seen: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    index.module_by_dir.values().for_each(|m| {
        seen.entry(m.module.name.as_str())
            .or_default()
            .push(m.module.crate_name.as_str());
    });
    seen.into_iter().for_each(|(name, crates)| {
        (crates.len() > 1).then(|| {
            report.push(Violation::new(
                ViolationKind::DuplicateModuleName,
                name.to_string(),
                format!(
                    "module name `{name}` is declared by multiple crates: {crates:?}; \
                     module names must be unique"
                ),
            ));
        });
    });
}

fn check_duplicate_module_capabilities(index: &ManifestIndex, report: &mut CheckReport) {
    let mut owner_of: BTreeMap<&str, &str> = BTreeMap::new();
    index.module_by_dir.values().for_each(|m| {
        m.module.introduced_capabilities.iter().for_each(|cap| {
            owner_of
                .insert(cap.as_str(), m.module.name.as_str())
                .into_iter()
                .for_each(|prev| {
                    report.push(Violation::new(
                        ViolationKind::DuplicateModuleCapability,
                        m.module.name.clone(),
                        format!(
                            "capability `{cap}` is introduced by both module `{prev}` and module `{}`; \
                             capabilities must be unique across modules",
                            m.module.name
                        ),
                    ));
                });
        });
    });
}

fn check_module_manifest_local_rules(index: &ManifestIndex, report: &mut CheckReport) {
    index.module_by_dir.values().for_each(|m| {
        // Feature modules may list modules they compose (validated
        // separately); only engine modules are barred from composing.
        (!m.module.is_feature_module() & !m.module.allowed_modules.is_empty()).then(|| {
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
        });
    });
}

/// A feature module's `allowed_modules` must name real modules.
fn check_feature_module_allowed_module_references(index: &ManifestIndex, report: &mut CheckReport) {
    let known_modules: BTreeSet<&str> = index
        .module_by_dir
        .values()
        .map(|m| m.module.name.as_str())
        .collect();
    index
        .module_by_dir
        .values()
        .filter(|m| m.module.is_feature_module())
        .for_each(|m| {
            m.module
                .allowed_modules
                .iter()
                .filter(|module_name| !known_modules.contains(module_name.as_str()))
                .for_each(|module_name| {
                    report.push(Violation::new(
                        ViolationKind::ModuleAllowedModuleUnknown,
                        m.module.name.clone(),
                        format!(
                            "feature module `{}` allows module `{module_name}`, but no such module exists; \
                             valid module names are: {known_modules:?}",
                            m.module.name
                        ),
                    ));
                });
        });
}

fn check_layer_manifest_crate_name_matches(
    index: &ManifestIndex,
    class_by_name: &BTreeMap<&str, PackageClass>,
    report: &mut CheckReport,
) {
    // Layer manifests may default the crate name; only flag a mismatch when
    // an explicit `crate_name` names no known package.
    index
        .layer_by_dir
        .values()
        .filter_map(|m| m.layer.crate_name.clone().map(|explicit| (m, explicit)))
        .filter(|(_, explicit)| !class_by_name.contains_key(explicit.as_str()))
        .for_each(|(m, explicit)| {
            report.push(Violation::new(
                ViolationKind::ManifestCrateNameMismatch,
                m.layer.name.clone(),
                format!(
                    "layer `{}` declares crate_name = `{}` but no workspace package by that name exists",
                    m.layer.name, explicit
                ),
            ));
        });
}

fn check_module_manifest_crate_name_matches(index: &ManifestIndex, report: &mut CheckReport) {
    // A non-matching `crate_name` is caught indirectly via
    // `UnknownPackageClass`; this only validates the name isn't empty.
    index
        .module_by_dir
        .values()
        .filter(|m| m.module.crate_name.trim().is_empty())
        .for_each(|m| {
            report.push(Violation::new(
                ViolationKind::ModuleManifestInvalid,
                m.module.name.clone(),
                "module manifest has empty `crate_name`",
            ));
        });
}

fn check_app_manifest_crate_name_matches(index: &ManifestIndex, report: &mut CheckReport) {
    index
        .app_by_dir
        .values()
        .filter(|a| a.app.crate_name.trim().is_empty())
        .for_each(|a| {
            report.push(Violation::new(
                ViolationKind::AppManifestInvalid,
                a.app.name.clone(),
                "app manifest has empty `crate_name`",
            ));
        });
}

fn check_module_allowed_layer_references(index: &ManifestIndex, report: &mut CheckReport) {
    let known_layers: BTreeSet<&str> = index
        .layer_by_dir
        .values()
        .map(|l| l.layer.name.as_str())
        .collect();
    index.module_by_dir.values().for_each(|m| {
        m.module
            .allowed_layers
            .iter()
            .filter(|layer_name| !known_layers.contains(layer_name.as_str()))
            .for_each(|layer_name| {
                report.push(Violation::new(
                    ViolationKind::ModuleAllowedLayerUnknown,
                    m.module.name.clone(),
                    format!(
                        "module `{}` allows layer `{layer_name}`, but no such layer exists; \
                         valid layer names are: {known_layers:?}",
                        m.module.name
                    ),
                ));
            });
    });
}

fn check_app_allowed_layer_references(index: &ManifestIndex, report: &mut CheckReport) {
    let known_layers: BTreeSet<&str> = index
        .layer_by_dir
        .values()
        .map(|l| l.layer.name.as_str())
        .collect();
    index.app_by_dir.values().for_each(|a| {
        a.app
            .allowed_layers
            .iter()
            .filter(|layer_name| !known_layers.contains(layer_name.as_str()))
            .for_each(|layer_name| {
                report.push(Violation::new(
                    ViolationKind::AppAllowedLayerUnknown,
                    a.app.name.clone(),
                    format!(
                        "app `{}` allows layer `{layer_name}`, but no such layer exists; \
                         valid layer names are: {known_layers:?}",
                        a.app.name
                    ),
                ));
            });
    });
}

fn check_app_allowed_module_references(index: &ManifestIndex, report: &mut CheckReport) {
    let known_modules: BTreeSet<&str> = index
        .module_by_dir
        .values()
        .map(|m| m.module.name.as_str())
        .collect();
    index.app_by_dir.values().for_each(|a| {
        a.app
            .allowed_modules
            .iter()
            .filter(|module_name| !known_modules.contains(module_name.as_str()))
            .for_each(|module_name| {
                report.push(Violation::new(
                    ViolationKind::AppAllowedModuleUnknown,
                    a.app.name.clone(),
                    format!(
                        "app `{}` allows module `{module_name}`, but no such module exists; \
                         valid module names are: {known_modules:?}",
                        a.app.name
                    ),
                ));
            });
    });
}

fn check_forward_dependencies(
    classified: &[Classified],
    class_by_name: &BTreeMap<&str, PackageClass>,
    index: &ManifestIndex,
    report: &mut CheckReport,
) {
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

    classified.iter().for_each(|c| {
        c.package.workspace_deps.iter().for_each(|dep_name| {
            // `None` => dep is itself an unclassified package; flagged separately.
            class_by_name
                .get(dep_name.as_str())
                .copied()
                .into_iter()
                .for_each(|dep_class| {
            // Each illegal `(source, dep)` pair is one arm of a flat
            // `.then().or_else()` chain keyed on `pair == (Src, Dep)`;
            // allowed pairs are simply absent and fall through to `None`.
            // The chain runs for its violation-pushing side effects; the
            // resulting `Option<()>` is discarded.
            let pair = (c.class, dep_class);
            use PackageClass::{App, Layer, Module, Tool};

            (pair == (Layer, Module))
                .then(|| {
                    report.push(Violation::new(
                        ViolationKind::LayerDependsOnModule,
                        c.package.name.clone(),
                        format!(
                            "layer crate `{}` depends on module crate `{dep_name}`; \
                             layers must never depend on modules",
                            c.package.name
                        ),
                    ));
                })
                .or_else(|| {
                    (pair == (Layer, App)).then(|| {
                        report.push(Violation::new(
                            ViolationKind::LayerDependsOnApp,
                            c.package.name.clone(),
                            format!(
                                "layer crate `{}` depends on app crate `{dep_name}`; \
                                 layers must never depend on apps",
                                c.package.name
                            ),
                        ));
                    })
                })
                .or_else(|| {
                    (pair == (Layer, Tool)).then(|| {
                        report.push(Violation::new(
                            ViolationKind::LayerDependsOnTool,
                            c.package.name.clone(),
                            format!(
                                "layer crate `{}` depends on tool crate `{dep_name}`; \
                                 layers must never depend on tools",
                                c.package.name
                            ),
                        ));
                    })
                })
                .or_else(|| {
                    (pair == (Module, Module)).then(|| {
                        let feature_src = module_by_crate
                            .get(c.package.name.as_str())
                            .filter(|s| s.module.is_feature_module());
                        let violation = feature_src.map_or_else(
                            || {
                                Some(Violation::new(
                                    ViolationKind::ModuleDependsOnModule,
                                    c.package.name.clone(),
                                    format!(
                                        "engine module crate `{}` depends on module crate `{dep_name}`; \
                                         only feature modules may compose other modules",
                                        c.package.name
                                    ),
                                ))
                            },
                            |src| {
                                let target_name = module_by_crate
                                    .get(dep_name.as_str())
                                    .map(|t| t.module.name.clone());
                                let allowed = target_name
                                    .as_ref()
                                    .is_some_and(|name| src.module.allowed_modules.contains(name));
                                (!allowed).then(|| {
                                    Violation::new(
                                        ViolationKind::ModuleDependsOnModuleNotAllowed,
                                        c.package.name.clone(),
                                        format!(
                                            "feature module `{}` depends on module crate `{dep_name}` but its \
                                             `allowed_modules` is {:?}; add `{}` to `allowed_modules` or drop the dependency",
                                            c.package.name,
                                            src.module.allowed_modules,
                                            target_name.unwrap_or_else(|| dep_name.clone())
                                        ),
                                    )
                                })
                            },
                        );
                        violation.into_iter().for_each(|v| report.push(v));
                    })
                })
                .or_else(|| {
                    (pair == (Module, App)).then(|| {
                        report.push(Violation::new(
                            ViolationKind::ModuleDependsOnApp,
                            c.package.name.clone(),
                            format!(
                                "module crate `{}` depends on app crate `{dep_name}`; \
                                 modules must never depend on apps",
                                c.package.name
                            ),
                        ));
                    })
                })
                .or_else(|| {
                    (pair == (Module, Tool)).then(|| {
                        report.push(Violation::new(
                            ViolationKind::ModuleDependsOnTool,
                            c.package.name.clone(),
                            format!(
                                "module crate `{}` depends on tool crate `{dep_name}`; \
                                 modules must never depend on tools",
                                c.package.name
                            ),
                        ));
                    })
                })
                .or_else(|| {
                    (pair == (Module, Layer)).then(|| {
                        module_by_crate
                            .get(c.package.name.as_str())
                            .into_iter()
                            .for_each(|module| {
                                let layer_name =
                                    layer_by_crate.iter().find_map(|(crate_name, l)| {
                                        (crate_name == dep_name).then(|| l.layer.name.clone())
                                    });
                                let allowed = layer_name
                                    .as_ref()
                                    .is_some_and(|name| module.module.allowed_layers.contains(name));
                                (!allowed).then(|| {
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
                                });
                            });
                    })
                })
                .or_else(|| {
                    (pair == (App, App)).then(|| {
                        report.push(Violation::new(
                            ViolationKind::AppDependsOnApp,
                            c.package.name.clone(),
                            format!(
                                "app crate `{}` depends on app crate `{dep_name}`; \
                                 apps must not depend on other apps",
                                c.package.name
                            ),
                        ));
                    })
                })
                .or_else(|| {
                    (pair == (App, Tool)).then(|| {
                        report.push(Violation::new(
                            ViolationKind::AppDependsOnTool,
                            c.package.name.clone(),
                            format!(
                                "app crate `{}` depends on tool crate `{dep_name}`; \
                                 apps must not depend on tools",
                                c.package.name
                            ),
                        ));
                    })
                })
                .or_else(|| {
                    (pair == (App, Layer)).then(|| {
                        app_by_crate
                            .get(c.package.name.as_str())
                            .into_iter()
                            .for_each(|app| {
                                let layer_name =
                                    layer_by_crate.iter().find_map(|(crate_name, l)| {
                                        (crate_name == dep_name).then(|| l.layer.name.clone())
                                    });
                                let allowed = layer_name
                                    .as_ref()
                                    .is_some_and(|name| app.app.allowed_layers.contains(name));
                                (!allowed).then(|| {
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
                                });
                            });
                    })
                })
                .or_else(|| {
                    (pair == (App, Module)).then(|| {
                        app_by_crate
                            .get(c.package.name.as_str())
                            .into_iter()
                            .for_each(|app| {
                                let module_name = module_by_crate
                                    .get(dep_name.as_str())
                                    .map(|m| m.module.name.clone());
                                let allowed = module_name
                                    .as_ref()
                                    .is_some_and(|name| app.app.allowed_modules.contains(name));
                                (!allowed).then(|| {
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
                                });
                            });
                    })
                });
                });
        });
    });
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
    // An empty `app_names` makes every `contains` below false, so the chain is
    // a no-op without a guard.
    classified.iter().for_each(|c| {
        c.package
            .workspace_deps
            .iter()
            // The dep-class branches above already flag layer/module/tool ->
            // app individually; this catches any other importer shape.
            .filter(|dep_name| {
                app_names.contains(dep_name.as_str())
                    & (class_by_name.get(c.package.name.as_str()) != Some(&PackageClass::App))
                    & (class_by_name.get(c.package.name.as_str()) == Some(&PackageClass::Tool))
                    // A sanctioned capture harness loads the apps it renders.
                    & !is_capture_harness(&c.package.name)
            })
            .for_each(|dep_name| {
                report.push(Violation::new(
                    ViolationKind::AppImportedBySomething,
                    c.package.name.clone(),
                    format!(
                        "tool crate `{}` depends on app crate `{dep_name}`; \
                         apps must not be depended on by anything",
                        c.package.name
                    ),
                ));
            });
    });
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
    // An empty `tool_names` makes every `contains` below false, so the chain is
    // a no-op without a guard.
    classified
        .iter()
        .filter(|c| c.class != PackageClass::Tool)
        .for_each(|c| {
            c.package
                .workspace_deps
                .iter()
                .filter(|dep_name| tool_names.contains(dep_name.as_str()))
                // Layer/module/app -> tool is already flagged above; avoid
                // double-counting those here.
                .filter(|_| {
                    let importer = class_by_name.get(c.package.name.as_str()).copied();
                    ![
                        Some(PackageClass::Layer),
                        Some(PackageClass::Module),
                        Some(PackageClass::App),
                    ]
                    .contains(&importer)
                })
                .for_each(|dep_name| {
                    report.push(Violation::new(
                        ViolationKind::ToolImportedByEngine,
                        c.package.name.clone(),
                        format!(
                            "non-tool crate `{}` depends on tool crate `{dep_name}`; \
                             tooling must not be part of the runtime dependency graph",
                            c.package.name
                        ),
                    ));
                });
        });
}
