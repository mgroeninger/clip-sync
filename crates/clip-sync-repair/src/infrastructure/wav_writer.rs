use std::io;
use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};

use clip_sync::{MultiChannelPcm, WavBitDepth, resolve_output_bit_depth};

use crate::application::error::RepairError;
use crate::application::ports::PatchedAudioWriter;
use crate::infrastructure::pcm::{f32_to_i24, validate_pcm_for_wav};

pub struct WavPatchedAudioWriter;

impl PatchedAudioWriter for WavPatchedAudioWriter {
    fn write(&self, audio: &MultiChannelPcm, path: &Path) -> Result<(), RepairError> {
        let depth = resolve_output_bit_depth(audio.source_bit_depth);
        validate_pcm_for_wav(audio, depth)?;

        let spec = WavSpec {
            channels: audio.channels,
            sample_rate: audio.sample_rate,
            bits_per_sample: match depth {
                WavBitDepth::Int16 => 16,
                WavBitDepth::Int24 => 24,
            },
            sample_format: SampleFormat::Int,
        };

        let mut writer = WavWriter::create(path, spec).map_err(|e| {
            RepairError::Write(io::Error::other(format!("{}: {}", path.display(), e)))
        })?;

        let write_err = |e: hound::Error| {
            RepairError::Write(io::Error::other(format!("{}: {}", path.display(), e)))
        };

        match depth {
            WavBitDepth::Int16 => {
                for &s in &audio.samples {
                    let v = (s * 32767.0).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                    writer.write_sample(v).map_err(write_err)?;
                }
            }
            WavBitDepth::Int24 => {
                for &s in &audio.samples {
                    writer.write_sample(f32_to_i24(s)).map_err(write_err)?;
                }
            }
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
