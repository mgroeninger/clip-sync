//! Loader for the curated per-gap-**type** fingerprint fixtures (Phase 1 of the gap-fixture-corpus plan).
//!
//! Reads `crates/clip-sync-repair/tests/gap_corpus/fingerprints/curated/manifest.json` plus each listed
//! single-gap [`GapCorpus`] JSON, and exposes typed `(cell_type, corpus, expected, provenance)` records —
//! the media-free input the per-type assertion test (Phase 2) and any harness consumes to get a **named gap
//! cell** without touching the ephemeral `gap-files/` corpus.
//!
//! Licensing-safe: the fixtures are the non-identifying fingerprint projection (hashed source ids, numbers
//! only — no samples, no titles/paths). Gap cells: `docs/dev/gap-vocabulary.md`; fixture-library format:
//! `docs/dev/gap-fingerprint.md` § Source identity.

use std::path::{Path, PathBuf};

use clip_sync_repair::application::gap_fingerprint::{GapCorpus, GapFingerprint};
use serde::Deserialize;

/// The gap **cell** a fixture represents — a `docs/dev/gap-vocabulary.md` cell crossed with the classifier the
/// analyzer emits (`GapEquivalenceClass` / `GapPatchSkipReason` / `LagVerdict` / `dualfit_target()`).
///
/// The enum is the complete taxonomy. Not every cell has a curated fixture: `Decorrelated` is a hand-built
/// synthetic; `ResidualVeto` and `Unfillable` are **not fingerprint-representable** (the dump never emits
/// them as characterized skips) and are covered by other tests — see their variant docs and
/// `is_fingerprint_representable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapCellType {
    /// Bracket search found a lag-0 placement; both seams pass, donor occupied. Normal patch path.
    BracketPatchClean,
    /// Patched even though donor is broken — donor state must **not** gate a bracket-cleared patch.
    BracketPatchDonorBroken,
    /// Bracket-exhausted; shoulders register at their own lag, donor bridges → dual-fit addressable.
    SilenceSpliceDualfitTarget,
    /// High seam correlation yet dead donor → permanent skip, **not** a dual-fit target.
    ProgramQuiet,
    /// Structure/anchor search found no candidate; never reached seam scoring.
    NoPlacement,
    /// Scan-time equivalence: A died ∧ B occupied → keep (proceeds into the seam cells).
    RepairableDropout,
    /// Scan-time equivalence: B silent at nominal → plan-time drop.
    SharedSilence,
    /// Scan-time equivalence: A is room tone (not a dropout) → drop, decided on A's character.
    AmbientQuiet,
    /// Synthetic fixture: donor occupied but seams recover at no lag on any peak layer — B has different
    /// content; verdict=decorrelated, gate does not pass ⇒ not a dual-fit target.
    Decorrelated,
    /// **Not fingerprint-representable** — taxonomy entry only, no curated fixture. The dump sets
    /// `outcome.tier` from seam scoring (`any_ok`), never from residual gating, so a residual veto (seams
    /// pass, but least-squares cancellation shows B ≠ A) never appears as a characterized skip. The gate
    /// *decision* is tested at score/region level (`validate_residual_gate` F4 cases, `seam_residual_oracle`);
    /// the end-to-end pipeline veto is the optional/unproved **C1b** item (`tests/residual_gate_catalog/`).
    ResidualVeto,
    /// Real fixture: length-mismatch tail (`duration ≥ tail cutoff`), filtered before per-gap scoring —
    /// `GapKind::Tail`, excluded from the matched denominator.
    TailGeometryMismatch,
    /// **Not fingerprint-representable** — taxonomy entry only, no curated fixture. Unfillable gaps
    /// (`BExtractFailed` / `AlignedSegmentOutOfRange` / `ZeroLengthGap`) fail at plan/execution time and
    /// never get characterized; covered by `GapPatchSkipReason` unit tests and the pipeline instead.
    Unfillable,
}

impl GapCellType {
    /// Every cell in the taxonomy (the source of truth for "is a representable cell missing a fixture?").
    pub const ALL: &'static [GapCellType] = &[
        GapCellType::BracketPatchClean,
        GapCellType::BracketPatchDonorBroken,
        GapCellType::SilenceSpliceDualfitTarget,
        GapCellType::ProgramQuiet,
        GapCellType::NoPlacement,
        GapCellType::RepairableDropout,
        GapCellType::SharedSilence,
        GapCellType::AmbientQuiet,
        GapCellType::Decorrelated,
        GapCellType::ResidualVeto,
        GapCellType::TailGeometryMismatch,
        GapCellType::Unfillable,
    ];

    /// Whether this cell can appear as a characterized fingerprint — i.e. it must have a curated fixture.
    /// `ResidualVeto` and `Unfillable` are dispositions the dump never emits as a characterized skip (see
    /// their variant docs), so they are covered by other tests, not a fixture.
    pub fn is_fingerprint_representable(self) -> bool {
        !matches!(self, GapCellType::ResidualVeto | GapCellType::Unfillable)
    }

    /// The snake_case identifier, matching the `type` field in `manifest.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            GapCellType::BracketPatchClean => "bracket_patch_clean",
            GapCellType::BracketPatchDonorBroken => "bracket_patch_donor_broken",
            GapCellType::SilenceSpliceDualfitTarget => "silence_splice_dualfit_target",
            GapCellType::ProgramQuiet => "program_quiet",
            GapCellType::NoPlacement => "no_placement",
            GapCellType::RepairableDropout => "repairable_dropout",
            GapCellType::SharedSilence => "shared_silence",
            GapCellType::AmbientQuiet => "ambient_quiet",
            GapCellType::Decorrelated => "decorrelated",
            GapCellType::ResidualVeto => "residual_veto",
            GapCellType::TailGeometryMismatch => "tail_geometry_mismatch",
            GapCellType::Unfillable => "unfillable",
        }
    }
}

/// Where a fixture was harvested from — recorded because the source `gap-files/` corpus is gitignored and
/// ephemeral (derived from licensed media), so it may be deleted at any time.
#[derive(Debug, Clone, Deserialize)]
pub struct FixtureProvenance {
    /// Source corpus directory name under `gap-files/` (e.g. `re-anchor-dual-fit-on-nominal`).
    pub corpus: String,
    /// Pair subdir within that corpus.
    pub pair: String,
    /// The gap's index within its pair.
    pub gap_index: usize,
    /// The original per-gap dump filename it was copied from.
    pub original_filename: String,
}

/// One manifest entry (before the corpus JSON it names is loaded).
#[derive(Debug, Clone, Deserialize)]
struct ManifestEntry {
    file: String,
    #[serde(rename = "type")]
    cell_type: GapCellType,
    expected: String,
    provenance: FixtureProvenance,
}

/// `manifest.json`'s shape. The human `note` field is ignored (serde skips unknown fields).
#[derive(Debug, Deserialize)]
struct Manifest {
    fixtures: Vec<ManifestEntry>,
}

/// A curated fixture: its declared cell type, the loaded single-gap corpus, the human-readable expectation,
/// and its provenance.
#[derive(Debug, Clone)]
pub struct GapCellFixture {
    pub cell_type: GapCellType,
    /// The fixture's filename under the curated dir.
    pub file: String,
    /// Human-readable statement of what this cell asserts (from the manifest).
    pub expected: String,
    pub provenance: FixtureProvenance,
    /// The loaded single-gap corpus.
    pub corpus: GapCorpus,
}

impl GapCellFixture {
    /// The fixture's one gap. (Every curated fixture is a single-gap corpus — enforced on load.)
    pub fn gap(&self) -> &GapFingerprint {
        &self.corpus.gaps[0]
    }
}

/// Absolute path to the committed curated fixtures directory, resolved from this crate's manifest dir so it
/// is independent of the working directory.
pub fn curated_fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("clip-sync-repair")
        .join("tests")
        .join("gap_corpus")
        .join("fingerprints")
        .join("curated")
}

/// Load every curated fixture named in `manifest.json`, in manifest order.
///
/// Panics with a clear message if the manifest or any named corpus is missing/unparseable, or if a fixture
/// is not a single-gap corpus — these are committed test assets, so a failure is a broken fixture, not a
/// recoverable runtime condition.
pub fn load_gap_cell_fixtures() -> Vec<GapCellFixture> {
    let dir = curated_fixtures_dir();
    let manifest_path = dir.join("manifest.json");
    let manifest_bytes = std::fs::read(&manifest_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest_path.display()));
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .unwrap_or_else(|e| panic!("parse {}: {e}", manifest_path.display()));

    manifest
        .fixtures
        .into_iter()
        .map(|entry| {
            let path = dir.join(&entry.file);
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let corpus: GapCorpus = serde_json::from_slice(&bytes)
                .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
            assert_eq!(
                corpus.gaps.len(),
                1,
                "{} must be a single-gap corpus (got {})",
                entry.file,
                corpus.gaps.len()
            );
            GapCellFixture {
                cell_type: entry.cell_type,
                file: entry.file,
                expected: entry.expected,
                provenance: entry.provenance,
                corpus,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn loads_every_curated_fixture_as_single_gap() {
        let fixtures = load_gap_cell_fixtures();
        assert!(!fixtures.is_empty(), "no curated fixtures loaded");
        for f in &fixtures {
            assert_eq!(f.corpus.gaps.len(), 1, "{} not single-gap", f.file);
            assert_eq!(f.gap().index, f.provenance.gap_index, "{} gap index != provenance", f.file);
            assert!(!f.expected.is_empty(), "{} missing expected note", f.file);
        }
    }

    /// Every **representable** cell must have a fixture, and no non-representable cell may (so a taxonomy
    /// member can't silently vanish, and `residual_veto`/`unfillable` can't sneak in a fabricated fixture).
    #[test]
    fn representable_cells_have_a_fixture_and_others_do_not() {
        let present: BTreeSet<&str> =
            load_gap_cell_fixtures().iter().map(|f| f.cell_type.as_str()).collect();
        for &cell in GapCellType::ALL {
            if cell.is_fingerprint_representable() {
                assert!(present.contains(cell.as_str()), "missing representable cell {}", cell.as_str());
            } else {
                assert!(
                    !present.contains(cell.as_str()),
                    "non-representable cell {} must not have a curated fixture",
                    cell.as_str()
                );
            }
        }
    }

    #[test]
    fn manifest_and_on_disk_fixture_files_agree() {
        let dir = curated_fixtures_dir();
        let manifest_files: BTreeSet<String> =
            load_gap_cell_fixtures().into_iter().map(|f| f.file).collect();
        let disk_files: BTreeSet<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read dir {}: {e}", dir.display()))
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".json") && name != "manifest.json")
            .collect();
        assert_eq!(
            manifest_files, disk_files,
            "manifest fixtures and on-disk .json files disagree (manifest-only or orphan file)"
        );
    }
}
