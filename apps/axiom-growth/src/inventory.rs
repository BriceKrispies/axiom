//! Sim-owned session inventory. Audit: GW-E14/E15, SV-0.4. Scaffold (M1).
use std::collections::HashMap;

/// A stack of one item type. Audit: item_id + count.
#[derive(Debug, Clone, Copy)]
pub struct ItemStack {
    pub item: u32,
    pub count: u32,
}

/// Sim-authoritative inventory exposed to presentation. Audit: GW-E14.
#[derive(Debug, Default)]
pub struct Inventory {
    stacks: HashMap<u32, u32>,
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            stacks: HashMap::new(),
        }
    }
    pub fn add(&mut self, item: u32, count: u32) {
        *self.stacks.entry(item).or_insert(0) += count;
    }
    pub fn remove(&mut self, item: u32, count: u32) -> bool {
        match self.stacks.get_mut(&item) {
            Some(c) if *c >= count => {
                *c -= count;
                true
            }
            _ => false,
        }
    }
    pub fn count(&self, item: u32) -> u32 {
        self.stacks.get(&item).copied().unwrap_or(0)
    }
}
