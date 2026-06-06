use tracing::debug;

use crate::application::config::ChromaprintPreset;
use crate::application::error::FingerprintError;
use crate::application::ports::Fingerprinter;
use crate::domain::{Fingerprint, MonoPcmClip};
use crate::infrastructure::chromaprint::config::configuration_for_preset;

use rusty_chromaprint::{Fingerprinter as ChromaprintEngine, ResetError};

const MONO_CHANNELS: u32 = 1;
const MIN_SAMPLE_RATE: u32 = 1_001;

#[derive(Debug, Clone, Copy)]
pub struct ChromaprintFingerprinter {
    preset: ChromaprintPreset,
}

impl ChromaprintFingerprinter {
    pub fn new(preset: ChromaprintPreset) -> Self {
        Self { preset }
    }
}

impl Default for ChromaprintFingerprinter {
    fn default() -> Self {
        Self::new(ChromaprintPreset::default())
    }
}

impl Fingerprinter for ChromaprintFingerprinter {
    fn fingerprint(&self, clip: &MonoPcmClip) -> Result<Fingerprint, FingerprintError> {
        fingerprint_clip(clip, self.preset)
    }
}

fn fingerprint_clip(
    clip: &MonoPcmClip,
    preset: ChromaprintPreset,
) -> Result<Fingerprint, FingerprintError> {
    validate_clip(clip)?;

    let config = configuration_for_preset(preset);
    let mut fingerprinter = ChromaprintEngine::new(&config);
    fingerprinter
        .start(clip.sample_rate, MONO_CHANNELS)
        .map_err(map_reset_error)?;

    fingerprinter.consume(&clip.samples);
    fingerprinter.finish();

    let data = fingerprinter.fingerprint().to_vec();
    if data.is_empty() {
        return Err(FingerprintError::InvalidPcm(
            "fingerprint produced no items".into(),
        ));
    }

    debug!(
        sample_rate = clip.sample_rate,
        samples = clip.samples.len(),
        fingerprint_items = data.len(),
        "fingerprint complete"
    );

    Ok(Fingerprint { data })
}

fn validate_clip(clip: &MonoPcmClip) -> Result<(), FingerprintError> {
    if clip.samples.is_empty() {
        return Err(FingerprintError::InvalidPcm("empty clip".into()));
    }

    if clip.sample_rate < MIN_SAMPLE_RATE {
        return Err(FingerprintError::InvalidPcm(format!(
            "sample rate {} Hz is below minimum {MIN_SAMPLE_RATE} Hz",
            clip.sample_rate
        )));
    }

    Ok(())
}

fn map_reset_error(error: ResetError) -> FingerprintError {
    match error {
        ResetError::SampleRateTooLow => FingerprintError::InvalidPcm(format!(
            "sample rate must be greater than {MIN_SAMPLE_RATE} Hz"
        )),
        ResetError::NoChannels => {
            FingerprintError::InvalidPcm("channel count must be at least 1".into())
        }
        ResetError::CannotResample(detail) => FingerprintError::EngineFailed(format!(
            "failed to configure resampler: {detail}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    fn tone_clip(sample_rate: u32, seconds: u32) -> MonoPcmClip {
        let count = sample_rate as usize * seconds as usize;
        let samples: Vec<i16> = (0..count)
            .map(|index| {
                let t = index as f32 / sample_rate as f32;
                ((TAU * 440.0 * t).sin() * (i16::MAX as f32 * 0.5)).round() as i16
            })
            .collect();
        MonoPcmClip {
            sample_rate,
            samples,
        }
    }

    #[test]
    fn rejects_empty_clip() {
        let clip = MonoPcmClip::new(44_100, vec![]);
        match fingerprint_clip(&clip, ChromaprintPreset::default()) {
            Err(FingerprintError::InvalidPcm(_)) => {}
            other => panic!("expected InvalidPcm, got {other:?}"),
        }
    }

    #[test]
    fn rejects_low_sample_rate() {
        let clip = MonoPcmClip::new(1_000, vec![0; 2_000]);
        match fingerprint_clip(&clip, ChromaprintPreset::default()) {
            Err(FingerprintError::InvalidPcm(_)) => {}
            other => panic!("expected InvalidPcm, got {other:?}"),
        }
    }

    #[test]
    fn fingerprints_tone_clip() {
        let clip = tone_clip(44_100, 10);
        let fingerprint = fingerprint_clip(&clip, ChromaprintPreset::default()).unwrap();
        assert!(!fingerprint.data.is_empty());
    }
}
