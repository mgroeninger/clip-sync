use symphonia::core::audio::{layouts, Channels};
use symphonia::core::errors::Result;
use symphonia::core::io::{BitReaderLtr, ReadBitsLtr};

macro_rules! validate {
    ($a:expr) => {
        if !$a {
            return symphonia::core::errors::decode_error("aac: invalid data");
        }
    };
}

#[derive(Default, Debug)]
pub(super) struct M4AInfo {
    pub(super) otype: M4AType,
    pub(super) sample_rate: u32,
    pub(super) sample_rate_index: u8,
    pub(super) channels: u8,
    pub(super) samples: usize,
}

impl M4AInfo {
    fn read_object_type<B: ReadBitsLtr>(bs: &mut B) -> Result<M4AType> {
        let otypeidx = match bs.read_bits_leq32(5)? {
            idx if idx < 31 => idx as usize,
            31 => (bs.read_bits_leq32(6)? + 32) as usize,
            _ => unreachable!(),
        };

        Ok(m4a_type_from_index(otypeidx))
    }

    fn read_sampling_frequency<B: ReadBitsLtr>(bs: &mut B) -> Result<u32> {
        match bs.read_bits_leq32(4)? {
            idx if idx < 15 => Ok(AAC_SAMPLE_RATES[idx as usize]),
            // ISO 14496-3: samplingFrequencyIndex == 0x0f → 24-bit explicit rate.
            _ => Ok(bs.read_bits_leq32(24)?),
        }
    }

    fn read_channel_config<B: ReadBitsLtr>(bs: &mut B) -> Result<usize> {
        let chidx = bs.read_bits_leq32(4)? as usize;
        if chidx < AAC_CHANNELS.len() {
            Ok(AAC_CHANNELS[chidx])
        } else {
            Ok(chidx)
        }
    }

    pub(super) fn read(&mut self, buf: &[u8]) -> Result<()> {
        let mut bs = BitReaderLtr::new(buf);

        self.otype = Self::read_object_type(&mut bs)?;
        self.sample_rate = Self::read_sampling_frequency(&mut bs)?;
        // Prefer a defined table index so ADTS headers stay valid. Escape-rate
        // streams whose Hz is not in the table cannot be wrapped as ADTS.
        self.sample_rate_index = sample_rate_index(self.sample_rate).ok_or(
            symphonia::core::errors::Error::DecodeError("aac: sample rate has no ADTS table index"),
        )?;

        validate!(self.sample_rate > 0);

        self.channels = Self::read_channel_config(&mut bs)? as u8;

        if (self.otype == M4AType::Sbr) || (self.otype == M4AType::PS) {
            let _ext_srate = Self::read_sampling_frequency(&mut bs)?;
            self.otype = Self::read_object_type(&mut bs)?;

            if self.otype == M4AType::ER_BSAC {
                let _ext_chans = Self::read_channel_config(&mut bs)?;
            }
        }
        let short_frame = bs.read_bool()?;
        self.samples = if short_frame { 960 } else { 1024 };

        Ok(())
    }
}

#[allow(non_camel_case_types)]
#[derive(Clone, Default, Copy, Debug, PartialEq, Eq)]
pub(super) enum M4AType {
    #[default]
    None,
    Main,
    Lc,
    Ssr,
    Ltp,
    Sbr,
    Scalable,
    TwinVQ,
    Celp,
    Hvxc,
    Ttsi,
    MainSynth,
    WavetableSynth,
    GeneralMIDI,
    Algorithmic,
    ER_AAC_LC,
    ER_AAC_LTP,
    ER_AAC_Scalable,
    ER_TwinVQ,
    ER_BSAC,
    ER_AAC_LD,
    ER_CELP,
    ER_HVXC,
    ER_HILN,
    ER_Parametric,
    Ssc,
    PS,
    MPEGSurround,
    Layer1,
    Layer2,
    Layer3,
    Dst,
    Als,
    Sls,
    SLSNonCore,
    ER_AAC_ELD,
    SMRSimple,
    SMRMain,
    Reserved,
    Unknown,
}

pub(super) const M4A_TYPES: &[M4AType] = &[
    M4AType::None,
    M4AType::Main,
    M4AType::Lc,
    M4AType::Ssr,
    M4AType::Ltp,
    M4AType::Sbr,
    M4AType::Scalable,
    M4AType::TwinVQ,
    M4AType::Celp,
    M4AType::Hvxc,
    M4AType::Reserved,
    M4AType::Reserved,
    M4AType::Ttsi,
    M4AType::MainSynth,
    M4AType::WavetableSynth,
    M4AType::GeneralMIDI,
    M4AType::Algorithmic,
    M4AType::ER_AAC_LC,
    M4AType::Reserved,
    M4AType::ER_AAC_LTP,
    M4AType::ER_AAC_Scalable,
    M4AType::ER_TwinVQ,
    M4AType::ER_BSAC,
    M4AType::ER_AAC_LD,
    M4AType::ER_CELP,
    M4AType::ER_HVXC,
    M4AType::ER_HILN,
    M4AType::ER_Parametric,
    M4AType::Ssc,
    M4AType::PS,
    M4AType::MPEGSurround,
    M4AType::Reserved,
    M4AType::Layer1,
    M4AType::Layer2,
    M4AType::Layer3,
    M4AType::Dst,
    M4AType::Als,
    M4AType::Sls,
    M4AType::SLSNonCore,
    M4AType::ER_AAC_ELD,
    M4AType::SMRSimple,
    M4AType::SMRMain,
];

/// Bounds-checked lookup: FDK can report audio object types (e.g. USAC = 42)
/// beyond this table, so out-of-range indices map to `Unknown` instead of panicking.
pub(super) fn m4a_type_from_index(index: usize) -> M4AType {
    M4A_TYPES.get(index).copied().unwrap_or(M4AType::Unknown)
}

const AAC_SAMPLE_RATES: [u32; 16] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350, 0, 0,
    0,
];

/// Map a sample rate to the AAC/ADTS table index.
///
/// Returns `None` when the rate is not one of the defined table entries (0..=12
/// with a non-zero rate). Callers must not invent index 0 (96 kHz) for unknown
/// rates — that silently corrupts ADTS headers.
pub(super) fn sample_rate_index(sample_rate: u32) -> Option<u8> {
    AAC_SAMPLE_RATES
        .iter()
        .enumerate()
        .find(|(_, rate)| **rate == sample_rate && **rate > 0)
        .map(|(index, _)| index as u8)
}

const AAC_CHANNELS: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 8];

pub(super) fn map_to_channels(num_channels: u8) -> Option<Channels> {
    let channels = match num_channels {
        1 => layouts::CHANNEL_LAYOUT_MONO,
        2 => layouts::CHANNEL_LAYOUT_STEREO,
        3 => layouts::CHANNEL_LAYOUT_AAC_3P0,
        4 => layouts::CHANNEL_LAYOUT_AAC_4P0,
        5 => layouts::CHANNEL_LAYOUT_AAC_5P0,
        6 => layouts::CHANNEL_LAYOUT_AAC_5P1,
        7 => layouts::CHANNEL_LAYOUT_AAC_7P1,
        8 => layouts::CHANNEL_LAYOUT_AAC_7P1,
        _ => return None,
    };

    Some(channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m4a_type_from_index_maps_out_of_range_to_unknown() {
        assert_eq!(m4a_type_from_index(2), M4AType::Lc);
        // FDK reports AOT values beyond the table (e.g. USAC = 42); these must
        // not panic.
        assert_eq!(m4a_type_from_index(42), M4AType::Unknown);
        assert_eq!(m4a_type_from_index(usize::MAX), M4AType::Unknown);
    }

    #[test]
    fn sample_rate_index_maps_table_rates_and_rejects_unknown() {
        assert_eq!(sample_rate_index(48_000), Some(3));
        assert_eq!(sample_rate_index(44_100), Some(4));
        // Must not silently map to index 0 (96 kHz).
        assert_eq!(sample_rate_index(50_000), None);
        assert_eq!(sample_rate_index(0), None);
    }

    #[test]
    fn read_asc_explicit_sample_rate_parses_24_bit_frequency() {
        // Build ASC: AAC LC + escape sample-rate index + explicit 48000 Hz + stereo.
        let mut bits = BitWriter::new();
        bits.write(5, 2); // LC
        bits.write(4, 15); // escape
        bits.write(24, 48_000);
        bits.write(4, 2); // stereo
        bits.write(1, 0); // frameLengthFlag
        bits.write(1, 0); // dependsOnCoreCoder
        bits.write(1, 0); // extensionFlag
        let asc = bits.into_bytes();

        let mut info = M4AInfo::default();
        info.read(&asc).expect("parse ASC with escape rate");
        assert_eq!(info.otype, M4AType::Lc);
        assert_eq!(info.sample_rate, 48_000);
        assert_eq!(info.sample_rate_index, 3);
        assert_eq!(info.channels, 2);
        assert_eq!(info.samples, 1024);
    }

    #[test]
    fn read_asc_explicit_non_table_rate_errors() {
        let mut bits = BitWriter::new();
        bits.write(5, 2); // LC
        bits.write(4, 15); // escape
        bits.write(24, 50_000); // not in AAC table
        bits.write(4, 2);
        bits.write(1, 0);
        bits.write(1, 0);
        bits.write(1, 0);
        let asc = bits.into_bytes();

        let mut info = M4AInfo::default();
        let err = info.read(&asc).expect_err("non-table escape rate");
        assert!(err.to_string().contains("sample rate"));
    }

    #[test]
    fn read_asc_table_index_still_works() {
        let mut bits = BitWriter::new();
        bits.write(5, 2); // LC
        bits.write(4, 3); // 48 kHz table index
        bits.write(4, 2); // stereo
        bits.write(1, 0);
        bits.write(1, 0);
        bits.write(1, 0);
        let asc = bits.into_bytes();

        let mut info = M4AInfo::default();
        info.read(&asc).expect("parse standard ASC");
        assert_eq!(info.sample_rate, 48_000);
        assert_eq!(info.sample_rate_index, 3);
        assert_eq!(info.channels, 2);
    }

    /// Tiny MSB-first bit packer for ASC test fixtures.
    struct BitWriter {
        bytes: Vec<u8>,
        bit: u8,
        cur: u8,
    }

    impl BitWriter {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                bit: 0,
                cur: 0,
            }
        }

        fn write(&mut self, nbits: u32, value: u32) {
            for i in (0..nbits).rev() {
                let bit = ((value >> i) & 1) as u8;
                self.cur = (self.cur << 1) | bit;
                self.bit += 1;
                if self.bit == 8 {
                    self.bytes.push(self.cur);
                    self.cur = 0;
                    self.bit = 0;
                }
            }
        }

        fn into_bytes(mut self) -> Vec<u8> {
            if self.bit > 0 {
                self.cur <<= 8 - self.bit;
                self.bytes.push(self.cur);
            }
            self.bytes
        }
    }
}
