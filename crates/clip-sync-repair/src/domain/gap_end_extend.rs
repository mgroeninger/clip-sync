//! Post-seam gap-end extension when waveform correlation fails at the closing boundary.

/// True when extending the gap end may fix a weak post seam without retrying hopeless cases.
pub fn post_seam_extension_candidate(pre: f64, post: f64, min_correlation: f32) -> bool {
    let min = min_correlation as f64;
    post < min && post >= pre - 0.05
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_when_post_fails_but_not_much_worse_than_pre() {
        assert!(post_seam_extension_candidate(0.0, 0.11, 0.12));
    }

    #[test]
    fn not_candidate_when_pre_seam_also_collapsed() {
        assert!(!post_seam_extension_candidate(0.05, -0.02, 0.12));
    }

    #[test]
    fn not_candidate_when_post_passes() {
        assert!(!post_seam_extension_candidate(0.4, 0.5, 0.35));
    }
}
