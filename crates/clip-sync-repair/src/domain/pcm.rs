//! Native-rate interleaved PCM used by domain silence scanning, plus the interleaved→`f64`
//! downmix converters every analysis path starts from.

/// Borrowed view of interleaved PCM for silence-run scanning.
pub trait InterleavedSamples {
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> u16;
    fn samples(&self) -> &[f32];

    fn frames(&self) -> usize {
        self.samples().len() / self.channels().max(1) as usize
    }
}

/// Owned interleaved PCM (tests and fixtures).
#[derive(Debug, Clone, PartialEq)]
pub struct InterleavedPcm {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved `f32` frames in `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
}

impl InterleavedSamples for InterleavedPcm {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn samples(&self) -> &[f32] {
        &self.samples
    }
}

/// Downmix interleaved f32 PCM to mono `f64` (channel average).
pub fn interleaved_to_mono(samples: &[f32], channels: usize) -> Vec<f64> {
    let channels = channels.max(1);
    samples
        .chunks(channels)
        .map(|frame| frame.iter().map(|&s| s as f64).sum::<f64>() / channels as f64)
        .collect()
}

/// Downmix interleaved PCM to one `f64` vector per channel.
pub fn interleaved_to_channels(samples: &[f32], channels: usize) -> Vec<Vec<f64>> {
    let channels = channels.max(1);
    (0..channels)
        .map(|ch| {
            samples
                .chunks(channels)
                .map(|frame| frame[ch] as f64)
                .collect()
        })
        .collect()
}
