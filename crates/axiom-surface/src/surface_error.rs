//! Stable, deterministic surface failures that name the channel, the layer and
//! the field node they concern.

use axiom_field::{FieldError, NodeId};

use crate::channel::SurfaceChannel;

/// Why a byte stream is not a valid surface, or why a surface is not a legal
/// appearance description.
///
/// Deterministic, fieldless and `Copy`. This is the *kind* only: the channel,
/// the layer index and the field node a failure concerns are fields on
/// [`SurfaceError`], never enum payloads — a data-carrying variant would force a
/// `match` on read and violate the Branchless Law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceErrorCode {
    /// The bytes could not be decoded, or they describe a tree whose parent
    /// links are not a tree.
    MalformedData,
    /// A blend code names no [`crate::LayerBlend`].
    UnknownBlend,
    /// A lighting-model code names no [`crate::LightingModel`].
    UnknownLightingModel,
    /// A binding kind code names no binding kind.
    UnknownBindingKind,
    /// A type code names no `FieldType`.
    UnknownType,
    /// A channel binding carries a different type than the channel declares.
    ChannelTypeMismatch,
    /// A layer mask is not a `Scalar`.
    MaskTypeMismatch,
    /// The layer tree holds more than [`crate::MAX_LAYERS`] layers. Never a
    /// silent truncation.
    LayerBudgetExceeded,
    /// A node of a bound field graph carries an operator code that names no
    /// field operator, so the graph cannot be composed.
    UnknownOperator,
    /// A bound or composed field graph is not a well-formed, well-typed
    /// program. The underlying field diagnostic rides in
    /// [`SurfaceError::field_code`], [`SurfaceError::node`] and the message.
    InvalidField,
}

impl SurfaceErrorCode {
    /// A stable numeric discriminant for asserting on *which* failure occurred
    /// without depending on the variant layout. Table-indexed by the fieldless
    /// discriminant, so it is branch-free.
    pub const fn code(self) -> u16 {
        [1_u16, 2, 3, 4, 5, 6, 7, 8, 9, 10][self as usize]
    }
}

/// The locator value meaning "this failure concerns no particular channel or
/// layer". A real channel code is `0..7` and a real layer index is
/// `0..=MAX_LAYERS`, so `u16::MAX` can never collide with one.
const NO_LOCATION: u16 = u16::MAX;

/// The [`SurfaceError::field_code`] of a failure that did not come from the
/// field layer. Field codes start at `1`, so zero can never collide with one.
const NO_FIELD_CODE: u16 = 0;

/// One surface failure: what went wrong, **where** it went wrong — the channel,
/// the layer index within the flattened tree, and the field-graph node — and a
/// human-readable explanation.
///
/// Identity is the machine data, never the message, so a test asserts on what
/// the failure *is* rather than on how it is worded.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceError {
    code: SurfaceErrorCode,
    channel: u16,
    layer: u16,
    node: NodeId,
    field_code: u16,
    message: &'static str,
}

impl SurfaceError {
    /// A failure that has not yet been located. The scan that walks the surface
    /// stamps the channel, layer or node onto it.
    pub const fn at(code: SurfaceErrorCode, message: &'static str) -> Self {
        SurfaceError {
            code,
            channel: NO_LOCATION,
            layer: NO_LOCATION,
            node: NodeId::NULL,
            field_code: NO_FIELD_CODE,
            message,
        }
    }

    /// Locate the failure at a channel.
    pub const fn about_channel(self, channel: SurfaceChannel) -> Self {
        SurfaceError {
            channel: channel.code(),
            ..self
        }
    }

    /// Locate the failure at a layer index — `0` is the root surface, `1..` are
    /// its layers in the order [`crate::Surface::flatten`] visits them.
    pub const fn about_layer(self, layer: u16) -> Self {
        SurfaceError { layer, ..self }
    }

    /// Locate the failure at a node of a bound field graph.
    pub const fn about_node(self, node: NodeId) -> Self {
        SurfaceError { node, ..self }
    }

    /// Lift a field failure. The field layer's own stable code, the node it
    /// named and its wording are all preserved, so nothing is lost crossing the
    /// boundary and a caller never has to unwrap two error vocabularies to find
    /// out which node of which graph is wrong.
    pub fn from_field(error: FieldError) -> Self {
        SurfaceError {
            code: SurfaceErrorCode::InvalidField,
            channel: NO_LOCATION,
            layer: NO_LOCATION,
            node: error.node(),
            field_code: error.code(),
            message: error.message(),
        }
    }

    /// Which failure this is.
    pub const fn kind(self) -> SurfaceErrorCode {
        self.code
    }

    /// The stable numeric discriminant.
    pub const fn code(self) -> u16 {
        self.code.code()
    }

    /// The channel this failure concerns, or `None`.
    pub fn channel(self) -> Option<SurfaceChannel> {
        SurfaceChannel::from_code(self.channel)
    }

    /// The layer index this failure concerns, or `None`.
    pub fn layer(self) -> Option<u16> {
        (self.layer != NO_LOCATION).then_some(self.layer)
    }

    /// The field-graph node this failure concerns, or [`NodeId::NULL`].
    pub const fn node(self) -> NodeId {
        self.node
    }

    /// The lifted field error code (nonzero), or `0` when this failure did not
    /// come from the field layer.
    pub const fn field_code(self) -> u16 {
        self.field_code
    }

    /// The human-readable explanation. Never part of identity.
    pub const fn message(self) -> &'static str {
        self.message
    }
}

/// Identity is `(code, channel, layer, node, field_code)` — never the message.
/// `&` rather than `&&` because the Branchless Law forbids the short-circuiting
/// form and every side is a pure comparison that is always safe to evaluate.
impl PartialEq for SurfaceError {
    fn eq(&self, other: &Self) -> bool {
        (self.code == other.code)
            & (self.channel == other.channel)
            & (self.layer == other.layer)
            & (self.node == other.node)
            & (self.field_code == other.field_code)
    }
}

impl Eq for SurfaceError {}

/// The result of a fallible surface operation.
pub type SurfaceResult<T> = Result<T, SurfaceError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::FieldErrorCode;

    /// Every code, in discriminant order. A test-only roster: production code
    /// never enumerates the codes, it only reports one.
    const EVERY_CODE: [SurfaceErrorCode; 10] = [
        SurfaceErrorCode::MalformedData,
        SurfaceErrorCode::UnknownBlend,
        SurfaceErrorCode::UnknownLightingModel,
        SurfaceErrorCode::UnknownBindingKind,
        SurfaceErrorCode::UnknownType,
        SurfaceErrorCode::ChannelTypeMismatch,
        SurfaceErrorCode::MaskTypeMismatch,
        SurfaceErrorCode::LayerBudgetExceeded,
        SurfaceErrorCode::UnknownOperator,
        SurfaceErrorCode::InvalidField,
    ];

    #[test]
    fn codes_are_stable_and_distinct() {
        assert_eq!(SurfaceErrorCode::MalformedData.code(), 1);
        assert_eq!(SurfaceErrorCode::LayerBudgetExceeded.code(), 8);
        assert_eq!(SurfaceErrorCode::InvalidField.code(), 10);
        EVERY_CODE
            .iter()
            .enumerate()
            .for_each(|(index, code)| assert_eq!(code.code() as usize, index + 1));
    }

    #[test]
    fn an_unlocated_error_names_nothing() {
        let error = SurfaceError::at(SurfaceErrorCode::MalformedData, "bad bytes");
        assert_eq!(error.kind(), SurfaceErrorCode::MalformedData);
        assert_eq!(error.code(), 1);
        assert_eq!(error.channel(), None);
        assert_eq!(error.layer(), None);
        assert_eq!(error.node(), NodeId::NULL);
        assert_eq!(error.field_code(), 0);
        assert_eq!(error.message(), "bad bytes");
    }

    #[test]
    fn an_error_can_be_located_after_the_fact() {
        let located = SurfaceError::at(SurfaceErrorCode::ChannelTypeMismatch, "wrong type")
            .about_channel(SurfaceChannel::Roughness)
            .about_layer(2)
            .about_node(NodeId::from_raw(5));
        assert_eq!(located.channel(), Some(SurfaceChannel::Roughness));
        assert_eq!(located.layer(), Some(2));
        assert_eq!(located.node(), NodeId::from_raw(5));
        assert_eq!(located.kind(), SurfaceErrorCode::ChannelTypeMismatch);
    }

    #[test]
    fn a_field_failure_keeps_its_code_node_and_wording() {
        let lifted = SurfaceError::from_field(FieldError::at(
            FieldErrorCode::TypeMismatch,
            NodeId::from_raw(3),
            "field said so",
        ));
        assert_eq!(lifted.kind(), SurfaceErrorCode::InvalidField);
        assert_eq!(lifted.node(), NodeId::from_raw(3));
        assert_eq!(lifted.field_code(), FieldErrorCode::TypeMismatch.code());
        assert_eq!(lifted.message(), "field said so");
        assert_eq!(lifted.channel(), None);
        assert_eq!(lifted.layer(), None);
    }

    #[test]
    fn the_message_is_not_part_of_identity_but_every_locator_is() {
        let one = SurfaceError::at(SurfaceErrorCode::UnknownBlend, "one wording");
        let other = SurfaceError::at(SurfaceErrorCode::UnknownBlend, "another wording");
        assert_eq!(one, other);
        assert_ne!(one, SurfaceError::at(SurfaceErrorCode::UnknownType, "one wording"));
        assert_ne!(one, one.about_channel(SurfaceChannel::Normal));
        assert_ne!(one, one.about_layer(0));
        assert_ne!(one, one.about_node(NodeId::from_raw(1)));
        assert_ne!(
            one,
            SurfaceError::from_field(FieldError::at(
                FieldErrorCode::MalformedData,
                NodeId::NULL,
                "x"
            ))
        );
    }
}
