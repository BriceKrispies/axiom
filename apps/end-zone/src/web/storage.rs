//! The sanctioned browser storage adapter: `web_sys::Storage` behind the
//! app-local [`ProfileStore`] trait (the ONLY place the app touches browser
//! storage), plus a console-backed kernel log sink for the edge.

use axiom_kernel::{LogRecord, LogSink};
use web_sys::Storage;

use crate::frontend::persistence::ProfileStore;

/// The versioned storage key.
const PROFILE_KEY: &str = "axiom-end-zone.profile";

fn local_storage() -> Option<Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// `localStorage`-backed profile store. Every operation degrades to a no-op
/// when storage is unavailable (private mode, disabled) — the frontend then
/// simply runs on defaults.
#[derive(Debug, Default)]
pub struct LocalStorageStore;

impl ProfileStore for LocalStorageStore {
    fn load(&self) -> Option<String> {
        local_storage()?.get_item(PROFILE_KEY).ok().flatten()
    }

    fn save(&mut self, profile: &str) -> bool {
        local_storage()
            .map(|s| s.set_item(PROFILE_KEY, profile).is_ok())
            .unwrap_or(false)
    }

    fn clear(&mut self) {
        if let Some(storage) = local_storage() {
            let _ = storage.remove_item(PROFILE_KEY);
        }
    }
}

/// Kernel log records forwarded to the browser console.
#[derive(Debug, Default)]
pub struct ConsoleSink;

impl LogSink for ConsoleSink {
    fn record(&mut self, record: LogRecord) {
        web_sys::console::log_1(&format!("{record:?}").into());
    }
}
