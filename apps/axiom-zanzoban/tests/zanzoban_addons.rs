//! Behaviour tests for the five configurable Zanzoban mechanics (add-ons), each
//! on a tiny hand-built level with just the relevant rule enabled. Drives the
//! deterministic core through the public `game_step::step` command path.

use axiom_zanzoban::coord::GridCoord;
use axiom_zanzoban::game_command::StepKind;
use axiom_zanzoban::game_state::PuzzleGameState;
use axiom_zanzoban::group_id::GroupId;
use axiom_zanzoban::level_definition::{
    BudgetRule, DecayRule, Door, LevelDefinition, RuleSet, Switch,
};
use axiom_zanzoban::{game_step, Direction, PuzzleCommand};

/// A bare `w×1` corridor: entrance at the left, exit at the right, no objects.
fn corridor(w: i32) -> LevelDefinition {
    LevelDefinition {
        title: "t".into(),
        width: w as u32,
        height: 1,
        entrance: GridCoord::new(0, 0),
        exit: GridCoord::new(w - 1, 0),
        walls: vec![],
        buttons: vec![],
        doors: vec![],
        wells: vec![],
        switches: vec![],
        crates: vec![],
        hazards: vec![],
        rules: RuleSet::default(),
    }
}

fn mv(s: &mut PuzzleGameState, d: Direction) {
    game_step::step(s, PuzzleCommand::Move(d));
}
fn tick(s: &mut PuzzleGameState) -> StepKind {
    game_step::step(s, PuzzleCommand::Tick).kind
}
fn q(s: &mut PuzzleGameState) -> StepKind {
    game_step::step(s, PuzzleCommand::ResetLifeFromRecording).kind
}

const STEP: u32 = 30; // ticks per ghost move (0.5 s at 60 Hz)

#[test]
fn decay_fades_a_ghost_after_its_lifetime() {
    let mut level = corridor(5);
    level.rules.decay = Some(DecayRule { lifetime_steps: 2 });
    let mut s = PuzzleGameState::new(level);
    // Record two moves, then leave a ghost of them.
    mv(&mut s, Direction::Right);
    mv(&mut s, Direction::Right);
    q(&mut s);
    assert_eq!(s.ghost_count(), 1);
    // Two ghost steps consume its 2-step life; the second reaps it.
    for _ in 0..STEP {
        tick(&mut s);
    }
    assert_eq!(s.ghost_count(), 1, "one step taken, one life left");
    let mut faded_total = 0;
    for _ in 0..STEP {
        if let StepKind::Ticked { ghosts_faded, .. } = tick(&mut s) {
            faded_total += ghosts_faded;
        }
    }
    assert_eq!(s.ghost_count(), 0, "the afterimage faded after 2 steps");
    assert_eq!(faded_total, 1, "the fade was reported");
}

#[test]
fn a_resonance_well_keeps_a_ghost_alive() {
    // lifetime 1, but a well on the ghost's path refreshes it, so it lives longer.
    let mut level = corridor(5);
    level.rules.decay = Some(DecayRule { lifetime_steps: 1 });
    level.wells = vec![GridCoord::new(1, 0)];
    let mut s = PuzzleGameState::new(level);
    mv(&mut s, Direction::Right); // onto the well
    mv(&mut s, Direction::Right);
    q(&mut s);
    // First ghost step lands on the well: life 1 -> 0 -> refreshed to 1 (survives).
    for _ in 0..STEP {
        tick(&mut s);
    }
    assert_eq!(
        s.ghost_count(),
        1,
        "the well refreshed the ghost past its base lifetime"
    );
}

#[test]
fn a_switch_latches_a_door_open_and_toggles() {
    let group = GroupId::new("g");
    let mut level = corridor(4); // entrance(0) switch(1) door(2) exit(3)
    level.rules.switches = true;
    level.switches = vec![Switch {
        position: GridCoord::new(1, 0),
        group: group.clone(),
    }];
    level.doors = vec![Door {
        position: GridCoord::new(2, 0),
        group: group.clone(),
    }];
    let mut s = PuzzleGameState::new(level);
    assert!(!s.is_group_open(&group));
    mv(&mut s, Direction::Right); // step onto the switch -> latch on
    assert!(s.is_group_open(&group), "switch latched the door open");
    // Step off onto the (now open) door: latch persists (not a hold).
    mv(&mut s, Direction::Right);
    assert_eq!(s.player().position, GridCoord::new(2, 0));
    assert!(s.is_group_open(&group), "latch persists after leaving the switch");
    // Step back onto the switch -> toggles the latch off (edge-triggered on entry).
    mv(&mut s, Direction::Left);
    assert_eq!(s.player().position, GridCoord::new(1, 0));
    assert!(!s.is_group_open(&group), "re-entering the switch toggled it off");
}

#[test]
fn a_crate_pushes_and_is_blocked_by_a_wall() {
    let mut level = corridor(5);
    level.rules.crates = true;
    level.crates = vec![GridCoord::new(1, 0)];
    let mut s = PuzzleGameState::new(level);
    mv(&mut s, Direction::Right); // push the crate from (1) to (2)
    assert_eq!(s.player().position, GridCoord::new(1, 0));
    assert_eq!(s.crates(), &[GridCoord::new(2, 0)]);

    // A crate with a wall immediately beyond cannot be pushed. (Width 4 so the
    // wall at (2,0) is not overwritten by the exit at (3,0).)
    let mut level = corridor(4);
    level.rules.crates = true;
    level.crates = vec![GridCoord::new(1, 0)];
    level.walls = vec![GridCoord::new(2, 0)];
    let mut s = PuzzleGameState::new(level);
    let blocked = game_step::step(&mut s, PuzzleCommand::Move(Direction::Right));
    assert!(blocked.player_move_rejected());
    assert_eq!(s.player().position, GridCoord::new(0, 0));
    assert_eq!(s.crates(), &[GridCoord::new(1, 0)], "crate did not move");
}

#[test]
fn the_ghost_budget_refuses_extra_lives() {
    let mut level = corridor(4);
    level.rules.budget = Some(BudgetRule {
        max_ghosts: 1,
        par: Some(1),
    });
    let mut s = PuzzleGameState::new(level);
    mv(&mut s, Direction::Right);
    assert!(matches!(q(&mut s), StepKind::LifeReset));
    assert_eq!(s.ghost_count(), 1);
    mv(&mut s, Direction::Right);
    assert!(matches!(q(&mut s), StepKind::LifeRejectedBudgetFull));
    assert_eq!(s.ghost_count(), 1, "budget refused the second ghost");
}

#[test]
fn a_mechanics_level_replays_identically() {
    // A level exercising decay + wells + switches replays byte-identically from the
    // same command stream (the determinism guarantee, with add-ons on).
    let build = || {
        let mut level = corridor(6);
        level.rules.decay = Some(DecayRule { lifetime_steps: 4 });
        level.rules.switches = true;
        level.wells = vec![GridCoord::new(2, 0)];
        level.switches = vec![Switch {
            position: GridCoord::new(3, 0),
            group: GroupId::new("g"),
        }];
        PuzzleGameState::new(level)
    };
    let script = |s: &mut PuzzleGameState| {
        mv(s, Direction::Right);
        mv(s, Direction::Right);
        mv(s, Direction::Right);
        q(s);
        for _ in 0..200 {
            tick(s);
        }
    };
    let mut a = build();
    let mut b = build();
    script(&mut a);
    script(&mut b);
    assert!(a == b, "same add-on level + commands -> identical state");
}

#[test]
fn a_hazard_kills_the_live_player() {
    let mut level = corridor(3); // entrance(0) hazard(1) exit(2)
    level.rules.hazards = true;
    level.hazards = vec![GridCoord::new(1, 0)];
    let mut s = PuzzleGameState::new(level);
    mv(&mut s, Direction::Right); // record a move first
    let died = game_step::step(&mut s, PuzzleCommand::Move(Direction::Right));
    // The step onto the hazard is entered, then the life is reset.
    assert!(
        died.player_died(),
        "stepping onto a hazard kills the current life"
    );
    assert_eq!(s.player().position, GridCoord::new(0, 0), "back at the entrance");
    assert_eq!(s.ghost_count(), 0, "no ghost is created by a death");
    assert_eq!(s.recording_len(), 0, "the recording was cleared");
}
