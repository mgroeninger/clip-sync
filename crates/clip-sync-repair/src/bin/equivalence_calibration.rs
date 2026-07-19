//! `equivalence-calibration` — diff the **coarse production scan gate** (250 ms scan blocks) against the
//! **fine `--gap-fingerprints` reference** (sample-level A RMS + fine-bin noise floor + 50 ms donor bins),
//! per gap. Both paths feed the same `classify_gap_equivalence`; they differ only in measurement
//! granularity. This tool quantifies where the cheap production path disagrees with the fine reference on
//! real media — especially the one dangerous direction: **scan says *drop* but the reference says *keep***
//! (a potential false drop / unrepaired hole).
//!
//! A single `--gap-fingerprints DIR` run now carries **both** verdicts per gap (`equivalence` = fine,
//! `scan_equivalence` = coarse), so this tool reads just that one corpus:
//!
//!   clip-sync-repair A B --gap-fingerprints out_dir
//!   equivalence-calibration out_dir            # (or out_dir/corpus.json)
//!
//! Exit code 1 if any **dangerous** divergence exists (scan drops, reference keeps), else 0 — so it can gate CI.
//! See `docs/TEMP-gap-equivalence-plan.md` § *Granularity tradeoff* and `docs/gap-vocabulary.md`
//! § *Silence-character pre-gate*.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use serde::Deserialize;

use clip_sync_repair::application::gap_fingerprint::GapCorpus;
use clip_sync_repair::domain::gap_equivalence::{GapEquivalenceClass, GapEquivalenceVerdict};

#[derive(Parser)]
#[command(about = "Diff the 250 ms scan equivalence gate against the fine-bin fingerprint reference")]
struct Args {
    /// `--gap-fingerprints` output: either the corpus directory or its `corpus.json` directly.
    corpus: PathBuf,
}

/// The per-gap comparison outcome between the coarse scan verdict and the fine reference verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairVerdict {
    /// Both paths landed on the same class.
    Agree,
    /// Classes differ but the scan does **not** drop a gap the reference keeps — a missed optimization
    /// (scan keeps, ref drops) or a benign class swap. Safe.
    SafeDiverge,
    /// Scan drops a gap the fine reference would keep — a potential **false drop** (unrepaired hole).
    Dangerous,
}

/// Compare a coarse (scan) verdict against a fine (reference) verdict. The only unsafe divergence is the
/// scan removing a gap the reference would keep.
fn pair_verdict(scan: &GapEquivalenceVerdict, refv: &GapEquivalenceVerdict) -> PairVerdict {
    if scan.drop && !refv.drop {
        PairVerdict::Dangerous
    } else if scan.class == refv.class {
        PairVerdict::Agree
    } else {
        PairVerdict::SafeDiverge
    }
}

fn class_label(c: GapEquivalenceClass) -> &'static str {
    match c {
        GapEquivalenceClass::RepairableDropout => "dropout",
        GapEquivalenceClass::SharedSilence => "shared_silence",
        GapEquivalenceClass::AmbientQuiet => "ambient_quiet",
        GapEquivalenceClass::NotEvaluated => "not_evaluated",
    }
}

fn hms(secs: f64) -> String {
    let s = secs.max(0.0);
    let h = (s / 3600.0) as u64;
    let m = ((s % 3600.0) / 60.0) as u64;
    let sec = s % 60.0;
    if h > 0 {
        format!("{h}:{m:02}:{sec:04.1}")
    } else {
        format!("{m}:{sec:04.1}")
    }
}

/// A signal delta string (reference − scan), only for signals both sides carry — shows *why* they diverge.
fn signal_deltas(scan: &GapEquivalenceVerdict, refv: &GapEquivalenceVerdict) -> String {
    let mut parts = Vec::new();
    if let (Some(a), Some(b)) = (scan.noise_floor_db, refv.noise_floor_db) {
        parts.push(format!("nf {:+.1}", b - a));
    }
    if let (Some(a), Some(b)) = (scan.a_gap_rms_db, refv.a_gap_rms_db) {
        parts.push(format!("aRMS {:+.1}", b - a));
    }
    if let (Some(a), Some(b)) = (scan.donor_silence_fraction, refv.donor_silence_fraction) {
        parts.push(format!("ds {:+.2}", b - a));
    }
    parts.join("  ")
}

/// Resolve the corpus path: a directory ⇒ `<dir>/corpus.json`, else the path itself.
fn corpus_json_path(p: &Path) -> PathBuf {
    if p.is_dir() {
        p.join("corpus.json")
    } else {
        p.to_path_buf()
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    let path = corpus_json_path(&args.corpus);

    let corpus: GapCorpus = match load(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: reading corpus {}: {e}", path.display());
            return ExitCode::from(2);
        }
    };

    println!(
        "  gap  range                scan(250ms)      ref(fine)        Δ(ref−scan)                verdict"
    );

    let (mut compared, mut divergent, mut dangerous, mut unpaired) = (0usize, 0usize, 0usize, 0usize);

    for fp in &corpus.gaps {
        // fine = the fingerprint `equivalence`; coarse = the copied-in `scan_equivalence`.
        let (Some(refv), Some(scanv)) = (fp.equivalence.as_ref(), fp.scan_equivalence.as_ref()) else {
            unpaired += 1;
            continue;
        };
        compared += 1;

        let verdict = match pair_verdict(scanv, refv) {
            PairVerdict::Agree => "ok",
            PairVerdict::SafeDiverge => {
                divergent += 1;
                "diverge (safe)"
            }
            PairVerdict::Dangerous => {
                divergent += 1;
                dangerous += 1;
                "⚠ DANGEROUS (scan drops, ref keeps)"
            }
        };

        println!(
            "  {:<4} {:<20} {:<16} {:<16} {:<26} {verdict}",
            fp.index + 1,
            hms(fp.geometry.a_start_secs),
            class_label(scanv.class),
            class_label(refv.class),
            signal_deltas(scanv, refv),
        );
    }

    println!(
        "\n{compared} gaps compared · {divergent} divergent · {dangerous} dangerous (scan-drop / ref-keep)"
    );
    if unpaired > 0 {
        println!(
            "note: {unpaired} gap(s) lacked both verdicts (characterize a full corpus — no --fingerprint-gap subset — so scan_equivalence is present)"
        );
    }

    if dangerous > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn load<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clip_sync_repair::domain::gap_equivalence::{classify_gap_equivalence, GapEquivalenceParams};

    fn on() -> GapEquivalenceParams {
        GapEquivalenceParams { enabled: true, ..Default::default() }
    }
    fn dropout() -> GapEquivalenceVerdict {
        classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.0), &on()) // repairable_dropout (keep)
    }
    fn shared() -> GapEquivalenceVerdict {
        classify_gap_equivalence(Some(-108.0), Some(-46.0), Some(1.0), &on()) // shared_silence (drop)
    }
    fn ambient() -> GapEquivalenceVerdict {
        classify_gap_equivalence(Some(-80.0), Some(-70.0), Some(0.0), &on()) // ambient_quiet (drop)
    }

    #[test]
    fn agreement_is_ok() {
        assert_eq!(pair_verdict(&dropout(), &dropout()), PairVerdict::Agree);
        assert_eq!(pair_verdict(&shared(), &shared()), PairVerdict::Agree);
    }

    #[test]
    fn scan_drops_but_reference_keeps_is_dangerous() {
        // Coarse path dropped (shared_silence); fine reference keeps (dropout) → false-drop risk.
        assert_eq!(pair_verdict(&shared(), &dropout()), PairVerdict::Dangerous);
        assert_eq!(pair_verdict(&ambient(), &dropout()), PairVerdict::Dangerous);
    }

    #[test]
    fn scan_keeps_but_reference_drops_is_safe() {
        // Coarse path kept; fine reference would drop → only a missed optimization.
        assert_eq!(pair_verdict(&dropout(), &shared()), PairVerdict::SafeDiverge);
    }

    #[test]
    fn both_drop_different_classes_is_safe() {
        // shared_silence vs ambient_quiet: classes differ but both drop → not dangerous.
        assert_eq!(pair_verdict(&shared(), &ambient()), PairVerdict::SafeDiverge);
    }

    #[test]
    fn hms_formats_hours_and_minutes() {
        assert_eq!(hms(0.0), "0:00.0");
        assert_eq!(hms(61.5), "1:01.5");
        assert_eq!(hms(3661.2), "1:01:01.2");
    }
}
