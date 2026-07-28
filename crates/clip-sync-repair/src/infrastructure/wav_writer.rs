use std::io;
use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};

use clip_sync::{resolve_output_bit_depth, MultiChannelPcm, WavBitDepth};

use crate::application::error::RepairError;
use crate::application::ports::PatchedAudioWriter;
use crate::infrastructure::pcm::{f32_to_i24, validate_pcm_for_wav};

pub struct WavPatchedAudioWriter;

impl PatchedAudioWriter for WavPatchedAudioWriter {
    fn write(&self, audio: &MultiChannelPcm, path: &Path) -> Result<(), RepairError> {
        let depth = resolve_output_bit_depth(audio.source_bit_depth);
        validate_pcm_for_wav(audio, depth)?;

        tracing::debug!(
            path = %path.display(),
            bits_per_sample = match depth { WavBitDepth::Int16 => 16u32, WavBitDepth::Int24 => 24 },
            sample_rate = audio.sample_rate,
            channels = audio.channels,
            frames = audio.frames(),
            "writing patched WAV"
        );

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
                    let v = (s * 32767.0)
                        .round()
                        .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                    writer.write_sample(v).map_err(write_err)?;
                }
            }
            WavBitDepth::Int24 => {
                for &s in &audio.samples {
                    writer.write_sample(f32_to_i24(s)).map_err(write_err)?;
                }
            }
        }

        // No 4 GiB special case here: `validate_pcm_for_wav` above already rejects both of the
        // conditions that could reach it (`samples.len() % channels != 0`, and a data chunk over
        // `u32::MAX` bytes), and does so with a message that names the real limit.
        writer.finalize().map_err(write_err)?;

        Ok(())
    }
}
