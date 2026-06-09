use symphonia::core::audio::{
    AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, Channels, GenericAudioBufferRef,
    layouts,
};
use symphonia::core::codecs::CodecInfo;
use symphonia::core::codecs::audio::well_known::{CODEC_ID_AC3, CODEC_ID_EAC3};
use symphonia::core::codecs::audio::{
    AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
};
use symphonia::core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia::core::errors::Error;
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
    sample_rate: u32,
    /// Populated on first decoded frame; `None` at probe time when the container
    /// (e.g. MP4 `dac3` box) does not supply a Symphonia channel layout.
    buf: Option<AudioBuffer<i16>>,
    /// Returned by `last_decoded` before the first frame is decoded.
    empty_buf: AudioBuffer<i16>,
}

// oxideav_core::Decoder requires only Send; all Symphonia decode methods take &mut self
// (no shared-reference access is possible), so Sync is safe to assert here.
unsafe impl Sync for Ac3Decoder {}

impl Ac3Decoder {
    fn try_new(params: &AudioCodecParameters, _opts: &AudioDecoderOptions) -> Result<Self> {
        let sample_rate = params.sample_rate.unwrap_or(48_000);
        let is_eac3 = params.codec == CODEC_ID_EAC3;

        let mut oxideav_params = oxideav_core::CodecParameters::audio(
            oxideav_core::CodecId::new(if is_eac3 { "eac3" } else { "ac3" }),
        );
        // params.channels is often None for AC-3 in MP4: Symphonia's isomp4 demuxer reads
        // the dac3 box but does not populate AudioCodecParameters.channels from it.
        // oxideav reads channel config from the bitstream, so passing None is fine here.
        oxideav_params.channels = params.channels.as_ref().map(|c| c.count() as u16);
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

        // Use the container's channel layout if available; otherwise defer to first decode.
        let buf = params.channels.clone().map(|ch| {
            AudioBuffer::new(AudioSpec::new(sample_rate, ch), BUF_CAPACITY)
        });
        let empty_buf = AudioBuffer::new(
            AudioSpec::new(sample_rate, layouts::CHANNEL_LAYOUT_STEREO),
            0,
        );

        Ok(Self { inner, codec_params: params.clone(), sample_rate, buf, empty_buf })
    }
}

/// Map a raw AC-3 channel count to a Symphonia layout.
///
/// oxideav outputs channels in WAVE order; the Symphonia layouts below match
/// that ordering for the standard AC-3 configurations.
fn ac3_channel_layout(n: usize) -> Option<Channels> {
    match n {
        1 => Some(layouts::CHANNEL_LAYOUT_MONO),
        2 => Some(layouts::CHANNEL_LAYOUT_STEREO),
        4 => Some(layouts::CHANNEL_LAYOUT_4P0),
        6 => Some(layouts::CHANNEL_LAYOUT_5P1),
        8 => Some(layouts::CHANNEL_LAYOUT_7P1),
        _ => None,
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
            oxideav_core::TimeBase::new(1, i64::from(self.sample_rate)),
            data,
        );

        self.inner
            .send_packet(&oxideav_packet)
            .map_err(|_| Error::DecodeError("ac3: send_packet failed"))?;

        let audio_frame = match self.inner.receive_frame() {
            Ok(oxideav_core::Frame::Audio(f)) => f,
            Err(oxideav_core::Error::NeedMore) => {
                if let Some(ref mut b) = self.buf {
                    b.clear();
                    return Ok(b.as_generic_audio_buffer_ref());
                }
                return Ok(self.empty_buf.as_generic_audio_buffer_ref());
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

        // Lazily initialize the buffer on the first decoded frame when the container
        // did not supply a channel layout (common for AC-3 in MP4).
        if self.buf.is_none() {
            let samples_per_ch = audio_frame.samples as usize;
            let n_ch = if samples_per_ch > 0 { pcm.len() / samples_per_ch } else { 0 };
            let layout = ac3_channel_layout(n_ch)
                .ok_or(Error::DecodeError("ac3: unsupported channel count from bitstream"))?;
            self.buf = Some(AudioBuffer::new(
                AudioSpec::new(self.sample_rate, layout.clone()),
                BUF_CAPACITY,
            ));
            // Update codec_params so callers see the correct layout after first decode.
            self.codec_params.channels = Some(layout);
        }

        let buf = self.buf.as_mut().unwrap();
        buf.clear();
        buf.render_uninit(None);
        buf.copy_from_slice_interleaved(&pcm);
        buf.trim(
            packet.trim_start.get() as usize,
            packet.trim_end.get() as usize,
        );

        Ok(buf.as_generic_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        self.buf
            .as_ref()
            .unwrap_or(&self.empty_buf)
            .as_generic_audio_buffer_ref()
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
