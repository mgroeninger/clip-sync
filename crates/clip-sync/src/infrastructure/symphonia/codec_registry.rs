use symphonia::core::codecs::registry::CodecRegistry;

#[cfg(feature = "he-aac")]
use std::sync::OnceLock;

#[cfg(feature = "he-aac")]
static HE_AAC_CODEC_REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();

/// Symphonia codec registry for probe, decodability checks, and decode.
///
/// With the `he-aac` feature, AAC tracks use Fraunhofer FDK instead of Symphonia's
/// native AAC-LC decoder. Native Symphonia AAC must not be registered alongside FDK.
pub fn codec_registry() -> &'static CodecRegistry {
    #[cfg(not(feature = "he-aac"))]
    {
        symphonia::default::get_codecs()
    }

    #[cfg(feature = "he-aac")]
    {
        HE_AAC_CODEC_REGISTRY.get_or_init(build_he_aac_codec_registry)
    }
}

#[cfg(feature = "he-aac")]
fn build_he_aac_codec_registry() -> CodecRegistry {
    let mut registry = CodecRegistry::new();
    register_symphonia_decoders_without_native_aac(&mut registry);
    registry.register_audio_decoder::<crate::infrastructure::symphonia::fdk_aac::AacDecoder>();
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
