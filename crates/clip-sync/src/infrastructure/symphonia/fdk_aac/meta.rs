use symphonia::core::audio::{Channels, layouts};
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

        if otypeidx >= M4A_TYPES.len() {
            Ok(M4AType::Unknown)
        } else {
            Ok(M4A_TYPES[otypeidx])
        }
    }

    fn read_sampling_frequency<B: ReadBitsLtr>(bs: &mut B) -> Result<u32> {
        match bs.read_bits_leq32(4)? {
            idx if idx < 15 => Ok(AAC_SAMPLE_RATES[idx as usize]),
            _ => {
                let srate = (0xf << 20) & bs.read_bits_leq32(20)?;
                Ok(srate)
            }
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
        self.sample_rate_index = sample_rate_index(self.sample_rate);

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

const AAC_SAMPLE_RATES: [u32; 16] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350, 0, 0,
    0,
];

pub(super) fn sample_rate_index(sample_rate: u32) -> u8 {
    AAC_SAMPLE_RATES
        .iter()
        .position(|s| *s == sample_rate)
        .unwrap_or_default() as u8
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
