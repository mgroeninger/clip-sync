use std::io;
use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};

use clip_sync::MultiChannelPcm;

use crate::application::error::RepairError;
use crate::application::ports::PatchedAudioWriter;
use crate::infrastructure::pcm::validate_pcm_for_wav;

pub struct WavPatchedAudioWriter;

impl PatchedAudioWriter for WavPatchedAudioWriter {
    fn write(&self, audio: &MultiChannelPcm, path: &Path) -> Result<(), RepairError> {
        validate_pcm_for_wav(audio)?;

        let spec = WavSpec {
            channels: audio.channels,
            sample_rate: audio.sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };

        let mut writer = WavWriter::create(path, spec).map_err(|e| {
            RepairError::Write(io::Error::other(format!("{}: {}", path.display(), e)))
        })?;

        for &s in &audio.samples {
            let sample = (s * 32767.0).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            writer.write_sample(sample).map_err(|e| {
                RepairError::Write(io::Error::other(format!("{}: {}", path.display(), e)))
            })?;
        }

        writer.finalize().map_err(|e| {
            let message = e.to_string();
            if message.contains("not a multiple of the number of channels") {
                RepairError::Write(io::Error::other(format!(
                    "{}: WAV finalize failed ({message}); this often indicates patched audio \
                     exceeds the classic WAV 4 GiB limit — use --mux instead",
                    path.display()
                )))
            } else {
                RepairError::Write(io::Error::other(format!("{}: {}", path.display(), e)))
            }
        })?;

        Ok(())
    }
}
