use symphonia::core::audio::sample::SampleFormat;
use symphonia::core::codecs::audio::AudioCodecParameters;

/// Source bit depth / sample format as reported by the container at probe time.
///
/// Carried on [`AudioTrack`](super::AudioTrack) and forwarded into
/// [`MultiChannelPcm`](super::MultiChannelPcm) so the WAV writer can choose the right output
/// depth without a second probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitDepth {
    Int16,
    Int24,
    Int32,
    Float32,
    /// Any other depth reported by the codec (e.g. 8-bit, 20-bit).
    Other(u32),
}

impl BitDepth {
    /// Derive from Symphonia `AudioCodecParameters`. Returns `None` when neither
    /// `bits_per_sample` nor `sample_format` is present (typical for lossy codecs).
    pub fn from_codec_params(params: &AudioCodecParameters) -> Option<Self> {
        match (params.sample_format, params.bits_per_sample) {
            (Some(SampleFormat::F32), _) => Some(Self::Float32),
            (Some(SampleFormat::S32), _) | (_, Some(32)) => Some(Self::Int32),
            (Some(SampleFormat::S24), _) | (_, Some(24)) => Some(Self::Int24),
            (Some(SampleFormat::S16), _) | (_, Some(16)) => Some(Self::Int16),
            (_, Some(bits)) => Some(Self::Other(bits)),
            _ => None,
        }
    }
}
