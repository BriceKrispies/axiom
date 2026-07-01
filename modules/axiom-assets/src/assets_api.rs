//! [`AssetsApi`] — the module's single public facade: the deterministic asset
//! streaming brain. It parses an Axiom-native binary manifest and drives the
//! load-state machine + scheduler from per-frame completions. It owns no I/O —
//! the app performs the (async, parallel) fetches the scheduler asks for and
//! feeds completions back — so the engine handles all of the scheduling,
//! dependency ordering, and state, and the nondeterministic timing enters as
//! explicit data (the completions), keeping a streaming session replayable.

use axiom_kernel::{AssetId, KernelResult};

use crate::asset_catalog::AssetCatalog;
use crate::manifest::{EntryTuple, Manifest};

/// The deterministic, I/O-free asset streaming facade.
#[derive(Debug)]
pub struct AssetsApi {
    catalog: AssetCatalog,
}

impl AssetsApi {
    /// Parse a canonical binary manifest and build a streaming catalog that
    /// keeps at most `max_in_flight` loads dispatched at once. A `max_in_flight`
    /// of `0` pauses streaming (nothing is dispatched until it is rebuilt).
    pub fn from_manifest_bytes(bytes: &[u8], max_in_flight: usize) -> KernelResult<AssetsApi> {
        Manifest::read(bytes).map(|manifest| AssetsApi {
            catalog: AssetCatalog::new(manifest, max_in_flight),
        })
    }

    /// Encode a manifest to canonical bytes from `(id, kind, priority,
    /// size_hint, content_hash, locator, dependencies)` tuples — the inverse of
    /// [`Self::from_manifest_bytes`], for an authoring tool to emit.
    pub fn encode_manifest(entries: &[EntryTuple]) -> Vec<u8> {
        Manifest::encode(entries)
    }

    /// Advance one streaming tick: record the loads that completed since the last
    /// call (`completed_ok` / `completed_failed`), then return the NEW loads to
    /// dispatch now — `(asset id, locator)` pairs the app should fetch — chosen
    /// by priority and dependency order within the concurrency budget.
    pub fn advance(
        &mut self,
        completed_ok: &[AssetId],
        completed_failed: &[AssetId],
    ) -> Vec<(AssetId, String)> {
        self.catalog.advance(completed_ok, completed_failed)
    }

    /// Drain the assets that became ready since the last drain (the app decodes
    /// their bytes and registers them, e.g. into `axiom-resources`).
    pub fn take_ready(&mut self) -> Vec<AssetId> {
        self.catalog.take_ready()
    }

    /// Whether `id`'s own bytes have loaded.
    pub fn is_ready(&self, id: AssetId) -> bool {
        self.catalog.is_ready(id)
    }

    /// Whether `id` and all of its direct dependencies are ready (usable).
    pub fn is_usable(&self, id: AssetId) -> bool {
        self.catalog.is_usable(id)
    }

    /// `id`'s load state as a stable code: `0` unrequested, `1` in-flight, `2`
    /// ready, `3` failed. An unknown id reads as `0`.
    pub fn state_code(&self, id: AssetId) -> u8 {
        self.catalog.state_code(id)
    }

    /// `id`'s opaque locator (what the app fetches), or `None` if unknown.
    pub fn locator(&self, id: AssetId) -> Option<String> {
        self.catalog.locator(id)
    }

    /// `id`'s app-defined kind tag, or `None` if unknown.
    pub fn kind(&self, id: AssetId) -> Option<u32> {
        self.catalog.kind(id)
    }

    /// `id`'s declared dependencies (its outgoing edges in the dependency DAG).
    pub fn dependencies_of(&self, id: AssetId) -> Vec<AssetId> {
        self.catalog.dependencies_of(id)
    }

    /// Every asset id in the manifest, in authored order.
    pub fn asset_ids(&self) -> Vec<AssetId> {
        self.catalog.asset_ids()
    }

    /// Total number of assets in the manifest.
    pub fn total_count(&self) -> usize {
        self.catalog.total_count()
    }

    /// How many assets are ready.
    pub fn ready_count(&self) -> usize {
        self.catalog.ready_count()
    }

    /// How many loads have failed.
    pub fn failed_count(&self) -> usize {
        self.catalog.failed_count()
    }

    /// How many loads are currently dispatched (in flight).
    pub fn in_flight_count(&self) -> usize {
        self.catalog.in_flight_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: AssetId = AssetId::from_raw(1);
    const B: AssetId = AssetId::from_raw(2);
    const C: AssetId = AssetId::from_raw(3);
    const D: AssetId = AssetId::from_raw(4);

    /// A diamond manifest: A and B are roots; C depends on A; D depends on B & C.
    fn diamond(max_in_flight: usize) -> AssetsApi {
        let bytes = AssetsApi::encode_manifest(&[
            (A, 10, 10, 64, 0xA, "a.bin", &[]),
            (B, 10, 20, 64, 0xB, "b.bin", &[]),
            (C, 11, 5, 64, 0xC, "c.bin", &[A]),
            (D, 12, 30, 64, 0xD, "d.bin", &[B, C]),
        ]);
        AssetsApi::from_manifest_bytes(&bytes, max_in_flight).unwrap()
    }

    #[test]
    fn manifest_metadata_is_queryable() {
        let api = diamond(2);
        assert_eq!(api.total_count(), 4);
        assert_eq!(api.asset_ids(), vec![A, B, C, D]);
        assert_eq!(api.kind(C), Some(11));
        assert_eq!(api.locator(D), Some("d.bin".to_string()));
        assert_eq!(api.dependencies_of(D), vec![B, C]);
        let unknown = AssetId::from_raw(999);
        assert_eq!(api.kind(unknown), None);
        assert_eq!(api.locator(unknown), None);
        assert!(api.dependencies_of(unknown).is_empty());
        assert_eq!(api.state_code(unknown), 0);
    }

    #[test]
    fn scheduler_dispatches_roots_by_priority_within_budget() {
        let mut api = diamond(2);
        let requests = api.advance(&[], &[]);
        assert_eq!(
            requests,
            vec![(B, "b.bin".to_string()), (A, "a.bin".to_string())]
        );
        assert_eq!(api.in_flight_count(), 2);
        assert_eq!(api.state_code(A), 1);
        assert_eq!(api.state_code(B), 1);
        assert!(api.advance(&[], &[]).is_empty());
    }

    #[test]
    fn dependencies_gate_then_release_in_order() {
        let mut api = diamond(2);
        api.advance(&[], &[]);
        let after_a = api.advance(&[A], &[]);
        assert_eq!(after_a, vec![(C, "c.bin".to_string())]);
        assert_eq!(api.take_ready(), vec![A]);
        assert!(api.is_ready(A));
        let after_bc = api.advance(&[B, C], &[]);
        assert_eq!(after_bc, vec![(D, "d.bin".to_string())]);
        assert_eq!(api.take_ready(), vec![B, C]);
        assert!(api.advance(&[D], &[]).is_empty());
        assert_eq!(api.take_ready(), vec![D]);
        assert_eq!(api.ready_count(), 4);
        assert!(api.is_usable(D));
    }

    #[test]
    fn a_failed_dependency_blocks_its_dependents_forever() {
        let mut api = diamond(4);
        api.advance(&[], &[]);
        let next = api.advance(&[], &[A]);
        assert_eq!(api.failed_count(), 1);
        assert_eq!(api.state_code(A), 3);
        assert!(next.is_empty());
        assert!(!api.is_ready(C));
        assert!(!api.is_usable(C));
    }

    #[test]
    fn stray_and_duplicate_completions_are_no_ops() {
        let mut api = diamond(2);
        api.advance(&[], &[]);
        api.advance(&[A], &[]);
        let ready_first = api.take_ready();
        assert_eq!(ready_first, vec![A]);
        api.advance(&[A], &[]);
        assert!(api.take_ready().is_empty());
        assert!(api.is_ready(A));
        let unknown = AssetId::from_raw(777);
        api.advance(&[unknown], &[]);
        assert_eq!(api.state_code(unknown), 0);
        assert!(api.take_ready().is_empty());
    }

    #[test]
    fn a_zero_budget_pauses_streaming() {
        let mut api = diamond(0);
        assert!(api.advance(&[], &[]).is_empty());
        assert_eq!(api.in_flight_count(), 0);
    }

    #[test]
    fn is_usable_is_false_until_dependencies_are_ready() {
        let mut api = diamond(4);
        api.advance(&[], &[]);
        api.advance(&[A], &[]);
        api.advance(&[C], &[]);
        assert!(api.is_usable(C));
        assert!(!api.is_usable(D));
    }

    #[test]
    fn the_schedule_is_deterministic_across_runs() {
        let first = diamond(2).advance(&[], &[]);
        let second = diamond(2).advance(&[], &[]);
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_a_malformed_manifest() {
        assert!(AssetsApi::from_manifest_bytes(&[], 4).is_err());
    }
}
