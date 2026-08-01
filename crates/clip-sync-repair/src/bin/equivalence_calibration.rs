//! `equivalence-calibration` — diff the **production scan gate** (the authoritative equivalence verdict)
//! against the **`--gap-fingerprints` diagnostic second opinion**, per gap. Both feed the same
//! `classify_gap_equivalence`.
//!
//! **Naming (corrected 2026-07-31).** Do not call the two sides "coarse" and "fine". Both now bin at
//! `scan_block_ms`; the diagnostic side's 50 ms binning was inherited by proximity from
//! `gap_signature_bin_ms` and was removed by I1. "Fine" also implied *more accurate* when the truth was
//! *more biased* — the inversion that repeatedly bred "the coarse gate is inaccurate". The axis is
//! **production/authoritative** vs **diagnostic**, not resolution. See `docs/dev/gap-vocabulary.md`
//! § Silence-character pre-gate.
//!
//! **The two paths are now nearly converged, and the history matters for reading this tool's output.**
//! They once differed in how all three inputs were *defined*, not merely sampled; through 2026-07-30/31
//! those definitional differences were fixed one at a time, each validated on media. What is left is one
//! window and one span mapping:
//!
//! | input | production (scan) | diagnostic second opinion | status |
//! |---|---|---|---|
//! | A RMS | silent blocks only, over the silent **core** | *same* | **converged** (F15) — 0.00 dB apart on 10/10 |
//! | `gap_floor` | max of **silent** A blocks (F2/R1-filtered) | *same* | **converged** (F15) — 0.00 dB apart on 10/10 |
//! | reduction | interleaved | *same* | **converged** (F15) |
//! | A span | scan blocks by centre-containment in the raw gap | *same* | **converged** (F15) |
//! | bin width | `scan_block_ms` | *same* | **converged** (I1, 2026-07-30) |
//! | donor predicate | `silent` bit **OR** `rms < gap_floor` | *same* | **converged** (I3, 2026-07-31) |
//! | noise floor | median, ±2.0 s (`EQUIVALENCE_CONTEXT_SECS`) | median, **±3.0 s** (`gap_signature_context_secs`) | **accepted residual** (I2) |
//! | donor window | the **core**, offset-mapped | the **nominal** `b_mapped` span | **accepted residual** — ~1 block |
//!
//! **The two accepted residuals, with measured sizes** (one characterized pair, ten gaps — this bounds
//! the axes on that pair, not on the corpus):
//!
//! - **Noise-floor context window** (I2): median |diag − scan| **0.606 dB**, flipping **one** gap of ten,
//!   in the safe direction. Deliberately not converged: 2.0 s and 3.0 s each encode a real judgement
//!   about how much surrounding material defines "the noise floor here", and `gap_signature_context_secs`
//!   is a configurable parameter with unrelated consumers. Re-open if a broader corpus flips gaps in the
//!   **dangerous** direction, or flips more than a small minority — this tool's exit code already
//!   automates the former trigger.
//! - **Donor window**: median `donor_silence_fraction` delta **0.008** (max 0.067), every gap within
//!   ±1 `scan_block_ms` of alignment, mixed signs.
//!
//! **The diagnostic side is a second opinion, not an oracle** — but the reason has changed, so do not
//! reach for the old one. Until 2026-07-30 this header argued that every known difference biased the
//! fingerprint side toward `drop`, principally because its floor was unfiltered (inflated ⇒ more of B
//! counts silent) and its noise floor read lower on 10/10 gaps by ~3–19 dB. **Both of those terms are
//! gone**: the floor is silence-filtered and the reduction is fixed, so the two floors agree exactly and
//! the noise-floor delta is mixed-sign at 0.606 dB. Bin-size granularity carried the drop-bias
//! afterwards; I1 removed that too. What remains is small, and only the noise-floor window is still
//! one-sided in the safe direction — the donor window is **mixed-sign**, so do not say *both* residuals
//! bias toward `drop`. Both
//! floors are recorded per gap (`GapEquivalenceVerdict::gap_floor_db`) so a divergence can be attributed
//! rather than assumed.
//!
//! **Measured population (2026-07-30):** 5 divergent / 297 gaps (1.7 %) over a 17-pair corpus,
//! recipe-invariant, with **0 dangerous**. The mechanism is a donor sitting *between* the two floors —
//! silent to the diagnostic read, occupied to scan — pinned media-free by
//! `tests/gap_corpus/fingerprints/equivalence_divergence/`. Note the population was measured **before**
//! I1/I3 landed and against the pre-I1 instrument; the divergence count should now be lower.
//!
//! Read `0 dangerous` with one caveat: every pair in that corpus is lossy and never reaches exact digital
//! silence. Until I3 (2026-07-31) the fingerprint donor predicate lacked scan's `silent`-bit disjunct, and
//! on a digitally-silent gap with a digitally-silent donor it would have read the donor **occupied**
//! (`−120 < −120` is false) and reported this tool's dangerous direction spuriously. Lossless or
//! genuinely-muted material would have tripped it; the corpus simply could not.
//!
//! This tool quantifies where the two disagree on real media — especially the one dangerous direction:
//! **scan says *drop* but the diagnostic says *keep*** (a potential false drop / unrepaired hole). That is
//! the direction the surviving residual does **not** manufacture, which is what makes it worth gating on.
//!
//! A single `--gap-fingerprints DIR` run carries **both** verdicts per gap (`equivalence` = diagnostic,
//! `scan_equivalence` = production). Two modes, auto-detected from the argument:
//!
//!   equivalence-calibration out_dir            # ONE corpus (dir or dir/corpus.json) → per-gap table
//!   equivalence-calibration gap-files/equiv-coarse-vs-fine   # PARENT of numbered corpora → roll-up
//!
//! (That last path is a real on-disk corpus directory, named before the 2026-07-31 correction. The name
//! is left alone so the invocation keeps working; it is not the vocabulary.)
//!
//! Exit code 1 if any **dangerous** divergence exists (scan drops, diagnostic keeps), else 0 — so it can
//! gate CI.
//! See `docs/dev/gap-fingerprint.md` § *`equivalence` vs `scan_equivalence`* and `docs/dev/gap-vocabulary.md`
//! § *Silence-character pre-gate*.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use serde::Deserialize;

use clip_sync_repair::application::gap_fingerprint::GapCorpus;
use clip_sync_repair::domain::gap_equivalence::{GapEquivalenceClass, GapEquivalenceVerdict};

#[derive(Parser)]
#[command(
    about = "Diff the production scan equivalence gate against the fingerprint diagnostic second opinion"
)]
struct Args {
    /// A `--gap-fingerprints` corpus (dir or `corpus.json`) for the per-gap table, OR a parent directory
    /// of numbered corpora for a one-line-per-pair roll-up.
    path: PathBuf,
}

/// The per-gap comparison outcome between the production scan verdict and the diagnostic second opinion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairVerdict {
    /// Both paths landed on the same class.
    Agree,
    /// Classes differ but the scan does **not** drop a gap the diagnostic keeps — a missed optimization
    /// (scan keeps, diagnostic drops) or a benign class swap. Safe, and expected at a low rate: the
    /// surviving noise-floor-window residual biases the diagnostic side toward `drop`, so this is the
    /// direction it produces. The ~2 % measured over the 17-pair corpus predates I1/I3 and should now
    /// read lower.
    SafeDiverge,
    /// Scan drops a gap the diagnostic path would keep — a potential **false drop** (unrepaired hole). No
    /// surviving instrument residual produces this direction, so a hit here warrants investigation
    /// rather than a shrug.
    Dangerous,
}

/// Compare a production (scan) verdict against the diagnostic one. The only unsafe divergence is the
/// scan removing a gap the diagnostic would keep.
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

fn span_token(s: clip_sync_repair::domain::gap_equivalence::SpanKind) -> &'static str {
    use clip_sync_repair::domain::gap_equivalence::SpanKind;
    match s {
        SpanKind::Core => "core",
        SpanKind::Nominal => "nominal",
    }
}

fn reduction_token(r: clip_sync_repair::domain::gap_equivalence::ChannelReduction) -> &'static str {
    use clip_sync_repair::domain::gap_equivalence::ChannelReduction;
    match r {
        ChannelReduction::Interleaved => "interleaved",
        ChannelReduction::Downmix => "downmix",
    }
}

/// Recipe Δ (diag − scan): **field diffs only**, when both sides carry `measurement`. The I2 /
/// donor-window residuals are structural and constant per corpus — surface those once via
/// [`instrument_line`]; per-row recipe Δ is for diverge rows or anomalies (§3d / §5).
fn recipe_deltas(scan: &GapEquivalenceVerdict, refv: &GapEquivalenceVerdict) -> String {
    let (Some(s), Some(r)) = (&scan.measurement, &refv.measurement) else {
        return String::new();
    };
    let mut parts = Vec::new();
    if (s.context_secs - r.context_secs).abs() > f64::EPSILON {
        parts.push(format!("ctx {:+.1}", r.context_secs - s.context_secs));
    }
    if s.bin_ms != r.bin_ms {
        parts.push(format!("bin {}→{}", s.bin_ms, r.bin_ms));
    }
    if s.reduction != r.reduction {
        parts.push(format!(
            "red {}→{}",
            reduction_token(s.reduction),
            reduction_token(r.reduction)
        ));
    }
    if s.a_span != r.a_span {
        parts.push(format!(
            "a_span {}→{}",
            span_token(s.a_span),
            span_token(r.a_span)
        ));
    }
    if s.donor_span != r.donor_span {
        parts.push(format!(
            "donor {}→{}",
            span_token(s.donor_span),
            span_token(r.donor_span)
        ));
    }
    parts.join("  ")
}

/// Corpus-level instrument Δ from the first gap that carries both `measurement`s — the structural
/// residuals (typically `ctx +1.0  donor core→nominal`) belong here, not on every agree row.
fn instrument_line(corpus: &GapCorpus) -> Option<String> {
    for fp in &corpus.gaps {
        let (Some(scan), Some(diag)) = (fp.scan_equivalence.as_ref(), fp.equivalence.as_ref())
        else {
            continue;
        };
        let d = recipe_deltas(scan, diag);
        if !d.is_empty() {
            return Some(d);
        }
    }
    None
}

/// Signal Δ always; recipe Δ only when the class diverges or the recipe differs from the corpus
/// baseline (so agree rows are not a constant two-field wall).
fn delta_line(
    scan: &GapEquivalenceVerdict,
    refv: &GapEquivalenceVerdict,
    show_recipe: bool,
) -> String {
    let sig = signal_deltas(scan, refv);
    let recipe = if show_recipe {
        recipe_deltas(scan, refv)
    } else {
        String::new()
    };
    match (sig.is_empty(), recipe.is_empty()) {
        (true, true) => String::new(),
        (false, true) => sig,
        (true, false) => recipe,
        (false, false) => format!("{sig}  {recipe}"),
    }
}

/// Print the per-gap table for one corpus and return its tally.
fn print_detail(corpus: &GapCorpus) -> Summary {
    let block_ms = corpus.source.scan_recipe.scan_block_ms.unwrap_or(250);
    let baseline = instrument_line(corpus).unwrap_or_default();
    println!(
        "  {:<4} {:<20} {:<16} {:<16} {:<56} verdict",
        "gap",
        "range",
        format!("scan({block_ms}ms)"),
        "diag",
        "Δ(diag−scan)",
    );
    for fp in &corpus.gaps {
        let (Some(refv), Some(scanv)) = (fp.equivalence.as_ref(), fp.scan_equivalence.as_ref())
        else {
            continue;
        };
        let pv = pair_verdict(scanv, refv);
        let verdict = match pv {
            PairVerdict::Agree => "ok",
            PairVerdict::SafeDiverge => "diverge (safe)",
            PairVerdict::Dangerous => "⚠ DANGEROUS (scan drops, diag keeps)",
        };
        let recipe = recipe_deltas(scanv, refv);
        let show_recipe = pv != PairVerdict::Agree || (!recipe.is_empty() && recipe != baseline);
        println!(
            "  {:<4} {:<20} {:<16} {:<16} {:<56} {verdict}",
            fp.index + 1,
            hms(fp.geometry.a_start_secs),
            class_label(scanv.class),
            class_label(refv.class),
            delta_line(scanv, refv, show_recipe),
        );
    }
    let s = summarize(corpus_pairs(corpus));
    println!(
        "\n{} gaps compared · {} divergent · {} dangerous (scan-drop / diag-keep)",
        s.compared, s.divergent, s.dangerous
    );
    if s.unpaired > 0 {
        println!("note: {} gap(s) lacked both verdicts (characterize a full corpus — no --fingerprint-gap subset)", s.unpaired);
    }
    println!("sources: {}", source_line(corpus));
    if !baseline.is_empty() {
        println!("instruments: {baseline}");
    }
    s
}

/// One line of source provenance for a corpus: what codecs it measured and whether either side was
/// rate-converted first. `?` where the corpus predates source provenance — informational only, so a
/// pre-Track-A corpus reads honestly instead of failing.
fn source_line(corpus: &GapCorpus) -> String {
    let a = &corpus.source.a_source;
    let b = &corpus.source.b_source;
    format!(
        "codecs {} · resampled a={} b={}",
        codec_pair_label(corpus),
        opt_bool(a.was_resampled()),
        opt_bool(b.was_resampled()),
    )
}

fn opt_bool(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "yes",
        Some(false) => "no",
        None => "?",
    }
}

/// `a_codec→b_codec`, with `?` for a side whose codec the corpus does not record.
fn codec_pair_label(corpus: &GapCorpus) -> String {
    let name = |c: &Option<String>| c.clone().unwrap_or_else(|| "?".into());
    format!(
        "{}→{}",
        name(&corpus.source.a_source.codec),
        name(&corpus.source.b_source.codec)
    )
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
    let mut codec_census: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
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
        *codec_census.entry(codec_pair_label(&corpus)).or_default() += 1;
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
        "\n{} pair(s) · {} gaps compared · {} divergent · {} dangerous (scan-drop / diag-keep)",
        dirs.len() - read_errors,
        total.compared,
        total.divergent,
        total.dangerous,
    );
    // Census only — a corpus mixing codecs is a stratification fact, never an exit-code condition.
    let census = codec_census
        .iter()
        .map(|(label, n)| format!("{label}×{n}"))
        .collect::<Vec<_>>()
        .join(" · ");
    println!(
        "codecs (a→b, pairs): {}",
        if census.is_empty() {
            "none".into()
        } else {
            census
        }
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
            (Some(&d), Some(&sh)),  // safe diverge (scan keeps, diag drops)
            (Some(&sh), Some(&d)),  // dangerous (scan drops, diag keeps)
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

    /// Track B §3d: recipe Δ prints only fields that differ (the live residuals).
    #[test]
    fn recipe_deltas_print_diffs_only() {
        use clip_sync_repair::domain::gap_equivalence::{
            ChannelReduction, EquivalenceMeasurement, SpanKind,
        };
        let mut scan = dropout();
        let mut diag = dropout();
        scan.measurement = Some(EquivalenceMeasurement {
            context_secs: 2.0,
            bin_ms: 100,
            reduction: ChannelReduction::Interleaved,
            a_span: SpanKind::Core,
            donor_span: SpanKind::Core,
        });
        diag.measurement = Some(EquivalenceMeasurement {
            context_secs: 3.0,
            bin_ms: 100,
            reduction: ChannelReduction::Interleaved,
            a_span: SpanKind::Core,
            donor_span: SpanKind::Nominal,
        });
        assert_eq!(recipe_deltas(&scan, &diag), "ctx +1.0  donor core→nominal");
        // Agreeing recipes → empty (no five-field wall).
        assert_eq!(recipe_deltas(&scan, &scan), "");
        // Missing measurement → empty.
        assert_eq!(recipe_deltas(&dropout(), &diag), "");
    }

    /// Agree rows omit the corpus-baseline recipe Δ; diverge / anomaly rows keep it.
    #[test]
    fn delta_line_reserves_recipe_for_diverge_or_anomaly() {
        use clip_sync_repair::domain::gap_equivalence::{
            ChannelReduction, EquivalenceMeasurement, SpanKind,
        };
        let mut scan = dropout();
        let mut diag = dropout();
        let structural = EquivalenceMeasurement {
            context_secs: 2.0,
            bin_ms: 100,
            reduction: ChannelReduction::Interleaved,
            a_span: SpanKind::Core,
            donor_span: SpanKind::Core,
        };
        scan.measurement = Some(structural.clone());
        diag.measurement = Some(EquivalenceMeasurement {
            context_secs: 3.0,
            donor_span: SpanKind::Nominal,
            ..structural.clone()
        });
        let baseline = recipe_deltas(&scan, &diag);
        assert_eq!(baseline, "ctx +1.0  donor core→nominal");
        // Agree + matches baseline ⇒ signal only (no recipe wall).
        assert_eq!(
            delta_line(&scan, &diag, /* show_recipe */ false),
            signal_deltas(&scan, &diag)
        );
        // Diverge ⇒ recipe included.
        let mut diag_drop = shared();
        diag_drop.measurement = diag.measurement.clone();
        assert!(
            delta_line(&scan, &diag_drop, true).contains("donor core→nominal"),
            "{}",
            delta_line(&scan, &diag_drop, true)
        );
        // Agree but anomalous bin ⇒ recipe shown even when "show" is driven by != baseline.
        let mut anom = diag.clone();
        anom.measurement.as_mut().unwrap().bin_ms = 50;
        let anom_recipe = recipe_deltas(&scan, &anom);
        assert!(anom_recipe.contains("bin 100→50"), "{anom_recipe}");
        assert_ne!(anom_recipe, baseline);
    }
}
