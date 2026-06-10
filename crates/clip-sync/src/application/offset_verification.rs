use std::time::Duration;

use tracing::debug;

use crate::application::config::{ClipConfig, ValidationConfig};
use crate::application::ports::{Aligner, Fingerprinter, MediaSession, ProgressReporter};
use crate::domain::{
    holdout_window_candidates, holdout_window_feasible, prepare_clip_for_fingerprint,
    resample_mono_pcm, AlignmentResult, AudioTrack, ClipWindow, OffsetVerification,
    PcmPreparationOptions, OFFSET_AGREEMENT_TOLERANCE_SECS,
};

pub struct OffsetVerificationInput<'a, MS: MediaSession> {
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

/// Extract a hold-out window and score lag-0 similarity to independently verify the recommended
/// offset. Writes `result.offset_verification` when `validation.verify_offset` is on (including
/// skip cases); leaves the field `None` when the flag is off.
pub fn apply_offset_verification<MS, FP, AL>(
    input: &OffsetVerificationInput<'_, MS>,
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

    let &OffsetVerificationInput {
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

    let clip_length = clip_config.clip_length;
    let clip_length_secs = clip_length.as_secs_f64();

    let pick_duration = duration_a
        .min(duration_b)
        .min(decoded_extent_a)
        .min(decoded_extent_b);
    let dur_a = duration_a.min(decoded_extent_a).as_secs_f64();
    let dur_b = duration_b.min(decoded_extent_b).as_secs_f64();

    let candidates =
        holdout_window_candidates(pick_duration, discovery_windows, clip_length, offset_secs);
    if candidates.is_empty() {
        debug!("offset verify skipped: no hold-out window candidates");
        result.offset_verification = Some(skipped("hold-out window unavailable"));
        return;
    }

    let prep_options = PcmPreparationOptions {
        normalize_loudness: clip_config.normalize_loudness,
        trim_silence: clip_config.trim_silence,
        window_slide_secs: 0,
    };

    let _ = session_a.reset_io();
    let _ = session_b.reset_io();

    for holdout in &candidates {
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

        let window_b_start = Duration::from_secs_f64(window_start_secs + offset_secs);
        let window_b_end =
            Duration::from_secs_f64(window_start_secs + clip_length_secs + offset_secs);
        let window_b = ClipWindow::new(window_b_start, window_b_end, holdout.label);

        let raw_a = match session_a.extract_mono(
            track_a,
            holdout,
            progress,
            "Verifying hold-out (video A)",
        ) {
            Ok(clip) => clip,
            Err(e) => {
                debug!(error = %e, "offset verify: extract A failed, trying next candidate");
                continue;
            }
        };
        let raw_b = match session_b.extract_mono(
            track_b,
            &window_b,
            progress,
            "Verifying hold-out (video B)",
        ) {
            Ok(clip) => clip,
            Err(e) => {
                debug!(error = %e, "offset verify: extract B failed, trying next candidate");
                continue;
            }
        };

        let raw_a = match clip_config.target_sample_rate {
            Some(rate) => resample_mono_pcm(&raw_a, rate),
            None => raw_a,
        };
        let raw_b = match clip_config.target_sample_rate {
            Some(rate) => resample_mono_pcm(&raw_b, rate),
            None => raw_b,
        };

        let prepared_a = match prepare_clip_for_fingerprint(&raw_a, prep_options) {
            Ok(clip) => clip,
            Err(e) => {
                debug!(error = ?e, "offset verify: prepare A failed, trying next candidate");
                continue;
            }
        };
        let prepared_b = match prepare_clip_for_fingerprint(&raw_b, prep_options) {
            Ok(clip) => clip,
            Err(e) => {
                debug!(error = ?e, "offset verify: prepare B failed, trying next candidate");
                continue;
            }
        };

        let fp_a = match fingerprinter.fingerprint(&prepared_a) {
            Ok(fp) => fp,
            Err(e) => {
                debug!(error = %e, "offset verify: fingerprint A failed, trying next candidate");
                continue;
            }
        };
        let fp_b = match fingerprinter.fingerprint(&prepared_b) {
            Ok(fp) => fp,
            Err(e) => {
                debug!(error = %e, "offset verify: fingerprint B failed, trying next candidate");
                continue;
            }
        };

        let estimate = match aligner.find_offset(&fp_a, &fp_b) {
            Ok(e) => e,
            Err(e) => {
                debug!(error = %e, "offset verify: aligner failed, trying next candidate");
                continue;
            }
        };

        let verified = estimate.confidence >= validation.min_verification_confidence
            && estimate.offset_secs.abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS;

        let window_b_start_secs = window_start_secs + offset_secs;
        let window_b_end_secs = window_start_secs + clip_length_secs + offset_secs;

        debug!(
            window_a_start_secs = window_start_secs,
            window_b_start_secs,
            confidence = estimate.confidence,
            lag_secs = estimate.offset_secs,
            verified,
            "offset verification result"
        );

        result.offset_verification = Some(OffsetVerification {
            window_a_start_secs: window_start_secs,
            window_a_end_secs: window_start_secs + clip_length_secs,
            window_b_start_secs,
            window_b_end_secs,
            confidence: estimate.confidence,
            verified,
            skipped: false,
            skip_reason: None,
        });
        return;
    }

    debug!("offset verify skipped: no feasible hold-out window");
    result.offset_verification = Some(skipped("hold-out window unavailable"));
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
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::application::config::{ClipConfig, ValidationConfig};
    use crate::application::testing::fakes::{
        FakeAligner, FakeFingerprinter, FakeMediaSession, FakeProgressReporter,
    };
    use crate::domain::{AudioTrack, ClipLabel, ClipMatchEstimate, ClipWindow};

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
        AlignmentResult {
            clips: vec![],
            start_aligned: true,
            end_aligned: None,
            recommended_offset_secs: Some(offset_secs),
            offsets_consistent: true,
            offset_drift_secs: None,
            start_overlap: None,
            high_rate_refinement: None,
            offset_verification: None,
        }
    }

    fn discovery_windows() -> Vec<ClipWindow> {
        vec![ClipWindow::new(
            Duration::ZERO,
            Duration::from_secs(30),
            ClipLabel::Start,
        )]
    }

    fn run_real_pipeline_verification(
        path_a: &std::path::Path,
        path_b: &std::path::Path,
        offset_secs: f64,
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

        let session_a = media_reader
            .open(&MediaSource::new(path_a))
            .expect("open a");
        let session_b = media_reader
            .open(&MediaSource::new(path_b))
            .expect("open b");
        let tracks_a = session_a.list_tracks().expect("tracks a");
        let tracks_b = session_b.list_tracks().expect("tracks b");
        let track_a = &tracks_a[0];
        let track_b = &tracks_b[0];
        let duration = track_a.duration.expect("duration");

        let mut result = result_with_offset(offset_secs);
        let clip_config = holdout_clip_config();
        let validation = verification_validation();
        let windows = discovery_windows();

        apply_offset_verification(
            &OffsetVerificationInput {
                session_a: &session_a,
                session_b: &session_b,
                track_a,
                track_b,
                discovery_windows: &windows,
                duration_a: duration,
                duration_b: duration,
                decoded_extent_a: duration,
                decoded_extent_b: duration,
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

        let result = run_real_pipeline_verification(&path_a, &path_b, f64::from(OFFSET_SECS));
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

        let result =
            run_real_pipeline_verification(&path_a, &path_b, -f64::from(OFFSET_SECS));
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
    fn verify_offset_fails_wrong_delta() {
        use crate::application::testing::audio_fixtures::write_offset_chirp_wav_pair;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) =
            write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);

        let result = run_real_pipeline_verification(&path_a, &path_b, 8.0);
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

    #[test]
    fn verify_offset_skips_when_window_infeasible() {
        let duration = Duration::from_secs(120);
        let large_offset = 100.0_f64;

        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bitrate: None,
            duration: Some(duration),
            decodable: true,
        };
        let session = FakeMediaSession::with_duration(duration);

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
            &OffsetVerificationInput {
                session_a: &session,
                session_b: &session,
                track_a: &track,
                track_b: &track,
                discovery_windows: &windows,
                duration_a: duration,
                duration_b: duration,
                decoded_extent_a: duration,
                decoded_extent_b: duration,
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
            .expect("offset_verification must be set even for skips");
        assert!(v.skipped, "expected skipped=true for infeasible window");
        assert!(!v.verified);
        assert!(v.skip_reason.is_some());
    }

    #[test]
    fn verify_offset_leaves_none_when_flag_off() {
        let duration = Duration::from_secs(120);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bitrate: None,
            duration: Some(duration),
            decodable: true,
        };
        let session = FakeMediaSession::with_duration(duration);
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
            &OffsetVerificationInput {
                session_a: &session,
                session_b: &session,
                track_a: &track,
                track_b: &track,
                discovery_windows: &windows,
                duration_a: duration,
                duration_b: duration,
                decoded_extent_a: duration,
                decoded_extent_b: duration,
            },
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

    // JSON shape tests (Phase 2)

    #[test]
    fn alignment_result_json_offset_verification_present_when_flag_on() {
        let duration = Duration::from_secs(120);
        let track = AudioTrack {
            index: 0,
            codec: "test".into(),
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bitrate: None,
            duration: Some(duration),
            decodable: true,
        };
        let session = FakeMediaSession::with_duration(duration);
        let mut result = result_with_offset(3.0);
        let clip_config = holdout_clip_config();
        let validation = verification_validation();
        let windows = discovery_windows();
        let fingerprinter = FakeFingerprinter::new();
        // Aligner returns lag ≈ 0 and high confidence → verified = true
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;

        apply_offset_verification(
            &OffsetVerificationInput {
                session_a: &session,
                session_b: &session,
                track_a: &track,
                track_b: &track,
                discovery_windows: &windows,
                duration_a: duration,
                duration_b: duration,
                decoded_extent_a: duration,
                decoded_extent_b: duration,
            },
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        let json = serde_json::to_string(&result).expect("serialize");
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
            bitrate: None,
            duration: Some(duration),
            decodable: true,
        };
        let session = FakeMediaSession::with_duration(duration);
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
            &OffsetVerificationInput {
                session_a: &session,
                session_b: &session,
                track_a: &track,
                track_b: &track,
                discovery_windows: &windows,
                duration_a: duration,
                duration_b: duration,
                decoded_extent_a: duration,
                decoded_extent_b: duration,
            },
            &clip_config,
            &validation,
            &mut result,
            &fingerprinter,
            &aligner,
            &progress,
        );

        let json = serde_json::to_string(&result).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");
        assert!(
            value.get("offset_verification").is_none(),
            "offset_verification must be absent (not null) when flag off"
        );
    }
}
