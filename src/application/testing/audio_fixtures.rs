use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};

fn chirp_sample(sample_rate: u32, index: u64) -> i16 {
    let rate = f64::from(sample_rate);
    let t = index as f64 / rate;
    let freq = 300.0 + 400.0 * t;
    ((TAU as f64 * freq * t).sin() * (i16::MAX as f64 * 0.5)).round() as i16
}

fn write_mono_wav(path: &Path, sample_rate: u32, samples: impl IntoIterator<Item = i16>) {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for sample in samples {
        writer.write_sample(sample).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

/// Which file receives leading silence for an inter-file offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChirpDelayOn {
    /// B is late: silence prefix on B (domain offset positive).
    #[default]
    B,
    /// A is late: silence prefix on A (domain offset negative).
    A,
}

/// Writes two mono WAV files with the same chirp and a fixed timing offset.
pub fn write_offset_chirp_wav_pair(
    dir: &Path,
    sample_rate: u32,
    total_secs: u32,
    offset_secs: u32,
) -> (PathBuf, PathBuf) {
    write_offset_chirp_wav_pair_with_delay(dir, sample_rate, total_secs, offset_secs, ChirpDelayOn::B)
}

pub fn write_offset_chirp_wav_pair_with_delay(
    dir: &Path,
    sample_rate: u32,
    total_secs: u32,
    offset_secs: u32,
    delay_on: ChirpDelayOn,
) -> (PathBuf, PathBuf) {
    let total_samples = u64::from(sample_rate) * u64::from(total_secs);
    let delay_samples = u64::from(sample_rate) * u64::from(offset_secs);

    let path_a = dir.join("a.wav");
    let path_b = dir.join("b.wav");

    let samples_for = |delay: u64| {
        (0..total_samples).map(move |index| {
            if index < delay {
                0
            } else {
                chirp_sample(sample_rate, index - delay)
            }
        })
    };

    match delay_on {
        ChirpDelayOn::B => {
            write_mono_wav(&path_a, sample_rate, samples_for(0));
            write_mono_wav(&path_b, sample_rate, samples_for(delay_samples));
        }
        ChirpDelayOn::A => {
            write_mono_wav(&path_a, sample_rate, samples_for(delay_samples));
            write_mono_wav(&path_b, sample_rate, samples_for(0));
        }
    }

    (path_a, path_b)
}

/// Steady tone (e.g. 220 Hz decoy track in dual-track MP4 cases).
pub fn write_tone_wav_at_frequency(path: &Path, sample_rate: u32, seconds: u32, frequency: f32) {
    let total_samples = u64::from(sample_rate) * u64::from(seconds);
    let samples = (0..total_samples).map(|index| {
        let t = index as f32 / sample_rate as f32;
        ((TAU * frequency * t).sin() * (i16::MAX as f32 * 0.5)).round() as i16
    });
    write_mono_wav(path, sample_rate, samples);
}

/// B shares the start offset with A, but trailing silence makes the end window disagree.
pub fn write_two_clip_inconsistent_pair(
    dir: &Path,
    sample_rate: u32,
    total_secs: u32,
    offset_secs: u32,
    tail_silence_secs: u32,
) -> (PathBuf, PathBuf) {
    let total_samples = u64::from(sample_rate) * u64::from(total_secs);
    let delay_samples = u64::from(sample_rate) * u64::from(offset_secs);
    let chirp_samples = total_samples.saturating_sub(delay_samples + u64::from(sample_rate) * u64::from(tail_silence_secs));

    let path_a = dir.join("a.wav");
    let path_b = dir.join("b.wav");

    write_mono_wav(
        &path_a,
        sample_rate,
        (0..total_samples).map(|index| chirp_sample(sample_rate, index)),
    );

    write_mono_wav(
        &path_b,
        sample_rate,
        (0..total_samples).map(|index| {
            if index < delay_samples {
                0
            } else if index < delay_samples + chirp_samples {
                chirp_sample(sample_rate, index - delay_samples)
            } else {
                0
            }
        }),
    );

    (path_a, path_b)
}

/// Piecewise delay: one offset for the start window, another for the end window.
pub fn write_piecewise_offset_chirp_pair(
    dir: &Path,
    sample_rate: u32,
    total_secs: u32,
    split_secs: u32,
    offset_start_secs: u32,
    offset_end_secs: u32,
) -> (PathBuf, PathBuf) {
    let total_samples = u64::from(sample_rate) * u64::from(total_secs);
    let split_samples = u64::from(sample_rate) * u64::from(split_secs);
    let delay_start = u64::from(sample_rate) * u64::from(offset_start_secs);
    let delay_end = u64::from(sample_rate) * u64::from(offset_end_secs);

    let path_a = dir.join("a.wav");
    let path_b = dir.join("b.wav");

    write_mono_wav(
        &path_a,
        sample_rate,
        (0..total_samples).map(|index| chirp_sample(sample_rate, index)),
    );

    write_mono_wav(
        &path_b,
        sample_rate,
        (0..total_samples).map(|index| {
            let delay = if index < split_samples {
                delay_start
            } else {
                delay_end
            };
            if index < delay {
                0
            } else {
                chirp_sample(sample_rate, index - delay)
            }
        }),
    );

    (path_a, path_b)
}

/// Near-silent PCM (fails energy gate; used for clip-skip corpus cases).
pub fn write_near_silence_wav_pair(
    dir: &Path,
    sample_rate: u32,
    seconds: u32,
) -> (PathBuf, PathBuf) {
    let total_samples = u64::from(sample_rate) * u64::from(seconds);
    let path_a = dir.join("a.wav");
    let path_b = dir.join("b.wav");
    write_mono_wav(&path_a, sample_rate, (0..total_samples).map(|_| 0_i16));
    write_mono_wav(&path_b, sample_rate, (0..total_samples).map(|_| 0_i16));
    (path_a, path_b)
}

/// Steady 440 Hz tone for negative-control corpus cases.
pub fn write_tone_wav(path: &Path, sample_rate: u32, seconds: u32) {
    write_tone_wav_at_frequency(path, sample_rate, seconds, 440.0);
}
