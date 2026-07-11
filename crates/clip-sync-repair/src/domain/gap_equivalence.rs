//! Cheap gap content-equivalence (Phase 0) — is B already the **same program audio** as A at the nominal
//! map, so a fill would be a no-op? Plan: `docs/TEMP-gap-equivalence-plan.md`.
//!
//! This module is the **composer + policy** over EXISTING primitives (seam Pearson, same-source residual,
//! donor interior) — it introduces **no** new correlation/residual math (plan §5.0 implementer rule). It owns
//! the pure metric→verdict **policy** (§5.4) and the **E1** nominal-seam read (lag 0, no shoulder search).
//! The coordinate-sensitive residual (E2), donor (E3), A-RMS (E4), and the bounded PCM extract are assembled
//! by the application layer, which then calls [`equivalence_verdict`].
//!
//! **Increment 1 (this file):** types + policy + E1. The application wiring (extract + E2/E3/E4 + fingerprint
//! emission) is the paired next increment where the A↔B coordinate contract for the residual is designed.

use serde::{Deserialize, Serialize};

use crate::domain::donor::{DonorInterior, PROGRAM_QUIET_SILENCE_FRAC};
use crate::domain::policies::{fill_seam_correlations, SeamPlacement, SeamResidualVerdict, SeamTemplates};

/// Policy thresholds for the cheap equivalence gate (§5.4 / §6).
#[derive(Debug, Clone, Copy)]
pub struct GapEquivalenceParams {
    /// **E1** — minimum `min(pre,post)` nominal seam Pearson to consider a skip
    /// (default `0.35`, matching `min_fill_correlation`).
    pub min_seam: f64,
    /// **E2** — max worst-side chosen-vs-floor residual headroom (dB) to accept same-source
    /// (default = `DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB`).
    pub residual_headroom_db: f64,
    /// **E3** — donor `silence_fraction` must be **strictly below** this for the donor to count as
    /// *occupied* (default `PROGRAM_QUIET_SILENCE_FRAC`; at/above ⇒ program-quiet, not equivalence).
    pub max_donor_silence_fraction: f64,
    /// **E4** — A gap RMS must be **at or below** this (dB) to confirm A is a real dropout, not a loud
    /// scan false-negative. Application should set this from the scan silence floor; the default is a
    /// conservative placeholder.
    pub a_quiet_floor_db: f64,
}

impl Default for GapEquivalenceParams {
    fn default() -> Self {
        Self {
            min_seam: 0.35,
            residual_headroom_db: crate::domain::DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
            max_donor_silence_fraction: PROGRAM_QUIET_SILENCE_FRAC,
            a_quiet_floor_db: -45.0,
        }
    }
}

/// Plan-time disposition (§5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquivalenceDisposition {
    /// Metrics say patch would be redundant / identity → plan-time skip.
    Skip,
    /// Metrics inconclusive or contradictory → attempt patch (the conservative default).
    AttemptPatch,
    /// Equivalence not evaluated (disabled, no B map, out of coverage). Set upstream, never by the policy.
    NotEvaluated,
}

/// Reason attached to an equivalence skip (§5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquivalenceSkipReason {
    /// Residual-confirmed: B already carries the same program audio at the nominal map.
    AlreadyMatchesReference,
    /// v1.5 weaker tier for scan false-positive low-level dips (not produced in v1).
    RedundantScanDip,
}

/// The four cheap measurements (§5.3) the policy consumes. `None` = not measured / decode failure ⇒ the
/// policy falls through to `AttemptPatch` (conservative).
#[derive(Debug, Clone, Default)]
pub struct CheapEquivalenceMetrics {
    /// **E1** nominal pre/post seam Pearson at lag 0 ([`nominal_seams`]).
    pub nominal_pre: Option<f64>,
    pub nominal_post: Option<f64>,
    /// **E2** same-source residual at the nominal throat (application-computed with the A↔B delta).
    pub residual: Option<SeamResidualVerdict>,
    /// **E3** donor interior at the nominal span.
    pub donor: Option<DonorInterior>,
    /// **E4** A gap interior RMS (dB).
    pub a_gap_rms_db: Option<f64>,
}

/// Serializable verdict (§5.5) — the metrics readout plus the policy disposition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquivalenceVerdict {
    pub disposition: EquivalenceDisposition,
    pub nominal_pre: Option<f64>,
    pub nominal_post: Option<f64>,
    pub residual_headroom_db: Option<f64>,
    pub residual_informative: bool,
    pub donor_silence_fraction: Option<f64>,
    pub a_gap_rms_db: Option<f64>,
    pub skip_reason: Option<EquivalenceSkipReason>,
}

impl EquivalenceVerdict {
    /// The upstream "not evaluated" verdict (disabled / no B map / out of coverage) — no metrics.
    pub fn not_evaluated() -> Self {
        Self {
            disposition: EquivalenceDisposition::NotEvaluated,
            nominal_pre: None,
            nominal_post: None,
            residual_headroom_db: None,
            residual_informative: false,
            donor_silence_fraction: None,
            a_gap_rms_db: None,
            skip_reason: None,
        }
    }
}

/// **§5.4 conservative policy.** Returns `Skip` (`AlreadyMatchesReference`) **only** when all four hold;
/// any missing metric or unmet condition ⇒ `AttemptPatch`. Never returns `NotEvaluated` (that is decided
/// upstream, before evaluation). Pure over the metrics — no I/O, no measurement.
///
/// 1. **E3 occupied** — donor `continuous` and `silence_fraction < max_donor_silence_fraction`.
/// 2. **E4 quiet A** — `a_gap_rms_db <= a_quiet_floor_db` (A is a real dropout).
/// 3. **E1 strong seams** — `min(pre, post) >= min_seam`.
/// 4. **E2 same-source** — residual `informative` and `worst_headroom_db() <= residual_headroom_db`.
pub fn equivalence_verdict(
    metrics: &CheapEquivalenceMetrics,
    params: &GapEquivalenceParams,
) -> EquivalenceVerdict {
    let residual_headroom_db = metrics
        .residual
        .as_ref()
        .map(SeamResidualVerdict::worst_headroom_db)
        .filter(|h| h.is_finite());
    let residual_informative = metrics.residual.as_ref().is_some_and(|r| r.informative);
    let donor_silence_fraction = metrics.donor.as_ref().map(|d| d.silence_fraction);

    // Condition 3 — E1 strong seams (both sides scored, min above floor).
    let seams_ok = matches!(
        (metrics.nominal_pre, metrics.nominal_post),
        (Some(pre), Some(post)) if pre.min(post) >= params.min_seam
    );
    // Condition 1 — E3 donor occupied (continuous, not program-quiet).
    let donor_ok = metrics
        .donor
        .as_ref()
        .is_some_and(|d| d.continuous && d.silence_fraction < params.max_donor_silence_fraction);
    // Condition 2 — E4 A is a real dropout.
    let a_quiet_ok = metrics.a_gap_rms_db.is_some_and(|rms| rms <= params.a_quiet_floor_db);
    // Condition 4 — E2 same-source (floor cancels; chosen no worse than floor by more than the margin).
    let residual_ok =
        residual_informative && residual_headroom_db.is_some_and(|h| h <= params.residual_headroom_db);

    let disposition = if donor_ok && a_quiet_ok && seams_ok && residual_ok {
        EquivalenceDisposition::Skip
    } else {
        EquivalenceDisposition::AttemptPatch
    };
    let skip_reason =
        (disposition == EquivalenceDisposition::Skip).then_some(EquivalenceSkipReason::AlreadyMatchesReference);

    EquivalenceVerdict {
        disposition,
        nominal_pre: metrics.nominal_pre,
        nominal_post: metrics.nominal_post,
        residual_headroom_db,
        residual_informative,
        donor_silence_fraction,
        a_gap_rms_db: metrics.a_gap_rms_db,
        skip_reason,
    }
}

/// **E1** — nominal seam Pearson at **lag 0** (no shoulder search), mono-only. Reuses
/// [`fill_seam_correlations`] with deliberately-empty per-channel templates (metrics use the mono downmix,
/// §5.2). Returns `(pre, post)`; a side is `None` when it has no scorable window (mirrors the internal
/// scorability guards of `fill_seam_correlations`, so `None` ⟺ the primitive would return its `0.0`
/// placeholder rather than a real correlation).
///
/// - `a_pre` / `a_post`: A border templates (from `border_templates_for_gap`, mono).
/// - `b_mono`: the nominal B extract (seam margins + gap), downmixed.
/// - `b_mapped_start`: the fill start (nominal gap start) index within `b_mono`.
pub fn nominal_seams(
    a_pre: &[f64],
    a_post: &[f64],
    b_mono: &[f64],
    b_mapped_start: usize,
    gap_frames: usize,
    pre_window: usize,
    post_window: usize,
) -> (Option<f64>, Option<f64>) {
    let templates = SeamTemplates {
        a_pre,
        a_post,
        a_pre_ch: &[],
        a_post_ch: &[],
        b_mono,
        b_ch: &[],
    };
    let placement = SeamPlacement { start: b_mapped_start, gap_frames, pre_window, post_window };
    let (pre, post) = fill_seam_correlations(&templates, placement);

    let pre_scorable =
        pre_window > 0 && !a_pre.is_empty() && b_mapped_start >= pre_window && b_mapped_start <= b_mono.len();
    let post_scorable =
        post_window > 0 && !a_post.is_empty() && b_mapped_start + gap_frames + post_window <= b_mono.len();

    (pre_scorable.then_some(pre), post_scorable.then_some(post))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policies::{SeamFloorProbe, SeamFloorSource};

    /// Build a residual verdict from per-side (chosen_db, floor_db) with a chosen `informative`.
    /// `informative` is derived by `from_parts_with_placement` from the floor probes vs floor-ok, so we
    /// pick floor `residual_db` accordingly: ≤ -15 (DEFAULT_RESIDUAL_FLOOR_OK_DB) ⇒ informative.
    fn residual(chosen_db: f64, floor_db: f64) -> SeamResidualVerdict {
        let probe = |db: f64| SeamFloorProbe { source: SeamFloorSource::Border, residual_db: db, gain: 1.0, best_lag: 0 };
        SeamResidualVerdict::from_parts_with_placement(
            &probe(chosen_db),
            &probe(chosen_db),
            &probe(floor_db),
            &probe(floor_db),
            crate::domain::policies::DEFAULT_RESIDUAL_FLOOR_OK_DB,
            0,
            0,
        )
    }

    fn donor(silence_fraction: f64, continuous: bool) -> DonorInterior {
        DonorInterior { rms_db: -20.0, silence_fraction, longest_silence_ms: 0.0, continuous }
    }

    /// All four conditions satisfied → Skip (same-master: strong seams, occupied donor, quiet A, floor cancels).
    #[test]
    fn same_master_skips() {
        let m = CheapEquivalenceMetrics {
            nominal_pre: Some(0.98),
            nominal_post: Some(0.95),
            residual: Some(residual(-30.0, -30.0)), // floor cancels (informative), chosen == floor (headroom 0)
            donor: Some(donor(0.05, true)),
            a_gap_rms_db: Some(-60.0),
        };
        let v = equivalence_verdict(&m, &GapEquivalenceParams::default());
        assert_eq!(v.disposition, EquivalenceDisposition::Skip);
        assert_eq!(v.skip_reason, Some(EquivalenceSkipReason::AlreadyMatchesReference));
        assert!(v.residual_informative);
    }

    /// Weak seams (decorrelated / timing-offset at lag 0) → AttemptPatch even if donor+A+residual look ok.
    #[test]
    fn weak_seams_attempt_patch() {
        let m = CheapEquivalenceMetrics {
            nominal_pre: Some(0.10),
            nominal_post: Some(0.12),
            residual: Some(residual(-30.0, -30.0)),
            donor: Some(donor(0.05, true)),
            a_gap_rms_db: Some(-60.0),
        };
        let v = equivalence_verdict(&m, &GapEquivalenceParams::default());
        assert_eq!(v.disposition, EquivalenceDisposition::AttemptPatch);
        assert_eq!(v.skip_reason, None);
    }

    /// Residual not informative (floor doesn't cancel = different source) → AttemptPatch despite strong seams.
    #[test]
    fn uninformative_residual_attempt_patch() {
        let m = CheapEquivalenceMetrics {
            nominal_pre: Some(0.98),
            nominal_post: Some(0.95),
            residual: Some(residual(-5.0, -5.0)), // floor -5 > -15 ⇒ not informative
            donor: Some(donor(0.05, true)),
            a_gap_rms_db: Some(-60.0),
        };
        let v = equivalence_verdict(&m, &GapEquivalenceParams::default());
        assert!(!v.residual_informative);
        assert_eq!(v.disposition, EquivalenceDisposition::AttemptPatch);
    }

    /// Program-quiet donor (mostly silent) → AttemptPatch (that is unfillable territory, not equivalence).
    #[test]
    fn program_quiet_donor_attempt_patch() {
        let m = CheapEquivalenceMetrics {
            nominal_pre: Some(0.98),
            nominal_post: Some(0.95),
            residual: Some(residual(-30.0, -30.0)),
            donor: Some(donor(0.9, false)),
            a_gap_rms_db: Some(-60.0),
        };
        assert_eq!(
            equivalence_verdict(&m, &GapEquivalenceParams::default()).disposition,
            EquivalenceDisposition::AttemptPatch
        );
    }

    /// Loud A (scan false-negative, not a real dropout) → AttemptPatch.
    #[test]
    fn loud_a_attempt_patch() {
        let m = CheapEquivalenceMetrics {
            nominal_pre: Some(0.98),
            nominal_post: Some(0.95),
            residual: Some(residual(-30.0, -30.0)),
            donor: Some(donor(0.05, true)),
            a_gap_rms_db: Some(-10.0), // above the -45 quiet floor
        };
        assert_eq!(
            equivalence_verdict(&m, &GapEquivalenceParams::default()).disposition,
            EquivalenceDisposition::AttemptPatch
        );
    }

    /// Any missing metric (decode failure) → AttemptPatch (conservative).
    #[test]
    fn missing_metric_attempt_patch() {
        let m = CheapEquivalenceMetrics {
            nominal_pre: None,
            nominal_post: Some(0.95),
            residual: Some(residual(-30.0, -30.0)),
            donor: Some(donor(0.05, true)),
            a_gap_rms_db: Some(-60.0),
        };
        assert_eq!(
            equivalence_verdict(&m, &GapEquivalenceParams::default()).disposition,
            EquivalenceDisposition::AttemptPatch
        );
    }

    /// E1 scorability: a placement with no room for the pre window returns `None` (not a spurious 0.0).
    #[test]
    fn nominal_seams_reports_unscorable_side_as_none() {
        let a_pre = vec![0.1_f64; 100];
        let a_post = vec![0.1_f64; 100];
        let b_mono = vec![0.1_f64; 500];
        // start (10) < pre_window (50) ⇒ pre unscorable; post has room.
        let (pre, post) = nominal_seams(&a_pre, &a_post, &b_mono, 10, 50, 50, 50);
        assert_eq!(pre, None, "pre window doesn't fit before start ⇒ None");
        assert!(post.is_some(), "post window fits ⇒ Some");
    }
}
