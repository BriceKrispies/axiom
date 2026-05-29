//! A message wrapped with deterministic ordering metadata.

use crate::message_id::MessageId;
use crate::message_kind::MessageKind;
use crate::tick::Tick;

/// A message together with the metadata the kernel needs to order it.
///
/// The kernel knows nothing about payload *contents* — they are opaque bytes.
/// What it does provide is a deterministic ordering key: envelopes order by
/// `(tick, id)`, so two runs that enqueue the same envelopes in the same order
/// observe the same sequence. The payload is owned bytes to keep envelopes
/// self-contained and copy-free of any external lifetime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEnvelope {
    id: MessageId,
    kind: MessageKind,
    tick: Tick,
    payload: Vec<u8>,
}

impl MessageEnvelope {
    /// Wrap a payload with its identity, kind and originating tick.
    pub fn new(id: MessageId, kind: MessageKind, tick: Tick, payload: Vec<u8>) -> Self {
        MessageEnvelope {
            id,
            kind,
            tick,
            payload,
        }
    }

    /// The message's stable identity.
    pub fn id(&self) -> MessageId {
        self.id
    }

    /// The message's classification.
    pub fn kind(&self) -> MessageKind {
        self.kind
    }

    /// The tick at which the message originated.
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// The opaque payload bytes.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// The deterministic ordering key, `(tick, id)`.
    ///
    /// Stable and independent of insertion order, so callers that need a
    /// canonical order can sort by it reproducibly.
    pub fn ordering_key(&self) -> (Tick, MessageId) {
        (self.tick, self.id)
    }
}

/// Total order by `(tick, id)` — deterministic and independent of allocation.
impl Ord for MessageEnvelope {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.ordering_key().cmp(&other.ordering_key())
    }
}

impl PartialOrd for MessageEnvelope {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(id: u64, tick: u64) -> MessageEnvelope {
        MessageEnvelope::new(
            MessageId::from_raw(id),
            MessageKind::new(1),
            Tick::new(tick),
            vec![id as u8],
        )
    }

    #[test]
    fn accessors_return_constructed_parts() {
        let e = MessageEnvelope::new(
            MessageId::from_raw(9),
            MessageKind::new(5),
            Tick::new(3),
            vec![1, 2, 3],
        );
        assert_eq!(e.id(), MessageId::from_raw(9));
        assert_eq!(e.kind(), MessageKind::new(5));
        assert_eq!(e.tick(), Tick::new(3));
        assert_eq!(e.payload(), &[1, 2, 3]);
    }

    #[test]
    fn ordering_key_is_tick_then_id() {
        assert_eq!(
            envelope(2, 5).ordering_key(),
            (Tick::new(5), MessageId::from_raw(2))
        );
    }

    #[test]
    fn ordering_prefers_earlier_tick_then_lower_id() {
        assert!(envelope(9, 1) < envelope(1, 2)); // earlier tick wins
        assert!(envelope(1, 5) < envelope(2, 5)); // same tick -> lower id wins
    }
}
