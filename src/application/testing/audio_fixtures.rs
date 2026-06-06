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

/// Writes two mono WAV files with the same chirp; `b` starts `offset_secs` later than `a`.
pub fn write_offset_chirp_wav_pair(
    dir: &Path,
    sample_rate: u32,
    total_secs: u32,
    offset_secs: u32,
) -> (PathBuf, PathBuf) {
    let total_samples = u64::from(sample_rate) * u64::from(total_secs);
    let delay_samples = u64::from(sample_rate) * u64::from(offset_secs);

    let path_a = dir.join("a.wav");
    let samples_a = (0..total_samples).map(|index| chirp_sample(sample_rate, index));
    write_mono_wav(&path_a, sample_rate, samples_a);

    let path_b = dir.join("b.wav");
    let samples_b = (0..total_samples).map(|index| {
        if index < delay_samples {
            0
        } else {
            chirp_sample(sample_rate, index - delay_samples)
        }
    });
    write_mono_wav(&path_b, sample_rate, samples_b);

    (path_a, path_b)
}
