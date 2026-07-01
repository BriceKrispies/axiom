//! The structured, deterministic causal-chain report and digests.
//!
//! Rows are read straight from the sim-core causal journal (no prose generation).
//! Parent attribution is recaptured after the run from the durable command and
//! grooming-process causes (see `Crucible::recapture_parents`) and passed in.

use std::collections::BTreeMap;

use axiom_sim_core::SimCoreApi;

use crate::scenario;

/// Which cause a causal event was parented to, as decoded raw ids (the report
/// table is plain data; the facade's `CauseRef` itself stays behind the facade).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParentRef {
    Command,
    /// Raw process id.
    Process(u64),
    /// Raw event id.
    Event(u64),
    Unknown,
}

impl ParentRef {
    fn parts(self) -> (u8, u64) {
        match self {
            ParentRef::Command => (0, 0),
            ParentRef::Process(id) => (1, id),
            ParentRef::Event(id) => (2, id),
            ParentRef::Unknown => (3, 0),
        }
    }
}

/// One row of the causal-chain report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalRow {
    pub tick: u64,
    pub event_id: u64,
    pub kind: u32,
    pub subject: Option<u64>,
    pub secondary: Option<u64>,
    pub route: Option<u8>,
    pub substance: Option<&'static str>,
    pub parent: ParentRef,
    pub code: u64,
    pub payload_present: bool,
    pub label: &'static str,
}

/// Decode a causal-event kind into `(label, route, substance)`. Scenario kinds
/// carry route/substance; substrate scheduler kinds are generic lifecycle events.
fn describe(kind: u32) -> (&'static str, Option<u8>, Option<&'static str>) {
    let beer = Some(scenario::SUBSTANCE_NAME);
    match kind {
        scenario::KIND_CONTACT_INTERACTION => {
            ("contact-interaction", Some(scenario::ROUTE_TOUCH), beer)
        }
        scenario::KIND_CONTACT_TRANSFER => ("contact-transfer", Some(scenario::ROUTE_TOUCH), beer),
        scenario::KIND_GROOM_TRANSFER => ("groom-transfer", Some(scenario::ROUTE_INGESTION), beer),
        scenario::KIND_INGESTION => (
            "ingestion-interaction",
            Some(scenario::ROUTE_INGESTION),
            beer,
        ),
        scenario::KIND_INTOX_EFFECT => {
            ("intoxication-effect", Some(scenario::ROUTE_INGESTION), beer)
        }
        SimCoreApi::SCHED_EVENT_SCHEDULED => ("process-scheduled", None, None),
        SimCoreApi::SCHED_EVENT_WOKE => ("process-woke", None, None),
        SimCoreApi::SCHED_EVENT_STARTED => ("process-started", None, None),
        SimCoreApi::SCHED_EVENT_COMPLETED => ("process-completed", None, None),
        SimCoreApi::SCHED_EVENT_PRODUCED_EFFECTS => ("process-produced-effects", None, None),
        SimCoreApi::SCHED_EVENT_EFFECTS_APPLIED => ("effects-applied", None, None),
        _ => ("event", None, None),
    }
}

/// Build the ordered causal-chain rows from the journal plus the captured parents.
pub fn build_rows(api: &SimCoreApi, parents: &BTreeMap<u64, ParentRef>) -> Vec<CausalRow> {
    api.all_causal_event_ids()
        .into_iter()
        .filter_map(|id| {
            api.causal_event(id).map(|event| {
                let kind = event.kind().code();
                let (label, route, substance) = describe(kind);
                CausalRow {
                    tick: event.tick(),
                    event_id: id.raw(),
                    kind,
                    subject: event.subject().map(|h| h.id().raw()),
                    secondary: event.secondary().map(|h| h.id().raw()),
                    route,
                    substance,
                    parent: parents
                        .get(&id.raw())
                        .copied()
                        .unwrap_or(ParentRef::Unknown),
                    code: event.code(),
                    payload_present: event.payload().is_some(),
                    label,
                }
            })
        })
        .collect()
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

fn fold(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
    }
    hash
}

/// A digest over just the causal chain (kinds, codes, subjects, ticks, parents).
pub fn causal_digest(rows: &[CausalRow]) -> u64 {
    let mut hash = FNV_OFFSET;
    for row in rows {
        hash = fold(hash, &row.kind.to_le_bytes());
        hash = fold(hash, &row.code.to_le_bytes());
        hash = fold(hash, &row.tick.to_le_bytes());
        hash = fold(hash, &row.subject.unwrap_or(0).to_le_bytes());
        hash = fold(hash, &row.secondary.unwrap_or(0).to_le_bytes());
        let (tag, id) = row.parent.parts();
        hash = fold(hash, &[tag, row.payload_present as u8]);
        hash = fold(hash, &id.to_le_bytes());
    }
    hash
}

/// A structural digest over the final outcome plus the causal chain.
pub fn state_digest(
    rows: &[CausalRow],
    paw_amount: i64,
    mouth_amount: i64,
    intox_active: bool,
    groomed: bool,
) -> u64 {
    let mut hash = causal_digest(rows);
    hash = fold(hash, &paw_amount.to_le_bytes());
    hash = fold(hash, &mouth_amount.to_le_bytes());
    hash = fold(hash, &[intox_active as u8, groomed as u8]);
    hash
}

/// Render the rows as a deterministic, structured table (one line per event).
pub fn render(rows: &[CausalRow]) -> String {
    let mut out = String::new();
    out.push_str("tick  event  kind   label                      subj  sec   route  substance  parent        code  payload\n");
    for row in rows {
        let route = row
            .route
            .map(|r| r.to_string())
            .unwrap_or_else(|| "-".to_string());
        let substance = row.substance.unwrap_or("-");
        let parent = match row.parent {
            ParentRef::Command => "command".to_string(),
            ParentRef::Process(id) => format!("process:{id}"),
            ParentRef::Event(id) => format!("event:{id}"),
            ParentRef::Unknown => "-".to_string(),
        };
        out.push_str(&format!(
            "{:<5} {:<6} {:<6} {:<26} {:<5} {:<5} {:<6} {:<10} {:<13} {:<5} {}\n",
            row.tick,
            row.event_id,
            row.kind,
            row.label,
            row.subject
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".to_string()),
            row.secondary
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".to_string()),
            route,
            substance,
            parent,
            row.code,
            row.payload_present,
        ));
    }
    out
}
