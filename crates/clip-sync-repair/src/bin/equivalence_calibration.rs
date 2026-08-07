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
//! | A span | scan blocks by centre-containment in the silent **core** | *same* | **converged** (2026-08-01) |
//! | bin width | `scan_block_ms` | *same* | **converged** (I1, 2026-07-30) |
//! | donor predicate | `silent` bit **OR** `rms < gap_floor` | *same* | **converged** (I3, 2026-07-31) |
//! | noise floor | median, ±2.0 s (`EQUIVALENCE_CONTEXT_SECS`) | *same* | **closed** (I2, 2026-08-01) |
//! | donor window | the **core**, offset-mapped | *same* | **converged** (2026-08-01) |
//!
//! **The A-span / donor-window rows converged on 2026-08-01, after the corpus refuted them.** Both had
//! been recorded as accepted residuals of "~1 block", measured on one pair / ten gaps. The 39-pair v0.5.0
//! corpus (802 comparable gaps) put them at: donor window differing on **100 %** of gaps, 42 % by more
//! than one block; `donor_silence_fraction` Δ median **0.025**, max **0.333**; and **10 dangerous**
//! divergences where the 17-pair predecessor over the same media had **0**. The cause was one defect, not
//! two: scan windows on `SilentRun::core_*` while the fingerprint's overlay windowed on
//! `geometry.a_*`/`b_mapped_*`, the raw hold-bridged run — the core is narrower by 1–2 blocks on 66.9 % of
//! gaps and the difference is fade shoulder, non-silent, which drags the donor fraction under 0.5. The
//! overlay now takes both spans off the index-parallel scan verdict
//! (`GapEquivalenceVerdict::a_span_secs` / `donor_span_secs`), so they agree by construction.
//!
//! **This is a property of the dumping binary, not of this tool.** A corpus dumped before 2026-08-01
//! still carries the divergence; `measurement.a_span` / `donor_span` now distinguish the cases
//! truthfully (they read `core` on both sides throughout the era when one side measured the raw span —
//! the provenance field concealed exactly what it was added to expose). Re-dump before attributing a
//! divergence in an old corpus to anything else. Do **not** re-quote the 0.008 / 0.067 / 0.606 dB figures
//! below as corpus-wide bounds.
//!
//! **I2 closed 2026-08-01 — by removal, not convergence, and the distinction matters.** The overlay now
//! passes `EQUIVALENCE_CONTEXT_SECS` to its `noise_floor_probe`. Nothing was measured to decide that 2.0 s
//! beats 3.0 s; what was decided is that the split was never a judgement on both sides. The 3.0 s came
//! from `gap_signature_context_secs`, sibling of the `gap_signature_bin_ms` that I1 had already found was
//! inherited by proximity rather than chosen for this job, so "each value encodes a real judgement" (the
//! claim this paragraph used to make) held for scan's value alone. Only the argument moved — the field
//! keeps 3.0 s for `build_gap_fingerprint`, the B-extract padding, and `patch_audio::geometry` / `::region`.
//!
//! Re-measured after the A-span fix and before the removal (4 pairs / 33 gaps, the first un-confounded
//! read): median **1.41 dB**, max **11.96 dB** — against a documented 0.606 dB that was one pair of ten.
//! **Do not quote 0.606 or 1.374 dB.** The axis is now unmeasurable from the live verdicts by
//! construction; take it from the `noise_floor_probes` grid, which still carries the 3.0 s row.
//!
//! **What I2's size actually bounds.** The floor reaches exactly one class boundary —
//! `RepairableDropout` ↔ `AmbientQuiet`. `SharedSilence` (448 of 802 gaps on the 39-pair corpus, most of
//! all drops) is decided by the donor fraction alone and is floor-independent. So the exposed population
//! is gaps with an occupied donor sitting near the −`dropout_margin_db` line: on the 39-pair corpus,
//! **23 gaps within 1 dB** and 78 within 3 dB of it; on the 4-pair re-dump, zero within 2 dB, and
//! swapping the two floors flipped `is_dropout` on **0/33**. For gaps that close to the boundary no
//! achievable convergence decides them — that tail wants a fail-safe margin band (refuse `AmbientQuiet`
//! inside it; a false keep costs a declined patch attempt, a false drop costs an unrepaired hole), which
//! is the open question this one replaces.
//!
//! **The axis that is NOT convergeable by parameter**, and now the largest known one: the two paths bin on
//! differently-*phased* lattices — scan on its media-absolute scan-time block timeline, the overlay from
//! `gap_start − context_frames` on A and from the donor window start on B, with a ragged final bin
//! entering the median at full weight. Equal spans, equal counts and equal floors still disagree by a
//! block (observed: `donor_silent_blocks` 3 vs 2, straddling the 0.5 donor threshold). Note this was
//! **confounded with I2** until the removal, since the A lattice origin moves with the context width —
//! which is why the probe grid's own 2.0-vs-3.0 delta swung up to 10.4 dB and changed sign on a third of
//! gaps, far more than a 1 s widening of a median-over-40-bins estimate can explain.
//!
//! **The diagnostic side is a second opinion, not an oracle** — but the reason has changed, so do not
//! reach for the old one. Until 2026-07-30 this header argued that every known difference biased the
//! fingerprint side toward `drop`, principally because its floor was unfiltered (inflated ⇒ more of B
//! counts silent) and its noise floor read lower on 10/10 gaps by ~3–19 dB. **Both of those terms are
//! gone**: the floor is silence-filtered and the reduction is fixed, so the two floors agree exactly and
//! the noise-floor delta is mixed-sign. Bin-size granularity carried the drop-bias afterwards; I1
//! removed that too. The span mismatch that replaced them was *not* mixed-sign — it was one-sided in the
//! **dangerous** direction, which is how it was caught — and it is now converged. Both
//! floors are recorded per gap (`GapEquivalenceVerdict::gap_floor_db`) so a divergence can be attributed
//! rather than assumed.
//!
//! **Measured populations.** 2026-07-30: 5 divergent / 297 gaps (1.7 %) over a 17-pair corpus, **0
//! dangerous** — measured before I1/I3 and against the pre-I1 instrument. 2026-08-01, over the *same
//! media* (the first 17 pairs of the 39-pair corpus, byte-identical by `a_source.id`/`b_source.id`):
//! **286 compared / 24 divergent / 4 dangerous**, and 802 / 67 / 10 across all 39. The convergence work
//! predicted divergence would fall; it rose ~5×, and the delta was instrument, not media — the span
//! mismatch above. Any population quoted here is a property of the binary that produced the dump. The
//! divergence *mechanism* — a donor sitting between the two floors, silent to one read and occupied to
//! the other — is pinned media-free by `tests/gap_corpus/fingerprints/equivalence_divergence/`.
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
//! A single `--gap-fingerprints DIR` run carries **both** verdicts per gap
//! (`equivalence_diagnostic`, `equivalence_production`). Those keys were `equivalence` /
//! `scan_equivalence` before the 2026-08-07 rename; old corpora still load via serde aliases.
//! Two modes, auto-detected from the argument:
//!
//!   equivalence-calibration out_dir            # ONE corpus (dir or dir/corpus.json) → per-gap table
//!   equivalence-calibration gap-files/equiv-coarse-vs-fine   # PARENT of numbered corpora → roll-up
//!
//! **`--replay`** is a third mode and a different question — it does not compare two paths at all. It
//! re-runs `register_donor_window` on the **recorded envelopes** (`donor_registration.envelopes`,
//! 2026-08-03) and checks that the dump reproduces itself, then re-counts the donor window at the
//! registered lag to say what `DonorRegistrationMode::Apply` decides. Two uses:
//!
//!   - **A replay-completeness gate.** A dump whose recorded inputs do not reproduce its own outputs is
//!     not evidence about anything else it says. Exit code 1 on any mismatch.
//!   - **Re-deciding a dump without re-running it.** The envelopes exist precisely because answering
//!     "how often would the registered lag flip a class?" once cost a full corpus re-dump. It is now
//!     a read.
//!
//! **Tense note (2026-08-04).** `Apply` is now the production default
//! (`RepairConfig::apply_donor_registration`), so on a dump taken since then the `apply` column is
//! what the gate *did*, not a hypothetical. On an older dump — or one taken with
//! `--no-apply-donor-registration` — it is still the counterfactual it was written to answer. The
//! dump records no mode, so the reader has to know which it is holding.
//!
//! The replay calls the production function on levels rebuilt from the record — it is not a
//! reimplementation, so "reproduces" means *identical*, not *close*. The rebuild is lossless by
//! construction: `EnvelopeSlice` stores `rms_db` at `f64` and the silent bits separately, and the
//! spans come off the verdict's own `a_span_secs` / `donor_span_secs`.
//!
//! Read `apply` against `drop` and not on its own. `Apply` also *abstains* below
//! `--replay-min-envelope-r`, which is a keep, and an abstain-driven keep is a different event from a
//! registration-driven one.
//!
//! (That last path is a real on-disk corpus directory, named before the 2026-07-31 correction. The name
//! is left alone so the invocation keeps working; it is not the vocabulary.)
//!
//! Exit code 1 if any **dangerous** divergence exists (scan drops, diagnostic keeps), else 0 — so it can
//! gate CI.
//! See `docs/dev/gap-fingerprint.md` § *`equivalence_diagnostic` vs `equivalence_production`* and `docs/dev/gap-vocabulary.md`
//! § *Silence-character pre-gate*.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use serde::Deserialize;

use clip_sync_repair::application::gap_fingerprint::GapCorpus;
use clip_sync_repair::domain::gap_equivalence::{
    register_donor_window, DonorRegistrationParams, EnvelopeSlice, GapEquivalenceClass,
    GapEquivalenceVerdict,
};
use clip_sync_repair::domain::policies::BlockLevel;

#[derive(Parser)]
#[command(
    about = "Diff the production scan equivalence gate against the fingerprint diagnostic second opinion"
)]
struct Args {
    /// A `--gap-fingerprints` corpus (dir or `corpus.json`) for the per-gap table, OR a parent directory
    /// of numbered corpora for a one-line-per-pair roll-up.
    path: PathBuf,

    /// Margin-band census instead of the divergence table: which gaps production **drops** that sit
    /// within a band of a class boundary, plus a `--only-gaps` token list per pair for re-running
    /// them with `--no-skip-equivalent-gaps`.
    #[arg(long)]
    band: bool,

    /// Band half-width on the dropout boundary, in dB. A dropped gap is banded when it would
    /// classify `repairable_dropout` had the dropout margin been this much shallower.
    #[arg(long, default_value_t = 1.0)]
    band_dropout_db: f64,

    /// Band width on the donor-occupancy boundary, in **blocks**. The donor lattice disagreement
    /// measured between the two front-ends is exactly one block, and short donor windows (5–18
    /// blocks on the flip-sensitive set) make one block worth 6–20 points of the fraction — so the
    /// natural unit here is blocks, not a fraction.
    #[arg(long, default_value_t = 1)]
    band_donor_blocks: usize,

    /// Replay the recorded registration envelopes: check the dump reproduces its own registration and
    /// donor fraction, and report what `DonorRegistrationMode::Apply` decides (what the gate *did*,
    /// on a dump taken with the 2026-08-04 default; the counterfactual on an older one).
    #[arg(long)]
    replay: bool,

    /// `Apply`'s abstain threshold, for the replay's `Apply` column. The default is
    /// `DonorRegistrationParams::min_envelope_r`; vary it to price the abstain rate against a corpus.
    #[arg(long, default_value_t = 0.70)]
    replay_min_envelope_r: f64,
}

/// The classification rule with each boundary loosened by the band — **not** a reimplementation of
/// it. At `band_dropout_db = 0` / `band_donor_blocks = 0` this reduces to
/// `domain::gap_equivalence`'s `(is_dropout, b_occupied)` match exactly, which is what stops the
/// band from drifting away from the gate it models.
///
/// Returns `None` when the verdict lacks a signal the rule needs (`not_evaluated`, no donor).
fn banded_keep(
    v: &GapEquivalenceVerdict,
    band_dropout_db: f64,
    band_donor_blocks: usize,
) -> Option<bool> {
    let t = v.thresholds?;
    let a_below = v.a_below_noise_db?;
    let (silent, total) = (v.donor_silent_blocks?, v.donor_total_blocks?);
    if total == 0 {
        return None;
    }
    // `a < nf − margin` ⟺ `a_below < −margin`; the band shallows the margin.
    let is_dropout = a_below < -t.dropout_margin_db + band_dropout_db;
    // The band lets the donor shed blocks, which is the direction that makes B *occupied*.
    let relaxed = silent.saturating_sub(band_donor_blocks) as f64 / total as f64;
    let b_occupied = relaxed < t.donor_silence_thresh;
    // Only the (dropout, occupied) cell keeps. A `shared_silence` gap whose donor relaxes into
    // occupancy but whose A is not a dropout lands in `ambient_quiet` — still a drop, not a rescue.
    Some(is_dropout && b_occupied)
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

/// Why a gap can carry neither verdict, phrased so the reader is not sent after the wrong remedy.
///
/// The old wording blamed `--fingerprint-gap` subsetting. The dominant real cause is a **head gap**
/// (A starts in silence at t=0) in a pair whose alignment offset is **negative**: B's mapped window
/// would start before zero, `Gap::mapped_b_span` fails closed, and the fingerprint's per-gap loop
/// `continue`s before it can attach either verdict. Measured on the 39-pair v0.5.0 corpus: 27/27
/// unpaired gaps were exactly that shape. Subsetting is still listed — it is a real, if rarer, cause.
fn unpaired_note(unpaired: usize) -> String {
    format!(
        "{unpaired} gap(s) lacked both verdicts — typically a head gap (A silent from t=0) in a pair \
         with a negative offset, so B's window maps before zero and no donor exists; also produced by \
         a --fingerprint-gap subset"
    )
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
    corpus.gaps.iter().map(|fp| {
        (
            fp.equivalence_production.as_ref(),
            fp.equivalence_diagnostic.as_ref(),
        )
    })
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

/// `donor_span` is `Option`: `none` means no donor window was measured (B unmapped, before zero, or
/// past B's end), which is a different statement from either window kind and must print as one.
fn donor_span_token(
    s: Option<clip_sync_repair::domain::gap_equivalence::SpanKind>,
) -> &'static str {
    s.map_or("none", span_token)
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
            donor_span_token(s.donor_span),
            donor_span_token(r.donor_span)
        ));
    }
    parts.join("  ")
}

/// Corpus-level instrument Δ from the first gap that carries both `measurement`s — the structural
/// residuals (typically `ctx +1.0  donor core→nominal`) belong here, not on every agree row.
fn instrument_line(corpus: &GapCorpus) -> Option<String> {
    for fp in &corpus.gaps {
        let (Some(scan), Some(diag)) = (
            fp.equivalence_production.as_ref(),
            fp.equivalence_diagnostic.as_ref(),
        ) else {
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
        "  {:<4} {:<20} {:<18} {:<16} {:<56} verdict",
        "gap",
        "range",
        format!("production({block_ms}ms)"),
        "diagnostic",
        "Δ(diagnostic−production)",
    );
    for fp in &corpus.gaps {
        let (Some(refv), Some(scanv)) = (
            fp.equivalence_diagnostic.as_ref(),
            fp.equivalence_production.as_ref(),
        ) else {
            continue;
        };
        let pv = pair_verdict(scanv, refv);
        let verdict = match pv {
            PairVerdict::Agree => "ok",
            PairVerdict::SafeDiverge => "diverge (safe)",
            PairVerdict::Dangerous => "⚠ DANGEROUS (production drops, diagnostic keeps)",
        };
        let recipe = recipe_deltas(scanv, refv);
        let show_recipe = pv != PairVerdict::Agree || (!recipe.is_empty() && recipe != baseline);
        println!(
            "  {:<4} {:<20} {:<18} {:<16} {:<56} {verdict}",
            fp.index + 1,
            hms(fp.geometry.a_start_secs),
            class_label(scanv.class),
            class_label(refv.class),
            delta_line(scanv, refv, show_recipe),
        );
    }
    let s = summarize(corpus_pairs(corpus));
    println!(
        "\n{} gaps compared · {} divergent · {} dangerous (production-drop / diagnostic-keep)",
        s.compared, s.divergent, s.dangerous
    );
    if s.unpaired > 0 {
        println!("note: {}", unpaired_note(s.unpaired));
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
        "\n{} pair(s) · {} gaps compared · {} divergent · {} dangerous (production-drop / diagnostic-keep)",
        dirs.len() - read_errors,
        total.compared,
        total.divergent,
        total.dangerous,
    );
    // Roll-up mode tallied `unpaired` but never printed it, so gaps excluded from the comparison
    // were visible only by spotting `gaps` != `cmp` on an individual row. A headline count over a
    // population that silently shed members is the §1.1 defect in miniature — say what was skipped.
    if total.unpaired > 0 {
        println!("note: {}", unpaired_note(total.unpaired));
    }
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

/// One pair's banded gaps: the label, the tokens, and the census behind them.
struct BandPair {
    label: String,
    drops: usize,
    /// Zero-based dump indices of dropped gaps the band would keep.
    rescued: Vec<usize>,
    /// Gaps whose verdict predates the `thresholds` field, so the band had to be skipped.
    unprovenanced: usize,
}

/// `--only-gaps` tokens for the rescued set.
///
/// **One-based**, because `--only-gaps` is (`resolve_only_gaps_is_order_insensitive_and_one_based`)
/// while `GapFingerprint::index` is zero-based. This `+ 1` is the entire reason this lives in a
/// binary with a test rather than in a shell one-liner: every token of an off-by-one list still
/// resolves, so the mistake produces a clean run against the neighbouring gaps and no error.
fn only_gaps_tokens(rescued: &[usize]) -> String {
    rescued
        .iter()
        .map(|i| (i + 1).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn band_pair(label: String, corpus: &GapCorpus, args: &Args) -> BandPair {
    let mut out = BandPair {
        label,
        drops: 0,
        rescued: Vec::new(),
        unprovenanced: 0,
    };
    for fp in &corpus.gaps {
        let Some(v) = fp.equivalence_production.as_ref() else {
            continue;
        };
        if !v.drop {
            continue;
        }
        out.drops += 1;
        // Absent thresholds ⇒ the dump predates them and the band would have to *assume* 35.0/0.5.
        // Counted and reported rather than assumed: assuming is exactly the hole this field closed.
        if v.thresholds.is_none() {
            out.unprovenanced += 1;
            continue;
        }
        if banded_keep(v, args.band_dropout_db, args.band_donor_blocks) == Some(true) {
            out.rescued.push(fp.index);
        }
    }
    out
}

/// The labelled corpora under `path`, accepting all three shapes the tool is pointed at: a
/// `corpus.json`, a directory holding one, or a parent of numbered pair directories. Numeric-aware
/// order (1, 2, …, 10) with a lexical fallback.
fn collect_corpora(path: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    if path.is_file() {
        return Ok(vec![("(corpus)".to_string(), path.to_path_buf())]);
    }
    if path.join("corpus.json").is_file() {
        let label = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("(corpus)")
            .to_string();
        return Ok(vec![(label, path.join("corpus.json"))]);
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|e| format!("reading {}: {e}", path.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir() && p.join("corpus.json").is_file())
        .collect();
    dirs.sort_by_key(|p| {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        (name.parse::<u64>().unwrap_or(u64::MAX), name)
    });
    Ok(dirs
        .into_iter()
        .map(|d| {
            let name = d
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();
            (name, d.join("corpus.json"))
        })
        .collect())
}

fn print_band(path: &Path, args: &Args) -> ExitCode {
    // Same single-vs-rollup shape as the divergence report.
    let corpora = match collect_corpora(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if corpora.is_empty() {
        eprintln!("no corpora found under {}", path.display());
        return ExitCode::from(2);
    }

    println!(
        "Margin band: dropout boundary ±{:.2} dB, donor boundary ±{} block(s).",
        args.band_dropout_db, args.band_donor_blocks
    );
    println!(
        "Gaps production DROPS that the band would KEEP — the set to re-run with \
         --no-skip-equivalent-gaps.\n"
    );
    println!(
        "  {:<16} {:<7} {:<9} --only-gaps (1-based)",
        "pair", "drops", "rescued"
    );

    let mut pairs = Vec::new();
    let mut read_errors = 0usize;
    for (label, file) in &corpora {
        let corpus: GapCorpus = match load(file) {
            Ok(c) => c,
            Err(e) => {
                println!("  {label:<16} (read error: {e})");
                read_errors += 1;
                continue;
            }
        };
        let bp = band_pair(label.clone(), &corpus, args);
        if !bp.rescued.is_empty() {
            println!(
                "  {:<16} {:<7} {:<9} {}",
                bp.label,
                bp.drops,
                bp.rescued.len(),
                only_gaps_tokens(&bp.rescued)
            );
        }
        pairs.push(bp);
    }

    let drops: usize = pairs.iter().map(|p| p.drops).sum();
    let rescued: usize = pairs.iter().map(|p| p.rescued.len()).sum();
    let unprovenanced: usize = pairs.iter().map(|p| p.unprovenanced).sum();
    let with_tokens = pairs.iter().filter(|p| !p.rescued.is_empty()).count();
    println!(
        "\n  {rescued} of {drops} dropped gap(s) banded, across {with_tokens} of {} pair(s).",
        pairs.len()
    );
    if unprovenanced > 0 {
        println!(
            "  ⚠ {unprovenanced} dropped gap(s) carry no `thresholds` block — dumps written before \
             2026-08-01. The band is NOT applied to them: it would have to assume the 35.0 dB / 0.5 \
             defaults were in force, which is the assumption that field exists to remove. Re-dump \
             those pairs to include them."
        );
    }
    if read_errors > 0 {
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

/// Rebuild a level stream from a recorded [`EnvelopeSlice`] — the whole of what a dump reader has to
/// do, and the reason the slice stores `rms_db` at `f64` and the silent bits as indices rather than a
/// rounded summary. Mirrors the domain's own replay helper.
fn levels_from(slice: &EnvelopeSlice, bin_ms: f64) -> Vec<BlockLevel> {
    let bin = bin_ms / 1000.0;
    slice
        .rms_db
        .iter()
        .enumerate()
        .map(|(i, &db)| BlockLevel {
            start_secs: slice.start_secs + i as f64 * bin,
            end_secs: slice.start_secs + (i + 1) as f64 * bin,
            rms_db: db,
            silent: slice.silent_bins.contains(&(i as u32)),
        })
        .collect()
}

/// The gate's donor predicate over `[lo, hi)` by block centre: `silent || rms_db < gap_floor`.
/// Returns `(silent, total)`; `None` when no block's centre lands in the window.
fn donor_census(levels: &[BlockLevel], lo: f64, hi: f64, floor: f64) -> Option<(usize, usize)> {
    let win: Vec<&BlockLevel> = levels
        .iter()
        .filter(|b| {
            let c = (b.start_secs + b.end_secs) / 2.0;
            c >= lo && c < hi
        })
        .collect();
    if win.is_empty() {
        return None;
    }
    let silent = win.iter().filter(|b| b.silent || b.rms_db < floor).count();
    Some((silent, win.len()))
}

/// What one gap's recorded envelopes say when replayed.
struct Replay {
    /// Registration reproduced field-for-field (lag, both correlations, bin count).
    reproduces: bool,
    /// Why not, when it did not.
    detail: String,
    /// Donor fraction at the nominal window, re-counted from the record.
    frac_nominal: f64,
    /// …and at the registered lag — the reading `Apply` would have taken.
    frac_registered: f64,
    /// Whether `Apply` would drop this gap, and whether that differs from what production decided.
    apply_drop: bool,
    /// `Apply` declined to place the window (`peak_r` below the abstain threshold), which is a keep
    /// for a different reason than the registration finding an occupied donor.
    abstained: bool,
}

/// Replay one gap. `None` when the verdict carries nothing to replay — no registration, no envelopes,
/// no spans, or a dump predating `thresholds`. Those are counted and named, never assumed.
fn replay_gap(v: &GapEquivalenceVerdict, min_envelope_r: f64) -> Option<Replay> {
    let reg = v.donor_registration.as_ref()?;
    let env = reg.envelopes.as_ref()?;
    let (core_lo, core_hi) = v.a_span_secs?;
    let (donor_lo, donor_hi) = v.donor_span_secs?;
    let floor = v.gap_floor_db?;
    let t = v.thresholds?;
    let a_below = v.a_below_noise_db?;

    let a_levels = levels_from(&env.a, env.bin_ms);
    let b_levels = levels_from(&env.b, env.bin_ms);

    // The production function, on the recorded inputs. Not a reimplementation — that is the point.
    let params = DonorRegistrationParams::default();
    let redone = register_donor_window(
        &a_levels,
        core_lo,
        core_hi,
        &b_levels,
        (donor_lo, donor_hi),
        &params,
    );
    let (reproduces, detail) = match &redone {
        Some(r) => {
            let mut diffs = Vec::new();
            if r.lag_blocks != reg.lag_blocks {
                diffs.push(format!("lag {}→{}", reg.lag_blocks, r.lag_blocks));
            }
            if (r.peak_r - reg.peak_r).abs() > f64::EPSILON {
                diffs.push(format!("peak_r {:+.6}", r.peak_r - reg.peak_r));
            }
            if (r.nominal_r - reg.nominal_r).abs() > f64::EPSILON {
                diffs.push(format!("nominal_r {:+.6}", r.nominal_r - reg.nominal_r));
            }
            if r.bins != reg.bins {
                diffs.push(format!("bins {}→{}", reg.bins, r.bins));
            }
            (diffs.is_empty(), diffs.join(" "))
        }
        None => (false, "registration did not run on the record".to_string()),
    };

    let lag_secs = reg.lag_ms / 1000.0;
    let (sn, tn) = donor_census(&b_levels, donor_lo, donor_hi, floor)?;
    let (sr, tr) = donor_census(&b_levels, donor_lo + lag_secs, donor_hi + lag_secs, floor)?;
    let frac_nominal = sn as f64 / tn as f64;
    let frac_registered = sr as f64 / tr as f64;

    // `Apply`'s decision, from the gate's own rule: keep only a dropout over an occupied donor.
    // Abstaining keeps too, but it is a refusal to place the window rather than a reading of it.
    let abstained = reg.peak_r < min_envelope_r;
    let is_dropout = a_below < -t.dropout_margin_db;
    let b_occupied = frac_registered < t.donor_silence_thresh;
    let apply_drop = if abstained {
        false
    } else {
        !(is_dropout && b_occupied)
    };

    Some(Replay {
        reproduces,
        detail,
        frac_nominal,
        frac_registered,
        apply_drop,
        abstained,
    })
}

/// Tallies for the replay mode.
#[derive(Default)]
struct ReplayTally {
    gaps: usize,
    replayed: usize,
    mismatched: usize,
    /// Recorded `donor_silence_fraction` not reproduced by re-counting the record at the nominal
    /// window — a stronger failure than a registration mismatch: the verdict's own decision input.
    frac_mismatched: usize,
    flips: usize,
    abstains: usize,
    /// Gaps carrying no envelopes (dumps written before 2026-08-03) or no registration at all.
    no_record: usize,
}

fn print_replay(path: &Path, args: &Args) -> ExitCode {
    let corpora = match collect_corpora(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if corpora.is_empty() {
        eprintln!("no corpora found under {}", path.display());
        return ExitCode::from(2);
    }

    println!(
        "Replaying recorded registration envelopes. `apply` is what DonorRegistrationMode::Apply \
         decides (abstain below peak_r {:.2}) — the shipped default since 2026-08-04, so on a \
         current dump this is the gate's own decision.\n",
        args.replay_min_envelope_r
    );
    println!(
        "  {:<10} {:<16} {:<5} {:<5} {:<7} {:<7} {:<7} {:<6} replay",
        "gap", "class", "drop", "lag", "peak_r", "donor@0", "donor@ℓ", "apply"
    );

    let mut tally = ReplayTally::default();
    let mut read_errors = 0usize;
    for (label, file) in &corpora {
        let corpus: GapCorpus = match load(file) {
            Ok(c) => c,
            Err(e) => {
                println!("  {label:<10} (read error: {e})");
                read_errors += 1;
                continue;
            }
        };
        for fp in &corpus.gaps {
            let Some(v) = fp.equivalence_production.as_ref() else {
                continue;
            };
            tally.gaps += 1;
            let Some(r) = replay_gap(v, args.replay_min_envelope_r) else {
                tally.no_record += 1;
                continue;
            };
            tally.replayed += 1;
            if !r.reproduces {
                tally.mismatched += 1;
            }
            let frac_ok = v
                .donor_silence_fraction
                .is_some_and(|f| (f - r.frac_nominal).abs() < 1e-9);
            if !frac_ok {
                tally.frac_mismatched += 1;
            }
            if r.abstained {
                tally.abstains += 1;
            }
            let flip = r.apply_drop != v.drop;
            if flip {
                tally.flips += 1;
            }
            let reg = v.donor_registration.as_ref().expect("replayed ⇒ present");
            let status = match (r.reproduces, frac_ok) {
                (true, true) => "ok".to_string(),
                (true, false) => format!(
                    "⚠ donor fraction {:.3} ≠ recorded {:.3}",
                    r.frac_nominal,
                    v.donor_silence_fraction.unwrap_or(f64::NAN)
                ),
                (false, _) => format!("⚠ {}", r.detail),
            };
            println!(
                "  {:<10} {:<16} {:<5} {:<5} {:<7.3} {:<7.3} {:<7.3} {:<6} {status}",
                format!("{label}/{}", fp.index),
                class_label(v.class),
                v.drop,
                reg.lag_blocks,
                reg.peak_r,
                r.frac_nominal,
                r.frac_registered,
                format!(
                    "{}{}{}",
                    r.apply_drop,
                    if flip { " FLIP" } else { "" },
                    if r.abstained { " abst" } else { "" }
                ),
            );
        }
    }

    println!(
        "\n{} gap(s) · {} replayed · {} registration mismatch · {} donor-fraction mismatch",
        tally.gaps, tally.replayed, tally.mismatched, tally.frac_mismatched
    );
    println!(
        "Apply: {} class flip(s), {} abstain(s) at peak_r < {:.2}",
        tally.flips, tally.abstains, args.replay_min_envelope_r
    );
    if tally.no_record > 0 {
        println!(
            "note: {} gap(s) carry no registration envelopes — dumps written before 2026-08-03, or \
             a donor that could not be registered. Nothing is assumed for them; re-dump to include \
             them.",
            tally.no_record
        );
    }
    if read_errors > 0 || tally.mismatched > 0 || tally.frac_mismatched > 0 {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args = Args::parse();
    let p = &args.path;

    if args.replay {
        return print_replay(p, &args);
    }
    if args.band {
        return print_band(p, &args);
    }

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

    /// Attach the donor populations `banded_keep` needs (`classify_gap_equivalence` takes only the
    /// fraction). `silent`/`total` must be consistent with the fraction the verdict was built from.
    fn with_donor(
        mut v: GapEquivalenceVerdict,
        silent: usize,
        total: usize,
    ) -> GapEquivalenceVerdict {
        v.donor_silent_blocks = Some(silent);
        v.donor_total_blocks = Some(total);
        v
    }

    /// The band's load-bearing property: at zero width it *is* the production rule. If this drifts,
    /// every banded count is measuring a second classifier rather than a margin around the first.
    #[test]
    fn band_of_zero_reproduces_the_production_verdict() {
        for (a, nf, ds, silent, total) in [
            (-106.0, -47.0, 0.0, 0, 10),  // repairable_dropout (keep)
            (-108.0, -46.0, 1.0, 10, 10), // shared_silence (drop)
            (-80.0, -70.0, 0.0, 0, 10),   // ambient_quiet (drop)
            (-106.0, -47.0, 0.6, 6, 10),  // shared_silence, A is a dropout
            (-80.0, -70.0, 0.6, 6, 10),   // shared_silence, A is not
        ] {
            let v = with_donor(
                classify_gap_equivalence(Some(a), Some(nf), Some(ds), &on()),
                silent,
                total,
            );
            assert_eq!(
                banded_keep(&v, 0.0, 0),
                Some(!v.drop),
                "band 0 must agree with production on {:?}",
                v.class
            );
        }
    }

    /// A `shared_silence` gap whose donor relaxes into occupancy is only *rescued* when A is also a
    /// dropout — otherwise it lands in `ambient_quiet`, which still drops. Banding the donor axis
    /// alone would over-count the re-run set.
    #[test]
    fn donor_relaxation_alone_does_not_rescue_a_non_dropout() {
        // Donor 5/9 = 0.556 (silent); shedding one block → 4/9 = 0.444 (occupied).
        let dropout = with_donor(
            classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(5.0 / 9.0), &on()),
            5,
            9,
        );
        let ambient = with_donor(
            classify_gap_equivalence(Some(-80.0), Some(-70.0), Some(5.0 / 9.0), &on()),
            5,
            9,
        );
        assert!(
            dropout.drop && ambient.drop,
            "both are shared_silence today"
        );
        assert_eq!(
            banded_keep(&dropout, 0.0, 1),
            Some(true),
            "A is a dropout — rescued"
        );
        assert_eq!(
            banded_keep(&ambient, 0.0, 1),
            Some(false),
            "A is room tone — relaxing the donor only moves it to ambient_quiet, still a drop"
        );
    }

    /// Dumps predating the `thresholds` field cannot be banded — the band needs both endpoints of a
    /// distance, and assuming the defaults is the hole the field closed.
    #[test]
    fn band_refuses_a_verdict_with_no_recorded_thresholds() {
        let mut v = with_donor(
            classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.6), &on()),
            6,
            10,
        );
        v.thresholds = None;
        assert_eq!(banded_keep(&v, 1.0, 1), None);
    }

    /// `GapFingerprint::index` is zero-based; `--only-gaps` is one-based. An off-by-one list still
    /// resolves cleanly against the neighbouring gaps, so nothing downstream would catch it.
    #[test]
    fn only_gaps_tokens_convert_zero_based_indices_to_one_based() {
        assert_eq!(only_gaps_tokens(&[0, 2, 11]), "1,3,12");
        assert_eq!(only_gaps_tokens(&[]), "");
    }

    /// A level timeline at 100 ms bins: `core` is a silent run, everything else is content whose
    /// value depends on `i - shift`, so two timelines built with different shifts correlate at
    /// exactly that lag.
    fn timeline(n: usize, core: std::ops::Range<usize>, shift: i64) -> Vec<BlockLevel> {
        (0..n)
            .map(|i| {
                let silent = core.contains(&i);
                BlockLevel {
                    start_secs: i as f64 * 0.1,
                    end_secs: (i + 1) as f64 * 0.1,
                    rms_db: if silent {
                        -110.0
                    } else {
                        -30.0 - f64::from(((i as i64 - shift) * 7).rem_euclid(13) as u32)
                    },
                    silent,
                }
            })
            .collect()
    }

    /// A verdict shaped like one a scan writes: registration attached, spans and floor recorded.
    fn verdict_with_registration() -> GapEquivalenceVerdict {
        let a = timeline(120, 50..56, 0);
        let b = timeline(120, 55..61, 5);
        let reg = register_donor_window(
            &a,
            5.0,
            5.6,
            &b,
            (5.0, 5.6),
            &DonorRegistrationParams::default(),
        )
        .expect("registration runs on the fixture");
        assert_eq!(reg.lag_blocks, 5, "B trails A by 5 bins by construction");

        let mut v = classify_gap_equivalence(Some(-110.0), Some(-35.0), Some(1.0 / 6.0), &on());
        v.a_span_secs = Some((5.0, 5.6));
        v.donor_span_secs = Some((5.0, 5.6));
        v.gap_floor_db = Some(-100.0);
        v.donor_registration = Some(reg);
        v
    }

    /// **The replay-completeness contract, in the tool that gates on it.** The recorded envelopes must
    /// reproduce the registration they came from — through the production function, so "reproduces"
    /// means identical rather than close.
    #[test]
    fn replay_reproduces_the_registration_it_recorded() {
        let v = verdict_with_registration();
        let r = replay_gap(&v, 0.70).expect("replayable");
        assert!(r.reproduces, "{}", r.detail);
    }

    /// The other half: re-counting the recorded B bins at the registered lag is what `Apply` would
    /// have read. The fixture's donor is occupied at the nominal window and silent at the registered
    /// one, so the two columns must differ — a replay that returned the same number twice would look
    /// healthy while measuring nothing.
    #[test]
    fn replay_recounts_the_donor_at_the_registered_lag() {
        let v = verdict_with_registration();
        let r = replay_gap(&v, 0.70).expect("replayable");
        assert!(
            r.frac_nominal < 0.5,
            "nominal window sits on B's content: {}",
            r.frac_nominal
        );
        assert_eq!(
            r.frac_registered, 1.0,
            "registered window sits on B's silent run"
        );
        // Occupied donor + A dropout ⇒ production keeps; registered donor is silent ⇒ Apply drops.
        assert!(!v.drop && r.apply_drop, "and that is a class flip");
    }

    /// Below the abstain threshold `Apply` declines to place the window, which is a **keep** — and a
    /// different event from a registration that read an occupied donor. Conflating them would report
    /// abstains as rescues.
    #[test]
    fn replay_abstains_below_the_envelope_threshold() {
        let v = verdict_with_registration();
        let r = replay_gap(&v, 1.01).expect("replayable");
        assert!(r.abstained && !r.apply_drop);
    }

    /// A dump with no envelopes is not replayable and must say so rather than assume anything — the
    /// pre-2026-08-03 corpora are all of this shape.
    #[test]
    fn replay_declines_a_verdict_with_no_envelopes() {
        let mut v = verdict_with_registration();
        v.donor_registration.as_mut().unwrap().envelopes = None;
        assert!(replay_gap(&v, 0.70).is_none());
        let mut v2 = verdict_with_registration();
        v2.donor_registration = None;
        assert!(replay_gap(&v2, 0.70).is_none());
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
            donor_span: Some(SpanKind::Core),
        });
        diag.measurement = Some(EquivalenceMeasurement {
            context_secs: 3.0,
            bin_ms: 100,
            reduction: ChannelReduction::Interleaved,
            a_span: SpanKind::Core,
            donor_span: Some(SpanKind::Nominal),
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
            donor_span: Some(SpanKind::Core),
        };
        scan.measurement = Some(structural.clone());
        diag.measurement = Some(EquivalenceMeasurement {
            context_secs: 3.0,
            donor_span: Some(SpanKind::Nominal),
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
