use std::io::{self, Write};

use clip_sync::MultiChannelPcm;

use crate::application::error::RepairError;

/// Classic RIFF WAVE data chunk size limit (`hound` tracks payload bytes in `u32`).
pub const MAX_CLASSIC_WAV_DATA_BYTES: u64 = u32::MAX as u64;

const PCM_WRITE_CHUNK_BYTES: usize = 8 * 1024 * 1024;

pub fn pcm_data_bytes(audio: &MultiChannelPcm) -> u64 {
    audio.samples.len() as u64 * 2
}

/// Ensures interleaved `i16` samples form complete frames.
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

/// Rejects outputs that cannot be represented in a classic WAV file.
pub fn validate_pcm_for_wav(audio: &MultiChannelPcm) -> Result<(), RepairError> {
    validate_pcm_layout(audio)?;
    let bytes = pcm_data_bytes(audio);
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

/// Writes interleaved signed 16-bit little-endian PCM.
pub fn write_pcm_s16le<W: Write>(writer: &mut W, samples: &[i16]) -> io::Result<()> {
    if samples.is_empty() {
        return Ok(());
    }

    #[cfg(target_endian = "little")]
    {
        let bytes = unsafe {
            std::slice::from_raw_parts(
                samples.as_ptr().cast::<u8>(),
                samples
                    .len()
                    .checked_mul(2)
                    .expect("PCM byte length overflow"),
            )
        };
        for chunk in bytes.chunks(PCM_WRITE_CHUNK_BYTES) {
            writer.write_all(chunk)?;
        }
        Ok(())
    }

    #[cfg(not(target_endian = "little"))]
    {
        for chunk in samples.chunks(PCM_WRITE_CHUNK_BYTES / 2) {
            let mut buf = Vec::with_capacity(chunk.len() * 2);
            for &sample in chunk {
                buf.extend_from_slice(&sample.to_le_bytes());
            }
            writer.write_all(&buf)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clip_sync::MultiChannelPcm;

    fn pcm(channels: u16, sample_count: usize) -> MultiChannelPcm {
        MultiChannelPcm {
            sample_rate: 48_000,
            channels,
            samples: vec![0i16; sample_count],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
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
    fn validate_pcm_for_wav_rejects_payload_over_limit() {
        let sample_count = (MAX_CLASSIC_WAV_DATA_BYTES / 2 + 1) as usize;
        let audio = pcm(1, sample_count);
        let err = validate_pcm_for_wav(&audio).expect_err("over limit");
        assert!(
            err.to_string().contains("classic WAV limit"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn write_pcm_s16le_round_trips_bytes() {
        let samples = vec![0i16, 1i16, -1i16, i16::MAX];
        let mut buf = Vec::new();
        write_pcm_s16le(&mut buf, &samples).expect("write");
        assert_eq!(buf.len(), samples.len() * 2);
        assert_eq!(buf[0..2], 0i16.to_le_bytes());
        assert_eq!(buf[2..4], 1i16.to_le_bytes());
    }
}
