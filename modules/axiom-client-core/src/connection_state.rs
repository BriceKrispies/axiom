//! The client's connection lifecycle state.

/// The three states of a client connection.
///
/// The lifecycle is strictly `Disconnected → Connecting → Connected`: a client
/// marks itself connecting, then a server `Welcome` promotes it to connected.
/// The `#[repr(u8)]` discriminants are stable so [`ClientCoreApi::status_code`]
/// can expose the state across the plain-primitive boundary without a branch.
///
/// [`ClientCoreApi::status_code`]: crate::ClientCoreApi::status_code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ConnectionState {
    /// No session: the client cannot send intents.
    Disconnected = 0,
    /// A connection is being established; still no intents until `Welcome`.
    Connecting = 1,
    /// The server has welcomed the client; intents may flow.
    Connected = 2,
}

impl ConnectionState {
    /// The stable numeric discriminant of this state.
    pub(crate) fn code(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(ConnectionState::Disconnected.code(), 0);
        assert_eq!(ConnectionState::Connecting.code(), 1);
        assert_eq!(ConnectionState::Connected.code(), 2);
    }

    #[test]
    fn states_are_distinct() {
        assert_ne!(ConnectionState::Disconnected, ConnectionState::Connected);
        assert_ne!(ConnectionState::Connecting, ConnectionState::Connected);
    }
}
