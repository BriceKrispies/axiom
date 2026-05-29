//! Stable identifier for a message.

use crate::id_macro::define_id;

define_id! {
    /// A stable, strongly-typed identifier for a message.
    ///
    /// Used by [`crate::message_envelope::MessageEnvelope`] to give every
    /// message a deterministic identity independent of queue position.
    MessageId
}
