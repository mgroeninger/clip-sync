//! Pure fit-joint routing decisions (`docs/TEMP-fit-routing-extraction-plan.md`).
//!
//! Separates the **decision** layer (which exit the router takes, in what order to try candidates)
//! from the **measurement** layer (scoring a bracket against B audio, and the residual/floor probe,
//! both in `patch_region`). Every input here is a Pearson-derived number, so these functions are
//! deterministic and unit-testable without audio — mirroring the residual-gate `FloorOracleRun`
//! oracles.
//!
//! ## Residual is applied by the driver, in router order
//!
//! `CandidateScore.confidence` and `ranking_score` are the **Pearson** classification/rank — they do
//! not include the residual/floor verdict. The driver applies residual *lazily at selection*: it
//! confirms the residual verdict for the candidate(s) the router points at, in router order, and
//! falls through to the next on a residual veto (mirroring `select_joint_fit_winner_with_residual`
//! and `try_finalize_high_joint_candidate`). This keeps the residual probe off the cold path (one
//! probe for the winner, not one per grid cell) while making the *ordering* pure. When residual
//! measurement is disabled the finalize step is a no-op, so the same router order drives both — which
//! is what lets the old `defer_residual` pool-vs-best fork collapse (plan §5).
//!
//! Only candidates that *passed* the structure/waveform gate become a [`CandidateScore`];
//! gate failures are tracked by the driver as `best_below_floor` and are not represented here.
//!
//! ## The seven terminal exits (precedence order)
//!
//! | # | Exit | Router primitive | Driver adds |
//! |---|------|------------------|-------------|
//! | E1 | Baseline High | [`terminates_high`] on baseline | residual confirm |
//! | E2 | Baseline accept | [`baseline_only_accepts`] on baseline | — |
//! | E3 | Anchor High | [`best_high`] over the pool, is-anchor | residual confirm |
//! | E4 | Anchor accept | [`best_by_ranking`] over the pool, is-anchor + accept | — |
//! | E5 | BaselineOnly winner | [`pool_winner_order`] | residual fall-through |
//! | E6 | Grid High (early) | [`terminates_high`] per grid cell, scan order | residual confirm |
//! | E7 | Grid winner | [`pool_winner_order`] | residual fall-through |

// Step 2 lands the pure module + number tests; the driver still inlines the equivalent logic until
// step 3 wires `patch_region` to these functions. Remove this allow when step 3 lands.
#![allow(dead_code)]

use std::cmp::Ordering;

use crate::domain::gap_fill_fit::FillConfidence;
use crate::domain::policies::RefinedGapFrames;
use crate::domain::repair_profile::FitBoundarySearch;

/// One gate-passing candidate placement, reduced to the fields the router decides on.
///
/// `refined` is the identity the driver maps back to the full `SeamGateOutcome`. `ranking_score` is
/// `fit_candidate_ranking_score(min(pre,post), boundary_move)` (Pearson-based, residual-independent),
/// computed at scoring time. `confidence` is the Pearson classification (`classify_fill_waveform_
/// confidence`); the driver may still downgrade/veto it via residual at selection (see module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CandidateScore {
    pub refined: RefinedGapFrames,
    pub confidence: FillConfidence,
    pub boundary_move: usize,
    pub ranking_score: f64,
    pub anchor_seam_used: bool,
}

/// E1/E3/E6 screen: a `High` (Pearson) candidate is eligible to terminate routing immediately, in any
/// boundary-search mode. The driver still confirms the residual verdict before accepting.
pub(crate) fn terminates_high(c: &CandidateScore) -> bool {
    c.confidence == FillConfidence::High
}

/// E2/E4: under `baseline_only`, a `High` or `Marginal` candidate is accepted without the grid.
///
/// Pure twin of `patch_region::accepts_baseline_without_boundary_grid` (which delegates here in
/// step 3); keyed on `confidence` alone so both call sites share one rule.
pub(crate) fn baseline_only_accepts(search: FitBoundarySearch, confidence: FillConfidence) -> bool {
    search == FitBoundarySearch::BaselineOnly
        && matches!(confidence, FillConfidence::High | FillConfidence::Marginal)
}

/// Ranking order for *selection* (`max_by`): higher `ranking_score`, then higher `boundary_move`.
///
/// Faithful to `patch_region::joint_candidate_ranking_cmp` (ranking asc, then move asc) used with
/// `Iterator::max_by`. NB: this tie-breaks toward the **larger** move, opposite to
/// [`pool_winner_order`]; the asymmetry is preserved from the pre-extraction code, not introduced
/// here (see plan §6 / decision log).
fn selection_ranking_cmp(a: &CandidateScore, b: &CandidateScore) -> Ordering {
    a.ranking_score
        .partial_cmp(&b.ranking_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.boundary_move.cmp(&b.boundary_move))
}

/// E3/E4 global: highest-ranked candidate over the whole pool (mirrors `global_best_joint_candidate`
/// / `max_by`). The driver checks `anchor_seam_used` on the result to distinguish the anchor exits.
pub(crate) fn best_by_ranking(pool: &[CandidateScore]) -> Option<&CandidateScore> {
    pool.iter().max_by(|a, b| selection_ranking_cmp(a, b))
}

/// E3: highest-ranked `High` candidate over the whole pool (mirrors `best_high_joint_candidate`).
///
/// At the E3 decision point the baseline is known non-`High` (E1 already returned otherwise), so the
/// best `High` here is necessarily an anchor candidate; the driver still asserts `anchor_seam_used`.
pub(crate) fn best_high(pool: &[CandidateScore]) -> Option<&CandidateScore> {
    pool.iter()
        .filter(|c| terminates_high(c))
        .max_by(|a, b| selection_ranking_cmp(a, b))
}

/// E5/E7 winner order: pool sorted by higher `ranking_score`, then **smaller** `boundary_move`.
///
/// Faithful to the sort in `select_joint_fit_winner_with_residual` (ranking desc, then move asc). The
/// driver walks this order applying residual, returning the first candidate whose residual verdict
/// passes (falling through on veto). Stable: equal-ranked candidates keep input order.
pub(crate) fn pool_winner_order(pool: &[CandidateScore]) -> Vec<&CandidateScore> {
    let mut order: Vec<&CandidateScore> = pool.iter().collect();
    order.sort_by(|a, b| {
        b.ranking_score
            .partial_cmp(&a.ranking_score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.boundary_move.cmp(&b.boundary_move))
    });
    order
}

/// Convenience for the residual-disabled path: the single top-ranked winner (first of
/// [`pool_winner_order`]). When residual is active the driver must walk the full order instead.
pub(crate) fn select_pool_winner(pool: &[CandidateScore]) -> Option<&CandidateScore> {
    pool_winner_order(pool).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(
        confidence: FillConfidence,
        boundary_move: usize,
        ranking_score: f64,
        anchor_seam_used: bool,
    ) -> CandidateScore {
        CandidateScore {
            refined: RefinedGapFrames {
                start_frame: boundary_move, // distinct identity per candidate in tests
                end_frame: boundary_move + 100,
            },
            confidence,
            boundary_move,
            ranking_score,
            anchor_seam_used,
        }
    }

    #[test]
    fn high_candidate_terminates_in_any_mode() {
        let c = cand(FillConfidence::High, 0, 0.5, false);
        assert!(terminates_high(&c));
        // E1 is mode-independent: a High baseline short-circuits even with the grid enabled.
        assert!(baseline_only_accepts(FitBoundarySearch::BaselineOnly, c.confidence));
        assert!(!baseline_only_accepts(FitBoundarySearch::FullGrid, c.confidence));
    }

    #[test]
    fn marginal_accepts_only_under_baseline_only() {
        assert!(baseline_only_accepts(
            FitBoundarySearch::BaselineOnly,
            FillConfidence::Marginal
        ));
        assert!(!baseline_only_accepts(
            FitBoundarySearch::FullGrid,
            FillConfidence::Marginal
        ));
    }

    #[test]
    fn marginal_grid_candidate_does_not_terminate_high() {
        assert!(!terminates_high(&cand(FillConfidence::Marginal, 0, 0.30, false)));
    }

    #[test]
    fn best_high_picks_highest_ranking_high_and_ignores_marginal() {
        let pool = vec![
            cand(FillConfidence::Marginal, 0, 0.95, false), // higher rank but Marginal
            cand(FillConfidence::High, 10, 0.40, false),
            cand(FillConfidence::High, 5, 0.55, true), // best High
        ];
        let best = best_high(&pool).expect("a High candidate");
        assert_eq!(best.ranking_score, 0.55);
        assert!(best.anchor_seam_used);
    }

    #[test]
    fn best_high_none_when_all_marginal() {
        let pool = vec![
            cand(FillConfidence::Marginal, 0, 0.30, false),
            cand(FillConfidence::Marginal, 0, 0.28, false),
        ];
        assert!(best_high(&pool).is_none());
    }

    #[test]
    fn selection_tie_breaks_toward_larger_move_winner_toward_smaller() {
        // Equal ranking_score; the asymmetry between selection (max_by) and the winner order (sort)
        // is load-bearing and must be preserved across the extraction.
        let pool = vec![
            cand(FillConfidence::High, 3, 0.50, false),
            cand(FillConfidence::High, 7, 0.50, false),
        ];
        // selection (E3) keeps the LARGER boundary_move on a tie (max_by over move-asc cmp).
        assert_eq!(best_by_ranking(&pool).unwrap().boundary_move, 7);
        // winner order (E5/E7) puts the SMALLER boundary_move first.
        assert_eq!(pool_winner_order(&pool)[0].boundary_move, 3);
        assert_eq!(select_pool_winner(&pool).unwrap().boundary_move, 3);
    }

    #[test]
    fn pool_winner_order_is_ranking_desc_for_lazy_residual_fallthrough() {
        // The driver walks this order applying residual; a vetoed top candidate falls through to the
        // next. Lock the order so that fall-through is deterministic.
        let pool = vec![
            cand(FillConfidence::High, 0, 0.40, false),
            cand(FillConfidence::High, 20, 0.51, false),
            cand(FillConfidence::Marginal, 0, 0.30, false),
        ];
        let order: Vec<f64> = pool_winner_order(&pool)
            .iter()
            .map(|c| c.ranking_score)
            .collect();
        assert_eq!(order, vec![0.51, 0.40, 0.30]);
    }

    #[test]
    fn select_pool_winner_none_on_empty_pool() {
        assert!(select_pool_winner(&[]).is_none());
        assert!(pool_winner_order(&[]).is_empty());
    }

    #[test]
    fn composed_baseline_high_short_circuits_before_anchor() {
        // The A5 reality as a pure decision: baseline scores High → E1, anchors never consulted.
        let baseline = cand(FillConfidence::High, 0, 0.99, false);
        assert!(terminates_high(&baseline));
    }

    #[test]
    fn composed_dead_zone_baseline_yields_to_anchor_high() {
        // Baseline below acceptance, an anchor bracket is High → E3 wins with anchor_seam_used.
        let baseline = cand(FillConfidence::Marginal, 0, 0.10, false);
        assert!(!terminates_high(&baseline));
        assert!(!baseline_only_accepts(FitBoundarySearch::FullGrid, baseline.confidence));
        // Pool at E3 = baseline (non-High) + anchor candidates.
        let pool = vec![baseline, cand(FillConfidence::High, 300, 0.31, true)];
        let winner = best_high(&pool).expect("anchor High");
        assert!(winner.anchor_seam_used);
        assert!(winner.boundary_move > 0);
    }
}
