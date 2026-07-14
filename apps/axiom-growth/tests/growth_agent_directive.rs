//! Locks the **data-driven directive runner**: the script "walk to the
//! mountaintop, look at the ground, take a screenshot" — expressed purely as data
//! over the world's semantic tags — drives the growth player to the summit, aims
//! at the resolved ground point, and yields a deterministic capture. Native +
//! `agent` feature only.
#![cfg(feature = "agent")]

use axiom_growth::agent::{parse_directives, AgentSession};

/// The same script the package ships (`package/agents/summit_view.toml`).
const SCRIPT: &str = r#"
[[directive]]
verb = "goto"
target = "mountaintop"

[[directive]]
verb = "look_at"
target = "ground"

[[directive]]
verb = "capture"
label = "summit_view"
"#;

#[test]
fn directive_script_climbs_resolves_tags_and_captures() {
    let mut session = AgentSession::earthlike();
    let script = parse_directives(SCRIPT);
    let captures = session.run_directives(&script);

    assert!(
        session.reached_summit_now(),
        "the goto directive should walk the player to the summit",
    );

    assert_eq!(captures.len(), 1, "one capture directive ⇒ one capture");
    assert_eq!(captures[0].label, "summit_view");

    let inputs = &captures[0].inputs;
    assert!(!inputs.vertices.is_empty(), "capture has terrain geometry");
    assert!(!inputs.indices.is_empty());
    assert!(inputs.view_proj.iter().all(|v| v.is_finite()), "finite view-projection");
}

#[test]
fn directive_runner_is_deterministic() {
    let run = || {
        let mut session = AgentSession::earthlike();
        session.run_directives(&parse_directives(SCRIPT))
    };
    let a = run();
    let b = run();
    assert_eq!(a.len(), b.len());
    assert_eq!(a[0].inputs.view_proj, b[0].inputs.view_proj, "view is replay-identical");
    assert_eq!(a[0].inputs.vertices, b[0].inputs.vertices, "mesh is replay-identical");
}

#[test]
fn authored_tags_merge_with_runtime_tags() {
    let mut session = AgentSession::earthlike();
    session.register_toml_tags("[[tag]]\nname = \"basecamp\"\nkind = \"spawn\"\nat = \"spawn\"\n");
    let script = parse_directives(
        "[[directive]]\nverb = \"look_at\"\ntarget = \"basecamp\"\n[[directive]]\nverb = \"capture\"\nlabel = \"basecamp_view\"\n",
    );
    let captures = session.run_directives(&script);
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].label, "basecamp_view");
}
