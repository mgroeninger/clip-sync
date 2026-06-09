use crate::application::error::RepairError;
use crate::domain::GapReport;

/// Output port: formats and emits a gap report (human or JSON).
pub trait GapReporter {
    fn report(&self, report: &GapReport) -> Result<(), RepairError>;
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
}

#[cfg(feature = "ffmpeg-mux")]
/// Output port: muxes replacement audio into a video container (R5, `ffmpeg-mux` feature).
pub trait MediaMuxer {
    fn mux_video_with_replaced_audio(
        &self,
        source_video: &std::path::Path,
        replacement_audio_wav: &std::path::Path,
        output: &std::path::Path,
        options: &MuxOptions,
        progress: &dyn clip_sync::ProgressReporter,
    ) -> Result<(), RepairError>;
}
