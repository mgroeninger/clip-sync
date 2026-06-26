//! Pure fit-joint routing decisions (`docs/TEMP-fit-routing-extraction-plan.md`).
//!
//! Separates the **decision** layer (which exit the router takes, which candidate wins) from the
//! **measurement** layer (scoring a bracket against B audio in `patch_region`). Every input here is
//! a residual-finalized number, so these functions are deterministic and unit-testable without
//! audio — mirroring the residual-gate `FloorOracleRun` oracles.
//!
//! The seven terminal exits this module encodes (precedence order):
//!
//! | # | Exit | Predicate |
//! |---|------|-----------|
//! | E1 | Baseline High | [`terminates_high`] on the baseline candidate (any search mode) |
//! | E2 | Baseline accept | [`baseline_only_accepts`] on the baseline candidate |
//! | E3 | Anchor High | [`best_high`] over anchor candidates |
//! | E4 | Anchor accept | [`best_by_ranking`] anchor + [`baseline_only_accepts`] |
//! | E5 | BaselineOnly winner | [`select_pool_winner`] (no grid) |
//! | E6 | Grid High (early) | [`terminates_high`] on a grid candidate, scan order |
//! | E7 | Grid winner | [`select_pool_winner`] over the full pool |
//!
//! `CandidateScore.confidence` is **already residual-finalized** by the driver, so the router never
//! sees a provisional `High` — which is what lets the old `defer_residual` pool-vs-best fork collapse
//! (see plan §5). Only candidates that *passed* the gate become a [`CandidateScore`]; gate/residual
//! failures are tracked by the driver as `best_below_floor` and are not represented here.

// Step 2 lands the pure module + number tests; the driver still inlines the equivalent logic until
// step 3 wires `patch_region` to these functions. Remove this allow when step 3 lands.
#![allow(dead_code)]

use std::cmp::Ordering;

use crate::domain::gap_fill_fit::FillConfidence;
use crate::domain::policies::RefinedGapFrames;
use crate::domain::repair_profile::FitBoundarySearch;

/// One residual-finalized candidate placement, reduced to the fields the router decides on.
///
/// `refined` is the identity the driver maps back to the full `SeamGateOutcome`; `ranking_score`
/// is `fit_candidate_ranking_score(min(pre,post), boundary_move)` computed at scoring time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CandidateScore {
    pub refined: RefinedGapFrames,
    pub pre: f64,
    pub post: f64,
    pub confidence: FillConfidence,
    pub boundary_move: usize,
    pub ranking_score: f64,
    pub anchor_seam_used: bool,
}

/// E1/E6: a `High` candidate terminates routing immediately, in any boundary-search mode.
pub(crate) fn terminates_high(c: &CandidateScore) -> bool {
    c.confidence == FillConfidence::High
}

/// E2/E4: under `baseline_only`, a `High` or `Marginal` candidate is accepted without the grid.
///
/// Pure twin of `patch_region::accepts_baseline_without_boundary_grid` (which `patch_region`
/// delegates here in step 3); kept on `confidence` alone so both call sites share one rule.
pub(crate) fn baseline_only_accepts(search: FitBoundarySearch, confidence: FillConfidence) -> bool {
    search == FitBoundarySearch::BaselineOnly
        && matches!(confidence, FillConfidence::High | FillConfidence::Marginal)
}

/// Ranking order for *selection* (`max_by`): higher `ranking_score`, then higher `boundary_move`.
///
/// Faithful to `patch_region::joint_candidate_ranking_cmp` (ranking asc, then move asc) used with
/// `Iterator::max_by`. NB: this tie-breaks toward the **larger** move, opposite to
/// [`select_pool_winner`]; the asymmetry is preserved from the pre-extraction code, not introduced
/// here (see plan §6 / decision log).
fn selection_ranking_cmp(a: &CandidateScore, b: &CandidateScore) -> Ordering {
    a.ranking_score
        .partial_cmp(&b.ranking_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.boundary_move.cmp(&b.boundary_move))
}

/// E3/global: highest-ranked candidate (mirrors `global_best_joint_candidate` / `max_by`).
pub(crate) fn best_by_ranking(candidates: &[CandidateScore]) -> Option<&CandidateScore> {
    candidates.iter().max_by(|a, b| selection_ranking_cmp(a, b))
}

/// E3: highest-ranked `High` candidate (mirrors `best_high_joint_candidate`).
pub(crate) fn best_high(candidates: &[CandidateScore]) -> Option<&CandidateScore> {
    candidates
        .iter()
        .filter(|c| terminates_high(c))
        .max_by(|a, b| selection_ranking_cmp(a, b))
}

/// E5/E7: winner from the passing pool — higher `ranking_score`, then **smaller** `boundary_move`.
///
/// Faithful to the sort in `select_joint_fit_winner_with_residual` (ranking desc, then move asc,
/// take first). Residual gating is already baked into the pool (failures never reach here), so the
/// lazy per-candidate residual finalize in the old code is a no-op at this layer.
pub(crate) fn select_pool_winner(pool: &[CandidateScore]) -> Option<&CandidateScore> {
    pool.iter().min_by(|a, b| {
        b.ranking_score
            .partial_cmp(&a.ranking_score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.boundary_move.cmp(&b.boundary_move))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(
        pre: f64,
        post: f64,
        confidence: FillConfidence,
        boundary_move: usize,
        ranking_score: f64,
        anchor_seam_used: bool,
    ) -> CandidateScore {
        CandidateScore {
            refined: RefinedGapFrames {
                start_frame: 0,
                end_frame: 100,
            },
            pre,
            post,
            confidence,
            boundary_move,
            ranking_score,
            anchor_seam_used,
        }
    }

    #[test]
    fn high_candidate_terminates_in_any_mode() {
        let c = cand(0.6, 0.5, FillConfidence::High, 0, 0.5, false);
        assert!(terminates_high(&c));
        // E1 is mode-independent: a High baseline short-circuits even with the grid enabled.
        assert!(baseline_only_accepts(FitBoundarySearch::BaselineOnly, c.confidence));
        assert!(!baseline_only_accepts(FitBoundarySearch::FullGrid, c.confidence));
    }

    #[test]
    fn marginal_accepts_only_under_baseline_only() {
        // E2/E4: Marginal is terminal only without the grid.
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
        let c = cand(0.30, 0.30, FillConfidence::Marginal, 0, 0.30, false);
        assert!(!terminates_high(&c));
    }

    #[test]
    fn best_high_picks_highest_ranking_high_and_ignores_marginal() {
        let pool = vec![
            cand(0.30, 0.30, FillConfidence::Marginal, 0, 0.95, false), // higher rank but Marginal
            cand(0.50, 0.50, FillConfidence::High, 10, 0.40, false),
            cand(0.60, 0.55, FillConfidence::High, 5, 0.55, true), // best High
        ];
        let best = best_high(&pool).expect("a High candidate");
        assert_eq!(best.ranking_score, 0.55);
        assert!(best.anchor_seam_used);
    }

    #[test]
    fn best_high_none_when_all_marginal() {
        let pool = vec![
            cand(0.30, 0.30, FillConfidence::Marginal, 0, 0.30, false),
            cand(0.28, 0.28, FillConfidence::Marginal, 0, 0.28, false),
        ];
        assert!(best_high(&pool).is_none());
    }

    #[test]
    fn selection_tie_breaks_toward_larger_move_winner_toward_smaller() {
        // Two equal ranking_score candidates; the asymmetry between selection (max_by) and the
        // pool winner (sort) is load-bearing and must be preserved.
        let a = cand(0.5, 0.5, FillConfidence::High, 3, 0.50, false);
        let b = cand(0.5, 0.5, FillConfidence::High, 7, 0.50, false);
        let pool = vec![a, b];
        // selection (E3) keeps the LARGER boundary_move on a tie (max_by over move-asc cmp).
        assert_eq!(best_by_ranking(&pool).unwrap().boundary_move, 7);
        // winner (E5/E7) keeps the SMALLER boundary_move on a tie.
        assert_eq!(select_pool_winner(&pool).unwrap().boundary_move, 3);
    }

    #[test]
    fn select_pool_winner_prefers_highest_ranking() {
        let pool = vec![
            cand(0.40, 0.40, FillConfidence::High, 0, 0.40, false),
            cand(0.55, 0.50, FillConfidence::High, 20, 0.51, false),
            cand(0.30, 0.30, FillConfidence::Marginal, 0, 0.30, false),
        ];
        assert_eq!(select_pool_winner(&pool).unwrap().ranking_score, 0.51);
    }

    #[test]
    fn select_pool_winner_none_on_empty_pool() {
        assert!(select_pool_winner(&[]).is_none());
    }

    #[test]
    fn composed_baseline_high_short_circuits_before_anchor() {
        // The A5 reality as a pure decision: baseline scores High → E1, anchors never consulted.
        let baseline = cand(0.99, 0.99, FillConfidence::High, 0, 0.99, false);
        assert!(terminates_high(&baseline));
        // (driver: because terminates_high is true, it returns before building anchor candidates.)
    }

    #[test]
    fn composed_dead_zone_baseline_yields_to_anchor_high() {
        // Baseline below acceptance, an anchor bracket is High → E3 wins with anchor_seam_used.
        let baseline = cand(0.10, 0.10, FillConfidence::Marginal, 0, 0.10, false);
        assert!(!terminates_high(&baseline));
        assert!(!baseline_only_accepts(FitBoundarySearch::FullGrid, baseline.confidence));
        let anchors = vec![cand(0.40, 0.40, FillConfidence::High, 300, 0.31, true)];
        let winner = best_high(&anchors).expect("anchor High");
        assert!(winner.anchor_seam_used);
        assert!(winner.boundary_move > 0);
    }
}
