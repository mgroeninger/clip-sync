//! Phase 0 window oracles for anchored end extraction.
//!
//! Phase 1 `clip_windows_paired` tests should assert against these values. The helpers encode
//! expected placement rules before the paired planner exists.

/// Default CI integration scale from [TEMP-anchored-end-extraction-plan.md](../../../../docs/TEMP-anchored-end-extraction-plan.md).
pub const CI_SHARED_SECS: u32 = 240;
pub const CI_LONG_SECS: u32 = 1800;
pub const CI_CLIP_LENGTH_SECS: u64 = 60;
pub const CI_SAMPLE_RATE: u32 = 11_025;

/// Inclusive-exclusive window bounds in whole seconds (matches clip planning config).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipWindowSecs {
    pub start_secs: u64,
    pub end_secs: u64,
}

/// Scenario inputs for oracle calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchoredEndScenario {
    pub t_anchor_secs: u64,
    pub clip_length_secs: u64,
    pub num_clips: u32,
}

impl AnchoredEndScenario {
    pub const CI_DEFAULT: Self = Self {
        t_anchor_secs: CI_SHARED_SECS as u64,
        clip_length_secs: CI_CLIP_LENGTH_SECS,
        num_clips: 2,
    };

    /// Documentary 40 min excerpt vs 300 min master (`clip_length` 15 min).
    pub const DOC_40_300: Self = Self {
        t_anchor_secs: 40 * 60,
        clip_length_secs: 15 * 60,
        num_clips: 2,
    };

    /// Near-equal pair regression (45 min each, 15 min clips).
    pub const EQUAL_45: Self = Self {
        t_anchor_secs: 45 * 60,
        clip_length_secs: 15 * 60,
        num_clips: 2,
    };
}

/// Start clip under symmetric multi-clip planning.
pub fn start_window(scenario: AnchoredEndScenario) -> ClipWindowSecs {
    ClipWindowSecs {
        start_secs: 0,
        end_secs: scenario.clip_length_secs,
    }
}

/// End clip when both files share `T_anchor` ([`SharedTimeline`] mode).
pub fn shared_timeline_end_window(scenario: AnchoredEndScenario) -> ClipWindowSecs {
    let end = scenario.t_anchor_secs;
    ClipWindowSecs {
        start_secs: end.saturating_sub(scenario.clip_length_secs),
        end_secs: end,
    }
}

/// End clip for one file in legacy [`FileTail`] mode.
pub fn file_tail_end_window(timeline_end_secs: u64, clip_length_secs: u64) -> ClipWindowSecs {
    ClipWindowSecs {
        start_secs: timeline_end_secs.saturating_sub(clip_length_secs),
        end_secs: timeline_end_secs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_default_end_window_oracle() {
        let end = shared_timeline_end_window(AnchoredEndScenario::CI_DEFAULT);
        assert_eq!(end.start_secs, 180);
        assert_eq!(end.end_secs, 240);
        let start = start_window(AnchoredEndScenario::CI_DEFAULT);
        assert_eq!(start.start_secs, 0);
        assert_eq!(start.end_secs, 60);
    }

    #[test]
    fn doc_40_300_shared_timeline_end_on_both_clocks() {
        let scenario = AnchoredEndScenario::DOC_40_300;
        let end = shared_timeline_end_window(scenario);
        assert_eq!(end.start_secs, 25 * 60);
        assert_eq!(end.end_secs, 40 * 60);

        let b_file_tail = file_tail_end_window(300 * 60, scenario.clip_length_secs);
        assert_eq!(b_file_tail.start_secs, 285 * 60);
        assert_eq!(b_file_tail.end_secs, 300 * 60);
        assert_ne!(end, b_file_tail);
    }

    #[test]
    fn equal_duration_shared_timeline_matches_file_tail() {
        let scenario = AnchoredEndScenario::EQUAL_45;
        let shared = shared_timeline_end_window(scenario);
        let file_tail = file_tail_end_window(scenario.t_anchor_secs, scenario.clip_length_secs);
        assert_eq!(shared, file_tail);
    }

    #[test]
    fn near_equal_offset_chirp_pair_has_same_per_file_end_as_shared_when_equal_length() {
        // Regression anchor: existing equal-length symmetric pairs (corpus / offset chirp) place
        // end at [duration − L, duration] per file — identical to SharedTimeline when T_a = T_b.
        let duration_secs = 180;
        let clip_length_secs = 60;
        let scenario = AnchoredEndScenario {
            t_anchor_secs: duration_secs,
            clip_length_secs,
            num_clips: 2,
        };
        assert_eq!(
            shared_timeline_end_window(scenario),
            file_tail_end_window(duration_secs, clip_length_secs)
        );
        assert_eq!(shared_timeline_end_window(scenario).start_secs, 120);
        assert_eq!(shared_timeline_end_window(scenario).end_secs, 180);
    }
}
