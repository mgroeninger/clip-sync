use std::path::PathBuf;

use clip_sync::{MediaReader, ProgressReporter};

use crate::application::error::RepairError;
use crate::application::patch_audio::{PatchAudio, PatchAudioRequest, PatchAudioResult};
use crate::application::ports::PatchedAudioWriter;
#[cfg(feature = "ffmpeg-mux")]
use crate::application::ports::{MediaMuxer, MuxOptions};

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
        let patch_result = PatchAudio::new(self.media_reader, self.progress)
            .execute(request.patch_request, request.crossfade_ms)?;

        if let Some(wav_path) = request.wav_path.as_ref() {
            let pcm = patch_result
                .pcm
                .as_ref()
                .expect("patched run should include decoded A PCM");
            self.wav_writer.write(pcm, wav_path)?;
        }

        Ok(patch_result)
    }

    #[cfg(feature = "ffmpeg-mux")]
    pub fn execute<MM: MediaMuxer>(
        &self,
        request: RepairWriteRequest,
        muxer: &MM,
    ) -> Result<PatchAudioResult, RepairError> {
        let patch_result = PatchAudio::new(self.media_reader, self.progress)
            .execute(request.patch_request, request.crossfade_ms)?;

        if patch_result.summary.patched_count == 0 {
            if request.wav_path.is_some() || request.video_path.is_some() {
                self.progress
                    .phase("No gaps were patched; skipping WAV/mux output.");
            }
            return Ok(patch_result);
        }

        let pcm = patch_result
            .pcm
            .as_ref()
            .expect("patched run should include decoded A PCM");

        if let Some(ref wav_path) = request.wav_path {
            self.wav_writer.write(pcm, wav_path)?;
            if let Some(ref video_path) = request.video_path {
                muxer.mux_video_with_replaced_audio(
                    &request.source_video,
                    pcm,
                    video_path,
                    &request.mux_options,
                    self.progress,
                )?;
            }
            return Ok(patch_result);
        }

        if let Some(ref video_path) = request.video_path {
            muxer.mux_video_with_replaced_audio(
                &request.source_video,
                pcm,
                video_path,
                &request.mux_options,
                self.progress,
            )?;
        }

        Ok(patch_result)
    }

}
