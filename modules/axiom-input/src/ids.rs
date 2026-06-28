//! The pure value-type vocabulary the [`crate::InputState`] facade traffics in.
//!
//! These are the nouns a caller must be able to *name* to drive the facade and
//! read its results: the action it binds and queries ([`ActionId`]), the neutral
//! key/button/gesture token ([`KeyToken`]), the recordable event bundle it
//! samples ([`DeviceFrame`]) and the contact it resolves ([`Pointer`]), the
//! quantized gesture it reports ([`SwipeDir`]), and the [`Tick`] the snapshot is
//! indexed by. They carry no behaviour; the behavioural contract lives behind
//! the one facade. This is the Module Law's sanctioned `ids` vocabulary.

pub use crate::action_id::ActionId;
pub use crate::device_frame::{DeviceFrame, Pointer};
pub use crate::key_token::KeyToken;
pub use crate::swipe_dir::SwipeDir;
pub use axiom_kernel::Tick;
