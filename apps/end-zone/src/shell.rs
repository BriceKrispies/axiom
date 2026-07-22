//! The production shell: the [`FrontendApp`] menu layer composed over the
//! [`EndZoneApp`] game. One `frame` per animation tick: the frontend translates
//! input and emits typed commands; this shell applies them (launch / restart /
//! return / pause), drives the run according to the frontend's [`SimDirective`],
//! and reports a finished run back to the frontend as the game-over screen. The
//! run never queries frontend state.

use axiom::prelude::FrameOutcome;
use axiom_input::KeyToken;

use crate::app::{EndZoneApp, TouchInput};
use crate::config::EndZoneConfig;
use crate::frontend::actions::FrontendCommand;
use crate::frontend::bindings::BindableAction;
use crate::frontend::input::FrontendInputFrame;
use crate::frontend::persistence::FrontendProfile;
use crate::frontend::state::Screen;
use crate::frontend::{FrontendApp, FrontendFrame, SimDirective};
use crate::launch::RunConfig;
use crate::showcase::ShowcaseRun;

/// Diagnostic keyboard tokens passed straight through to the game (camera
/// forcing, debug overlay) — never surfaced in any menu.
const DIAGNOSTIC_KEYS: [&str; 6] = ["Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "F1"];

/// The bound gameplay actions and the canonical key each maps onto in the
/// game's fixed input map.
const GAME_ACTIONS: [(BindableAction, &str); 5] = [
    (BindableAction::GamePrimary, "Enter"),
    (BindableAction::NavUp, "KeyW"),
    (BindableAction::NavDown, "KeyS"),
    (BindableAction::NavLeft, "KeyA"),
    (BindableAction::NavRight, "KeyD"),
];

/// One shell frame's outputs: the rendered engine frame + the frontend view.
#[derive(Debug)]
pub struct ShellOutput {
    pub outcome: FrameOutcome,
    pub view: FrontendFrame,
}

/// The composed production app.
#[derive(Debug)]
pub struct EndZoneShell {
    pub frontend: FrontendApp,
    pub app: EndZoneApp,
    paused: bool,
    /// The active run's frozen config (for RESTART RUN).
    run_config: Option<RunConfig>,
    /// The menu-confirm press that launched/restarted a run stays latched until
    /// released, so it can never double as the first SNAP.
    primary_latched: bool,
}

impl EndZoneShell {
    pub fn new(seed: u64, profile: FrontendProfile) -> Self {
        EndZoneShell {
            frontend: FrontendApp::new(seed, profile),
            app: EndZoneApp::new(EndZoneConfig::default()),
            paused: false,
            run_config: None,
            primary_latched: false,
        }
    }

    /// Whether the run simulation is currently suspended.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Advance everything one animation frame.
    pub fn frame(
        &mut self,
        input: &FrontendInputFrame,
        touch: TouchInput,
        css_width: f32,
        css_height: f32,
    ) -> ShellOutput {
        let view = self.frontend.frame(input, css_width, css_height);
        let launched = view.commands.iter().any(|c| {
            matches!(
                c,
                FrontendCommand::LaunchRun { .. } | FrontendCommand::RestartRun
            )
        });
        for command in &view.commands {
            self.apply(command);
        }

        // Latch the primary key across a launch/restart until it is released.
        let primary_held = self
            .frontend
            .bindings()
            .tokens(BindableAction::GamePrimary)
            .iter()
            .any(|t| held_in(input, t));
        if launched {
            self.primary_latched = primary_held;
        }
        if !primary_held {
            self.primary_latched = false;
        }

        let outcome = match self.frontend.sim_directive() {
            SimDirective::Live if !self.paused => {
                // Gameplay input reaches the run only on the field itself — never
                // during the pre-snap huddle, and never on the launch frame.
                let in_game = self.frontend.screen() == Screen::InGame;
                let keys = if in_game && !launched {
                    self.game_keys(input)
                } else {
                    Vec::new()
                };
                let touch = if in_game { touch } else { TouchInput::default() };
                self.app.advance(&keys, touch);
                // A failed fourth-down conversion ends the run.
                if let Some(summary) = self
                    .app
                    .run
                    .drive_state()
                    .filter(|d| d.over)
                    .map(|d| d.summary())
                {
                    self.frontend.enter_game_over(summary);
                }
                // Mirror the drive's pre-snap huddle into the screen machine: it
                // opens the play-call screen and closes it when the huddle breaks.
                match self.app.run.huddle() {
                    Some(view) => self.frontend.enter_huddle(view),
                    None => self.frontend.exit_huddle(),
                }
                self.app.present()
            }
            SimDirective::Menu => {
                // The ambient title showcase loops behind the title; user input
                // never reaches it.
                self.app.advance(&[], TouchInput::default());
                self.app.present()
            }
            // Frozen (paused / settings / controls / game over): re-present.
            _ => self.app.present(),
        };
        ShellOutput { outcome, view }
    }

    fn apply(&mut self, command: &FrontendCommand) {
        match command {
            FrontendCommand::LaunchRun { seed } => {
                let s = &self.frontend.profile().settings;
                let config =
                    RunConfig::new(*seed).with_presentation(s.screen_shake, s.reduced_motion);
                self.run_config = Some(config);
                self.app.replace_run(ShowcaseRun::new_run(&config));
                self.paused = false;
            }
            FrontendCommand::RestartRun => {
                if let Some(config) = self.run_config {
                    self.app.replace_run(ShowcaseRun::new_run(&config));
                }
                self.paused = false;
            }
            FrontendCommand::ReturnToTitle => {
                self.run_config = None;
                self.app
                    .replace_run(ShowcaseRun::new(EndZoneConfig::default()));
            }
            FrontendCommand::SetPaused(paused) => self.paused = *paused,
            FrontendCommand::CallPlay { index } => self.app.run.call_play(*index),
        }
    }

    /// The game-facing key list: bound gameplay actions map onto the game's
    /// canonical keys; diagnostic keys pass straight through.
    fn game_keys(&self, input: &FrontendInputFrame) -> Vec<KeyToken> {
        let bindings = self.frontend.bindings();
        let mut keys: Vec<KeyToken> = Vec::new();
        for (action, canonical) in GAME_ACTIONS {
            let suppressed = action == BindableAction::GamePrimary && self.primary_latched;
            if !suppressed && bindings.tokens(action).iter().any(|t| held_in(input, t)) {
                keys.push(KeyToken::new(canonical));
            }
        }
        for diagnostic in DIAGNOSTIC_KEYS {
            if held_in(input, diagnostic) {
                keys.push(KeyToken::new(diagnostic));
            }
        }
        keys
    }
}

fn held_in(input: &FrontendInputFrame, token: &str) -> bool {
    input.keys_down.iter().any(|t| t == token) || input.pad_down.iter().any(|t| t == token)
}
