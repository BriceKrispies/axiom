//! # Axiom Kernel
//!
//! The kernel is the deterministic runtime substrate every Axiom layer is
//! allowed to trust. It is a root of the layer DAG (it depends on nothing). It
//! defines time, identity, errors, memory addressing, messaging, binary
//! serialization, structured logging and telemetry — and nothing else.
//!
//! ## Hard rules (enforced by `tests/architecture.rs`)
//! - Deterministic: same inputs always produce the same outputs.
//! - No wall-clock time, no randomness, no global mutable state.
//! - No browser / WebGPU / DOM / JS APIs.
//! - No imports from any higher layer (none exist).
//! - No rendering / ECS / asset / physics / input / plugin concepts.
//! - Logging and telemetry are emitted as structured data records, never printed.
//!
//! ## Public surface
//! [`KernelApi`] is the kernel's primary facade and entry point. In addition,
//! a curated set of *primitive* types is re-exported at the crate root because
//! the layers that depend on the kernel must be able to *name* the
//! deterministic primitives they adapt — for example, `axiom-runtime` stores a
//! [`SimulationClock`] field and constructs [`LogRecord`] values. The approved
//! set is enforced by
//! `tests/architecture.rs::lib_exports_are_curated_set` so accidental new
//! surface still fails the build. Every other type remains private to the
//! crate.

mod error;
mod error_code;
mod error_scope;
mod result;

mod fixed_step;
mod frame_index;
mod replay_timeline;
mod simulation_clock;
mod tick;
mod tick_delta;
mod tick_divider;
mod tick_schedule;

mod deterministic_rng;

mod meters;
mod radians;
mod ratio;
mod seconds;

mod asset_id;
mod entity_id;
mod handle_id;
mod id_macro;
mod message_id;
mod resource_id;

mod alignment;
mod byte_length;
mod byte_offset;
mod memory_range;

mod message_envelope;
mod message_kind;
mod message_queue;

mod binary_reader;
mod binary_writer;
mod endian;
mod reflect;
mod schema_version;
mod type_schema;

mod stable_hash;

mod in_memory_log_sink;
mod log_field;
mod log_level;
mod log_record;
mod log_sink;

mod in_memory_telemetry_sink;
mod metric_kind;
mod metric_value;
mod telemetry_metric;
mod telemetry_sink;

mod facade;

pub use facade::KernelApi;

pub use error::KernelError;
pub use error_code::KernelErrorCode;
pub use error_scope::KernelErrorScope;
pub use result::KernelResult;

pub use fixed_step::FixedStep;
pub use frame_index::FrameIndex;
pub use simulation_clock::SimulationClock;
pub use tick::Tick;
// `ReplayTimeline<T>` is the kernel's first type-generic primitive; the
// replayed item type belongs to the caller (see ARCHITECTURE.md).
pub use replay_timeline::ReplayTimeline;
pub use tick_divider::TickDivider;
// `TickSchedule<Id, P>` is type-generic for the same reason `ReplayTimeline`
// is; named by sim-core's wake queue and the `axiom-tick` timers facade.
pub use tick_delta::TickDelta;
pub use tick_schedule::TickSchedule;

pub use deterministic_rng::DeterministicRng;

pub use meters::Meters;
pub use radians::Radians;
pub use ratio::Ratio;
// `Seconds` is the presentation frame-delta (wall-clock `dt` for visual-only
// systems), deliberately distinct from the deterministic `Tick`/`TickDelta`.
pub use seconds::Seconds;

pub use asset_id::AssetId;
pub use entity_id::EntityId;
pub use handle_id::HandleId;
pub use message_id::MessageId;

pub use binary_reader::BinaryReader;
pub use binary_writer::BinaryWriter;
pub use schema_version::SchemaVersion;
// The branchless tagged-union read-dispatch helper lives on
// `BinaryReader::read_tagged`, alongside its sibling primitive reads.
pub use reflect::Reflect;
pub use type_schema::{FieldSchema, TypeSchema};

// A diagnostic index, never a determinism proof: byte equality proves replay
// determinism, a hash only labels/locates artifacts (recording, procgen).
pub use stable_hash::StableHash;

pub use in_memory_log_sink::InMemoryLogSink;
pub use log_field::LogField;
pub use log_level::LogLevel;
pub use log_record::LogRecord;
pub use log_sink::LogSink;

pub use in_memory_telemetry_sink::InMemoryTelemetrySink;
pub use metric_kind::MetricKind;
pub use metric_value::MetricValue;
pub use telemetry_metric::TelemetryMetric;
pub use telemetry_sink::TelemetrySink;
