use symphonia::core::audio::{
    AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, GenericAudioBufferRef,
};
use symphonia::core::codecs::CodecInfo;
use symphonia::core::codecs::audio::well_known::{CODEC_ID_AC3, CODEC_ID_EAC3};
use symphonia::core::codecs::audio::{
    AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
};
use symphonia::core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia::core::errors::{Error, unsupported_error};
use symphonia::core::packet::PacketRef;
use symphonia::core::support_audio_codec;

use oxideav_core::Decoder as OxideDecoder;

type Result<T> = symphonia::core::errors::Result<T>;

/// Maximum samples per channel in one AC-3 syncframe (6 blocks × 256).
/// E-AC-3 can emit up to 6× this in one container packet (independent
/// + dependent substream pairs), so we allocate 6× as the buffer cap.
const AC3_FRAME_SAMPLES: usize = 1536;
const BUF_CAPACITY: usize = AC3_FRAME_SAMPLES * 6;

pub struct Ac3Decoder {
    inner: Box<dyn OxideDecoder>,
    codec_params: AudioCodecParameters,
    buf: AudioBuffer<i16>,
}

// oxideav_core::Decoder requires only Send; all Symphonia decode methods take &mut self
// (no shared-reference access is possible), so Sync is safe to assert here.
unsafe impl Sync for Ac3Decoder {}

impl Ac3Decoder {
    fn try_new(params: &AudioCodecParameters, _opts: &AudioDecoderOptions) -> Result<Self> {
        let channels = params
            .channels
            .as_ref()
            .map(|c| c.count())
            .unwrap_or(0);

        if channels == 0 {
            return unsupported_error("ac3: channel layout required");
        }

        let sample_rate = params.sample_rate.unwrap_or(48_000);
        let is_eac3 = params.codec == CODEC_ID_EAC3;

        let mut oxideav_params = oxideav_core::CodecParameters::audio(
            oxideav_core::CodecId::new(if is_eac3 { "eac3" } else { "ac3" }),
        );
        oxideav_params.channels = Some(channels as u16);
        oxideav_params.sample_rate = Some(sample_rate);
        oxideav_params.extradata = params
            .extra_data
            .as_deref()
            .map(<[u8]>::to_vec)
            .unwrap_or_default();

        let inner = if is_eac3 {
            oxideav_ac3::decoder::make_eac3_decoder(&oxideav_params)
        } else {
            oxideav_ac3::decoder::make_decoder(&oxideav_params)
        }
        .map_err(|_| Error::DecodeError("ac3: decoder init failed"))?;

        let sym_channels = params.channels.clone().unwrap();
        let buf = AudioBuffer::new(AudioSpec::new(sample_rate, sym_channels), BUF_CAPACITY);

        Ok(Self { inner, codec_params: params.clone(), buf })
    }
}

impl AudioDecoder for Ac3Decoder {
    fn reset(&mut self) {}

    fn codec_info(&self) -> &CodecInfo {
        let codecs = Self::supported_codecs();
        codecs
            .iter()
            .find(|sc| sc.id == self.codec_params.codec)
            .map(|sc| &sc.info)
            .unwrap_or_else(|| &codecs[0].info)
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        &self.codec_params
    }

    fn decode_ref(&mut self, packet: &PacketRef) -> Result<GenericAudioBufferRef<'_>> {
        let mut reader = packet.as_buf_reader();
        let data = reader.read_buf_bytes_available_ref().to_vec();

        let oxideav_packet = oxideav_core::Packet::new(
            0,
            oxideav_core::TimeBase::new(1, i64::from(self.codec_params.sample_rate.unwrap_or(48_000))),
            data,
        );

        self.inner
            .send_packet(&oxideav_packet)
            .map_err(|_| Error::DecodeError("ac3: send_packet failed"))?;

        let audio_frame = match self.inner.receive_frame() {
            Ok(oxideav_core::Frame::Audio(f)) => f,
            Err(oxideav_core::Error::NeedMore) => {
                self.buf.clear();
                return Ok(self.buf.as_generic_audio_buffer_ref());
            }
            Ok(_) => return Err(Error::DecodeError("ac3: non-audio frame from decoder")),
            Err(_) => return Err(Error::DecodeError("ac3: receive_frame failed")),
        };

        // AudioFrame.data[0] holds interleaved S16 samples in native byte order.
        let bytes = audio_frame.data.first().map(Vec::as_slice).unwrap_or(&[]);
        let pcm: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|b| i16::from_ne_bytes([b[0], b[1]]))
            .collect();

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

impl RegisterableAudioDecoder for Ac3Decoder {
    fn try_registry_new(
        params: &AudioCodecParameters,
        opts: &AudioDecoderOptions,
    ) -> symphonia::core::errors::Result<Box<dyn AudioDecoder>>
    where
        Self: Sized,
    {
        Ok(Box::new(Ac3Decoder::try_new(params, opts)?))
    }

    fn supported_codecs() -> &'static [SupportedAudioCodec] {
        &[
            support_audio_codec!(CODEC_ID_AC3, "ac3", "Dolby Digital AC-3", &[]),
            support_audio_codec!(CODEC_ID_EAC3, "eac3", "Dolby Digital Plus E-AC-3", &[]),
        ]
    }
}
