//! Scan → optional patch/write orchestration (port-injected; adapter wiring in `composition`).

use std::path::PathBuf;

use clip_sync::{MediaReader, ProgressReporter};

use crate::application::error::RepairError;
use crate::application::patch_audio::{PatchAudio, PatchAudioResult, PatchRequestSettings};
use crate::application::ports::Aligner;
use crate::application::ports::PatchedAudioWriter;
use crate::application::repair_videos::{RepairVideos, RepairWriteRequest};
use crate::application::scan_gaps::{ScanGaps, ScanGapsRequest};
use crate::domain::GapReport;

#[cfg(feature = "ffmpeg-mux")]
use crate::application::mux_bitrate::MuxAudioBitratePolicy;
#[cfg(feature = "ffmpeg-mux")]
use crate::application::ports::{MediaMuxer, MuxOptions};

/// Patch/write step deferred until after gap scan (report filled in at execution).
pub struct PendingRepairWrite {
    pub source_video: PathBuf,
    pub patch_settings: PatchRequestSettings,
    pub crossfade_ms: u64,
    pub wav_path: Option<PathBuf>,
    #[cfg(feature = "ffmpeg-mux")]
    pub video_path: Option<PathBuf>,
    #[cfg(feature = "ffmpeg-mux")]
    pub mux_options: MuxOptions,
    #[cfg(feature = "ffmpeg-mux")]
    pub mux_audio_bitrate_policy: MuxAudioBitratePolicy,
}

/// Characterize-only step after scan (`--repair-preview`): no splice / file write.
pub struct PendingRepairPreview {
    pub patch_settings: PatchRequestSettings,
    pub crossfade_ms: u64,
}

pub struct RepairRunInput {
    pub scan: ScanGapsRequest,
    /// Full write path (`--wav` / `--mux`). Mutually exclusive with [`Self::preview`].
    pub write: Option<PendingRepairWrite>,
    /// Pass-1 characterize only. Mutually exclusive with [`Self::write`].
    pub preview: Option<PendingRepairPreview>,
}

pub struct RepairRunOutcome {
    pub report: GapReport,
    pub alignment_detail: clip_sync::AlignmentResult,
    pub patch_result: Result<Option<PatchAudioResult>, RepairError>,
}

fn into_write_request(
    pending: PendingRepairWrite,
    report: GapReport,
) -> Result<RepairWriteRequest, RepairError> {
    Ok(RepairWriteRequest {
        source_video: pending.source_video,
        patch_request: pending.patch_settings.into_request(report),
        crossfade_ms: pending.crossfade_ms,
        wav_path: pending.wav_path,
        #[cfg(feature = "ffmpeg-mux")]
        video_path: pending.video_path,
        #[cfg(feature = "ffmpeg-mux")]
        mux_options: pending.mux_options,
        #[cfg(feature = "ffmpeg-mux")]
        mux_audio_bitrate_policy: pending.mux_audio_bitrate_policy,
    })
}

fn run_preview_patch<MR: MediaReader>(
    media_reader: &MR,
    progress: &dyn ProgressReporter,
    pending: PendingRepairPreview,
    report: GapReport,
) -> Result<PatchAudioResult, RepairError> {
    let request = pending.patch_settings.into_request(report);
    PatchAudio::new(media_reader, progress).preview(request, pending.crossfade_ms)
}

#[cfg(feature = "ffmpeg-mux")]
pub fn run_repair<MR, A, PW, MM>(
    input: RepairRunInput,
    media_reader: &MR,
    aligner: &A,
    wav_writer: &PW,
    muxer: &MM,
    progress: &dyn ProgressReporter,
) -> Result<RepairRunOutcome, RepairError>
where
    MR: MediaReader,
    A: Aligner,
    PW: PatchedAudioWriter,
    MM: MediaMuxer,
{
    let scan = ScanGaps::new(media_reader, progress, aligner).execute(input.scan)?;
    let report = scan.report;
    let alignment_detail = scan.alignment_detail;

    let patch_result = match (input.write, input.preview) {
        (Some(_), Some(_)) => Err(RepairError::Config(
            "internal: write and repair-preview both set".into(),
        )),
        (Some(pending), None) => into_write_request(pending, report.clone()).and_then(
            |write_request| {
                let repair = RepairVideos::new(media_reader, progress, wav_writer);
                repair.execute(write_request, muxer).map(Some)
            },
        ),
        (None, Some(pending)) => {
            run_preview_patch(media_reader, progress, pending, report.clone()).map(Some)
        }
        (None, None) => Ok(None),
    };

    Ok(RepairRunOutcome {
        report,
        alignment_detail,
        patch_result,
    })
}

#[cfg(not(feature = "ffmpeg-mux"))]
pub fn run_repair<MR, A, PW>(
    input: RepairRunInput,
    media_reader: &MR,
    aligner: &A,
    wav_writer: &PW,
    progress: &dyn ProgressReporter,
) -> Result<RepairRunOutcome, RepairError>
where
    MR: MediaReader,
    A: Aligner,
    PW: PatchedAudioWriter,
{
    let scan = ScanGaps::new(media_reader, progress, aligner).execute(input.scan)?;
    let report = scan.report;
    let alignment_detail = scan.alignment_detail;

    let patch_result = match (input.write, input.preview) {
        (Some(_), Some(_)) => Err(RepairError::Config(
            "internal: write and repair-preview both set".into(),
        )),
        (Some(pending), None) => into_write_request(pending, report.clone()).and_then(
            |write_request| {
                let repair = RepairVideos::new(media_reader, progress, wav_writer);
                repair.execute(write_request).map(Some)
            },
        ),
        (None, Some(pending)) => {
            run_preview_patch(media_reader, progress, pending, report.clone()).map(Some)
        }
        (None, None) => Ok(None),
    };

    Ok(RepairRunOutcome {
        report,
        alignment_detail,
        patch_result,
    })
}
