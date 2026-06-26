use std::path::PathBuf;

use clip_sync::{MediaReader, MultiChannelPcm, ProgressReporter};

use crate::application::error::RepairError;
use crate::application::patch_audio::{PatchAudio, PatchAudioRequest, PatchAudioResult};
use crate::application::ports::PatchedAudioWriter;
#[cfg(feature = "ffmpeg-mux")]
use crate::application::ports::{MediaMuxer, MuxOptions};
#[cfg(feature = "ffmpeg-mux")]
use crate::application::mux_bitrate::{
    format_mux_bitrate_policy, format_optional_bitrate_kbps, resolve_mux_audio_bitrate,
    MuxAudioBitratePolicy,
};

pub struct RepairWriteRequest {
    /// Video A path — used as the video source for mux.
    pub source_video: PathBuf,
    pub patch_request: PatchAudioRequest,
    pub crossfade_ms: u64,
    pub wav_path: Option<PathBuf>,
    #[cfg(feature = "ffmpeg-mux")]
    pub video_path: Option<PathBuf>,
    #[cfg(feature = "ffmpeg-mux")]
    pub mux_options: MuxOptions,
    #[cfg(feature = "ffmpeg-mux")]
    pub mux_audio_bitrate_policy: MuxAudioBitratePolicy,
}

/// Output paths for a repair write pass (patch request is handled separately).
pub(crate) struct RepairFileOutput {
    #[cfg(feature = "ffmpeg-mux")]
    source_video: PathBuf,
    wav_path: Option<PathBuf>,
    #[cfg(feature = "ffmpeg-mux")]
    video_path: Option<PathBuf>,
    #[cfg(feature = "ffmpeg-mux")]
    mux_options: MuxOptions,
    #[cfg(feature = "ffmpeg-mux")]
    mux_audio_bitrate_policy: MuxAudioBitratePolicy,
}

impl RepairFileOutput {
    fn wants_file_output(&self) -> bool {
        if self.wav_path.is_some() {
            return true;
        }
        #[cfg(feature = "ffmpeg-mux")]
        if self.video_path.is_some() {
            return true;
        }
        false
    }
}

pub struct RepairVideos<'r, MR: MediaReader, PW: PatchedAudioWriter> {
    media_reader: &'r MR,
    progress: &'r dyn ProgressReporter,
    wav_writer: &'r PW,
}

impl<'r, MR: MediaReader, PW: PatchedAudioWriter> RepairVideos<'r, MR, PW> {
    pub fn new(
        media_reader: &'r MR,
        progress: &'r dyn ProgressReporter,
        wav_writer: &'r PW,
    ) -> Self {
        Self {
            media_reader,
            progress,
            wav_writer,
        }
    }

    #[cfg(not(feature = "ffmpeg-mux"))]
    pub fn execute(&self, request: RepairWriteRequest) -> Result<PatchAudioResult, RepairError> {
        let RepairWriteRequest {
            source_video: _,
            patch_request,
            crossfade_ms,
            wav_path,
        } = request;
        let file_output = RepairFileOutput { wav_path };

        let patch_result = PatchAudio::new(self.media_reader, self.progress)
            .execute(patch_request, crossfade_ms)?;

        self.write_outputs(&patch_result, &file_output)?;
        Ok(patch_result)
    }

    #[cfg(feature = "ffmpeg-mux")]
    pub fn execute<MM: MediaMuxer>(
        &self,
        request: RepairWriteRequest,
        muxer: &MM,
    ) -> Result<PatchAudioResult, RepairError> {
        let RepairWriteRequest {
            source_video,
            patch_request,
            crossfade_ms,
            wav_path,
            video_path,
            mux_options,
            mux_audio_bitrate_policy,
        } = request;
        let file_output = RepairFileOutput {
            source_video,
            wav_path,
            video_path,
            mux_options,
            mux_audio_bitrate_policy,
        };

        let patch_result = PatchAudio::new(self.media_reader, self.progress)
            .execute(patch_request, crossfade_ms)?;

        self.write_outputs(&patch_result, &file_output, muxer)?;
        Ok(patch_result)
    }

    /// Returns decoded A PCM when at least one gap was patched; otherwise `None`.
    fn gated_pcm<'a>(
        &self,
        result: &'a PatchAudioResult,
        output: &RepairFileOutput,
    ) -> Result<Option<&'a MultiChannelPcm>, RepairError> {
        if !result.summary.has_patches() {
            if output.wants_file_output() {
                self.progress
                    .phase("No gaps were patched; skipping WAV/mux output.");
            }
            return Ok(None);
        }

        let pcm = result.pcm.as_ref().ok_or_else(|| {
            RepairError::Config("internal: patched run missing decoded PCM".into())
        })?;
        Ok(Some(pcm))
    }

    fn write_wav_if_requested(
        &self,
        pcm: &MultiChannelPcm,
        output: &RepairFileOutput,
    ) -> Result<(), RepairError> {
        if let Some(wav_path) = &output.wav_path {
            self.wav_writer.write(pcm, wav_path)?;
        }
        Ok(())
    }

    #[cfg(not(feature = "ffmpeg-mux"))]
    pub(crate) fn write_outputs(
        &self,
        result: &PatchAudioResult,
        output: &RepairFileOutput,
    ) -> Result<(), RepairError> {
        let Some(pcm) = self.gated_pcm(result, output)? else {
            return Ok(());
        };
        self.write_wav_if_requested(pcm, output)
    }

    #[cfg(feature = "ffmpeg-mux")]
    pub(crate) fn write_outputs<MM: MediaMuxer>(
        &self,
        result: &PatchAudioResult,
        output: &RepairFileOutput,
        muxer: &MM,
    ) -> Result<(), RepairError> {
        let Some(pcm) = self.gated_pcm(result, output)? else {
            return Ok(());
        };

        self.write_wav_if_requested(pcm, output)?;

        if let Some(video_path) = &output.video_path {
            let mut mux_options = output.mux_options.clone();
            mux_options.audio_bitrate = resolve_mux_audio_bitrate(
                output.mux_audio_bitrate_policy,
                result.source_audio_bitrate_a_bps,
                result.source_audio_bitrate_b_bps,
            );
            if let Some(ref bitrate) = mux_options.audio_bitrate {
                self.progress.phase_verbose(&format!(
                    "Mux AAC bitrate {bitrate} (A {}, B {}, policy {})",
                    format_optional_bitrate_kbps(result.source_audio_bitrate_a_bps),
                    format_optional_bitrate_kbps(result.source_audio_bitrate_b_bps),
                    format_mux_bitrate_policy(output.mux_audio_bitrate_policy),
                ));
            }
            muxer.mux_video_with_replaced_audio(
                &output.source_video,
                pcm,
                video_path,
                &mux_options,
                self.progress,
            )?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use clip_sync::testing::fakes::FakeProgressReporter;
    use clip_sync::{MediaError, MediaReader, MediaSession, MediaSource, MultiChannelPcm};

    use crate::application::patch_audio::PatchAudioResult;
    use crate::domain::fill_mode::FillMode;
    use crate::domain::gap_tags::FillTierThresholds;
    use crate::domain::patch_result::{GapPatchOutcome, GapPatchStatus, PatchSummary};

    #[cfg(feature = "ffmpeg-mux")]
    use crate::application::mux_bitrate::MuxAudioBitratePolicy;

    use super::*;

    struct UnusedMediaReader;

    impl MediaReader for UnusedMediaReader {
        type Session = UnusedSession;

        fn open(&self, _source: &MediaSource) -> Result<Self::Session, MediaError> {
            unreachable!("write_outputs tests do not open media")
        }
    }

    struct UnusedSession;

    impl MediaSession for UnusedSession {
        fn list_tracks(&self) -> Result<Vec<clip_sync::AudioTrack>, MediaError> {
            unreachable!()
        }

        fn extract_mono(
            &mut self,
            _track: &clip_sync::AudioTrack,
            _window: &clip_sync::ClipWindow,
            _progress: &dyn ProgressReporter,
            _label: &str,
        ) -> Result<clip_sync::MonoPcmClip, MediaError> {
            unreachable!()
        }
    }

    struct RecordingWavWriter {
        write_count: AtomicUsize,
    }

    impl RecordingWavWriter {
        fn new() -> Self {
            Self {
                write_count: AtomicUsize::new(0),
            }
        }

        fn writes(&self) -> usize {
            self.write_count.load(Ordering::SeqCst)
        }
    }

    impl PatchedAudioWriter for RecordingWavWriter {
        fn write(&self, _audio: &MultiChannelPcm, _path: &Path) -> Result<(), RepairError> {
            self.write_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[cfg(feature = "ffmpeg-mux")]
    struct RecordingMuxer {
        mux_count: AtomicUsize,
    }

    #[cfg(feature = "ffmpeg-mux")]
    impl RecordingMuxer {
        fn new() -> Self {
            Self {
                mux_count: AtomicUsize::new(0),
            }
        }

        fn muxes(&self) -> usize {
            self.mux_count.load(Ordering::SeqCst)
        }
    }

    #[cfg(feature = "ffmpeg-mux")]
    impl MediaMuxer for RecordingMuxer {
        fn mux_video_with_replaced_audio(
            &self,
            _source_video: &Path,
            _replacement_audio: &MultiChannelPcm,
            _output: &Path,
            _options: &MuxOptions,
            _progress: &dyn ProgressReporter,
        ) -> Result<(), RepairError> {
            self.mux_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct RecordingProgress {
        skip_message: Cell<bool>,
    }

    impl RecordingProgress {
        fn new() -> Self {
            Self {
                skip_message: Cell::new(false),
            }
        }

        fn saw_skip_message(&self) -> bool {
            self.skip_message.get()
        }
    }

    impl ProgressReporter for RecordingProgress {
        fn phase(&self, message: &str) {
            if message.contains("skipping WAV/mux output") {
                self.skip_message.set(true);
            }
        }

        fn progress(&self, _label: &str, _current: u64, _total: u64) {}
    }

    fn sample_pcm() -> MultiChannelPcm {
        MultiChannelPcm {
            sample_rate: 44_100,
            channels: 1,
            samples: vec![1_000.0_f32 / 32767.0; 100],
            decode_error_skips: 0,
            decoded_frame_count: Some(100),
            compressed_bytes: None,
            source_bit_depth: None,
        }
    }

    fn file_output(wav_path: Option<PathBuf>) -> RepairFileOutput {
        RepairFileOutput {
            #[cfg(feature = "ffmpeg-mux")]
            source_video: PathBuf::from("a.mp4"),
            wav_path,
            #[cfg(feature = "ffmpeg-mux")]
            video_path: None,
            #[cfg(feature = "ffmpeg-mux")]
            mux_options: MuxOptions {
                video_codec: "copy".into(),
                audio_codec: "aac".into(),
                audio_bitrate: None,
            },
            #[cfg(feature = "ffmpeg-mux")]
            mux_audio_bitrate_policy: MuxAudioBitratePolicy::MatchMin,
        }
    }

    fn patch_result(patched_count: usize, pcm: Option<MultiChannelPcm>) -> PatchAudioResult {
        use crate::domain::fill_mode::FillMode;
        use crate::domain::gap_tags::FillTierThresholds;

        let gaps = if patched_count > 0 {
            vec![GapPatchOutcome::with_tags_from_status(
                1.0,
                2.0,
                GapPatchStatus::Patched {
                    pre_correlation: 0.9,
                    post_correlation: 0.9,
                    align_adjustment_secs: 0.0,
                    waveform_adjustment_secs: 0.0,
                    structure_trusted: false,
                    confidence: crate::domain::FillConfidence::High,
                    gap_start_adjust_frames: 0,
                    gap_end_adjust_frames: 0,
                    residual_db: None,
                    floor_db: None,
                    headroom_db: None,
                    anchor_seam_used: false,
                    anchor_bracket_move_frames: 0,
                },
                FillMode::Fit,
                FillTierThresholds::DEFAULT,
            )]
        } else {
            vec![]
        };

        PatchAudioResult {
            pcm,
            summary: PatchSummary::from_outcomes(gaps),
            source_audio_bitrate_a_bps: None,
            source_audio_bitrate_b_bps: None,
            pcm_container_skew: None,
        }
    }

    #[test]
    fn write_outputs_skips_wav_when_no_gaps_patched() {
        let progress = RecordingProgress::new();
        let writer = RecordingWavWriter::new();
        let repair = RepairVideos::new(&UnusedMediaReader, &progress, &writer);

        let result = patch_result(0, Some(sample_pcm()));
        let output = file_output(Some(PathBuf::from("out.wav")));

        #[cfg(not(feature = "ffmpeg-mux"))]
        repair
            .write_outputs(&result, &output)
            .expect("write_outputs should succeed");

        #[cfg(feature = "ffmpeg-mux")]
        {
            let muxer = RecordingMuxer::new();
            repair
                .write_outputs(&result, &output, &muxer)
                .expect("write_outputs should succeed");
            assert_eq!(muxer.muxes(), 0);
        }

        assert_eq!(writer.writes(), 0);
        assert!(progress.saw_skip_message());
    }

    #[test]
    fn write_outputs_writes_wav_when_gaps_patched() {
        let progress = FakeProgressReporter;
        let writer = RecordingWavWriter::new();
        let repair = RepairVideos::new(&UnusedMediaReader, &progress, &writer);

        let result = patch_result(1, Some(sample_pcm()));
        let output = file_output(Some(PathBuf::from("out.wav")));

        #[cfg(not(feature = "ffmpeg-mux"))]
        repair
            .write_outputs(&result, &output)
            .expect("write_outputs should succeed");

        #[cfg(feature = "ffmpeg-mux")]
        {
            let muxer = RecordingMuxer::new();
            repair
                .write_outputs(&result, &output, &muxer)
                .expect("write_outputs should succeed");
            assert_eq!(muxer.muxes(), 0);
        }

        assert_eq!(writer.writes(), 1);
    }

    #[cfg(feature = "ffmpeg-mux")]
    #[test]
    fn write_outputs_skips_mux_when_no_gaps_patched() {
        let progress = RecordingProgress::new();
        let writer = RecordingWavWriter::new();
        let muxer = RecordingMuxer::new();
        let repair = RepairVideos::new(&UnusedMediaReader, &progress, &writer);

        let result = patch_result(0, None);
        let output = RepairFileOutput {
            source_video: PathBuf::from("a.mp4"),
            wav_path: None,
            video_path: Some(PathBuf::from("out.mp4")),
            mux_options: MuxOptions {
                video_codec: "copy".into(),
                audio_codec: "aac".into(),
                audio_bitrate: None,
            },
            mux_audio_bitrate_policy: MuxAudioBitratePolicy::MatchMin,
        };

        repair
            .write_outputs(&result, &output, &muxer)
            .expect("write_outputs should succeed");

        assert_eq!(writer.writes(), 0);
        assert_eq!(muxer.muxes(), 0);
        assert!(progress.saw_skip_message());
    }

    #[test]
    fn patch_summary_has_patches() {
        let empty = PatchSummary::from_outcomes(vec![]);
        assert!(!empty.has_patches());

        let patched = PatchSummary::from_outcomes(vec![GapPatchOutcome::with_tags_from_status(
            0.0,
            1.0,
            GapPatchStatus::Patched {
                pre_correlation: 1.0,
                post_correlation: 1.0,
                align_adjustment_secs: 0.0,
                waveform_adjustment_secs: 0.0,
                structure_trusted: false,
                confidence: crate::domain::FillConfidence::High,
                gap_start_adjust_frames: 0,
                gap_end_adjust_frames: 0,
                residual_db: None,
                floor_db: None,
                headroom_db: None,
                anchor_seam_used: false,
                anchor_bracket_move_frames: 0,
            },
            FillMode::Fit,
            FillTierThresholds::DEFAULT,
        )]);
        assert!(patched.has_patches());
    }

    #[test]
    fn repair_file_output_wants_file_output() {
        assert!(!file_output(None).wants_file_output());
        assert!(file_output(Some(PathBuf::from("out.wav"))).wants_file_output());

        #[cfg(feature = "ffmpeg-mux")]
        {
            let mux_only = RepairFileOutput {
                source_video: PathBuf::from("a.mp4"),
                wav_path: None,
                video_path: Some(PathBuf::from("out.mp4")),
                mux_options: MuxOptions {
                    video_codec: "copy".into(),
                    audio_codec: "aac".into(),
                    audio_bitrate: None,
                },
                mux_audio_bitrate_policy: MuxAudioBitratePolicy::MatchMin,
            };
            assert!(mux_only.wants_file_output());
        }
    }
}
