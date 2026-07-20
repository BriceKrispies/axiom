//! The explicit frontend screen state machine. Every transition is a typed
//! method on [`FrontendState`], recorded in a bounded history — never a pile of
//! booleans. The machine owns frontend-only state (focus, focus memory, the
//! active transition, the game-over summary) and communicates with the
//! composition layer exclusively through drained [`FrontendCommand`]s.

use crate::drive::RunSummary;
use crate::frontend::actions::{AudioIntent, FrontendCommand};
use crate::frontend::navigation::{FocusList, WidgetId};
use crate::frontend::persistence::FrontendProfile;
use crate::frontend::settings::EndZoneSettings;
use crate::frontend::transitions::{ActiveTransition, TransitionKind};

pub use super::screen::{Screen, SCREEN_COUNT};

/// Bounded transition history cap.
const HISTORY_CAP: usize = 128;

/// The frontend state machine.
#[derive(Debug, Clone)]
pub struct FrontendState {
    pub tick: u64,
    pub screen: Screen,
    pub profile: FrontendProfile,
    pub focus_memory: [Option<WidgetId>; SCREEN_COUNT],
    pub focus: FocusList,
    pub transition: Option<ActiveTransition>,
    /// The final run summary, set when the shell reports the run is over.
    pub summary: Option<RunSummary>,
    /// Where a Settings/Controls sub-screen returns on BACK — the pre-game
    /// `Menu` or the in-game `Paused` menu, whichever opened it.
    pub sub_return: Screen,
    /// Monotonic run counter (drives the deterministic per-run launch seed).
    run_counter: u64,
    base_seed: u64,
    // Drained outputs.
    pub commands: Vec<FrontendCommand>,
    pub sounds: Vec<AudioIntent>,
    /// Set when the committed settings changed and should be saved.
    pub persist_requested: bool,
    /// Bounded `(tick, screen)` transition history (replay-compared).
    pub history: Vec<(u64, Screen)>,
}

impl FrontendState {
    pub fn new(base_seed: u64, profile: FrontendProfile) -> Self {
        FrontendState {
            tick: 0,
            screen: Screen::Title,
            profile,
            focus_memory: [None; SCREEN_COUNT],
            focus: FocusList::default(),
            transition: None,
            summary: None,
            sub_return: Screen::Paused,
            run_counter: 0,
            base_seed,
            commands: Vec::new(),
            sounds: Vec::new(),
            persist_requested: false,
            history: vec![(0, Screen::Title)],
        }
    }

    /// The settings currently shaping presentation (applied immediately).
    pub fn effective_settings(&self) -> &EndZoneSettings {
        &self.profile.settings
    }

    /// Mutate the committed settings and request a persist (changes apply
    /// immediately — there is no working copy).
    pub fn edit_settings(&mut self, edit: impl FnOnce(&mut EndZoneSettings)) {
        edit(&mut self.profile.settings);
        self.profile.settings = self.profile.settings.sanitized();
        self.persist_requested = true;
    }

    /// Explicit, recorded screen transition.
    pub fn go(&mut self, to: Screen, kind: TransitionKind) {
        if self.screen == to {
            return;
        }
        let reduced = self.effective_settings().reduced_motion;
        self.focus_memory[self.screen.index()] = self.focus.focused();
        self.transition = Some(ActiveTransition::start(kind, self.screen, to, reduced));
        self.screen = to;
        if self.history.len() < HISTORY_CAP {
            self.history.push((self.tick, to));
        }
    }

    pub fn sound(&mut self, intent: AudioIntent) {
        if self.sounds.len() < 16 {
            self.sounds.push(intent);
        }
    }

    pub fn command(&mut self, command: FrontendCommand) {
        if self.commands.len() < 8 {
            self.commands.push(command);
        }
    }

    /// Advance the per-tick clocks (the active transition overlay only).
    pub fn advance_clocks(&mut self) {
        self.tick += 1;
        if let Some(mut t) = self.transition.take() {
            if t.advance() {
                self.transition = Some(t);
            }
        }
    }

    /// A fresh explicit run seed for the NEXT run (title START, play again).
    pub fn next_run_seed(&mut self) -> u64 {
        self.run_counter += 1;
        splitmix(self.base_seed ^ self.run_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }
}

/// A small deterministic seed mixer (splitmix64 finalizer).
fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
