//! A flat, static description of a reflectable type's shape.

/// One field of a [`TypeSchema`]: its name and the name of its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldSchema {
    name: &'static str,
    type_name: &'static str,
}

impl FieldSchema {
    /// Describe a field.
    pub const fn new(name: &'static str, type_name: &'static str) -> Self {
        FieldSchema { name, type_name }
    }

    /// The field's name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The name of the field's type.
    pub const fn type_name(&self) -> &'static str {
        self.type_name
    }
}

/// A flat description of a reflectable type: its name and its fields.
///
/// Static, allocation-free, and `Copy` — a type describes itself with a `const`
/// so the description costs nothing at runtime. Leaf types (scalars) have no
/// fields; composites list their fields in serialization order. This is the
/// "reflection" half of [`crate::Reflect`]: enough for a caller (or agent) to
/// understand a value's shape without compile-time knowledge of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeSchema {
    name: &'static str,
    fields: &'static [FieldSchema],
}

impl TypeSchema {
    /// Describe a type by name and fields (empty for a leaf/scalar).
    pub const fn new(name: &'static str, fields: &'static [FieldSchema]) -> Self {
        TypeSchema { name, fields }
    }

    /// The type's name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The type's fields, in serialization order.
    pub const fn fields(&self) -> &'static [FieldSchema] {
        self.fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_schema_accessors() {
        let f = FieldSchema::new("translation", "Vec3");
        assert_eq!(f.name(), "translation");
        assert_eq!(f.type_name(), "Vec3");
    }

    #[test]
    fn leaf_type_schema_has_no_fields() {
        let s = TypeSchema::new("f32", &[]);
        assert_eq!(s.name(), "f32");
        assert!(s.fields().is_empty());
    }

    #[test]
    fn composite_type_schema_lists_fields_in_order() {
        const FIELDS: &[FieldSchema] = &[
            FieldSchema::new("x", "f32"),
            FieldSchema::new("y", "f32"),
        ];
        let s = TypeSchema::new("Vec2", FIELDS);
        assert_eq!(s.name(), "Vec2");
        assert_eq!(s.fields().len(), 2);
        assert_eq!(s.fields()[0].name(), "x");
        assert_eq!(s.fields()[1].type_name(), "f32");
    }
}
