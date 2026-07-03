//! Golden regression for the retro FPS agent (driven through `axiom-agent`).
//!
//! Locks what the agent does over a fixed action script against a committed
//! **golden snapshot** (`tests/retro_fps/golden/retro_fps_agent_play.txt`): the per-step HUD,
//! draw count, and deterministic frame fingerprint. A change to the agent's
//! translation, the engine, or the level that alters play fails this test.
//!
//! Regenerate the golden after an intentional change with
//! `AXIOM_UPDATE_GOLDEN=1 cargo test -p axiom-retro-fps-browser --features agent
//! --test agent_golden`.
#![cfg(feature = "retro-fps-agent")]

use std::fs;
use std::path::PathBuf;

use axiom_game_retro_fps::agent::{Action, AgentSession, Observation};

/// One held-key action (single tick).
fn key(name: &str) -> Action {
    Action {
        keys: vec![name.to_string()],
        ..Action::default()
    }
}

/// One held-key + fire action (single tick).
fn key_fire(name: &str) -> Action {
    Action {
        keys: vec![name.to_string()],
        fire: true,
        ..Action::default()
    }
}

/// The fixed play the agent performs: advance up the room, spin left while
/// firing (spending ammo, scoring as enemies close into the aim cone), then
/// strafe and advance again. Discrete controls only — exactly what an agent
/// drives through `axiom-agent`.
fn script() -> Vec<Action> {
    let mut s = Vec::new();
    (0..6).for_each(|_| s.push(key("forward")));
    // Long enough for enemies to chase into the sweeping aim cone and die; kept
    // under the ~300-tick death window so a respawn never resets the score.
    (0..240).for_each(|_| s.push(key_fire("turn_left")));
    (0..6).for_each(|_| s.push(key("strafe_right")));
    (0..6).for_each(|_| s.push(key("forward")));
    s
}

/// Run the agent over the script, one observation per action.
fn run_agent(script: &[Action]) -> Vec<Observation> {
    let mut session = AgentSession::new();
    script.iter().map(|action| session.step(action)).collect()
}

/// The committed, all-textual golden line for one step.
fn golden_line(index: usize, o: &Observation) -> String {
    format!(
        "{index}\ttick={}\thp={}\tscore={}\tammo={}\tenemies={}\tdraws={}\thash={}",
        o.tick, o.hud.hp, o.hud.score, o.hud.ammo, o.hud.enemies, o.draw_count, o.state_hash
    )
}

fn render_golden(observations: &[Observation]) -> String {
    let body = observations
        .iter()
        .enumerate()
        .map(|(i, o)| golden_line(i, o))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{body}\n")
}

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/retro_fps/golden/retro_fps_agent_play.txt")
}

#[test]
fn the_agent_reproduces_the_golden_snapshot() {
    let script = script();
    let play = run_agent(&script);
    assert_eq!(play.len(), script.len());

    let rendered = render_golden(&play);
    let path = golden_path();
    if std::env::var_os("AXIOM_UPDATE_GOLDEN").is_some() {
        fs::create_dir_all(path.parent().expect("golden path has a parent"))
            .expect("create the golden directory");
        fs::write(&path, &rendered).expect("write the golden snapshot");
    }
    let golden = fs::read_to_string(&path)
        .expect("golden snapshot exists (regenerate with AXIOM_UPDATE_GOLDEN=1)");
    assert_eq!(
        rendered, golden,
        "the agent's play changed vs the committed golden snapshot"
    );

    // A readable summary for `-- --nocapture`.
    let first = play.first().expect("at least one step");
    let last = play.last().expect("at least one step");
    eprintln!("retro FPS agent (axiom-agent-driven): {} steps", play.len());
    eprintln!(
        "  start: hp={} score={} ammo={} enemies={} hash={}",
        first.hud.hp, first.hud.score, first.hud.ammo, first.hud.enemies, first.state_hash
    );
    eprintln!(
        "  end:   hp={} score={} ammo={} enemies={} hash={}",
        last.hud.hp, last.hud.score, last.hud.ammo, last.hud.enemies, last.state_hash
    );
}
