use std::sync::Mutex;

use symphonia::core::audio::{
    layouts, AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, Channels,
    GenericAudioBufferRef,
};
use symphonia::core::codecs::audio::well_known::{CODEC_ID_AC3, CODEC_ID_EAC3};
use symphonia::core::codecs::audio::{
    AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
};
use symphonia::core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia::core::codecs::CodecInfo;
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
    /// `oxideav_core::Decoder` is `Send` but not `Sync`. Symphonia's
    /// `AudioDecoder` requires `Send + Sync`; a `Mutex` makes shared
    /// references sound without an `unsafe impl Sync`.
    inner: Mutex<Box<dyn OxideDecoder>>,
    codec_params: AudioCodecParameters,
    sample_rate: u32,
    /// Populated on first decoded frame; `None` at probe time when the container
    /// (e.g. MP4 `dac3` box) does not supply a Symphonia channel layout.
    buf: Option<AudioBuffer<i16>>,
    /// Returned by `last_decoded` before the first frame is decoded.
    empty_buf: AudioBuffer<i16>,
    /// Reusable interleaved-PCM accumulator for the per-packet drain loop.
    /// Avoids a per-packet allocation (cleared, not reallocated, each call).
    pcm_scratch: Vec<i16>,
}

impl Ac3Decoder {
    fn try_new(params: &AudioCodecParameters, _opts: &AudioDecoderOptions) -> Result<Self> {
        let sample_rate = params.sample_rate.unwrap_or(48_000);
        let is_eac3 = params.codec == CODEC_ID_EAC3;

        let mut oxideav_params =
            oxideav_core::CodecParameters::audio(oxideav_core::CodecId::new(if is_eac3 {
                "eac3"
            } else {
                "ac3"
            }));
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
        let buf = params
            .channels
            .clone()
            .map(|ch| AudioBuffer::new(AudioSpec::new(sample_rate, ch), BUF_CAPACITY));
        let empty_buf = AudioBuffer::new(
            AudioSpec::new(sample_rate, layouts::CHANNEL_LAYOUT_STEREO),
            0,
        );

        Ok(Self {
            inner: Mutex::new(inner),
            codec_params: params.clone(),
            sample_rate,
            buf,
            empty_buf,
            pcm_scratch: Vec::with_capacity(BUF_CAPACITY * 2),
        })
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

/// Send one packet and drain *all* frames it produces into `scratch` (cleared
/// first), as interleaved S16 samples.
///
/// Returns `(samples_total, n_ch)` where `samples_total` is samples-per-channel
/// summed across every drained frame and `n_ch` is the channel count. A packet
/// that decodes to nothing yet (decoder still buffering) returns `(0, 0)`.
///
/// Draining to `NeedMore` is required for E-AC-3, whose container packets can
/// carry several substreams: pulling a single frame would strand the rest for
/// the next packet, silently dropping audio and skewing sample counts. All
/// drained frames must share a channel count (they cover successive time spans
/// of the same program); a mismatch is a hard decode error rather than a
/// mis-interleave.
fn drain_packet(
    inner: &mut dyn OxideDecoder,
    packet: &oxideav_core::Packet,
    scratch: &mut Vec<i16>,
) -> Result<(usize, usize)> {
    inner
        .send_packet(packet)
        .map_err(|_| Error::DecodeError("ac3: send_packet failed"))?;

    scratch.clear();
    let mut samples_total: usize = 0;
    let mut n_ch: usize = 0;
    loop {
        let frame = match inner.receive_frame() {
            Ok(oxideav_core::Frame::Audio(f)) => f,
            Err(oxideav_core::Error::NeedMore) => break,
            Ok(_) => return Err(Error::DecodeError("ac3: non-audio frame from decoder")),
            Err(_) => return Err(Error::DecodeError("ac3: receive_frame failed")),
        };

        let frame_samples = frame.samples as usize;
        if frame_samples == 0 {
            continue;
        }
        // AudioFrame.data[0] holds interleaved S16 samples in native byte order.
        let bytes = frame.data.first().map(Vec::as_slice).unwrap_or(&[]);
        let frame_ch = (bytes.len() / 2).checked_div(frame_samples).unwrap_or(0);
        if n_ch == 0 {
            n_ch = frame_ch;
        } else if frame_ch != n_ch {
            return Err(Error::DecodeError(
                "ac3: inconsistent channel count across frames in packet",
            ));
        }

        scratch.extend(
            bytes
                .chunks_exact(2)
                .map(|b| i16::from_ne_bytes([b[0], b[1]])),
        );
        samples_total += frame_samples;
    }

    Ok((samples_total, n_ch))
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

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| Error::DecodeError("ac3: decoder mutex poisoned"))?;

        // Drain every frame the packet produced. AC-3 emits one syncframe per
        // packet, but E-AC-3 can carry several substreams whose frames must all
        // be pulled — a single `receive_frame` would strand the rest until the
        // next packet, silently dropping audio and skewing sample counts.
        let (samples_total, n_ch) =
            drain_packet(&mut **inner, &oxideav_packet, &mut self.pcm_scratch)?;
        drop(inner);

        // No decoded output (decoder still buffering): return an empty buffer,
        // matching Symphonia's "decoded nothing this packet" contract.
        if samples_total == 0 {
            if let Some(ref mut b) = self.buf {
                b.clear();
                return Ok(b.as_generic_audio_buffer_ref());
            }
            return Ok(self.empty_buf.as_generic_audio_buffer_ref());
        }

        // Lazily initialize the buffer on the first decoded frame when the container
        // did not supply a channel layout (common for AC-3 in MP4).
        if self.buf.is_none() {
            let layout = ac3_channel_layout(n_ch).ok_or(Error::DecodeError(
                "ac3: unsupported channel count from bitstream",
            ))?;
            self.buf = Some(AudioBuffer::new(
                AudioSpec::new(self.sample_rate, layout.clone()),
                BUF_CAPACITY.max(samples_total),
            ));
            // Update codec_params so callers see the correct layout after first decode.
            self.codec_params.channels = Some(layout);
        }

        let buf = self.buf.as_mut().unwrap();
        // A packet with many substreams can exceed the preallocated cap; grow so
        // `render_uninit` (which panics past capacity) always has room.
        buf.grow_capacity(samples_total);
        buf.clear();
        buf.render_uninit(Some(samples_total));
        buf.copy_from_slice_interleaved(&self.pcm_scratch);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    use oxideav_core::{AudioFrame, CodecId, Frame, Packet, TimeBase};

    /// Decoder stub that replays a scripted sequence of frames per packet, then
    /// signals `NeedMore` — exactly the shape `drain_packet` must fully drain.
    struct ScriptedDecoder {
        codec_id: CodecId,
        frames: VecDeque<AudioFrame>,
    }

    impl ScriptedDecoder {
        fn new(frames: Vec<AudioFrame>) -> Self {
            Self {
                codec_id: CodecId::new("eac3"),
                frames: frames.into(),
            }
        }
    }

    impl OxideDecoder for ScriptedDecoder {
        fn codec_id(&self) -> &CodecId {
            &self.codec_id
        }

        fn send_packet(&mut self, _packet: &Packet) -> oxideav_core::Result<()> {
            Ok(())
        }

        fn receive_frame(&mut self) -> oxideav_core::Result<Frame> {
            match self.frames.pop_front() {
                Some(frame) => Ok(Frame::Audio(frame)),
                None => Err(oxideav_core::Error::NeedMore),
            }
        }

        fn flush(&mut self) -> oxideav_core::Result<()> {
            Ok(())
        }
    }

    /// Build an interleaved-S16 audio frame from per-channel sample lanes.
    fn interleaved_frame(lanes: &[&[i16]]) -> AudioFrame {
        let samples = lanes.first().map_or(0, |l| l.len());
        assert!(
            lanes.iter().all(|l| l.len() == samples),
            "lanes must share length"
        );
        let mut bytes = Vec::with_capacity(samples * lanes.len() * 2);
        for i in 0..samples {
            for lane in lanes {
                bytes.extend_from_slice(&lane[i].to_ne_bytes());
            }
        }
        AudioFrame {
            samples: samples as u32,
            pts: None,
            data: vec![bytes],
        }
    }

    fn dummy_packet() -> Packet {
        Packet::new(0, TimeBase::new(1, 48_000), Vec::new())
    }

    #[test]
    fn drain_packet_accumulates_every_frame_until_needmore() {
        // Two stereo frames in one packet — the exact E-AC-3 multi-substream
        // case a single receive_frame would truncate.
        let frame_a = interleaved_frame(&[&[1, 2, 3], &[-1, -2, -3]]);
        let frame_b = interleaved_frame(&[&[4, 5], &[-4, -5]]);
        let mut decoder = ScriptedDecoder::new(vec![frame_a, frame_b]);
        let mut scratch = Vec::new();

        let (samples_total, n_ch) =
            drain_packet(&mut decoder, &dummy_packet(), &mut scratch).expect("drain succeeds");

        assert_eq!(n_ch, 2);
        assert_eq!(
            samples_total, 5,
            "3 + 2 samples per channel across two frames"
        );
        // Interleaved: frame A (L0 R0 L1 R1 L2 R2) then frame B (L0 R0 L1 R1).
        assert_eq!(
            scratch,
            vec![1, -1, 2, -2, 3, -3, 4, -4, 5, -5],
            "both frames must be concatenated, none stranded"
        );
    }

    #[test]
    fn drain_packet_returns_zero_when_decoder_still_buffering() {
        let mut decoder = ScriptedDecoder::new(vec![]);
        let mut scratch = vec![99]; // must be cleared

        let (samples_total, n_ch) =
            drain_packet(&mut decoder, &dummy_packet(), &mut scratch).expect("drain succeeds");

        assert_eq!((samples_total, n_ch), (0, 0));
        assert!(scratch.is_empty(), "scratch cleared even with no frames");
    }

    #[test]
    fn drain_packet_skips_empty_frames() {
        let empty = AudioFrame {
            samples: 0,
            pts: None,
            data: vec![Vec::new()],
        };
        let real = interleaved_frame(&[&[7], &[-7]]);
        let mut decoder = ScriptedDecoder::new(vec![empty, real]);
        let mut scratch = Vec::new();

        let (samples_total, n_ch) =
            drain_packet(&mut decoder, &dummy_packet(), &mut scratch).expect("drain succeeds");

        assert_eq!((samples_total, n_ch), (1, 2));
        assert_eq!(scratch, vec![7, -7]);
    }

    #[test]
    fn drain_packet_rejects_inconsistent_channel_count() {
        let stereo = interleaved_frame(&[&[1, 2], &[-1, -2]]);
        let mono = interleaved_frame(&[&[3, 4]]);
        let mut decoder = ScriptedDecoder::new(vec![stereo, mono]);
        let mut scratch = Vec::new();

        let err = drain_packet(&mut decoder, &dummy_packet(), &mut scratch)
            .expect_err("channel-count mismatch must be a hard error, not a mis-interleave");
        assert!(matches!(err, Error::DecodeError(_)));
    }
}
