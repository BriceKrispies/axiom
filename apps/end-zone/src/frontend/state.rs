//! The explicit frontend screen state machine. Every transition is a typed
//! method on [`FrontendState`], recorded in a bounded history — never a pile
//! of booleans. The machine owns frontend-only state (selections, working
//! settings, focus memory, inactivity, transitions) and communicates with the
//! composition layer exclusively through drained [`FrontendCommand`]s.

use crate::data::team::LeagueTeamId;
use crate::frontend::actions::{AudioIntent, FrontendCommand, HapticIntent};
use crate::frontend::bindings::BindableAction;
use crate::frontend::navigation::{FocusList, WidgetId};
use crate::frontend::persistence::FrontendProfile;
use crate::frontend::settings::{EndZoneSettings, SettingsCategory};
use crate::frontend::transitions::{ActiveTransition, TransitionKind};
use crate::launch::{Difficulty, GameSpeed, MatchLaunchConfig};

pub use super::screen::{Screen, SCREEN_COUNT};

/// Team selection stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamStage {
    Player,
    Opponent,
}

/// The team-select screen's state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeamSelectState {
    pub stage: TeamStage,
    /// Cursor over the league while picking the PLAYER team.
    pub player_cursor: u8,
    /// Cursor over the league while picking the OPPONENT.
    pub opponent_cursor: u8,
    /// The locked player team (stage 2).
    pub locked_player: Option<LeagueTeamId>,
}

/// Per-match quick options (seeded from committed settings when team select
/// confirms; adjustable on the matchup screen without touching settings).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchOptions {
    pub difficulty: Difficulty,
    pub game_speed: GameSpeed,
}

/// The settings editor: an explicit WORKING copy over the committed profile.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsEdit {
    pub origin: Screen,
    pub category: SettingsCategory,
    pub working: EndZoneSettings,
    pub working_bindings: crate::frontend::bindings::ControlBindings,
    /// A rebind capture in progress: `(action, ticks remaining)`.
    pub capture: Option<(BindableAction, u32)>,
}

/// Modal dialogs (focus-confined, app-styled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    DiscardSettings,
    ReturnToMenu,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModalState {
    pub kind: ModalKind,
    /// Focused option index (0 = safe option).
    pub focused: u8,
}

/// Ticks of inactivity on Title / MainMenu before attract mode (~30 s).
pub const ATTRACT_TIMEOUT: u64 = 1800;
/// Rebind capture timeout, ticks (~8 s).
pub const CAPTURE_TIMEOUT: u32 = 480;
/// Bounded histories.
const HISTORY_CAP: usize = 128;

/// The frontend state machine.
#[derive(Debug, Clone)]
pub struct FrontendState {
    pub tick: u64,
    pub screen: Screen,
    pub profile: FrontendProfile,
    pub team_select: TeamSelectState,
    pub match_options: MatchOptions,
    pub settings_edit: Option<SettingsEdit>,
    pub modal: Option<ModalState>,
    pub focus_memory: [Option<WidgetId>; SCREEN_COUNT],
    pub focus: FocusList,
    pub transition: Option<ActiveTransition>,
    pub inactivity: u64,
    /// The active match's frozen launch config.
    pub launch: Option<MatchLaunchConfig>,
    /// Monotonic match counter (drives the deterministic per-match seed).
    pub match_counter: u64,
    /// The seed the NEXT match will launch with (shown on match setup).
    pub pending_seed: u64,
    base_seed: u64,
    // Drained outputs.
    pub commands: Vec<FrontendCommand>,
    pub sounds: Vec<AudioIntent>,
    pub haptics: Vec<HapticIntent>,
    /// Bounded `(tick, screen)` transition history (replay-compared).
    pub history: Vec<(u64, Screen)>,
    /// Set when the committed profile changed and should be saved.
    pub persist_requested: bool,
}

impl FrontendState {
    pub fn new(base_seed: u64, profile: FrontendProfile) -> Self {
        let match_options = MatchOptions {
            difficulty: profile.settings.difficulty,
            game_speed: profile.settings.game_speed,
        };
        let team_select = TeamSelectState {
            stage: TeamStage::Player,
            player_cursor: profile.last_player_team.0,
            opponent_cursor: profile.last_opponent_team.0,
            locked_player: None,
        };
        FrontendState {
            tick: 0,
            screen: Screen::Title,
            profile,
            team_select,
            match_options,
            settings_edit: None,
            modal: None,
            focus_memory: [None; SCREEN_COUNT],
            focus: FocusList::default(),
            transition: None,
            inactivity: 0,
            launch: None,
            match_counter: 0,
            pending_seed: splitmix(base_seed),
            base_seed,
            commands: Vec::new(),
            sounds: Vec::new(),
            haptics: Vec::new(),
            history: vec![(0, Screen::Title)],
            persist_requested: false,
        }
    }

    /// The settings currently shaping presentation: the WORKING copy while
    /// the editor is open (live preview), else the committed settings.
    pub fn effective_settings(&self) -> &EndZoneSettings {
        self.settings_edit
            .as_ref()
            .map(|e| &e.working)
            .unwrap_or(&self.profile.settings)
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
        self.inactivity = 0;
    }

    /// Instant transition (no animation) — used by transition completions.
    pub fn arrive(&mut self, to: Screen) {
        if self.screen == to {
            return;
        }
        self.focus_memory[self.screen.index()] = self.focus.focused();
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

    pub fn haptic(&mut self, intent: HapticIntent) {
        if self.haptics.len() < 8 {
            self.haptics.push(intent);
        }
    }

    pub fn command(&mut self, command: FrontendCommand) {
        if self.commands.len() < 8 {
            self.commands.push(command);
        }
    }

    /// Advance the per-tick clocks: the transition, the inactivity timer,
    /// and any rebind capture timeout. Returns the screen a finished
    /// transition arrives at.
    pub fn advance_clocks(&mut self) {
        self.tick += 1;
        self.inactivity = self.inactivity.saturating_add(1);
        if let Some(mut t) = self.transition.take() {
            if t.advance() {
                self.transition = Some(t);
            } else {
                // Transitional SCREENS complete into their destination.
                match self.screen {
                    Screen::TransitionToGame => self.arrive(Screen::InGame),
                    Screen::TransitionToMenu => self.arrive(Screen::MainMenu),
                    _ => {}
                }
            }
        }
        if let Some(edit) = self.settings_edit.as_mut() {
            if let Some((action, ticks)) = edit.capture.take() {
                if ticks > 1 {
                    edit.capture = Some((action, ticks - 1));
                }
            }
        }
        // Attract entry: inactivity on Title / MainMenu only.
        if matches!(self.screen, Screen::Title | Screen::MainMenu)
            && self.inactivity >= ATTRACT_TIMEOUT
            && self.modal.is_none()
        {
            self.go(Screen::Attract, TransitionKind::Fade);
        }
    }

    /// Freeze the pending seed into a launch config for the current
    /// selections + match options + committed presentation settings.
    pub fn build_launch(&self) -> Option<MatchLaunchConfig> {
        let player = self.team_select.locked_player?;
        let opponent = LeagueTeamId(self.team_select.opponent_cursor);
        let s = &self.profile.settings;
        let config = MatchLaunchConfig {
            player_team: player,
            opponent_team: opponent,
            player_is_home: true,
            field: crate::launch::FieldPresentation::Standard,
            difficulty: self.match_options.difficulty,
            game_speed: self.match_options.game_speed,
            camera_style: s.camera_style,
            seed: self.pending_seed,
            presentation: crate::launch::PresentationProfile {
                effects: s.effects_intensity,
                screen_shake: s.screen_shake,
                flash: s.flash_intensity,
            },
            control_profile: self.profile.control_profile,
        };
        config.validate().ok().map(|_| config)
    }

    /// Roll the deterministic seed for the NEXT match.
    pub fn roll_seed(&mut self) {
        self.match_counter += 1;
        self.pending_seed =
            splitmix(self.base_seed ^ self.match_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
}

/// A small deterministic seed mixer (splitmix64 finalizer).
fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
