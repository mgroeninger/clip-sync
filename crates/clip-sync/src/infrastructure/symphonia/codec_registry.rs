use symphonia::core::codecs::registry::CodecRegistry;

#[cfg(any(feature = "he-aac", feature = "ac3"))]
use std::sync::OnceLock;

#[cfg(any(feature = "he-aac", feature = "ac3"))]
static CODEC_REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();

/// Symphonia codec registry for probe, decodability checks, and decode.
///
/// Returns the default Symphonia registry when no extra-decoder features are active.
/// With `he-aac`, Symphonia's native decoder handles AAC-LC (including MP4/MKV raw
/// access units and 5.1 PCE); FDK is registered for HE-AAC / HE-AACv2 only.
/// With `ac3`, oxideav-ac3 is added for AC-3 and E-AC-3 tracks.
/// The registry is built once and cached for the process lifetime.
pub fn codec_registry() -> &'static CodecRegistry {
    #[cfg(not(any(feature = "he-aac", feature = "ac3")))]
    {
        symphonia::default::get_codecs()
    }

    #[cfg(any(feature = "he-aac", feature = "ac3"))]
    {
        CODEC_REGISTRY.get_or_init(build_codec_registry)
    }
}

#[cfg(any(feature = "he-aac", feature = "ac3"))]
fn build_codec_registry() -> CodecRegistry {
    let mut registry = CodecRegistry::new();

    #[cfg(feature = "he-aac")]
    {
        register_symphonia_decoders_without_native_aac(&mut registry);
        // Native AAC-LC first: MP4/MKV store raw access units (not ADTS). FDK's ADTS
        // wrapper breaks ffmpeg PCE-style 5.1; Symphonia handles container AAC correctly.
        use symphonia::default::codecs;
        registry.register_audio_decoder::<codecs::AacDecoder>();
        registry.register_audio_decoder::<crate::infrastructure::symphonia::fdk_aac::AacDecoder>();
    }

    #[cfg(not(feature = "he-aac"))]
    {
        // Register all default Symphonia decoders when HE-AAC is not replacing them.
        use symphonia::default::codecs;
        registry.register_audio_decoder::<codecs::AacDecoder>();
        registry.register_audio_decoder::<codecs::AdpcmDecoder>();
        registry.register_audio_decoder::<codecs::FlacDecoder>();
        registry.register_audio_decoder::<codecs::PcmDecoder>();
        registry.register_audio_decoder::<codecs::VorbisDecoder>();
        registry.register_audio_decoder::<codecs::MpaDecoder>();
    }

    #[cfg(feature = "ac3")]
    {
        registry.register_audio_decoder::<crate::infrastructure::symphonia::oxideav_ac3::Ac3Decoder>();
    }

    registry
}

#[cfg(feature = "he-aac")]
fn register_symphonia_decoders_without_native_aac(registry: &mut CodecRegistry) {
    use symphonia::default::codecs;

    registry.register_audio_decoder::<codecs::AdpcmDecoder>();
    registry.register_audio_decoder::<codecs::FlacDecoder>();
    registry.register_audio_decoder::<codecs::PcmDecoder>();
    registry.register_audio_decoder::<codecs::VorbisDecoder>();
}
