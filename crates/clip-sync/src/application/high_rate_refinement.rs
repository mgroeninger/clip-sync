use std::time::Duration;

use tracing::debug;

use crate::application::config::AlignmentConfig;
use crate::application::error::{debug_media_error, MediaError};
use crate::application::offset_refinement::refine_holdout_segment_lag;
use crate::application::ports::{MediaSession, PcmCorrelator, ProgressReporter, Resampler};
use crate::domain::clip_window::ClipLabel;
use crate::domain::{
    alignment::clip_with_label,
    holdout_pick_duration, holdout_window_centered_in, holdout_window_feasible,
    refresh_alignment_drift_summary, refresh_start_overlap, resolve_holdout_candidates,
    AlignmentModeUsed, AlignmentResult, AudioTrack, ClipWindow, HighRateAnchorRefinement,
    HighRateRefinement, MediaExtent, MonoPcmClip,
};

pub struct HighRateRefinementInput<'a, MS: MediaSession> {
    pub session_a: &'a mut MS,
    pub session_b: &'a mut MS,
    pub track_a: &'a AudioTrack,
    pub track_b: &'a AudioTrack,
    pub discovery_windows: &'a [ClipWindow],
    pub extent_a: MediaExtent,
    pub extent_b: MediaExtent,
    pub resampler: &'a dyn Resampler,
    pub correlator: &'a dyn PcmCorrelator,
}

pub fn apply_high_rate_refinement<MS: MediaSession>(
    input: &mut HighRateRefinementInput<'_, MS>,
    alignment: &AlignmentConfig,
    result: &mut AlignmentResult,
    progress: &dyn ProgressReporter,
) {
    if !alignment.refine_offset_high_rate {
        return;
    }

    let Some(recommended_offset_secs) = result.recommended_offset_secs else {
        result.high_rate_refinement = Some(skipped_refinement(
            0.0,
            segment_length_secs(alignment),
            "no recommended offset".into(),
        ));
        return;
    };

    progress.phase_verbose("High-rate offset refinement...");
    let segment_length = Duration::from_secs(u64::from(alignment.high_rate_refine_secs));
    let segment_length_secs = segment_length.as_secs_f64();

    let pick_duration = holdout_pick_duration(result, input.extent_a, input.extent_b);
    if segment_length.is_zero() || pick_duration < segment_length {
        debug!(
            pick_duration_secs = pick_duration.as_secs_f64(),
            segment_length_secs,
            "high-rate refine skipped: hold-out longer than available region"
        );
        result.high_rate_refinement = Some(skipped_refinement(
            0.0,
            segment_length_secs,
            "hold-out window unavailable".into(),
        ));
        return;
    }

    let dur_a = input.extent_a.effective().as_secs_f64();
    let dur_b = input.extent_b.effective().as_secs_f64();

    if dual_anchor_eligible(result, input.discovery_windows) {
        apply_dual_anchor_high_rate_refinement(
            input,
            alignment,
            result,
            progress,
            recommended_offset_secs,
            segment_length,
            dur_a,
            dur_b,
        );
        return;
    }

    apply_single_holdout_high_rate_refinement(
        input,
        alignment,
        result,
        progress,
        recommended_offset_secs,
        segment_length,
        segment_length_secs,
        dur_a,
        dur_b,
    );
}

fn dual_anchor_eligible(result: &AlignmentResult, discovery_windows: &[ClipWindow]) -> bool {
    if result.alignment_mode_used == Some(AlignmentModeUsed::QueryReference) {
        return false;
    }
    let start_ready = clip_with_label(&result.clips, ClipLabel::Start)
        .is_some_and(|clip| clip.aligned && clip.offset_secs.is_some());
    let end_ready = clip_with_label(&result.clips, ClipLabel::End)
        .is_some_and(|clip| clip.aligned && clip.offset_secs.is_some());
    let has_windows = discovery_windows.iter().any(|w| w.label == ClipLabel::Start)
        && discovery_windows.iter().any(|w| w.label == ClipLabel::End);
    start_ready && end_ready && has_windows
}

fn apply_dual_anchor_high_rate_refinement<MS: MediaSession>(
    input: &mut HighRateRefinementInput<'_, MS>,
    alignment: &AlignmentConfig,
    result: &mut AlignmentResult,
    progress: &dyn ProgressReporter,
    recommended_offset_secs: f64,
    segment_length: Duration,
    dur_a: f64,
    dur_b: f64,
) {
    let segment_length_secs = segment_length.as_secs_f64();
    let max_adjustment = alignment.high_rate_refine_max_adjustment_secs;

    let start_window = discovery_window(input.discovery_windows, ClipLabel::Start);
    let end_window = discovery_window(input.discovery_windows, ClipLabel::End);

    let start_prior = clip_with_label(&result.clips, ClipLabel::Start)
        .and_then(|clip| clip.offset_secs)
        .unwrap_or(recommended_offset_secs);
    let end_prior = clip_with_label(&result.clips, ClipLabel::End)
        .and_then(|clip| clip.offset_secs)
        .expect("dual anchor requires end offset");

    let start_anchor = refine_at_discovery_anchor(
        input,
        progress,
        start_window,
        start_prior,
        segment_length,
        segment_length_secs,
        max_adjustment,
        dur_a,
        dur_b,
        "Extracting high-rate hold-out (video A, start anchor)",
        "Extracting high-rate hold-out (video B, start anchor)",
    );

    if start_anchor.applied {
        apply_anchor_adjustment(result, ClipLabel::Start, start_anchor.adjustment_secs);
        result.recommended_offset_secs =
            Some(recommended_offset_secs + start_anchor.adjustment_secs);
        refresh_start_overlap(
            result,
            input.extent_a.effective(),
            input.extent_b.effective(),
        );
    }

    let end_anchor = refine_at_discovery_anchor(
        input,
        progress,
        end_window,
        end_prior,
        segment_length,
        segment_length_secs,
        max_adjustment,
        dur_a,
        dur_b,
        "Extracting high-rate hold-out (video A, end anchor)",
        "Extracting high-rate hold-out (video B, end anchor)",
    );

    if end_anchor.applied {
        apply_anchor_adjustment(result, ClipLabel::End, end_anchor.adjustment_secs);
    }

    refresh_alignment_drift_summary(result);
    let refined_drift_secs = result.offset_drift_secs;

    let report = HighRateRefinement {
        segment_start_secs: start_anchor.segment_start_secs,
        segment_length_secs: start_anchor.segment_length_secs,
        adjustment_secs: start_anchor.adjustment_secs,
        correlation_peak: start_anchor.correlation_peak,
        applied: start_anchor.applied || end_anchor.applied,
        skipped: start_anchor.skipped && end_anchor.skipped,
        skip_reason: dual_skip_reason(&start_anchor, &end_anchor),
        end_anchor: Some(end_anchor),
        refined_drift_secs,
    };

    debug!(
        start_adjustment = report.adjustment_secs,
        end_adjustment = report.end_anchor.as_ref().map(|a| a.adjustment_secs),
        refined_drift_secs,
        "dual-anchor high-rate refinement complete"
    );

    result.high_rate_refinement = Some(report);
}

fn apply_single_holdout_high_rate_refinement<MS: MediaSession>(
    input: &mut HighRateRefinementInput<'_, MS>,
    alignment: &AlignmentConfig,
    result: &mut AlignmentResult,
    progress: &dyn ProgressReporter,
    offset_secs: f64,
    segment_length: Duration,
    segment_length_secs: f64,
    dur_a: f64,
    dur_b: f64,
) {
    let candidates = resolve_holdout_candidates(
        result,
        input.extent_a,
        input.extent_b,
        input.discovery_windows,
        segment_length,
        offset_secs,
    );
    if candidates.is_empty() {
        debug!("high-rate refine skipped: hold-out window unavailable");
        result.high_rate_refinement = Some(skipped_refinement(
            0.0,
            segment_length_secs,
            "hold-out window unavailable".into(),
        ));
        return;
    }

    let mut last_failure = String::from("hold-out extract failed for all candidate windows");
    let mut chosen_start_secs = 0.0;
    let mut clip_a = None;
    let mut clip_b = None;

    for holdout in &candidates {
        let window_start_secs = holdout.start.as_secs_f64();
        if !holdout_window_feasible(
            window_start_secs,
            segment_length_secs,
            offset_secs,
            dur_a,
            dur_b,
        ) {
            continue;
        }

        let window_b_start = Duration::from_secs_f64(window_start_secs + offset_secs);
        let window_b_end =
            Duration::from_secs_f64(window_start_secs + segment_length_secs + offset_secs);

        let ra = extract_native_holdout(
            input.session_a,
            input.track_a,
            holdout,
            progress,
            "Extracting high-rate hold-out (video A)",
        );
        match ra {
            Ok(clip) => {
                let rb = extract_native_holdout(
                    input.session_b,
                    input.track_b,
                    &ClipWindow::new(window_b_start, window_b_end, holdout.label),
                    progress,
                    "Extracting high-rate hold-out (video B)",
                );
                match rb {
                    Ok(other) => {
                        chosen_start_secs = window_start_secs;
                        clip_a = Some(clip);
                        clip_b = Some(other);
                        break;
                    }
                    Err(e) => {
                        debug_media_error(
                            &e,
                            "high-rate hold-out extract B failed, trying next candidate",
                        );
                        last_failure = format!("{e}");
                    }
                }
            }
            Err(e) => {
                debug_media_error(&e, "high-rate hold-out extract A failed, trying next candidate");
                last_failure = format!("{e}");
            }
        }
    }

    let (clip_a, clip_b) = match (clip_a, clip_b) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            result.high_rate_refinement = Some(skipped_refinement(
                chosen_start_secs,
                segment_length_secs,
                last_failure,
            ));
            return;
        }
    };

    let Some((adjustment_secs, correlation_peak)) = refine_holdout_segment_lag(
        &clip_a,
        &clip_b,
        alignment.high_rate_refine_max_adjustment_secs,
        input.resampler,
        input.correlator,
    ) else {
        debug!("high-rate refine skipped: correlation did not produce adjustment");
        result.high_rate_refinement = Some(HighRateRefinement {
            segment_start_secs: chosen_start_secs,
            segment_length_secs,
            adjustment_secs: 0.0,
            correlation_peak: 0.0,
            applied: false,
            skipped: false,
            skip_reason: Some("correlation produced no usable adjustment".into()),
            end_anchor: None,
            refined_drift_secs: None,
        });
        return;
    };

    debug!(
        adjustment_secs,
        correlation_peak,
        window_start_secs = chosen_start_secs,
        "high-rate offset refinement applied"
    );

    result.recommended_offset_secs = Some(offset_secs + adjustment_secs);
    refresh_start_overlap(
        result,
        input.extent_a.effective(),
        input.extent_b.effective(),
    );
    result.high_rate_refinement = Some(HighRateRefinement {
        segment_start_secs: chosen_start_secs,
        segment_length_secs,
        adjustment_secs,
        correlation_peak,
        applied: true,
        skipped: false,
        skip_reason: None,
        end_anchor: None,
        refined_drift_secs: None,
    });
}

fn refine_at_discovery_anchor<MS: MediaSession>(
    input: &mut HighRateRefinementInput<'_, MS>,
    progress: &dyn ProgressReporter,
    discovery_window: &ClipWindow,
    prior_offset_secs: f64,
    segment_length: Duration,
    segment_length_secs: f64,
    max_adjustment_secs: f64,
    dur_a: f64,
    dur_b: f64,
    label_a: &str,
    label_b: &str,
) -> HighRateAnchorRefinement {
    let Some(holdout) = holdout_window_centered_in(discovery_window, segment_length) else {
        return skipped_anchor(
            0.0,
            segment_length_secs,
            prior_offset_secs,
            "hold-out does not fit discovery window".into(),
        );
    };

    let window_start_secs = holdout.start.as_secs_f64();
    if !holdout_window_feasible(
        window_start_secs,
        segment_length_secs,
        prior_offset_secs,
        dur_a,
        dur_b,
    ) {
        return skipped_anchor(
            window_start_secs,
            segment_length_secs,
            prior_offset_secs,
            "hold-out window infeasible on A or B".into(),
        );
    }

    let window_b_start = Duration::from_secs_f64(window_start_secs + prior_offset_secs);
    let window_b_end =
        Duration::from_secs_f64(window_start_secs + segment_length_secs + prior_offset_secs);

    let clip_a = match extract_native_holdout(
        input.session_a,
        input.track_a,
        &holdout,
        progress,
        label_a,
    ) {
        Ok(clip) => clip,
        Err(e) => {
            debug_media_error(&e, "dual high-rate hold-out extract A failed");
            return skipped_anchor(
                window_start_secs,
                segment_length_secs,
                prior_offset_secs,
                format!("{e}"),
            );
        }
    };

    let clip_b = match extract_native_holdout(
        input.session_b,
        input.track_b,
        &ClipWindow::new(window_b_start, window_b_end, holdout.label),
        progress,
        label_b,
    ) {
        Ok(clip) => clip,
        Err(e) => {
            debug_media_error(&e, "dual high-rate hold-out extract B failed");
            return skipped_anchor(
                window_start_secs,
                segment_length_secs,
                prior_offset_secs,
                format!("{e}"),
            );
        }
    };

    let Some((adjustment_secs, correlation_peak)) = refine_holdout_segment_lag(
        &clip_a,
        &clip_b,
        max_adjustment_secs,
        input.resampler,
        input.correlator,
    ) else {
        return HighRateAnchorRefinement {
            segment_start_secs: window_start_secs,
            segment_length_secs,
            offset_before_secs: prior_offset_secs,
            adjustment_secs: 0.0,
            correlation_peak: 0.0,
            applied: false,
            skipped: false,
            skip_reason: Some("correlation produced no usable adjustment".into()),
        };
    };

    HighRateAnchorRefinement {
        segment_start_secs: window_start_secs,
        segment_length_secs,
        offset_before_secs: prior_offset_secs,
        adjustment_secs,
        correlation_peak,
        applied: true,
        skipped: false,
        skip_reason: None,
    }
}

fn discovery_window<'a>(windows: &'a [ClipWindow], label: ClipLabel) -> &'a ClipWindow {
    windows
        .iter()
        .find(|window| window.label == label)
        .expect("dual anchor requires discovery window")
}

fn apply_anchor_adjustment(result: &mut AlignmentResult, label: ClipLabel, adjustment_secs: f64) {
    if let Some(clip) = result
        .clips
        .iter_mut()
        .find(|clip| clip.label == label)
    {
        if let Some(offset) = clip.offset_secs.as_mut() {
            *offset += adjustment_secs;
        }
    }
}

fn dual_skip_reason(
    start: &HighRateAnchorRefinement,
    end: &HighRateAnchorRefinement,
) -> Option<String> {
    if start.skipped && end.skipped {
        let start_reason = start.skip_reason.as_deref().unwrap_or("start anchor skipped");
        let end_reason = end.skip_reason.as_deref().unwrap_or("end anchor skipped");
        Some(format!("start: {start_reason}; end: {end_reason}"))
    } else {
        None
    }
}

fn segment_length_secs(alignment: &AlignmentConfig) -> f64 {
    Duration::from_secs(u64::from(alignment.high_rate_refine_secs)).as_secs_f64()
}

fn extract_native_holdout<MS: MediaSession>(
    session: &mut MS,
    track: &AudioTrack,
    window: &ClipWindow,
    progress: &dyn ProgressReporter,
    label: &str,
) -> Result<MonoPcmClip, MediaError> {
    session.extract_mono(track, window, progress, label)
}

fn skipped_anchor(
    segment_start_secs: f64,
    segment_length_secs: f64,
    offset_before_secs: f64,
    reason: String,
) -> HighRateAnchorRefinement {
    HighRateAnchorRefinement {
        segment_start_secs,
        segment_length_secs,
        offset_before_secs,
        adjustment_secs: 0.0,
        correlation_peak: 0.0,
        applied: false,
        skipped: true,
        skip_reason: Some(reason),
    }
}

fn skipped_refinement(
    segment_start_secs: f64,
    segment_length_secs: f64,
    reason: String,
) -> HighRateRefinement {
    debug!(reason, "high-rate refine skipped");
    HighRateRefinement {
        segment_start_secs,
        segment_length_secs,
        adjustment_secs: 0.0,
        correlation_peak: 0.0,
        applied: false,
        skipped: true,
        skip_reason: Some(reason),
        end_anchor: None,
        refined_drift_secs: None,
    }
}
