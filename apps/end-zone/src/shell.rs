//! The production shell: the [`FrontendApp`] menu layer composed over the
//! [`EndZoneApp`] game. One `frame` per animation tick: the frontend
//! translates input and emits typed commands; this shell applies them
//! (launch / restart / return / pause) and drives the simulation according
//! to the frontend's [`SimDirective`] — the sim never queries the frontend.

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
use crate::showcase::ShowcaseRun;

/// Diagnostic keyboard tokens passed straight through to the game
/// (camera forcing, debug overlays, start/reset).
const DIAGNOSTIC_KEYS: [&str; 8] = [
    "Space", "KeyR", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "F1",
];

/// The bound gameplay actions and the canonical key each maps onto in the
/// game's fixed input map (rebinding stays real without the sim knowing).
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
    frame_n: u64,
    /// The menu-confirm press that launched/restarted a match stays latched
    /// until released, so it can never double as the first SNAP.
    primary_latched: bool,
}

impl EndZoneShell {
    pub fn new(seed: u64, profile: FrontendProfile) -> Self {
        EndZoneShell {
            frontend: FrontendApp::new(seed, profile),
            app: EndZoneApp::new(EndZoneConfig::default()),
            paused: false,
            frame_n: 0,
            primary_latched: false,
        }
    }

    /// Whether the game simulation is currently suspended.
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
                FrontendCommand::LaunchMatch(_) | FrontendCommand::RestartMatch
            )
        });
        for command in &view.commands {
            self.apply(command);
        }

        // Latch the primary key across a launch/restart until it is released.
        let primary_held = self
            .frontend
            .profile()
            .bindings
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
                let steps = self
                    .frontend
                    .state()
                    .launch
                    .map(|l| l.game_speed.steps_for_frame(self.frame_n))
                    .unwrap_or(1);
                // Keys reach gameplay only once the frontend is actually
                // in-game (never during the transition-in) — and never on
                // the frame that launched.
                let keys = if self.frontend.state().screen == Screen::InGame && !launched {
                    self.game_keys(input)
                } else {
                    Vec::new()
                };
                for _ in 0..steps.max(1) {
                    self.app.advance(&keys, touch);
                }
                self.app.present()
            }
            SimDirective::Menu => {
                // The ambient showcase loop runs behind menus and attract
                // mode; user input never reaches it.
                self.app.advance(&[], TouchInput::default());
                self.app.present()
            }
            // Frozen (paused / pause-settings): re-present without stepping.
            _ => self.app.present(),
        };
        self.frame_n += 1;
        ShellOutput { outcome, view }
    }

    fn apply(&mut self, command: &FrontendCommand) {
        match command {
            FrontendCommand::LaunchMatch(config) => {
                self.app.replace_run(ShowcaseRun::new_match(config));
                self.paused = false;
            }
            FrontendCommand::RestartMatch => {
                if let Some(config) = self.frontend.state().launch {
                    self.app.replace_run(ShowcaseRun::new_match(&config));
                }
            }
            FrontendCommand::ReturnToMenu => {
                self.app
                    .replace_run(ShowcaseRun::new(EndZoneConfig::default()));
            }
            FrontendCommand::SetPaused(paused) => self.paused = *paused,
        }
    }

    /// The game-facing key list: bound gameplay actions map onto the game's
    /// canonical keys; diagnostic keys pass straight through.
    fn game_keys(&self, input: &FrontendInputFrame) -> Vec<KeyToken> {
        let bindings = &self.frontend.profile().bindings;
        let mut keys: Vec<KeyToken> = Vec::new();
        for (action, canonical) in GAME_ACTIONS {
            let suppressed = action == BindableAction::GamePrimary && self.primary_latched;
            if !suppressed && bindings.tokens(action).iter().any(|t| held_in(input, t)) {
                keys.push(KeyToken::new(canonical));
            }
        }
        // (The diagnostic set is disjoint from the canonical game keys.)
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
