use std::io::{self, Write};

use clip_sync::{MultiChannelPcm, WavBitDepth};

use crate::application::error::RepairError;

/// Classic RIFF WAVE data chunk size limit (`hound` tracks payload bytes in `u32`).
pub const MAX_CLASSIC_WAV_DATA_BYTES: u64 = u32::MAX as u64;

const PCM_WRITE_CHUNK_BYTES: usize = 8 * 1024 * 1024;

/// Total on-disk bytes for `audio` at the given output depth.
pub fn pcm_data_bytes(audio: &MultiChannelPcm, depth: WavBitDepth) -> u64 {
    audio.samples.len() as u64 * depth.bytes_per_sample()
}

/// Ensures interleaved `f32` samples form complete frames.
pub fn validate_pcm_layout(audio: &MultiChannelPcm) -> Result<(), RepairError> {
    let channels = audio.channels.max(1) as usize;
    let remainder = audio.samples.len() % channels;
    if remainder != 0 {
        return Err(RepairError::Write(io::Error::other(format!(
            "PCM sample count ({}) is not a multiple of channel count ({channels})",
            audio.samples.len()
        ))));
    }
    Ok(())
}

/// Rejects outputs that cannot be represented in a classic WAV file at the given depth.
pub fn validate_pcm_for_wav(
    audio: &MultiChannelPcm,
    depth: WavBitDepth,
) -> Result<(), RepairError> {
    validate_pcm_layout(audio)?;
    let bytes = pcm_data_bytes(audio, depth);
    if bytes > MAX_CLASSIC_WAV_DATA_BYTES {
        let gib = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let limit_gib = MAX_CLASSIC_WAV_DATA_BYTES as f64 / (1024.0 * 1024.0 * 1024.0);
        return Err(RepairError::Write(io::Error::other(format!(
            "patched audio is {gib:.2} GiB of PCM data, which exceeds the {limit_gib:.2} GiB \
             classic WAV limit; use --mux for long surround outputs instead of --wav"
        ))));
    }
    Ok(())
}

/// Convert a normalized `f32` sample to a true 24-bit signed integer (`i32` in range
/// `[-8_388_608, 8_388_607]`). `hound` requires this range for `bits_per_sample: 24`.
#[inline]
pub fn f32_to_i24(s: f32) -> i32 {
    (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32
}

/// Writes `f32` samples as little-endian PCM at the requested bit depth.
pub fn write_pcm_le<W: Write>(
    writer: &mut W,
    samples: &[f32],
    depth: WavBitDepth,
) -> io::Result<()> {
    match depth {
        WavBitDepth::Int16 => write_pcm_s16le(writer, samples),
        WavBitDepth::Int24 => write_pcm_s24le(writer, samples),
    }
}

fn write_pcm_s16le<W: Write>(writer: &mut W, samples: &[f32]) -> io::Result<()> {
    for chunk in samples.chunks(PCM_WRITE_CHUNK_BYTES / 2) {
        let mut buf = Vec::with_capacity(chunk.len() * 2);
        for &s in chunk {
            let v = (s * 32767.0)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            buf.extend_from_slice(&v.to_le_bytes());
        }
        writer.write_all(&buf)?;
    }
    Ok(())
}

fn write_pcm_s24le<W: Write>(writer: &mut W, samples: &[f32]) -> io::Result<()> {
    for chunk in samples.chunks(PCM_WRITE_CHUNK_BYTES / 3) {
        let mut buf = Vec::with_capacity(chunk.len() * 3);
        for &s in chunk {
            let v = f32_to_i24(s);
            buf.extend_from_slice(&v.to_le_bytes()[..3]);
        }
        writer.write_all(&buf)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clip_sync::{MultiChannelPcm, WavBitDepth};

    fn pcm(channels: u16, sample_count: usize) -> MultiChannelPcm {
        MultiChannelPcm {
            sample_rate: 48_000,
            channels,
            samples: vec![0.0f32; sample_count],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        }
    }

    #[test]
    fn validate_pcm_layout_rejects_partial_frame() {
        let audio = pcm(6, 5);
        let err = validate_pcm_layout(&audio).expect_err("partial frame");
        assert!(
            err.to_string().contains("not a multiple"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn validate_pcm_for_wav_rejects_payload_over_limit_16bit() {
        let sample_count = (MAX_CLASSIC_WAV_DATA_BYTES / 2 + 1) as usize;
        let audio = pcm(1, sample_count);
        let err = validate_pcm_for_wav(&audio, WavBitDepth::Int16).expect_err("over limit");
        assert!(
            err.to_string().contains("classic WAV limit"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn validate_pcm_for_wav_rejects_payload_over_limit_24bit() {
        let sample_count = (MAX_CLASSIC_WAV_DATA_BYTES / 3 + 1) as usize;
        let audio = pcm(1, sample_count);
        let err = validate_pcm_for_wav(&audio, WavBitDepth::Int24).expect_err("over limit");
        assert!(
            err.to_string().contains("classic WAV limit"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn write_pcm_le_s16le_round_trips_bytes() {
        let samples = vec![0.0f32, 1.0 / 32767.0, -1.0 / 32767.0, 1.0];
        let mut buf = Vec::new();
        write_pcm_le(&mut buf, &samples, WavBitDepth::Int16).expect("write");
        assert_eq!(buf.len(), samples.len() * 2);
        assert_eq!(buf[0..2], 0i16.to_le_bytes());
        assert_eq!(buf[2..4], 1i16.to_le_bytes());
    }

    #[test]
    fn write_pcm_le_s24le_round_trips_bytes() {
        let samples = vec![0.0f32, 1.0 / 8_388_607.0, -1.0 / 8_388_607.0, 1.0];
        let mut buf = Vec::new();
        write_pcm_le(&mut buf, &samples, WavBitDepth::Int24).expect("write");
        assert_eq!(buf.len(), samples.len() * 3);
        // 0.0 → 0
        assert_eq!(buf[0..3], 0i32.to_le_bytes()[..3]);
        // 1/8_388_607 → 1
        assert_eq!(buf[3..6], 1i32.to_le_bytes()[..3]);
    }

    #[test]
    fn f32_to_i24_clamps_and_scales() {
        assert_eq!(f32_to_i24(0.0), 0);
        assert_eq!(f32_to_i24(1.0), 8_388_607);
        assert_eq!(f32_to_i24(-1.0), -8_388_607);
        assert_eq!(f32_to_i24(2.0), 8_388_607);
        assert_eq!(f32_to_i24(-2.0), -8_388_607);
    }
}
