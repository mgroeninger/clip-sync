//! Pure fit-joint routing decisions (`docs/TEMP-fit-routing-extraction-plan.md`).
//!
//! Separates the **decision** layer (which candidate screens as terminal, how candidates rank and
//! tie-break) from the **measurement** layer (scoring a bracket against B audio, and the
//! residual/floor probe, both in `patch_region`). Every input here is a Pearson-derived number, so
//! these functions are deterministic and unit-testable without audio — mirroring the residual-gate
//! `FloorOracleRun` oracles. `patch_region` projects each `FitJointCandidate` to a [`CandidateScore`]
//! and drives its `max_by` / `sort_by` / screen decisions through these functions, so the routing
//! rules have one tested home.
//!
//! ## Residual is applied by the driver, in router order
//!
//! `CandidateScore.confidence` and `ranking_score` are the **Pearson** classification/rank — they do
//! not include the residual/floor verdict. The driver applies residual *lazily at selection*: it
//! confirms the residual verdict for the candidate(s) these functions point at, in this order, and
//! falls through to the next on a residual veto (`select_joint_fit_winner_with_residual`,
//! `try_finalize_high_joint_candidate`). This keeps the residual probe off the cold path while the
//! *ordering* stays pure.
//!
//! ## The decision rules
//!
//! - [`terminates_high`] — E1/E3/E6 screen: a `High` (Pearson) candidate may terminate (driver then
//!   confirms residual).
//! - [`baseline_only_accepts`] — E2/E4: under `baseline_only`, `High`/`Marginal` is accepted without
//!   the grid.
//! - [`selection_cmp`] — `max_by` order for picking the best candidate (E3 `best_high`, E4 global
//!   best): higher `ranking_score`, then **larger** `boundary_move`.
//! - [`winner_cmp`] — `sort_by` order for the residual winner walk (E5/E7): higher `ranking_score`,
//!   then **smaller** `boundary_move`.
//!
//! The tie-break asymmetry between [`selection_cmp`] and [`winner_cmp`] is load-bearing and preserved
//! verbatim from the pre-extraction code (see plan §6 / decision log), not introduced here.

use std::cmp::Ordering;

use crate::domain::gap_fill_fit::FillConfidence;
use crate::domain::repair_profile::FitBoundarySearch;

/// The decision inputs the router needs from one gate-passing candidate placement.
///
/// `ranking_score` is `fit_candidate_ranking_score(min(pre,post), boundary_move)` (Pearson-based,
/// residual-independent). `confidence` is the Pearson classification (`classify_fill_waveform_
/// confidence`); the driver may still downgrade/veto via residual at selection (see module docs).
/// Identity, `anchor_seam_used`, residual, etc. stay on the driver's `FitJointCandidate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CandidateScore {
    pub confidence: FillConfidence,
    pub boundary_move: usize,
    pub ranking_score: f64,
}

/// E1/E3/E6 screen: a `High` (Pearson) candidate is eligible to terminate routing immediately, in
/// any boundary-search mode. The driver still confirms the residual verdict before accepting.
pub(crate) fn terminates_high(confidence: FillConfidence) -> bool {
    confidence == FillConfidence::High
}

/// E2/E4: under `baseline_only`, a `High` or `Marginal` candidate is accepted without the grid.
///
/// Pure twin of `patch_region::accepts_baseline_without_boundary_grid` (which delegates here).
pub(crate) fn baseline_only_accepts(search: FitBoundarySearch, confidence: FillConfidence) -> bool {
    search == FitBoundarySearch::BaselineOnly
        && matches!(confidence, FillConfidence::High | FillConfidence::Marginal)
}

/// `max_by` order for *selecting* the best candidate: higher `ranking_score`, then higher
/// `boundary_move`. Faithful to `patch_region::joint_candidate_ranking_cmp` (ranking asc, then move
/// asc) used with `Iterator::max_by`.
pub(crate) fn selection_cmp(a: &CandidateScore, b: &CandidateScore) -> Ordering {
    a.ranking_score
        .partial_cmp(&b.ranking_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.boundary_move.cmp(&b.boundary_move))
}

/// `sort_by` order for the residual winner walk: higher `ranking_score` first, then **smaller**
/// `boundary_move`. Faithful to the sort in `select_joint_fit_winner_with_residual`.
pub(crate) fn winner_cmp(a: &CandidateScore, b: &CandidateScore) -> Ordering {
    b.ranking_score
        .partial_cmp(&a.ranking_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.boundary_move.cmp(&b.boundary_move))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(confidence: FillConfidence, boundary_move: usize, ranking_score: f64) -> CandidateScore {
        CandidateScore {
            confidence,
            boundary_move,
            ranking_score,
        }
    }

    #[test]
    fn high_screens_in_any_mode() {
        assert!(terminates_high(FillConfidence::High));
        // E1 is mode-independent: a High baseline short-circuits even with the grid enabled.
        assert!(baseline_only_accepts(FitBoundarySearch::BaselineOnly, FillConfidence::High));
        assert!(!baseline_only_accepts(FitBoundarySearch::FullGrid, FillConfidence::High));
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
        assert!(!terminates_high(FillConfidence::Marginal));
    }

    #[test]
    fn selection_max_by_picks_highest_ranking_then_larger_move() {
        let pool = vec![
            cand(FillConfidence::High, 10, 0.40),
            cand(FillConfidence::High, 5, 0.55), // highest rank
        ];
        let best = pool.iter().max_by(|a, b| selection_cmp(a, b)).unwrap();
        assert_eq!(best.ranking_score, 0.55);

        // Tie on ranking_score → max_by keeps the LARGER boundary_move.
        let tied = vec![cand(FillConfidence::High, 3, 0.5), cand(FillConfidence::High, 7, 0.5)];
        assert_eq!(
            tied.iter().max_by(|a, b| selection_cmp(a, b)).unwrap().boundary_move,
            7
        );
    }

    #[test]
    fn best_high_via_filter_max_by_ignores_marginal() {
        // The driver's E3 selection: filter High, then max_by selection_cmp.
        let pool = vec![
            cand(FillConfidence::Marginal, 0, 0.95), // higher rank but Marginal
            cand(FillConfidence::High, 10, 0.40),
            cand(FillConfidence::High, 5, 0.55),
        ];
        let best = pool
            .iter()
            .filter(|c| terminates_high(c.confidence))
            .max_by(|a, b| selection_cmp(a, b));
        assert_eq!(best.unwrap().ranking_score, 0.55);

        let all_marginal = vec![cand(FillConfidence::Marginal, 0, 0.3)];
        assert!(all_marginal
            .iter()
            .filter(|c| terminates_high(c.confidence))
            .max_by(|a, b| selection_cmp(a, b))
            .is_none());
    }

    #[test]
    fn winner_sort_is_ranking_desc_then_smaller_move() {
        // The driver walks this order applying residual; a vetoed top candidate falls through.
        let mut pool = vec![
            cand(FillConfidence::High, 0, 0.40),
            cand(FillConfidence::High, 20, 0.51),
            cand(FillConfidence::Marginal, 0, 0.30),
        ];
        pool.sort_by(|a, b| winner_cmp(a, b));
        let order: Vec<f64> = pool.iter().map(|c| c.ranking_score).collect();
        assert_eq!(order, vec![0.51, 0.40, 0.30]);
    }

    #[test]
    fn selection_and_winner_tie_break_in_opposite_directions() {
        // The asymmetry is load-bearing and must survive the extraction:
        let mut pool = vec![cand(FillConfidence::High, 3, 0.5), cand(FillConfidence::High, 7, 0.5)];
        // selection (E3) keeps the LARGER move:
        assert_eq!(
            pool.iter().max_by(|a, b| selection_cmp(a, b)).unwrap().boundary_move,
            7
        );
        // winner (E5/E7) puts the SMALLER move first:
        pool.sort_by(|a, b| winner_cmp(a, b));
        assert_eq!(pool[0].boundary_move, 3);
    }

    #[test]
    fn composed_baseline_high_short_circuits_before_anchor() {
        // The A5 reality as a pure decision: baseline scores High → E1 screen passes, anchors
        // never consulted (the driver returns before building anchor candidates).
        assert!(terminates_high(FillConfidence::High));
    }

    #[test]
    fn composed_dead_zone_baseline_does_not_screen_or_accept() {
        // Baseline below acceptance → driver proceeds to anchor/grid; an anchor High would screen.
        assert!(!terminates_high(FillConfidence::Marginal));
        assert!(!baseline_only_accepts(FitBoundarySearch::FullGrid, FillConfidence::Marginal));
        assert!(terminates_high(FillConfidence::High));
    }
}
