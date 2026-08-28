//! The recorded audio graph — this port's stand-in for `BaseAudioContext`.
//!
//! Ported from the Web Audio surface that `C:/dev/Claude-of-Duty/src/audio/*.js`
//! uses: `createGain`/`createBiquadFilter`/`createOscillator`/
//! `createBufferSource`/`createWaveShaper`/`createConvolver`/
//! `createDynamicsCompressor`/`createPanner`/`createStereoPanner`/
//! `createBuffer`/`createPeriodicWave`, the `AudioParam` automation methods, and
//! `connect`/`disconnect`/`start`/`stop`.
//!
//! ## Why this type exists
//!
//! The source's synthesis voices are, structurally, two things braided together:
//! **the recipe** (which nodes, at which frequencies, with which envelopes, in
//! which order, driven by which `rng` draws) and **the plumbing** (asking a
//! browser to instantiate that). The recipe is the content — 4,241 lines of it —
//! and it is engine-agnostic. The plumbing is a browser binding.
//!
//! So the voices are written against *this* type, which does exactly what a
//! `BaseAudioContext` does except that instead of instantiating DSP it **records
//! what was asked for**: the node list in creation order, every connection,
//! every `AudioParam` automation event and every source start/stop. A recorded
//! graph is then either
//!
//!   - realised against real Web Audio nodes by [`crate::audio::web_audio`]
//!     (`wasm32` only), or
//!   - asserted on directly by a native test.
//!
//! That second property is the point. `selftest.js` verifies the subsystem by
//! rendering it through an `OfflineAudioContext` and measuring peak/RMS/DC —
//! possible only because `dsp.js` is written strictly against `BaseAudioContext`
//! and never against the live `AudioContext`. This type preserves that property
//! and sharpens it: a test can compare the *exact* graph the port builds against
//! the exact graph the JavaScript builds for the same seed, node for node and
//! automation event for automation event. `tests/audio_port.rs` does precisely
//! that against goldens captured from the original under Node.
//!
//! Nothing here is an abstraction over Web Audio in the "make it nicer" sense.
//! It is the same vocabulary with the same defaults, recorded rather than
//! executed.

use std::collections::HashMap;

/// A node's identity: its index in creation order, exactly as the goldens key
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize);

impl NodeId {
    /// `node.gain` — a `GainNode`'s gain, and also a `BiquadFilterNode`'s
    /// shelf/peaking gain, which is the same `AudioParam` name in Web Audio.
    pub fn gain(self) -> ParamRef {
        ParamRef(self, Param::Gain)
    }
    /// `node.frequency`.
    pub fn frequency(self) -> ParamRef {
        ParamRef(self, Param::Frequency)
    }
    /// `node.Q`.
    pub fn q(self) -> ParamRef {
        ParamRef(self, Param::Q)
    }
    /// `node.detune`.
    pub fn detune(self) -> ParamRef {
        ParamRef(self, Param::Detune)
    }
    /// `node.playbackRate`.
    pub fn playback_rate(self) -> ParamRef {
        ParamRef(self, Param::PlaybackRate)
    }
    /// `node.pan` (`StereoPannerNode`).
    pub fn pan(self) -> ParamRef {
        ParamRef(self, Param::Pan)
    }
    /// `panner.positionX` / `positionY` / `positionZ`, by axis 0/1/2.
    pub fn position(self, axis: usize) -> ParamRef {
        ParamRef(
            self,
            [Param::PositionX, Param::PositionY, Param::PositionZ][axis],
        )
    }
}

/// Index into [`AudioGraph::buffers`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferId(pub usize);

/// Index into [`AudioGraph::curves`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurveId(pub usize);

/// Index into [`AudioGraph::waves`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WaveId(pub usize);

/// The `AudioParam`s the source automates. The string each maps to is the Web
/// Audio property name, which is also the key the goldens record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Param {
    Gain,
    Frequency,
    Q,
    Detune,
    PlaybackRate,
    Pan,
    PositionX,
    PositionY,
    PositionZ,
}

impl Param {
    pub fn as_str(self) -> &'static str {
        match self {
            Param::Gain => "gain",
            Param::Frequency => "frequency",
            Param::Q => "Q",
            Param::Detune => "detune",
            Param::PlaybackRate => "playbackRate",
            Param::Pan => "pan",
            Param::PositionX => "positionX",
            Param::PositionY => "positionY",
            Param::PositionZ => "positionZ",
        }
    }
}

/// One node's one parameter — what a JavaScript call site writes as `g.gain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParamRef(pub NodeId, pub Param);

/// `BiquadFilterNode.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Lowpass,
    Highpass,
    Bandpass,
    Peaking,
    Highshelf,
}

impl FilterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FilterKind::Lowpass => "lowpass",
            FilterKind::Highpass => "highpass",
            FilterKind::Bandpass => "bandpass",
            FilterKind::Peaking => "peaking",
            FilterKind::Highshelf => "highshelf",
        }
    }
}

/// `OscillatorNode.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wave {
    Sine,
    Square,
    Sawtooth,
    Triangle,
}

impl Wave {
    pub fn as_str(self) -> &'static str {
        match self {
            Wave::Sine => "sine",
            Wave::Square => "square",
            Wave::Sawtooth => "sawtooth",
            Wave::Triangle => "triangle",
        }
    }
}

/// What kind of node this is, with the fields the source sets on it.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    Gain {
        gain: f64,
    },
    Biquad {
        filter: FilterKind,
        frequency: f64,
        q: f64,
        gain: f64,
    },
    Oscillator {
        /// `None` once `setPeriodicWave` has replaced the built-in type — the
        /// glottal source in `vox.js`.
        wave: Option<Wave>,
        periodic: Option<WaveId>,
        frequency: f64,
        detune: f64,
    },
    BufferSource {
        buffer: BufferId,
        playback_rate: f64,
        looping: bool,
        loop_start: f64,
        loop_end: f64,
        /// `src._offset` — the random read offset `dsp.js` stashes on the node
        /// and hands back to `start()`. See [`AudioGraph::start_source`].
        offset: f64,
    },
    WaveShaper {
        curve: CurveId,
        oversample: &'static str,
    },
    Convolver {
        buffer: Option<BufferId>,
        normalize: bool,
    },
    Compressor {
        threshold: f64,
        knee: f64,
        ratio: f64,
        attack: f64,
        release: f64,
    },
    StereoPanner {
        pan: f64,
    },
    Panner {
        panning_model: &'static str,
        distance_model: &'static str,
        ref_distance: f64,
        rolloff_factor: f64,
        max_distance: f64,
        cone_inner_angle: f64,
    },
}

/// Where a `connect()` lands: another node's input, or an `AudioParam`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sink {
    Node(NodeId),
    Param(ParamRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Connection {
    pub from: NodeId,
    pub to: Sink,
}

/// The `AudioParam` automation methods the source calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Automation {
    SetValueAtTime,
    ExponentialRampToValueAtTime,
    SetTargetAtTime,
    CancelScheduledValues,
}

impl Automation {
    /// The tag the goldens record.
    pub fn as_str(self) -> &'static str {
        match self {
            Automation::SetValueAtTime => "set",
            Automation::ExponentialRampToValueAtTime => "expo",
            Automation::SetTargetAtTime => "target",
            Automation::CancelScheduledValues => "cancel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomationEvent {
    pub param: ParamRef,
    pub kind: Automation,
    pub value: f64,
    pub time: f64,
    /// Only meaningful for [`Automation::SetTargetAtTime`].
    pub time_constant: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    Start,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduleEvent {
    pub node: NodeId,
    pub kind: Schedule,
    pub when: f64,
    /// `start(when, offset, duration)` — `None` for an oscillator, which takes
    /// neither.
    pub offset: Option<f64>,
    pub duration: Option<f64>,
}

/// An `AudioBuffer`: interleaved-by-channel `f32` sample data.
///
/// `f32`, not `f64`, and deliberately: a `Float32Array` store rounds, and both
/// `fillNoise` and `generateIR` *read their own stores back* (the crackle grain
/// accumulator, the IR's early-reflection taps added on top of the diffuse
/// field). Holding the data at `f64` would quietly change the arithmetic.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    pub channels: Vec<Vec<f32>>,
    pub sample_rate: f64,
}

impl AudioBuffer {
    pub fn length(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn number_of_channels(&self) -> usize {
        self.channels.len()
    }

    /// `buffer.duration`.
    pub fn duration(&self) -> f64 {
        self.length() as f64 / self.sample_rate
    }
}

/// A `PeriodicWave` — `vox.js`'s glottal pulse.
#[derive(Debug, Clone, PartialEq)]
pub struct PeriodicWave {
    pub real: Vec<f32>,
    pub imag: Vec<f32>,
    pub disable_normalization: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeRecord {
    pub kind: NodeKind,
}

/// `actx.listener`, as last set by
/// [`SpatialField::set_listener`](crate::audio::spatial::SpatialField::set_listener).
///
/// The listener's nine parameters are `AudioParam`s like any other, but nothing
/// ever *connects* to them and only one call site writes them, so recording each
/// `setTargetAtTime` individually would be nine near-identical rows a frame with
/// nothing to say. The state plus an update count is what a test needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListenerState {
    pub position: [f64; 3],
    pub forward: [f64; 3],
    pub up: [f64; 3],
    /// Seconds — the source's `0.02` for position/forward and `0.05` for up.
    pub position_smoothing: f64,
    pub up_smoothing: f64,
    pub updates: usize,
}

impl Default for ListenerState {
    fn default() -> Self {
        ListenerState {
            position: [0.0; 3],
            forward: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
            position_smoothing: 0.02,
            up_smoothing: 0.05,
            updates: 0,
        }
    }
}

/// The recorded graph. One of these stands in for one `BaseAudioContext`.
#[derive(Debug, Clone)]
pub struct AudioGraph {
    sample_rate: f64,
    current_time: f64,
    pub nodes: Vec<NodeRecord>,
    pub connections: Vec<Connection>,
    pub automation: Vec<AutomationEvent>,
    pub schedule: Vec<ScheduleEvent>,
    pub buffers: Vec<AudioBuffer>,
    pub curves: Vec<Vec<f32>>,
    pub waves: Vec<PeriodicWave>,
    /// `dsp.js`'s module-level `CURVE_CACHE`, and `vox.js`'s `WAVE_CACHE`.
    ///
    /// **Divergence, deliberate.** Both are module-global in the source — the
    /// wave cache is even a `WeakMap` keyed on the context, i.e. already
    /// per-context in spirit. Hoisting them onto the graph keeps them
    /// per-context in fact, which is identical behaviour for the game (there is
    /// exactly one context) and removes two pieces of hidden global mutable
    /// state, which Axiom's determinism rules ask for.
    curve_cache: HashMap<String, CurveId>,
    wave_cache: Option<WaveId>,
    /// Count of `disconnect()` calls, so a test can prove a teardown happened.
    pub disconnects: usize,
    pub listener: ListenerState,
}

impl AudioGraph {
    /// A fresh context at `sample_rate` Hz, with `currentTime` at zero.
    pub fn new(sample_rate: f64) -> Self {
        AudioGraph {
            sample_rate,
            current_time: 0.0,
            nodes: Vec::new(),
            connections: Vec::new(),
            automation: Vec::new(),
            schedule: Vec::new(),
            buffers: Vec::new(),
            curves: Vec::new(),
            waves: Vec::new(),
            curve_cache: HashMap::new(),
            wave_cache: None,
            disconnects: 0,
            listener: ListenerState::default(),
        }
    }

    /// `actx.listener` — position, forward and up, smoothed.
    pub fn set_listener(&mut self, position: [f64; 3], forward: [f64; 3], up: [f64; 3]) {
        self.listener.position = position;
        self.listener.forward = forward;
        self.listener.up = up;
        self.listener.updates += 1;
    }

    /// `actx.destination` — a sentinel, not a recorded node, so that every
    /// created node's index is its creation ordinal and matches the goldens
    /// captured from the JavaScript one-for-one. Nothing reads a field off it;
    /// it is only ever a `connect` target.
    pub const DESTINATION: NodeId = NodeId(usize::MAX);

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// `actx.currentTime`.
    pub fn current_time(&self) -> f64 {
        self.current_time
    }

    /// Advance the context clock. In the browser this ticks by itself; in a test
    /// (and in an `OfflineAudioContext` render) it is driven.
    pub fn set_current_time(&mut self, t: f64) {
        self.current_time = t;
    }

    fn push(&mut self, kind: NodeKind) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(NodeRecord { kind });
        id
    }

    pub fn node(&self, id: NodeId) -> &NodeKind {
        &self.nodes[id.0].kind
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut NodeKind {
        &mut self.nodes[id.0].kind
    }

    /* ---------------- factories ---------------- */

    pub fn create_gain(&mut self, gain: f64) -> NodeId {
        self.push(NodeKind::Gain { gain })
    }

    pub fn create_biquad(
        &mut self,
        filter: FilterKind,
        frequency: f64,
        q: f64,
        gain: f64,
    ) -> NodeId {
        self.push(NodeKind::Biquad {
            filter,
            frequency,
            q,
            gain,
        })
    }

    pub fn create_oscillator(&mut self, wave: Wave, frequency: f64, detune: f64) -> NodeId {
        self.push(NodeKind::Oscillator {
            wave: Some(wave),
            periodic: None,
            frequency,
            detune,
        })
    }

    /// `actx.createOscillator()` with no `type` assignment — `vox.js` builds one
    /// and immediately calls `setPeriodicWave`.
    pub fn create_periodic_oscillator(&mut self, wave: WaveId) -> NodeId {
        self.push(NodeKind::Oscillator {
            wave: None,
            periodic: Some(wave),
            frequency: 440.0,
            detune: 0.0,
        })
    }

    pub fn create_buffer_source(
        &mut self,
        buffer: BufferId,
        playback_rate: f64,
        looping: bool,
        loop_start: f64,
        loop_end: f64,
        offset: f64,
    ) -> NodeId {
        self.push(NodeKind::BufferSource {
            buffer,
            playback_rate,
            looping,
            loop_start,
            loop_end,
            offset,
        })
    }

    pub fn create_wave_shaper(&mut self, curve: CurveId, oversample: &'static str) -> NodeId {
        self.push(NodeKind::WaveShaper { curve, oversample })
    }

    pub fn create_convolver(&mut self, normalize: bool) -> NodeId {
        self.push(NodeKind::Convolver {
            buffer: None,
            normalize,
        })
    }

    pub fn create_dynamics_compressor(
        &mut self,
        threshold: f64,
        knee: f64,
        ratio: f64,
        attack: f64,
        release: f64,
    ) -> NodeId {
        self.push(NodeKind::Compressor {
            threshold,
            knee,
            ratio,
            attack,
            release,
        })
    }

    pub fn create_stereo_panner(&mut self, pan: f64) -> NodeId {
        self.push(NodeKind::StereoPanner { pan })
    }

    /// The HRTF emitter panner of `spatial.js:42-49`, with its distance model
    /// switched off (`rolloffFactor` 0) so attenuation can be applied separately.
    pub fn create_panner(&mut self) -> NodeId {
        self.push(NodeKind::Panner {
            panning_model: "HRTF",
            distance_model: "inverse",
            ref_distance: 1.0,
            rolloff_factor: 0.0,
            max_distance: 10000.0,
            cone_inner_angle: 360.0,
        })
    }

    /// `actx.createBuffer(channels, length, sampleRate)` — zero-filled.
    pub fn create_buffer(&mut self, channels: usize, length: usize, sample_rate: f64) -> BufferId {
        let id = BufferId(self.buffers.len());
        self.buffers.push(AudioBuffer {
            channels: vec![vec![0.0f32; length]; channels],
            sample_rate,
        });
        id
    }

    pub fn buffer(&self, id: BufferId) -> &AudioBuffer {
        &self.buffers[id.0]
    }

    pub fn buffer_mut(&mut self, id: BufferId) -> &mut AudioBuffer {
        &mut self.buffers[id.0]
    }

    pub fn create_periodic_wave(
        &mut self,
        real: Vec<f32>,
        imag: Vec<f32>,
        disable_normalization: bool,
    ) -> WaveId {
        let id = WaveId(self.waves.len());
        self.waves.push(PeriodicWave {
            real,
            imag,
            disable_normalization,
        });
        id
    }

    /// Store a waveshaper curve, deduplicated on `key` exactly as `dsp.js`'s
    /// `CURVE_CACHE` is. `build` is only run on a miss.
    pub fn cached_curve(&mut self, key: String, build: impl FnOnce() -> Vec<f32>) -> CurveId {
        if let Some(&id) = self.curve_cache.get(&key) {
            return id;
        }
        let id = CurveId(self.curves.len());
        self.curves.push(build());
        self.curve_cache.insert(key, id);
        id
    }

    pub fn curve(&self, id: CurveId) -> &[f32] {
        &self.curves[id.0]
    }

    /// `vox.js`'s `WAVE_CACHE.get(actx)`.
    pub fn cached_wave(&mut self, build: impl FnOnce() -> (Vec<f32>, Vec<f32>)) -> WaveId {
        if let Some(id) = self.wave_cache {
            return id;
        }
        let (real, imag) = build();
        let id = self.create_periodic_wave(real, imag, false);
        self.wave_cache = Some(id);
        id
    }

    /* ---------------- wiring ---------------- */

    /// `from.connect(to)`.
    pub fn connect(&mut self, from: NodeId, to: NodeId) {
        self.connections.push(Connection {
            from,
            to: Sink::Node(to),
        });
    }

    /// `from.connect(someNode.someParam)` — the LFO idiom.
    pub fn connect_param(&mut self, from: NodeId, to: ParamRef) {
        self.connections.push(Connection {
            from,
            to: Sink::Param(to),
        });
    }

    /// `dsp.js`'s `series(...nodes)`, fused with the `.connect(tail)` that every
    /// one of its call sites immediately performs.
    ///
    /// `series(a, b, c).connect(d)` connects a→b, b→c, c→d in that order, which
    /// is exactly `series(&[a, b, c, d])` — same edges, same order. Folding the
    /// two together loses the intermediate return value that no call site in the
    /// source ever uses for anything else.
    pub fn series(&mut self, nodes: &[NodeId]) -> NodeId {
        for pair in nodes.windows(2) {
            self.connect(pair[0], pair[1]);
        }
        nodes[nodes.len() - 1]
    }

    /// `from.disconnect(to)`. Removes the edge; a missing edge is a no-op, where
    /// Web Audio throws and every call site in the source already wraps it in a
    /// `try`/`catch` that swallows exactly that.
    pub fn disconnect(&mut self, from: NodeId, to: Sink) {
        self.disconnects += 1;
        self.connections
            .retain(|c| !(c.from == from && c.to == to));
    }

    /// `from.disconnect()` — every outgoing edge.
    pub fn disconnect_all(&mut self, from: NodeId) {
        self.disconnects += 1;
        self.connections.retain(|c| c.from != from);
    }

    /* ---------------- automation ---------------- */

    fn automate(&mut self, param: ParamRef, kind: Automation, value: f64, time: f64, tc: f64) {
        self.automation.push(AutomationEvent {
            param,
            kind,
            value,
            time,
            time_constant: tc,
        });
    }

    pub fn set_value_at_time(&mut self, param: ParamRef, value: f64, time: f64) {
        self.automate(param, Automation::SetValueAtTime, value, time, 0.0);
    }

    pub fn exponential_ramp_to_value_at_time(&mut self, param: ParamRef, value: f64, time: f64) {
        self.automate(
            param,
            Automation::ExponentialRampToValueAtTime,
            value,
            time,
            0.0,
        );
    }

    pub fn set_target_at_time(&mut self, param: ParamRef, value: f64, time: f64, tc: f64) {
        self.automate(param, Automation::SetTargetAtTime, value, time, tc);
    }

    pub fn cancel_scheduled_values(&mut self, param: ParamRef, time: f64) {
        self.automate(param, Automation::CancelScheduledValues, 0.0, time, 0.0);
    }

    /// `param.value = v` — the immediate, un-scheduled assignment. Mutates the
    /// node's stored value rather than pushing an automation event, exactly as
    /// Web Audio's `value` setter does.
    pub fn set_param_value(&mut self, param: ParamRef, value: f64) {
        let ParamRef(node, which) = param;
        match (&mut self.nodes[node.0].kind, which) {
            (NodeKind::Gain { gain }, Param::Gain)
            | (NodeKind::Biquad { gain, .. }, Param::Gain) => *gain = value,
            (NodeKind::Biquad { frequency, .. }, Param::Frequency)
            | (NodeKind::Oscillator { frequency, .. }, Param::Frequency) => *frequency = value,
            (NodeKind::Biquad { q, .. }, Param::Q) => *q = value,
            (NodeKind::Oscillator { detune, .. }, Param::Detune) => *detune = value,
            (NodeKind::BufferSource { playback_rate, .. }, Param::PlaybackRate) => {
                *playback_rate = value;
            }
            (NodeKind::StereoPanner { pan }, Param::Pan) => *pan = value,
            _ => {}
        }
    }

    /* ---------------- scheduling ---------------- */

    /// `node.start(when)` — an oscillator, or a looping bed with no duration.
    pub fn start(&mut self, node: NodeId, when: f64) {
        self.schedule.push(ScheduleEvent {
            node,
            kind: Schedule::Start,
            when,
            offset: None,
            duration: None,
        });
    }

    /// `src.start(when, src._offset)` — a looping source read from its stashed
    /// random offset, with no duration limit.
    pub fn start_source_open(&mut self, node: NodeId, when: f64) {
        let offset = self.source_offset(node);
        self.schedule.push(ScheduleEvent {
            node,
            kind: Schedule::Start,
            when,
            offset: Some(offset),
            duration: None,
        });
    }

    /// `src.start(when, src._offset, duration)`.
    ///
    /// The offset argument is not at the call site because it never varies: every
    /// buffer-source `start` in the source passes back the very `_offset` that
    /// `NoiseBank.source` stashed on that node moments earlier. The graph holds
    /// it on the node for the same reason the JavaScript holds it on the node.
    pub fn start_source(&mut self, node: NodeId, when: f64, duration: f64) {
        let offset = self.source_offset(node);
        self.schedule.push(ScheduleEvent {
            node,
            kind: Schedule::Start,
            when,
            offset: Some(offset),
            duration: Some(duration),
        });
    }

    pub fn stop(&mut self, node: NodeId, when: f64) {
        self.schedule.push(ScheduleEvent {
            node,
            kind: Schedule::Stop,
            when,
            offset: None,
            duration: None,
        });
    }

    /// `src._offset`, or zero for a node that is not a buffer source.
    pub fn source_offset(&self, node: NodeId) -> f64 {
        match self.nodes[node.0].kind {
            NodeKind::BufferSource { offset, .. } => offset,
            _ => 0.0,
        }
    }

    /// Attach a rendered impulse response to a convolver.
    pub fn set_convolver_buffer(&mut self, node: NodeId, buffer: Option<BufferId>) {
        if let NodeKind::Convolver { buffer: slot, .. } = &mut self.nodes[node.0].kind {
            *slot = buffer;
        }
    }
}
