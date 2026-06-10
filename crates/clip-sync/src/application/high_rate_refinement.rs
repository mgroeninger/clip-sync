use std::time::Duration;

use tracing::debug;

use crate::application::config::AlignmentConfig;
use crate::application::offset_refinement::refine_holdout_segment_lag;
use crate::application::ports::{MediaSession, ProgressReporter};
use crate::domain::{
    holdout_window_candidates, holdout_window_feasible, refresh_start_overlap, AlignmentResult,
    AudioTrack, ClipWindow, HighRateRefinement, MonoPcmClip,
};

pub struct HighRateRefinementInput<'a, MS: MediaSession> {
    pub session_a: &'a MS,
    pub session_b: &'a MS,
    pub track_a: &'a AudioTrack,
    pub track_b: &'a AudioTrack,
    pub discovery_windows: &'a [ClipWindow],
    pub duration_a: Duration,
    pub duration_b: Duration,
    pub decoded_extent_a: Duration,
    pub decoded_extent_b: Duration,
}

pub fn apply_high_rate_refinement<MS: MediaSession>(
    input: &HighRateRefinementInput<'_, MS>,
    alignment: &AlignmentConfig,
    result: &mut AlignmentResult,
    progress: &dyn ProgressReporter,
) {
    let &HighRateRefinementInput {
        session_a,
        session_b,
        track_a,
        track_b,
        discovery_windows,
        duration_a,
        duration_b,
        decoded_extent_a,
        decoded_extent_b,
    } = input;
    if !alignment.refine_offset_high_rate {
        return;
    }

    let Some(offset_secs) = result.recommended_offset_secs else {
        result.high_rate_refinement = Some(HighRateRefinement {
            segment_start_secs: 0.0,
            segment_length_secs: 0.0,
            adjustment_secs: 0.0,
            correlation_peak: 0.0,
            applied: false,
            skipped: true,
            skip_reason: Some("no recommended offset".into()),
        });
        return;
    };

    progress.phase_verbose("High-rate offset refinement...");
    let segment_length = Duration::from_secs(u64::from(alignment.high_rate_refine_secs));
    let segment_length_secs = segment_length.as_secs_f64();

    let _ = session_a.reset_io();
    let _ = session_b.reset_io();

    let pick_duration = duration_a
        .min(duration_b)
        .min(decoded_extent_a)
        .min(decoded_extent_b);
    let dur_a = duration_a.min(decoded_extent_a).as_secs_f64();
    let dur_b = duration_b.min(decoded_extent_b).as_secs_f64();

    let candidates =
        holdout_window_candidates(pick_duration, discovery_windows, segment_length, offset_secs);
    if candidates.is_empty() {
        debug!("high-rate refine skipped: hold-out window unavailable");
        result.high_rate_refinement = Some(HighRateRefinement {
            segment_start_secs: 0.0,
            segment_length_secs,
            adjustment_secs: 0.0,
            correlation_peak: 0.0,
            applied: false,
            skipped: true,
            skip_reason: Some("hold-out window unavailable".into()),
        });
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

        match extract_native_holdout(
            session_a,
            track_a,
            holdout,
            progress,
            "Extracting high-rate hold-out (video A)",
        ) {
            Ok(clip) => match extract_native_holdout(
                session_b,
                track_b,
                &ClipWindow::new(window_b_start, window_b_end, holdout.label),
                progress,
                "Extracting high-rate hold-out (video B)",
            ) {
                Ok(other) => {
                    chosen_start_secs = window_start_secs;
                    clip_a = Some(clip);
                    clip_b = Some(other);
                    break;
                }
                Err(reason) => last_failure = reason,
            },
            Err(reason) => last_failure = reason,
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
    refresh_start_overlap(result, duration_a, duration_b);
    result.high_rate_refinement = Some(HighRateRefinement {
        segment_start_secs: chosen_start_secs,
        segment_length_secs,
        adjustment_secs,
        correlation_peak,
        applied: true,
        skipped: false,
        skip_reason: None,
    });
}

fn extract_native_holdout<MS: MediaSession>(
    session: &MS,
    track: &AudioTrack,
    window: &ClipWindow,
    progress: &dyn ProgressReporter,
    label: &str,
) -> Result<MonoPcmClip, String> {
    session
        .extract_mono(track, window, progress, label)
        .map_err(|error| error.to_string())
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
    }
}
