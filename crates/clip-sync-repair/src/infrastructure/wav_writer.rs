use std::io;
use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};

use clip_sync::MultiChannelPcm;

use crate::application::error::RepairError;
use crate::application::ports::PatchedAudioWriter;

pub struct WavPatchedAudioWriter;

impl PatchedAudioWriter for WavPatchedAudioWriter {
    fn write(&self, audio: &MultiChannelPcm, path: &Path) -> Result<(), RepairError> {
        let spec = WavSpec {
            channels: audio.channels,
            sample_rate: audio.sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };

        let mut writer = WavWriter::create(path, spec).map_err(|e| {
            RepairError::Write(io::Error::new(
                io::ErrorKind::Other,
                format!("{}: {}", path.display(), e),
            ))
        })?;

        for &sample in &audio.samples {
            writer.write_sample(sample).map_err(|e| {
                RepairError::Write(io::Error::new(
                    io::ErrorKind::Other,
                    format!("{}: {}", path.display(), e),
                ))
            })?;
        }

        writer.finalize().map_err(|e| {
            RepairError::Write(io::Error::new(
                io::ErrorKind::Other,
                format!("{}: {}", path.display(), e),
            ))
        })?;

        Ok(())
    }
}
