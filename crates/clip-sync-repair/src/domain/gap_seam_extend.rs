//! Gap boundary adjustment when waveform seam correlation fails at one end.

/// Post failed narrowly relative to pre (both may be below threshold).
const POST_NARROW_FAIL_TOLERANCE: f64 = 0.05;

/// Minimum pre−post lead when post collapsed but pre is still below threshold.
const POST_COLLAPSED_PRE_LEAD: f64 = 0.10;

/// True when extending the gap end may fix a weak post seam without retrying hopeless cases.
pub fn post_seam_extension_candidate(pre: f64, post: f64, min_correlation: f32) -> bool {
    let min = min_correlation as f64;
    if post >= min {
        return false;
    }
    // Mirror of pre-seam extension: pre passes, post fails.
    if pre >= min {
        return true;
    }
    // Post failed narrowly relative to pre.
    if post >= pre - POST_NARROW_FAIL_TOLERANCE {
        return true;
    }
    // Post collapsed but pre is materially stronger (neither passes min).
    pre >= post + POST_COLLAPSED_PRE_LEAD
}

/// True when shifting the gap start earlier may fix a weak pre seam while post already passes.
pub fn pre_seam_extension_candidate(pre: f64, post: f64, min_correlation: f32) -> bool {
    let min = min_correlation as f64;
    pre < min && post >= min
}

/// Short gaps may pass when either seam meets the threshold (after mean rule fails).
pub fn short_gap_one_strong_seam_passes(pre: f32, post: f32, min_correlation: f32) -> bool {
    pre >= min_correlation || post >= min_correlation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_candidate_when_post_fails_but_not_much_worse_than_pre() {
        assert!(post_seam_extension_candidate(0.0, 0.11, 0.12));
    }

    #[test]
    fn post_not_candidate_when_pre_seam_also_collapsed() {
        assert!(!post_seam_extension_candidate(0.05, -0.02, 0.12));
    }

    #[test]
    fn post_not_candidate_when_post_passes() {
        assert!(!post_seam_extension_candidate(0.4, 0.5, 0.35));
    }

    #[test]
    fn post_candidate_when_pre_passes_but_post_fails() {
        assert!(post_seam_extension_candidate(0.17, 0.05, 0.12));
    }

    #[test]
    fn post_candidate_when_post_collapsed_but_pre_materially_stronger() {
        assert!(post_seam_extension_candidate(0.06, -0.13, 0.12));
    }

    #[test]
    fn pre_candidate_when_pre_fails_but_post_passes() {
        assert!(pre_seam_extension_candidate(-0.01, 0.17, 0.12));
    }

    #[test]
    fn pre_not_candidate_when_post_also_fails() {
        assert!(!pre_seam_extension_candidate(0.06, -0.13, 0.12));
    }

    #[test]
    fn pre_not_candidate_when_pre_passes() {
        assert!(!pre_seam_extension_candidate(0.4, 0.2, 0.35));
    }

    #[test]
    fn one_strong_seam_passes_when_post_is_strong() {
        assert!(short_gap_one_strong_seam_passes(-0.01_f32, 0.17, 0.12));
    }

    #[test]
    fn one_strong_seam_fails_when_both_weak() {
        assert!(!short_gap_one_strong_seam_passes(0.06, -0.13, 0.12));
    }
}
