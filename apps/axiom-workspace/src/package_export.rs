//! [`PackageExportState`] — the typed placeholder state of the Package Export
//! panel: a request/status **contract** for exporting a package.
//!
//! Placeholder — no real packaging is performed. This panel only records the
//! *intent* to export (a target string and a requested flag) and a coarse status.
//! A future integration wires a real packaging pipeline behind this contract.

/// The coarse status of a package export request. A contract vocabulary; the
/// panel never performs real work, so it only ever advances to `Requested` here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackageStatus {
    /// No export has been requested.
    #[default]
    Idle,
    /// An export has been requested (the only transition this placeholder makes).
    Requested,
    /// An export is in progress (set only by a future real pipeline).
    InProgress,
    /// An export completed (set only by a future real pipeline).
    Done,
    /// An export failed (set only by a future real pipeline).
    Failed,
}

/// The Package Export panel state: a request/status contract only. `Default` is
/// idle with no target and no request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageExportState {
    status: PackageStatus,
    target: String,
    requested: bool,
}

impl PackageExportState {
    /// The current export status.
    #[must_use]
    pub fn status(&self) -> PackageStatus {
        self.status
    }

    /// The requested export target string.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Whether an export has been requested.
    #[must_use]
    pub fn requested(&self) -> bool {
        self.requested
    }

    /// Record an export request. Placeholder — no real packaging is performed:
    /// this only marks `requested`, moves the status to
    /// [`PackageStatus::Requested`], and stores the target.
    pub fn request(&mut self, target: &str) {
        self.requested = true;
        self.status = PackageStatus::Requested;
        self.target = target.to_string();
    }
}
