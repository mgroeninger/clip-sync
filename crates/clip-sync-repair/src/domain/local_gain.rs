//! Hole-local fill gain from the paired collar either side of a gap.
//!
//! The apply half of `docs/dev/archive/TEMP-local-paired-gain-plan.md`. Measurement lives in
//! [`program_levels`](crate::domain::program_levels), which stamps a
//! [`CollarLevelOffset`] on every gap at scan time; this module is the one place that turns one
//! into a multiplier, and the split is deliberate — a floor inside the estimator would have
//! censored the census that chose these constants at exactly the value it existed to choose.
//!
//! **What replaced what.** The previous local half was `policies::compute_fill_gain`: the RMS of
//! the assembled fill against a 5 s + 5 s pool of A's borders, boost-only, clamped at
//! `max_fill_gain_db`. That estimator answered "is this fill quieter than the neighbourhood?" when
//! the question is "do A and B disagree about level *here*?". On quiet material inside a loud
//! programme the two come apart badly: the corpus's one ear-confirmed too-loud fill is that
//! estimator's own +11.78 dB boost, measured against borders sitting 52 dB below the pool that set
//! the reference. The collar asks the second question directly, on A-versus-B block pairs drawn
//! from the material immediately flanking the hole, and it needs no fill RMS at all — which is why
//! it can serve the dual-fit path that the RMS estimator never could.
//!
//! **This is a residual, not a second opinion.** Whole-programme gain
//! ([`program_gain`](crate::domain::program_gain)) has already matched masters across the file. What
//! is left for the collar is the part the file average does not explain, so the prior is subtracted
//! before the gain is formed and the two multiply to `10^(-d_local/20)` — the hole's own paired
//! offset — rather than to the square of anything. See [`collar_residual_gain`].

use serde::Serialize;

use crate::domain::program_levels::{
    CollarLevelOffset, MAX_LOCAL_RESIDUAL_DB, MAX_LOCAL_SE_DB, MIN_LOCAL_PAIRS,
};

/// Why a gap took the local gain it did. Serialized next to `local_gain` on the patch record.
///
/// Unlike [`ProgramGainRefusal`](crate::domain::ProgramGainRefusal) this enum carries its success
/// case too. Every failure is unity, so `local_gain: 1.0` alone cannot say whether the estimator
/// declined or ran and found nothing to correct — and those are different facts about the gap. A
/// corpus that mostly reads `abstained_n` has not had its estimator replaced; it has had it
/// deleted, and that has to be visible without a second field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalGainReason {
    /// `normalize_fill` is off (`--no-normalize`). The collar may still be on the scan report:
    /// measurement does not depend on the flag, only application does.
    FlagOff,
    /// A and B channel counts differ, so no paired level reading from this pair is a gain: RMS over
    /// interleaved samples makes a stereo/mono pair carry a correlation-dependent constant that is
    /// indistinguishable from master gain. Read from `ProgramLevels::channels_comparable`, and
    /// **never** inferred from the collar's absence in either direction — such a pair is still
    /// measured, so its collar is normally present, and absence separately means "nothing paired",
    /// "no alignment offset" or "B not scanned".
    Channels,
    /// Nothing paired at all, or fewer than [`MIN_LOCAL_PAIRS`] pairs did. The remedy is a wider
    /// collar; distinguished from [`Self::AbstainedDispersion`] because that one has no remedy.
    AbstainedN,
    /// Populated enough, but the reading is not trustworthy: one-sided, or `se_db` past the bound.
    /// Nothing widens a collar out of two shoulders that disagree.
    AbstainedDispersion,
    /// The residual is past [`MAX_LOCAL_RESIDUAL_DB`]. A detection, not a level — the collar is
    /// reaching material that is not this hole's neighbourhood.
    RefusedResidual,
    /// A gain was applied. Includes a residual of ~0, which is the designed outcome on a
    /// same-master pair and is **not** the same as any abstention above.
    Applied,
}

/// The hole-local half of the fill gain, per gap.
///
/// Same shape as [`ProgramGainDecision`](crate::domain::ProgramGainDecision) — which is per *run* —
/// so the two compose without either having to know the other's failure modes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalGainDecision {
    /// Multiply this gap's B-derived fill by `gain`, on top of the programme scalar.
    Applied {
        /// `10^(-residual_db/20)`. **May be below 1.0**: a constant scalar taken from paired
        /// shoulders does not crush intra-fill dynamics, so the boost-only rail that governed the
        /// RMS estimator does not apply here (`settled.md` § C is about the RMS-ratio latch, a
        /// different mechanism).
        gain: f32,
    },
    Refused {
        reason: LocalGainReason,
    },
}

impl LocalGainDecision {
    /// The multiplier to apply. `1.0` on every refusal — fail closed is unity, which leaves the
    /// gap exactly where the programme half put it.
    #[must_use]
    pub fn gain(self) -> f32 {
        match self {
            Self::Applied { gain } => gain,
            Self::Refused { .. } => 1.0,
        }
    }

    /// Unity with the flag-off reason: the shape of a run with `--no-normalize`, which is the
    /// default. The right filler wherever a strategy or outcome is built on a path that never
    /// consults a collar — projections, fixtures, the anchored-retry placeholders — because it says
    /// *no local gain was applied and none was asked for*, which is what those paths mean.
    pub const OFF: Self = Self::Refused {
        reason: LocalGainReason::FlagOff,
    };

    /// What to stamp on the patch record. Always present, including on success.
    #[must_use]
    pub fn reason(self) -> LocalGainReason {
        match self {
            Self::Applied { .. } => LocalGainReason::Applied,
            Self::Refused { reason } => reason,
        }
    }
}

/// Turn one gap's collar into a fill multiplier.
///
/// ```text
/// residual_db = collar.offset_db − offset_prior
/// gain        = 10^(−residual_db / 20)
/// ```
///
/// **`offset_prior` is the *applied* programme offset, not the measured one.** Pass
/// [`ProgramGainDecision::offset_db`](crate::domain::ProgramGainDecision::offset_db)`.unwrap_or(0.0)`:
/// on a refusal there is no trusted average to subtract, so the prior is zero and the collar
/// becomes a hole-local master match on its own. Subtracting an offset the run *refused* would bake
/// a phantom into every gap on the pair — the plan's § 4 names the −8.4 dB compressed-B reading as
/// the shape that does it.
///
/// When both halves apply, `g_program_db + g_local_db = −d_local`: the file average cancels and the
/// hole's own paired offset is what lands. Programme gain is then the *fallback* for collars that
/// abstain, not a second copy of the same correction.
///
/// Order follows [`program_fill_gain`](crate::domain::program_fill_gain): the flag first, so
/// `--no-normalize` reports as itself on a gap that would also have abstained, then the
/// preconditions (channels, then a reading at all), then the admission test, then the residual
/// bound. Order mostly decides only which reason is reported — every failure is unity — but the
/// channel precondition is load-bearing rather than cosmetic, since a mismatched pair reaches the
/// admission test with a populated collar that passes it. See [`LocalGainReason::Channels`].
///
/// **The comparisons are written inverted (`!(x <= bound)`) on purpose.** A NaN `spread_db` or
/// `offset_db` would pass `x > bound` and reach the fill as a NaN multiplier; written this way a
/// non-finite reading fails closed into the nearest refusal instead. There is deliberately no
/// separate `not_finite` reason: unlike the programme half this is per gap, and a reason variant
/// that fires on nothing across 1073 corpus gaps is a branch nobody can test.
#[must_use]
// The inverted comparisons below are the point, not an accident: `x > bound` would let a NaN
// through to the fill as a NaN multiplier, and `partial_cmp` would only move the same decision
// somewhere less obvious. See the "written inverted" paragraph above.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn collar_residual_gain(
    collar: Option<&CollarLevelOffset>,
    enabled: bool,
    offset_prior: f64,
    max_se_db: f64,
    max_residual_db: f64,
    channels_comparable: bool,
) -> LocalGainDecision {
    let refuse = |reason| LocalGainDecision::Refused { reason };
    if !enabled {
        return refuse(LocalGainReason::FlagOff);
    }
    // A precondition, not a threshold, and checked **before** the collar rather than only in its
    // absence. Today the two are equivalent: `scan_gaps` wraps the whole stamping loop in
    // `if channels_comparable`, so a mismatched pair reaches this function with `collar: None` and
    // either position reports `channels`. That equivalence is a *remote* invariant — it lives in
    // the scan, one crate layer away, and nothing links the two sites. `paired_level_offset` itself
    // does not skip mismatched pairs (`program_levels` module docs: "That case still reports, with
    // channels_comparable false"), so if the collar ever came to be stamped the way the whole-file
    // offset already is, a stereo-versus-mono collar would arrive fully populated and clear every
    // test below — and undiluted, since the programme half refuses that pair for this same reason
    // and leaves a zero prior. `offset_db` is not a gain there: block RMS over interleaved samples
    // carries a correlation-dependent constant indistinguishable from master gain. Checking it here
    // costs one branch and makes the refusal this function's own. Same position and same reasoning
    // as `program_fill_gain`'s `channels_comparable` refusal.
    if !channels_comparable {
        return refuse(LocalGainReason::Channels);
    }
    let Some(collar) = collar else {
        // Absence stays ambiguous by design (§ 6): no alignment offset, B not scanned, or nothing
        // in the collar paired. None of those are separable here, and all of them mean the same
        // thing to a caller — there was no reading — so they share `abstained_n`.
        return refuse(LocalGainReason::AbstainedN);
    };
    if collar.n_pairs < MIN_LOCAL_PAIRS {
        return refuse(LocalGainReason::AbstainedN);
    }
    // Both shoulders had to speak. Eight of the eleven worst collars in the corpus are one-sided,
    // and a one-sided collar cannot disagree with itself, so `se_db` alone does not catch them.
    if collar.n_pre == 0 || collar.n_post == 0 {
        return refuse(LocalGainReason::AbstainedDispersion);
    }
    let Some(se_db) = collar.se_db() else {
        // Unreachable while MIN_LOCAL_PAIRS > 0 — `se_db` is `None` only at zero pairs. Handled
        // rather than unwrapped so lowering that constant cannot turn into a panic.
        return refuse(LocalGainReason::AbstainedN);
    };
    if !(se_db <= max_se_db) {
        return refuse(LocalGainReason::AbstainedDispersion);
    }
    let residual_db = collar.offset_db - offset_prior;
    if !(residual_db.abs() <= max_residual_db) {
        return refuse(LocalGainReason::RefusedResidual);
    }
    LocalGainDecision::Applied {
        gain: 10f64.powf(-residual_db / 20.0) as f32,
    }
}

/// [`collar_residual_gain`] with the shipping constants, for call sites that have no reason to
/// vary them.
///
/// The bounds stay parameters on the function above so the tests can drive each branch without a
/// corpus-sized fixture; [`MIN_LOCAL_PAIRS`] is not a parameter at all, because it is the same
/// policy on every gap and nothing has ever wanted to vary it per call.
#[must_use]
pub fn collar_residual_gain_default(
    collar: Option<&CollarLevelOffset>,
    enabled: bool,
    offset_prior: f64,
    channels_comparable: bool,
) -> LocalGainDecision {
    collar_residual_gain(
        collar,
        enabled,
        offset_prior,
        MAX_LOCAL_SE_DB,
        MAX_LOCAL_RESIDUAL_DB,
        channels_comparable,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clean two-sided collar: enough pairs, both shoulders, tight spread.
    fn collar(offset_db: f64, spread_db: f64, n_pairs: usize) -> CollarLevelOffset {
        CollarLevelOffset {
            offset_db,
            spread_db,
            n_pairs,
            n_active: n_pairs * 2,
            n_steady: n_pairs + 2,
            n_pre: n_pairs / 2,
            n_post: n_pairs - n_pairs / 2,
            pre_offset_db: Some(offset_db),
            post_offset_db: Some(offset_db),
            first_pair_secs: 10.0,
            last_pair_secs: 20.0,
            map_lag_ms: Some(0.0),
        }
    }

    fn decide(collar: Option<&CollarLevelOffset>, enabled: bool, prior: f64) -> LocalGainDecision {
        collar_residual_gain_default(collar, enabled, prior, true)
    }

    #[test]
    fn flag_off_is_unity_and_reports_itself() {
        // The collar is fine; only the flag is not. Reported as `flag_off` rather than as whatever
        // the gap would also have failed, so `--no-normalize` runs are readable.
        let c = collar(6.0, 0.2, 20);
        let got = decide(Some(&c), false, 0.0);
        assert_eq!(got.gain(), 1.0);
        assert_eq!(got.reason(), LocalGainReason::FlagOff);
    }

    #[test]
    fn absent_collar_with_incomparable_channels_is_channels_not_abstained_n() {
        // The case the `channels_comparable` hoist exists for: this must not read as `abstained_n`
        // even though the collar is absent for that reason too.
        let got = collar_residual_gain_default(None, true, 0.0, false);
        assert_eq!(got.reason(), LocalGainReason::Channels);
    }

    /// Pins the branch that makes the `channels` refusal this function's own rather than a
    /// consequence of what `scan_gaps` happens to stamp.
    ///
    /// It is **not reachable through production today**: the scan wraps its whole collar-stamping
    /// loop in `if channels_comparable`, so a mismatched pair arrives with `collar: None` and the
    /// absent-collar test above covers it. That is exactly why this one is worth writing — the
    /// argument for the placement is about what happens if that remote gate moves, and an argument
    /// with no test is the kind that gets refactored away. `paired_level_offset` does report on
    /// mismatched pairs, so the populated-collar shape is a stamping decision away, and it is the
    /// shape where the refusal matters most: the programme half is refused for the same reason, so
    /// the prior is zero and the confound would land on the fill at full size.
    #[test]
    fn a_populated_collar_on_incomparable_channels_is_still_refused() {
        let c = collar(2.0, 0.1, 20);
        assert_eq!(
            decide(Some(&c), true, 0.0).reason(),
            LocalGainReason::Applied,
            "control: this collar is otherwise admissible"
        );
        let got = collar_residual_gain_default(Some(&c), true, 0.0, false);
        assert_eq!(got.reason(), LocalGainReason::Channels);
        assert_eq!(got.gain(), 1.0);
    }

    #[test]
    fn absent_collar_with_comparable_channels_is_abstained_n() {
        let got = collar_residual_gain_default(None, true, 0.0, true);
        assert_eq!(got.reason(), LocalGainReason::AbstainedN);
    }

    #[test]
    fn thin_collar_abstains_on_count() {
        let c = collar(6.0, 0.2, MIN_LOCAL_PAIRS - 1);
        let got = decide(Some(&c), true, 0.0);
        assert_eq!(got.gain(), 1.0);
        assert_eq!(got.reason(), LocalGainReason::AbstainedN);
    }

    #[test]
    fn exactly_the_floor_is_admitted() {
        // Pins the comparison as `<`, not `<=`: the constant is a minimum, not an exclusive bound.
        let c = collar(0.0, 0.1, MIN_LOCAL_PAIRS);
        assert_eq!(
            decide(Some(&c), true, 0.0).reason(),
            LocalGainReason::Applied
        );
    }

    #[test]
    fn one_sided_collar_abstains_on_dispersion_at_any_pair_count() {
        // The tail MIN_LOCAL_PAIRS does not carry (§ 4b Result 4): eight of the eleven worst
        // collars in the corpus are one-sided, and they are not thin.
        let mut c = collar(9.0, 0.05, 200);
        c.n_pre = 0;
        c.n_post = 200;
        c.pre_offset_db = None;
        let got = decide(Some(&c), true, 0.0);
        assert_eq!(got.gain(), 1.0);
        assert_eq!(got.reason(), LocalGainReason::AbstainedDispersion);
    }

    #[test]
    fn dispersed_collar_abstains_just_past_the_se_bound() {
        // Drive the branch off the bound itself, not a literal, so moving the constant does not
        // rewrite the test. `spread` is chosen so `se_db` lands either side of it.
        let n = 16usize;
        let per_spread = 1.2533 * 1.4826 / (n as f64).sqrt();
        let under = collar(1.0, (MAX_LOCAL_SE_DB / per_spread) * 0.95, n);
        let over = collar(1.0, (MAX_LOCAL_SE_DB / per_spread) * 1.05, n);
        assert!(under.se_db().unwrap() < MAX_LOCAL_SE_DB);
        assert!(over.se_db().unwrap() > MAX_LOCAL_SE_DB);
        assert_eq!(
            decide(Some(&under), true, 0.0).reason(),
            LocalGainReason::Applied
        );
        assert_eq!(
            decide(Some(&over), true, 0.0).reason(),
            LocalGainReason::AbstainedDispersion
        );
    }

    #[test]
    fn residual_is_taken_against_the_prior_not_the_raw_offset() {
        // The identity: programme applied +6, collar reads +6, so the file average already explains
        // the whole difference and the local half has nothing left to do.
        let c = collar(6.0, 0.1, 20);
        let got = decide(Some(&c), true, 6.0);
        assert_eq!(got.reason(), LocalGainReason::Applied);
        assert!((got.gain() - 1.0).abs() < 1e-6, "got {}", got.gain());
    }

    #[test]
    fn refused_programme_gain_means_a_zero_prior_not_a_missing_one() {
        // No corpus pair exercises this — all 51 apply their programme offset — so it exists only
        // here. Prior 0 makes the collar a hole-local master match on its own: g_local = -d_local.
        let c = collar(2.0, 0.1, 20);
        let got = decide(Some(&c), true, 0.0);
        assert_eq!(got.reason(), LocalGainReason::Applied);
        assert!((got.gain() - 0.794_328).abs() < 1e-4, "got {}", got.gain());
    }

    #[test]
    fn a_zero_prior_puts_the_residual_bound_on_d_local_itself() {
        // **Known consequence, pinned so it is a decision rather than a surprise.** With the
        // programme half refused the prior is 0, so `residual == d_local` and
        // MAX_LOCAL_RESIDUAL_DB bounds the pair's whole master difference instead of the part the
        // file average failed to explain. A pair genuinely offset by more than the bound is then
        // corrected by neither half: programme gain refused it, and every collar on it reads
        // `refused_residual`.
        //
        // That is the specified behaviour (§ 4: refuse past the bound, unconditionally) and the
        // bound is doing its stated job — a several-dB reading against a *zero* prior really can be
        // a collar reaching wrong material. But the 3.0 was calibrated on residuals taken against
        // an APPLIED prior, where the corpus maximum is 1.573 dB; nothing has measured the
        // prior-zero distribution, because no corpus pair produces one. Do not read this test as
        // evidence that 3.0 is right for this case — read it as the reason someone has to look
        // before a refusing-programme corpus arrives.
        let c = collar(MAX_LOCAL_RESIDUAL_DB + 0.5, 0.1, 20);
        assert_eq!(
            decide(Some(&c), true, 0.0).reason(),
            LocalGainReason::RefusedResidual
        );
        // The same collar against a prior that explains most of it is admitted, which is the point:
        // the bound is on the residual, and only a refusal makes the residual the whole offset.
        assert_eq!(
            decide(Some(&c), true, MAX_LOCAL_RESIDUAL_DB).reason(),
            LocalGainReason::Applied
        );
    }

    #[test]
    fn the_two_halves_multiply_to_the_local_offset_not_its_square() {
        // g_program_db + g_local_db = -d_local. The double-count guard, stated as the plan states
        // it: with the programme half applied at +6 and the collar reading +6, the product is the
        // single correction, not 0.501^2 = 0.251.
        let d_local = 6.0f64;
        let program_offset_db = 6.0f64;
        let g_program = 10f64.powf(-program_offset_db / 20.0) as f32;
        let c = collar(d_local, 0.1, 20);
        let g_local = decide(Some(&c), true, program_offset_db).gain();
        let expected = 10f64.powf(-d_local / 20.0) as f32;
        assert!(
            (g_program * g_local - expected).abs() < 1e-6,
            "product {} vs {}",
            g_program * g_local,
            expected
        );
    }

    #[test]
    fn attenuation_is_allowed() {
        // Explicitly not boost-only: the RMS estimator's rail does not transfer to a scalar taken
        // from paired shoulders (plan § 4, gap 24/g46).
        let c = collar(2.0, 0.1, 20);
        let got = decide(Some(&c), true, 0.0);
        assert!(got.gain() < 1.0, "got {}", got.gain());
    }

    #[test]
    fn residual_past_the_bound_refuses_rather_than_clamping() {
        // Assert against the constant, not a literal: past the bound the answer is unity, NOT
        // `10^(-MAX_LOCAL_RESIDUAL_DB/20)`. Clamping would apply the bound's worth of a number the
        // gate has just said it disbelieves.
        let c = collar(MAX_LOCAL_RESIDUAL_DB + 0.5, 0.1, 20);
        let got = decide(Some(&c), true, 0.0);
        assert_eq!(got.reason(), LocalGainReason::RefusedResidual);
        assert_eq!(got.gain(), 1.0);
    }

    #[test]
    fn residual_exactly_at_the_bound_is_applied() {
        let c = collar(MAX_LOCAL_RESIDUAL_DB, 0.1, 20);
        assert_eq!(
            decide(Some(&c), true, 0.0).reason(),
            LocalGainReason::Applied
        );
    }

    #[test]
    fn a_non_finite_reading_fails_closed_instead_of_reaching_the_fill() {
        // What the inverted comparisons are for. A NaN spread makes `se_db` NaN, which passes every
        // `>` test written the obvious way and lands a NaN multiplier on the fill.
        let nan_spread = collar(1.0, f64::NAN, 20);
        assert_eq!(
            decide(Some(&nan_spread), true, 0.0).reason(),
            LocalGainReason::AbstainedDispersion
        );
        let nan_offset = collar(f64::NAN, 0.1, 20);
        let got = decide(Some(&nan_offset), true, 0.0);
        assert_eq!(got.reason(), LocalGainReason::RefusedResidual);
        assert_eq!(got.gain(), 1.0);
    }

    #[test]
    fn reason_serializes_as_snake_case() {
        let json = serde_json::to_string(&LocalGainReason::AbstainedDispersion).unwrap();
        assert_eq!(json, "\"abstained_dispersion\"");
    }
}
