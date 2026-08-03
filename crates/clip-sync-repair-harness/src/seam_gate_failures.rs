//! **Why did the seam gate reject these brackets?** — a shared tally over
//! [`W5BracketGateScore`], the one reusable piece of the retired `diag_w5_anchor_rescue` sweep.
//!
//! Every seam/anchor diagnostic asks the same question when a fixture fails to route: how many
//! brackets were offered, how many passed, and at which stage the rejections happened. Three
//! copies of that histogram had accumulated — one per diag test — each with its own missing-stage
//! placeholder and its own idea of whether to count stationary brackets. This is that logic once.
//!
//! Scoring lives in `clip_sync_repair_fixtures::w5_anchor_rescue_diag`; this only summarizes it, so
//! it applies to anything producing `W5BracketGateScore` — `score_w5_fixture` and any successor.

use clip_sync_repair_fixtures::w5_anchor_rescue_diag::W5BracketGateScore;
use std::collections::BTreeMap;

/// The rejection stage that means the bracket *was* structure-aligned and died on waveform
/// correlation alone. Brackets failing earlier never got far enough for their Pearson to mean
/// anything, which is why [`SeamGateFailureTally::best_aligned_post_pearson`] is restricted to this
/// stage — a max over all failures would report a number produced by an unaligned comparison.
pub const WAVEFORM_FLOOR_STAGE: &str = "waveform_floor";

/// Placeholder for a rejected bracket that recorded no stage. Distinct from any real stage name so
/// a gap in the instrumentation cannot masquerade as a gate decision.
pub const UNKNOWN_STAGE: &str = "?";

/// Which brackets to count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BracketFilter {
    /// Every bracket offered.
    All,
    /// Only brackets that actually move (`move_frames != 0`) — the anchor-rescue question, where a
    /// stationary bracket passing tells you nothing about whether the *rescue* works.
    Moving,
}

impl BracketFilter {
    fn admits(self, b: &W5BracketGateScore) -> bool {
        match self {
            Self::All => true,
            Self::Moving => b.move_frames != 0,
        }
    }
}

/// Pass/reject counts plus the per-stage rejection histogram for one set of brackets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeamGateFailureTally {
    /// Brackets admitted by the filter.
    pub considered: usize,
    pub passed: usize,
    /// Rejections by `failure_stage`, [`UNKNOWN_STAGE`] when the bracket recorded none.
    pub by_stage: BTreeMap<&'static str, usize>,
    /// Best `post_pearson` among brackets rejected at [`WAVEFORM_FLOOR_STAGE`] — how close a
    /// structure-aligned bracket came to the confidence floor. `None` when none reached that stage.
    pub best_aligned_post_pearson: Option<f64>,
}

impl SeamGateFailureTally {
    /// Tally `brackets` under `filter`.
    pub fn collect<'a, I>(brackets: I, filter: BracketFilter) -> Self
    where
        I: IntoIterator<Item = &'a W5BracketGateScore>,
    {
        let mut t = Self::default();
        for b in brackets {
            if !filter.admits(b) {
                continue;
            }
            t.considered += 1;
            if b.passed_gate {
                t.passed += 1;
                continue;
            }
            *t.by_stage
                .entry(b.failure_stage.unwrap_or(UNKNOWN_STAGE))
                .or_default() += 1;
            if b.failure_stage == Some(WAVEFORM_FLOOR_STAGE) {
                if let Some(post) = b.post_pearson {
                    t.best_aligned_post_pearson = Some(
                        t.best_aligned_post_pearson
                            .map_or(post, |c: f64| c.max(post)),
                    );
                }
            }
        }
        t
    }

    /// Convenience for the common `BracketFilter::All` case.
    pub fn of<'a, I>(brackets: I) -> Self
    where
        I: IntoIterator<Item = &'a W5BracketGateScore>,
    {
        Self::collect(brackets, BracketFilter::All)
    }

    pub fn rejected(&self) -> usize {
        self.considered - self.passed
    }

    /// One-line summary for diagnostic output. `floor` is the confidence threshold the aligned
    /// Pearson is being measured against, printed alongside it so the number has a scale.
    pub fn summary(&self, floor: Option<f64>) -> String {
        let mut s = format!(
            "{}/{} passed; failure stages: {:?}",
            self.passed, self.considered, self.by_stage
        );
        if let Some(post) = self.best_aligned_post_pearson {
            s.push_str(&format!(" best aligned post_pearson={post:.4}"));
            if let Some(floor) = floor {
                s.push_str(&format!(" (floor {floor:.2})"));
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clip_sync_repair_fixtures::w5_anchor_rescue_diag::W5BracketGateScore;

    fn bracket(
        move_frames: usize,
        passed_gate: bool,
        failure_stage: Option<&'static str>,
        post_pearson: Option<f64>,
    ) -> W5BracketGateScore {
        W5BracketGateScore {
            pre_frame: 0,
            post_frame: 0,
            move_frames,
            pre_prominence: 0.0,
            post_prominence: 0.0,
            passed_gate,
            pre_pearson: None,
            post_pearson,
            min_pearson: None,
            confidence: None,
            ranking_score: None,
            failure_stage,
        }
    }

    #[test]
    fn tallies_passes_and_groups_rejections_by_stage() {
        let brackets = vec![
            bracket(0, true, None, Some(0.9)),
            bracket(2, false, Some(WAVEFORM_FLOOR_STAGE), Some(0.21)),
            bracket(3, false, Some(WAVEFORM_FLOOR_STAGE), Some(0.28)),
            bracket(4, false, Some("structure_align"), Some(0.99)),
            bracket(5, false, None, None),
        ];
        let t = SeamGateFailureTally::of(&brackets);
        assert_eq!((t.considered, t.passed, t.rejected()), (5, 1, 4));
        assert_eq!(t.by_stage[WAVEFORM_FLOOR_STAGE], 2);
        assert_eq!(t.by_stage["structure_align"], 1);
        assert_eq!(t.by_stage[UNKNOWN_STAGE], 1, "missing stage is not silent");
        // Restricted to waveform_floor: the 0.99 from an unaligned bracket must not win.
        assert_eq!(t.best_aligned_post_pearson, Some(0.28));
    }

    /// The anchor-rescue question excludes stationary brackets — a pass there says nothing about
    /// whether the rescue works.
    #[test]
    fn moving_filter_excludes_stationary_brackets() {
        let brackets = vec![
            bracket(0, true, None, None),
            bracket(0, false, Some("structure_align"), None),
            bracket(3, false, Some(WAVEFORM_FLOOR_STAGE), Some(0.3)),
        ];
        let t = SeamGateFailureTally::collect(&brackets, BracketFilter::Moving);
        assert_eq!((t.considered, t.passed), (1, 0));
        assert_eq!(t.by_stage.len(), 1);
        assert_eq!(t.by_stage[WAVEFORM_FLOOR_STAGE], 1);
    }

    #[test]
    fn empty_input_tallies_nothing_rather_than_panicking() {
        let t = SeamGateFailureTally::of(&[]);
        assert_eq!((t.considered, t.passed, t.rejected()), (0, 0, 0));
        assert!(t.by_stage.is_empty());
        assert!(t.best_aligned_post_pearson.is_none());
        assert!(t.summary(Some(0.35)).contains("0/0"));
    }
}
