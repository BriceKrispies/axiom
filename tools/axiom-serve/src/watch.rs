//! Recursive mtime-polling watcher: snapshot, diff, debounce.
//!
//! Like `tools/axiom-dev-reload`'s single-file watcher, but recursive over a
//! set of roots with exclusions (build outputs — `web/pkg/`, `web/dist/` —
//! are excluded so a rebuild's own writes never re-trigger it). Polls every
//! 200ms; a change fires the callback only after the tree has been stable for
//! the 150ms debounce window, so a multi-file save triggers one rebuild.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/// How often the watcher polls the tree's modification times.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// How long the tree must stay stable after a change before rebuilding.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// What to watch: `roots` may be files or directories (walked recursively);
/// anything under an `exclude` prefix is ignored.
pub struct WatchSpec {
    pub roots: Vec<PathBuf>,
    pub exclude: Vec<PathBuf>,
}

/// A point-in-time view of the watched tree: every file's path → mtime.
/// (A `BTreeMap` so snapshots compare deterministically.)
pub type Snapshot = BTreeMap<PathBuf, SystemTime>;

/// Whether `path` falls under any excluded prefix.
pub fn is_excluded(path: &Path, exclude: &[PathBuf]) -> bool {
    exclude.iter().any(|prefix| path.starts_with(prefix))
}

/// Take a snapshot of every watched file's mtime. Missing roots are fine —
/// they simply contribute nothing (and are picked up once created).
pub fn snapshot(spec: &WatchSpec) -> Snapshot {
    let mut snap = Snapshot::new();
    for root in &spec.roots {
        collect(root, &spec.exclude, &mut snap);
    }
    snap
}

fn collect(path: &Path, exclude: &[PathBuf], snap: &mut Snapshot) {
    if is_excluded(path, exclude) {
        return;
    }
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect(&entry.path(), exclude, snap);
        }
    } else if let Ok(mtime) = meta.modified() {
        snap.insert(path.to_path_buf(), mtime);
    }
}

/// Poll forever, invoking `on_change` once per debounced burst of changes
/// (adds, removals, and mtime bumps all count).
pub fn run(spec: &WatchSpec, mut on_change: impl FnMut()) {
    let mut last = snapshot(spec);
    let mut dirty_since: Option<Instant> = None;
    loop {
        thread::sleep(POLL_INTERVAL);
        let now = snapshot(spec);
        if now != last {
            last = now;
            dirty_since = Some(Instant::now());
        } else if dirty_since.is_some_and(|since| since.elapsed() >= DEBOUNCE) {
            dirty_since = None;
            on_change();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "axiom-serve-watch-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn exclusion_is_prefix_based() {
        let exclude = vec![PathBuf::from("app/web/pkg")];
        assert!(is_excluded(Path::new("app/web/pkg/glue.js"), &exclude));
        assert!(is_excluded(Path::new("app/web/pkg"), &exclude));
        assert!(!is_excluded(Path::new("app/web/index.html"), &exclude));
        // Component-wise, not string-prefix: pkg-extra is a different dir.
        assert!(!is_excluded(Path::new("app/web/pkg-extra/x.js"), &exclude));
    }

    #[test]
    fn snapshot_walks_recursively_skips_excludes_and_diffs_on_change() {
        let root = scratch("snap");
        fs::create_dir_all(root.join("src").join("sub")).unwrap();
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(root.join("src").join("a.ts"), "a").unwrap();
        fs::write(root.join("src").join("sub").join("b.ts"), "b").unwrap();
        fs::write(root.join("pkg").join("out.js"), "out").unwrap();
        fs::write(root.join("top.html"), "hi").unwrap();

        let spec = WatchSpec {
            roots: vec![root.join("src"), root.join("top.html"), root.join("pkg")],
            exclude: vec![root.join("pkg")],
        };
        let before = snapshot(&spec);
        assert_eq!(before.len(), 3, "a.ts + sub/b.ts + top.html: {before:?}");
        assert!(!before.contains_key(&root.join("pkg").join("out.js")));

        // A new file changes the snapshot; excluded churn does not.
        fs::write(root.join("src").join("c.ts"), "c").unwrap();
        fs::write(root.join("pkg").join("out2.js"), "out2").unwrap();
        let after = snapshot(&spec);
        assert_ne!(before, after);
        assert_eq!(after.len(), 4);

        // A missing root contributes nothing rather than failing.
        let sparse = WatchSpec {
            roots: vec![root.join("no-such-dir")],
            exclude: vec![],
        };
        assert!(snapshot(&sparse).is_empty());

        fs::remove_dir_all(&root).ok();
    }
}
