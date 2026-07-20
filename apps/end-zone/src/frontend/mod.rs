//! The End Zone frontend: a pure, native-testable six-screen shell over the
//! deterministic run. The platform edge feeds one neutral input frame per tick
//! and renders the returned [`widgets::SceneView`] (plus the game HUD, which the
//! edge builds from authoritative run state). Everything else — the screen
//! state machine, focus, settings, persistence encoding, theme — lives here
//! with zero browser types.

pub mod actions;
pub mod audio;
pub mod bindings;
pub mod input;
pub mod layout;
pub mod navigation;
pub mod persistence;
pub mod screen;
pub mod screens;
pub mod settings;
pub mod state;
pub mod theme;
pub mod transitions;
pub mod widgets;

use crate::drive::RunSummary;

use actions::{AudioIntent, FrontendCommand, InputDevice};
use bindings::ControlBindings;
use input::{FrontendInputFrame, InputTranslator};
use layout::LayoutContext;
use navigation::FocusList;
use persistence::FrontendProfile;
use state::{FrontendState, Screen};
use theme::Theme;
use transitions::TransitionKind;
use widgets::SceneView;

/// What the simulation behind the interface should be doing this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimDirective {
    /// The ambient title showcase loop runs (no real run active).
    Menu,
    /// The live run advances (input reaches gameplay).
    Live,
    /// The run exists but is frozen (paused / settings / controls / game over).
    Frozen,
}

/// One tick's frontend output for the platform edge.
#[derive(Debug, Clone)]
pub struct FrontendFrame {
    pub scene: SceneView,
    pub theme: Theme,
    pub commands: Vec<FrontendCommand>,
    pub sounds: Vec<AudioIntent>,
    /// The committed profile changed; the edge should persist through its store.
    pub persist: bool,
}

/// The frontend orchestrator: state machine + input translator + focus.
#[derive(Debug, Clone)]
pub struct FrontendApp {
    state: FrontendState,
    translator: InputTranslator,
    bindings: ControlBindings,
    /// The screen the current focus list was built for.
    focus_screen: Screen,
}

impl FrontendApp {
    pub fn new(seed: u64, profile: FrontendProfile) -> Self {
        FrontendApp {
            state: FrontendState::new(seed, profile),
            translator: InputTranslator::new(),
            bindings: ControlBindings::default(),
            focus_screen: Screen::Title,
        }
    }

    pub fn state(&self) -> &FrontendState {
        &self.state
    }

    /// Tests drive transitions and selections directly.
    pub fn state_mut(&mut self) -> &mut FrontendState {
        &mut self.state
    }

    pub fn profile(&self) -> &FrontendProfile {
        &self.state.profile
    }

    pub fn bindings(&self) -> &ControlBindings {
        &self.bindings
    }

    pub fn hint_device(&self) -> InputDevice {
        self.translator.hint_device()
    }

    /// The gain menu tones should play at (the master volume).
    pub fn menu_tone_gain(&self) -> f32 {
        self.state.profile.settings.master_volume.ratio()
    }

    /// The gain the menu music should play at: the music volume beneath the
    /// master gain, so master still scales everything and music trims below it.
    pub fn menu_music_gain(&self) -> f32 {
        let settings = &self.state.profile.settings;
        settings.master_volume.ratio() * settings.music_volume.ratio()
    }

    /// Whether the menu music should be audible now: on the pre-game `Menu`, and
    /// on a Settings/Controls sub-screen that the `Menu` (not the in-game pause)
    /// opened. Off on the title, in gameplay, and in the paused-game menus.
    pub fn menu_music_active(&self) -> bool {
        match self.state.screen {
            Screen::Menu => true,
            Screen::Settings | Screen::Controls => self.state.sub_return == Screen::Menu,
            _ => false,
        }
    }

    /// What the simulation should do behind the current screen.
    pub fn sim_directive(&self) -> SimDirective {
        match self.state.screen {
            Screen::InGame => SimDirective::Live,
            Screen::Paused | Screen::Settings | Screen::Controls | Screen::GameOver => {
                SimDirective::Frozen
            }
            Screen::Title | Screen::Menu => SimDirective::Menu,
        }
    }

    /// The active screen.
    pub fn screen(&self) -> Screen {
        self.state.screen
    }

    /// The shell reports the run ended: show the game-over summary.
    pub fn enter_game_over(&mut self, summary: RunSummary) {
        if self.state.screen != Screen::GameOver {
            self.state.summary = Some(summary);
            self.state.go(Screen::GameOver, TransitionKind::Fade);
        }
    }

    /// Advance the frontend one tick: translate input, run the state machine,
    /// rebuild focus, and emit the scene + drained intents.
    pub fn frame(
        &mut self,
        raw: &FrontendInputFrame,
        css_width: f32,
        css_height: f32,
    ) -> FrontendFrame {
        let actions = self.translator.tick(raw, &self.bindings);
        for action in actions {
            screens::handle(&mut self.state, action);
        }
        self.state.advance_clocks();

        let theme = Theme::from_settings(self.state.effective_settings());
        let ctx = LayoutContext::new(css_width, css_height);
        let device = self.translator.hint_device();

        // Rebuild focus from this tick's entries: keep the current focus on the
        // same screen, restore the remembered focus when returning.
        let (_, entries) = screens::build(&self.state, &ctx, &theme, device);
        let remembered = if self.focus_screen == self.state.screen {
            self.state.focus.focused()
        } else {
            self.state.focus_memory[self.state.screen.index()]
        };
        self.state.focus = FocusList::new(entries, remembered);
        self.focus_screen = self.state.screen;

        let (scene, _) = screens::build(&self.state, &ctx, &theme, device);

        let persist = std::mem::take(&mut self.state.persist_requested);
        FrontendFrame {
            scene,
            theme,
            commands: std::mem::take(&mut self.state.commands),
            sounds: std::mem::take(&mut self.state.sounds),
            persist,
        }
    }
}
