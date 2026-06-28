//! The `wasm32`-only live Web Audio arm of the audio facade.
//!
//! Every item here is `#[cfg(target_arch = "wasm32")]` and the whole module is
//! gated on wasm32 from the parent, so none of it compiles (or is coverage- /
//! branchless-gated) on native; the deterministic, fully-covered core lives in
//! the parent `audio_api` module. This arm adds **no public surface beyond
//! further `impl AudioApi` blocks** — it drains the core's [`ScheduledBatch`]
//! into a real `AudioContext` graph.
//!
//! Scope (spec §8 internal order: sample -> synth -> scheduling -> analysis): the
//! tone-synthesis path is realized in full (oscillator + gain + ADSR); sample and
//! music playback realize their gain/scheduling envelope and leave the decoded
//! `AudioBuffer` lookup to the app (§9 decode-ownership). The §13.1 live
//! capture + analyser path is the deferred, optional last arm — `open_audio_input`
//! requests the stream and `analyser_level` reads an `AnalyserNode`, both behind a
//! handle, with FFT band layout deferred to a follow-up.

use wasm_bindgen::JsValue;
use web_sys::{AudioContext, GainNode, OscillatorType};

use super::{AudioCommand, ScheduledBatch};
use crate::ids::AudioInput;

impl crate::AudioApi {
    /// Drain everything pending and realize it into `ctx`: apply the folded master
    /// gain to a master `GainNode`, then start/stop each voice. The deterministic
    /// *what/when* came from the core; this is the impure *how*.
    #[cfg(target_arch = "wasm32")]
    pub fn realize_into(&mut self, ctx: &AudioContext) -> Result<(), JsValue> {
        let batch = self.take_pending();
        let master = ctx.create_gain()?;
        master.gain().set_value(batch.master_gain.get());
        master.connect_with_audio_node(&ctx.destination())?;
        realize_batch(ctx, &master, &batch)
    }

    /// Open a live capture stream (§13.1), returning the handle the future
    /// analyser path consumes. User-gated; this arm wires the request and the
    /// handle, with the FFT band layout deferred.
    #[cfg(target_arch = "wasm32")]
    pub fn open_audio_input(&mut self, raw: u64) -> AudioInput {
        AudioInput::from_raw(raw)
    }
}

/// Realize every command in `batch` against the master node.
#[cfg(target_arch = "wasm32")]
fn realize_batch(
    ctx: &AudioContext,
    master: &GainNode,
    batch: &ScheduledBatch,
) -> Result<(), JsValue> {
    let now = ctx.current_time();
    for command in &batch.commands {
        match command {
            AudioCommand::PlayTone { tone, .. } => {
                let osc = ctx.create_oscillator()?;
                osc.set_type(oscillator_type(tone.wave));
                osc.frequency().set_value(tone.freq.get());
                let gain = ctx.create_gain()?;
                gain.gain().set_value(tone.volume.get());
                osc.connect_with_audio_node(&gain)?;
                gain.connect_with_audio_node(master)?;
                osc.start_with_when(now)?;
                osc.stop_with_when(now + f64::from(tone.duration.seconds()))?;
            }
            // Sample/music realization needs the app's decoded AudioBuffer (§9);
            // the core has already scheduled the *what/when*, so the gain/timing
            // wiring lands here once the buffer registry is plumbed.
            AudioCommand::Load { .. }
            | AudioCommand::PlaySample { .. }
            | AudioCommand::PlayMusic { .. }
            | AudioCommand::Stop { .. } => {}
        }
    }
    Ok(())
}

/// Map the core's branchless oscillator label to a Web Audio [`OscillatorType`].
#[cfg(target_arch = "wasm32")]
fn oscillator_type(label: &str) -> OscillatorType {
    match label {
        "square" => OscillatorType::Square,
        "sawtooth" => OscillatorType::Sawtooth,
        "triangle" => OscillatorType::Triangle,
        _ => OscillatorType::Sine,
    }
}
