//! Cross-corpus aggregation of gap-fingerprint output (the **P0 prevalence scan**, see
//! `docs/TEMP-w5-timing-offset-rescue-plan.md` §5 P0).
//!
//! `--gap-fingerprints DIR` writes one `corpus.json` per A/B pair (all that pair's gaps) plus per-gap
//! library files. Point this at the parent of several such runs (e.g. `gap-files/`, holding `1/`..`6/`)
//! and it tallies, **across every pair**, the lag verdicts and gate outcomes — the numbers P0 needs:
//! how many gaps are `timing_offset` (a recoverable seam the gate skipped), split **constant** vs
//! **drift**, vs genuinely `decorrelated` skips.
//!
//! Parses a **minimal** projection of the schema (ids, index, geometry duration, lag, outcome) so it is
//! robust to unrelated `GapCorpus` schema drift. Prefers each pair dir's `corpus.json` (authoritative,
//! all gaps, no per-gap-file accumulation); falls back to globbing per-gap `*.json` and de-duping by
//! gap index when `corpus.json` is absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clip_sync_repair::domain::donor::PROGRAM_QUIET_SILENCE_FRAC;
use clip_sync_repair::domain::dual_fit::DUALFIT_STEP_REAL_MARGIN;
use serde::Deserialize;

// ── minimal schema projection ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct CorpusFile {
    source: SourceMeta,
    #[serde(default)]
    gaps: Vec<GapEntry>,
}

#[derive(Deserialize)]
struct SourceMeta {
    a_source: FileId,
    b_source: FileId,
}

#[derive(Deserialize)]
struct FileId {
    id: String,
}

#[derive(Deserialize)]
struct GapEntry {
    index: usize,
    #[serde(default)]
    geometry: Option<Geometry>,
    /// A-side level profile (summary levels only) — dropout depth vs program-quiet (D11).
    #[serde(default)]
    levels: Option<BLevels>,
    /// Diagnostic lag at the best editorial bracket (may sit far from the decision seam).
    #[serde(default)]
    lag: Option<Lag>,
    /// Lag at the **decision** placement (structure-slid throat) — the registration-relevant one (#2).
    #[serde(default)]
    baseline_lag: Option<Lag>,
    /// Residual cancellation at the decision seam (the strong same-source confirm).
    #[serde(default)]
    residual: Option<Residual>,
    /// Envelope/structure match at the baseline placement (bucketed, 50 ms bins, ~3 s context).
    #[serde(default)]
    structure: Option<PairScore>,
    /// Waveform seam Pearson at the throat (sample-level, ~250 ms) — the gate's actual decision seam.
    #[serde(default)]
    seams: Option<PairScore>,
    /// Per-bracket gate scores + which stage rejected each (the authoritative skip reason).
    #[serde(default)]
    brackets: Vec<Bracket>,
    /// Seam recovery / encoding-robust envelope / level at the decision seam (diagnoses dead seams).
    #[serde(default)]
    seam_probe: Option<SeamProbeFp>,
    /// Donor B energy across the gap-mapped span (bridges the hole?).
    #[serde(default)]
    donor_interior: Option<DonorInterior>,
    /// Donor occupancy at the **nominal** geometry span (no lag) — registration-independent (D11).
    #[serde(default)]
    donor_interior_nominal: Option<DonorInterior>,
    /// Symmetric B-side level profile over the nominal span (counterpart to A `levels`; D11).
    #[serde(default)]
    b_levels: Option<BLevels>,
    /// First-class splice summary (step + per-side peaks / peak_z).
    #[serde(default)]
    splice: Option<Splice>,
    /// Dual-fit repair viability at the per-shoulder placement (gate-equivalent seam score; C3/C7).
    #[serde(default)]
    splice_dualfit: Option<SpliceDualfit>,
    /// Wide (100 ms-bin) envelope segment-identity confirmer at the decision seam.
    #[serde(default)]
    wide_envelope: Option<WideEnvelopeFp>,
    #[serde(default)]
    outcome: Option<Outcome>,
}

#[derive(Deserialize)]
struct DonorInterior {
    rms_db: f64,
    silence_fraction: f64,
    continuous: bool,
}

/// Projection of the symmetric B-side `LevelProfile` (only the summary levels the analyzer reads).
#[derive(Deserialize)]
struct BLevels {
    gap_floor_db: f64,
    noise_floor_db: f64,
    #[allow(dead_code)]
    speech_peak_db: f64,
}

#[derive(Deserialize)]
struct Splice {
    step_ms: f64,
    pre_peak_r: f64,
    post_peak_r: f64,
    #[serde(default)]
    pre_peak_z: Option<f64>,
    #[serde(default)]
    post_peak_z: Option<f64>,
    /// Either shoulder's `baseline_lag` peak was search-exhausted (clipped at ±max_lag) ⇒ `step_ms` is
    /// GIGO (ledger A5/C6). `None` for fingerprints predating the edge-pin flag.
    #[serde(default)]
    edge_pinned: Option<bool>,
}

#[derive(Deserialize, Clone, Copy)]
struct SpliceDualfit {
    pre_seam_r: f64,
    post_seam_r: f64,
    #[allow(dead_code)]
    trim_frames: i64,
    gate_pass: bool,
    #[serde(default)]
    post_seam_global_r: f64,
    #[serde(default)]
    pre_seam_prom: Option<f64>,
    #[serde(default)]
    post_seam_prom: Option<f64>,
}

#[derive(Deserialize, Clone, Copy)]
struct EnvPeak {
    #[allow(dead_code)]
    peak_r: f64,
    peak_lag_ms: f64,
    #[allow(dead_code)]
    prominence: f64,
}

#[derive(Deserialize)]
struct WideEnvelopeFp {
    #[serde(default)]
    pre: Option<EnvPeak>,
    #[serde(default)]
    post: Option<EnvPeak>,
}

#[derive(Deserialize)]
struct SeamProbeFp {
    #[serde(default)]
    pre: Option<SeamProbe>,
    #[serde(default)]
    post: Option<SeamProbe>,
}

#[derive(Deserialize, Clone, Copy)]
struct SeamProbe {
    waveform_r: f64,
    recovered_r: f64,
    #[allow(dead_code)]
    recovered_lag_ms: f64,
    #[serde(default)]
    bandlimited_r: f64,
    #[serde(default)]
    spectrum_r: f64,
    envelope_r: f64,
    #[allow(dead_code)]
    rms_db: f64,
    snr_db: f64,
}

#[derive(Deserialize)]
struct PairScore {
    #[serde(default)]
    baseline_pre: Option<f64>,
    #[serde(default)]
    baseline_post: Option<f64>,
}

#[derive(Deserialize)]
struct Bracket {
    #[serde(default)]
    seam_pre: Option<f64>,
    #[serde(default)]
    seam_post: Option<f64>,
    #[serde(default)]
    failure_stage: Option<String>,
}

#[derive(Deserialize)]
struct Residual {
    // `Option` because the writer emits non-finite dB (a silent gap cancels to ~0 ⇒ `to_db(0) = -inf`) as
    // JSON `null`; a required `f64` here would fail the **whole** `corpus.json` parse and silently drop
    // every measured gap in the pair (it did — see the residual-null bug). `None` ⇒ residual unavailable.
    #[serde(default)]
    chosen_pre_db: Option<f64>,
    #[serde(default)]
    chosen_post_db: Option<f64>,
    #[serde(default)]
    floor_pre_db: Option<f64>,
    #[serde(default)]
    floor_post_db: Option<f64>,
    #[serde(default)]
    informative: bool,
}

#[derive(Deserialize)]
struct Geometry {
    #[serde(default)]
    duration_secs: Option<f64>,
    #[serde(default)]
    a_refined_start_secs: Option<f64>,
}

#[derive(Deserialize)]
struct Lag {
    #[serde(default)]
    pre_anchor: Vec<LagEntry>,
    #[serde(default)]
    post_anchor: Vec<LagEntry>,
}

/// Mirrors `gap_fingerprint::LagChannel`'s externally-tagged repr (`"mono"` / `{"selected":N}`).
#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum LagChannelTag {
    Mono,
    Selected(usize),
}

#[derive(Deserialize)]
struct LagEntry {
    /// Absent in pre-A2 fingerprints (single-entry mono-only arrays); `.first()`-style fallback below
    /// treats a missing channel as mono so old data keeps working.
    #[serde(default)]
    channel: Option<LagChannelTag>,
    peak_r: f64,
    #[serde(default)]
    second_peak_r: Option<f64>,
    /// Robust uniqueness (post-rescan): z-score of the peak over the whole lag curve. `None` for older
    /// fingerprints. Once populated this is the addressability gate (§3.6a: unique ≥ ~12 at the 1 s window).
    /// (`prominence` is also written to the fingerprint but the analyzer gates on `peak_z`.)
    #[serde(default)]
    peak_z: Option<f64>,
    /// `peak_r − second_peak_r` — prominence of the chosen lag over the tallest rival.
    #[serde(default)]
    prominence: Option<f64>,
    frac_lag_ms: f64,
    verdict: String,
}

#[derive(Deserialize)]
struct Outcome {
    #[serde(default)]
    plan_kind: Option<String>,
    tier: String,
    #[serde(default)]
    skip_reason: Option<String>,
}

// ── analysis result ─────────────────────────────────────────────────────────────

/// Constant vs drift classification for a `timing_offset` gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkewClass {
    /// Pre/post seam lags within `eps` → a single shift aligns both seams.
    Constant,
    /// Pre/post seam lags differ by more than `eps` → needs a time-warp (g003-like).
    Drift,
    /// Not a `timing_offset` gap, or lag missing on one side.
    NotApplicable,
}

/// Honest "what is this region" bucket — used so the addressable rate isn't diluted by track tails or
/// gaps with no matchable B seam. Maps onto the gap-repair-guide Layer-1 types where it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapKind {
    /// Long / end-of-file region (guide **P6**): `fillable` but duration ≥ the tail threshold. These
    /// are track-length mismatches / huge silences, not seam-repairable content — excluded from rates.
    Tail,
    /// No lag fingerprint: structure never found a matchable B bracket (genuine content gap, or
    /// structure-align fail — guide **W6/C5**). B carries nothing to time-align at the seam.
    NoLag,
    /// Has a seam lag fingerprint: B carries matchable content at the seam. **The analysis denominator.**
    Matched,
}

impl GapKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GapKind::Tail => "tail",
            GapKind::NoLag => "no-lag",
            GapKind::Matched => "matched",
        }
    }
}

/// Why a waveform seam is dead, from the seam probe (the measurement that was missing). Decides the
/// *opposite* fixes: `Misaligned` → finer alignment; `CrossEncoding` → encoding-robust validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeamDiag {
    /// Seam too quiet to score reliably (SNR below floor).
    Quiet,
    /// Raw waveform recovers under a fine ±lag ⇒ residual mis-alignment (the lag-best/R3 diagnostic;
    /// shelved as a *fix* but informative — a sub-bin shift would clear the sample-level seam).
    Misaligned,
    /// Doesn't recover by a shift, but a **cross-codec-robust** metric (R2 band-limited / R4 spectrum)
    /// agrees ⇒ same content, sample-level differs (cross-encoding); the validator-mismatch candidate.
    CrossCodec,
    /// Neither — no recovery and robust metrics low (genuinely weak / different at the seam).
    Unresolved,
}

impl SeamDiag {
    pub fn as_str(&self) -> &'static str {
        match self {
            SeamDiag::Quiet => "quiet",
            SeamDiag::Misaligned => "misaligned(R3)",
            SeamDiag::CrossCodec => "cross-codec(R2/R4)",
            SeamDiag::Unresolved => "unresolved",
        }
    }
}

/// Silence-splice classification from the **±600 ms** per-side `baseline_lag` peaks, sequentially
/// centered (ledger A2: post search is centered on `S + D_A + round(L_pre)`,
/// not the naive `S + D_A`), not the ±25 ms `seam_probe.recovered_r`, which mislabels any step > 25 ms as
/// "cross-codec". See `docs/archive/TEMP-seam-splice-dualfit-plan.md` §1/§3. Decides whether a skipped gap is the
/// addressable silence-splice (both shoulders clean at their own lag, separated by a step) or something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpliceDiag {
    /// Both shoulders align uniquely at their own lag ⇒ a silence-splice: fit each seam independently and
    /// reconcile the `step` with a length edit (the addressable case).
    Splice,
    /// Both shoulders align, but a uniqueness margin is thin ⇒ the per-side lag (and so the `step`) may be
    /// a periodic alias — confirm before trusting.
    AliasSuspect,
    /// A shoulder fails to reach the peak floor at **any** lag in ±600 ms of its sequentially-centered
    /// search ⇒ not a splice. This is the only signature that would revive a genuine cross-encoding /
    /// different-content case.
    OneSidedDead,
}

impl SpliceDiag {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpliceDiag::Splice => "splice",
            SpliceDiag::AliasSuspect => "alias-suspect",
            SpliceDiag::OneSidedDead => "one-sided-dead",
        }
    }
}

const SEAM_QUIET_SNR_DB: f64 = 6.0;
const SEAM_RECOVER_R: f64 = 0.5;
/// R2/R4 above this (with waveform dead) is the cross-codec validator-mismatch signal.
const SEAM_ROBUST_R: f64 = 0.5;
/// A shoulder must reach this `peak_r` at its own lag (over the ±600 ms `baseline_lag` sweep) to count as
/// cleanly recoverable — the both-sides-recoverable floor. Heuristic; calibrate against the corpus.
const SPLICE_MIN_PEAK_R: f64 = 0.85;
/// §3.6a robust-uniqueness floors (1 s window) — used when `peak_z` is present on the fingerprint.
/// **`peak_z` is primary** (whole-curve z-score; deflates on periodic/leveled content, so it is the
/// periodicity-robust test — ledger B3). `SPLICE_MIN_PROMINENCE` is a *tiebreaker* at a deliberately low
/// floor: prominence is a single-rival term that over-flags leveled/limited audio (2026-07-01: 9/32
/// alias-suspect were prominence-only false-flags with `peak_z` 12.6–26; 4 were gate-patched — A5/C6). The
/// 0.15 floor only catches a genuine near-duplicate rival (e.g. 6·g9 `prom 0.11`).
const SPLICE_MIN_PEAK_Z: f64 = 12.0;
const SPLICE_MIN_PROMINENCE: f64 = 0.15;

/// One gap's aggregated row.
#[derive(Debug, Clone)]
pub struct GapRow {
    pub pair: String,
    pub a_id: String,
    pub b_id: String,
    pub index: usize,
    pub duration_secs: Option<f64>,
    /// A-timeline position of the gap (refined start, s) — for the per-file offset-trend (drift) test.
    pub a_start_secs: Option<f64>,
    /// Plan classification from the fingerprint outcome (`fillable` / …). Today the fingerprint only
    /// characterizes `fillable` gaps, so this is uniform — surfaced for validation + future-proofing.
    pub plan_kind: Option<String>,
    /// Region bucket (tail / no-lag / matched) — the honest denominator selector.
    pub kind: GapKind,
    /// Lag verdict (`timing_offset` / `decorrelated` / `ambiguous`), or `None` if no lag fingerprint.
    /// **One-sided (pre-preferred):** this is the pre shoulder's verdict (or post's when pre is absent), so
    /// it can disagree with the post shoulder. It labels the gap only — for classification prefer the
    /// two-sided `splice_diag()` / `seam_step_ms()` / `skew` (C-harness-2).
    pub verdict: Option<String>,
    /// Gate outcome tier (`skip`, or a patch tier). `None` if the gap carries no outcome.
    pub outcome_tier: Option<String>,
    pub skip_reason: Option<String>,
    pub frac_lag_pre_ms: Option<f64>,
    pub frac_lag_post_ms: Option<f64>,
    pub peak_r_pre: Option<f64>,
    pub peak_r_post: Option<f64>,
    /// **Uniqueness margin** = worst (smaller) of pre/post `peak_r − second_peak_r`. Low ⇒ the lag
    /// match is ambiguous (a competing peak nearly as tall — periodic content), so the `timing_offset` /
    /// same-source verdict may be a false positive. `None` for fingerprints predating `second_peak_r`.
    pub uniqueness_margin: Option<f64>,
    /// **Robust uniqueness** = worst (smaller) of pre/post `peak_z` (peak's z-score over the lag curve).
    /// The §3.6a-validated metric that separates registration from periodicity at the 1 s window
    /// (unique ≥ ~12). `None` for fingerprints predating the rescan that captures `peak_z`. Preferred over
    /// `uniqueness_margin` once present.
    pub uniqueness_z: Option<f64>,
    /// Worst (smaller) of pre/post lag-curve **prominence** (`peak_r − second_peak_r`). Paired with
    /// `uniqueness_z` for the §3.6a addressability gate once the rescan populates both.
    pub uniqueness_prom: Option<f64>,
    /// First-class splice step (`post_lag − pre_lag`) when the fingerprint carries `splice`.
    pub splice_step_ms: Option<f64>,
    /// **`splice_step_ms` is search-exhausted** — a shoulder's `baseline_lag` peak was clipped at ±max_lag,
    /// so the step (and dual-fit per-shoulder placement) is GIGO (ledger A5/C6). `None` predates the flag.
    pub splice_edge_pinned: Option<bool>,
    /// Registration came from the **legacy diagnostic `lag`** (pre-A2 fingerprint), not the decision-seam
    /// `baseline_lag` — a *different* placement (structure throat vs `b_mapped`). Any `true` in a run means
    /// the corpus mixes pre-/post-A2 schemas and the lag reads aren't comparable (C-harness-3).
    pub registration_from_legacy_lag: bool,
    /// Donor B bridges the gap interior without a sub-floor hole (from `donor_interior.continuous`).
    pub donor_continuous: Option<bool>,
    /// Donor-interior RMS (dBFS) over the gap-mapped B span.
    pub donor_rms_db: Option<f64>,
    /// Wide-envelope (100 ms-bin) peak lag at the pre seam — cross-scale check vs `baseline_lag`.
    pub wide_env_pre_lag_ms: Option<f64>,
    pub wide_env_post_lag_ms: Option<f64>,
    /// **Residual headroom** (dB) = worst-side `chosen_db − floor_db`. ≤ 0 ⇒ B cancels A to the noise
    /// floor ⇒ genuine same source (the strong confirm); > 0 ⇒ residual above floor ⇒ B differs.
    pub residual_headroom_db: Option<f64>,
    /// Whether the residual floor was measurable (the headroom is interpretable).
    pub residual_informative: Option<bool>,
    /// `min(pre,post)` envelope/structure match at baseline — how well the **placement** matched.
    pub structure_min: Option<f64>,
    /// `min(pre,post)` waveform seam Pearson at the throat — the gate's **decision** seam.
    pub seam_min: Option<f64>,
    /// Best `min(pre,post)` waveform seam any bracket reached (how close it got to passing).
    pub best_bracket_seam: Option<f64>,
    /// Anchor/grid brackets the gate scored, and how many **passed** (no `failure_stage`). Patch/skip is
    /// **bracket-pass success, not step magnitude** (review C1): ≥1 passing ⇒ patch; 0 passing ⇒ skip. The
    /// dual-fit target is the *bracket-exhausted* skips, not all stepped gaps.
    pub brackets_total: usize,
    pub brackets_passing: usize,
    /// Failure stage of the bracket that came closest (`structure_align`/`structure_floor`/
    /// `waveform_floor`/`residual`); `None` if a bracket passed (gap patched).
    pub closest_failure_stage: Option<String>,
    /// Seam-probe metrics at the worst (most-blocking) side, and the resulting diagnosis. `None` until
    /// the corpus is re-fingerprinted with the seam probe.
    pub seam_recovered_r: Option<f64>,
    pub seam_bandlimited_r: Option<f64>,
    pub seam_spectrum_r: Option<f64>,
    pub seam_envelope_r: Option<f64>,
    pub seam_snr_db: Option<f64>,
    pub seam_diag: Option<SeamDiag>,
    pub skew: SkewClass,
    /// Dual-fit viability (scan-native `splice_dualfit`): pre/post seam Pearson at the per-shoulder
    /// placement and whether they clear the gate — "would a length-reconciled fill pass?" (C3/C7).
    pub dualfit_pre_r: Option<f64>,
    pub dualfit_post_r: Option<f64>,
    pub dualfit_pass: Option<bool>,
    /// Post seam at the pre offset (step forced 0): validates whether the step is *necessary* (real) or a
    /// single constant shift also works (spurious step / registration artifact).
    pub dualfit_post_global_r: Option<f64>,
    /// Min of the two seam-peak prominences (±30 ms): low ⇒ periodic/alias match, PASS not trustworthy.
    pub dualfit_seam_prom: Option<f64>,
    /// D11 — registration-independent B occupancy at the nominal geometry span. `silence ≈ 1` ⇒ B is quiet
    /// at the same program time as A's gap ⇒ program-quiet, not a fillable dropout.
    pub donor_nominal_silence: Option<f64>,
    pub donor_nominal_cont: Option<bool>,
    /// Donor occupancy at the *aligned* span (from `donor_interior`) — disagreement with the nominal one
    /// flags a registration that moved the span onto different content (alias signal).
    pub donor_aligned_silence: Option<f64>,
    /// B-side gap floor and noise floor (symmetric `b_levels`) — is B's gap quiet vs B's *own* floor?
    pub b_gap_floor_db: Option<f64>,
    pub b_noise_floor_db: Option<f64>,
    /// A-side gap floor (from `levels`) — deep-silent dropout vs quiet-at-noise-floor passage.
    pub a_gap_floor_db: Option<f64>,
    pub a_noise_floor_db: Option<f64>,
}

impl GapRow {
    /// Patched = the gate produced a patch tier (anything but `skip`). `None` outcome → not patched.
    pub fn patched(&self) -> bool {
        matches!(&self.outcome_tier, Some(t) if t != "skip")
    }

    /// `|frac_lag_pre − frac_lag_post|` in ms when both are present.
    pub fn drift_ms(&self) -> Option<f64> {
        Some((self.frac_lag_pre_ms? - self.frac_lag_post_ms?).abs())
    }

    /// **Registration decomposition — offset:** the signed lag at the gap centre, `(pre + post)/2`.
    /// This is "where the kept content sits in B" — a constant clip offset / clip-drift residual that a
    /// single shift would fix. (The structure search centres B on the gap, so this is the average pull.)
    pub fn seam_mid_ms(&self) -> Option<f64> {
        Some((self.frac_lag_pre_ms? + self.frac_lag_post_ms?) / 2.0)
    }

    /// **Registration decomposition — step:** the signed `post − pre`. Zero ⇒ a clean constant-offset
    /// dropout (one shift aligns both seams). A large step ⇒ the A↔B timeline *steps* at the gap — the
    /// two kept sides need different alignment (an edit / length discontinuity), recoverable by neither a
    /// shift nor a smooth warp. (Whether a large step is a *real* divergence or a spurious lag lock is
    /// what the `uniqueness_margin` then decides.)
    pub fn seam_step_ms(&self) -> Option<f64> {
        self.splice_step_ms
            .or_else(|| Some(self.frac_lag_post_ms? - self.frac_lag_pre_ms?))
    }

    /// Wide-envelope peak lag agrees with fine waveform lag within this tolerance (ms).
    pub fn wide_env_agrees(&self, tol_ms: f64) -> Option<bool> {
        let pre_ok = match (self.frac_lag_pre_ms, self.wide_env_pre_lag_ms) {
            (Some(f), Some(w)) => (f - w).abs() <= tol_ms,
            _ => true,
        };
        let post_ok = match (self.frac_lag_post_ms, self.wide_env_post_lag_ms) {
            (Some(f), Some(w)) => (f - w).abs() <= tol_ms,
            _ => true,
        };
        if self.wide_env_pre_lag_ms.is_none() && self.wide_env_post_lag_ms.is_none() {
            None
        } else {
            Some(pre_ok && post_ok)
        }
    }

    /// **Silence-splice classification** from the ±600 ms per-side `baseline_lag` peaks — the
    /// authoritative read (the ±25 ms `seam_probe.recovered_r` mislabels any step > 25 ms). `Splice` =
    /// both shoulders clean & unique ⇒ addressable by independent fit + length reconciliation;
    /// `OneSidedDead` = a shoulder aligns at no lag ⇒ the only genuine cross-encoding candidate. `None`
    /// when a side has no lag peak (can't judge).
    pub fn splice_diag(&self) -> Option<SpliceDiag> {
        let (pre, post) = (self.peak_r_pre?, self.peak_r_post?);
        if pre.min(post) < SPLICE_MIN_PEAK_R {
            return Some(SpliceDiag::OneSidedDead);
        }
        // §3.6a: `peak_z` is the primary (periodicity-robust) uniqueness test; prominence is only a
        // low-floor tiebreaker for genuine near-duplicate rivals (see `SPLICE_MIN_PROMINENCE`). Prefer both
        // when the rescan captured them on both sides.
        if let (Some(z), Some(prom)) = (self.uniqueness_z, self.uniqueness_prom) {
            if z < SPLICE_MIN_PEAK_Z || prom < SPLICE_MIN_PROMINENCE {
                return Some(SpliceDiag::AliasSuspect);
            }
            return Some(SpliceDiag::Splice);
        }
        if self.uniqueness_margin.is_some_and(|m| m < LOW_UNIQUENESS_MARGIN) {
            Some(SpliceDiag::AliasSuspect)
        } else {
            Some(SpliceDiag::Splice)
        }
    }

    /// Both shoulders reach the peak floor at their own lag AND the match is unique ⇒ the step is a
    /// trustworthy splice amount the length-reconciliation repair can act on.
    pub fn both_sides_recoverable(&self) -> bool {
        self.splice_diag() == Some(SpliceDiag::Splice)
    }

    /// The gate scored brackets but **none passed** — anchor/boundary search is exhausted. This (not step
    /// magnitude) is what makes a gap skip (review C1). `false` for patched gaps and gaps with no brackets.
    pub fn bracket_exhausted(&self) -> bool {
        self.brackets_total > 0 && self.brackets_passing == 0
    }

    /// The measured step (and its per-shoulder placement) is **search-exhausted** — a `baseline_lag` peak
    /// was clipped at ±max_lag, so `splice_step_ms` and dual-fit shoulder lags are GIGO (ledger A5/C6).
    /// `false` when the flag is absent (older fingerprint) or explicitly clear; widen the sweep to resolve.
    pub fn step_edge_pinned(&self) -> bool {
        self.splice_edge_pinned == Some(true)
    }

    /// A gap dual-fit should target: a **skip** whose brackets are exhausted (no single placement passes)
    /// yet **both shoulders recover at their own lag** — the residual that a per-seam fit + length
    /// reconciliation could rescue, where today's boundary search cannot. Distinct from a gap that already
    /// patches (≥1 bracket passes) — dual-fit must NOT run on those. An **edge-pinned** step is excluded:
    /// its per-shoulder placement is clipped at the search boundary, so the dual-fit lags can't be trusted
    /// until the sweep is widened (ledger A5/C6 — the GIGO guard). A **program-quiet** gap (D11 — B silent
    /// at the same program time) is excluded too: there is nothing to fill, so it is not a repair target.
    pub fn dualfit_candidate(&self) -> bool {
        self.outcome_tier.as_deref() == Some("skip")
            && self.bracket_exhausted()
            && self.both_sides_recoverable()
            && !self.step_edge_pinned()
            && self.program_quiet() != Some(true)
    }

    /// **The A3 repair-scope predicate** — the gaps a dual-fit repair should actually run on, measured
    /// directly rather than proxied by uniqueness. A gap is a target when the scan-native `splice_dualfit`
    /// shows: the seams **pass** the unchanged gate (`dualfit_pass`); the **step is real** ([`step_is_real`] —
    /// the step materially improves the post seam vs a constant offset); the **donor bridges** the hole
    /// (`donor_continuous`); and it is **not program-quiet** (there is content to fill). This supersedes
    /// `dualfit_candidate` (uniqueness), which mispredicts placement seam viability — edge-pin/D11 rescan:
    /// the two overlap in only 2/7 (ledger A3). `None` fields ⇒ not a target (can't confirm).
    ///
    /// Only a **bracket-exhausted skip** qualifies: dual-fit must NOT run on gaps that already patch (a
    /// passing bracket exists) — B1/B11.
    pub fn dualfit_target(&self) -> bool {
        self.outcome_tier.as_deref() == Some("skip")
            && self.bracket_exhausted()
            && self.dualfit_pass == Some(true)
            && self.step_is_real()
            && self.donor_continuous == Some(true)
            && self.program_quiet() != Some(true)
    }

    /// The registration **step is real** (necessary), not a constant-offset artifact: placing the post seam
    /// at its own lag beats placing it at the pre lag (step = 0) by ≥ [`DUALFIT_STEP_REAL_MARGIN`]. `false`
    /// when either seam is unmeasured. Replaces the old `post@pre < 0.35` floor, which mis-flagged gaps whose
    /// constant offset merely cleared the gate floor (7·g4).
    pub fn step_is_real(&self) -> bool {
        match (self.dualfit_post_r, self.dualfit_post_global_r) {
            (Some(own), Some(at_pre)) => own - at_pre >= DUALFIT_STEP_REAL_MARGIN,
            _ => false,
        }
    }

    /// D11 — a **skip** that is program-quiet (B silent at the same program time). Correctly skipped and
    /// **not a fill miss**: it leaves the addressable-dropout denominator (there is nothing to fill), rather
    /// than counting against the dual-fit / repair rate. `false` when occupancy wasn't captured.
    pub fn program_quiet_skip(&self) -> bool {
        self.outcome_tier.as_deref() == Some("skip") && self.program_quiet() == Some(true)
    }

    /// D11 classifier — is this matched gap an **addressable dropout** (a real hole to fill), as opposed to a
    /// program-quiet passage (quiet in both masters)? A program-quiet gap is *not* addressable; when
    /// occupancy is uncaptured (`None`) the gap stays addressable (backward-compatible — no silent drops).
    pub fn addressable_dropout(&self) -> bool {
        self.kind == GapKind::Matched && self.program_quiet() != Some(true)
    }

    /// D11 — is this "gap" a **program-quiet** passage (quiet in *both* masters) rather than a fillable
    /// dropout? Uses the registration-independent nominal-span B occupancy: `silence ≈ 1` ⇒ B is quiet at
    /// the same program time as A's gap ⇒ nothing to fill, not a repair failure. `None` if not captured.
    pub fn program_quiet(&self) -> Option<bool> {
        self.donor_nominal_silence.map(|s| s >= PROGRAM_QUIET_SILENCE_FRAC)
    }

    /// Nominal vs aligned donor occupancy disagree strongly ⇒ the per-shoulder registration moved the span
    /// onto different content — a registration/alias smell independent of `peak_z`. `None` if either absent.
    pub fn donor_span_disagrees(&self) -> Option<bool> {
        match (self.donor_nominal_silence, self.donor_aligned_silence) {
            (Some(n), Some(a)) => Some((n - a).abs() >= 0.5),
            _ => None,
        }
    }

    /// Largest seam-offset magnitude (ms) seen on either side — recoverability needs this within the
    /// lag search window.
    pub fn max_offset_ms(&self) -> Option<f64> {
        match (self.frac_lag_pre_ms, self.frac_lag_post_ms) {
            (Some(a), Some(b)) => Some(a.abs().max(b.abs())),
            (Some(a), None) => Some(a.abs()),
            (None, Some(b)) => Some(b.abs()),
            (None, None) => None,
        }
    }
}

// The dual-fit **step-real margin**: the step is *necessary* (a real splice, not a registration artifact)
// only when placing the post seam at its own lag beats placing it at the pre lag (step forced to 0) by at
// least this much — i.e. the step **materially improves** the seam. The earlier `post@pre < 0.35` floor was
// wrong: it flagged the step spurious whenever the constant offset merely *cleared* the gate floor, dropping
// `DUALFIT_STEP_REAL_MARGIN` (0.15) — the step-is-real margin — now lives canonically in
// `clip_sync_repair::domain::dual_fit` (imported at the top; single source of truth, no drift). Rationale:
// a true artifact reads `post_own ≈ post@pre` (Δ ≈ 0); 7·g4 barely-real drops (`post@pre 0.393`, `post_own
// 0.96`, Δ 0.57) confirm the 0.15 floor. Calibrate at ledger A5/C6.

/// Full aggregation across every pair dir found.
#[derive(Debug, Clone, Default)]
pub struct CorpusReport {
    pub rows: Vec<GapRow>,
    pub pairs: Vec<String>,
    /// `eps` (ms) used for the constant/drift split.
    pub drift_eps_ms: f64,
    /// Duration (s) at/above which a `fillable` gap is bucketed as a `Tail` (guide P6).
    pub tail_secs: f64,
}

const DEFAULT_DRIFT_EPS_MS: f64 = 1.0;
const DEFAULT_TAIL_SECS: f64 = 30.0;
/// A uniqueness margin below this flags a same-source verdict as **periodicity-suspect** (a competing
/// lag peak nearly as tall). Heuristic; calibrate against the corpus.
const LOW_UNIQUENESS_MARGIN: f64 = 0.30;
/// Below this `|seam step|` (ms) the gap is a **clean constant-offset** dropout (one shift aligns both
/// seams); above it the A↔B timeline steps at the gap. Heuristic; calibrate against the corpus.
const CLEAN_STEP_MS: f64 = 2.0;

/// Aggregate every A/B-pair `corpus.json` found under `roots` (each root is scanned for its own
/// `corpus.json` and for immediate subdirs that have one). `drift_eps_ms` splits constant vs drift;
/// `tail_secs` is the long-tail (P6) duration cutoff.
pub fn analyze_dirs(roots: &[PathBuf], drift_eps_ms: f64, tail_secs: f64) -> CorpusReport {
    let mut report = CorpusReport {
        drift_eps_ms,
        tail_secs,
        ..Default::default()
    };
    for root in roots {
        for pair_dir in pair_dirs(root) {
            let label = pair_label(root, &pair_dir);
            match read_pair(&pair_dir) {
                Some(file) => {
                    report.pairs.push(label.clone());
                    for gap in &file.gaps {
                        report
                            .rows
                            .push(gap_row(&label, &file.source, gap, drift_eps_ms, tail_secs));
                    }
                }
                None => eprintln!("warn: no parseable corpus in {}", pair_dir.display()),
            }
        }
    }
    report
}

/// A pair dir holds either a `corpus.json` or loose per-gap `*.json` library files. `root` itself
/// counts, as do its immediate subdirs (so `gap-files/` with `1/`..`6/` resolves to six pairs in one
/// call).
fn pair_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if is_pair_dir(root) {
        out.push(root.to_path_buf());
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        let mut subs: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && is_pair_dir(p))
            .collect();
        subs.sort();
        out.extend(subs);
    }
    out
}

/// True if `dir` holds a `corpus.json`, or any non-`manifest` `*.json` (per-gap library files).
fn is_pair_dir(dir: &Path) -> bool {
    if dir.join("corpus.json").is_file() {
        return true;
    }
    std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let p = e.path();
            p.extension().and_then(|x| x.to_str()) == Some("json")
                && p.file_name().and_then(|n| n.to_str()) != Some("manifest.json")
        })
}

fn pair_label(root: &Path, pair_dir: &Path) -> String {
    pair_dir
        .strip_prefix(root.parent().unwrap_or(root))
        .unwrap_or(pair_dir)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Read a pair dir: prefer `corpus.json`; else merge per-gap `*.json`, de-duping by gap index.
fn read_pair(dir: &Path) -> Option<CorpusFile> {
    if let Some(file) = read_corpus_json(&dir.join("corpus.json")) {
        return Some(file);
    }
    // Fallback: per-gap library files (each a single-gap corpus). Dedup by index.
    let mut source: Option<SourceMeta> = None;
    let mut seen: BTreeMap<usize, GapEntry> = BTreeMap::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if p.file_name().and_then(|n| n.to_str()) == Some("manifest.json") {
            continue;
        }
        if let Some(file) = read_corpus_json(&p) {
            source.get_or_insert(file.source);
            for gap in file.gaps {
                seen.entry(gap.index).or_insert(gap);
            }
        }
    }
    Some(CorpusFile {
        source: source?,
        gaps: seen.into_values().collect(),
    })
}

fn read_corpus_json(path: &Path) -> Option<CorpusFile> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn gap_row(pair: &str, source: &SourceMeta, gap: &GapEntry, eps: f64, tail_secs: f64) -> GapRow {
    // Registration is read at the **decision** seam (`baseline_lag`, #2); fall back to the diagnostic
    // best-bracket `lag` for older fingerprints that predate it. Select the mono entry explicitly (P2-1):
    // capture pushes mono first today, but `.first()` silently picks whatever is first if channel order
    // ever changes — match on `channel` instead, falling back to `.first()` only when `channel` is absent
    // (pre-A2 fingerprints, which only ever wrote one mono entry anyway).
    fn mono_entry(v: &[LagEntry]) -> Option<&LagEntry> {
        v.iter()
            .find(|e| e.channel == Some(LagChannelTag::Mono))
            .or_else(|| v.first())
    }
    // Registration prefers the decision-seam `baseline_lag` (b_mapped, ledger A2); the diagnostic
    // best-bracket `lag` is only a **legacy fallback** for pre-A2 fingerprints and sits at a *different*
    // placement (structure throat). Flag when a row falls back so a run mixing pre-/post-A2 corpora is
    // visible instead of silently conflating placements (C-harness-3).
    let registration_from_legacy_lag = gap.baseline_lag.is_none() && gap.lag.is_some();
    let lag = gap.baseline_lag.as_ref().or(gap.lag.as_ref());
    let pre = lag.and_then(|l| mono_entry(&l.pre_anchor));
    let post = lag.and_then(|l| mono_entry(&l.post_anchor));
    let verdict = pre.or(post).map(|s| s.verdict.clone());
    let frac_lag_pre_ms = pre.map(|s| s.frac_lag_ms);
    let frac_lag_post_ms = post.map(|s| s.frac_lag_ms);
    let duration_secs = gap.geometry.as_ref().and_then(|g| g.duration_secs);
    let has_lag = lag.is_some();
    let seam = gap.seam_probe.as_ref().and_then(worst_seam_side);

    // Uniqueness margin: peak_r − second_peak_r per seam; the worst (min) over pre/post is the gap's.
    let margin = |s: &LagEntry| s.second_peak_r.map(|sp| s.peak_r - sp);
    let worst = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    let uniqueness_margin = worst(pre.and_then(margin), post.and_then(margin));
    // **Robust uniqueness is a both-shoulders test** (C-harness-1): a splice is only trustworthy if *both*
    // seams stand out, so take the min over pre/post only when BOTH sides carry the metric. Falling back to
    // a single present side (the old `worst()`) over-states uniqueness for a gap missing a shoulder z.
    let both = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        _ => None,
    };
    let base_uniqueness_z = both(pre.and_then(|s| s.peak_z), post.and_then(|s| s.peak_z));
    let uniqueness_prom = worst(pre.and_then(|s| s.prominence), post.and_then(|s| s.prominence));

    // First-class splice / donor / wide-envelope when present (post-rescan).
    let splice = gap.splice.as_ref();
    let peak_r_pre = splice.map(|s| s.pre_peak_r).or_else(|| pre.map(|s| s.peak_r));
    let peak_r_post = splice.map(|s| s.post_peak_r).or_else(|| post.map(|s| s.peak_r));
    let splice_step_ms = splice.map(|s| s.step_ms);
    let splice_edge_pinned = splice.and_then(|s| s.edge_pinned);
    // Prefer the splice's per-shoulder z (same `baseline_lag` source), but stay two-sided: fall back to the
    // lag-entry both-sided z only when the splice carries neither shoulder.
    let uniqueness_z = match splice {
        Some(s) => both(s.pre_peak_z, s.post_peak_z).or(base_uniqueness_z),
        None => base_uniqueness_z,
    };

    // **Skew is a two-sided classification** (C-harness-2): constant-vs-drift needs *both* shoulders, and
    // both must read `timing_offset`. The headline `verdict` is pre-preferred (one-sided) and only labels
    // the gap; don't let it drive skew when the shoulders disagree or one is missing.
    let both_timing = pre.is_some_and(|s| s.verdict == "timing_offset")
        && post.is_some_and(|s| s.verdict == "timing_offset");
    let skew = match (both_timing, frac_lag_pre_ms, frac_lag_post_ms) {
        (true, Some(a), Some(b)) if (a - b).abs() <= eps => SkewClass::Constant,
        (true, Some(_), Some(_)) => SkewClass::Drift,
        _ => SkewClass::NotApplicable,
    };

    // Tail by duration takes precedence (a P6 region isn't seam-repairable regardless of a lag read).
    let kind = if duration_secs.is_some_and(|d| d >= tail_secs) {
        GapKind::Tail
    } else if has_lag {
        GapKind::Matched
    } else {
        GapKind::NoLag
    };

    GapRow {
        pair: pair.to_string(),
        a_id: source.a_source.id.clone(),
        b_id: source.b_source.id.clone(),
        index: gap.index,
        duration_secs,
        a_start_secs: gap.geometry.as_ref().and_then(|g| g.a_refined_start_secs),
        plan_kind: gap.outcome.as_ref().and_then(|o| o.plan_kind.clone()),
        kind,
        verdict,
        outcome_tier: gap.outcome.as_ref().map(|o| o.tier.clone()),
        skip_reason: gap.outcome.as_ref().and_then(|o| o.skip_reason.clone()),
        frac_lag_pre_ms,
        frac_lag_post_ms,
        peak_r_pre,
        peak_r_post,
        uniqueness_margin,
        uniqueness_z,
        uniqueness_prom,
        splice_step_ms,
        splice_edge_pinned,
        registration_from_legacy_lag,
        donor_continuous: gap.donor_interior.as_ref().map(|d| d.continuous),
        donor_rms_db: gap.donor_interior.as_ref().map(|d| d.rms_db),
        wide_env_pre_lag_ms: gap.wide_envelope.as_ref().and_then(|w| w.pre.map(|p| p.peak_lag_ms)),
        wide_env_post_lag_ms: gap.wide_envelope.as_ref().and_then(|w| w.post.map(|p| p.peak_lag_ms)),
        // Worst (least-cancelling) side: max of the two headrooms. `None` if any dB was null (non-finite).
        residual_headroom_db: gap.residual.as_ref().and_then(|r| {
            match (r.chosen_pre_db, r.floor_pre_db, r.chosen_post_db, r.floor_post_db) {
                (Some(cp), Some(fp), Some(cq), Some(fq)) => Some((cp - fp).max(cq - fq)),
                _ => None,
            }
        }),
        residual_informative: gap.residual.as_ref().map(|r| r.informative),
        structure_min: gap.structure.as_ref().and_then(pair_min),
        seam_min: gap.seams.as_ref().and_then(pair_min),
        best_bracket_seam: gap
            .brackets
            .iter()
            .filter_map(|b| match (b.seam_pre, b.seam_post) {
                (Some(a), Some(c)) => Some(a.min(c)),
                _ => None,
            })
            .fold(None, |acc, v| Some(acc.map_or(v, |a: f64| a.max(v)))),
        brackets_total: gap.brackets.len(),
        brackets_passing: gap.brackets.iter().filter(|b| b.failure_stage.is_none()).count(),
        // Failure stage of the bracket with the highest min-seam (the closest to passing).
        closest_failure_stage: gap
            .brackets
            .iter()
            .filter(|b| b.failure_stage.is_some())
            .max_by(|x, y| {
                let mn = |b: &Bracket| match (b.seam_pre, b.seam_post) {
                    (Some(a), Some(c)) => a.min(c),
                    _ => f64::NEG_INFINITY,
                };
                mn(x).partial_cmp(&mn(y)).unwrap_or(std::cmp::Ordering::Equal)
            })
            .and_then(|b| b.failure_stage.clone()),
        seam_recovered_r: seam.map(|p| p.recovered_r),
        seam_bandlimited_r: seam.map(|p| p.bandlimited_r),
        seam_spectrum_r: seam.map(|p| p.spectrum_r),
        seam_envelope_r: seam.map(|p| p.envelope_r),
        seam_snr_db: seam.map(|p| p.snr_db),
        seam_diag: seam.map(seam_diag),
        skew,
        dualfit_pre_r: gap.splice_dualfit.as_ref().map(|d| d.pre_seam_r),
        dualfit_post_r: gap.splice_dualfit.as_ref().map(|d| d.post_seam_r),
        dualfit_pass: gap.splice_dualfit.as_ref().map(|d| d.gate_pass),
        dualfit_post_global_r: gap.splice_dualfit.as_ref().map(|d| d.post_seam_global_r),
        dualfit_seam_prom: gap.splice_dualfit.as_ref().and_then(|d| match (d.pre_seam_prom, d.post_seam_prom) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }),
        donor_nominal_silence: gap.donor_interior_nominal.as_ref().map(|d| d.silence_fraction),
        donor_nominal_cont: gap.donor_interior_nominal.as_ref().map(|d| d.continuous),
        donor_aligned_silence: gap.donor_interior.as_ref().map(|d| d.silence_fraction),
        b_gap_floor_db: gap.b_levels.as_ref().map(|l| l.gap_floor_db),
        b_noise_floor_db: gap.b_levels.as_ref().map(|l| l.noise_floor_db),
        a_gap_floor_db: gap.levels.as_ref().map(|l| l.gap_floor_db),
        a_noise_floor_db: gap.levels.as_ref().map(|l| l.noise_floor_db),
    }
}

/// The worst (most-blocking) side of a seam probe: the present side with the lower `waveform_r`.
fn worst_seam_side(p: &SeamProbeFp) -> Option<SeamProbe> {
    match (p.pre, p.post) {
        (Some(a), Some(b)) => Some(if a.waveform_r <= b.waveform_r { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Classify a dead waveform seam from the probe — the measurement that was missing. Simplest-fix-first:
/// quiet → a sub-bin shift recovers the raw waveform (R3) → a cross-codec-robust metric agrees (R2/R4)
/// → none.
fn seam_diag(p: SeamProbe) -> SeamDiag {
    if p.snr_db < SEAM_QUIET_SNR_DB {
        SeamDiag::Quiet
    } else if p.recovered_r >= SEAM_RECOVER_R {
        SeamDiag::Misaligned
    } else if p.bandlimited_r.max(p.spectrum_r) >= SEAM_ROBUST_R {
        SeamDiag::CrossCodec
    } else {
        SeamDiag::Unresolved
    }
}

fn pair_min(p: &PairScore) -> Option<f64> {
    match (p.baseline_pre, p.baseline_post) {
        (Some(a), Some(b)) => Some(a.min(b)),
        _ => None,
    }
}

// ── reporting ───────────────────────────────────────────────────────────────────

fn pct(n: usize, total: usize) -> String {
    if total == 0 {
        "  0.0%".to_string()
    } else {
        format!("{:5.1}%", 100.0 * n as f64 / total as f64)
    }
}

fn stats(mut v: Vec<f64>) -> Option<(f64, f64, f64)> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min = v[0];
    let max = v[v.len() - 1];
    let med = v[v.len() / 2];
    Some((min, med, max))
}

/// Least-squares line `y = slope·x + b`; returns `(slope, residual_std)`. `None` for < 2 points or no
/// x-spread. Used to test the offset for **clip drift** (a real drift is a consistent slope vs gap time
/// with small residual; a per-gap-local offset has a large residual around any line).
fn linfit(pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let nf = n as f64;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |(ax, ay), &(x, y)| (ax + x, ay + y));
    let (mx, my) = (sx / nf, sy / nf);
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for &(x, y) in pts {
        sxx += (x - mx) * (x - mx);
        sxy += (x - mx) * (y - my);
    }
    if sxx <= f64::EPSILON {
        return None;
    }
    let slope = sxy / sxx;
    let b = my - slope * mx;
    let resid = (pts.iter().map(|&(x, y)| (y - (slope * x + b)).powi(2)).sum::<f64>() / nf).sqrt();
    Some((slope, resid))
}

/// RMS of each value folded into `[−q/2, q/2]` — small ⇒ the values cluster near multiples of `q` (a
/// dropped/duplicated buffer or video frame leaves the step quantized to a block size).
fn quantization_residual(values: &[f64], q: f64) -> f64 {
    if values.is_empty() || q <= 0.0 {
        return f64::INFINITY;
    }
    let folded: f64 = values
        .iter()
        .map(|&v| {
            let r = v.rem_euclid(q);
            let r = if r > q / 2.0 { r - q } else { r };
            r * r
        })
        .sum();
    (folded / values.len() as f64).sqrt()
}

impl CorpusReport {
    pub fn total_gaps(&self) -> usize {
        self.rows.len()
    }

    fn count<F: Fn(&GapRow) -> bool>(&self, f: F) -> usize {
        self.rows.iter().filter(|r| f(r)).count()
    }

    /// Rows with a matchable B seam (the honest analysis denominator — excludes tails and no-lag gaps).
    pub fn matched(&self) -> Vec<&GapRow> {
        self.rows.iter().filter(|r| r.kind == GapKind::Matched).collect()
    }

    /// Human summary aimed at the P0 go/no-go. The denominator is **matched** gaps (B carries seam
    /// content); tails (P6) and no-lag gaps are bucketed out so the addressable rate isn't diluted.
    pub fn summary_text(&self) -> String {
        use std::fmt::Write;
        let n = self.total_gaps();
        let mut s = String::new();
        let _ = writeln!(
            s,
            "=== gap-fingerprint corpus: {} pair(s), {n} gap(s) (tail ≥ {:.0}s, drift eps {:.1} ms) ===",
            self.pairs.len(),
            self.tail_secs,
            self.drift_eps_ms
        );

        // C-harness-3: warn loudly when the corpus mixes pre-/post-A2 schemas — the legacy `lag` fallback
        // sits at a different placement than `baseline_lag`, so the two aren't comparable in one run.
        let legacy = self.count(|r| r.registration_from_legacy_lag);
        if legacy > 0 {
            let _ = writeln!(
                s,
                "  ⚠ registration schema mix: {legacy}/{n} row(s) fell back to the legacy diagnostic `lag` \
                 (pre-A2, structure-throat placement) — do NOT compare their lags with `baseline_lag` rows."
            );
        }

        // Plan kind (vocabulary validation — today the fingerprint only emits `fillable`).
        let mut plan: BTreeMap<String, usize> = BTreeMap::new();
        for r in &self.rows {
            *plan.entry(r.plan_kind.clone().unwrap_or_else(|| "(none)".into())).or_default() += 1;
        }
        let plan_str: Vec<String> = plan.iter().map(|(k, c)| format!("{k} {c}")).collect();
        let _ = writeln!(s, "plan_kind: {}", plan_str.join(" · "));

        // Region kind — the denominator selector.
        let tail = self.count(|r| r.kind == GapKind::Tail);
        let nolag = self.count(|r| r.kind == GapKind::NoLag);
        let matched = self.matched();
        let m = matched.len();
        let _ = writeln!(
            s,
            "gap kind:  matched {m} · no-lag {nolag} · tail {tail}   [analysis denominator = matched]"
        );

        // Verdict mix among matched.
        let mut verdicts: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &matched {
            *verdicts.entry(r.verdict.as_deref().unwrap_or("(none)")).or_default() += 1;
        }
        let vstr: Vec<String> = verdicts.iter().map(|(v, c)| format!("{v} {c} ({})", pct(*c, m))).collect();
        let _ = writeln!(s, "── among matched ({m}) ──");
        let _ = writeln!(s, "  verdict: {}", vstr.join(" · "));

        // Uniqueness — how trustworthy the same-source verdicts are (low margin ⇒ a competing lag peak
        // nearly as tall ⇒ possible periodic false positive). Guards the "all same-source" headline.
        let margins: Vec<f64> = matched.iter().filter_map(|r| r.uniqueness_margin).collect();
        if margins.is_empty() {
            let _ = writeln!(
                s,
                "  uniqueness: (no second_peak_r — re-fingerprint with the current binary to populate)"
            );
        } else if let Some((mn, md, _)) = stats(margins.clone()) {
            let suspect = margins.iter().filter(|&&v| v < LOW_UNIQUENESS_MARGIN).count();
            let _ = writeln!(
                s,
                "  uniqueness: {suspect}/{} periodicity-suspect (margin < {:.2}); margin min {mn:.2} / median {md:.2}",
                margins.len(),
                LOW_UNIQUENESS_MARGIN
            );
        }

        // Residual — the strong same-source confirm. Cross-checks the lag verdict: does B actually
        // cancel A to the noise floor (informative & headroom ≤ 0)?
        let with_resid: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.residual_headroom_db.is_some())
            .collect();
        if with_resid.is_empty() {
            let _ = writeln!(
                s,
                "  residual: (no residual probe — re-fingerprint with the current binary to populate)"
            );
        } else {
            let confirmed = with_resid
                .iter()
                .filter(|r| {
                    r.residual_informative == Some(true)
                        && r.residual_headroom_db.is_some_and(|h| h <= 0.0)
                })
                .count();
            let headrooms: Vec<f64> = with_resid.iter().filter_map(|r| r.residual_headroom_db).collect();
            if let Some((mn, md, mx)) = stats(headrooms) {
                let _ = writeln!(
                    s,
                    "  residual: {confirmed}/{} same-source-confirmed (informative & headroom ≤ 0); headroom dB min {mn:.1} / median {md:.1} / max {mx:.1}",
                    with_resid.len()
                );
            }
        }

        // Outcome among matched.
        let mpatched = matched.iter().filter(|r| r.patched()).count();
        let mskipped = matched.iter().filter(|r| r.outcome_tier.as_deref() == Some("skip")).count();
        let _ = writeln!(
            s,
            "  outcome: patched {mpatched} ({}) · skipped {mskipped} ({})",
            pct(mpatched, m),
            pct(mskipped, m)
        );

        // Headline: addressable = matched + timing_offset + skipped.
        let addr: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| {
                r.verdict.as_deref() == Some("timing_offset")
                    && r.outcome_tier.as_deref() == Some("skip")
            })
            .collect();
        let constant = addr.iter().filter(|r| r.skew == SkewClass::Constant).count();
        let drift = addr.iter().filter(|r| r.skew == SkewClass::Drift).count();
        let _ = writeln!(
            s,
            "  addressable (timing_offset AND skipped): {} ({} of matched)",
            addr.len(),
            pct(addr.len(), m)
        );
        let _ = writeln!(s, "    constant (single shift): {constant}   drift (needs time-warp): {drift}");
        if let Some((mn, md, mx)) = stats(addr.iter().filter_map(|r| r.drift_ms()).collect()) {
            let _ = writeln!(s, "    pre↔post drift ms  min {mn:.2} / median {md:.2} / max {mx:.2}");
        }
        if let Some((mn, md, mx)) = stats(addr.iter().filter_map(|r| r.max_offset_ms()).collect()) {
            let _ = writeln!(s, "    seam offset ms     min {mn:.2} / median {md:.2} / max {mx:.2}");
        }
        if let Some((mn, md, _)) = stats(
            addr.iter()
                .filter_map(|r| match (r.peak_r_pre, r.peak_r_post) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    _ => None,
                })
                .collect(),
        ) {
            let _ = writeln!(s, "    min(peak_r) pre/post  worst {mn:.3} / median {md:.3} (recover floor 0.5)");
        }

        // Contrast: decorrelated skips (genuinely unfixable).
        let decorr_skipped = matched
            .iter()
            .filter(|r| {
                r.verdict.as_deref() == Some("decorrelated")
                    && r.outcome_tier.as_deref() == Some("skip")
            })
            .count();
        let _ = writeln!(s, "    (contrast) decorrelated AND skipped = {decorr_skipped}");

        // Registration decomposition (decision seam): offset (where in B — a shiftable clip-drift
        // residual) vs step (the pre↔post discontinuity). `step ≈ 0` ⇒ clean constant offset; large
        // `step` ⇒ the A↔B timeline steps at the gap (an edit/length divergence, not shift- or
        // warp-recoverable). NOTE: a large step alone does not prove real divergence vs a spurious lag
        // lock — `uniqueness_margin` decides that (needs the current binary's `second_peak_r`).
        let steps_abs: Vec<f64> = matched.iter().filter_map(|r| r.seam_step_ms().map(f64::abs)).collect();
        if !steps_abs.is_empty() {
            let clean = steps_abs.iter().filter(|&&v| v < CLEAN_STEP_MS).count();
            let _ = writeln!(
                s,
                "  registration: {clean}/{} clean (|step| < {:.0} ms, ≈ constant offset); {} stepped",
                steps_abs.len(),
                CLEAN_STEP_MS,
                steps_abs.len() - clean
            );
            if let Some((mn, md, mx)) = stats(matched.iter().filter_map(|r| r.seam_mid_ms().map(f64::abs)).collect()) {
                let _ = writeln!(s, "    |offset| ms (shiftable): min {mn:.1} / median {md:.1} / max {mx:.1}");
            }
            if let Some((mn, md, mx)) = stats(steps_abs.clone()) {
                let _ = writeln!(s, "    |step| ms (divergence) : min {mn:.1} / median {md:.1} / max {mx:.1}");
            }
        }

        // Per-pair breakdown (matched / skipped-matched).
        let _ = writeln!(s, "per pair (matched / skipped-matched):");
        let mut by_pair: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for r in &matched {
            let e = by_pair.entry(r.pair.as_str()).or_default();
            e.0 += 1;
            if r.outcome_tier.as_deref() == Some("skip") {
                e.1 += 1;
            }
        }
        for (pair, (mm, sk)) in &by_pair {
            let _ = writeln!(s, "  {pair:<20} {mm:>3} / {sk:>3}");
        }
        s
    }

    /// **Mechanism checks** — is the pre↔post step *clip drift* or a *local discontinuity* (dropped
    /// buffer / frame)? Two tests: (1) is the offset clip drift (a consistent slope vs gap time per
    /// file)? (2) does the step cluster near a sample-block / video-frame size (the buffer-drop tell)?
    pub fn mechanism_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(s, "=== mechanism: is the step clip drift, or a local discontinuity? ===");

        // (1) Offset trend per pair — clip drift ⇒ a consistent slope with small residual.
        let _ = writeln!(s, "offset (mid) vs gap time, per pair  [drift ⇒ residual ≪ offset-spread]:");
        let mut by_pair: BTreeMap<&str, Vec<(f64, f64)>> = BTreeMap::new();
        for r in self.matched() {
            if let (Some(t), Some(mid)) = (r.a_start_secs, r.seam_mid_ms()) {
                by_pair.entry(r.pair.as_str()).or_default().push((t, mid));
            }
        }
        for (pair, pts) in &by_pair {
            let spread = stats(pts.iter().map(|&(_, y)| y).collect())
                .map(|(mn, _, mx)| mx - mn)
                .unwrap_or(0.0);
            let xspan = {
                let xs: Vec<f64> = pts.iter().map(|&(x, _)| x).collect();
                stats(xs).map(|(mn, _, mx)| mx - mn).unwrap_or(0.0)
            };
            match linfit(pts) {
                // Drift only if the line's rise over the file (|slope|·span) explains most of the
                // offset spread. A ~0 slope (offset scattered, no time trend) is *not* drift. Fewer
                // than 4 gaps can't tell a line from scatter — don't render a verdict.
                Some((slope, resid)) => {
                    let explained = slope.abs() * xspan;
                    let verdict = if pts.len() < 4 {
                        "too few to judge"
                    } else if explained > 0.5 * spread && spread > 4.0 {
                        "drift-like"
                    } else {
                        "local (not drift)"
                    };
                    let _ = writeln!(
                        s,
                        "  {pair:<10} {} gaps: slope {slope:+.3} ms/s → {explained:.1} ms over file, residual {resid:.1} ms, spread {spread:.1} ms  → {verdict}",
                        pts.len()
                    );
                }
                None => {
                    let _ = writeln!(s, "  {pair:<10} (too few gaps)");
                }
            }
        }

        // (2) Step quantization — a dropped/duplicated buffer or frame leaves |step| near a block size.
        let steps: Vec<f64> = self
            .matched()
            .iter()
            .filter_map(|r| r.seam_step_ms().map(f64::abs))
            .filter(|&v| v > CLEAN_STEP_MS)
            .collect();
        let _ = writeln!(
            s,
            "step quantization over {} stepped gaps  [ratio = residual / chance (q/√12); ≪ 1 ⇒ quantized]:",
            steps.len()
        );
        let candidates: [(&str, f64); 7] = [
            ("512 smp", 512.0 / 48.0),
            ("1024 smp", 1024.0 / 48.0),
            ("2048 smp", 2048.0 / 48.0),
            ("20ms/50fps", 20.0),
            ("33.4ms/30fps", 1001.0 / 30.0),
            ("40ms/25fps", 40.0),
            ("41.7ms/24fps", 1001.0 / 24.0),
        ];
        // A small *residual* is meaningless on its own (a tiny q always fits). Compare to the chance
        // level for uniform values, `q/√12`: a real drop event sits well below it (ratio ≪ 1).
        let mut best: Option<(&str, f64)> = None;
        for (label, q) in candidates {
            let resid = quantization_residual(&steps, q);
            let ratio = resid / (q / 12.0_f64.sqrt());
            let _ = writeln!(s, "    {label:<13} (q={q:5.1} ms): residual {resid:4.1} ms  ({ratio:.2}× chance)");
            if best.is_none_or(|(_, br)| ratio < br) {
                best = Some((label, ratio));
            }
        }
        match best {
            Some((label, ratio)) if ratio < 0.6 => {
                let _ = writeln!(s, "  → quantized to {label} ({ratio:.2}× chance) — supports a discrete drop event");
            }
            Some((_, ratio)) => {
                let _ = writeln!(s, "  → no quantization (best {ratio:.2}× chance ≈ random) — steps are continuous, NOT a clean block-drop (or measurements are periodicity-corrupted; needs uniqueness)");
            }
            None => {}
        }
        s
    }

    /// **Trustworthy funnel** — does *any* clean recoverable timing offset survive once we demand the
    /// match be unique (not periodicity-suspect)? Filters matched gaps by `uniqueness_margin`, then
    /// residual-confirmed same-source, then clean step, and lists the high-uniqueness survivors so they
    /// can be eyeballed. If nothing clean survives, the timing-offset rescue is settled no-go.
    pub fn trustworthy_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let matched = self.matched();
        let unique: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.uniqueness_margin.is_some_and(|m| m >= LOW_UNIQUENESS_MARGIN))
            .collect();
        let confirmed: Vec<&&&GapRow> = unique
            .iter()
            .filter(|r| {
                r.residual_informative == Some(true)
                    && r.residual_headroom_db.is_some_and(|h| h <= 0.0)
            })
            .collect();
        let clean: usize = confirmed
            .iter()
            .filter(|r| r.seam_step_ms().is_some_and(|st| st.abs() < CLEAN_STEP_MS))
            .count();

        let _ = writeln!(s, "=== trustworthy funnel (is any clean recoverable offset left?) ===");
        let _ = writeln!(
            s,
            "  matched {} → unique (margin ≥ {:.2}) {} → +residual-confirmed {} → +clean step (<{:.0} ms) {}",
            matched.len(),
            LOW_UNIQUENESS_MARGIN,
            unique.len(),
            confirmed.len(),
            CLEAN_STEP_MS,
            clean
        );
        if unique.is_empty() {
            let _ = writeln!(s, "  → no unique-peak gaps: every match is periodicity-suspect. Rescue NO-GO.");
            return s;
        }
        let _ = writeln!(s, "  high-uniqueness survivors (pair idx | offset step uniq | residual | outcome):");
        for r in &unique {
            let _ = writeln!(
                s,
                "    {:<12} g{:<3} | off {:>7.1} step {:>7.1} uniq {:.2} | resid {:>5.1} dB {} | {}",
                r.pair,
                r.index,
                r.seam_mid_ms().unwrap_or(f64::NAN),
                r.seam_step_ms().unwrap_or(f64::NAN),
                r.uniqueness_margin.unwrap_or(f64::NAN),
                r.residual_headroom_db.unwrap_or(f64::NAN),
                if r.residual_informative == Some(true) { "inf" } else { "—" },
                if r.patched() { "patched" } else { "skip" },
            );
        }
        s
    }

    /// **Gate decision** — what actually drives patch vs skip (as opposed to the lag overlay, which is
    /// orthogonal to it). Compares the gate's own scores — **structure** (envelope placement) vs
    /// **seam** (sample-level waveform Pearson) — between patched and skipped gaps, and tallies the
    /// `failure_stage` of the closest bracket on skips. The diagnostic test of "placed by envelope but
    /// rejected by the waveform seam": skipped gaps with structure OK yet seam below the floor.
    pub fn gate_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let matched = self.matched();
        let patched: Vec<&&GapRow> = matched.iter().filter(|r| r.patched()).collect();
        let skipped: Vec<&&GapRow> =
            matched.iter().filter(|r| r.outcome_tier.as_deref() == Some("skip")).collect();
        let _ = writeln!(s, "=== gate decision: structure (placement) vs seam (waveform) ===");

        let group = |label: &str, g: &[&&GapRow], out: &mut String| {
            let st = stats(g.iter().filter_map(|r| r.structure_min).collect());
            let sm = stats(g.iter().filter_map(|r| r.seam_min).collect());
            let bb = stats(g.iter().filter_map(|r| r.best_bracket_seam).collect());
            let f = |o: Option<(f64, f64, f64)>| {
                o.map(|(mn, md, mx)| format!("min {mn:.2} / med {md:.2} / max {mx:.2}"))
                    .unwrap_or_else(|| "—".into())
            };
            let _ = writeln!(out, "  {label} ({}):", g.len());
            let _ = writeln!(out, "    structure (envelope) min : {}", f(st));
            let _ = writeln!(out, "    seam (waveform) @throat  : {}", f(sm));
            let _ = writeln!(out, "    best bracket seam        : {}", f(bb));
        };
        group("patched", &patched, &mut s);
        group("skipped", &skipped, &mut s);

        // Skip reasons (closest bracket's failure stage).
        let mut stages: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &skipped {
            *stages.entry(r.closest_failure_stage.as_deref().unwrap_or("(none)")).or_default() += 1;
        }
        let _ = writeln!(s, "  skipped failure stages (closest bracket): {stages:?}");

        // The hypothesis test: structure passed (≥ 0.5) but waveform seam below the High floor (0.35).
        let placed_but_rejected = skipped
            .iter()
            .filter(|r| r.structure_min.is_some_and(|v| v >= 0.5) && r.seam_min.is_some_and(|v| v < 0.35))
            .count();
        let _ = writeln!(
            s,
            "  → skipped with structure ≥ 0.5 but waveform seam < 0.35 (placed, waveform-rejected): {}/{}",
            placed_but_rejected,
            skipped.len()
        );
        s
    }

    /// **Seam diagnosis** — the missing measurement. For skipped gaps (dead waveform seam), classify
    /// *why* from the seam probe: `misaligned` (recovers under fine lag → finer alignment), `cross-
    /// encoding` (envelope matches, waveform won't align → encoding-robust metric), `quiet`, or
    /// `unresolved`. This is the data that decides the fix.
    pub fn seam_probe_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(s, "=== seam diagnosis: why is the waveform seam dead? ===");
        let _ = writeln!(
            s,
            "  (NOTE: recov/cross-codec here use the seam_probe (±25 ms pre / ±600 ms sequentially-centered \
             post) — see the silence-splice view for the ±600 ms baseline_lag truth)"
        );
        let matched = self.matched();
        let skipped: Vec<&&GapRow> =
            matched.iter().filter(|r| r.outcome_tier.as_deref() == Some("skip")).collect();
        if skipped.iter().all(|r| r.seam_diag.is_none()) {
            let _ = writeln!(s, "  (no seam probe — re-fingerprint with the current binary to populate)");
            return s;
        }
        let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &skipped {
            if let Some(d) = r.seam_diag {
                *tally.entry(d.as_str()).or_default() += 1;
            }
        }
        let _ = writeln!(s, "  skipped seam diagnosis: {tally:?}");

        // The cross-codec hypothesis (plan §3): robust (R2 or R4) ≥ 0.5 while waveform seam < 0.35.
        let with_diag = skipped.iter().filter(|r| r.seam_diag.is_some()).count();
        let robust_high = skipped
            .iter()
            .filter(|r| {
                r.seam_min.is_some_and(|w| w < 0.35)
                    && r.seam_bandlimited_r.unwrap_or(0.0).max(r.seam_spectrum_r.unwrap_or(0.0))
                        >= SEAM_ROBUST_R
            })
            .count();
        let _ = writeln!(
            s,
            "  hypothesis (robust ≥ {:.2} while waveform < 0.35): {robust_high}/{with_diag}",
            SEAM_ROBUST_R
        );

        for r in &skipped {
            if let Some(d) = r.seam_diag {
                let _ = writeln!(
                    s,
                    "    {:<12} g{:<3} | wav {:.2} R2 {:.2} R4 {:.2} env {:.2} recov {:.2} snr {:>5.1} → {}",
                    r.pair,
                    r.index,
                    r.seam_min.unwrap_or(f64::NAN),
                    r.seam_bandlimited_r.unwrap_or(f64::NAN),
                    r.seam_spectrum_r.unwrap_or(f64::NAN),
                    r.seam_envelope_r.unwrap_or(f64::NAN),
                    r.seam_recovered_r.unwrap_or(f64::NAN),
                    r.seam_snr_db.unwrap_or(f64::NAN),
                    d.as_str()
                );
            }
        }
        s
    }

    /// **Silence-splice view** — the authoritative seam read, from the ±600 ms per-side `baseline_lag`
    /// peaks, sequentially centered (ledger A2) so pre offset and
    /// bridge-length mismatch don't stack into one search window (the ±25 ms `seam_probe.recovered_r`
    /// underlying `seam_probe_text` mislabels any step > 25 ms as "cross-codec"). Classifies every matched
    /// gap: `splice` (both shoulders clean & unique at their own lag, separated by a step — addressable by
    /// independent fit + length reconciliation), `alias-suspect` (a thin uniqueness margin), or
    /// `one-sided-dead` (a shoulder aligns at no lag — the only genuine cross-encoding candidate). Lists
    /// the skipped gaps with both per-side peaks + the step.
    pub fn splice_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "=== silence-splice view (±600 ms sequentially-centered baseline per-side; supersedes the ±25 ms recov diagnosis) ==="
        );
        let matched = self.matched();

        // Tally over all matched gaps (the mechanism is shared patch/skip).
        let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
        let mut classified = 0usize;
        for r in &matched {
            if let Some(d) = r.splice_diag() {
                *tally.entry(d.as_str()).or_default() += 1;
                classified += 1;
            }
        }
        if classified == 0 {
            let _ = writeln!(s, "  (no per-side baseline_lag peaks — nothing to classify)");
            return s;
        }
        let tstr: Vec<String> = tally.iter().map(|(k, c)| format!("{k} {c}")).collect();
        let _ = writeln!(s, "  among matched ({classified}): {}", tstr.join(" · "));
        let recoverable = matched.iter().filter(|r| r.both_sides_recoverable()).count();
        let _ = writeln!(
            s,
            "  both-sides-recoverable (peak_r ≥ {:.2} each AND uniqueness ≥ {:.2} or peak_z ≥ {:.0}/prom ≥ {:.2}): {}/{}",
            SPLICE_MIN_PEAK_R, LOW_UNIQUENESS_MARGIN, SPLICE_MIN_PEAK_Z, SPLICE_MIN_PROMINENCE, recoverable, classified
        );
        // Search-exhausted steps: a shoulder peak was clipped at ±max_lag, so `step` is GIGO and the gap is
        // excluded from `dualfit_candidate` (ledger A5/C6). A nonzero count here means widen the lag sweep.
        let edge_pinned = matched.iter().filter(|r| r.step_edge_pinned()).count();
        let edge_pinned_known = matched.iter().filter(|r| r.splice_edge_pinned.is_some()).count();
        if edge_pinned_known == 0 {
            let _ = writeln!(s, "  edge-pinned steps: — (pre-flag fingerprint; re-scan to populate)");
        } else {
            let _ = writeln!(
                s,
                "  edge-pinned steps (search-exhausted ⇒ step GIGO, excluded from dual-fit): {}/{}",
                edge_pinned, edge_pinned_known
            );
        }

        // The skipped gaps in detail — these are the repair targets.
        let skipped: Vec<&&GapRow> =
            matched.iter().filter(|r| r.outcome_tier.as_deref() == Some("skip")).collect();
        if !skipped.is_empty() {
            let _ = writeln!(s, "  skipped gaps (pre peak@lag | post peak@lag | step | uniq → class):");
            for r in &skipped {
                let base_cls = r.splice_diag().map(|d| d.as_str()).unwrap_or("—");
                let cls = if r.step_edge_pinned() {
                    format!("{base_cls} [edge-pinned]")
                } else {
                    base_cls.to_string()
                };
                let z = r.uniqueness_z.map(|v| format!("z {v:.1}")).unwrap_or_else(|| "z —".into());
                let _ = writeln!(
                    s,
                    "    {:<12} g{:<3} | pre {:.3}@{:>7.1} | post {:.3}@{:>7.1} | step {:>7.1} | uniq {:.2} {} → {}",
                    r.pair,
                    r.index,
                    r.peak_r_pre.unwrap_or(f64::NAN),
                    r.frac_lag_pre_ms.unwrap_or(f64::NAN),
                    r.peak_r_post.unwrap_or(f64::NAN),
                    r.frac_lag_post_ms.unwrap_or(f64::NAN),
                    r.seam_step_ms().unwrap_or(f64::NAN),
                    r.uniqueness_margin.unwrap_or(f64::NAN),
                    z,
                    cls,
                );
            }
        }

        // One-sided-dead anywhere is the signal that would revive a cross-encoding validator — call it out.
        let dead: Vec<&&GapRow> =
            matched.iter().filter(|r| r.splice_diag() == Some(SpliceDiag::OneSidedDead)).collect();
        let _ = writeln!(
            s,
            "  one-sided-dead (a shoulder aligns at NO lag — would revive cross-encoding): {}",
            dead.len()
        );
        for r in &dead {
            let _ = writeln!(
                s,
                "    {:<12} g{:<3} | pre {:.3} | post {:.3} | {}",
                r.pair,
                r.index,
                r.peak_r_pre.unwrap_or(f64::NAN),
                r.peak_r_post.unwrap_or(f64::NAN),
                if r.patched() { "patched" } else { "skip" },
            );
        }
        s
    }

    /// **Occupancy — dropout vs program-quiet (D11).** Uses the registration-independent nominal-span B
    /// occupancy (`donor_interior_nominal`): a "gap" where B is *also* silent at the same program time is a
    /// program-quiet passage, not a fillable dropout — correctly skipped, and NOT a repair failure. Also
    /// flags nominal-vs-aligned donor disagreement (the per-shoulder registration moved onto other content).
    pub fn occupancy_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "=== occupancy: fillable dropout vs program-quiet (nominal-span B silence, registration-independent) ==="
        );
        let matched = self.matched();
        let have: Vec<&&GapRow> = matched.iter().filter(|r| r.donor_nominal_silence.is_some()).collect();
        if have.is_empty() {
            let _ = writeln!(s, "  (no donor_interior_nominal in corpus — re-scan with the current binary to populate)");
            return s;
        }
        let quiet = have.iter().filter(|r| r.program_quiet() == Some(true)).count();
        let _ = writeln!(
            s,
            "  among matched with nominal donor ({}): dropout {} · program-quiet {} (B ≥ {:.0}% silent at the same program time)",
            have.len(),
            have.len() - quiet,
            quiet,
            PROGRAM_QUIET_SILENCE_FRAC * 100.0
        );
        let skip_quiet: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.outcome_tier.as_deref() == Some("skip") && r.program_quiet() == Some(true))
            .collect();
        let _ = writeln!(
            s,
            "  → skipped-and-program-quiet: {} — correctly skipped, NOT fill misses (drop from the addressable denominator)",
            skip_quiet.len()
        );
        let disagree = have.iter().filter(|r| r.donor_span_disagrees() == Some(true)).count();
        let _ = writeln!(
            s,
            "  nominal-vs-aligned donor disagreement (registration moved span onto other content): {}/{}",
            disagree,
            have.len()
        );
        if !skip_quiet.is_empty() {
            let _ = writeln!(s, "  program-quiet skips (A gapflr/nflr | B gapflr/nflr | Bsil nom/aln | step):");
            for r in &skip_quiet {
                let o = |v: Option<f64>| v.map(|x| format!("{x:6.1}")).unwrap_or_else(|| "   —  ".into());
                let _ = writeln!(
                    s,
                    "    {:<12} g{:<3} | A {}/{} | B {}/{} | {:.2}/{:.2} | step {:>7.1}",
                    r.pair,
                    r.index,
                    o(r.a_gap_floor_db),
                    o(r.a_noise_floor_db),
                    o(r.b_gap_floor_db),
                    o(r.b_noise_floor_db),
                    r.donor_nominal_silence.unwrap_or(f64::NAN),
                    r.donor_aligned_silence.unwrap_or(f64::NAN),
                    r.seam_step_ms().unwrap_or(f64::NAN),
                );
            }
        }
        s
    }

    /// **Dual-fit viability (scan-native `splice_dualfit`; C3/C7).** The offline `diag_splice_dualfit`
    /// simulation promoted into the scan: each shoulder placed at its own baseline lag, seams scored at
    /// lag 0 vs the gate thresholds — "would a length-reconciled fill pass?" — on the scan's own decode
    /// (no separate ffmpeg path). Reports the pass rate among the bracket-exhausted skips and lists each.
    pub fn dualfit_viability_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "=== dual-fit viability (scan-native splice_dualfit; per-shoulder placement, gate-equiv seam @ lag 0) ==="
        );
        let matched = self.matched();
        let measured: Vec<&&GapRow> = matched.iter().filter(|r| r.dualfit_pass.is_some()).collect();
        if measured.is_empty() {
            let _ = writeln!(
                s,
                "  (no splice_dualfit in corpus — re-scan with the current binary to populate)"
            );
            return s;
        }
        let pass = measured.iter().filter(|r| r.dualfit_pass == Some(true)).count();
        let _ = writeln!(s, "  among matched with splice_dualfit ({}): {pass} would pass a length-reconciled fill", measured.len());

        // The decision cohort: bracket-exhausted skips (0 brackets pass) — the dual-fit targets.
        let skips: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.outcome_tier.as_deref() == Some("skip") && r.brackets_passing == 0)
            .collect();
        let skip_pass = skips.iter().filter(|r| r.dualfit_pass == Some(true)).count();
        let _ = writeln!(
            s,
            "  bracket-exhausted skips: {}/{} would pass dual-fit (the C3 answer)",
            skip_pass,
            skips.len()
        );
        // Validators: is the step necessary (materially improves post vs a constant offset), unique (prom)?
        let step_spurious = skips
            .iter()
            .filter(|r| r.dualfit_pass == Some(true) && !r.step_is_real())
            .count();
        let _ = writeln!(
            s,
            "  validators — of the {skip_pass} dual-fit passes: {} also pass a single global shift (step SPURIOUS ⇒ registration miss, not a splice); {} need the step (real splice)",
            step_spurious,
            skip_pass.saturating_sub(step_spurious)
        );
        // The A3 scope: gate_pass ∧ step-real ∧ donor-continuous ∧ ¬program-quiet (dualfit_target).
        let targets: Vec<String> = matched
            .iter()
            .filter(|r| r.dualfit_target())
            .map(|r| format!("{}·g{}", r.pair, r.index))
            .collect();
        let _ = writeln!(
            s,
            "  ⇒ A3 repair targets (gate_pass ∧ step-real ∧ donor-continuous ∧ ¬program-quiet): {} — {}",
            targets.len(),
            if targets.is_empty() { "(none)".into() } else { targets.join(", ") }
        );
        if !skips.is_empty() {
            let _ = writeln!(
                s,
                "  skipped gaps (dualfit pre | post | post@pre-off | seam-prom | donor | step | br → verdict):"
            );
            for r in &skips {
                let glob = r.dualfit_post_global_r.map(|v| format!("{v:>6.3}")).unwrap_or_else(|| "  —  ".into());
                let prom = r.dualfit_seam_prom.map(|v| format!("{v:.2}")).unwrap_or_else(|| " — ".into());
                let donor = match r.donor_continuous {
                    Some(true) => "cont",
                    Some(false) => "BROKEN",
                    None => "—",
                };
                let verdict = match r.dualfit_pass {
                    Some(true) if r.step_is_real() => "PASS (step real)",
                    Some(true) => "PASS (step spurious)",
                    Some(false) => "fail",
                    None => "—",
                };
                let _ = writeln!(
                    s,
                    "    {:<12} g{:<3} | pre {:>6.3} | post {:>6.3} | {} | prom {} | {:<6} | step {:>7.1} | {}/{} → {}",
                    r.pair,
                    r.index,
                    r.dualfit_pre_r.unwrap_or(f64::NAN),
                    r.dualfit_post_r.unwrap_or(f64::NAN),
                    glob,
                    prom,
                    donor,
                    r.seam_step_ms().unwrap_or(f64::NAN),
                    r.brackets_passing,
                    r.brackets_total,
                    verdict,
                );
            }
            let _ = writeln!(
                s,
                "  (post@pre-off ≥ 0.35 ⇒ a constant shift also fixes it — the \"step\" is a registration artifact.\n   \
                 seam-prom low ⇒ periodic/alias match, PASS not trustworthy.  donor BROKEN ⇒ nothing clean to fill with.)"
            );
        }
        s
    }

    /// **Dual-fit scope (review C1/S4)** — proves patch/skip is *bracket-search success*, not step
    /// magnitude, and narrows the dual-fit target to bracket-exhausted-yet-recoverable skips. Answerable
    /// from `brackets[]` + `baseline_lag` on the *current* corpora (no re-scan needed).
    pub fn dualfit_scope_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let matched = self.matched();
        let patched: Vec<&&GapRow> = matched.iter().filter(|r| r.patched()).collect();
        let skipped: Vec<&&GapRow> =
            matched.iter().filter(|r| r.outcome_tier.as_deref() == Some("skip")).collect();
        let _ = writeln!(s, "=== dual-fit scope: patch/skip is bracket success, NOT step magnitude (C1) ===");

        // Bracket-pass vs outcome (they should coincide).
        let patched_with_pass = patched.iter().filter(|r| r.brackets_passing > 0).count();
        let skipped_exhausted = skipped.iter().filter(|r| r.bracket_exhausted()).count();
        let _ = writeln!(
            s,
            "  patched {} ({} have ≥1 passing bracket) · skipped {} ({} bracket-exhausted, 0 passing)",
            patched.len(), patched_with_pass, skipped.len(), skipped_exhausted
        );

        // Step does NOT separate patch from skip — show the overlapping ranges.
        let step_abs = |g: &[&&GapRow]| stats(g.iter().filter_map(|r| r.seam_step_ms().map(f64::abs)).collect());
        if let (Some((pn, pm, px)), Some((sn, sm, sx))) = (step_abs(&patched), step_abs(&skipped)) {
            let _ = writeln!(s, "  |step| ms  patched: {pn:.1}/{pm:.1}/{px:.1}  ·  skipped: {sn:.1}/{sm:.1}/{sx:.1}  (overlap ⇒ step is not the discriminator)");
        }
        let bb = |g: &[&&GapRow]| stats(g.iter().filter_map(|r| r.best_bracket_seam).collect());
        if let Some((_, pmed, _)) = bb(&patched) {
            let smax = bb(&skipped).map(|(_, _, mx)| mx).unwrap_or(f64::NAN);
            let _ = writeln!(s, "  best-bracket seam  patched median {pmed:.2}  ·  skipped max {smax:.2}  (the real separator)");
        }

        // D11: program-quiet skips (B silent at the same program time) leave the addressable denominator —
        // there is nothing to fill, so they are not dual-fit misses. Report the rate over addressable skips.
        let program_quiet: Vec<&&GapRow> = skipped.iter().filter(|r| r.program_quiet_skip()).copied().collect();
        let addressable: Vec<&&GapRow> =
            skipped.iter().filter(|r| !r.program_quiet_skip()).copied().collect();
        let cand = addressable.iter().filter(|r| r.dualfit_candidate()).count();
        let _ = writeln!(
            s,
            "  dual-fit candidates (skip + bracket-exhausted + both-sides-recoverable): {cand}/{} addressable skips ({} program-quiet dropped, D11)",
            addressable.len(),
            program_quiet.len(),
        );
        let _ = writeln!(s, "  skipped gaps (step | brackets pass/total | best-seam | recoverable → candidate?):");
        for r in &skipped {
            let verdict = if r.program_quiet_skip() {
                "program-quiet".to_string()
            } else if r.dualfit_candidate() {
                "DUAL-FIT".to_string()
            } else {
                "—".to_string()
            };
            let _ = writeln!(
                s,
                "    {:<12} g{:<3} | step {:>7.1} | {:>2}/{:<2} | best {:.2} | {} → {}",
                r.pair,
                r.index,
                r.seam_step_ms().unwrap_or(f64::NAN),
                r.brackets_passing,
                r.brackets_total,
                r.best_bracket_seam.unwrap_or(f64::NAN),
                if r.both_sides_recoverable() { "recoverable" } else { "not-recov  " },
                verdict,
            );
        }
        let _ = writeln!(s, "  → dual-fit targets the bracket-exhausted recoverable skips, NOT high-step patches (e.g. 5·g3 patches with a +72 ms step).");
        s
    }

    /// Provenance legend — what each surfaced measurement actually is (representation · window ·
    /// placement), so the numbers are never read without their definition.
    pub fn legend_text(&self) -> String {
        "=== measurement provenance ===\n\
         structure (envelope) : bucketed 50 ms-bin envelope correlation · ~3 s context each side · at the structure/envelope placement\n\
         seam (waveform)      : sample-level Pearson · ~250 ms seam border · at the throat placement · scored at lag 0\n\
         baseline_lag         : waveform correlation sweep · 1 s border · ±600 ms search, post sequentially\n\
                                 centered on pre lag (S + D_A + round(L_pre), not the naive S + D_A) · at\n\
                                 b_mapped (not the throat) · mono; reported gross-relative to b_mapped_end\n\
         splice               : first-class step + per-side peak_r / peak_z from baseline_lag (post − pre);\n\
                                 ≈ bridge-length mismatch (D_B − D_A) once sequential centering is in effect\n\
         wide_envelope        : 100 ms-bin RMS envelope · 2 s window · ±400 ms pre / ±600 ms sequentially-\n\
                                 centered post · segment-identity confirmer · at b_mapped\n\
         offset/step          : (pre+post)/2 and post−pre of baseline_lag (or splice.step_ms), ms\n\
         residual headroom    : sample-level least-squares cancellation (dB vs floor) · ~250 ms seam · at the throat\n\
         uniqueness           : peak_z + prominence at 1 s window (≥12 / ≥0.45); legacy margin = peak_r − 2nd peak\n\
         donor_interior       : B RMS / continuity over the sequentially-aligned bridge span [S + L_pre,\n\
                                 b_mapped_end + L_post_gross) (bridges the hole?) · at b_mapped\n\
         seam probe           : at b_mapped (not the throat) — wav (Pearson@0) · R2 · R4 · env (10 ms-bin) ·\n\
                                 recov (±25 ms pre / ±600 ms sequentially-centered post) · snr (energy-weighted downmix)\n"
            .to_string()
    }

    /// One CSV row per gap for drill-down.
    pub fn csv(&self) -> String {
        use std::fmt::Write;
        let mut s = String::from(
            "pair,a_id,b_id,index,duration_secs,plan_kind,kind,verdict,outcome_tier,patched,\
frac_lag_pre_ms,frac_lag_post_ms,seam_mid_ms,seam_step_ms,drift_ms,peak_r_pre,peak_r_post,\
uniqueness_margin,residual_headroom_db,residual_informative,skew\n",
        );
        let opt = |v: Option<f64>| v.map(|x| format!("{x:.4}")).unwrap_or_default();
        let optb = |v: Option<bool>| v.map(|x| x.to_string()).unwrap_or_default();
        for r in &self.rows {
            let _ = writeln!(
                s,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:?}",
                r.pair,
                r.a_id,
                r.b_id,
                r.index,
                opt(r.duration_secs),
                r.plan_kind.clone().unwrap_or_default(),
                r.kind.as_str(),
                r.verdict.clone().unwrap_or_default(),
                r.outcome_tier.clone().unwrap_or_default(),
                r.patched(),
                opt(r.frac_lag_pre_ms),
                opt(r.frac_lag_post_ms),
                opt(r.seam_mid_ms()),
                opt(r.seam_step_ms()),
                opt(r.drift_ms()),
                opt(r.peak_r_pre),
                opt(r.peak_r_post),
                opt(r.uniqueness_margin),
                opt(r.residual_headroom_db),
                optb(r.residual_informative),
                r.skew,
            );
        }
        s
    }

    /// **Golden baseline for the perf §4 decision-invariance harness.** See [`crate::golden_baseline`].
    pub fn golden_baseline(&self) -> crate::golden_baseline::GoldenBaseline {
        crate::golden_baseline::baseline_from_report(self)
    }

    /// JSON serialization of [`Self::golden_baseline`].
    pub fn golden_json(&self) -> String {
        crate::golden_baseline::golden_json(self)
    }
}

/// Build the analyzed [`GapRow`]s from one corpus's JSON bytes — the same minimal-projection parse
/// `analyze_dirs` uses, exposed for per-gap fixture tests that hold a single `corpus.json` (or a curated
/// single-gap fixture) rather than a directory tree. `pair` labels the rows (diagnostics only).
pub fn gap_rows_from_corpus_json(
    bytes: &[u8],
    pair: &str,
    drift_eps_ms: f64,
    tail_secs: f64,
) -> Result<Vec<GapRow>, serde_json::Error> {
    let cf: CorpusFile = serde_json::from_slice(bytes)?;
    Ok(cf
        .gaps
        .iter()
        .map(|g| gap_row(pair, &cf.source, g, drift_eps_ms, tail_secs))
        .collect())
}

/// Convenience: env knob for the constant/drift split (`GAP_FP_DRIFT_EPS_MS`, default 1.0 ms).
pub fn drift_eps_from_env() -> f64 {
    std::env::var("GAP_FP_DRIFT_EPS_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_DRIFT_EPS_MS)
}

/// Convenience: env knob for the long-tail (P6) duration cutoff (`GAP_FP_TAIL_SECS`, default 30 s).
pub fn tail_secs_from_env() -> f64 {
    std::env::var("GAP_FP_TAIL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TAIL_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_corpus(dir: &Path, a_id: &str, b_id: &str, gaps_json: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let json = format!(
            r#"{{"source":{{"a_source":{{"id":"{a_id}","sample_rate":48000,"channels":2,"duration_secs":60.0}},
            "b_source":{{"id":"{b_id}","sample_rate":48000,"channels":2,"duration_secs":60.0}},
            "scan_recipe":{{"silence_peak_fraction":0.01}},"gap_count":1}},"gaps":{gaps_json}}}"#
        );
        std::fs::write(dir.join("corpus.json"), json).unwrap();
    }

    fn gap(index: usize, verdict: &str, tier: &str, pre_ms: f64, post_ms: f64) -> String {
        format!(
            r#"{{"index":{index},"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{{"a_start_secs":0,"a_end_secs":0,"a_refined_start_secs":0,"a_refined_end_secs":0,"duration_secs":1.8}},
            "levels":{{"bin_ms":50,"profile_db":[],"floor_db":-120,"speech_peak_db":-40,"noise_floor_db":-50,"gap_floor_db":-90}},
            "silence":{{"collar_rms_peak_ratio":0.1,"collar_above_relative_floor":true,"silence_peak_fraction":0.01}},
            "contour":{{"has_anchor_seam_contour":true,"pre_flatness":0,"post_flatness":0}},
            "anchors":{{"pre":[],"post":[]}},
            "lag":{{"pre_anchor":[{{"window_ms":250,"max_lag_ms":200,"channel":"mono","lag0_r":-0.1,"peak_r":0.95,"second_peak_r":0.20,"peak_lag_samples":-100,"frac_lag_samples":-100,"frac_lag_ms":{pre_ms},"verdict":"{verdict}"}}],
                    "post_anchor":[{{"window_ms":250,"max_lag_ms":200,"channel":"mono","lag0_r":-0.1,"peak_r":0.95,"second_peak_r":0.20,"peak_lag_samples":-50,"frac_lag_samples":-50,"frac_lag_ms":{post_ms},"verdict":"{verdict}"}}]}},
            "baseline_lag":{{"pre_anchor":[{{"window_ms":250,"max_lag_ms":200,"channel":"mono","lag0_r":-0.1,"peak_r":0.95,"second_peak_r":0.20,"peak_lag_samples":-100,"frac_lag_samples":-100,"frac_lag_ms":{pre_ms},"verdict":"{verdict}"}}],
                    "post_anchor":[{{"window_ms":250,"max_lag_ms":200,"channel":"mono","lag0_r":-0.1,"peak_r":0.95,"second_peak_r":0.20,"peak_lag_samples":-50,"frac_lag_samples":-50,"frac_lag_ms":{post_ms},"verdict":"{verdict}"}}]}},
            "residual":{{"chosen_pre_db":-42.0,"chosen_post_db":-41.0,"floor_pre_db":-40.0,"floor_post_db":-39.0,"informative":true}},
            "outcome":{{"plan_kind":"fillable","tier":"{tier}","seam_shape":""}}}}"#
        )
    }

    /// A gap with **no** lag block, at a given duration — exercises the `NoLag` / `Tail` kinds.
    fn gap_nolag(index: usize, duration_secs: f64, tier: &str) -> String {
        format!(
            r#"{{"index":{index},"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{{"a_start_secs":0,"a_end_secs":0,"a_refined_start_secs":0,"a_refined_end_secs":0,"duration_secs":{duration_secs}}},
            "levels":{{"bin_ms":50,"profile_db":[],"floor_db":-120,"speech_peak_db":-40,"noise_floor_db":-50,"gap_floor_db":-90}},
            "silence":{{"collar_rms_peak_ratio":0.1,"collar_above_relative_floor":true,"silence_peak_fraction":0.01}},
            "contour":{{"has_anchor_seam_contour":true,"pre_flatness":0,"post_flatness":0}},
            "anchors":{{"pre":[],"post":[]}},
            "outcome":{{"plan_kind":"fillable","tier":"{tier}","seam_shape":""}}}}"#
        )
    }

    #[test]
    fn aggregates_verdicts_and_skew_across_pairs() {
        let root = tempfile::tempdir().unwrap();
        // pair 1: drifting timing_offset skip (−16 vs −8), decorrelated skip, and a 200 s tail.
        write_corpus(
            &root.path().join("1"),
            "aaaa",
            "bbbb",
            &format!(
                "[{},{},{}]",
                gap(0, "timing_offset", "skip", -16.0, -8.0),
                gap(1, "decorrelated", "skip", 2.0, 3.0),
                gap_nolag(2, 200.0, "skip"),
            ),
        );
        // pair 2: constant timing_offset skip (−5 vs −5.2), a patched gap, and a short no-lag skip.
        write_corpus(
            &root.path().join("2"),
            "cccc",
            "dddd",
            &format!(
                "[{},{},{}]",
                gap(0, "timing_offset", "skip", -5.0, -5.2),
                gap(1, "timing_offset", "patch", -5.0, -5.0),
                gap_nolag(2, 3.0, "skip"),
            ),
        );

        let report = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0);
        assert_eq!(report.pairs.len(), 2, "two pair dirs");
        assert_eq!(report.total_gaps(), 6);

        // Region kinds: 4 matched (the lag-bearing gaps), 1 tail (200 s), 1 no-lag (3 s).
        assert_eq!(report.matched().len(), 4, "four lag-bearing gaps are matched");
        assert_eq!(report.rows.iter().filter(|r| r.kind == GapKind::Tail).count(), 1);
        assert_eq!(report.rows.iter().filter(|r| r.kind == GapKind::NoLag).count(), 1);

        // Addressable = matched + timing_offset + skipped (the patched and decorrelated excluded).
        let matched = report.matched();
        let addr: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.verdict.as_deref() == Some("timing_offset") && !r.patched())
            .collect();
        assert_eq!(addr.len(), 2, "two timing_offset skips");
        assert_eq!(addr.iter().filter(|r| r.skew == SkewClass::Drift).count(), 1, "−16/−8 is drift");
        assert_eq!(addr.iter().filter(|r| r.skew == SkewClass::Constant).count(), 1, "−5/−5.2 is constant");

        // Uniqueness margin = peak_r − second_peak_r = 0.95 − 0.20 = 0.75 on the lag-bearing gaps.
        assert!(
            matched.iter().all(|r| r.uniqueness_margin.is_some_and(|mgn| (mgn - 0.75).abs() < 1e-6)),
            "matched gaps carry a 0.75 uniqueness margin"
        );
        // Residual headroom = worst of (−42−(−40), −41−(−39)) = −2 dB; informative ⇒ same-source.
        assert!(
            matched.iter().all(|r| {
                r.residual_headroom_db.is_some_and(|h| (h - (-2.0)).abs() < 1e-6)
                    && r.residual_informative == Some(true)
            }),
            "matched gaps carry a −2 dB informative residual headroom"
        );

        // Plan kind surfaced; summary + CSV render and carry the new columns.
        assert!(report.rows.iter().all(|r| r.plan_kind.as_deref() == Some("fillable")));
        // Registration decomposition: the −16/−8 drift gap → step +8, mid −12.
        let drift_gap = matched
            .iter()
            .find(|r| r.frac_lag_pre_ms == Some(-16.0))
            .expect("the −16/−8 gap");
        assert!(drift_gap.seam_step_ms().is_some_and(|v| (v - 8.0).abs() < 1e-6));
        assert!(drift_gap.seam_mid_ms().is_some_and(|v| (v - (-12.0)).abs() < 1e-6));

        // Silence-splice view: both lag-bearing skips have peak_r 0.95 ≥ 0.85 and margin 0.75 ≥ 0.30 ⇒
        // `splice` (both-sides-recoverable). None are one-sided-dead.
        assert!(
            addr.iter().all(|r| r.splice_diag() == Some(SpliceDiag::Splice) && r.both_sides_recoverable()),
            "clean high-peak unique skips classify as recoverable splices"
        );
        let splice = report.splice_text();
        assert!(splice.contains("both-sides-recoverable"));
        assert!(splice.contains("one-sided-dead (a shoulder aligns at NO lag"));

        let summary = report.summary_text();
        assert!(summary.contains("plan_kind: fillable"));
        assert!(summary.contains("gap kind:"));
        assert!(summary.contains("uniqueness:"));
        assert!(summary.contains("residual:"));
        assert!(summary.contains("registration:"));
        let header = report.csv().lines().next().unwrap().to_string();
        assert!(header.contains("seam_mid_ms,seam_step_ms"));
        assert!(header.contains("uniqueness_margin") && header.contains("residual_headroom_db"));
        assert_eq!(report.csv().lines().count(), 7); // header + 6 gaps
    }

    #[test]
    fn splice_diag_uses_peak_z_when_present() {
        let root = tempfile::tempdir().unwrap();
        let gap_json = r#"{"index":0,"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{"duration_secs":1.8,"a_refined_start_secs":0},
            "baseline_lag":{"pre_anchor":[{"peak_r":0.95,"peak_z":8.0,"prominence":0.6,"frac_lag_ms":-10.0,"verdict":"timing_offset"}],
                    "post_anchor":[{"peak_r":0.95,"peak_z":15.0,"prominence":0.6,"frac_lag_ms":-5.0,"verdict":"timing_offset"}]},
            "outcome":{"tier":"skip"}}"#.to_string();
        write_corpus(&root.path().join("1"), "aaaa", "bbbb", &format!("[{gap_json}]"));
        let row = &analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0).rows[0];
        assert_eq!(row.splice_diag(), Some(SpliceDiag::AliasSuspect));
        assert!(!row.both_sides_recoverable());
    }

    #[test]
    fn program_quiet_skip_leaves_addressable_denominator() {
        // Two skips with the identical dual-fit *shape* (bracket-exhausted, both shoulders recoverable, not
        // edge-pinned); the only difference is donor occupancy at the nominal program time. D11: the one
        // where B is also silent is program-quiet — nothing to fill — and must drop out of the repair set.
        let gap = |index: usize, nominal_silence: f64| {
            format!(
                r#"{{"index":{index},"tier":"full","sample_rate":48000,"channels":2,
                "geometry":{{"duration_secs":1.8,"a_refined_start_secs":0}},
                "baseline_lag":{{"pre_anchor":[{{"peak_r":0.95,"peak_z":16.0,"prominence":0.6,"frac_lag_ms":-16.0,"verdict":"timing_offset"}}],
                        "post_anchor":[{{"peak_r":0.95,"peak_z":15.0,"prominence":0.6,"frac_lag_ms":-8.0,"verdict":"timing_offset"}}]}},
                "splice":{{"step_ms":8.0,"pre_peak_r":0.95,"post_peak_r":0.95,"pre_peak_z":16.0,"post_peak_z":15.0,"edge_pinned":false}},
                "donor_interior_nominal":{{"rms_db":-80.0,"silence_fraction":{nominal_silence},"continuous":false}},
                "brackets":[{{"failure_stage":"waveform_floor"}}],
                "outcome":{{"tier":"skip"}}}}"#
            )
        };
        let root = tempfile::tempdir().unwrap();
        write_corpus(
            &root.path().join("1"),
            "aaaa",
            "bbbb",
            &format!("[{},{}]", gap(0, 0.95), gap(1, 0.02)),
        );
        let rows = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0).rows;
        let (quiet, dropout) = (&rows[0], &rows[1]);

        // Same shape — both would be dual-fit candidates on the pre-D11 predicate.
        assert!(quiet.bracket_exhausted() && quiet.both_sides_recoverable());
        assert!(dropout.bracket_exhausted() && dropout.both_sides_recoverable());

        // D11 classification: B-silent ⇒ program-quiet, out of the addressable set and not a repair target.
        assert!(quiet.program_quiet_skip(), "B silent at program time ⇒ program-quiet");
        assert!(!quiet.addressable_dropout());
        assert!(!quiet.dualfit_candidate(), "program-quiet must not be a dual-fit target");

        // The real dropout (B occupied) keeps its place in the denominator and stays a candidate.
        assert!(!dropout.program_quiet_skip());
        assert!(dropout.addressable_dropout());
        assert!(dropout.dualfit_candidate(), "occupied-donor dropout is a candidate");
    }

    #[test]
    fn analyzer_hygiene_two_sided_metrics_and_legacy_flag() {
        let root = tempfile::tempdir().unwrap();

        // g0: shoulders DISAGREE (pre timing_offset, post decorrelated). C-harness-2: skew must be
        // NotApplicable (not Drift) — the one-sided verdict must not drive it. C-harness-1: only the pre
        // shoulder carries `peak_z`, so the two-sided robust uniqueness is `None`, not the pre value.
        let disagree = r#"{"index":0,"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{"duration_secs":1.8,"a_refined_start_secs":0},
            "baseline_lag":{"pre_anchor":[{"peak_r":0.95,"peak_z":16.0,"prominence":0.6,"frac_lag_ms":-16.0,"verdict":"timing_offset"}],
                    "post_anchor":[{"peak_r":0.40,"frac_lag_ms":-8.0,"verdict":"decorrelated"}]},
            "outcome":{"tier":"skip"}}"#;

        // g1: pre-A2 fingerprint — only the diagnostic `lag` block, no `baseline_lag`. C-harness-3: the
        // legacy fallback must be flagged and the summary must warn about the schema mix.
        let legacy = r#"{"index":1,"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{"duration_secs":1.8,"a_refined_start_secs":0},
            "lag":{"pre_anchor":[{"peak_r":0.95,"frac_lag_ms":-10.0,"verdict":"timing_offset"}],
                    "post_anchor":[{"peak_r":0.95,"frac_lag_ms":-9.5,"verdict":"timing_offset"}]},
            "outcome":{"tier":"skip"}}"#;

        write_corpus(&root.path().join("1"), "aaaa", "bbbb", &format!("[{disagree},{legacy}]"));
        let report = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0);
        let (g0, g1) = (&report.rows[0], &report.rows[1]);

        // C-harness-2 + C-harness-1.
        assert_eq!(g0.skew, SkewClass::NotApplicable, "disagreeing shoulders are not a timing skew");
        assert_eq!(g0.uniqueness_z, None, "robust uniqueness needs both shoulders' peak_z");

        // C-harness-3.
        assert!(!g0.registration_from_legacy_lag, "g0 has baseline_lag");
        assert!(g1.registration_from_legacy_lag, "g1 fell back to legacy `lag`");
        assert!(
            report.summary_text().contains("registration schema mix"),
            "mixed-schema corpus must warn"
        );
    }

    #[test]
    fn dualfit_target_scopes_on_gate_pass_and_donor_not_uniqueness() {
        // The A3 predicate: gate_pass ∧ step-real (post_own − post@pre ≥ margin) ∧ donor-continuous ∧
        // ¬program-quiet. Each non-target row flips exactly one condition, holding the others at pass.
        // (post_seam_r = 0.95, so step-spurious needs post_global ≳ 0.80 to make Δ < 0.15.)
        let gap = |index: usize, gate_pass: bool, post_global: f64, cont: bool, nominal_sil: f64| {
            format!(
                r#"{{"index":{index},"tier":"full","sample_rate":48000,"channels":2,
                "geometry":{{"duration_secs":1.8,"a_refined_start_secs":0}},
                "baseline_lag":{{"pre_anchor":[{{"peak_r":0.95,"frac_lag_ms":-16.0,"verdict":"timing_offset"}}],
                        "post_anchor":[{{"peak_r":0.95,"frac_lag_ms":-8.0,"verdict":"timing_offset"}}]}},
                "donor_interior":{{"rms_db":-40.0,"silence_fraction":0.02,"continuous":{cont}}},
                "donor_interior_nominal":{{"rms_db":-40.0,"silence_fraction":{nominal_sil},"continuous":{cont}}},
                "splice_dualfit":{{"pre_seam_r":0.99,"post_seam_r":0.95,"trim_frames":10,"gate_pass":{gate_pass},"post_seam_global_r":{post_global}}},
                "brackets":[{{"failure_stage":"waveform_floor"}}],
                "outcome":{{"tier":"skip"}}}}"#
            )
        };
        // A gap that satisfies every dual-fit condition but ALREADY PATCHES (a passing bracket) must be
        // excluded — dual-fit never runs on patched gaps (B1/B11).
        let patched = r#"{"index":5,"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{"duration_secs":1.8,"a_refined_start_secs":0},
            "baseline_lag":{"pre_anchor":[{"peak_r":0.95,"frac_lag_ms":-16.0,"verdict":"timing_offset"}],
                    "post_anchor":[{"peak_r":0.95,"frac_lag_ms":-8.0,"verdict":"timing_offset"}]},
            "donor_interior":{"rms_db":-40.0,"silence_fraction":0.02,"continuous":true},
            "donor_interior_nominal":{"rms_db":-40.0,"silence_fraction":0.02,"continuous":true},
            "splice_dualfit":{"pre_seam_r":0.99,"post_seam_r":0.95,"trim_frames":10,"gate_pass":true,"post_seam_global_r":0.05},
            "brackets":[{"seam_pre":0.9,"seam_post":0.9}],
            "outcome":{"tier":"full"}}"#;
        let root = tempfile::tempdir().unwrap();
        write_corpus(
            &root.path().join("1"),
            "aaaa",
            "bbbb",
            &format!(
                "[{},{},{},{},{},{patched}]",
                gap(0, true, 0.05, true, 0.02),   // clean target
                gap(1, true, 0.90, true, 0.02),   // step spurious: post@pre 0.90 vs post_own 0.95 (Δ 0.05) → not a target
                gap(2, true, 0.05, false, 0.02),  // donor BROKEN (nothing to bridge) → not a target
                gap(3, true, 0.05, true, 0.95),   // program-quiet (nothing to fill) → not a target
                gap(4, false, 0.05, true, 0.02),  // gate FAIL → not a target
            ),
        );
        let rows = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0).rows;
        assert!(rows[0].dualfit_target(), "gate_pass + real step + continuous donor + occupied");
        assert!(!rows[1].dualfit_target(), "spurious step excluded");
        assert!(!rows[2].dualfit_target(), "broken donor excluded");
        assert!(!rows[3].dualfit_target(), "program-quiet excluded");
        assert!(!rows[4].dualfit_target(), "gate fail excluded");
        assert!(!rows[5].dualfit_target(), "already-patched gap excluded (bracket-exhausted skips only)");
    }
}
