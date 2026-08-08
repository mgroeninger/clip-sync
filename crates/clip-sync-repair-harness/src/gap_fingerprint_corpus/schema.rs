//! Analyzed row + public taxonomy for the gap-fingerprint corpus scan.
//!
//! The classification vocabulary (`SkewClass` / `GapKind` / `SeamDiag` / `SpliceDiag`), the
//! calibration thresholds those classifiers use, and the flattened per-gap [`GapRow`] (the analysis
//! denominator). This is the leaf of the module: it depends only on the two canonical repair-domain
//! margins, never on the JSON projection or the report formatters. The minimal `Deserialize` projection
//! that produces a `GapRow` lives with the analysis code that consumes it.

use clip_sync_repair::domain::donor::PROGRAM_QUIET_SILENCE_FRAC;
use clip_sync_repair::domain::dual_fit::DUALFIT_STEP_REAL_MARGIN;

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

/// Silence-splice classification from the **±600 ms** per-side `lag_decision` peaks, sequentially
/// centered (ledger A2: post search is centered on `S + D_A + round(L_pre)`,
/// not the naive `S + D_A`), not the ±25 ms `seam_probe.recovered_r`, which mislabels any step > 25 ms as
/// "cross-codec". See `docs/dev/archive/TEMP-seam-splice-dualfit-plan.md` §1/§3. Decides whether a skipped gap is the
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

pub(crate) const SEAM_QUIET_SNR_DB: f64 = 6.0;
pub(crate) const SEAM_RECOVER_R: f64 = 0.5;
/// R2/R4 above this (with waveform dead) is the cross-codec validator-mismatch signal.
pub(crate) const SEAM_ROBUST_R: f64 = 0.5;
/// A shoulder must reach this `peak_r` at its own lag (over the ±600 ms `lag_decision` sweep) to count as
/// cleanly recoverable — the both-sides-recoverable floor. Heuristic; calibrate against the corpus.
pub(crate) const SPLICE_MIN_PEAK_R: f64 = 0.85;
/// §3.6a robust-uniqueness floors (1 s window) — used when `peak_z` is present on the fingerprint.
/// **`peak_z` is primary** (whole-curve z-score; deflates on periodic/leveled content, so it is the
/// periodicity-robust test — ledger B3). `SPLICE_MIN_PROMINENCE` is a *tiebreaker* at a deliberately low
/// floor: prominence is a single-rival term that over-flags leveled/limited audio (2026-07-01: 9/32
/// alias-suspect were prominence-only false-flags with `peak_z` 12.6–26; 4 were gate-patched — A5/C6). The
/// 0.15 floor only catches a genuine near-duplicate rival (e.g. 6·g9 `prom 0.11`).
pub(crate) const SPLICE_MIN_PEAK_Z: f64 = 12.0;
pub(crate) const SPLICE_MIN_PROMINENCE: f64 = 0.15;
/// A uniqueness margin below this flags a same-source verdict as **periodicity-suspect** (a competing
/// lag peak nearly as tall). Heuristic; calibrate against the corpus.
pub(crate) const LOW_UNIQUENESS_MARGIN: f64 = 0.30;

/// One gap's aggregated row.
#[derive(Debug, Clone)]
pub struct GapRow {
    pub pair: String,
    pub a_id: String,
    pub b_id: String,
    /// Source codec of each side (`aac`, `ac3`, `flac`, …) as the probe named it. `None` for corpora
    /// dumped before source provenance existed — absent, not "unknown lossless".
    pub a_codec: Option<String>,
    pub b_codec: Option<String>,
    /// Each side's **native** (pre-decode) sample rate. The measurement rate is A's; B is resampled to it.
    pub a_native_sample_rate: Option<u32>,
    pub b_native_sample_rate: Option<u32>,
    /// Each side's **native** (pre-decode) channel count, which can differ from the measured `channels`.
    /// Recorded, not acted on: a channel mismatch is a known measurement hazard the fingerprint does not
    /// currently correct for, and the column exists so such pairs can be found rather than fixed here.
    pub a_native_channels: Option<u16>,
    pub b_native_channels: Option<u16>,
    /// Was this side rate-converted before measurement (`native != measured`)? `None` when the corpus
    /// carries no native rate — "unanswerable" and "no" are different readings.
    pub a_was_resampled: Option<bool>,
    pub b_was_resampled: Option<bool>,
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
    /// **`splice_step_ms` is search-exhausted** — a shoulder's `lag_decision` peak was clipped at ±max_lag,
    /// so the step (and dual-fit per-shoulder placement) is GIGO (ledger A5/C6). `None` predates the flag.
    pub splice_edge_pinned: Option<bool>,
    /// Registration came from the **legacy diagnostic `lag`** (pre-A2 fingerprint), not the decision-seam
    /// `lag_decision` — a *different* placement (structure throat vs `b_mapped`). Any `true` in a run means
    /// the corpus mixes pre-/post-A2 schemas and the lag reads aren't comparable (C-harness-3).
    pub registration_from_legacy_lag: bool,
    /// Donor B bridges the gap interior without a sub-floor hole (from `donor_interior.continuous`).
    pub donor_continuous: Option<bool>,
    /// Donor-interior RMS (dBFS) over the gap-mapped B span.
    pub donor_rms_db: Option<f64>,
    /// Wide-envelope (100 ms-bin) peak lag at the pre seam — cross-scale check vs `lag_decision`.
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
    /// Placement chosen for that best-min-seam bracket: B start frame, and the fill length the end
    /// sweep picked. Nominal is the bracket span (`span_secs`), not the original silent-run gap —
    /// excursion is `|fill − span|` (≤ `fill_length_slack_secs`); `|fill − gap|` is mostly anchor
    /// widening. `None` on dumps written before 2026-07-25. The only record of the end search's
    /// decision — a change that moved every fill length on the corpus used to leave the golden
    /// diff green.
    pub best_bracket_start_frame: Option<usize>,
    pub best_bracket_fill_frames: Option<usize>,
    /// That bracket's two seam correlations, unaggregated — `best_bracket_seam` is their `min`, which
    /// cannot show a placement change that trades one shoulder against the other.
    pub best_bracket_seam_pre: Option<f64>,
    pub best_bracket_seam_post: Option<f64>,
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
    /// **F14 — would production's dual-fit rescue this bracket-gate skip?** (`outcome.dual_fit_rescue`.)
    /// `outcome_tier` is the bracket-gate result *only*; production runs `skip_or_dual_fit` afterwards and
    /// can still patch, so a `skip` row here can be a patched gap in production. `None` on gaps the gate
    /// patched (not applicable), on unmeasured gaps, and on every fingerprint written before 2026-07-30.
    /// See [`Self::production_patched`] — do not fold this into [`Self::patched`].
    pub dual_fit_rescue: Option<bool>,
}

impl GapRow {
    /// Patched = the gate produced a patch tier (anything but `skip`). `None` outcome → not patched.
    ///
    /// **This is the bracket-gate `any_ok` axis and must stay that way** — it is a frozen Tier-1
    /// golden-baseline field, and it is the only axis in the dump that means something precise. It is
    /// *not* "production repaired this gap": production's dual-fit can rescue a gap this returns `false`
    /// for. Use [`Self::production_patched`] when the question is whether a hole was left unrepaired.
    pub fn patched(&self) -> bool {
        matches!(&self.outcome_tier, Some(t) if t != "skip")
    }

    /// **F14 — did production most likely leave audio unrepaired?** The bracket gate patched it, *or*
    /// dual-fit would have rescued it. This is the predicate a "how many holes remain?" roll-up wants;
    /// [`Self::patched`] under-counts by exactly the dual-fit rescues, which is the oracle bias F14
    /// recorded.
    ///
    /// Inherits `dual_fit_rescue`'s caveats: it assumes `--dual-fit` is on, and it is a gate prediction,
    /// not an observed patch — production re-validates the assembled fill and may still decline. On
    /// fingerprints predating the field it degrades to [`Self::patched`].
    pub fn production_patched(&self) -> bool {
        self.patched() || self.dual_fit_rescue == Some(true)
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

    /// **Silence-splice classification** from the ±600 ms per-side `lag_decision` peaks — the
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
        if self
            .uniqueness_margin
            .is_some_and(|m| m < LOW_UNIQUENESS_MARGIN)
        {
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

    /// The measured step (and its per-shoulder placement) is **search-exhausted** — a `lag_decision` peak
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
    /// shows: the seams **pass** the unchanged gate (`dualfit_pass`); the **step is real** ([`Self::step_is_real`] —
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
        self.donor_nominal_silence
            .map(|s| s >= PROGRAM_QUIET_SILENCE_FRAC)
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
