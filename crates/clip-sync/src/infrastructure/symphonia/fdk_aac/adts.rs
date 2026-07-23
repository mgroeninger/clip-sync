use super::meta::M4AType;

/// Maximum ADTS frame length (13-bit field), including the 7-byte header.
const ADTS_MAX_FRAME_LENGTH: u64 = 0x1FFF;
const ADTS_HEADER_LENGTH: u64 = 7;

/// Build a 7-byte ADTS header for one AAC access unit.
///
/// Returns a decode error when the object type is outside the 2-bit ADTS profile
/// range (Main/LC/SSR/LTP), the sample-rate index is not a defined table entry,
/// or the packet would overflow the 13-bit frame_length field.
pub(super) fn construct_adts_header(
    object_type: M4AType,
    sample_freq_index: u8,
    channel_config: u8,
    num_bytes: u64,
) -> symphonia::core::errors::Result<Vec<u8>> {
    // ADTS profile is 2 bits (AOT − 1 for Main/LC/SSR/LTP). HE-AAC reports
    // SBR/PS from FDK after the first frame; those wrap as LC in ADTS.
    let adts_object_type = match object_type {
        M4AType::Main => 0u8,
        M4AType::Lc | M4AType::Sbr | M4AType::PS | M4AType::ER_AAC_LC => 1,
        M4AType::Ssr => 2,
        M4AType::Ltp | M4AType::ER_AAC_LTP => 3,
        M4AType::None | M4AType::Unknown | M4AType::Reserved => {
            return symphonia::core::errors::decode_error(
                "aac: object type not representable in ADTS profile field",
            );
        }
        _ => {
            return symphonia::core::errors::decode_error(
                "aac: object type not representable in ADTS profile field",
            );
        }
    };

    // Indices 0..=11 are defined sample rates; 12..=14 reserved; 15 is ASC escape
    // (not valid as a standalone ADTS sampling_frequency_index for FDK).
    if sample_freq_index > 11 {
        return symphonia::core::errors::decode_error(
            "aac: sample rate index not representable in ADTS header",
        );
    }

    let frame_length = ADTS_HEADER_LENGTH.checked_add(num_bytes).ok_or(
        symphonia::core::errors::Error::DecodeError("aac: ADTS frame length overflow"),
    )?;
    if frame_length > ADTS_MAX_FRAME_LENGTH {
        return symphonia::core::errors::decode_error(
            "aac: access unit exceeds ADTS frame_length maximum (8191)",
        );
    }
    let frame_length = frame_length as u16;

    let byte0 = 0b1111_1111;
    let byte1 = 0b1111_0001;

    // profile(2) | sampling_frequency_index(4) | private_bit(1)=1 | channel_config[2](1)
    let mut byte2 = 0b0000_0000;
    byte2 = (byte2 << 2) | adts_object_type;
    byte2 = (byte2 << 4) | (sample_freq_index & 0x0f);
    byte2 = (byte2 << 1) | 0b1;
    byte2 = (byte2 << 1) | ((channel_config >> 2) & 0b1);

    // channel_config[1:0](2) | originality(1)=1 | home(1)=1 | copyright_id_bit(1)=1
    // | copyright_id_start(1)=1 | frame_length[12:11](2)
    let mut byte3 = 0b0000_0000;
    byte3 = (byte3 << 2) | (channel_config & 0b11);
    byte3 = (byte3 << 4) | 0b1111;
    byte3 = (byte3 << 2) | (((frame_length >> 11) & 0b11) as u8);

    let byte4 = ((frame_length >> 3) & 0xff) as u8;

    // frame_length[2:0](3) | adts_buffer_fullness(5)=0b11111
    let mut byte5 = 0b0000_0000;
    byte5 = (byte5 << 3) | ((frame_length & 0b111) as u8);
    byte5 = (byte5 << 5) | 0b11111;

    // adts_buffer_fullness cont.(6)=0b111111 | number_of_raw_data_blocks(2)=0
    let mut byte6 = 0b0000_0000;
    byte6 = (byte6 << 6) | 0b111111;
    byte6 <<= 2;

    Ok(vec![byte0, byte1, byte2, byte3, byte4, byte5, byte6])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construct_adts_header_lc_stereo_48k() {
        let header = construct_adts_header(M4AType::Lc, 3, 2, 100).expect("header");
        assert_eq!(header.len(), 7);
        assert_eq!(header[0], 0xff);
        assert_eq!(header[1], 0xf1);
        // profile=1 (LC), sf_index=3, private=1, chan_hi=0 → 0b01_0011_1_0 = 0x4e
        assert_eq!(header[2], 0x4e);
        let frame_length = 7u16 + 100;
        let reconstructed = ((u16::from(header[3] & 0b11) << 11)
            | (u16::from(header[4]) << 3)
            | (u16::from(header[5] >> 5))) as u16;
        assert_eq!(reconstructed, frame_length);
    }

    #[test]
    fn construct_adts_header_rejects_none_object_type() {
        let err = construct_adts_header(M4AType::None, 3, 2, 100).expect_err("None AOT");
        assert!(err.to_string().contains("object type"));
    }

    #[test]
    fn construct_adts_header_maps_sbr_to_lc_profile() {
        // HE-AAC: FDK reports SBR after configure_metadata; ADTS profile stays LC.
        let header = construct_adts_header(M4AType::Sbr, 3, 2, 100).expect("SBR→LC");
        assert_eq!(header[2] >> 6, 1, "ADTS profile must be LC (1)");
    }

    #[test]
    fn construct_adts_header_rejects_scalable_object_type() {
        let err = construct_adts_header(M4AType::Scalable, 3, 2, 100).expect_err("Scalable");
        assert!(err.to_string().contains("object type"));
    }

    #[test]
    fn construct_adts_header_rejects_oversized_packet() {
        let err = construct_adts_header(M4AType::Lc, 3, 2, 8192).expect_err("too large");
        assert!(err.to_string().contains("frame_length"));
    }

    #[test]
    fn construct_adts_header_rejects_escape_sample_rate_index() {
        let err = construct_adts_header(M4AType::Lc, 15, 2, 100).expect_err("escape index");
        assert!(err.to_string().contains("sample rate index"));
    }

    #[test]
    fn construct_adts_header_accepts_max_payload() {
        // 8191 total − 7 header = 8184 payload bytes.
        let header = construct_adts_header(M4AType::Lc, 3, 2, 8184).expect("max frame");
        let reconstructed = ((u16::from(header[3] & 0b11) << 11)
            | (u16::from(header[4]) << 3)
            | (u16::from(header[5] >> 5))) as u16;
        assert_eq!(reconstructed, 8191);
    }
}
