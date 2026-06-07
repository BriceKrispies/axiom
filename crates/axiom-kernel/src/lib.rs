//! # Axiom Kernel — Layer 00
//!
//! The kernel is the deterministic runtime substrate every future Axiom layer
//! is allowed to trust. It defines time, identity, errors, memory addressing,
//! messaging, binary serialization, the layer contract, structured logging and
//! telemetry — and nothing else.
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
//! future layers must be able to *name* the deterministic primitives they
//! adapt — for example, Layer 1 (`axiom-runtime`) stores a [`SimulationClock`]
//! field and constructs [`LogRecord`] values. The approved set is enforced by
//! `tests/architecture.rs::lib_exports_are_curated_set` so accidental new
//! surface still fails the build. Every other type remains private to the
//! crate.

// --- Result / error model ---
mod error;
mod error_code;
mod error_scope;
mod result;

// --- Deterministic time model ---
mod fixed_step;
mod frame_index;
mod simulation_clock;
mod tick;

// --- Dimensioned scalar quantity model ---
mod meters;
mod radians;
mod ratio;

// --- Stable ID model ---
mod asset_id;
mod entity_id;
mod handle_id;
mod id_macro;
mod layer_id;
mod message_id;
mod resource_id;

// --- Memory / address model ---
mod alignment;
mod byte_length;
mod byte_offset;
mod memory_range;

// --- Message / event model ---
mod message_envelope;
mod message_kind;
mod message_queue;

// --- Binary serialization primitives ---
mod binary_reader;
mod binary_writer;
mod endian;
mod reflect;
mod schema_version;
mod type_schema;

// --- Layer contract model ---
mod layer_capability;
mod layer_dependency;
mod layer_import_rule;
mod layer_manifest;

// --- Structured logging model ---
mod in_memory_log_sink;
mod log_field;
mod log_level;
mod log_record;
mod log_sink;

// --- Telemetry model ---
mod in_memory_telemetry_sink;
mod metric_kind;
mod metric_value;
mod telemetry_metric;
mod telemetry_sink;

// --- The single public facade ---
mod facade;

// --- Public surface (curated; see `tests/architecture.rs`) ---

// Primary entry point.
pub use facade::KernelApi;

// Error and result primitives — layers return KernelResult and match on
// (scope, code) identity.
pub use error::KernelError;
pub use error_code::KernelErrorCode;
pub use error_scope::KernelErrorScope;
pub use result::KernelResult;

// Deterministic time primitives — layers stamp steps with Tick/FrameIndex and
// own a SimulationClock advanced by a FixedStep.
pub use fixed_step::FixedStep;
pub use frame_index::FrameIndex;
pub use simulation_clock::SimulationClock;
pub use tick::Tick;

// Dimensioned scalar quantities — the typed boundary where a raw f32 becomes a
// length/angle/ratio, so layers above the kernel and math stop exposing naked
// floats whose unit a caller must guess. Higher layers name and construct these
// directly (e.g. a camera's `fovy: Radians`, a viewport's `aspect: Ratio`).
pub use meters::Meters;
pub use radians::Radians;
pub use ratio::Ratio;

// Identity primitives used by higher layers.
pub use entity_id::EntityId;
pub use handle_id::HandleId;
pub use message_id::MessageId;

// Binary serialization primitives — layers store typed `BinaryWriter` /
// `BinaryReader` values (e.g. math's `Vec3::write_to`) and feed bytes through
// them. The kernel facade still constructs them.
pub use binary_reader::BinaryReader;
pub use binary_writer::BinaryWriter;
// SchemaVersion lets higher layers stamp a `major.minor` header on their own
// wire formats and reject incompatible data — e.g. axiom-introspect's
// serialized FrameReport snapshot.
pub use schema_version::SchemaVersion;
// Reflection: a type describes its shape (TypeSchema) and round-trips its
// values. The composable formalization of the write_to/read_from idiom that
// makes engine data (e.g. ECS components) serializable and describable.
pub use reflect::Reflect;
pub use type_schema::{FieldSchema, TypeSchema};

// Layer-contract primitives — layers carry their own manifests and need to
// construct dependencies and capabilities by code to validate them in tests.
pub use layer_capability::LayerCapability;
pub use layer_dependency::LayerDependency;
pub use layer_manifest::LayerManifest;

// Structured logging — layers construct records and hand them to sinks via the
// facade; the kernel never prints.
pub use in_memory_log_sink::InMemoryLogSink;
pub use log_field::LogField;
pub use log_level::LogLevel;
pub use log_record::LogRecord;
pub use log_sink::LogSink;

// Structured telemetry.
pub use in_memory_telemetry_sink::InMemoryTelemetrySink;
pub use metric_kind::MetricKind;
pub use metric_value::MetricValue;
pub use telemetry_metric::TelemetryMetric;
pub use telemetry_sink::TelemetrySink;
