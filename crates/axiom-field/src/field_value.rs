//! The runtime carrier of a field value: a **tagged struct**, never a
//! data-carrying enum.

use axiom_math::{Vec2, Vec3, Vec4};
use axiom_recipe::Scalar;

use crate::field_type::FieldType;

/// One typed field value.
///
/// This is a **tagged struct**, not a data-carrying enum (the Branchless Law; the
/// `RenderCommand` precedent in `modules/axiom-render`): [`FieldValue::ty`]
/// selects which lanes are meaningful, and every other lane holds
/// [`FieldValue::UNUSED_LANE`] — a fixed default that is never read for the wrong
/// type. Construction goes through [`FieldValue::scalar`] /
/// [`FieldValue::vec2`] / [`FieldValue::vec3`] / [`FieldValue::vec4`];
/// inspection through the `as_*` accessors. There is no `match` over the value's
/// shape anywhere in this layer.
///
/// A wider accessor on a narrower value is **defined, not undefined**: reading
/// [`FieldValue::as_vec4`] on a `Scalar` yields `(x, 0, 0, 0)`. That is the
/// documented default, and it is what makes the accessors branchless.
///
/// The lane type is [`axiom_recipe::Scalar`], reused rather than redefined: it is
/// already the sanctioned quantity newtype for a raw `f32` inside a graph, so no
/// naked float reaches this layer's public API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldValue {
    ty: FieldType,
    x: Scalar,
    y: Scalar,
    z: Scalar,
    w: Scalar,
}

impl FieldValue {
    /// The fixed value every lane holds while it is not meaningful for the
    /// value's [`FieldType`]. Chosen once and used everywhere, so a decoded
    /// value with garbage in an unused lane is impossible.
    pub const UNUSED_LANE: Scalar = Scalar::new(0.0);

    /// The zero scalar — the parameter table's fill value for a slot no author
    /// has declared yet.
    pub const ZERO: FieldValue = FieldValue {
        ty: FieldType::Scalar,
        x: FieldValue::UNUSED_LANE,
        y: FieldValue::UNUSED_LANE,
        z: FieldValue::UNUSED_LANE,
        w: FieldValue::UNUSED_LANE,
    };

    /// A one-lane value.
    pub const fn scalar(value: Scalar) -> Self {
        FieldValue {
            ty: FieldType::Scalar,
            x: value,
            ..FieldValue::ZERO
        }
    }

    /// A two-lane value.
    pub const fn vec2(value: Vec2) -> Self {
        FieldValue {
            ty: FieldType::Vec2,
            x: Scalar::new(value.x),
            y: Scalar::new(value.y),
            ..FieldValue::ZERO
        }
    }

    /// A three-lane value.
    pub const fn vec3(value: Vec3) -> Self {
        FieldValue {
            ty: FieldType::Vec3,
            x: Scalar::new(value.x),
            y: Scalar::new(value.y),
            z: Scalar::new(value.z),
            ..FieldValue::ZERO
        }
    }

    /// A four-lane value — and, by convention, a linear RGBA colour.
    pub const fn vec4(value: Vec4) -> Self {
        FieldValue {
            ty: FieldType::Vec4,
            x: Scalar::new(value.x),
            y: Scalar::new(value.y),
            z: Scalar::new(value.z),
            w: Scalar::new(value.w),
        }
    }

    /// Which of the four types this value carries.
    pub const fn ty(self) -> FieldType {
        self.ty
    }

    /// The first lane. On a vector value this is its `x` component.
    pub const fn as_scalar(self) -> Scalar {
        self.x
    }

    /// The first two lanes. On a `Scalar` the second lane is
    /// [`FieldValue::UNUSED_LANE`].
    pub const fn as_vec2(self) -> Vec2 {
        Vec2::new(self.x.get(), self.y.get())
    }

    /// The first three lanes. Lanes past this value's width are
    /// [`FieldValue::UNUSED_LANE`].
    pub const fn as_vec3(self) -> Vec3 {
        Vec3::new(self.x.get(), self.y.get(), self.z.get())
    }

    /// All four lanes. Lanes past this value's width are
    /// [`FieldValue::UNUSED_LANE`].
    pub const fn as_vec4(self) -> Vec4 {
        Vec4::new(self.x.get(), self.y.get(), self.z.get(), self.w.get())
    }

    /// The four lanes as canonical little-endian bit patterns — the wire form of
    /// a value, and the parameter words of a `Const` node.
    pub(crate) fn words(self) -> [u32; 4] {
        [
            self.x.get().to_bits(),
            self.y.get().to_bits(),
            self.z.get().to_bits(),
            self.w.get().to_bits(),
        ]
    }

    /// Rebuild a value from a type and four lane words.
    ///
    /// Lanes past `ty`'s width are forced back to [`FieldValue::UNUSED_LANE`]
    /// rather than trusted, so the tagged struct's invariant survives hostile
    /// bytes and a decoded value always digests identically to a constructed one.
    pub(crate) fn from_words(ty: FieldType, words: [u32; 4]) -> Self {
        let lane = |index: usize| {
            Scalar::new(f32::from_bits(
                [FieldValue::UNUSED_LANE.get().to_bits(), words[index]]
                    [usize::from((index as u8) < ty.lanes())],
            ))
        };
        FieldValue {
            ty,
            x: lane(0),
            y: lane(1),
            z: lane(2),
            w: lane(3),
        }
    }
}

impl Default for FieldValue {
    /// [`FieldValue::ZERO`].
    fn default() -> Self {
        FieldValue::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scalar_carries_one_lane_and_defaults_the_rest() {
        let v = FieldValue::scalar(Scalar::new(2.5));
        assert_eq!(v.ty(), FieldType::Scalar);
        assert_eq!(v.as_scalar().get(), 2.5);
        assert_eq!(v.as_vec2(), Vec2::new(2.5, 0.0));
        assert_eq!(v.as_vec3(), Vec3::new(2.5, 0.0, 0.0));
        assert_eq!(v.as_vec4(), Vec4::new(2.5, 0.0, 0.0, 0.0));
    }

    #[test]
    fn a_vec2_carries_two_lanes_and_defaults_the_rest() {
        let v = FieldValue::vec2(Vec2::new(1.0, -2.0));
        assert_eq!(v.ty(), FieldType::Vec2);
        assert_eq!(v.as_scalar().get(), 1.0);
        assert_eq!(v.as_vec2(), Vec2::new(1.0, -2.0));
        assert_eq!(v.as_vec4(), Vec4::new(1.0, -2.0, 0.0, 0.0));
    }

    #[test]
    fn a_vec3_carries_three_lanes_and_defaults_the_fourth() {
        let v = FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(v.ty(), FieldType::Vec3);
        assert_eq!(v.as_vec3(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(v.as_vec4(), Vec4::new(1.0, 2.0, 3.0, 0.0));
    }

    #[test]
    fn a_vec4_carries_every_lane() {
        let v = FieldValue::vec4(Vec4::new(0.1, 0.2, 0.3, 0.4));
        assert_eq!(v.ty(), FieldType::Vec4);
        assert_eq!(v.as_vec4(), Vec4::new(0.1, 0.2, 0.3, 0.4));
        assert_eq!(v.as_vec3(), Vec3::new(0.1, 0.2, 0.3));
        assert_eq!(v.as_vec2(), Vec2::new(0.1, 0.2));
        assert_eq!(v.as_scalar().get(), 0.1);
    }

    #[test]
    fn the_unused_lane_default_and_zero_agree() {
        assert_eq!(FieldValue::UNUSED_LANE.get(), 0.0);
        assert_eq!(FieldValue::ZERO, FieldValue::scalar(Scalar::new(0.0)));
        assert_eq!(FieldValue::default(), FieldValue::ZERO);
    }

    #[test]
    fn words_round_trip_every_type() {
        let values = [
            FieldValue::scalar(Scalar::new(-7.25)),
            FieldValue::vec2(Vec2::new(1.5, 2.5)),
            FieldValue::vec3(Vec3::new(1.5, 2.5, 3.5)),
            FieldValue::vec4(Vec4::new(1.5, 2.5, 3.5, 4.5)),
        ];
        values.iter().for_each(|v| {
            assert_eq!(FieldValue::from_words(v.ty(), v.words()), *v);
        });
    }

    #[test]
    fn decoding_scrubs_lanes_past_the_declared_width() {
        let hostile = FieldValue::from_words(
            FieldType::Vec2,
            [1.0_f32.to_bits(), 2.0_f32.to_bits(), 9.0_f32.to_bits(), 9.0_f32.to_bits()],
        );
        assert_eq!(hostile, FieldValue::vec2(Vec2::new(1.0, 2.0)));
        assert_eq!(hostile.as_vec4(), Vec4::new(1.0, 2.0, 0.0, 0.0));
    }

    #[test]
    fn values_of_different_types_are_never_equal() {
        assert_ne!(
            FieldValue::scalar(Scalar::new(1.0)),
            FieldValue::vec2(Vec2::new(1.0, 0.0))
        );
    }
}
