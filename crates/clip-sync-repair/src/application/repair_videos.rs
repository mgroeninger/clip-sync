use std::path::PathBuf;

use clip_sync::{MediaReader, ProgressReporter};

use crate::application::error::RepairError;
use crate::application::patch_audio::{PatchAudio, PatchAudioRequest};
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
    pub fn execute(&self, request: RepairWriteRequest) -> Result<(), RepairError> {
        let Some(wav_path) = request.wav_path.as_ref() else {
            return Ok(());
        };

        let patched = PatchAudio::new(self.media_reader, self.progress)
            .execute(request.patch_request, request.crossfade_ms)?;
        self.wav_writer.write(&patched, wav_path)
    }

    #[cfg(feature = "ffmpeg-mux")]
    pub fn execute<MM: MediaMuxer>(
        &self,
        request: RepairWriteRequest,
        muxer: &MM,
    ) -> Result<(), RepairError> {
        let wants_wav = request.wav_path.is_some();
        let wants_mux = request.video_path.is_some();
        if !wants_wav && !wants_mux {
            return Ok(());
        }

        let patched = PatchAudio::new(self.media_reader, self.progress)
            .execute(request.patch_request, request.crossfade_ms)?;

        if let Some(ref wav_path) = request.wav_path {
            self.wav_writer.write(&patched, wav_path)?;
            if let Some(ref video_path) = request.video_path {
                muxer.mux_video_with_replaced_audio(
                    &request.source_video,
                    wav_path,
                    video_path,
                    &request.mux_options,
                )?;
            }
            return Ok(());
        }

        if let Some(ref video_path) = request.video_path {
            let temp = tempfile::NamedTempFile::new().map_err(RepairError::Io)?;
            let temp_path = temp.path();
            self.wav_writer.write(&patched, temp_path)?;
            muxer.mux_video_with_replaced_audio(
                &request.source_video,
                temp_path,
                video_path,
                &request.mux_options,
            )?;
        }

        Ok(())
    }
}
