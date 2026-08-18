//! The Web Audio binding — `wasm32` only.
//!
//! Everything else under [`crate::audio`] is pure arithmetic over an
//! [`AudioGraph`](super::graph::AudioGraph): the voices decide *what* nodes to
//! make, with what parameters, connected how, automated when. This file is the
//! only place that turns that decision into real DSP, and it is the only place
//! in the subsystem that names a browser API.
//!
//! That split is the point. `selftest.js` verifies the original by rendering it
//! through an `OfflineAudioContext` — possible only because `dsp.js` is written
//! against `BaseAudioContext` and never against the live `AudioContext`. The
//! port keeps that property and hardens it: the synthesis is verified natively,
//! with no browser and no audio device, against goldens captured from the
//! JavaScript; this file only has to be a faithful *transcriber*, and it has no
//! decisions of its own to get wrong.
//!
//! **Placement.** Module Law #9 confines `web_sys`/`wasm_bindgen` to the `host`
//! layer and the `windowing` module — that rule governs layers and modules.
//! `claude-of-duty` is an **app**: a composition leaf, and the tier that owns
//! browser bootstrap. Audio is not an engine capability the port is adding to
//! Axiom; it is this game's synthesis, so it stays here, and this is the edge of
//! it.
//!
//! ## What is realised, and what is not
//!
//! The realiser walks a recorded graph once and instantiates it in order, so
//! node indices line up and a connection can always name a node that already
//! exists. It is deliberately *append-only*: [`WebAudioBridge::flush`] realises
//! everything created since the last flush, so a per-frame call materialises
//! exactly that frame's new voices.
//!
//! The two things it cannot carry over are the two that are not decisions:
//! `DynamicsCompressorNode.reduction` (a live audio-thread measurement, read
//! back through [`WebAudioBridge::reduction`]) and the audio device's clock,
//! pushed the other way through
//! [`AudioGraph::set_current_time`](super::graph::AudioGraph::set_current_time).

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::{
    AudioBuffer, AudioBufferSourceNode, AudioContext, AudioNode, AudioParam, BiquadFilterNode,
    BiquadFilterType, ConvolverNode, DynamicsCompressorNode, GainNode, OscillatorNode,
    OscillatorType, OverSampleType, PannerNode, PanningModelType, PeriodicWave,
    PeriodicWaveOptions, StereoPannerNode, WaveShaperNode,
};

use super::graph::{
    AudioGraph, Automation, FilterKind, NodeId, NodeKind, Param, ParamRef, Schedule, Sink, Wave,
};

/// A realised node, kept as its concrete type so parameters stay reachable.
enum Realised {
    Gain(GainNode),
    Biquad(BiquadFilterNode),
    Oscillator(OscillatorNode),
    BufferSource(AudioBufferSourceNode),
    WaveShaper(WaveShaperNode),
    Convolver(ConvolverNode),
    Compressor(DynamicsCompressorNode),
    StereoPanner(StereoPannerNode),
    Panner(PannerNode),
}

impl Realised {
    fn node(&self) -> &AudioNode {
        match self {
            Realised::Gain(n) => n.as_ref(),
            Realised::Biquad(n) => n.as_ref(),
            Realised::Oscillator(n) => n.as_ref(),
            Realised::BufferSource(n) => n.as_ref(),
            Realised::WaveShaper(n) => n.as_ref(),
            Realised::Convolver(n) => n.as_ref(),
            Realised::Compressor(n) => n.as_ref(),
            Realised::StereoPanner(n) => n.as_ref(),
            Realised::Panner(n) => n.as_ref(),
        }
    }

    fn param(&self, which: Param) -> Option<AudioParam> {
        match (self, which) {
            (Realised::Gain(n), Param::Gain) => Some(n.gain()),
            (Realised::Biquad(n), Param::Gain) => Some(n.gain()),
            (Realised::Biquad(n), Param::Frequency) => Some(n.frequency()),
            (Realised::Biquad(n), Param::Q) => Some(n.q()),
            (Realised::Biquad(n), Param::Detune) => Some(n.detune()),
            (Realised::Oscillator(n), Param::Frequency) => Some(n.frequency()),
            (Realised::Oscillator(n), Param::Detune) => Some(n.detune()),
            (Realised::BufferSource(n), Param::PlaybackRate) => Some(n.playback_rate()),
            (Realised::StereoPanner(n), Param::Pan) => Some(n.pan()),
            (Realised::Panner(n), Param::PositionX) => Some(n.position_x()),
            (Realised::Panner(n), Param::PositionY) => Some(n.position_y()),
            (Realised::Panner(n), Param::PositionZ) => Some(n.position_z()),
            _ => None,
        }
    }
}

fn filter_type(kind: FilterKind) -> BiquadFilterType {
    match kind {
        FilterKind::Lowpass => BiquadFilterType::Lowpass,
        FilterKind::Highpass => BiquadFilterType::Highpass,
        FilterKind::Bandpass => BiquadFilterType::Bandpass,
        FilterKind::Peaking => BiquadFilterType::Peaking,
        FilterKind::Highshelf => BiquadFilterType::Highshelf,
    }
}

fn osc_type(wave: Wave) -> OscillatorType {
    match wave {
        Wave::Sine => OscillatorType::Sine,
        Wave::Square => OscillatorType::Square,
        Wave::Sawtooth => OscillatorType::Sawtooth,
        Wave::Triangle => OscillatorType::Triangle,
    }
}

fn over_sample(tag: &str) -> OverSampleType {
    match tag {
        "4x" => OverSampleType::N4x,
        "2x" => OverSampleType::N2x,
        _ => OverSampleType::None,
    }
}

/// Realises a recorded [`AudioGraph`] against a live `AudioContext`.
pub struct WebAudioBridge {
    ctx: AudioContext,
    nodes: Vec<Realised>,
    buffers: Vec<AudioBuffer>,
    waves: Vec<PeriodicWave>,
    /// High-water marks: everything below these has already been realised.
    done_nodes: usize,
    done_connections: usize,
    done_automation: usize,
    done_schedule: usize,
    done_buffers: usize,
    done_waves: usize,
}

impl WebAudioBridge {
    /// `new AudioContext({ latencyHint: 'interactive' })` — the source's
    /// `index.js:163`. Web Audio needs a user gesture; the caller arms one.
    pub fn new() -> Result<Self, JsValue> {
        let ctx = AudioContext::new()?;
        Ok(WebAudioBridge {
            ctx,
            nodes: Vec::new(),
            buffers: Vec::new(),
            waves: Vec::new(),
            done_nodes: 0,
            done_connections: 0,
            done_automation: 0,
            done_schedule: 0,
            done_buffers: 0,
            done_waves: 0,
        })
    }

    pub fn sample_rate(&self) -> f64 {
        f64::from(self.ctx.sample_rate())
    }

    pub fn current_time(&self) -> f64 {
        self.ctx.current_time()
    }

    pub fn context(&self) -> &AudioContext {
        &self.ctx
    }

    /// Instantiate everything recorded since the last call. Call once per frame,
    /// after the subsystem's `update`.
    pub fn flush(&mut self, graph: &AudioGraph) -> Result<(), JsValue> {
        self.realise_buffers(graph)?;
        self.realise_waves(graph)?;
        self.realise_nodes(graph)?;
        self.realise_connections(graph);
        self.realise_automation(graph);
        self.realise_schedule(graph);
        Ok(())
    }

    fn realise_buffers(&mut self, graph: &AudioGraph) -> Result<(), JsValue> {
        for src in &graph.buffers[self.done_buffers..] {
            let buf = self.ctx.create_buffer(
                src.number_of_channels() as u32,
                src.length() as u32,
                src.sample_rate as f32,
            )?;
            for (ch, data) in src.channels.iter().enumerate() {
                // `copy_to_channel` takes a &mut [f32] purely because the JS
                // binding is typed that way; it does not write back.
                let mut scratch = data.clone();
                buf.copy_to_channel(&mut scratch, ch as i32)?;
            }
            self.buffers.push(buf);
        }
        self.done_buffers = graph.buffers.len();
        Ok(())
    }

    fn realise_waves(&mut self, graph: &AudioGraph) -> Result<(), JsValue> {
        for w in &graph.waves[self.done_waves..] {
            let mut real = w.real.clone();
            let mut imag = w.imag.clone();
            let opts = PeriodicWaveOptions::new();
            opts.set_real(&mut real);
            opts.set_imag(&mut imag);
            opts.set_disable_normalization(w.disable_normalization);
            self.waves
                .push(PeriodicWave::new_with_options(&self.ctx, &opts)?);
        }
        self.done_waves = graph.waves.len();
        Ok(())
    }

    fn realise_nodes(&mut self, graph: &AudioGraph) -> Result<(), JsValue> {
        for record in &graph.nodes[self.done_nodes..] {
            let realised = match &record.kind {
                NodeKind::Gain { gain } => {
                    let n = self.ctx.create_gain()?;
                    n.gain().set_value(*gain as f32);
                    Realised::Gain(n)
                }
                NodeKind::Biquad {
                    filter,
                    frequency,
                    q,
                    gain,
                } => {
                    let n = self.ctx.create_biquad_filter()?;
                    n.set_type(filter_type(*filter));
                    n.frequency().set_value(*frequency as f32);
                    n.q().set_value(*q as f32);
                    n.gain().set_value(*gain as f32);
                    Realised::Biquad(n)
                }
                NodeKind::Oscillator {
                    wave,
                    periodic,
                    frequency,
                    detune,
                } => {
                    let n = self.ctx.create_oscillator()?;
                    if let Some(w) = wave {
                        n.set_type(osc_type(*w));
                    }
                    if let Some(id) = periodic {
                        n.set_periodic_wave(&self.waves[id.0]);
                    }
                    n.frequency().set_value(*frequency as f32);
                    n.detune().set_value(*detune as f32);
                    Realised::Oscillator(n)
                }
                NodeKind::BufferSource {
                    buffer,
                    playback_rate,
                    looping,
                    loop_start,
                    loop_end,
                    ..
                } => {
                    let n = self.ctx.create_buffer_source()?;
                    n.set_buffer(Some(&self.buffers[buffer.0]));
                    n.playback_rate().set_value(*playback_rate as f32);
                    n.set_loop(*looping);
                    if *looping {
                        n.set_loop_start(*loop_start);
                        n.set_loop_end(*loop_end);
                    }
                    Realised::BufferSource(n)
                }
                NodeKind::WaveShaper { curve, oversample } => {
                    let n = self.ctx.create_wave_shaper()?;
                    let mut data = graph.curve(*curve).to_vec();
                    n.set_curve(Some(&mut data));
                    n.set_oversample(over_sample(oversample));
                    Realised::WaveShaper(n)
                }
                NodeKind::Convolver { buffer, normalize } => {
                    let n = self.ctx.create_convolver()?;
                    // `normalize` MUST be set before `buffer`: setting it after
                    // re-normalises the response, and these IRs are already
                    // peak-trimmed to 0.42 by `generate_ir`.
                    n.set_normalize(*normalize);
                    n.set_buffer(buffer.map(|b| &self.buffers[b.0]));
                    Realised::Convolver(n)
                }
                NodeKind::Compressor {
                    threshold,
                    knee,
                    ratio,
                    attack,
                    release,
                } => {
                    let n = self.ctx.create_dynamics_compressor()?;
                    n.threshold().set_value(*threshold as f32);
                    n.knee().set_value(*knee as f32);
                    n.ratio().set_value(*ratio as f32);
                    n.attack().set_value(*attack as f32);
                    n.release().set_value(*release as f32);
                    Realised::Compressor(n)
                }
                NodeKind::StereoPanner { pan } => {
                    let n = self.ctx.create_stereo_panner()?;
                    n.pan().set_value(*pan as f32);
                    Realised::StereoPanner(n)
                }
                NodeKind::Panner {
                    panning_model,
                    ref_distance,
                    rolloff_factor,
                    max_distance,
                    cone_inner_angle,
                    ..
                } => {
                    let n = self.ctx.create_panner()?;
                    if *panning_model == "HRTF" {
                        n.set_panning_model(PanningModelType::Hrtf);
                    }
                    n.set_ref_distance(*ref_distance);
                    // Zero on purpose: attenuation is `distGain`'s job, which is
                    // what puts the reverb send post-distance but pre-panning.
                    n.set_rolloff_factor(*rolloff_factor);
                    n.set_max_distance(*max_distance);
                    n.set_cone_inner_angle(*cone_inner_angle);
                    Realised::Panner(n)
                }
            };
            self.nodes.push(realised);
        }
        self.done_nodes = graph.nodes.len();
        Ok(())
    }

    fn realise_connections(&mut self, graph: &AudioGraph) {
        for c in &graph.connections[self.done_connections..] {
            let Some(from) = self.nodes.get(c.from.0) else {
                continue;
            };
            match c.to {
                Sink::Node(to) if to == AudioGraph::DESTINATION => {
                    let _ = from.node().connect_with_audio_node(&self.ctx.destination());
                }
                Sink::Node(to) => {
                    if let Some(target) = self.nodes.get(to.0) {
                        let _ = from.node().connect_with_audio_node(target.node());
                    }
                }
                Sink::Param(ParamRef(node, which)) => {
                    if let Some(p) = self.nodes.get(node.0).and_then(|n| n.param(which)) {
                        let _ = from.node().connect_with_audio_param(&p);
                    }
                }
            }
        }
        self.done_connections = graph.connections.len();
    }

    fn realise_automation(&mut self, graph: &AudioGraph) {
        for e in &graph.automation[self.done_automation..] {
            let ParamRef(node, which) = e.param;
            let Some(p) = self.nodes.get(node.0).and_then(|n| n.param(which)) else {
                continue;
            };
            let _ = match e.kind {
                Automation::SetValueAtTime => p.set_value_at_time(e.value as f32, e.time),
                Automation::ExponentialRampToValueAtTime => {
                    p.exponential_ramp_to_value_at_time(e.value as f32, e.time)
                }
                Automation::SetTargetAtTime => {
                    p.set_target_at_time(e.value as f32, e.time, e.time_constant as f32)
                }
                Automation::CancelScheduledValues => p.cancel_scheduled_values(e.time),
            };
        }
        self.done_automation = graph.automation.len();
    }

    fn realise_schedule(&mut self, graph: &AudioGraph) {
        for e in &graph.schedule[self.done_schedule..] {
            let Some(n) = self.nodes.get(e.node.0) else {
                continue;
            };
            match (e.kind, n) {
                (Schedule::Start, Realised::Oscillator(o)) => {
                    let _ = o.start_with_when(e.when);
                }
                (Schedule::Stop, Realised::Oscillator(o)) => {
                    let _ = o.stop_with_when(e.when);
                }
                (Schedule::Start, Realised::BufferSource(s)) => {
                    let offset = e.offset.unwrap_or(0.0);
                    let _ = match e.duration {
                        Some(d) => s.start_with_when_and_grain_offset_and_grain_duration(
                            e.when, offset, d,
                        ),
                        None => s.start_with_when_and_grain_offset(e.when, offset),
                    };
                }
                (Schedule::Stop, Realised::BufferSource(s)) => {
                    let _ = s.stop_with_when(e.when);
                }
                _ => {}
            }
        }
        self.done_schedule = graph.schedule.len();
    }

    /// The master compressor's live gain reduction — the one readout a recorded
    /// graph cannot produce (`mixer.js:317-319`).
    pub fn reduction(&self, master_comp: NodeId) -> f64 {
        match self.nodes.get(master_comp.0) {
            Some(Realised::Compressor(c)) => f64::from(c.reduction()),
            _ => 0.0,
        }
    }

    /// `actx.close()`.
    pub fn close(&self) {
        let _ = self.ctx.close();
    }
}
