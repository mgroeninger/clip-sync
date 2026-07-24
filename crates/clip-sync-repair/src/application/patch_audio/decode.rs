use std::time::Duration;

use clip_sync::{
    resample_interleaved, select_best_track, select_track_for_reference, ClipLabel, ClipWindow,
    DomainError, MediaReader, MediaSession, MediaSource, MultiChannelPcm, ProgressReporter,
};

use crate::application::error::RepairError;
use crate::domain::GapReport;

/// Full-track decoded A/B for the repair fill path (and the gap-fingerprint diagnostic).
pub(crate) struct DecodedAb {
    pub a_pcm: MultiChannelPcm,
    /// B resampled to A's rate (interleaved).
    pub b_samples_full: Vec<f32>,
    pub source_audio_bitrate_a_bps: Option<u32>,
    pub source_audio_bitrate_b_bps: Option<u32>,
    /// A track's container-reported duration (for PCM-vs-container skew).
    pub container_duration_a_secs: f64,
}

/// Open A and B, select tracks, decode both full timelines, and resample B to A's rate. Extracted
/// verbatim from `PatchAudio::run` (steps 2–6) so the fingerprint diagnostic decodes identically.
pub(crate) fn decode_ab<MR: MediaReader>(
    media_reader: &MR,
    report: &GapReport,
    progress: &dyn ProgressReporter,
) -> Result<DecodedAb, RepairError> {
    // Step 2: Open A, select best track, get duration.
    let source_a = MediaSource::new(report.video_a.clone());
    let mut session_a = media_reader.open(&source_a).map_err(RepairError::Media)?;
    let tracks_a = session_a.list_tracks().map_err(RepairError::Media)?;
    let track_a = select_best_track(&tracks_a)?.clone();
    let duration_a = track_a
        .duration
        .ok_or(RepairError::Domain(DomainError::InvalidDuration))?;

    // Step 3: Extract full A timeline.
    let full_window_a = ClipWindow::new(Duration::ZERO, duration_a, ClipLabel::Interior);
    let a_pcm = {
        let _decode_a = tracing::info_span!(
            "patch_decode_a",
            path = %report.video_a.display(),
            duration_secs = duration_a.as_secs_f64(),
            channels = track_a.channels,
            sample_rate = track_a.sample_rate,
            bit_depth = ?track_a.bit_depth,
        )
        .entered();
        session_a
            .extract_interleaved(&track_a, &full_window_a, progress, "patch-a")
            .map_err(RepairError::Media)?
    };
    let source_audio_bitrate_a_bps = a_pcm.measured_bitrate_bps();

    // Step 5: Open B, select best track.
    let source_b = MediaSource::new(report.video_b.clone());
    let mut session_b = media_reader.open(&source_b).map_err(RepairError::Media)?;
    let tracks_b = session_b.list_tracks().map_err(RepairError::Media)?;
    let track_b = select_track_for_reference(&track_a, &tracks_b)?.clone();

    // Step 6: Decode full B timeline once (sequential from t=0) to avoid per-gap MKV seeks.
    let duration_b = track_b
        .duration
        .ok_or(RepairError::Domain(DomainError::InvalidDuration))?;
    let full_window_b = ClipWindow::new(Duration::ZERO, duration_b, ClipLabel::Interior);
    let b_pcm_full = {
        let _decode_b = tracing::info_span!(
            "patch_decode_b",
            path = %report.video_b.display(),
            duration_secs = duration_b.as_secs_f64(),
            channels = track_b.channels,
            sample_rate = track_b.sample_rate,
            bit_depth = ?track_b.bit_depth,
        )
        .entered();
        session_b
            .extract_interleaved(&track_b, &full_window_b, progress, "patch-b")
            .map_err(RepairError::Media)?
    };
    let source_audio_bitrate_b_bps = b_pcm_full.measured_bitrate_bps();
    let b_samples_full = if b_pcm_full.sample_rate != a_pcm.sample_rate {
        let _resample = tracing::debug_span!(
            "patch_resample_b",
            from_rate = b_pcm_full.sample_rate,
            to_rate = a_pcm.sample_rate,
        )
        .entered();
        resample_interleaved(
            &b_pcm_full.samples,
            b_pcm_full.channels,
            b_pcm_full.sample_rate,
            a_pcm.sample_rate,
        )
    } else {
        b_pcm_full.samples
    };

    Ok(DecodedAb {
        a_pcm,
        b_samples_full,
        source_audio_bitrate_a_bps,
        source_audio_bitrate_b_bps,
        container_duration_a_secs: duration_a.as_secs_f64(),
    })
}
