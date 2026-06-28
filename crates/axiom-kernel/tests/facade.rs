//! External-facing smoke test.
//!
//! An integration test compiles as a *separate crate*, so it sees exactly what
//! a real downstream layer sees: the single public export `KernelApi`. This
//! proves the facade is genuinely reachable and usable across the crate
//! boundary using only that one name — internal types are exercised through the
//! values the facade returns, never named directly.

use axiom_kernel::KernelApi;

#[test]
fn the_only_public_name_drives_every_capability() {
    let api = KernelApi::new();

    // Schema.
    assert_eq!(api.schema_version().major(), 0);

    // Deterministic clock.
    let step = api.fixed_step(16_666_667).expect("positive step");
    let mut clock = api.simulation_clock(step);
    clock.advance_by(60).expect("no overflow");
    assert_eq!(clock.tick().raw(), 60);
    assert_eq!(clock.elapsed_nanos(), 16_666_667 * 60);

    // Binary round-trip through an id.
    let id = api.entity_id(0xABCD);
    assert!(id.is_valid());
    let mut writer = api.binary_writer();
    id.write_to(&mut writer);
    let bytes = writer.into_bytes();
    let mut reader = api.binary_reader(&bytes);
    assert_eq!(reader.read_u64().unwrap(), 0xABCD);

    // Memory math.
    let range = api.memory_range(0, 64).expect("valid range");
    assert!(range.contains_offset(0));
    assert!(!range.contains_offset(64));
    assert!(range.is_aligned(api.alignment(64).unwrap()));

    // Message queue FIFO is reachable.
    let mut queue = api.message_queue();
    assert!(queue.is_empty());
    assert!(queue.pop().is_none());

    // Sinks are reachable and start empty.
    assert!(api.log_sink().is_empty());
    assert!(api.telemetry_sink().is_empty());
}
