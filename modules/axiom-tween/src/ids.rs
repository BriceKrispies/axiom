//! The module's value vocabulary — the pure data types [`TweenApi`] traffics in,
//! re-exported from `lib.rs` via a single `pub use ids::{…}` so they sit
//! alongside the one behavioral facade without counting as a second facade
//! (Module Law #8).
//!
//! [`TweenApi`]: crate::TweenApi

use axiom_kernel::HandleId;

/// An opaque handle to a live tween, returned by [`TweenApi::start`] and used to
/// `cancel` or `value` it. Backed by the kernel's [`HandleId`].
///
/// [`TweenApi::start`]: crate::TweenApi::start
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TweenId(HandleId);

impl TweenId {
    /// The raw `u64` the handle carries — the key an app-side `onUpdate`/
    /// `onComplete` closure table is keyed by.
    pub fn raw(self) -> u64 {
        self.0.raw()
    }

    /// Mint a tween id from a raw counter value. Crate-internal: ids are handed
    /// out only by [`TweenApi::start`](crate::TweenApi::start).
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(HandleId::from_raw(raw))
    }
}

/// A presentation display value — the `from`/`to`/sampled number a tween
/// animates. A float quantity newtype (its `new`/`get` are the boundary where a
/// raw scalar enters/leaves), deliberately *unclamped*: an overshooting curve
/// like [`Ease::BackOut`] yields a value outside `[from, to]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TweenValue(f32);

impl TweenValue {
    /// Wrap a raw display value.
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// The raw display value.
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// The seven ease curves. Dispatched branchlessly by `curve as usize` into a
/// fn-pointer table, so the discriminant order here *is* the table order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ease {
    /// `t` — no easing.
    Linear,
    /// Accelerating from zero: `t²`.
    QuadIn,
    /// Decelerating to one: `1 - (1-t)²`.
    QuadOut,
    /// Accelerate then decelerate.
    QuadInOut,
    /// A softer decelerate-to-one: `1 - (1-t)³`.
    CubicOut,
    /// A sharp decelerate-to-one (normalized exponential, exact endpoints).
    ExpoOut,
    /// Overshoots past one near the end, then settles to one.
    BackOut,
}

/// What to animate: a value `from → to` over `duration_nanos`, under `ease`. The
/// author's `onUpdate`/`onComplete` closures are *not* here — they live app-side
/// keyed by the returned [`TweenId`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TweenSpec {
    /// The start value (sampled at `t = 0`).
    pub from: TweenValue,
    /// The end value (sampled at `t = 1`).
    pub to: TweenValue,
    /// The animation length, in nanoseconds. Zero completes on the next advance.
    pub duration_nanos: u64,
    /// The curve mapping normalized time to eased progress.
    pub ease: Ease,
}

/// One tween's state this frame: its handle, current display value, and whether
/// it has completed (reached or passed its duration).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TweenSample {
    /// Which tween this sample belongs to.
    pub id: TweenId,
    /// The eased display value at the current elapsed time.
    pub value: TweenValue,
    /// `true` on the frame the tween reaches/passes its duration (fires once).
    pub completed: bool,
}
