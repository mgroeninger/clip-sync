use std::ops::Range;

use super::meta::M4AType;

pub(super) fn construct_adts_header(
    object_type: M4AType,
    sample_freq_index: u8,
    channel_config: u8,
    num_bytes: u64,
) -> Vec<u8> {
    let adts_header_length = 7;
    let byte0 = 0b1111_1111;
    let byte1 = 0b1111_0001;

    let mut byte2 = 0b0000_0000;
    let adts_object_type = object_type as u8 - 1;
    byte2 = (byte2 << 2) | adts_object_type;
    byte2 = (byte2 << 4) | sample_freq_index;
    byte2 = (byte2 << 1) | 0b1;
    byte2 = (byte2 << 1) | get_bits_u8(channel_config, 6..6);

    let mut byte3 = 0b0000_0000;
    byte3 = (byte3 << 2) | get_bits_u8(channel_config, 7..8);
    byte3 = (byte3 << 4) | 0b1111;

    let frame_length = adts_header_length + num_bytes as u16;
    byte3 = (byte3 << 2) | get_bits_u16(frame_length, 3..5) as u8;

    let byte4 = get_bits_u16(frame_length, 6..13) as u8;

    let mut byte5 = 0b0000_0000;
    byte5 = (byte5 << 3) | get_bits_u16(frame_length, 14..16) as u8;
    byte5 = (byte5 << 5) | 0b11111;

    let mut byte6 = 0b0000_0000;
    byte6 = (byte6 << 6) | 0b111111;
    byte6 <<= 2;

    vec![byte0, byte1, byte2, byte3, byte4, byte5, byte6]
}

fn get_bits_u16(byte: u16, range: Range<u16>) -> u16 {
    let shaved_left = byte << (range.start - 1);
    let moved_back = shaved_left >> (range.start - 1);
    moved_back >> (16 - range.end)
}

fn get_bits_u8(byte: u8, range: Range<u8>) -> u8 {
    let shaved_left = byte << (range.start - 1);
    let moved_back = shaved_left >> (range.start - 1);
    moved_back >> (8 - range.end)
}
