//! Map clip-sync PCM types to domain [`InterleavedSamples`].

use clip_sync::MultiChannelPcm;

use crate::domain::pcm::InterleavedSamples;

impl InterleavedSamples for MultiChannelPcm {
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
