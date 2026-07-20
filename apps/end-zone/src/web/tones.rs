//! Menu tone synthesis: the frontend's typed [`ToneRecipe`]s played through the
//! shared `AudioContext` owned by the [`super::music::AudioEdge`] (browser
//! autoplay policy requires a user gesture before that context exists). All
//! volume shaping comes from the typed settings via the gain the shell computes.

use web_sys::{AudioContext, OscillatorType};

use crate::frontend::audio::{ToneRecipe, ToneWave};

fn osc_type(wave: ToneWave) -> OscillatorType {
    match wave {
        ToneWave::Sine => OscillatorType::Sine,
        ToneWave::Square => OscillatorType::Square,
        ToneWave::Sawtooth => OscillatorType::Sawtooth,
        ToneWave::Triangle => OscillatorType::Triangle,
    }
}

/// Stateless menu-tone synthesis over a shared, already-unlocked context.
#[derive(Debug)]
pub struct MenuTones;

impl MenuTones {
    /// Play one recipe at `gain` (master × menu volume) through `context`.
    pub fn play(context: &AudioContext, recipe: ToneRecipe, gain: f32) {
        if gain <= 0.0 {
            return;
        }
        let now = context.current_time();
        Self::tone(
            context,
            recipe.wave,
            recipe.freq,
            now,
            recipe.duration,
            recipe.volume * gain,
        );
        if let Some((freq, duration)) = recipe.second {
            Self::tone(
                context,
                recipe.wave,
                freq,
                now + f64::from(recipe.duration),
                duration,
                recipe.volume * gain,
            );
        }
    }

    fn tone(
        context: &AudioContext,
        wave: ToneWave,
        freq: f32,
        at: f64,
        duration: f32,
        volume: f32,
    ) {
        let (Ok(osc), Ok(gain)) = (context.create_oscillator(), context.create_gain()) else {
            return;
        };
        osc.set_type(osc_type(wave));
        osc.frequency().set_value(freq);
        let level = volume.clamp(0.0, 1.0);
        // A short attack/decay envelope keeps the square/saw tones click-free.
        let _ = gain.gain().set_value_at_time(0.0001, at);
        let _ = gain
            .gain()
            .exponential_ramp_to_value_at_time(level.max(0.0002), at + 0.008);
        let _ = gain
            .gain()
            .exponential_ramp_to_value_at_time(0.0001, at + f64::from(duration.max(0.02)));
        let _ = osc.connect_with_audio_node(&gain);
        let _ = gain.connect_with_audio_node(&context.destination());
        let _ = osc.start_with_when(at);
        let _ = osc.stop_with_when(at + f64::from(duration.max(0.02)) + 0.05);
    }
}
