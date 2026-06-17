/// Native-rate, all-channels PCM for a single window — the repair fill path counterpart to
/// [`MonoPcmClip`](crate::domain::MonoPcmClip), which downmixes to mono at the fingerprint rate.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiChannelPcm {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved `i16` frames: `samples.len() == frames * channels`.
    pub samples: Vec<i16>,
    /// Corrupt packets skipped during decode (`0` for synthetic / unknown sources).
    pub decode_error_skips: u32,
    /// Frames decoded before end-of-window silence padding, when padding was applied.
    pub decoded_frame_count: Option<usize>,
    /// Compressed codec bytes read for this window (`None` when not measured, e.g. scan buckets).
    pub compressed_bytes: Option<u64>,
}

/// One fixed-duration bucket from a sequential interleaved timeline scan.
#[derive(Debug, Clone, PartialEq)]
pub struct InterleavedScanBucket {
    pub start_secs: f64,
    pub end_secs: f64,
    pub pcm: MultiChannelPcm,
}

impl MultiChannelPcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate.max(1))
    }

    pub fn effective_decoded_frame_count(&self) -> usize {
        self.decoded_frame_count.unwrap_or_else(|| self.frames())
    }

    /// Average encoded bitrate (bits/s) from [`compressed_bytes`](Self::compressed_bytes) and
    /// [`duration_secs`](Self::duration_secs).
    pub fn measured_bitrate_bps(&self) -> Option<u32> {
        let bytes = self.compressed_bytes?;
        let duration_secs = self.duration_secs();
        if bytes == 0 || !duration_secs.is_finite() || duration_secs <= 0.0 {
            return None;
        }
        Some(((bytes as f64) * 8.0 / duration_secs).round() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::MultiChannelPcm;

    #[test]
    fn measured_bitrate_from_compressed_bytes_and_duration() {
        let pcm = MultiChannelPcm {
            sample_rate: 48_000,
            channels: 2,
            samples: vec![0; 48_000 * 2],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: Some(31_000),
        };
        assert_eq!(pcm.measured_bitrate_bps(), Some(248_000));
    }
}
