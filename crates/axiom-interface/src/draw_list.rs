//! Neutral interface draw descriptions — the renderer-agnostic output of the
//! interface layer.
//!
//! A panel renders as a deterministic, ordered sequence: its background rect, a
//! header, zero-or-more label/value rows, zero-or-more console result lines, and
//! the console input marker. A platform backend (DOM, canvas, native) turns these
//! into pixels; this layer never touches a renderer.

/// One neutral draw instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceDrawItem {
    /// The panel background at an integer position + size.
    Panel {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    /// The header / title bar: a primary (left) and secondary (right) label.
    Header { primary: String, secondary: String },
    /// A label/value read-out row.
    Row { label: String, value: String },
    /// A clickable action button. `action` is a consumer-defined id: this layer
    /// only stores and emits the button; a platform backend renders it and routes
    /// a click back to the consumer, which maps the id to a meaning. The layer
    /// stays neutral about behaviour.
    Button { action: u32, label: String },
    /// A console result line (`ok` selects success vs error styling).
    ConsoleLine { ok: bool, text: String },
    /// The console input marker: a prompt and whether it currently has focus.
    ConsoleInput { prompt: String, focused: bool },
}

/// An ordered list of draw instructions for one repaint.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InterfaceDrawList {
    items: Vec<InterfaceDrawItem>,
}

impl InterfaceDrawList {
    pub(crate) fn new(items: Vec<InterfaceDrawItem>) -> Self {
        InterfaceDrawList { items }
    }

    /// The draw instructions, in deterministic render order.
    pub fn items(&self) -> &[InterfaceDrawItem] {
        &self.items
    }

    /// How many instructions the list holds.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the list is empty (the panel is hidden).
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_exposes_its_items_in_order() {
        let list = InterfaceDrawList::new(vec![
            InterfaceDrawItem::Panel {
                x: 8,
                y: 8,
                width: 360,
                height: 0,
            },
            InterfaceDrawItem::Header {
                primary: "T".to_string(),
                secondary: "S".to_string(),
            },
        ]);
        assert_eq!(list.len(), 2);
        assert!(!list.is_empty());
        assert_eq!(
            list.items()[0],
            InterfaceDrawItem::Panel {
                x: 8,
                y: 8,
                width: 360,
                height: 0
            }
        );
    }

    #[test]
    fn default_list_is_empty() {
        let list = InterfaceDrawList::default();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert!(list.items().is_empty());
    }

    #[test]
    fn every_item_kind_clones_compares_and_debugs() {
        let items = vec![
            InterfaceDrawItem::Panel {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            },
            InterfaceDrawItem::Header {
                primary: "p".to_string(),
                secondary: "s".to_string(),
            },
            InterfaceDrawItem::Row {
                label: "l".to_string(),
                value: "v".to_string(),
            },
            InterfaceDrawItem::Button {
                action: 7,
                label: "go".to_string(),
            },
            InterfaceDrawItem::ConsoleLine {
                ok: true,
                text: "t".to_string(),
            },
            InterfaceDrawItem::ConsoleInput {
                prompt: ">".to_string(),
                focused: false,
            },
        ];
        items.iter().for_each(|item| {
            assert_eq!(item.clone(), *item);
            assert!(!format!("{item:?}").is_empty());
        });
        // A cross-variant comparison exercises the discriminant-mismatch path.
        assert_ne!(items[0], items[1]);
        let list = InterfaceDrawList::new(items);
        assert_eq!(list.clone(), list);
        assert!(!format!("{list:?}").is_empty());
    }
}
