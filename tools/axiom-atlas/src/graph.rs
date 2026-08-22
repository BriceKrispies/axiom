//! The architecture query surface.
//!
//! Reuses the architecture checker's own manifest loaders, so the graph `ax`
//! reports and the graph `cargo xtask check-architecture` enforces are read
//! from one definition. If the schema moves, both move together.
//!
//! `ax owns <path>` is the point of this module: before an agent edits a file
//! it can ask which package owns it, and therefore which of Axiom's laws are in
//! force there - branchless or not, covered or not, allowed to touch `web_sys`
//! or not.

use std::fmt::Write as _;

use xtask::app_manifest::load_app_manifests;
use xtask::manifest::load_manifests;
use xtask::module_manifest::load_module_manifests;

use crate::repo::Repo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Layer,
    EngineModule,
    FeatureModule,
    App,
    Tool,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Self::Layer => "Layer (engine spine)",
            Self::EngineModule => "Engine module (isolated capability)",
            Self::FeatureModule => "Feature module (composition tier)",
            Self::App => "App (leaf composition root)",
            Self::Tool => "Tool (repo tooling, outside the engine graph)",
        }
    }

    /// Whether the branchless and coverage gates apply here.
    pub fn is_spine(self) -> bool {
        matches!(self, Self::Layer | Self::EngineModule | Self::FeatureModule)
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub crate_name: String,
    pub dir: String,
    pub class: Class,
    pub layers: Vec<String>,
    pub modules: Vec<String>,
    pub capabilities: Vec<String>,
    pub rationale: Option<String>,
}

/// Loads every layer, module and app the checker knows about, plus the tool
/// crates it deliberately classifies outside the graph.
pub fn load(repo: &Repo) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::new();

    let (layers, _) = load_manifests(&repo.root);
    for m in layers {
        let crate_name = m
            .layer
            .crate_name
            .clone()
            .unwrap_or_else(|| format!("axiom-{}", m.layer.name));
        out.push(Node {
            name: m.layer.name.clone(),
            crate_name,
            dir: repo.rel(&m.dir),
            class: Class::Layer,
            layers: m.layer.depends_on.clone(),
            modules: Vec::new(),
            capabilities: m.layer.introduced_capabilities.clone(),
            rationale: Some(m.layer.meaningful_dependency.clone()),
        });
    }

    let (modules, _) = load_module_manifests(&repo.root);
    for m in modules {
        let class = if m.module.is_feature_module() {
            Class::FeatureModule
        } else {
            Class::EngineModule
        };
        out.push(Node {
            name: m.module.name.clone(),
            crate_name: m.module.crate_name.clone(),
            dir: repo.rel(&m.dir),
            class,
            layers: m.module.allowed_layers.clone(),
            modules: m.module.allowed_modules.clone(),
            capabilities: m.module.introduced_capabilities.clone(),
            rationale: None,
        });
    }

    let (apps, _) = load_app_manifests(&repo.root);
    for m in apps {
        out.push(Node {
            name: m.app.name.clone(),
            crate_name: m.app.crate_name.clone(),
            dir: repo.rel(&m.dir),
            class: Class::App,
            layers: m.app.allowed_layers.clone(),
            modules: m.app.allowed_modules.clone(),
            capabilities: Vec::new(),
            rationale: None,
        });
    }

    // Tooling carries no manifest by design; the checker classifies it by
    // location, and so do we.
    if let Ok(rd) = std::fs::read_dir(repo.root.join("tools")) {
        let mut dirs: Vec<_> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.join("Cargo.toml").is_file())
            .collect();
        dirs.sort();
        for dir in dirs {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(Node {
                name: name.clone(),
                crate_name: name,
                dir: repo.rel(&dir),
                class: Class::Tool,
                layers: Vec::new(),
                modules: Vec::new(),
                capabilities: Vec::new(),
                rationale: None,
            });
        }
    }
    out.push(Node {
        name: "xtask".to_owned(),
        crate_name: "xtask".to_owned(),
        dir: "crates/xtask".to_owned(),
        class: Class::Tool,
        layers: Vec::new(),
        modules: Vec::new(),
        capabilities: Vec::new(),
        rationale: None,
    });

    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

/// Finds the package that owns a repo-relative path.
pub fn owner<'a>(nodes: &'a [Node], rel: &str) -> Option<&'a Node> {
    nodes
        .iter()
        .filter(|n| !n.dir.is_empty() && rel.starts_with(&format!("{}/", n.dir)))
        // Longest directory wins, so a nested crate beats its parent.
        .max_by_key(|n| n.dir.len())
}

/// Resolves a user-supplied name to a node: exact first, then fuzzy.
pub fn find<'a>(nodes: &'a [Node], query: &str) -> Option<&'a Node> {
    nodes
        .iter()
        .find(|n| n.name == query || n.crate_name == query || n.dir == query)
        .or_else(|| {
            nodes
                .iter()
                .find(|n| n.crate_name.ends_with(query) || n.name.contains(query))
        })
}

/// Everything that declares a dependency on `node`.
pub fn dependents<'a>(nodes: &'a [Node], node: &Node) -> Vec<&'a Node> {
    let is_layer = node.class == Class::Layer;
    nodes
        .iter()
        .filter(|n| {
            if is_layer {
                n.layers.contains(&node.name)
            } else {
                n.modules.contains(&node.name)
            }
        })
        .collect()
}

/// The laws in force for a class, as an agent needs to hear them.
pub fn laws(class: Class) -> Vec<&'static str> {
    if !class.is_spine() {
        return vec![
            "Outside the branchless gate - ordinary control flow is allowed",
            "Outside the 100% coverage gate - ships with the tests its behavior warrants",
            match class {
                Class::App => "Apps are leaves - no layer, module or other app may depend on this",
                _ => "Tools are outside the engine graph - no layer, module or app may depend on this",
            },
        ];
    }

    let mut v = vec![
        "Branchless Law - no if/else, match, for/while/loop, &&/||, ?, if let in non-test code",
        "Coverage Law - 100% regions/lines/functions; new code ships with its tests",
        "No console output (println!/dbg!/todo!/unimplemented!) outside tests",
        "No junk-drawer modules (utils/helpers/common/misc)",
        "No browser APIs (web_sys/js_sys/wasm_bindgen/canvas) - host layer and windowing module excepted",
    ];
    v.push(match class {
        Class::Layer => {
            "Layer Law - import only the layers in depends_on, via public paths, and genuinely use each"
        }
        Class::EngineModule => {
            "Module Law - allowed_modules must be empty; never depend on another module, an app or a tool"
        }
        _ => "Module Law - may depend only on the modules listed in allowed_modules; never an app or a tool",
    });
    v
}

/// Renders the full report for one node.
pub fn describe(nodes: &[Node], node: &Node) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{}  [{}]", node.dir, node.name);
    let _ = writeln!(s, "  class          {}", node.class.label());
    let _ = writeln!(s, "  crate          {}", node.crate_name);

    if !node.layers.is_empty() {
        let _ = writeln!(s, "  layers         {}", node.layers.join(", "));
    }
    if !node.modules.is_empty() {
        let _ = writeln!(s, "  modules        {}", node.modules.join(", "));
    }
    if !node.capabilities.is_empty() {
        let _ = writeln!(s, "  capabilities   {}", node.capabilities.join(", "));
    }

    let deps = dependents(nodes, node);
    if deps.is_empty() {
        if node.class.is_spine() {
            let _ = writeln!(s, "  depended on by (nothing - this is a leaf of the spine)");
        }
    } else {
        let names: Vec<String> = deps.iter().map(|d| d.name.clone()).collect();
        let _ = writeln!(s, "  depended on by {}", names.join(", "));
    }

    if let Some(r) = &node.rationale {
        let _ = writeln!(s, "  rationale      {r}");
    }

    let _ = writeln!(s, "  laws in force");
    for law in laws(node.class) {
        let _ = writeln!(s, "    - {law}");
    }
    s
}
