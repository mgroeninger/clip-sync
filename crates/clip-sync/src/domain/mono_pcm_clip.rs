#[derive(Debug, Clone, PartialEq)]
pub struct MonoPcmClip {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
    /// Corrupt packets skipped during decode (`0` for synthetic / unknown sources).
    pub decode_error_skips: u32,
    /// Samples decoded before end-of-window silence padding, when padding was applied.
    pub decoded_sample_count: Option<usize>,
}

impl MonoPcmClip {
    pub fn effective_decoded_sample_count(&self) -> usize {
        self.decoded_sample_count.unwrap_or(self.samples.len())
    }

    pub fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_pcm_clip_duration_secs() {
        let clip = MonoPcmClip {
            sample_rate: 11_025,
            samples: vec![0_i16; 11_025 * 30],
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        assert!((clip.duration_secs() - 30.0).abs() < 1e-9);
    }
}

/// One fixed-duration bucket from a sequential mono timeline scan.
#[derive(Debug, Clone, PartialEq)]
pub struct MonoScanBucket {
    pub start_secs: f64,
    pub end_secs: f64,
    pub pcm: MonoPcmClip,
}
