//! The deterministic asset-load scheduler and state machine over a [`Manifest`].
//!
//! Each tick, [`AssetCatalog::advance`] folds in the completions that arrived
//! since the last tick, then chooses the next loads to dispatch: only assets
//! whose dependencies are all `ready` are eligible, ordered by priority (then id
//! for a total, stable order), capped so at most `max_in_flight` loads are in
//! flight. Identical inputs always produce an identical schedule.

use std::collections::BTreeMap;

use axiom_kernel::AssetId;

use crate::asset_entry::AssetEntry;
use crate::asset_state::{AssetState, CompletionOutcome};
use crate::manifest::{build_index, Manifest};

#[derive(Debug)]
pub(crate) struct AssetCatalog {
    manifest: Manifest,
    index: BTreeMap<AssetId, usize>,
    states: BTreeMap<AssetId, AssetState>,
    ready_inbox: Vec<AssetId>,
    max_in_flight: usize,
}

impl AssetCatalog {
    pub(crate) fn new(manifest: Manifest, max_in_flight: usize) -> AssetCatalog {
        let index = build_index(manifest.entries());
        let states = manifest
            .entries()
            .iter()
            .map(|entry| (entry.id, AssetState::Unrequested))
            .collect();
        AssetCatalog {
            manifest,
            index,
            states,
            ready_inbox: Vec::new(),
            max_in_flight,
        }
    }

    /// Apply completions, then return the new `(id, locator)` loads to dispatch.
    pub(crate) fn advance(
        &mut self,
        completed_ok: &[AssetId],
        completed_failed: &[AssetId],
    ) -> Vec<(AssetId, String)> {
        completed_ok
            .iter()
            .for_each(|id| self.apply(*id, CompletionOutcome::Success));
        completed_failed
            .iter()
            .for_each(|id| self.apply(*id, CompletionOutcome::Failure));
        let slots = self
            .max_in_flight
            .saturating_sub(self.count(AssetState::Requested));
        let mut eligible = self.eligible();
        eligible.sort_by_key(|(priority, id, _)| (core::cmp::Reverse(*priority), *id));
        eligible
            .into_iter()
            .take(slots)
            .map(|(_, id, locator)| {
                self.set_state(id, AssetState::Requested);
                (id, locator)
            })
            .collect()
    }

    /// Drain the ids that became ready since the last drain.
    pub(crate) fn take_ready(&mut self) -> Vec<AssetId> {
        std::mem::take(&mut self.ready_inbox)
    }

    pub(crate) fn is_ready(&self, id: AssetId) -> bool {
        self.state_of(id) == AssetState::Ready
    }

    /// Whether `id` and all its direct dependencies are ready (usable).
    pub(crate) fn is_usable(&self, id: AssetId) -> bool {
        self.entry(id)
            .map(|entry| {
                self.is_ready(id) & entry.dependencies.iter().all(|dep| self.is_ready(*dep))
            })
            .unwrap_or(false)
    }

    pub(crate) fn state_code(&self, id: AssetId) -> u8 {
        self.state_of(id) as u8
    }

    pub(crate) fn locator(&self, id: AssetId) -> Option<String> {
        self.entry(id)
            .map(|entry| String::from_utf8_lossy(&entry.locator).into_owned())
    }

    pub(crate) fn kind(&self, id: AssetId) -> Option<u32> {
        self.entry(id).map(|entry| entry.kind)
    }

    pub(crate) fn dependencies_of(&self, id: AssetId) -> Vec<AssetId> {
        self.entry(id)
            .map(|entry| entry.dependencies.clone())
            .unwrap_or_default()
    }

    pub(crate) fn asset_ids(&self) -> Vec<AssetId> {
        self.manifest
            .entries()
            .iter()
            .map(|entry| entry.id)
            .collect()
    }

    pub(crate) fn total_count(&self) -> usize {
        self.manifest.entries().len()
    }

    pub(crate) fn ready_count(&self) -> usize {
        self.count(AssetState::Ready)
    }

    pub(crate) fn failed_count(&self) -> usize {
        self.count(AssetState::Failed)
    }

    pub(crate) fn in_flight_count(&self) -> usize {
        self.count(AssetState::Requested)
    }

    /// The `(priority, id, locator)` of every asset that may be dispatched now:
    /// still unrequested and with every dependency already ready.
    fn eligible(&self) -> Vec<(u32, AssetId, String)> {
        self.manifest
            .entries()
            .iter()
            .filter(|entry| self.state_of(entry.id) == AssetState::Unrequested)
            .filter(|entry| {
                entry
                    .dependencies
                    .iter()
                    .all(|dep| self.state_of(*dep) == AssetState::Ready)
            })
            .map(|entry| {
                (
                    entry.priority,
                    entry.id,
                    String::from_utf8_lossy(&entry.locator).into_owned(),
                )
            })
            .collect()
    }

    fn apply(&mut self, id: AssetId, outcome: CompletionOutcome) {
        let known = self.index.contains_key(&id);
        let previous = self.state_of(id);
        let next = previous.on_completion(outcome);
        let became_ready = known & (previous != AssetState::Ready) & (next == AssetState::Ready);
        known.then(|| self.states.insert(id, next));
        became_ready.then(|| self.ready_inbox.push(id));
    }

    fn set_state(&mut self, id: AssetId, state: AssetState) {
        self.states.insert(id, state);
    }

    fn state_of(&self, id: AssetId) -> AssetState {
        self.states
            .get(&id)
            .copied()
            .unwrap_or(AssetState::Unrequested)
    }

    fn count(&self, state: AssetState) -> usize {
        self.states
            .values()
            .filter(|value| **value == state)
            .count()
    }

    fn entry(&self, id: AssetId) -> Option<&AssetEntry> {
        self.index
            .get(&id)
            .and_then(|position| self.manifest.entries().get(*position))
    }
}
