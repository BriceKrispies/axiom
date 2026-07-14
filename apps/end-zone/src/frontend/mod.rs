//! The End Zone frontend: a pure, native-testable menu shell over the
//! deterministic showcase. The platform edge feeds one neutral input frame
//! per tick and renders the returned [`widgets::SceneView`]; everything else
//! — the screen state machine, focus, settings, persistence encoding, theme
//! — lives here with zero browser types.

pub mod actions;
pub mod audio;
pub mod bindings;
pub mod input;
pub mod layout;
pub mod navigation;
pub mod persistence;
pub mod profile_codec;
pub mod screen;
pub mod screens;
pub mod settings;
pub mod state;
pub mod theme;
pub mod transitions;
pub mod widgets;

use actions::{AudioIntent, FrontendCommand, HapticIntent, InputDevice};
use input::{FrontendInputFrame, InputTranslator};
use layout::LayoutContext;
use navigation::FocusList;
use persistence::FrontendProfile;
use state::{FrontendState, Screen};
use theme::Theme;
use widgets::SceneView;

/// What the simulation behind the interface should be doing this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimDirective {
    /// The ambient menu/attract showcase loop runs.
    Menu,
    /// The live match advances (input reaches gameplay).
    Live,
    /// The live match exists but is frozen (paused / pause-settings).
    Frozen,
}

/// One tick's frontend output for the platform edge.
#[derive(Debug, Clone)]
pub struct FrontendFrame {
    pub scene: SceneView,
    pub theme: Theme,
    pub commands: Vec<FrontendCommand>,
    pub sounds: Vec<AudioIntent>,
    pub haptics: Vec<HapticIntent>,
    /// The committed profile changed; the edge should persist
    /// [`FrontendApp::profile`] through its store.
    pub persist: bool,
}

/// The frontend orchestrator: state machine + input translator + focus.
#[derive(Debug, Clone)]
pub struct FrontendApp {
    state: FrontendState,
    translator: InputTranslator,
    /// The screen the current focus list was built for (guards remembered
    /// focus ids from leaking across screens that reuse ids).
    focus_screen: Screen,
}

impl FrontendApp {
    pub fn new(seed: u64, profile: FrontendProfile) -> Self {
        FrontendApp {
            state: FrontendState::new(seed, profile),
            translator: InputTranslator::new(),
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

    pub fn hint_device(&self) -> InputDevice {
        self.translator.hint_device()
    }

    /// The gain menu tones should play at (master × menu volume).
    pub fn menu_tone_gain(&self) -> f32 {
        let s = &self.state.profile.settings;
        s.master_volume.ratio() * s.menu_volume.ratio()
    }

    /// What the simulation should do behind the current screen.
    pub fn sim_directive(&self) -> SimDirective {
        match self.state.screen {
            Screen::InGame | Screen::TransitionToGame => SimDirective::Live,
            Screen::Paused => SimDirective::Frozen,
            // Settings opened from pause keeps the frozen match behind it.
            Screen::Settings if self.state.launch.is_some() => SimDirective::Frozen,
            _ => SimDirective::Menu,
        }
    }

    /// Advance the frontend one tick: translate input, run the state
    /// machine, rebuild focus, and emit the scene + drained intents.
    /// `css_width`/`css_height` are the presenter's CSS-pixel size; pointer
    /// coordinates in `raw` are CSS pixels too — the UI scale is applied
    /// here so screens and hit tests always work in logical pixels.
    pub fn frame(
        &mut self,
        raw: &FrontendInputFrame,
        css_width: f32,
        css_height: f32,
    ) -> FrontendFrame {
        let ui_scale = Theme::from_settings(self.state.effective_settings())
            .ui_scale
            .max(0.1);
        let mut input = raw.clone();
        if let Some((x, y)) = input.pointer {
            input.pointer = Some((x / ui_scale, y / ui_scale));
        }

        // While a rebind capture is armed, raw tokens go to the capture —
        // never to navigation (pressing ENTER must bind, not activate).
        let capturing = self
            .state
            .settings_edit
            .as_ref()
            .map(|e| e.capture.is_some())
            .unwrap_or(false);
        if capturing {
            let tokens = self.translator.captured_tokens(&input);
            let _ = self.translator.tick(&input, &self.state.profile.bindings);
            if let Some(token) = tokens.first() {
                self.state.inactivity = 0;
                screens::settings::captured_token(&mut self.state, token);
            }
        } else {
            let actions = self.translator.tick(&input, &self.state.profile.bindings);
            for action in actions {
                screens::handle(&mut self.state, action);
            }
        }
        self.state.advance_clocks();

        let theme = Theme::from_settings(self.state.effective_settings());
        let ctx = LayoutContext::new(css_width / ui_scale, css_height / ui_scale);
        let device = self.translator.hint_device();

        // Rebuild focus from this tick's entries: keep the current focus on
        // the same screen, restore the remembered focus when returning.
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
            haptics: std::mem::take(&mut self.state.haptics),
            persist,
        }
    }
}
