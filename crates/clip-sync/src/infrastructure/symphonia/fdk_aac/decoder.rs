use std::fmt;

extern crate symphonia_core;

use fdk_aac::dec::{Decoder, DecoderError, Transport};
use symphonia::core::audio::{
    AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, GenericAudioBufferRef,
};
use symphonia::core::codecs::CodecInfo;
use symphonia::core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia::core::codecs::audio::well_known::profiles::{
    CODEC_PROFILE_AAC_HE, CODEC_PROFILE_AAC_HE_V2,
};
use symphonia::core::codecs::audio::{
    AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
};
use symphonia::core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia::core::errors::{Error, unsupported_error};
use symphonia::core::packet::PacketRef;
use symphonia::core::{codec_profile, support_audio_codec};
use tracing::warn;

use super::adts::construct_adts_header;
use super::meta::{M4AInfo, M4AType, m4a_type_from_index, map_to_channels, sample_rate_index};

type Result<T> = symphonia::core::errors::Result<T>;

/// Interleaved PCM capacity for one decoded AAC frame (8 channels × 2048 samples).
const MAX_SAMPLES: usize = 16_384;

macro_rules! validate {
    ($a:expr) => {
        if !$a {
            return symphonia::core::errors::decode_error("aac: invalid data");
        }
    };
}

/// Symphonia-compatible FDK AAC decoder with multichannel HE-AAC support.
pub struct AacDecoder {
    decoder: Decoder,
    buf: AudioBuffer<i16>,
    codec_params: AudioCodecParameters,
    m4a_info: M4AInfo,
    m4a_info_validated: bool,
    pcm: [i16; MAX_SAMPLES],
    /// Reusable ADTS header + access-unit payload for [`Decoder::fill`].
    /// FDK requires one contiguous `&[u8]`; capacity is kept across packets (and
    /// across `reset()`), so steady-state decode avoids per-packet heap allocs.
    adts_scratch: Vec<u8>,
}

impl fmt::Debug for AacDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AacDecoder")
            .field("codec_params", &self.codec_params)
            .field("m4a_info", &self.m4a_info)
            .field("m4a_info_validated", &self.m4a_info_validated)
            .finish_non_exhaustive()
    }
}

impl AacDecoder {
    fn try_new(params: &AudioCodecParameters, _opts: &AudioDecoderOptions) -> Result<Self> {
        let mut m4a_info = M4AInfo::default();
        if let Some(extra_data_buf) = &params.extra_data {
            validate!(extra_data_buf.len() >= 2);
            m4a_info.read(extra_data_buf)?;
            // ASC channelConfiguration 0 means layout is in the PCE, not the header.
            // ffmpeg often muxes 5.1 this way; Symphonia still provides channel count from the container.
            if m4a_info.channels == 0 {
                // ASC channelConfiguration 0 = layout carried in the PCE inside the bitstream.
                // Symphonia often leaves `params.channels` unset for this case; FDK learns the
                // true layout from the first access unit in `configure_metadata`.
                m4a_info.channels = params
                    .channels
                    .as_ref()
                    .map(|channels| channels.count() as u8)
                    .unwrap_or(1);
            }
        } else {
            m4a_info.otype = M4AType::Lc;
            m4a_info.sample_rate = params.sample_rate.unwrap_or_default();
            m4a_info.sample_rate_index = sample_rate_index(m4a_info.sample_rate).ok_or(
                Error::DecodeError("aac: sample rate has no ADTS table index"),
            )?;

            m4a_info.channels = if let Some(channels) = &params.channels {
                channels.count() as u8
            } else {
                return unsupported_error("aac: channels or channel layout is required");
            };
        }
        let decoder = Decoder::new(Transport::Adts);

        let buf = audio_buffer(&m4a_info, m4a_info.sample_rate)?;
        Ok(Self {
            decoder,
            codec_params: params.clone(),
            buf,
            m4a_info,
            m4a_info_validated: false,
            pcm: [0; MAX_SAMPLES],
            adts_scratch: Vec::new(),
        })
    }

    fn configure_metadata(&mut self) -> Result<()> {
        let stream_info = self.decoder.stream_info();
        let capacity = self.decoder.decoded_frame_size();
        let channels = stream_info.numChannels as u8;
        let sample_rate = stream_info.aacSampleRate as u32;
        let sample_rate_index = sample_rate_index(sample_rate).ok_or(Error::DecodeError(
            "aac: sample rate has no ADTS table index",
        ))?;

        self.m4a_info = M4AInfo {
            otype: m4a_type_from_index(stream_info.aot as usize),
            channels,
            sample_rate,
            sample_rate_index,
            samples: capacity / channels.max(1) as usize,
        };

        self.buf = audio_buffer(&self.m4a_info, stream_info.sampleRate as u32)?;
        self.m4a_info_validated = true;

        Ok(())
    }
}

fn audio_buffer(m4a_info: &M4AInfo, sample_rate: u32) -> Result<AudioBuffer<i16>> {
    let Some(channels) = map_to_channels(m4a_info.channels) else {
        return unsupported_error("aac: unsupported number of channels");
    };
    Ok(AudioBuffer::new(
        AudioSpec::new(sample_rate, channels),
        m4a_info.samples,
    ))
}

impl AudioDecoder for AacDecoder {
    fn reset(&mut self) {
        // `fdk-aac` 0.8 exposes no flush/clear on `Decoder`, so recreate it to drop
        // all SBR/overlap carry-over. Symphonia calls `reset()` after seeks; a no-op
        // here would let pre-seek state contaminate the first post-seek frame(s).
        // `m4a_info` is re-validated from the first decoded frame after reset.
        self.decoder = Decoder::new(Transport::Adts);
        self.m4a_info_validated = false;
    }

    fn codec_info(&self) -> &CodecInfo {
        &Self::supported_codecs()
            .first()
            .expect("missing codecs")
            .info
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        &self.codec_params
    }

    fn decode_ref(&mut self, packet: &PacketRef) -> Result<GenericAudioBufferRef<'_>> {
        let mut reader = packet.as_buf_reader();
        let payload = reader.read_buf_bytes_available_ref();
        let adts_header = construct_adts_header(
            self.m4a_info.otype,
            self.m4a_info.sample_rate_index,
            self.m4a_info.channels,
            payload.len() as u64,
        )?;

        self.adts_scratch.clear();
        self.adts_scratch.reserve(adts_header.len() + payload.len());
        self.adts_scratch.extend_from_slice(&adts_header);
        self.adts_scratch.extend_from_slice(payload);

        self.decoder
            .fill(&self.adts_scratch)
            .map_err(|e| Error::DecodeError(e.message()))?;

        match self.decoder.decode_frame(&mut self.pcm) {
            Ok(_) => {}
            Err(e @ DecoderError::TRANSPORT_SYNC_ERROR) => {
                warn!(error = e.message(), "aac: transport sync error");
                self.buf.clear();
                return Ok(self.buf.as_generic_audio_buffer_ref());
            }
            Err(e) => {
                return Err(Error::DecodeError(e.message()));
            }
        }
        if !self.m4a_info_validated {
            self.configure_metadata()?;
        }

        let capacity = self.decoder.decoded_frame_size();
        if capacity > MAX_SAMPLES {
            return Err(Error::DecodeError(
                "aac: decoded frame exceeds internal buffer capacity",
            ));
        }
        let pcm = &self.pcm[..capacity];
        self.buf.clear();

        self.buf.render_uninit(None);
        self.buf.copy_from_slice_interleaved(&pcm);
        self.buf.trim(
            packet.trim_start.get() as usize,
            packet.trim_end.get() as usize,
        );

        Ok(self.buf.as_generic_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        self.buf.as_generic_audio_buffer_ref()
    }
}

impl RegisterableAudioDecoder for AacDecoder {
    fn try_registry_new(
        params: &AudioCodecParameters,
        opts: &AudioDecoderOptions,
    ) -> symphonia::core::errors::Result<Box<dyn AudioDecoder>>
    where
        Self: Sized,
    {
        Ok(Box::new(AacDecoder::try_new(params, opts)?))
    }

    fn supported_codecs() -> &'static [SupportedAudioCodec] {
        &[support_audio_codec!(
            CODEC_ID_AAC,
            "aac",
            "Advanced Audio Coding",
            &[
                codec_profile!(CODEC_PROFILE_AAC_HE, "aac-he", "High Efficiency"),
                codec_profile!(CODEC_PROFILE_AAC_HE_V2, "aac-he-v2", "High Efficiency V2"),
            ]
        )]
    }
}
