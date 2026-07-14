//! Frontend audio: typed intents mapped to procedural tone recipes (pure
//! data — the platform edge synthesizes them through the engine's
//! `axiom-audio` tone path). Volumes come from the settings model; nothing
//! here claims audible output the engine cannot produce.

use super::actions::AudioIntent;

/// The waveform vocabulary (mirrors `axiom_audio::Wave` without making the
//  deterministic core depend on the audio module).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneWave {
    Sine,
    Square,
    Sawtooth,
    Triangle,
}

/// One procedural tone recipe (frequencies in Hz, times in seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneRecipe {
    pub wave: ToneWave,
    pub freq: f32,
    pub duration: f32,
    pub volume: f32,
    /// Optional immediate second tone (simple two-note hits).
    pub second: Option<(f32, f32)>,
}

/// The original menu sound design: synthesized, loud, arcade.
pub fn recipe(intent: AudioIntent) -> ToneRecipe {
    match intent {
        AudioIntent::Navigate => ToneRecipe {
            wave: ToneWave::Square,
            freq: 620.0,
            duration: 0.045,
            volume: 0.35,
            second: None,
        },
        AudioIntent::Confirm => ToneRecipe {
            wave: ToneWave::Square,
            freq: 440.0,
            duration: 0.10,
            volume: 0.55,
            second: Some((660.0, 0.09)),
        },
        AudioIntent::Cancel => ToneRecipe {
            wave: ToneWave::Sawtooth,
            freq: 330.0,
            duration: 0.09,
            volume: 0.4,
            second: Some((220.0, 0.08)),
        },
        AudioIntent::Denied => ToneRecipe {
            wave: ToneWave::Square,
            freq: 140.0,
            duration: 0.12,
            volume: 0.5,
            second: None,
        },
        AudioIntent::TeamLock => ToneRecipe {
            wave: ToneWave::Square,
            freq: 520.0,
            duration: 0.12,
            volume: 0.6,
            second: Some((780.0, 0.14)),
        },
        AudioIntent::VsImpact => ToneRecipe {
            wave: ToneWave::Sawtooth,
            freq: 110.0,
            duration: 0.28,
            volume: 0.7,
            second: Some((165.0, 0.2)),
        },
        AudioIntent::Transition => ToneRecipe {
            wave: ToneWave::Triangle,
            freq: 240.0,
            duration: 0.22,
            volume: 0.4,
            second: Some((480.0, 0.16)),
        },
        AudioIntent::PauseHit => ToneRecipe {
            wave: ToneWave::Square,
            freq: 300.0,
            duration: 0.11,
            volume: 0.55,
            second: Some((200.0, 0.1)),
        },
        AudioIntent::ResumeRise => ToneRecipe {
            wave: ToneWave::Triangle,
            freq: 260.0,
            duration: 0.1,
            volume: 0.5,
            second: Some((390.0, 0.12)),
        },
    }
}
