use crate::domain::{HighRateRefinement, OffsetVerification};

/// Human-readable lines for high-rate refinement (CLI / repair reports).
pub fn format_high_rate_refinement_lines(
    refine: &HighRateRefinement,
    show_diagnostics: bool,
) -> Vec<String> {
    if refine.applied {
        if show_diagnostics {
            return vec![format!(
                "High-rate: +{:.3}s refinement applied (peak {:.2})",
                refine.adjustment_secs, refine.correlation_peak
            )];
        }
        return vec![format!(
            "High-rate: +{:.3}s refinement applied",
            refine.adjustment_secs
        )];
    }
    if show_diagnostics {
        let reason = refine.skip_reason.as_deref().unwrap_or("not applied");
        return vec![format!("High-rate: skipped ({reason})")];
    }
    vec![]
}

/// Human-readable lines for hold-out offset verification.
pub fn format_offset_verification_lines(
    verify: &OffsetVerification,
    show_diagnostics: bool,
) -> Vec<String> {
    if verify.skipped {
        if show_diagnostics {
            let reason = verify.skip_reason.as_deref().unwrap_or("unknown");
            return vec![format!("Verify:    skipped ({reason})")];
        }
        return vec![];
    }
    if !verify.verified {
        return vec![format!(
            "Verify:    offset not independently verified (hold-out confidence {:.2})",
            verify.confidence
        )];
    }
    if show_diagnostics {
        return vec![format!(
            "Verify:    offset confirmed at hold-out window (confidence {:.2})",
            verify.confidence
        )];
    }
    vec![]
}
