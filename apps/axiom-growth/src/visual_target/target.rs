//! A **visual target on disk**: the directory that holds one convergence target's
//! artifacts and the orchestration that drives an iteration over them.
//!
//! Layout of a target directory (e.g. `visual_targets/prologue_postcard_001/`):
//!
//! ```text
//! manifest.toml              the CHAMPION scene (source of champion.png)
//! manifest.candidate.toml    the CANDIDATE scene (one bounded edit of the champion)
//! reference.png              the target reference image (the goal)
//! champion.png               the current champion screenshot
//! candidate.png              the latest candidate screenshot
//! scorecard.champion.toml     the champion's twelve-axis scores
//! scorecard.candidate.toml    the candidate's twelve-axis scores
//! verdict.toml               optional human override for this iteration
//! ledger.toml                the append-only iteration ledger
//! diagnostics/               diff heatmaps + consumed verdicts, per iteration
//! abstractions/              justified abstraction records (0001.toml, …)
//! ```
//!
//! The comparator's *judgement* — assigning the twelve scores — is a human/agent
//! act, authored into the two scorecard files. Everything this module does with
//! those scores (deciding, promoting, ledgering) is deterministic.

use std::path::{Path, PathBuf};

use super::axes::{Axis, Scorecard};
use super::compare;
use super::ledger::{Ledger, LedgerEntry};
use super::review::{self, HumanVerdict, ResolvedDecision};

/// A convergence target rooted at a directory.
#[derive(Debug, Clone)]
pub struct Target {
    dir: PathBuf,
}

/// A read-only snapshot of a target's champion state.
#[derive(Debug, Clone)]
pub struct TargetStatus {
    pub champion: Scorecard,
    pub final_score: f32,
    pub complete: bool,
    pub next_axis: Option<Axis>,
    pub iterations: u32,
}

/// The result of reviewing one candidate.
#[derive(Debug, Clone)]
pub struct ReviewOutcome {
    pub attacked_axis: Axis,
    pub resolved: ResolvedDecision,
    pub promoted: bool,
    pub entry: LedgerEntry,
    pub diagnostics: Vec<String>,
}

impl Target {
    /// Root a target at `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Target {
        Target { dir: dir.into() }
    }

    // --- artifact paths -----------------------------------------------------

    pub fn champion_manifest(&self) -> PathBuf {
        self.dir.join("manifest.toml")
    }
    pub fn candidate_manifest(&self) -> PathBuf {
        self.dir.join("manifest.candidate.toml")
    }
    pub fn reference_png(&self) -> PathBuf {
        self.dir.join("reference.png")
    }
    pub fn champion_png(&self) -> PathBuf {
        self.dir.join("champion.png")
    }
    pub fn candidate_png(&self) -> PathBuf {
        self.dir.join("candidate.png")
    }
    pub fn champion_scorecard_path(&self) -> PathBuf {
        self.dir.join("scorecard.champion.toml")
    }
    pub fn candidate_scorecard_path(&self) -> PathBuf {
        self.dir.join("scorecard.candidate.toml")
    }
    pub fn verdict_path(&self) -> PathBuf {
        self.dir.join("verdict.toml")
    }
    pub fn ledger_path(&self) -> PathBuf {
        self.dir.join("ledger.toml")
    }
    pub fn diagnostics_dir(&self) -> PathBuf {
        self.dir.join("diagnostics")
    }
    pub fn abstractions_dir(&self) -> PathBuf {
        self.dir.join("abstractions")
    }

    // --- loads --------------------------------------------------------------

    pub fn champion_scorecard(&self) -> Result<Scorecard, String> {
        Scorecard::load(&self.champion_scorecard_path())
    }
    pub fn candidate_scorecard(&self) -> Result<Scorecard, String> {
        Scorecard::load(&self.candidate_scorecard_path())
    }
    pub fn verdict(&self) -> Result<Option<HumanVerdict>, String> {
        HumanVerdict::load_optional(&self.verdict_path())
    }
    pub fn ledger(&self) -> Result<Ledger, String> {
        Ledger::load_or_empty(&self.ledger_path())
    }

    /// The champion's current status: score, completion, and the next flaw to
    /// attack (respecting any human verdict).
    pub fn status(&self) -> Result<TargetStatus, String> {
        let champion = self.champion_scorecard()?;
        let verdict = self.verdict()?;
        let ledger = self.ledger()?;
        Ok(TargetStatus {
            final_score: champion.final_score(),
            complete: review::is_complete(&champion, verdict.as_ref()),
            next_axis: review::next_attacked_axis(&champion, verdict.as_ref()),
            iterations: ledger.entries.len() as u32,
            champion,
        })
    }

    /// Review the current candidate against the champion: decide, append a ledger
    /// entry, promote the candidate if it wins, and emit diagnostic diff renders.
    ///
    /// The attacked axis is the champion's current lowest axis (or a human
    /// override); reviewing a champion that is already complete is an error, since
    /// there is no flaw to attack.
    #[allow(clippy::obfuscated_if_else)] // branchless-style selection, moved as-is
    pub fn review(
        &self,
        changed_files: Vec<String>,
        abstraction_introduced: bool,
    ) -> Result<ReviewOutcome, String> {
        let champion = self.champion_scorecard()?;
        let candidate = self.candidate_scorecard()?;
        let verdict = self.verdict()?;
        let mut ledger = self.ledger()?;

        let attacked = review::next_attacked_axis(&champion, verdict.as_ref()).ok_or_else(|| {
            "champion is already complete (every axis >= 4, or human-accepted); no axis to attack"
                .to_string()
        })?;

        let machine = review::decide(&champion, &candidate, attacked);
        let resolved = review::resolve_decision(machine, verdict.as_ref());
        let machine_reason = review::reason(&champion, &candidate, attacked, resolved.effective);
        let human_note = verdict.as_ref().map(|v| v.note.clone()).unwrap_or_default();
        let reason = resolved
            .human_overrode
            .then(|| format!("{machine_reason} [human override: machine said {}]", resolved.machine))
            .unwrap_or(machine_reason);

        let promoted = resolved.effective.replaces_champion();
        // The champion that the *next* attacked axis is computed from is the
        // candidate iff we promoted, else the unchanged champion. The verdict's
        // `attacked_axis` override is one-shot (it steered THIS iteration and is
        // archived below), so the *next* flaw is the new champion's natural lowest —
        // only completion (all-pass or human accept) can override it to "none".
        let next_champion = promoted.then_some(candidate).unwrap_or(champion);
        let next_axis = (!review::is_complete(&next_champion, verdict.as_ref()))
            .then(|| next_champion.lowest_axis());

        let iteration = ledger.next_iteration();
        let diagnostics = self.write_diagnostics(iteration)?;

        let entry = LedgerEntry {
            iteration,
            attacked_axis: attacked,
            changed_files,
            champion_screenshot: file_name(&self.champion_png()),
            candidate_screenshot: file_name(&self.candidate_png()),
            decision: resolved.effective,
            reason,
            next_attacked_axis: next_axis,
            abstraction_introduced,
            human_note,
            scorecard_before: champion,
            scorecard_after: candidate,
        };
        ledger.append(entry.clone());
        ledger.save(&self.ledger_path())?;

        promoted.then(|| self.promote()).transpose()?;
        // Consume the one-shot verdict so it never silently re-applies next round.
        self.archive_verdict(iteration)?;

        Ok(ReviewOutcome { attacked_axis: attacked, resolved, promoted, entry, diagnostics })
    }

    /// Promote the candidate to champion: the candidate manifest, screenshot, and
    /// scorecard overwrite the champion's.
    fn promote(&self) -> Result<(), String> {
        copy_if_exists(&self.candidate_manifest(), &self.champion_manifest())?;
        copy_if_exists(&self.candidate_png(), &self.champion_png())?;
        copy_if_exists(&self.candidate_scorecard_path(), &self.champion_scorecard_path())?;
        Ok(())
    }

    /// Move a consumed `verdict.toml` into `diagnostics/verdict.iterNNNN.toml`, so a
    /// per-iteration override is preserved but not re-applied.
    fn archive_verdict(&self, iteration: u32) -> Result<(), String> {
        let path = self.verdict_path();
        path.exists()
            .then(|| {
                std::fs::create_dir_all(self.diagnostics_dir())
                    .map_err(|e| format!("create diagnostics dir: {e}"))?;
                let dst = self.diagnostics_dir().join(format!("verdict.iter{iteration:04}.toml"));
                std::fs::rename(&path, &dst).map_err(|e| format!("archive verdict: {e}"))
            })
            .transpose()
            .map(|_| ())
    }

    /// Emit diff-heatmap diagnostics for this iteration: candidate-vs-champion and
    /// (if a reference exists) candidate-vs-reference. Size mismatches are skipped,
    /// not fatal — a diagnostic must never fail a review.
    fn write_diagnostics(&self, iteration: u32) -> Result<Vec<String>, String> {
        std::fs::create_dir_all(self.diagnostics_dir())
            .map_err(|e| format!("create diagnostics dir: {e}"))?;
        let pairs = [
            (self.champion_png(), "candidate_vs_champion"),
            (self.reference_png(), "candidate_vs_reference"),
        ];
        let written = pairs
            .into_iter()
            .filter_map(|(base, label)| {
                self.try_diff(&self.candidate_png(), &base, iteration, label).transpose()
            })
            .collect::<Result<Vec<String>, String>>()?;
        Ok(written)
    }

    /// Write one diff heatmap, or `None` if it cannot: either input missing, either
    /// not a decodable RGBA8 PNG (e.g. an external RGB reference screenshot), or the
    /// two differ in size. A diagnostic must never fail a review, so only a genuine
    /// write error propagates — every "can't compare these" case is a graceful skip.
    fn try_diff(
        &self,
        a: &Path,
        b: &Path,
        iteration: u32,
        label: &str,
    ) -> Result<Option<String>, String> {
        (a.exists() & b.exists())
            .then(|| (decode(a).ok(), decode(b).ok()))
            .and_then(|(a, b)| a.zip(b))
            .filter(|((_, aw, ah), (_, bw, bh))| aw == bw && ah == bh)
            .map(|((ap, aw, ah), (bp, _, _))| {
                let heat = compare::diff_heatmap(&ap, &bp);
                let out = self.diagnostics_dir().join(format!("iter{iteration:04}_{label}.png"));
                write_png(&out, &heat, aw, ah).map(|()| file_name(&out))
            })
            .transpose()
    }
}

/// Decode a PNG into `(rgba, w, h)`.
fn decode(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    compare::decode_rgba_png(&bytes)
}

/// Encode RGBA8 pixels to a PNG (creating parent dirs).
fn write_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    path.parent()
        .map(|p| std::fs::create_dir_all(p).map_err(|e| format!("create {}: {e}", p.display())))
        .transpose()?;
    let file = std::fs::File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| format!("PNG header: {e}"))?;
    writer.write_image_data(rgba).map_err(|e| format!("PNG data: {e}"))
}

/// Copy `src` over `dst`, erroring if `src` is missing.
#[allow(clippy::obfuscated_if_else)] // branchless-style selection, moved as-is
fn copy_if_exists(src: &Path, dst: &Path) -> Result<(), String> {
    src.exists()
        .then(|| std::fs::copy(src, dst).map(|_| ()).map_err(|e| {
            format!("copy {} -> {}: {e}", src.display(), dst.display())
        }))
        .unwrap_or_else(|| Err(format!("cannot promote: {} is missing", src.display())))
}

/// The file name of a path as an owned `String` (for ledger paths).
fn file_name(path: &Path) -> String {
    path.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::review::Decision;

    /// A 2×2 solid-colour RGBA8 PNG written to `path`.
    fn write_solid(path: &Path, rgba: [u8; 4]) {
        let pixels: Vec<u8> = rgba.iter().copied().cycle().take(2 * 2 * 4).collect();
        write_png(path, &pixels, 2, 2).unwrap();
    }

    /// A 2×2 solid-colour **RGB** (not RGBA) PNG — models an external reference
    /// screenshot the RGBA8 diff decoder cannot read.
    fn write_solid_rgb(path: &Path, rgb: [u8; 3]) {
        let pixels: Vec<u8> = rgb.iter().copied().cycle().take(2 * 2 * 3).collect();
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 2, 2);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels).unwrap();
    }

    /// A target dir seeded with champion + candidate scorecards and screenshots.
    fn seed(name: &str, champion: Scorecard, candidate: Scorecard) -> Target {
        let dir = std::env::temp_dir().join("axiom_vt_target_test").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let t = Target::new(&dir);
        std::fs::write(t.champion_scorecard_path(), champion.to_toml()).unwrap();
        std::fs::write(t.candidate_scorecard_path(), candidate.to_toml()).unwrap();
        std::fs::write(t.champion_manifest(), "champion").unwrap();
        std::fs::write(t.candidate_manifest(), "candidate").unwrap();
        write_solid(&t.champion_png(), [10, 10, 10, 255]);
        write_solid(&t.candidate_png(), [20, 20, 20, 255]);
        write_solid(&t.reference_png(), [30, 30, 30, 255]);
        t
    }

    #[test]
    fn status_reports_lowest_axis_and_score() {
        let mut champ = Scorecard::uniform(3);
        champ.set(Axis::FogAndHaze, 1);
        let t = seed("status", champ, champ);
        let s = t.status().unwrap();
        assert_eq!(s.next_axis, Some(Axis::FogAndHaze));
        assert!(!s.complete);
        assert_eq!(s.iterations, 0);
        assert!((s.final_score - champ.final_score()).abs() < 1e-6);
    }

    #[test]
    fn review_promotes_a_clean_win_and_updates_champion() {
        let mut champ = Scorecard::uniform(3);
        champ.set(Axis::FogAndHaze, 1);
        let mut cand = champ;
        cand.set(Axis::FogAndHaze, 3); // attacked axis improves, nothing drops
        let t = seed("promote", champ, cand);
        let out = t.review(vec!["manifest.candidate.toml".into()], false).unwrap();
        assert_eq!(out.attacked_axis, Axis::FogAndHaze);
        assert_eq!(out.resolved.effective, Decision::KeepCandidate);
        assert!(out.promoted);
        // Champion scorecard + png were overwritten by the candidate's.
        assert_eq!(t.champion_scorecard().unwrap(), cand);
        assert_eq!(std::fs::read(t.champion_png()).unwrap(), std::fs::read(t.candidate_png()).unwrap());
        // Ledger has one full entry, and diagnostics were emitted.
        let ledger = t.ledger().unwrap();
        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.entries[0].scorecard_after, cand);
        assert!(!out.diagnostics.is_empty());
    }

    #[test]
    fn review_keeps_champion_on_significant_regression() {
        let mut champ = Scorecard::uniform(3);
        champ.set(Axis::FogAndHaze, 1);
        let mut cand = champ;
        cand.set(Axis::FogAndHaze, 4); // attacked up
        cand.set(Axis::ColorPalette, 1); // non-attacked down by 2 (significant)
        let t = seed("reject", champ, cand);
        let out = t.review(vec![], false).unwrap();
        assert_eq!(out.resolved.effective, Decision::RejectCandidate);
        assert!(!out.promoted);
        assert_eq!(t.champion_scorecard().unwrap(), champ); // unchanged
    }

    #[test]
    fn human_verdict_overrides_and_is_archived() {
        let mut champ = Scorecard::uniform(3);
        champ.set(Axis::FogAndHaze, 1);
        let mut cand = champ;
        cand.set(Axis::FogAndHaze, 4);
        cand.set(Axis::ColorPalette, 1); // machine would reject
        let t = seed("override", champ, cand);
        std::fs::write(
            t.verdict_path(),
            "decision = \"keep_candidate\"\nnote = \"the palette dip is fine\"\n",
        )
        .unwrap();
        let out = t.review(vec![], false).unwrap();
        assert!(out.resolved.human_overrode);
        assert_eq!(out.resolved.effective, Decision::KeepCandidate);
        assert!(out.promoted);
        assert!(out.entry.reason.contains("human override"));
        assert_eq!(out.entry.human_note, "the palette dip is fine");
        // verdict.toml consumed → archived under diagnostics, gone from root.
        assert!(!t.verdict_path().exists());
        assert!(t.diagnostics_dir().join("verdict.iter0001.toml").exists());
    }

    #[test]
    fn one_shot_axis_override_does_not_leak_into_next_flaw() {
        // Champion's natural lowest is fog_and_haze; a verdict redirects THIS
        // iteration to color_palette. After promoting, the NEXT flaw must be the new
        // champion's natural lowest (fog_and_haze), not the consumed override.
        let mut champ = Scorecard::uniform(3);
        champ.set(Axis::FogAndHaze, 1);
        let mut cand = champ;
        cand.set(Axis::ColorPalette, 4); // improves the overridden axis
        let t = seed("override_no_leak", champ, cand);
        std::fs::write(t.verdict_path(), "attacked_axis = \"color_palette\"\n").unwrap();
        let out = t.review(vec![], false).unwrap();
        assert_eq!(out.attacked_axis, Axis::ColorPalette); // this iteration
        assert!(out.promoted);
        assert_eq!(out.entry.next_attacked_axis, Some(Axis::FogAndHaze)); // not color_palette
    }

    #[test]
    fn review_refuses_a_complete_champion() {
        let champ = Scorecard::uniform(5);
        let t = seed("complete", champ, champ);
        assert!(t.review(vec![], false).unwrap_err().contains("complete"));
    }

    #[test]
    fn undecodable_reference_is_skipped_not_fatal() {
        // An external RGB reference the RGBA8 diff decoder can't read must not fail
        // the review — the candidate-vs-champion diagnostic still renders, the
        // candidate-vs-reference one is silently skipped.
        let mut champ = Scorecard::uniform(3);
        champ.set(Axis::FogAndHaze, 1);
        let mut cand = champ;
        cand.set(Axis::FogAndHaze, 3);
        let t = seed("rgb_ref", champ, cand);
        write_solid_rgb(&t.reference_png(), [30, 30, 30]); // overwrite RGBA ref with RGB
        let out = t.review(vec![], false).unwrap();
        assert!(out.promoted);
        assert!(out.diagnostics.iter().any(|d| d.contains("candidate_vs_champion")));
        assert!(!out.diagnostics.iter().any(|d| d.contains("candidate_vs_reference")));
    }
}
