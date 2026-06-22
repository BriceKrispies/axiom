//! Behavioral tests for the class-aware architecture checker
//! (layer / module / app / tool). Each test constructs a small synthetic
//! workspace under a temp directory, runs `check_architecture` against it,
//! and asserts the expected [`ViolationKind`].

// These flat fixture builders take one positional argument per field of the
// synthetic crate/module/app they assemble; folding them into params structs
// would only add ceremony to the test fixtures, so the argument-count lint is
// allowed across this test module.
#![allow(clippy::too_many_arguments)]

use std::path::{Path, PathBuf};

use xtask::check::check_architecture;
use xtask::violation::ViolationKind;

// ---------- temp-dir fixture builder ----------

/// A small helper that paves a synthetic workspace into a unique
/// temp directory. Each test gets a fresh root so tests can run in
/// parallel without sharing state.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("axiom_xtask_class_fx_{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn workspace(&self, members: &[&str]) -> &Self {
        let members_str = members
            .iter()
            .map(|m| format!("\"{m}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let body = format!("[workspace]\nresolver = \"2\"\nmembers = [{members_str}]\n");
        std::fs::write(self.root.join("Cargo.toml"), body).unwrap();
        self
    }

    fn layer_crate(
        &self,
        dir_rel: &str,
        crate_name: &str,
        layer_name: &str,
        index: u32,
        previous: Option<&str>,
        allowed_deps: &[&str],
        deps: &[(&str, &str)],
        lib_rs: &str,
    ) -> &Self {
        self.layer_crate_with_proof(
            dir_rel,
            crate_name,
            layer_name,
            index,
            previous,
            allowed_deps,
            deps,
            lib_rs,
            None,
        )
    }

    fn layer_crate_with_proof(
        &self,
        dir_rel: &str,
        crate_name: &str,
        layer_name: &str,
        index: u32,
        previous: Option<&str>,
        allowed_deps: &[&str],
        deps: &[(&str, &str)],
        lib_rs: &str,
        proof: Option<(&str, &[&str])>,
    ) -> &Self {
        let dir = self.root.join(dir_rel);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        self.write_cargo_toml(&dir, crate_name, deps);
        // `index`/`previous` are legacy parameters kept so existing call sites
        // compile; the DAG model expresses dependencies through `depends_on`
        // (populated from `allowed_deps`).
        let _ = (index, previous);
        let depends_list = allowed_deps
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let introduced = match proof {
            Some((export, _)) => format!("[\"{export}\"]"),
            None => "[]".to_string(),
        };
        let mut layer_toml = format!(
            "[layer]\n\
             name = \"{layer_name}\"\n\
             crate_name = \"{crate_name}\"\n\
             depends_on = [{depends_list}]\n\
             meaningful_dependency = \"fixture layer\"\n\
             introduced_capabilities = {introduced}\n\
             consumed_capabilities = []\n"
        );
        if let Some((export, must_ref)) = proof {
            let refs = must_ref
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", ");
            layer_toml.push_str(&format!(
                "\n[[proof_exports]]\nexport = \"{export}\"\nmust_reference = [{refs}]\n"
            ));
        }
        std::fs::write(dir.join("layer.toml"), layer_toml).unwrap();
        std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
        self
    }

    fn module_crate(
        &self,
        dir_rel: &str,
        crate_name: &str,
        module_name: &str,
        allowed_layers: &[&str],
        allowed_modules: &[&str],
        capabilities: &[&str],
        deps: &[(&str, &str)],
        lib_rs: &str,
    ) -> &Self {
        let dir = self.root.join(dir_rel);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        self.write_cargo_toml(&dir, crate_name, deps);
        let join = |xs: &[&str]| {
            xs.iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let module_toml = format!(
            "[module]\n\
             name = \"{module_name}\"\n\
             crate_name = \"{crate_name}\"\n\
             allowed_layers = [{}]\n\
             allowed_modules = [{}]\n\
             introduced_capabilities = [{}]\n",
            join(allowed_layers),
            join(allowed_modules),
            join(capabilities)
        );
        std::fs::write(dir.join("module.toml"), module_toml).unwrap();
        std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
        self
    }

    /// A feature (composition) module: `kind = "feature-module"`, so a
    /// non-empty `allowed_modules` is permitted.
    fn feature_module_crate(
        &self,
        dir_rel: &str,
        crate_name: &str,
        module_name: &str,
        allowed_layers: &[&str],
        allowed_modules: &[&str],
        capabilities: &[&str],
        deps: &[(&str, &str)],
        lib_rs: &str,
    ) -> &Self {
        let dir = self.root.join(dir_rel);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        self.write_cargo_toml(&dir, crate_name, deps);
        let join = |xs: &[&str]| {
            xs.iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let module_toml = format!(
            "[module]\n\
             name = \"{module_name}\"\n\
             crate_name = \"{crate_name}\"\n\
             kind = \"feature-module\"\n\
             allowed_layers = [{}]\n\
             allowed_modules = [{}]\n\
             introduced_capabilities = [{}]\n",
            join(allowed_layers),
            join(allowed_modules),
            join(capabilities)
        );
        std::fs::write(dir.join("module.toml"), module_toml).unwrap();
        std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
        self
    }

    fn app_crate(
        &self,
        dir_rel: &str,
        crate_name: &str,
        app_name: &str,
        allowed_layers: &[&str],
        allowed_modules: &[&str],
        deps: &[(&str, &str)],
        lib_rs: &str,
    ) -> &Self {
        let dir = self.root.join(dir_rel);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        self.write_cargo_toml(&dir, crate_name, deps);
        let join = |xs: &[&str]| {
            xs.iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let app_toml = format!(
            "[app]\n\
             name = \"{app_name}\"\n\
             crate_name = \"{crate_name}\"\n\
             allowed_layers = [{}]\n\
             allowed_modules = [{}]\n",
            join(allowed_layers),
            join(allowed_modules)
        );
        std::fs::write(dir.join("app.toml"), app_toml).unwrap();
        std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
        self
    }

    /// A workspace member with a Cargo.toml but no manifest of any kind.
    /// Used to test `UnknownPackageClass`.
    fn unclassified_crate(&self, dir_rel: &str, crate_name: &str, deps: &[(&str, &str)]) -> &Self {
        let dir = self.root.join(dir_rel);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        self.write_cargo_toml(&dir, crate_name, deps);
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
        self
    }

    fn write_cargo_toml(&self, dir: &Path, crate_name: &str, deps: &[(&str, &str)]) {
        let mut body = format!(
            "[package]\nname = \"{crate_name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[lib]\npath = \"src/lib.rs\"\n"
        );
        if !deps.is_empty() {
            body.push_str("\n[dependencies]\n");
            for (name, path) in deps {
                body.push_str(&format!("{name} = {{ path = \"{path}\" }}\n"));
            }
        }
        std::fs::write(dir.join("Cargo.toml"), body).unwrap();
    }
}

// A minimal lib.rs that publicly re-exports one item — the canonical shape
// for a module facade and a layer that touches its previous layer.
fn one_pub_facade() -> &'static str {
    "pub use self::inner::Facade;\nmod inner { pub struct Facade; }\n"
}

// A lib.rs that exports two public items — used to trigger
// `ModuleFacadeMustExportOne`.
fn two_pub_exports() -> &'static str {
    "pub use self::a::A;\npub use self::b::B;\nmod a { pub struct A; }\nmod b { pub struct B; }\n"
}

// A lib.rs that exports one facade plus an identity-vocabulary re-export
// (`pub use ids::…`). The ids carry no behavior, so this is the canonical shape
// for a handle-based module and must NOT trigger `ModuleFacadeMustExportOne`.
fn facade_plus_id_vocabulary() -> &'static str {
    "pub use self::inner::Facade;\npub use self::ids::Id;\nmod inner { pub struct Facade; }\nmod ids { pub struct Id; }\n"
}

// A lib.rs that imports the kernel layer's prefix (satisfies
// MissingPreviousImport for a non-kernel layer).
fn lib_using_axiomkernel() -> &'static str {
    "pub use self::inner::Facade;\nmod inner { use axiomkernel::*; pub struct Facade; }\n"
}

fn lib_using_real_kernel() -> &'static str {
    "pub use self::inner::Facade;\nmod inner { use kernelfx::*; pub struct Facade; }\n"
}

// ---------- tests ----------

#[test]
fn case_a_valid_layer_chain_passes() {
    let f = Fixture::new("a_valid_chain");
    f.workspace(&["crates/kernel", "crates/runtime"])
        .layer_crate_with_proof(
            "crates/kernel",
            "vch-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct KernelApi;\n",
            None,
        )
        .layer_crate_with_proof(
            "crates/runtime",
            "vch-runtime",
            "runtime",
            1,
            Some("kernel"),
            &["kernel"],
            &[("vch-kernel", "../kernel")],
            "pub use self::inner::Runtime;\n\
             mod inner {\n\
                 use vch_kernel::KernelApi;\n\
                 pub struct Runtime { _k: Option<KernelApi> }\n\
             }\n",
            Some(("Runtime", &["KernelApi"])),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.is_ok(),
        "expected clean report, got: {:?}",
        report.violations()
    );
}

#[test]
fn case_b_layer_importing_undeclared_layer_fails() {
    // Reuse the existing static fixture: `mid` imports `top`, which is not in
    // `mid`'s `depends_on`.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("02_disallowed_import");
    let report = check_architecture(&path);
    assert!(report.has_kind(ViolationKind::DisallowedLayerImport));
}

#[test]
fn case_c_layer_depending_on_module_fails() {
    let f = Fixture::new("c_layer_on_module");
    f.workspace(&["crates/kernel", "modules/scene"])
        .layer_crate(
            "crates/kernel",
            "lom-kernel",
            "kernel",
            0,
            None,
            &[],
            &[("lom-scene", "../../modules/scene")],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "lom-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::LayerDependsOnModule),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_d_layer_with_unknown_dependency_fails() {
    let f = Fixture::new("d_unknown_dependency");
    // `jumper` declares `depends_on = ["ghost"]`, but no `ghost` layer exists.
    f.workspace(&["crates/kernel", "crates/jumper"])
        .layer_crate(
            "crates/kernel",
            "ud-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .layer_crate(
            "crates/jumper",
            "ud-jumper",
            "jumper",
            1,
            Some("kernel"),
            &["ghost"],
            &[],
            "pub struct J;\n",
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::UnknownDependency),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_e_module_with_allowed_layers_only_passes() {
    let f = Fixture::new("e_module_allowed_only");
    f.workspace(&["crates/kernel", "modules/scene"])
        .layer_crate(
            "crates/kernel",
            "ealo-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "ealo-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[("ealo-kernel", "../../crates/kernel")],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.is_ok(),
        "expected clean report, got: {:?}",
        report.violations()
    );
}

#[test]
fn case_f_module_importing_unlisted_layer_fails() {
    let f = Fixture::new("f_module_unlisted_layer");
    f.workspace(&["crates/kernel", "crates/runtime", "modules/scene"])
        .layer_crate(
            "crates/kernel",
            "ful-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .layer_crate(
            "crates/runtime",
            "ful-runtime",
            "runtime",
            1,
            Some("kernel"),
            &["kernel"],
            &[("ful-kernel", "../kernel")],
            "pub use self::inner::R;\nmod inner { use ful_kernel::*; pub struct R; }\n",
        )
        // Module allows kernel but its Cargo deps include runtime.
        .module_crate(
            "modules/scene",
            "ful-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[
                ("ful-kernel", "../../crates/kernel"),
                ("ful-runtime", "../../crates/runtime"),
            ],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::ModuleDependsOnLayerNotAllowed),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_g_module_importing_another_module_fails() {
    let f = Fixture::new("g_module_to_module");
    f.workspace(&["crates/kernel", "modules/scene", "modules/render"])
        .layer_crate(
            "crates/kernel",
            "gmm-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "gmm-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[("gmm-kernel", "../../crates/kernel")],
            one_pub_facade(),
        )
        .module_crate(
            "modules/render",
            "gmm-render",
            "render",
            &["kernel"],
            &[],
            &["render-pipeline"],
            &[
                ("gmm-kernel", "../../crates/kernel"),
                ("gmm-scene", "../scene"),
            ],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::ModuleDependsOnModule),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_h_module_importing_app_fails() {
    let f = Fixture::new("h_module_to_app");
    f.workspace(&["crates/kernel", "modules/scene", "apps/demo"])
        .layer_crate(
            "crates/kernel",
            "hma-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .app_crate(
            "apps/demo",
            "hma-demo",
            "demo",
            &["kernel"],
            &[],
            &[("hma-kernel", "../../crates/kernel")],
            "pub fn main() {}\n",
        )
        .module_crate(
            "modules/scene",
            "hma-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[
                ("hma-kernel", "../../crates/kernel"),
                ("hma-demo", "../../apps/demo"),
            ],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::ModuleDependsOnApp),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_i_app_with_listed_layers_and_modules_passes() {
    let f = Fixture::new("i_app_valid");
    f.workspace(&["crates/kernel", "modules/scene", "apps/demo"])
        .layer_crate(
            "crates/kernel",
            "iav-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "iav-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[("iav-kernel", "../../crates/kernel")],
            one_pub_facade(),
        )
        .app_crate(
            "apps/demo",
            "iav-demo",
            "demo",
            &["kernel"],
            &["scene"],
            &[
                ("iav-kernel", "../../crates/kernel"),
                ("iav-scene", "../../modules/scene"),
            ],
            "pub fn main() {}\n",
        );
    let report = check_architecture(&f.root);
    assert!(
        report.is_ok(),
        "expected clean report, got: {:?}",
        report.violations()
    );
}

#[test]
fn case_j_app_importing_unlisted_module_fails() {
    let f = Fixture::new("j_app_unlisted_module");
    f.workspace(&["crates/kernel", "modules/scene", "apps/demo"])
        .layer_crate(
            "crates/kernel",
            "jau-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "jau-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[("jau-kernel", "../../crates/kernel")],
            one_pub_facade(),
        )
        .app_crate(
            "apps/demo",
            "jau-demo",
            "demo",
            &["kernel"],
            &[],
            &[
                ("jau-kernel", "../../crates/kernel"),
                ("jau-scene", "../../modules/scene"),
            ],
            "pub fn main() {}\n",
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::AppDependsOnModuleNotAllowed),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_k_app_imported_by_a_layer_fails() {
    // A layer crate that depends on an app crate. Reported as
    // `LayerDependsOnApp` (the per-class specific failure that subsumes
    // the generic "apps must be leaves" rule for this caller class).
    let f = Fixture::new("k_app_imported");
    f.workspace(&["crates/kernel", "apps/demo"])
        .app_crate(
            "apps/demo",
            "kim-demo",
            "demo",
            &["kernel"],
            &[],
            &[("kim-kernel", "../../crates/kernel")],
            "pub fn main() {}\n",
        )
        .layer_crate(
            "crates/kernel",
            "kim-kernel",
            "kernel",
            0,
            None,
            &[],
            &[("kim-demo", "../../apps/demo")],
            "pub struct K;\n",
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::LayerDependsOnApp),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_l_unknown_workspace_package_fails() {
    let f = Fixture::new("l_unknown_package");
    f.workspace(&["crates/kernel", "wildcard/stray"])
        .layer_crate(
            "crates/kernel",
            "luw-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .unclassified_crate("wildcard/stray", "luw-stray", &[]);
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::UnknownPackageClass),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_m_duplicate_module_names_fail() {
    let f = Fixture::new("m_dup_module_names");
    f.workspace(&["crates/kernel", "modules/scene", "modules/scene2"])
        .layer_crate(
            "crates/kernel",
            "mdn-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "mdn-scene-a",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[("mdn-kernel", "../../crates/kernel")],
            one_pub_facade(),
        )
        .module_crate(
            "modules/scene2",
            "mdn-scene-b",
            "scene", // same logical name as the first one
            &["kernel"],
            &[],
            &["other-cap"],
            &[("mdn-kernel", "../../crates/kernel")],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::DuplicateModuleName),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_n_duplicate_module_capabilities_fail() {
    let f = Fixture::new("n_dup_caps");
    f.workspace(&["crates/kernel", "modules/scene", "modules/render"])
        .layer_crate(
            "crates/kernel",
            "ndc-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "ndc-scene",
            "scene",
            &["kernel"],
            &[],
            &["shared-cap"],
            &[("ndc-kernel", "../../crates/kernel")],
            one_pub_facade(),
        )
        .module_crate(
            "modules/render",
            "ndc-render",
            "render",
            &["kernel"],
            &[],
            &["shared-cap"], // duplicate across modules
            &[("ndc-kernel", "../../crates/kernel")],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::DuplicateModuleCapability),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_o_non_empty_allowed_modules_fails() {
    let f = Fixture::new("o_nonempty_allowed_modules");
    f.workspace(&["crates/kernel", "modules/scene"])
        .layer_crate(
            "crates/kernel",
            "ona-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "ona-scene",
            "scene",
            &["kernel"],
            &["render"], // not allowed today
            &["scene-graph"],
            &[("ona-kernel", "../../crates/kernel")],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::ModuleHasNonEmptyAllowedModules),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_p_module_facade_must_export_one() {
    let f = Fixture::new("p_two_pub_exports");
    f.workspace(&["crates/kernel", "modules/scene"])
        .layer_crate(
            "crates/kernel",
            "pmf-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "pmf-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[("pmf-kernel", "../../crates/kernel")],
            two_pub_exports(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::ModuleFacadeMustExportOne),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_module_facade_plus_id_vocabulary_ok() {
    let f = Fixture::new("module_facade_plus_ids");
    f.workspace(&["crates/kernel", "modules/scene"])
        .layer_crate(
            "crates/kernel",
            "mfi-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "mfi-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[("mfi-kernel", "../../crates/kernel")],
            facade_plus_id_vocabulary(),
        );
    let report = check_architecture(&f.root);
    assert!(
        !report.has_kind(ViolationKind::ModuleFacadeMustExportOne),
        "one facade + an `ids` vocabulary re-export must be allowed; violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_q_forbidden_source_macro_fails() {
    let f = Fixture::new("q_forbidden_macro");
    f.workspace(&["crates/kernel"]).layer_crate(
        "crates/kernel",
        "qfm-kernel",
        "kernel",
        0,
        None,
        &[],
        &[],
        "pub fn boom() { println!(\"forbidden\"); }\n",
    );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::SourceHygieneForbiddenMacro),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_r_browser_api_in_non_host_layer_fails() {
    let f = Fixture::new("r_browser_api");
    f.workspace(&["crates/kernel"]).layer_crate(
        "crates/kernel",
        "rba-kernel",
        "kernel",
        0,
        None,
        &[],
        &[],
        "use web_sys::Window;\npub struct K;\n",
    );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::SourceHygieneBrowserApi),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_s_junk_drawer_module_fails() {
    let f = Fixture::new("s_junk_drawer");
    f.workspace(&["crates/kernel"]);
    // Layer + an extra utils.rs file alongside lib.rs.
    f.layer_crate(
        "crates/kernel",
        "sjd-kernel",
        "kernel",
        0,
        None,
        &[],
        &[],
        "pub struct K;\n",
    );
    std::fs::write(
        f.root.join("crates/kernel/src/utils.rs"),
        "pub fn nope() {}\n",
    )
    .unwrap();
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::SourceHygieneJunkDrawerModule),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_t_feature_module_composing_a_listed_module_passes() {
    // A feature module that declares `allowed_modules = ["scene"]` and depends
    // on the scene module is legal — the composition tier.
    let f = Fixture::new("t_feature_composes");
    f.workspace(&["crates/kernel", "modules/scene", "modules/pipeline"])
        .layer_crate(
            "crates/kernel",
            "fc-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "fc-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[("fc-kernel", "../../crates/kernel")],
            one_pub_facade(),
        )
        .feature_module_crate(
            "modules/pipeline",
            "fc-pipeline",
            "pipeline",
            &["kernel"],
            &["scene"],
            &["render-pipeline"],
            &[
                ("fc-kernel", "../../crates/kernel"),
                ("fc-scene", "../scene"),
            ],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.is_ok(),
        "expected clean report, got: {:?}",
        report.violations()
    );
}

#[test]
fn case_u_feature_module_depending_on_unlisted_module_fails() {
    // A feature module that depends on a module it did NOT list in
    // `allowed_modules` is rejected.
    let f = Fixture::new("u_feature_unlisted");
    f.workspace(&["crates/kernel", "modules/scene", "modules/pipeline"])
        .layer_crate(
            "crates/kernel",
            "fu-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .module_crate(
            "modules/scene",
            "fu-scene",
            "scene",
            &["kernel"],
            &[],
            &["scene-graph"],
            &[("fu-kernel", "../../crates/kernel")],
            one_pub_facade(),
        )
        .feature_module_crate(
            "modules/pipeline",
            "fu-pipeline",
            "pipeline",
            &["kernel"],
            &[], // does NOT list scene
            &["render-pipeline"],
            &[
                ("fu-kernel", "../../crates/kernel"),
                ("fu-scene", "../scene"),
            ],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::ModuleDependsOnModuleNotAllowed),
        "violations: {:?}",
        report.violations()
    );
}

#[test]
fn case_v_feature_module_allowing_unknown_module_fails() {
    // A feature module whose `allowed_modules` names a module that does not
    // exist is rejected.
    let f = Fixture::new("v_feature_unknown_allowed");
    f.workspace(&["crates/kernel", "modules/pipeline"])
        .layer_crate(
            "crates/kernel",
            "fv-kernel",
            "kernel",
            0,
            None,
            &[],
            &[],
            "pub struct K;\n",
        )
        .feature_module_crate(
            "modules/pipeline",
            "fv-pipeline",
            "pipeline",
            &["kernel"],
            &["ghost"], // no such module
            &["render-pipeline"],
            &[("fv-kernel", "../../crates/kernel")],
            one_pub_facade(),
        );
    let report = check_architecture(&f.root);
    assert!(
        report.has_kind(ViolationKind::ModuleAllowedModuleUnknown),
        "violations: {:?}",
        report.violations()
    );
}

/// The real repository must satisfy the new class-aware rules too.
#[test]
fn real_repo_class_aware_check_passes() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("repo root is two levels above crates/xtask");
    let report = check_architecture(&repo_root);
    assert!(
        report.is_ok(),
        "the real repo violates the class-aware architecture rules: {:?}",
        report.violations()
    );
}

// Helper references kept so dead-code lints don't fire if a test is
// commented out during local development.
#[allow(dead_code)]
fn _unused() {
    let _ = lib_using_axiomkernel();
    let _ = lib_using_real_kernel();
}
