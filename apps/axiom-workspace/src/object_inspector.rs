//! [`ObjectInspectorState`] — the typed placeholder state of the Object Inspector
//! panel: an optionally-selected entity and its ordered field rows.
//!
//! Pure value data — the panel simulates nothing. The selected entity is named by
//! the kernel's [`EntityId`]; the field rows are placeholder key/value strings
//! until a future integration reflects a real entity's components.

use axiom_kernel::EntityId;

/// One placeholder field row in the Object Inspector: a name and a value, both
/// rendered as strings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InspectorField {
    /// The field name.
    pub name: String,
    /// The field value, rendered as a placeholder string.
    pub value: String,
}

impl InspectorField {
    /// Build a placeholder inspector field.
    #[must_use]
    pub fn new(name: &str, value: &str) -> Self {
        InspectorField {
            name: name.to_string(),
            value: value.to_string(),
        }
    }
}

/// The Object Inspector panel state: an optional selected entity plus its ordered
/// placeholder field rows. `Default` is empty (no selection, no fields).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjectInspectorState {
    selected: Option<EntityId>,
    fields: Vec<InspectorField>,
}

impl ObjectInspectorState {
    /// The selected entity, if any.
    #[must_use]
    pub fn selected(&self) -> Option<EntityId> {
        self.selected
    }

    /// Set the selected entity.
    pub fn select(&mut self, selected: Option<EntityId>) {
        self.selected = selected;
    }

    /// Append a field row, preserving insertion order exactly.
    pub fn add_field(&mut self, field: InspectorField) {
        self.fields.push(field);
    }

    /// The field rows, in insertion order.
    #[must_use]
    pub fn fields(&self) -> &[InspectorField] {
        &self.fields
    }
}
