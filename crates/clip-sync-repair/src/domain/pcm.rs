//! Native-rate interleaved PCM used by domain silence scanning.

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
