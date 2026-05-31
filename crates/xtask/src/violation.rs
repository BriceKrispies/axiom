//! The structured result of an architecture check.
//!
//! A [`Violation`] is a single, specific, actionable failure. [`CheckReport`]
//! collects them deterministically (already sorted) so output and tests are
//! reproducible run-to-run.

use std::fmt;
use std::path::PathBuf;

/// The specific rule a [`Violation`] breaks. Tests assert on this so they do not
/// depend on exact message wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViolationKind {
    /// A `layer.toml` could not be parsed or was structurally invalid.
    ManifestInvalid,
    /// Two layers declare the same index.
    DuplicateIndex,
    /// Layer indexes are not the continuous sequence 0, 1, 2, ...
    IndexNotContinuous,
    /// A non-kernel layer is missing its `previous` link, or the previous layer
    /// (index N-1) does not exist.
    MissingPreviousLayer,
    /// `previous` names a layer that is not the one at index N-1.
    PreviousNameMismatch,
    /// A layer imports from a higher-or-equal-index (future) layer.
    FutureImport,
    /// A layer imports from a lower layer that is not in `allowed_dependencies`
    /// (or is explicitly in `forbidden_dependencies`).
    DisallowedLayerImport,
    /// A non-kernel layer never references its immediately previous layer.
    MissingPreviousImport,
    /// A cross-layer import reaches into another layer's private module path
    /// instead of using its public root export.
    PrivatePathImport,
    /// An `introduced_capabilities` symbol is not publicly exported by the layer.
    CapabilityNotExported,
    /// A non-kernel layer declares no proof exports, or a declared
    /// `proof_exports.export` is not a public export of the layer.
    MissingProofExport,
    /// A proof export exists but its implementation does not reference any of its
    /// required previous-layer `must_reference` symbols.
    ProofReferenceMissing,

    // --- Module / app / tool classification and dependency rules ---
    /// A workspace package could not be classified as a layer, module, app,
    /// or tool. Every workspace package must fit exactly one class.
    UnknownPackageClass,
    /// A `module.toml` could not be parsed or was structurally invalid.
    ModuleManifestInvalid,
    /// An `app.toml` could not be parsed or was structurally invalid.
    AppManifestInvalid,
    /// A layer's Cargo deps include a module crate.
    LayerDependsOnModule,
    /// A layer's Cargo deps include an app crate.
    LayerDependsOnApp,
    /// A layer's Cargo deps include a tool crate.
    LayerDependsOnTool,
    /// A module's Cargo deps include another module crate.
    ModuleDependsOnModule,
    /// A module's Cargo deps include a layer that is not in `allowed_layers`.
    ModuleDependsOnLayerNotAllowed,
    /// A module's Cargo deps include an app crate.
    ModuleDependsOnApp,
    /// A module's Cargo deps include a tool crate.
    ModuleDependsOnTool,
    /// A `module.toml` declares a non-empty `allowed_modules` list.
    ModuleHasNonEmptyAllowedModules,
    /// A `module.toml` names a layer in `allowed_layers` that does not exist.
    ModuleAllowedLayerUnknown,
    /// A module crate's `lib.rs` does not publicly export exactly one item.
    ModuleFacadeMustExportOne,
    /// Two modules declare the same `name`.
    DuplicateModuleName,
    /// Two modules introduce the same capability name.
    DuplicateModuleCapability,
    /// An app's Cargo deps include a layer that is not in `allowed_layers`.
    AppDependsOnLayerNotAllowed,
    /// An app's Cargo deps include a module that is not in `allowed_modules`.
    AppDependsOnModuleNotAllowed,
    /// An app's Cargo deps include another app crate.
    AppDependsOnApp,
    /// An app's Cargo deps include a tool crate.
    AppDependsOnTool,
    /// Another package depends on an app crate (apps are leaves).
    AppImportedBySomething,
    /// A non-tool crate depends on a tool crate (runtime/engine code must
    /// not depend on tooling).
    ToolImportedByEngine,
    /// An `app.toml` names a layer in `allowed_layers` that does not exist.
    AppAllowedLayerUnknown,
    /// An `app.toml` names a module in `allowed_modules` that does not exist.
    AppAllowedModuleUnknown,
    /// A layer or module crate name from a manifest does not match the
    /// actual cargo package name.
    ManifestCrateNameMismatch,

    // --- Source hygiene (centralized in xtask) ---
    /// A layer or module source file uses a forbidden macro (`println!`,
    /// `eprintln!`, `dbg!`, `todo!`, `unimplemented!`).
    SourceHygieneForbiddenMacro,
    /// A layer or module has a file named `utils.rs`, `helpers.rs`,
    /// `common.rs`, or `misc.rs` (or the same as a directory module).
    SourceHygieneJunkDrawerModule,
    /// A non-host layer or module references a browser/platform API
    /// (`web_sys`, `js_sys`, `wasm_bindgen`, `WebGPU`, `WebGL`,
    /// `requestAnimationFrame`, `window`, `document`, `canvas`).
    SourceHygieneBrowserApi,
    /// A layer or module source file uses the `#[coverage(off)]` attribute or
    /// the `coverage_attribute` feature to exclude code from coverage. Banned:
    /// coverage is earned by reachable tests, not by silencing the tool.
    SourceHygieneCoverageOff,

    // --- Coverage gate scope (the Axiom Coverage Law) ---
    /// The coverage gate's sanctioned `--ignore-filename-regex` matches a layer
    /// or module source path. The 100% gate may exclude only apps and tooling;
    /// excluding engine code is forbidden.
    CoverageIgnoreExcludesEngine,
    /// A `scripts/coverage.*` gate script does not apply exactly the sanctioned
    /// ignore pattern once (it is missing, altered, or a second ignore was
    /// added) — a path to silently widen what the gate hides.
    CoverageIgnoreScriptDrift,
}

impl fmt::Display for ViolationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A stable, greppable token for each kind.
        let token = match self {
            ViolationKind::ManifestInvalid => "ManifestInvalid",
            ViolationKind::DuplicateIndex => "DuplicateIndex",
            ViolationKind::IndexNotContinuous => "IndexNotContinuous",
            ViolationKind::MissingPreviousLayer => "MissingPreviousLayer",
            ViolationKind::PreviousNameMismatch => "PreviousNameMismatch",
            ViolationKind::FutureImport => "FutureImport",
            ViolationKind::DisallowedLayerImport => "DisallowedLayerImport",
            ViolationKind::MissingPreviousImport => "MissingPreviousImport",
            ViolationKind::PrivatePathImport => "PrivatePathImport",
            ViolationKind::CapabilityNotExported => "CapabilityNotExported",
            ViolationKind::MissingProofExport => "MissingProofExport",
            ViolationKind::ProofReferenceMissing => "ProofReferenceMissing",
            ViolationKind::UnknownPackageClass => "UnknownPackageClass",
            ViolationKind::ModuleManifestInvalid => "ModuleManifestInvalid",
            ViolationKind::AppManifestInvalid => "AppManifestInvalid",
            ViolationKind::LayerDependsOnModule => "LayerDependsOnModule",
            ViolationKind::LayerDependsOnApp => "LayerDependsOnApp",
            ViolationKind::LayerDependsOnTool => "LayerDependsOnTool",
            ViolationKind::ModuleDependsOnModule => "ModuleDependsOnModule",
            ViolationKind::ModuleDependsOnLayerNotAllowed => "ModuleDependsOnLayerNotAllowed",
            ViolationKind::ModuleDependsOnApp => "ModuleDependsOnApp",
            ViolationKind::ModuleDependsOnTool => "ModuleDependsOnTool",
            ViolationKind::ModuleHasNonEmptyAllowedModules => "ModuleHasNonEmptyAllowedModules",
            ViolationKind::ModuleAllowedLayerUnknown => "ModuleAllowedLayerUnknown",
            ViolationKind::ModuleFacadeMustExportOne => "ModuleFacadeMustExportOne",
            ViolationKind::DuplicateModuleName => "DuplicateModuleName",
            ViolationKind::DuplicateModuleCapability => "DuplicateModuleCapability",
            ViolationKind::AppDependsOnLayerNotAllowed => "AppDependsOnLayerNotAllowed",
            ViolationKind::AppDependsOnModuleNotAllowed => "AppDependsOnModuleNotAllowed",
            ViolationKind::AppDependsOnApp => "AppDependsOnApp",
            ViolationKind::AppDependsOnTool => "AppDependsOnTool",
            ViolationKind::AppImportedBySomething => "AppImportedBySomething",
            ViolationKind::ToolImportedByEngine => "ToolImportedByEngine",
            ViolationKind::AppAllowedLayerUnknown => "AppAllowedLayerUnknown",
            ViolationKind::AppAllowedModuleUnknown => "AppAllowedModuleUnknown",
            ViolationKind::ManifestCrateNameMismatch => "ManifestCrateNameMismatch",
            ViolationKind::SourceHygieneForbiddenMacro => "SourceHygieneForbiddenMacro",
            ViolationKind::SourceHygieneJunkDrawerModule => "SourceHygieneJunkDrawerModule",
            ViolationKind::SourceHygieneBrowserApi => "SourceHygieneBrowserApi",
            ViolationKind::SourceHygieneCoverageOff => "SourceHygieneCoverageOff",
            ViolationKind::CoverageIgnoreExcludesEngine => "CoverageIgnoreExcludesEngine",
            ViolationKind::CoverageIgnoreScriptDrift => "CoverageIgnoreScriptDrift",
        };
        f.write_str(token)
    }
}

/// One specific architecture-rule failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub kind: ViolationKind,
    /// The layer the failure is attributed to (logical layer name).
    pub layer: String,
    /// The source file the failure was found in, if any (relative to the repo).
    pub file: Option<PathBuf>,
    /// 1-based line number within `file`, if applicable.
    pub line: Option<usize>,
    /// A specific, actionable human message including the suggested remedy.
    pub message: String,
}

impl Violation {
    pub fn new(kind: ViolationKind, layer: impl Into<String>, message: impl Into<String>) -> Self {
        Violation {
            kind,
            layer: layer.into(),
            file: None,
            line: None,
            message: message.into(),
        }
    }

    pub fn at(mut self, file: PathBuf, line: usize) -> Self {
        self.file = Some(file);
        self.line = Some(line);
        self
    }

    /// The sort key that makes reports deterministic.
    fn sort_key(&self) -> (String, usize, String, String) {
        (
            self.file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            self.line.unwrap_or(0),
            self.kind.to_string(),
            self.layer.clone(),
        )
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let location = match (&self.file, self.line) {
            (Some(file), Some(line)) => format!("{}:{}", file.display(), line),
            (Some(file), None) => file.display().to_string(),
            _ => "<manifest>".to_string(),
        };
        write!(
            f,
            "{location} [{}] layer `{}`: {}",
            self.kind, self.layer, self.message
        )
    }
}

/// The full outcome of a check, with violations pre-sorted for determinism.
#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    /// Logical names of the layers that were discovered and checked, sorted by index.
    pub layers_checked: Vec<String>,
    violations: Vec<Violation>,
}

impl CheckReport {
    pub fn push(&mut self, violation: Violation) {
        self.violations.push(violation);
    }

    /// Whether the architecture is valid (no violations).
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }

    /// All violations, sorted deterministically.
    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }

    /// Whether any violation of `kind` is present (used by tests).
    pub fn has_kind(&self, kind: ViolationKind) -> bool {
        self.violations.iter().any(|v| v.kind == kind)
    }

    /// Sort violations into a stable order. Called once after collection.
    pub fn finish(mut self) -> Self {
        self.violations.sort_by_key(Violation::sort_key);
        self
    }
}
