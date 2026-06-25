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

/// Output WAV bit depth — 16-bit int or 24-bit int (32-bit float output is out of scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavBitDepth {
    Int16,
    Int24,
}

impl WavBitDepth {
    /// Bytes per sample on disk.
    pub fn bytes_per_sample(self) -> u64 {
        match self {
            Self::Int16 => 2,
            Self::Int24 => 3,
        }
    }

    /// ffmpeg raw PCM format string for `-f`.
    pub fn ffmpeg_format(self) -> &'static str {
        match self {
            Self::Int16 => "s16le",
            Self::Int24 => "s24le",
        }
    }
}

/// Resolve the output WAV bit depth from the source track's detected depth.
///
/// Rule: sources with more than 16 bits of precision (Int24, Int32, Float32, or Other(>16))
/// map to 24-bit int output; everything else (Int16, None, Other(≤16)) keeps the default
/// 16-bit int output so lossy-codec sources (no detectable depth) remain unchanged.
pub fn resolve_output_bit_depth(source: Option<BitDepth>) -> WavBitDepth {
    match source {
        Some(BitDepth::Int24) | Some(BitDepth::Int32) | Some(BitDepth::Float32) => {
            WavBitDepth::Int24
        }
        Some(BitDepth::Other(bits)) if bits > 16 => WavBitDepth::Int24,
        _ => WavBitDepth::Int16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_output_depth_lossless_hi_depth_to_int24() {
        assert_eq!(resolve_output_bit_depth(Some(BitDepth::Int24)), WavBitDepth::Int24);
        assert_eq!(resolve_output_bit_depth(Some(BitDepth::Int32)), WavBitDepth::Int24);
        assert_eq!(resolve_output_bit_depth(Some(BitDepth::Float32)), WavBitDepth::Int24);
        assert_eq!(resolve_output_bit_depth(Some(BitDepth::Other(20))), WavBitDepth::Int24);
        assert_eq!(resolve_output_bit_depth(Some(BitDepth::Other(32))), WavBitDepth::Int24);
    }

    #[test]
    fn resolve_output_depth_lossy_and_16bit_to_int16() {
        assert_eq!(resolve_output_bit_depth(None), WavBitDepth::Int16);
        assert_eq!(resolve_output_bit_depth(Some(BitDepth::Int16)), WavBitDepth::Int16);
        assert_eq!(resolve_output_bit_depth(Some(BitDepth::Other(8))), WavBitDepth::Int16);
        assert_eq!(resolve_output_bit_depth(Some(BitDepth::Other(16))), WavBitDepth::Int16);
    }
}
