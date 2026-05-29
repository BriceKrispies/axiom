//! The kernel facade: the crate's single public export.

use crate::alignment::Alignment;
use crate::asset_id::AssetId;
use crate::binary_reader::BinaryReader;
use crate::binary_writer::BinaryWriter;
use crate::byte_length::ByteLength;
use crate::byte_offset::ByteOffset;
use crate::endian::Endian;
use crate::entity_id::EntityId;
use crate::fixed_step::FixedStep;
use crate::handle_id::HandleId;
use crate::in_memory_log_sink::InMemoryLogSink;
use crate::in_memory_telemetry_sink::InMemoryTelemetrySink;
use crate::layer_id::LayerId;
use crate::layer_manifest::LayerManifest;
use crate::log_record::LogRecord;
use crate::log_sink::LogSink;
use crate::message_id::MessageId;
use crate::message_queue::MessageQueue;
use crate::resource_id::ResourceId;
use crate::result::KernelResult;
use crate::schema_version::SchemaVersion;
use crate::simulation_clock::SimulationClock;
use crate::telemetry_metric::TelemetryMetric;
use crate::telemetry_sink::TelemetrySink;

/// The single public entry point to the Axiom kernel.
///
/// `KernelApi` is the *only* item `lib.rs` exports. Every kernel capability is
/// reached through one of its constructors, so callers depend on one stable
/// name rather than dozens of loose exports. The facade is a zero-sized value;
/// it holds no state and reads nothing ambient.
#[derive(Debug, Clone, Copy, Default)]
pub struct KernelApi {
    // Sealed: a private unit field keeps `KernelApi` zero-sized and stateless
    // today while leaving room to carry deterministic configuration later
    // without a breaking change, and prevents external literal construction.
    _sealed: (),
}

impl KernelApi {
    /// The binary schema version this kernel serializes with.
    pub const SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(0, 1);

    /// Construct the facade.
    pub const fn new() -> Self {
        KernelApi { _sealed: () }
    }

    /// The binary schema version this kernel serializes with.
    pub const fn schema_version(&self) -> SchemaVersion {
        Self::SCHEMA_VERSION
    }

    /// The byte order the kernel serializes in (always little-endian).
    pub const fn kernel_endian(&self) -> Endian {
        Endian::KERNEL
    }

    /// Whether the kernel serializes in little-endian order. Always `true`;
    /// exposed so layers can assert the byte-order contract explicitly.
    pub const fn serializes_little_endian(&self) -> bool {
        Endian::KERNEL.is_little()
    }

    /// Hand a structured log record to a sink. This is the kernel's only
    /// logging path: records are emitted as data, never printed.
    pub fn log(&self, sink: &mut impl LogSink, record: LogRecord) {
        sink.record(record);
    }

    /// Hand a telemetry sample to a sink. Samples are recorded as data; the
    /// kernel never exports them externally.
    pub fn record_metric(&self, sink: &mut impl TelemetrySink, metric: TelemetryMetric) {
        sink.record(metric);
    }

    /// The canonical kernel (Layer 00) manifest.
    pub fn kernel_manifest(&self) -> LayerManifest {
        LayerManifest::kernel()
    }

    /// Begin a manifest for a higher layer at `index` named `name`.
    pub fn layer_manifest(&self, index: u16, name: &'static str) -> LayerManifest {
        LayerManifest::new(index, name)
    }

    /// Construct a validated fixed timestep of `nanos` nanoseconds.
    pub const fn fixed_step(&self, nanos: u64) -> KernelResult<FixedStep> {
        FixedStep::new(nanos)
    }

    /// Create an initial simulation clock advancing by `step`.
    pub const fn simulation_clock(&self, step: FixedStep) -> SimulationClock {
        SimulationClock::new(step)
    }

    /// Construct a validated power-of-two alignment.
    pub const fn alignment(&self, value: u64) -> KernelResult<Alignment> {
        Alignment::new(value)
    }

    /// Construct a checked memory range from raw offset and length.
    pub const fn memory_range(
        &self,
        offset: u64,
        length: u64,
    ) -> KernelResult<crate::memory_range::MemoryRange> {
        crate::memory_range::MemoryRange::new(ByteOffset::new(offset), ByteLength::new(length))
    }

    /// Create an empty message queue.
    pub fn message_queue(&self) -> MessageQueue {
        MessageQueue::new()
    }

    /// Create an empty binary writer.
    pub fn binary_writer(&self) -> BinaryWriter {
        BinaryWriter::new()
    }

    /// Create a binary reader over `data`.
    pub fn binary_reader<'a>(&self, data: &'a [u8]) -> BinaryReader<'a> {
        BinaryReader::new(data)
    }

    /// Create an empty in-memory log sink.
    pub fn log_sink(&self) -> InMemoryLogSink {
        InMemoryLogSink::new()
    }

    /// Create an empty in-memory telemetry sink.
    pub fn telemetry_sink(&self) -> InMemoryTelemetrySink {
        InMemoryTelemetrySink::new()
    }

    /// Construct an [`EntityId`] from a raw value.
    pub const fn entity_id(&self, raw: u64) -> EntityId {
        EntityId::from_raw(raw)
    }

    /// Construct a [`ResourceId`] from a raw value.
    pub const fn resource_id(&self, raw: u64) -> ResourceId {
        ResourceId::from_raw(raw)
    }

    /// Construct an [`AssetId`] from a raw value.
    pub const fn asset_id(&self, raw: u64) -> AssetId {
        AssetId::from_raw(raw)
    }

    /// Construct a [`HandleId`] from a raw value.
    pub const fn handle_id(&self, raw: u64) -> HandleId {
        HandleId::from_raw(raw)
    }

    /// Construct a [`LayerId`] from a raw value.
    pub const fn layer_id(&self, raw: u64) -> LayerId {
        LayerId::from_raw(raw)
    }

    /// Construct a [`MessageId`] from a raw value.
    pub const fn message_id(&self, raw: u64) -> MessageId {
        MessageId::from_raw(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_field::LogField;
    use crate::log_level::LogLevel;
    use crate::log_record::LogRecord;
    use crate::message_envelope::MessageEnvelope;
    use crate::message_kind::MessageKind;
    use crate::metric_value::MetricValue;
    use crate::telemetry_metric::TelemetryMetric;
    use crate::tick::Tick;

    #[test]
    fn facade_exposes_schema_and_kernel_manifest() {
        let api = KernelApi::new();
        assert_eq!(api.schema_version(), SchemaVersion::new(0, 1));
        let manifest = api.kernel_manifest();
        assert_eq!(manifest.name(), "axiom-kernel");
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn facade_drives_the_simulation_clock() {
        let api = KernelApi::new();
        let step = api.fixed_step(1_000).unwrap();
        let mut clock = api.simulation_clock(step);
        clock.advance_by(3).unwrap();
        assert_eq!(clock.tick(), Tick::new(3));
        assert_eq!(clock.elapsed_nanos(), 3_000);
    }

    #[test]
    fn facade_round_trips_binary_with_an_id() {
        let api = KernelApi::new();
        let id = api.entity_id(123);

        let mut writer = api.binary_writer();
        id.write_to(&mut writer);
        writer.write_u32(7);
        let bytes = writer.into_bytes();

        let mut reader = api.binary_reader(&bytes);
        assert_eq!(EntityId::read_from(&mut reader).unwrap(), id);
        assert_eq!(reader.read_u32().unwrap(), 7);
    }

    #[test]
    fn facade_memory_and_alignment_helpers() {
        let api = KernelApi::new();
        let range = api.memory_range(16, 32).unwrap();
        assert_eq!(range.end(), 48);
        assert!(range.is_aligned(api.alignment(16).unwrap()));
        assert!(api.alignment(0).is_err());
    }

    #[test]
    fn facade_message_queue_is_fifo() {
        let api = KernelApi::new();
        let mut q = api.message_queue();
        q.push(MessageEnvelope::new(
            api.message_id(1),
            MessageKind::new(0),
            Tick::new(0),
            vec![],
        ));
        q.push(MessageEnvelope::new(
            api.message_id(2),
            MessageKind::new(0),
            Tick::new(1),
            vec![],
        ));
        assert_eq!(q.pop().unwrap().id(), api.message_id(1));
        assert_eq!(q.pop().unwrap().id(), api.message_id(2));
    }

    #[test]
    fn facade_emits_logs_and_telemetry_through_sinks() {
        let api = KernelApi::new();

        let mut logs = api.log_sink();
        api.log(
            &mut logs,
            LogRecord::new(LogLevel::Info, "kernel.facade", 1, "smoke")
                .with_field(LogField::u64("n", 1)),
        );
        assert_eq!(logs.len(), 1);

        let mut telemetry = api.telemetry_sink();
        api.record_metric(
            &mut telemetry,
            TelemetryMetric::counter("smoke", 1, Some(Tick::new(0))),
        );
        api.record_metric(
            &mut telemetry,
            TelemetryMetric::gauge("load", MetricValue::float(0.5), None),
        );
        assert_eq!(telemetry.counter_total("smoke"), 1);
        assert_eq!(telemetry.len(), 2);
    }

    #[test]
    fn facade_reports_little_endian_byte_order() {
        let api = KernelApi::new();
        assert_eq!(api.kernel_endian(), crate::endian::Endian::Little);
        assert!(api.serializes_little_endian());
    }

    #[test]
    fn new_and_default_facades_are_equivalent() {
        let from_default = KernelApi::default();
        assert_eq!(
            from_default.schema_version(),
            KernelApi::new().schema_version()
        );
    }

    #[test]
    fn facade_validates_a_higher_layer_manifest() {
        use crate::layer_dependency::LayerDependency;
        let api = KernelApi::new();
        let manifest = api
            .layer_manifest(1, "axiom-fake")
            .with_dependency(LayerDependency::new(0))
            .unwrap();
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn facade_id_constructors_cover_every_id_type() {
        let api = KernelApi::new();
        assert!(api.entity_id(1).is_valid());
        assert!(api.resource_id(1).is_valid());
        assert!(api.asset_id(1).is_valid());
        assert!(api.handle_id(1).is_valid());
        assert!(api.layer_id(1).is_valid());
        assert!(api.message_id(1).is_valid());
    }
}
