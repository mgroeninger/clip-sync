//! `equivalence-calibration` — diff the **coarse production scan gate** (the scan-block equivalence gate)
//! against the **fine `--gap-fingerprints` reference** (sample-level A RMS + fine-bin noise floor + 50 ms
//! donor bins), per gap. Both paths feed the same `classify_gap_equivalence`; they differ only in
//! measurement granularity. This tool quantifies where the cheap production path disagrees with the fine
//! reference on real media — especially the one dangerous direction: **scan says *drop* but the reference
//! says *keep*** (a potential false drop / unrepaired hole).
//!
//! A single `--gap-fingerprints DIR` run carries **both** verdicts per gap (`equivalence` = fine,
//! `scan_equivalence` = coarse). Two modes, auto-detected from the argument:
//!
//!   equivalence-calibration out_dir            # ONE corpus (dir or dir/corpus.json) → per-gap table
//!   equivalence-calibration gap-files/equiv-coarse-vs-fine   # PARENT of numbered corpora → roll-up
//!
//! Exit code 1 if any **dangerous** divergence exists (scan drops, reference keeps), else 0 — so it can gate CI.
//! See `docs/dev/gap-fingerprint.md` § equivalence (fine vs coarse) and `docs/dev/gap-vocabulary.md`
//! § *Silence-character pre-gate*.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use serde::Deserialize;

use clip_sync_repair::application::gap_fingerprint::GapCorpus;
use clip_sync_repair::domain::gap_equivalence::{GapEquivalenceClass, GapEquivalenceVerdict};

#[derive(Parser)]
#[command(
    about = "Diff the scan-block equivalence gate against the fine-bin fingerprint reference"
)]
struct Args {
    /// A `--gap-fingerprints` corpus (dir or `corpus.json`) for the per-gap table, OR a parent directory
    /// of numbered corpora for a one-line-per-pair roll-up.
    path: PathBuf,
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

/// Tallies across a corpus (or a roll-up total).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Summary {
    compared: usize,
    divergent: usize,
    dangerous: usize,
    unpaired: usize,
}

impl Summary {
    fn add(&mut self, o: Summary) {
        self.compared += o.compared;
        self.divergent += o.divergent;
        self.dangerous += o.dangerous;
        self.unpaired += o.unpaired;
    }
}

/// Tally `(scan, reference)` verdict pairs. `None` on either side ⇒ unpaired (not compared).
fn summarize<'a>(
    pairs: impl Iterator<
        Item = (
            Option<&'a GapEquivalenceVerdict>,
            Option<&'a GapEquivalenceVerdict>,
        ),
    >,
) -> Summary {
    let mut s = Summary::default();
    for (scan, refv) in pairs {
        match (scan, refv) {
            (Some(scanv), Some(refv)) => {
                s.compared += 1;
                match pair_verdict(scanv, refv) {
                    PairVerdict::Agree => {}
                    PairVerdict::SafeDiverge => s.divergent += 1,
                    PairVerdict::Dangerous => {
                        s.divergent += 1;
                        s.dangerous += 1;
                    }
                }
            }
            _ => s.unpaired += 1,
        }
    }
    s
}

/// `(scan, reference)` verdict pairs for a corpus, in gap order.
fn corpus_pairs(
    corpus: &GapCorpus,
) -> impl Iterator<
    Item = (
        Option<&GapEquivalenceVerdict>,
        Option<&GapEquivalenceVerdict>,
    ),
> {
    corpus
        .gaps
        .iter()
        .map(|fp| (fp.scan_equivalence.as_ref(), fp.equivalence.as_ref()))
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

/// Print the per-gap table for one corpus and return its tally.
fn print_detail(corpus: &GapCorpus) -> Summary {
    let block_ms = corpus.source.scan_recipe.scan_block_ms.unwrap_or(250);
    println!(
        "  {:<4} {:<20} {:<16} {:<16} {:<26} verdict",
        "gap",
        "range",
        format!("scan({block_ms}ms)"),
        "ref(fine)",
        "Δ(ref−scan)",
    );
    for fp in &corpus.gaps {
        let (Some(refv), Some(scanv)) = (fp.equivalence.as_ref(), fp.scan_equivalence.as_ref())
        else {
            continue;
        };
        let verdict = match pair_verdict(scanv, refv) {
            PairVerdict::Agree => "ok",
            PairVerdict::SafeDiverge => "diverge (safe)",
            PairVerdict::Dangerous => "⚠ DANGEROUS (scan drops, ref keeps)",
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
    let s = summarize(corpus_pairs(corpus));
    println!(
        "\n{} gaps compared · {} divergent · {} dangerous (scan-drop / ref-keep)",
        s.compared, s.divergent, s.dangerous
    );
    if s.unpaired > 0 {
        println!("note: {} gap(s) lacked both verdicts (characterize a full corpus — no --fingerprint-gap subset)", s.unpaired);
    }
    s
}

/// Roll up every numbered corpus under `parent` (immediate subdirs containing `corpus.json`).
fn print_rollup(parent: &Path) -> ExitCode {
    let mut dirs: Vec<PathBuf> = match std::fs::read_dir(parent) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir() && p.join("corpus.json").is_file())
            .collect(),
        Err(e) => {
            eprintln!("error: reading {}: {e}", parent.display());
            return ExitCode::from(2);
        }
    };
    if dirs.is_empty() {
        eprintln!(
            "no corpora found under {} (expected numbered subdirs each with corpus.json)",
            parent.display()
        );
        return ExitCode::from(2);
    }
    // Numeric-aware order (1, 2, …, 10) with a lexical fallback.
    dirs.sort_by_key(|p| {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        (name.parse::<u64>().unwrap_or(u64::MAX), name)
    });

    println!(
        "  {:<16} {:<6} {:<5} {:<8} {:<8} verdict",
        "pair", "gaps", "cmp", "diverg", "danger"
    );
    let mut total = Summary::default();
    let mut read_errors = 0usize;
    for d in &dirs {
        let name = d
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let corpus: GapCorpus = match load(&d.join("corpus.json")) {
            Ok(c) => c,
            Err(e) => {
                println!("  {name:<16} (read error: {e})");
                read_errors += 1;
                continue;
            }
        };
        let s = summarize(corpus_pairs(&corpus));
        total.add(s);
        let verdict = if s.dangerous > 0 {
            "⚠ DANGEROUS"
        } else if s.divergent > 0 {
            "ok (safe diverge)"
        } else {
            "ok"
        };
        println!(
            "  {name:<16} {:<6} {:<5} {:<8} {:<8} {verdict}",
            corpus.gaps.len(),
            s.compared,
            s.divergent,
            s.dangerous,
        );
    }
    println!(
        "  {:<16} {:<6} {:<5} {:<8} {:<8} {}",
        "TOTAL",
        "",
        total.compared,
        total.divergent,
        total.dangerous,
        if total.dangerous > 0 {
            "⚠ DANGEROUS"
        } else {
            "ok"
        },
    );
    println!(
        "\n{} pair(s) · {} gaps compared · {} divergent · {} dangerous (scan-drop / ref-keep)",
        dirs.len() - read_errors,
        total.compared,
        total.divergent,
        total.dangerous,
    );

    if total.dangerous > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    let p = &args.path;

    // Single-corpus if the arg is a corpus.json file or a dir directly holding one; else roll up its subdirs.
    let direct = if p.is_file() {
        Some(p.clone())
    } else if p.join("corpus.json").is_file() {
        Some(p.join("corpus.json"))
    } else {
        None
    };

    match direct {
        Some(path) => match load::<GapCorpus>(&path) {
            Ok(corpus) => {
                let s = print_detail(&corpus);
                if s.dangerous > 0 {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(e) => {
                eprintln!("error: reading corpus {}: {e}", path.display());
                ExitCode::from(2)
            }
        },
        None => print_rollup(p),
    }
}

fn load<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clip_sync_repair::domain::gap_equivalence::{
        classify_gap_equivalence, GapEquivalenceParams,
    };

    fn on() -> GapEquivalenceParams {
        GapEquivalenceParams {
            enabled: true,
            ..Default::default()
        }
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
        assert_eq!(pair_verdict(&shared(), &dropout()), PairVerdict::Dangerous);
        assert_eq!(pair_verdict(&ambient(), &dropout()), PairVerdict::Dangerous);
    }

    #[test]
    fn scan_keeps_but_reference_drops_is_safe() {
        assert_eq!(
            pair_verdict(&dropout(), &shared()),
            PairVerdict::SafeDiverge
        );
    }

    #[test]
    fn both_drop_different_classes_is_safe() {
        assert_eq!(
            pair_verdict(&shared(), &ambient()),
            PairVerdict::SafeDiverge
        );
    }

    #[test]
    fn hms_formats_hours_and_minutes() {
        assert_eq!(hms(0.0), "0:00.0");
        assert_eq!(hms(61.5), "1:01.5");
        assert_eq!(hms(3661.2), "1:01:01.2");
    }

    #[test]
    fn summarize_counts_agree_safe_dangerous_and_unpaired() {
        let (d, sh, am) = (dropout(), shared(), ambient());
        let pairs = vec![
            (Some(&d), Some(&d)),   // agree
            (Some(&sh), Some(&sh)), // agree
            (Some(&d), Some(&sh)),  // safe diverge (scan keeps, ref drops)
            (Some(&sh), Some(&d)),  // dangerous (scan drops, ref keeps)
            (Some(&am), Some(&d)),  // dangerous
            (None, Some(&d)),       // unpaired
        ];
        let s = summarize(pairs.into_iter());
        assert_eq!(s.compared, 5);
        assert_eq!(s.divergent, 3); // 1 safe + 2 dangerous
        assert_eq!(s.dangerous, 2);
        assert_eq!(s.unpaired, 1);
    }

    #[test]
    fn summary_add_accumulates() {
        let mut a = Summary {
            compared: 3,
            divergent: 1,
            dangerous: 0,
            unpaired: 0,
        };
        a.add(Summary {
            compared: 2,
            divergent: 1,
            dangerous: 1,
            unpaired: 1,
        });
        assert_eq!(
            a,
            Summary {
                compared: 5,
                divergent: 2,
                dangerous: 1,
                unpaired: 1
            }
        );
    }
}
