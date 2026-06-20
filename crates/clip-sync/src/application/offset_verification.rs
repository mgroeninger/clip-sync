use std::time::Duration;

use tracing::debug;

use crate::application::config::{ClipConfig, ValidationConfig};
use crate::application::error::debug_media_error;
use crate::application::offset_refinement::pcm_cross_correlate_lag;
use crate::application::ports::{
    Aligner, Fingerprinter, MediaSession, PcmCorrelator, ProgressReporter, Resampler,
};
use crate::domain::{
    holdout_b_window_for_offset, holdout_extract_sufficient, holdout_pick_duration,
    holdout_window_feasible,
    parallel_holdout_window_candidates, periodic_ambiguity_period, periodic_recheck_period_multiple,
    display_repeat_period, prepare_clip_for_fingerprint, resolve_holdout_candidates,
    should_downgrade_repetition_confidence, AlignmentResult, AudioTrack, ClipWindow, MediaExtent,
    OffsetVerification, PcmPreparationOptions, OFFSET_AGREEMENT_TOLERANCE_SECS,
};
use crate::infrastructure::chromaprint::repetition::detect_clip_repetition;

pub struct OffsetVerificationInput<'a, MS: MediaSession> {
    pub session_a: &'a mut MS,
    pub session_b: &'a mut MS,
    pub track_a: &'a AudioTrack,
    pub track_b: &'a AudioTrack,
    pub discovery_windows: &'a [ClipWindow],
    pub extent_a: MediaExtent,
    pub extent_b: MediaExtent,
    pub min_holdout_decode_fraction: f64,
    pub max_holdout_decode_skips: u32,
    pub resampler: &'a dyn Resampler,
    pub correlator: &'a dyn PcmCorrelator,
}

/// Extract a hold-out window and score lag-0 similarity to independently verify the recommended
/// offset. Writes `result.offset_verification` when `validation.verify_offset` is on (including
/// skip cases); leaves the field `None` when the flag is off.
pub fn apply_offset_verification<MS, FP, AL>(
    input: &mut OffsetVerificationInput<'_, MS>,
    clip_config: &ClipConfig,
    validation: &ValidationConfig,
    result: &mut AlignmentResult,
    fingerprinter: &FP,
    aligner: &AL,
    progress: &dyn ProgressReporter,
) where
    MS: MediaSession,
    FP: Fingerprinter,
    AL: Aligner,
{
    if !validation.verify_offset {
        return;
    }

    let Some(offset_secs) = result.recommended_offset_secs else {
        debug!("offset verify skipped: no recommended offset");
        result.offset_verification = Some(skipped("no recommended offset"));
        return;
    };

    progress.phase_verbose("Verifying offset at hold-out window...");

    let pick_duration = holdout_pick_duration(result, input.extent_a, input.extent_b);
    let clip_length = clip_config.clip_length.min(pick_duration);
    let clip_length_secs = clip_length.as_secs_f64();

    if clip_length.is_zero() || pick_duration < clip_config.clip_length {
        debug!(
            pick_duration_secs = pick_duration.as_secs_f64(),
            clip_length_secs = clip_config.clip_length.as_secs_f64(),
            "offset verify skipped: media shorter than clip_length"
        );
        result.offset_verification = Some(skipped("hold-out window unavailable"));
        return;
    }
    let dur_a = input.extent_a.effective().as_secs_f64();
    let dur_b = input.extent_b.effective().as_secs_f64();

    let (_period_secs, parallel_independent) = resolve_parallel_periodic_recheck(
        input,
        clip_config,
        validation,
        result,
        clip_length,
        fingerprinter,
        progress,
    );

    let candidates = resolve_holdout_candidates(
        result,
        input.extent_a,
        input.extent_b,
        input.discovery_windows,
        clip_length,
        offset_secs,
    );
    if candidates.is_empty() {
        debug!("offset verify skipped: no hold-out window candidates");
        result.offset_verification = Some(skipped_with_periodic_context(
            "hold-out window unavailable",
            offset_secs,
            parallel_independent,
            result,
        ));
        return;
    }

    let prep_options = PcmPreparationOptions {
        normalize_loudness: clip_config.normalize_loudness,
        trim_silence: clip_config.trim_silence,
        window_slide_secs: 0,
    };

    let mut last_failure = String::from("hold-out extract failed for all candidate windows");
    let mut saw_feasible = false;
    let mut scored_attempts: Vec<OffsetVerification> = Vec::new();
    const MAX_SCORED_ATTEMPTS: usize = 3;

    for holdout in &candidates {
        if scored_attempts.len() >= MAX_SCORED_ATTEMPTS {
            break;
        }
        let window_start_secs = holdout.start.as_secs_f64();
        if !holdout_window_feasible(
            window_start_secs,
            clip_length_secs,
            offset_secs,
            dur_a,
            dur_b,
        ) {
            continue;
        }
        saw_feasible = true;

        let Some(window_b) = holdout_b_window_for_offset(holdout, clip_length, offset_secs) else {
            continue;
        };

        let raw_a = match input.session_a.extract_mono(
            input.track_a,
            holdout,
            progress,
            "Verifying hold-out (video A)",
        ) {
            Ok(clip) => clip,
            Err(e) => {
                last_failure = format!("extract A failed: {e}");
                debug_media_error(&e, "offset verify: extract A failed, trying next candidate");
                continue;
            }
        };
        let raw_b = match input.session_b.extract_mono(
            input.track_b,
            &window_b,
            progress,
            "Verifying hold-out (video B)",
        ) {
            Ok(clip) => clip,
            Err(e) => {
                last_failure = format!("extract B failed: {e}");
                debug_media_error(&e, "offset verify: extract B failed, trying next candidate");
                continue;
            }
        };

        if !holdout_extract_sufficient(
            &raw_a,
            clip_length,
            input.min_holdout_decode_fraction,
            input.max_holdout_decode_skips,
        ) {
            last_failure = "hold-out extract A shorter than clip_length".into();
            debug!("offset verify: extract A truncated, trying next candidate");
            continue;
        }
        if !holdout_extract_sufficient(
            &raw_b,
            clip_length,
            input.min_holdout_decode_fraction,
            input.max_holdout_decode_skips,
        ) {
            last_failure = "hold-out extract B shorter than clip_length".into();
            debug!("offset verify: extract B truncated, trying next candidate");
            continue;
        }

        let source_duration_a = raw_a.duration_secs();
        let source_duration_b = raw_b.duration_secs();

        let raw_a = match clip_config.target_sample_rate {
            Some(rate) => input.resampler.resample_mono(&raw_a, rate),
            None => raw_a,
        };
        let raw_b = match clip_config.target_sample_rate {
            Some(rate) => input.resampler.resample_mono(&raw_b, rate),
            None => raw_b,
        };

        let prepared_a = match prepare_clip_for_fingerprint(&raw_a, prep_options) {
            Ok(clip) => clip,
            Err(e) => {
                last_failure = format!("prepare A failed: {e:?}");
                debug!(error = ?e, "offset verify: prepare A failed, trying next candidate");
                continue;
            }
        };
        let prepared_b = match prepare_clip_for_fingerprint(&raw_b, prep_options) {
            Ok(clip) => clip,
            Err(e) => {
                last_failure = format!("prepare B failed: {e:?}");
                debug!(error = ?e, "offset verify: prepare B failed, trying next candidate");
                continue;
            }
        };

        let fp_a = match fingerprinter.fingerprint(&prepared_a) {
            Ok(fp) => fp,
            Err(e) => {
                last_failure = format!("fingerprint A failed: {e}");
                debug!(error = %e, "offset verify: fingerprint A failed, trying next candidate");
                continue;
            }
        };
        let fp_b = match fingerprinter.fingerprint(&prepared_b) {
            Ok(fp) => fp,
            Err(e) => {
                last_failure = format!("fingerprint B failed: {e}");
                debug!(error = %e, "offset verify: fingerprint B failed, trying next candidate");
                continue;
            }
        };

        let estimate = match aligner.find_offset(&fp_a, &fp_b) {
            Ok(e) => e,
            Err(e) => {
                last_failure = format!("aligner failed: {e}");
                debug!(error = %e, "offset verify: aligner failed, trying next candidate");
                continue;
            }
        };

        let mut confidence = estimate.confidence;
        if validation.check_clip_repetition {
            let preset = clip_config.chromaprint_preset;
            let min_conf = validation.min_repetition_confidence;
            let rep_a = detect_clip_repetition(
                &fp_a,
                prepared_a.duration_secs(),
                preset,
                min_conf,
                source_duration_a,
            );
            let rep_b = detect_clip_repetition(
                &fp_b,
                prepared_b.duration_secs(),
                preset,
                min_conf,
                source_duration_b,
            );
            if should_downgrade_repetition_confidence(&rep_a, &rep_b, offset_secs) {
                confidence *= 0.5;
            }
        }

        let verified = confidence >= validation.min_verification_confidence
            && estimate.offset_secs.abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS;

        let window_b_start_secs = window_start_secs + offset_secs;
        let window_b_end_secs = window_start_secs + clip_length_secs + offset_secs;

        debug!(
            window_a_start_secs = window_start_secs,
            window_b_start_secs,
            confidence,
            lag_secs = estimate.offset_secs,
            verified,
            "offset verification result"
        );

        progress.phase_verbose(&format!(
            "Hold-out verify A [{:.1}–{:.1}]: confidence {:.2}, lag {:+.3}s, {}",
            window_start_secs,
            window_start_secs + clip_length_secs,
            confidence,
            estimate.offset_secs,
            if verified {
                "verified"
            } else {
                "not verified"
            }
        ));

        scored_attempts.push(OffsetVerification {
            window_a_start_secs: window_start_secs,
            window_a_end_secs: window_start_secs + clip_length_secs,
            window_b_start_secs,
            window_b_end_secs,
            confidence,
            verified,
            skipped: false,
            skip_reason: None,
            candidates_tried: 0,
            independent_offset_secs: None,
            parallel_recheck_delta_secs: None,
            verify_inconclusive: false,
        });

        if verified {
            break;
        }
    }

    if let Some(best) = pick_best_scored_attempt(&scored_attempts) {
        let mut best = best.clone();
        best.candidates_tried = scored_attempts.len() as u32;
        apply_periodic_verify_gating(
            &mut best,
            offset_secs,
            parallel_independent,
            result,
            progress,
        );
        result.offset_verification = Some(best);
        return;
    }

    let reason = if saw_feasible {
        last_failure
    } else {
        "hold-out window unavailable".into()
    };
    debug!(reason, "offset verify skipped");
    result.offset_verification = Some(skipped(&reason));
}

fn skipped_with_periodic_context(
    reason: &str,
    recommended_offset_secs: f64,
    parallel_independent: Option<f64>,
    result: &AlignmentResult,
) -> OffsetVerification {
    let mut verify = skipped(reason);
    if result.offset_ambiguous_mod_secs.is_some() || parallel_independent.is_some() {
        verify.verify_inconclusive = true;
        if let Some(independent) = parallel_independent {
            verify.independent_offset_secs = Some(independent);
            verify.parallel_recheck_delta_secs = Some(recommended_offset_secs - independent);
        }
    }
    verify
}

fn skipped(reason: &str) -> OffsetVerification {
    OffsetVerification {
        window_a_start_secs: 0.0,
        window_a_end_secs: 0.0,
        window_b_start_secs: 0.0,
        window_b_end_secs: 0.0,
        confidence: 0.0,
        verified: false,
        skipped: true,
        skip_reason: Some(reason.into()),
        candidates_tried: 0,
        independent_offset_secs: None,
        parallel_recheck_delta_secs: None,
        verify_inconclusive: false,
    }
}

fn known_periodic_period_secs(
    result: &AlignmentResult,
    validation: &ValidationConfig,
) -> Option<f64> {
    if let Some(period) = result.offset_ambiguous_mod_secs {
        return Some(period);
    }
    if !validation.check_clip_repetition {
        return None;
    }
    let clip = result.start_clip()?;
    let clip_duration_secs = clip.window_end_secs - clip.window_start_secs;
    clip.repetition.as_ref().and_then(|report| {
        periodic_ambiguity_period(
            report,
            validation.min_repetition_confidence,
            Some(clip_duration_secs),
        )
    })
}

fn should_run_parallel_periodic_recheck(period_secs: Option<f64>) -> bool {
    period_secs.is_some()
}

fn resolve_parallel_periodic_recheck<MS, FP>(
    input: &mut OffsetVerificationInput<'_, MS>,
    clip_config: &ClipConfig,
    validation: &ValidationConfig,
    result: &mut AlignmentResult,
    clip_length: Duration,
    fingerprinter: &FP,
    progress: &dyn ProgressReporter,
) -> (Option<f64>, Option<f64>)
where
    MS: MediaSession,
    FP: Fingerprinter,
{
    let clip_length_secs = clip_length.as_secs_f64();
    let mut period_secs = known_periodic_period_secs(result, validation);
    let parallel = if should_run_parallel_periodic_recheck(period_secs) {
        run_parallel_offset_recheck(
            input,
            clip_config,
            validation,
            clip_length,
            fingerprinter,
            progress,
            &mut period_secs,
        )
    } else {
        None
    };
    if let Some(period) = period_secs {
        let normalized = display_repeat_period(period, clip_length_secs);
        period_secs = Some(normalized);
        if result.offset_ambiguous_mod_secs.is_none() {
            result.offset_ambiguous_mod_secs = Some(normalized);
        } else if let Some(existing) = result.offset_ambiguous_mod_secs {
            result.offset_ambiguous_mod_secs = Some(display_repeat_period(existing, clip_length_secs));
        }
    }
    (period_secs, parallel)
}

fn parallel_recheck_disagrees(
    recommended_offset_secs: f64,
    independent_offset_secs: f64,
    period_secs: Option<f64>,
) -> bool {
    if let Some(period) = period_secs {
        match periodic_recheck_period_multiple(
            recommended_offset_secs,
            independent_offset_secs,
            period,
        ) {
            Some(0) => return false,
            Some(_) => return true,
            None => {}
        }
    }
    (recommended_offset_secs - independent_offset_secs).abs() > OFFSET_AGREEMENT_TOLERANCE_SECS
}

fn apply_periodic_verify_gating(
    verify: &mut OffsetVerification,
    recommended_offset_secs: f64,
    parallel_independent: Option<f64>,
    result: &mut AlignmentResult,
    progress: &dyn ProgressReporter,
) {
    if let Some(independent) = parallel_independent {
        verify.independent_offset_secs = Some(independent);
        verify.parallel_recheck_delta_secs = Some(recommended_offset_secs - independent);

        if verify.verified
            && parallel_recheck_disagrees(
                recommended_offset_secs,
                independent,
                result.offset_ambiguous_mod_secs,
            )
        {
            verify.verified = false;
            verify.verify_inconclusive = true;
            let period_note = result
                .offset_ambiguous_mod_secs
                .map(|p| format!("~{p:.0}s repeat"))
                .unwrap_or_else(|| "parallel recheck".into());
            progress.phase_verbose(&format!(
                "Hold-out verify: periodic ambiguity ({period_note}); offset-shifted pass rejected \
                 (parallel {independent:.3}s vs recommended {recommended_offset_secs:.3}s)"
            ));
            return;
        }

        if verify.verified
            && result.offset_ambiguous_mod_secs.is_some_and(|period| {
                periodic_recheck_period_multiple(recommended_offset_secs, independent, period)
                    == Some(0)
            })
        {
            result.offset_ambiguous_mod_secs = None;
            progress.phase_verbose(
                "Hold-out verify: parallel recheck agrees with recommended offset; periodic ambiguity cleared",
            );
        }
        return;
    }

    if result.offset_ambiguous_mod_secs.is_some() && verify.verified {
        verify.verified = false;
        verify.verify_inconclusive = true;
        progress.phase_verbose(
            "Hold-out verify: periodic content; offset-shifted pass rejected (no parallel recheck)",
        );
    }
}

fn run_parallel_offset_recheck<MS, FP>(
    input: &mut OffsetVerificationInput<'_, MS>,
    clip_config: &ClipConfig,
    validation: &ValidationConfig,
    clip_length: Duration,
    fingerprinter: &FP,
    progress: &dyn ProgressReporter,
    period_secs: &mut Option<f64>,
) -> Option<f64>
where
    MS: MediaSession,
    FP: Fingerprinter,
{
    let clip_length_secs = clip_length.as_secs_f64();
    const PARALLEL_PCM_WINDOW_SECS: u32 = 20;
    let prep_options = PcmPreparationOptions {
        normalize_loudness: clip_config.normalize_loudness,
        trim_silence: clip_config.trim_silence,
        window_slide_secs: 0,
    };

    let duration_a = input.extent_a.effective();
    let duration_b = input.extent_b.effective();
    let windows = parallel_holdout_window_candidates(duration_a, duration_b, clip_length);
    let pcm_window_secs = PARALLEL_PCM_WINDOW_SECS
        .min(clip_length_secs.floor() as u32)
        .max(1);
    let sniff_period = period_secs.is_none();

    struct ParallelAttempt {
        offset_secs: f64,
        peak: f64,
    }

    let mut attempts: Vec<ParallelAttempt> = Vec::new();

    for window in &windows {
        let window_start_secs = window.start.as_secs_f64();
        progress.phase_verbose(&format!(
            "Parallel hold-out recheck A/B [{:.1}–{:.1}] (calendar-aligned, PCM {:.0}s)",
            window_start_secs,
            window_start_secs + f64::from(pcm_window_secs),
            pcm_window_secs,
        ));

        let raw_a = match input.session_a.extract_mono(
            input.track_a,
            window,
            progress,
            "Parallel recheck (video A)",
        ) {
            Ok(clip) => clip,
            Err(e) => {
                debug_media_error(&e, "parallel recheck: extract A failed, trying next window");
                continue;
            }
        };
        let raw_b = match input.session_b.extract_mono(
            input.track_b,
            window,
            progress,
            "Parallel recheck (video B)",
        ) {
            Ok(clip) => clip,
            Err(e) => {
                debug_media_error(&e, "parallel recheck: extract B failed, trying next window");
                continue;
            }
        };

        if !holdout_extract_sufficient(
            &raw_a,
            clip_length,
            input.min_holdout_decode_fraction,
            input.max_holdout_decode_skips,
        ) || !holdout_extract_sufficient(
            &raw_b,
            clip_length,
            input.min_holdout_decode_fraction,
            input.max_holdout_decode_skips,
        ) {
            continue;
        }

        let raw_a = match clip_config.target_sample_rate {
            Some(rate) => input.resampler.resample_mono(&raw_a, rate),
            None => raw_a,
        };
        let raw_b = match clip_config.target_sample_rate {
            Some(rate) => input.resampler.resample_mono(&raw_b, rate),
            None => raw_b,
        };

        let prepared_a = match prepare_clip_for_fingerprint(&raw_a, prep_options) {
            Ok(clip) => clip,
            Err(e) => {
                debug!(error = ?e, "parallel recheck: prepare A failed, trying next window");
                continue;
            }
        };
        let prepared_b = match prepare_clip_for_fingerprint(&raw_b, prep_options) {
            Ok(clip) => clip,
            Err(e) => {
                debug!(error = ?e, "parallel recheck: prepare B failed, trying next window");
                continue;
            }
        };

        if sniff_period {
            let fp_a = match fingerprinter.fingerprint(&prepared_a) {
                Ok(fp) => fp,
                Err(e) => {
                    debug!(error = %e, "parallel recheck: fingerprint A failed, trying next window");
                    continue;
                }
            };
            let fp_b = match fingerprinter.fingerprint(&prepared_b) {
                Ok(fp) => fp,
                Err(e) => {
                    debug!(error = %e, "parallel recheck: fingerprint B failed, trying next window");
                    continue;
                }
            };

            let preset = clip_config.chromaprint_preset;
            let min_conf = validation.min_repetition_confidence;
            let repetition_report = crate::domain::ClipRepetitionReport {
                a: detect_clip_repetition(
                    &fp_a,
                    prepared_a.duration_secs(),
                    preset,
                    min_conf,
                    raw_a.duration_secs(),
                ),
                b: detect_clip_repetition(
                    &fp_b,
                    prepared_b.duration_secs(),
                    preset,
                    min_conf,
                    raw_b.duration_secs(),
                ),
            };
            if period_secs.is_none() {
                *period_secs = periodic_ambiguity_period(
                    &repetition_report,
                    min_conf,
                    Some(clip_length_secs),
                );
            }
        }

        let (adjustment, peak) = match pcm_cross_correlate_lag(
            &raw_a,
            &raw_b,
            0.0,
            pcm_window_secs,
            input.resampler,
            input.correlator,
        ) {
            Some(result) => result,
            None => {
                debug!("parallel recheck: PCM correlate failed, trying next window");
                continue;
            }
        };
        debug!(
            window_start_secs,
            independent_offset_secs = adjustment,
            correlation_peak = peak,
            "parallel hold-out PCM recheck"
        );
        attempts.push(ParallelAttempt {
            offset_secs: adjustment,
            peak,
        });
    }

    if attempts.is_empty() {
        return None;
    }

    let best = attempts
        .iter()
        .max_by(|left, right| {
            left.peak
                .partial_cmp(&right.peak)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("attempts non-empty");

    if attempts.len() > 1 {
        let agreeing = attempts.iter().filter(|attempt| {
            (attempt.offset_secs - best.offset_secs).abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS
        });
        if agreeing.count() < attempts.len() {
            debug!(
                best_offset_secs = best.offset_secs,
                best_peak = best.peak,
                windows_tried = attempts.len(),
                "parallel recheck: PCM windows disagree; using highest-correlation peak"
            );
        }
    }

    Some(best.offset_secs)
}

fn pick_best_scored_attempt(attempts: &[OffsetVerification]) -> Option<&OffsetVerification> {
    attempts.iter().enumerate().max_by(|(left_idx, left), (right_idx, right)| {
        left.confidence
            .partial_cmp(&right.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right_idx.cmp(left_idx))
    })
    .map(|(_, attempt)| attempt)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::application::config::{ClipConfig, ValidationConfig};
    use crate::application::testing::fakes::{
        FakeAligner, FakeFingerprinter, FakeMediaSession, FakePcmCorrelator, FakeProgressReporter,
    };
    use crate::domain::{AudioTrack, ClipLabel, ClipMatchEstimate, ClipWindow, MonoPcmClip};

    const SAMPLE_RATE: u32 = 11_025;
    const TOTAL_SECS: u32 = 120;
    const OFFSET_SECS: u32 = 3;
    const HOLDOUT_CLIP_SECS: u64 = 60;

    fn verification_validation() -> ValidationConfig {
        ValidationConfig {
            verify_offset: true,
            min_verification_confidence: 0.5,
            ..Default::default()
        }
    }

    fn holdout_clip_config() -> ClipConfig {
        ClipConfig {
            clip_length: Duration::from_secs(HOLDOUT_CLIP_SECS),
            num_clips: 1,
            target_sample_rate: Some(SAMPLE_RATE),
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
            ..ClipConfig::default()
        }
    }

    fn result_with_offset(offset_secs: f64) -> AlignmentResult {
        use crate::application::testing::alignment_fixtures::minimal_alignment_result;

        minimal_alignment_result(Some(offset_secs))
            .with_clips(vec![])
            .build()
    }

    fn discovery_windows() -> Vec<ClipWindow> {
        vec![ClipWindow::new(
            Duration::ZERO,
            Duration::from_secs(30),
            ClipLabel::Start,
        )]
    }

    fn default_decode_policy() -> (f64, u32) {
        use crate::application::config::AlignmentConfig;
        let alignment = AlignmentConfig::default();
        (
            alignment.min_end_clip_decode_fraction,
            alignment.max_end_clip_decode_skips,
        )
    }

    fn verification_input<'a>(
        session_a: &'a mut FakeMediaSession,
        session_b: &'a mut FakeMediaSession,
        track_a: &'a AudioTrack,
        track_b: &'a AudioTrack,
        windows: &'a [ClipWindow],
        duration: Duration,
    ) -> OffsetVerificationInput<'a, FakeMediaSession> {
        let (min_holdout_decode_fraction, max_holdout_decode_skips) = default_decode_policy();
        let extent = MediaExtent::from_declared(duration);
        OffsetVerificationInput {
            session_a,
            session_b,
            track_a,
            track_b,
            discovery_windows: windows,
            extent_a: extent,
            extent_b: extent,
            min_holdout_decode_fraction,
            max_holdout_decode_skips,
            resampler: &crate::infrastructure::resample::RubatoResampler,
            correlator: &crate::infrastructure::correlation::FftCorrelator,
        }
    }

    fn run_real_pipeline_verification(
        path_a: &std::path::Path,
        path_b: &std::path::Path,
        offset_secs: f64,
        validation: ValidationConfig,
        offset_ambiguous_mod_secs: Option<f64>,
    ) -> AlignmentResult {
        use crate::application::config::ChromaprintPreset;
        use crate::application::ports::MediaReader;
        use crate::domain::MediaSource;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;

        let mut session_a = media_reader
            .open(&MediaSource::new(path_a))
            .expect("open a");
        let mut session_b = media_reader
            .open(&MediaSource::new(path_b))
            .expect("open b");
        let tracks_a = session_a.list_tracks().expect("tracks a");
        let tracks_b = session_b.list_tracks().expect("tracks b");
        let track_a = &tracks_a[0];
        let track_b = &tracks_b[0];
        let duration = track_a.duration.expect("duration");

        let mut result = result_with_offset(offset_secs);
        result.offset_ambiguous_mod_secs = offset_ambiguous_mod_secs;
        let clip_config = holdout_clip_config();
        let windows = discovery_windows();
        let (min_holdout_decode_fraction, max_holdout_decode_skips) = default_decode_policy();

        let extent = MediaExtent::from_declared(duration);

        apply_offset_verification(
            &mut OffsetVerificationInput {
                session_a: &mut session_a,
                session_b: &mut session_b,
                track_a,
                track_b,
                discovery_windows: &windows,
                extent_a: extent,
                extent_b: extent,
                min_holdout_decode_fraction,
                max_holdout_decode_skips,
                resampler: &crate::infrastructure::resample::RubatoResampler,
                correlator: &crate::infrastructure::correlation::FftCorrelator,
            },
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );
        result
    }

    #[test]
    fn verify_offset_passes_known_leader() {
        use crate::application::testing::audio_fixtures::write_offset_chirp_wav_pair;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) =
            write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);

        let result = run_real_pipeline_verification(
            &path_a,
            &path_b,
            f64::from(OFFSET_SECS),
            verification_validation(),
            None,
        );
        let v = result
            .offset_verification
            .expect("offset_verification must be set when flag on");

        assert!(
            !v.skipped,
            "should not skip with feasible window: skip_reason={:?}",
            v.skip_reason
        );
        assert!(
            v.confidence >= 0.5,
            "confidence={} expected >= 0.5",
            v.confidence
        );
        assert!(
            v.verified,
            "verified=false; confidence={}, lag would exceed tolerance",
            v.confidence
        );
    }

    #[test]
    fn verify_offset_passes_negative_delta() {
        use crate::application::testing::audio_fixtures::{
            write_offset_chirp_wav_pair_with_delay, ChirpDelayOn,
        };

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = write_offset_chirp_wav_pair_with_delay(
            temp.path(),
            SAMPLE_RATE,
            TOTAL_SECS,
            OFFSET_SECS,
            ChirpDelayOn::A,
        );

        let result = run_real_pipeline_verification(
            &path_a,
            &path_b,
            -f64::from(OFFSET_SECS),
            verification_validation(),
            None,
        );
        let v = result
            .offset_verification
            .expect("offset_verification must be set when flag on");

        assert!(
            !v.skipped,
            "should not skip with feasible negative-delta window: skip_reason={:?}",
            v.skip_reason
        );
        assert!(
            v.confidence >= 0.5,
            "confidence={} expected >= 0.5",
            v.confidence
        );
        assert!(
            v.verified,
            "verified=false for negative delta; confidence={}",
            v.confidence
        );
    }

    #[test]
    fn parallel_recheck_looped_chirp_disagrees_with_period_alias() {
        use crate::application::testing::audio_fixtures::write_looped_chirp_wav_pair;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) =
            write_looped_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);

        let mut validation = verification_validation();
        validation.check_clip_repetition = true;

        let result = run_real_pipeline_verification(&path_a, &path_b, 13.0, validation, Some(10.0));
        let v = result
            .offset_verification
            .expect("offset_verification must be set");

        assert!(
            v.independent_offset_secs.is_some(),
            "parallel recheck should run on looped pair"
        );
        let independent = v.independent_offset_secs.unwrap();
        assert!(
            (independent - 3.0).abs() < 2.0,
            "parallel recheck expected ~+3s true offset, got {independent}"
        );
        assert!(!v.verified, "period alias +13s must not verify");
        assert!(v.verify_inconclusive);
    }

    #[test]
    fn verify_inconclusive_when_ambiguous_without_parallel_pcm() {
        let duration = Duration::from_secs(120);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            duration: Some(duration),
            decodable: true,
        };
        let mut session_a = FakeMediaSession::with_duration(duration);
        let mut session_b = session_a.clone();
        let mut result = result_with_offset(3.0);
        result.offset_ambiguous_mod_secs = Some(10.0);

        let clip_config = holdout_clip_config();
        let validation = verification_validation();
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;
        let correlator = FakePcmCorrelator::new();
        let (min_holdout_decode_fraction, max_holdout_decode_skips) = default_decode_policy();
        let extent = MediaExtent::from_declared(duration);

        apply_offset_verification(
            &mut OffsetVerificationInput {
                session_a: &mut session_a,
                session_b: &mut session_b,
                track_a: &track,
                track_b: &track,
                discovery_windows: &windows,
                extent_a: extent,
                extent_b: extent,
                min_holdout_decode_fraction,
                max_holdout_decode_skips,
                resampler: &crate::infrastructure::resample::RubatoResampler,
                correlator: &correlator,
            },
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        let v = result
            .offset_verification
            .expect("offset_verification must be set");
        assert!(
            v.verify_inconclusive,
            "ambiguous flag without parallel PCM should force inconclusive"
        );
        assert!(!v.verified);
    }

    #[test]
    fn skipped_with_periodic_context_sets_inconclusive() {
        let mut result = result_with_offset(13.0);
        result.offset_ambiguous_mod_secs = Some(10.0);
        let verify = skipped_with_periodic_context(
            "hold-out window unavailable",
            13.0,
            Some(3.0),
            &result,
        );
        assert!(verify.skipped);
        assert!(verify.verify_inconclusive);
        assert_eq!(verify.independent_offset_secs, Some(3.0));
        assert!((verify.parallel_recheck_delta_secs.unwrap() - 10.0).abs() < 0.001);
        assert_eq!(
            verify.skip_reason.as_deref(),
            Some("hold-out window unavailable")
        );
    }

    #[test]
    fn parallel_recheck_looped_chirp_negative_period_alias() {
        use crate::application::testing::audio_fixtures::{
            write_looped_chirp_wav_pair_with_delay, ChirpDelayOn,
        };

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = write_looped_chirp_wav_pair_with_delay(
            temp.path(),
            SAMPLE_RATE,
            TOTAL_SECS,
            OFFSET_SECS,
            ChirpDelayOn::A,
        );

        let mut validation = verification_validation();
        validation.check_clip_repetition = true;

        // True offset −3 s; period alias −13 s (−3 − 10 s loop).
        let result = run_real_pipeline_verification(
            &path_a,
            &path_b,
            -13.0,
            validation,
            Some(10.0),
        );
        let v = result
            .offset_verification
            .expect("offset_verification must be set");

        assert!(
            v.independent_offset_secs.is_some(),
            "parallel recheck should run on looped pair with negative alias"
        );
        let independent = v.independent_offset_secs.unwrap();
        assert!(
            (independent - (-3.0)).abs() < 2.0,
            "parallel recheck expected ~−3s true offset, got {independent}"
        );
        assert!(!v.verified, "period alias −13s must not verify");
        assert!(v.verify_inconclusive);
    }

    struct SequencePcmCorrelator {
        responses: std::sync::Mutex<Vec<(f64, f64)>>,
    }

    impl SequencePcmCorrelator {
        fn new(responses: Vec<(f64, f64)>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
            }
        }
    }

    impl crate::application::ports::PcmCorrelator for SequencePcmCorrelator {
        fn cross_correlate_lag(&self, _a: &[f64], _b: &[f64]) -> Option<(f64, f64)> {
            let mut guard = self.responses.lock().ok()?;
            if guard.is_empty() {
                return None;
            }
            Some(guard.remove(0))
        }

        fn segment_similarity(&self, _a: &[f64], _b: &[f64]) -> f64 {
            0.0
        }

        fn slide_template_scores(&self, _template: &[f64], _signal: &[f64]) -> Vec<f64> {
            Vec::new()
        }
    }

    #[test]
    fn parallel_recheck_picks_highest_peak_when_windows_disagree() {
        let duration = Duration::from_secs(120);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            duration: Some(duration),
            decodable: true,
        };
        let mut session_a = FakeMediaSession::with_duration(duration);
        let mut session_b = session_a.clone();
        let mut result = result_with_offset(13.0);
        result.offset_ambiguous_mod_secs = Some(10.0);

        let clip_config = holdout_clip_config();
        let validation = verification_validation();
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;
        // Window T=0: weak peak, ~0 s adjustment; window T=max: strong peak, ~+3 s.
        let correlator = SequencePcmCorrelator::new(vec![
            (0.0, 1.0),
            (-f64::from(SAMPLE_RATE) * f64::from(OFFSET_SECS), 50.0),
        ]);
        let (min_holdout_decode_fraction, max_holdout_decode_skips) = default_decode_policy();
        let extent = MediaExtent::from_declared(duration);

        apply_offset_verification(
            &mut OffsetVerificationInput {
                session_a: &mut session_a,
                session_b: &mut session_b,
                track_a: &track,
                track_b: &track,
                discovery_windows: &windows,
                extent_a: extent,
                extent_b: extent,
                min_holdout_decode_fraction,
                max_holdout_decode_skips,
                resampler: &crate::infrastructure::resample::RubatoResampler,
                correlator: &correlator,
            },
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        let v = result
            .offset_verification
            .expect("offset_verification must be set");
        let independent = v
            .independent_offset_secs
            .expect("parallel recheck should run");
        assert!(
            (independent - f64::from(OFFSET_SECS)).abs() < 0.5,
            "expected highest-peak window offset ~+{OFFSET_SECS}s, got {independent}"
        );
    }

    #[test]
    fn verify_offset_fails_wrong_delta() {
        use crate::application::testing::audio_fixtures::write_offset_chirp_wav_pair;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) =
            write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);

        let result = run_real_pipeline_verification(&path_a, &path_b, 8.0, verification_validation(), None);
        let v = result
            .offset_verification
            .expect("offset_verification must be set when flag on");

        assert!(
            !v.skipped,
            "window should be feasible even with wrong delta: skip_reason={:?}",
            v.skip_reason
        );
        assert!(
            !v.verified,
            "should not verify with wrong delta (confidence={}, would need lag ≤ 0.5 s and confidence ≥ 0.5)",
            v.confidence
        );
    }

    fn run_fake_holdout_verification(aligner: FakeAligner, offset_secs: f64) -> AlignmentResult {
        let duration = Duration::from_secs(120);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            duration: Some(duration),
            decodable: true,
        };
        let mut session_a = FakeMediaSession::with_duration(duration);
        let mut session_b = session_a.clone();

        let mut result = result_with_offset(offset_secs);
        let clip_config = holdout_clip_config();
        let validation = verification_validation();
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        let progress = FakeProgressReporter;

        apply_offset_verification(
            &mut verification_input(
                &mut session_a,
                &mut session_b,
                &track,
                &track,
                &windows,
                duration,
            ),
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        result
    }

    #[test]
    fn verify_offset_retries_until_verified() {
        let aligner = FakeAligner::with_estimates(vec![
            ClipMatchEstimate {
                offset_secs: 0.0,
                confidence: 0.3,
            },
            ClipMatchEstimate {
                offset_secs: 0.0,
                confidence: 0.9,
            },
        ]);

        let result = run_fake_holdout_verification(aligner, 3.0);
        let v = result.offset_verification.expect("verification");

        assert!(!v.skipped);
        assert!(v.verified, "confidence={}", v.confidence);
        assert_eq!(v.candidates_tried, 2);
    }

    #[test]
    fn verify_offset_reports_best_attempt_when_all_fail() {
        let aligner = FakeAligner::with_estimates(vec![
            ClipMatchEstimate {
                offset_secs: 0.0,
                confidence: 0.2,
            },
            ClipMatchEstimate {
                offset_secs: 0.0,
                confidence: 0.45,
            },
            ClipMatchEstimate {
                offset_secs: 0.0,
                confidence: 0.35,
            },
        ]);

        let result = run_fake_holdout_verification(aligner, 3.0);
        let v = result.offset_verification.expect("verification");

        assert!(!v.skipped);
        assert!(!v.verified);
        assert!((v.confidence - 0.45).abs() < f32::EPSILON);
        assert_eq!(v.candidates_tried, 3);
    }

    #[test]
    fn pick_best_scored_attempt_prefers_earlier_on_confidence_tie() {
        let attempts = vec![
            OffsetVerification {
                window_a_start_secs: 0.0,
                window_a_end_secs: 60.0,
                window_b_start_secs: 3.0,
                window_b_end_secs: 63.0,
                confidence: 0.4,
                verified: false,
                skipped: false,
                skip_reason: None,
                candidates_tried: 0,
                independent_offset_secs: None,
                parallel_recheck_delta_secs: None,
                verify_inconclusive: false,
            },
            OffsetVerification {
                window_a_start_secs: 30.0,
                window_a_end_secs: 90.0,
                window_b_start_secs: 33.0,
                window_b_end_secs: 93.0,
                confidence: 0.4,
                verified: false,
                skipped: false,
                skip_reason: None,
                candidates_tried: 0,
                independent_offset_secs: None,
                parallel_recheck_delta_secs: None,
                verify_inconclusive: false,
            },
        ];

        let best = pick_best_scored_attempt(&attempts).expect("best");
        assert_eq!(best.window_a_start_secs, 0.0);
    }

    #[test]
    fn verify_offset_skips_when_media_shorter_than_clip_length() {
        let duration = Duration::from_secs(30);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            duration: Some(duration),
            decodable: true,
        };
        let mut session_a = FakeMediaSession::with_duration(duration);
        let mut session_b = session_a.clone();

        let mut result = result_with_offset(3.0);
        let clip_config = ClipConfig {
            clip_length: Duration::from_secs(60),
            ..holdout_clip_config()
        };
        let validation = verification_validation();
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;

        apply_offset_verification(
            &mut verification_input(&mut session_a, &mut session_b, &track, &track, &windows, duration),
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        let v = result.offset_verification.expect("verification");
        assert!(v.skipped);
        assert_eq!(
            v.skip_reason.as_deref(),
            Some("hold-out window unavailable")
        );
    }

    #[test]
    fn verify_offset_skips_when_window_infeasible() {
        let duration = Duration::from_secs(120);
        let large_offset = 100.0_f64;

        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            duration: Some(duration),
            decodable: true,
        };
        let mut session_a = FakeMediaSession::with_duration(duration);
        let mut session_b = session_a.clone();

        let mut result = result_with_offset(large_offset);
        let clip_config = holdout_clip_config();
        let validation = verification_validation();
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;

        apply_offset_verification(
            &mut verification_input(&mut session_a, &mut session_b, &track, &track, &windows, duration),
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        let v = result
            .offset_verification
            .expect("offset_verification must be set even for skips");
        assert!(v.skipped, "expected skipped=true for infeasible window");
        assert!(!v.verified);
        assert_eq!(
            v.skip_reason.as_deref(),
            Some("hold-out window unavailable")
        );
    }

    #[test]
    fn verify_offset_skips_when_all_extracts_fail() {
        let duration = Duration::from_secs(120);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            duration: Some(duration),
            decodable: true,
        };
        let mut session_a = FakeMediaSession::with_duration(duration).with_extract_error(
            crate::application::error::MediaError::decode_failed(0, "boom"),
        );
        let mut session_b = session_a.clone();

        let mut result = result_with_offset(3.0);
        let clip_config = holdout_clip_config();
        let validation = verification_validation();
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;

        apply_offset_verification(
            &mut verification_input(&mut session_a, &mut session_b, &track, &track, &windows, duration),
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        let v = result.offset_verification.expect("verification");
        assert!(v.skipped);
        assert!(
            v.skip_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("extract")),
            "skip_reason={:?}",
            v.skip_reason
        );
    }

    #[test]
    fn verify_offset_skips_truncated_holdout_extract() {
        let duration = Duration::from_secs(120);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            duration: Some(duration),
            decodable: true,
        };
        let short_clip = MonoPcmClip {
            sample_rate: SAMPLE_RATE,
            samples: vec![0_i16; SAMPLE_RATE as usize * 10],
            decode_error_skips: 0,
            decoded_sample_count: Some(SAMPLE_RATE as usize * 10),
        };
        let mut session_a = FakeMediaSession::with_duration(duration).with_fixed_extract(short_clip);
        let mut session_b = session_a.clone();

        let mut result = result_with_offset(3.0);
        let clip_config = holdout_clip_config();
        let validation = verification_validation();
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;

        apply_offset_verification(
            &mut verification_input(&mut session_a, &mut session_b, &track, &track, &windows, duration),
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        let v = result.offset_verification.expect("verification");
        assert!(v.skipped);
        assert!(
            v.skip_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("shorter than clip_length")),
            "skip_reason={:?}",
            v.skip_reason
        );
    }

    #[test]
    fn verify_offset_leaves_none_when_flag_off() {
        let duration = Duration::from_secs(120);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            duration: Some(duration),
            decodable: true,
        };
        let mut session_a = FakeMediaSession::with_duration(duration);
        let mut session_b = session_a.clone();
        let mut result = result_with_offset(3.0);
        let clip_config = holdout_clip_config();
        let validation = ValidationConfig {
            verify_offset: false,
            ..Default::default()
        };
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;

        apply_offset_verification(
            &mut verification_input(&mut session_a, &mut session_b, &track, &track, &windows, duration),
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        assert!(
            result.offset_verification.is_none(),
            "flag off must leave offset_verification = None"
        );
    }

    #[test]
    fn alignment_result_json_offset_verification_present_when_flag_on() {
        let duration = Duration::from_secs(120);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            duration: Some(duration),
            decodable: true,
        };
        let mut session_a = FakeMediaSession::with_duration(duration);
        let mut session_b = session_a.clone();
        let mut result = result_with_offset(3.0);
        let clip_config = holdout_clip_config();
        let validation = verification_validation();
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;

        apply_offset_verification(
            &mut verification_input(&mut session_a, &mut session_b, &track, &track, &windows, duration),
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        let report = crate::application::report::AlignmentReport::from(&result);
        let json = serde_json::to_string(&report).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

        let ov = &value["offset_verification"];
        assert!(ov.is_object(), "offset_verification must be present when flag on");
        assert!(ov["verified"].is_boolean());
        assert!(ov["skipped"].is_boolean());
        assert!(ov["confidence"].is_number());
    }

    #[test]
    fn alignment_result_json_offset_verification_absent_when_flag_off() {
        let duration = Duration::from_secs(120);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            duration: Some(duration),
            decodable: true,
        };
        let mut session_a = FakeMediaSession::with_duration(duration);
        let mut session_b = session_a.clone();
        let mut result = result_with_offset(3.0);
        let clip_config = holdout_clip_config();
        let validation = ValidationConfig { verify_offset: false, ..Default::default() };
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;

        apply_offset_verification(
            &mut verification_input(&mut session_a, &mut session_b, &track, &track, &windows, duration),
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        let report = crate::application::report::AlignmentReport::from(&result);
        let json = serde_json::to_string(&report).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");
        assert!(
            value.get("offset_verification").is_none(),
            "offset_verification must be absent (not null) when flag off"
        );
    }

    #[test]
    fn verification_downgrade_when_holdout_repeats() {
        use crate::application::testing::audio_fixtures::write_pure_tone_repeat_wav_pair;

        const ALIGN_OFFSET: f64 = 30.0;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) =
            write_pure_tone_repeat_wav_pair(temp.path(), 44_100, 130, 30);

        let baseline = run_real_pipeline_verification(
            &path_a,
            &path_b,
            ALIGN_OFFSET,
            verification_validation(),
            None,
        );
        let base = baseline
            .offset_verification
            .as_ref()
            .expect("baseline verification");
        assert!(!base.skipped, "baseline skip_reason={:?}", base.skip_reason);
        let base_confidence = base.confidence;

        let mut validation = verification_validation();
        validation.check_clip_repetition = true;
        let downgraded_result = run_real_pipeline_verification(
            &path_a,
            &path_b,
            ALIGN_OFFSET,
            validation,
            None,
        );
        let downgraded = downgraded_result
            .offset_verification
            .as_ref()
            .expect("downgraded verification");
        assert!(!downgraded.skipped);
        assert!(
            downgraded.confidence < base_confidence,
            "base={base_confidence}, downgraded={}",
            downgraded.confidence
        );
        assert!(
            (downgraded.confidence - base_confidence * 0.5).abs() < 0.05,
            "expected ~halved confidence: base={base_confidence}, got={}",
            downgraded.confidence
        );
    }
}
