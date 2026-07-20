//! Menu music: a looping MP3 played through WebAudio so a gain node can
//! cross-fade it as the title menu appears and disappears. This is the app's
//! first sampled-audio path and lives entirely in the wasm edge. The shared
//! `AudioContext` also feeds the procedural [`MenuTones`], so both are unlocked
//! by the same user gesture (browser autoplay policy requires one).

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsValue;
use web_sys::{AudioContext, GainNode, HtmlAudioElement, MediaElementAudioSourceNode};

use crate::frontend::audio::ToneRecipe;

use super::tones::MenuTones;

/// The served, looping menu-music asset (see `web/audio/menu.mp3`).
const MUSIC_URL: &str = "./audio/menu.mp3";
/// Cross-fade duration (seconds) when entering / leaving the title menu.
const FADE_SECS: f64 = 0.8;

/// The live WebAudio nodes for the music track. `_element`/`_source` are held
/// only to keep the graph alive.
#[derive(Debug)]
struct MusicNodes {
    element: HtmlAudioElement,
    _source: MediaElementAudioSourceNode,
    /// Ramped 0 <-> 1 on a title enter/leave transition (the fade).
    fade: GainNode,
    /// Set to (master × music) volume every frame.
    master: GainNode,
}

/// The looping menu-music player and its fade envelope.
#[derive(Debug, Default)]
pub struct MenuMusic {
    nodes: Option<MusicNodes>,
    on_menu: Option<bool>,
}

impl MenuMusic {
    /// Build the audio element + graph on first use.
    fn ensure_started(&mut self, context: &AudioContext) {
        if self.nodes.is_none() {
            self.nodes = build(context);
        }
    }

    /// (Re)start playback if the element is paused. Idempotent, and it swallows
    /// the autoplay-policy rejection so a blocked attempt at startup stays silent
    /// (no console noise) until a later gesture makes play() succeed.
    fn kick(&self) {
        if let Some(nodes) = &self.nodes {
            if nodes.element.paused() {
                if let Ok(promise) = nodes.element.play() {
                    let ignore = Closure::<dyn FnMut(JsValue)>::new(|_| {});
                    let _ = promise.catch(&ignore);
                    ignore.forget();
                }
            }
        }
    }

    /// Per-frame: track the master×music gain live, and ramp the fade toward
    /// 1.0 on the title menu / 0.0 elsewhere — only when the target changes.
    fn update(&mut self, context: &AudioContext, on_menu: bool, master_gain: f32) {
        let Some(nodes) = self.nodes.as_ref() else {
            return;
        };
        let _ = nodes.master.gain().set_value(master_gain.clamp(0.0, 1.0));
        if self.on_menu != Some(on_menu) {
            self.on_menu = Some(on_menu);
            let target = if on_menu { 1.0_f32 } else { 0.0 };
            let now = context.current_time();
            let gain = nodes.fade.gain();
            let _ = gain.cancel_scheduled_values(now);
            let _ = gain.set_value_at_time(gain.value(), now);
            let _ = gain.linear_ramp_to_value_at_time(target, now + FADE_SECS);
        }
    }
}

/// Wire `<audio loop>` → fade gain → master gain → destination and start it.
fn build(context: &AudioContext) -> Option<MusicNodes> {
    let element = HtmlAudioElement::new_with_src(MUSIC_URL).ok()?;
    element.set_loop(true);
    let source = context.create_media_element_source(&element).ok()?;
    let fade = context.create_gain().ok()?;
    let master = context.create_gain().ok()?;
    let _ = fade.gain().set_value_at_time(0.0, context.current_time());
    source.connect_with_audio_node(&fade).ok()?;
    fade.connect_with_audio_node(&master).ok()?;
    master.connect_with_audio_node(&context.destination()).ok()?;
    Some(MusicNodes {
        element,
        _source: source,
        fade,
        master,
    })
}

/// The shared audio edge: owns the one lazily created `AudioContext` and drives
/// both the procedural tones and the menu music through it.
#[derive(Debug, Default)]
pub struct AudioEdge {
    context: Option<AudioContext>,
    music: MenuMusic,
}

impl AudioEdge {
    pub fn new() -> Self {
        AudioEdge::default()
    }

    /// Create/resume the audio context and start the music. Call on a user
    /// gesture — both the context and media playback need one.
    pub fn unlock(&mut self) {
        if self.context.is_none() {
            self.context = AudioContext::new().ok();
        }
        if let Some(context) = &self.context {
            let _ = context.resume();
            self.music.ensure_started(context);
            self.music.kick();
        }
    }

    /// Play one menu tone at `gain`. No-op until unlocked.
    pub fn play_tone(&self, recipe: ToneRecipe, gain: f32) {
        if let Some(context) = &self.context {
            MenuTones::play(context, recipe, gain);
        }
    }

    /// Advance the menu-music fade (target 1.0 on the title menu, else 0.0) and
    /// track its master×music gain. No-op until unlocked.
    pub fn update_music(&mut self, on_menu: bool, gain: f32) {
        if let Some(context) = &self.context {
            self.music.update(context, on_menu, gain);
        }
    }
}
