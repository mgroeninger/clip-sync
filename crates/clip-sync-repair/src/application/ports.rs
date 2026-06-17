use crate::application::error::RepairError;
use crate::domain::GapReport;

/// Output port: formats and emits a gap report (human or JSON).
pub trait GapReporter {
    fn report(&self, report: &GapReport) -> Result<(), RepairError>;
}

/// Input port: aligns two video files to find their temporal offset.
pub trait Aligner {
    fn align(
        &self,
        request: clip_sync::AlignVideosRequest,
        progress: &dyn clip_sync::ProgressReporter,
    ) -> Result<clip_sync::AlignmentResult, clip_sync::AppError>;
}

/// Output port: writes patched audio to a file (e.g. WAV).
pub trait PatchedAudioWriter {
    fn write(
        &self,
        audio: &clip_sync::MultiChannelPcm,
        path: &std::path::Path,
    ) -> Result<(), RepairError>;
}

#[cfg(feature = "ffmpeg-mux")]
/// Options passed to [`MediaMuxer::mux_video_with_replaced_audio`].
#[derive(Debug, Clone)]
pub struct MuxOptions {
    pub video_codec: String,
    pub audio_codec: String,
    /// ffmpeg `-b:a` value (e.g. `247k`). `None` omits the flag (ffmpeg encoder default).
    pub audio_bitrate: Option<String>,
}

#[cfg(feature = "ffmpeg-mux")]
/// Output port: muxes replacement audio into a video container (`ffmpeg-mux` feature).
pub trait MediaMuxer {
    fn mux_video_with_replaced_audio(
        &self,
        source_video: &std::path::Path,
        replacement_audio: &clip_sync::MultiChannelPcm,
        output: &std::path::Path,
        options: &MuxOptions,
        progress: &dyn clip_sync::ProgressReporter,
    ) -> Result<(), RepairError>;
}
