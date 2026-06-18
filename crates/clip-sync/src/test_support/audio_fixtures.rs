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

/// Mono WAV using the same 300–700 Hz sweep as committed corpus chirp fixtures (`chirp_a.wav`).
pub fn write_corpus_chirp_wav(path: &Path, sample_rate: u32, total_secs: u32) {
    let total_samples = u64::from(sample_rate) * u64::from(total_secs);
    write_mono_wav(
        path,
        sample_rate,
        (0..total_samples).map(|index| chirp_sample(sample_rate, index)),
    );
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

/// Writes two mono WAV files with a short chirp segment tiled for the full duration.
///
/// Strongly self-similar content for hold-out verification probes: any window sees the same
/// repeating chirp pattern. File B is delayed by `offset_secs` of leading silence.
///
/// **Not a discovery oracle:** Chromaprint discovery aliases to any offset ≡ `offset_secs`
/// (mod 10 s loop period), e.g. true +3 s often reports ≈ +13 s. Hold-out verification
/// with Option A can also false-pass period-equivalent wrong Δ (+13 s when true is +3 s).
/// Use [`write_offset_chirp_wav_pair`] for alignment offset assertions; use this generator
/// only in dedicated verify probes (`corpus_verify_option_a_false_pass_probe`).
pub fn write_looped_chirp_wav_pair(
    dir: &Path,
    sample_rate: u32,
    total_secs: u32,
    offset_secs: u32,
) -> (PathBuf, PathBuf) {
    write_looped_chirp_wav_pair_with_delay(
        dir,
        sample_rate,
        total_secs,
        offset_secs,
        ChirpDelayOn::B,
    )
}

/// Looped chirp pair with leading silence on A or B (see [`ChirpDelayOn`]).
pub fn write_looped_chirp_wav_pair_with_delay(
    dir: &Path,
    sample_rate: u32,
    total_secs: u32,
    offset_secs: u32,
    delay_on: ChirpDelayOn,
) -> (PathBuf, PathBuf) {
    const LOOP_SECS: u32 = 10;

    let loop_samples = u64::from(sample_rate) * u64::from(LOOP_SECS);
    let total_samples = u64::from(sample_rate) * u64::from(total_secs);
    let delay_samples = u64::from(sample_rate) * u64::from(offset_secs);
    let path_a = dir.join("a.wav");
    let path_b = dir.join("b.wav");

    let samples_for = |delay: u64| {
        (0..total_samples).map(move |index| {
            if index < delay {
                0
            } else {
                chirp_sample(sample_rate, (index - delay) % loop_samples)
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

/// Writes two mono WAV files with pure 440 Hz tone repeats (no chirp background).
///
/// Each file has identical 10 s tone blocks at content positions [0..10 s] and [30..40 s].
/// File B is delayed by `offset_secs` of leading silence. Use a long enough `total_secs`
/// and clip window so both start clips include the repeated block when `offset_secs` > 0.
pub fn write_pure_tone_repeat_wav_pair(
    dir: &Path,
    sample_rate: u32,
    total_secs: u32,
    offset_secs: u32,
) -> (PathBuf, PathBuf) {
    const TONE_HZ: f32 = 440.0;
    const TONE_BLOCK_SECS: usize = 10;
    const REPEAT_AT_SECS: usize = 30;

    let total_n = sample_rate as usize * total_secs as usize;
    let block_n = sample_rate as usize * TONE_BLOCK_SECS;
    let repeat_at_n = sample_rate as usize * REPEAT_AT_SECS;
    let path_a = dir.join("a.wav");
    let path_b = dir.join("b.wav");

    let make_samples = |delay: usize| -> Vec<i16> {
        (0..total_n)
            .map(|i| {
                if i < delay {
                    return 0;
                }
                let ci = i - delay;
                let in_tone_block =
                    ci < block_n || (ci >= repeat_at_n && ci < repeat_at_n + block_n);
                if !in_tone_block {
                    return 0;
                }
                let t = ci as f32 / sample_rate as f32;
                ((TAU * TONE_HZ * t).sin() * (i16::MAX as f32 * 0.5)).round() as i16
            })
            .collect()
    };

    let delay = sample_rate as usize * offset_secs as usize;
    write_mono_wav(&path_a, sample_rate, make_samples(0));
    write_mono_wav(&path_b, sample_rate, make_samples(delay));
    (path_a, path_b)
}

/// Writes two mono WAV files that each contain an internal repeated segment.
///
/// Both files share identical 440 Hz tone blocks at [0..10 s] and [30..40 s] with silence
/// between (self-repetition at ~30 s lag). File B is delayed by `offset_secs` of leading
/// silence for cross-file alignment on the tone blocks.
pub fn write_repeated_segment_wav_pair(
    dir: &Path,
    sample_rate: u32,
    total_secs: u32,
    offset_secs: u32,
) -> (PathBuf, PathBuf) {
    const TONE_HZ: f32 = 440.0;
    const TONE_BLOCK_SECS: usize = 10;
    const REPEAT_AT_SECS: usize = 30;

    let total_n = sample_rate as usize * total_secs as usize;
    let block_n = sample_rate as usize * TONE_BLOCK_SECS;
    let repeat_at_n = sample_rate as usize * REPEAT_AT_SECS;
    let path_a = dir.join("a.wav");
    let path_b = dir.join("b.wav");

    let tone_block: Vec<i16> = (0..block_n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            ((TAU * TONE_HZ * t).sin() * (i16::MAX as f32 * 0.4)).round() as i16
        })
        .collect();

    let make_samples = |delay: usize| -> Vec<i16> {
        let mut samples = vec![0_i16; total_n];
        for (ci, sample) in samples[delay..].iter_mut().enumerate() {
            if ci < block_n {
                *sample = tone_block[ci];
            } else if ci >= repeat_at_n && ci < repeat_at_n + block_n {
                *sample = tone_block[ci - repeat_at_n];
            }
        }
        samples
    };

    let delay = sample_rate as usize * offset_secs as usize;
    write_mono_wav(&path_a, sample_rate, make_samples(0));
    write_mono_wav(&path_b, sample_rate, make_samples(delay));
    (path_a, path_b)
}

/// Bounded linear chirp: instantaneous frequency f0 + k·t stays below Nyquist for the whole
/// `sweep_secs` span, so long references do not alias into quasi-periodic (ambiguous) content.
/// Unlike [`chirp_sample`] (naive `300 + 400·t`, which aliases past ~6.5 s), every window stays
/// spectrally distinct — required for unambiguous query localization over minutes-long files.
fn bounded_chirp_sample(sample_rate: u32, index: u64, sweep_secs: f64) -> i16 {
    let rate = f64::from(sample_rate);
    let t = index as f64 / rate;
    let f0 = 200.0;
    let f1 = 0.45 * rate; // stay below Nyquist (rate / 2) across the whole sweep
    let k = (f1 - f0) / sweep_secs.max(1.0);
    let phase = TAU as f64 * (f0 * t + 0.5 * k * t * t);
    (phase.sin() * (f64::from(i16::MAX) * 0.5)).round() as i16
}

/// Long reference A (full chirp) + short query B (slice from A at `query_anchor_secs`).
///
/// Uses [`bounded_chirp_sample`] so the reference does not alias over long durations — the slice
/// localizes unambiguously (confidence ≈ 1.0) rather than tripping the repeated-content downgrade.
pub fn write_query_reference_chirp_pair(
    dir: &Path,
    sample_rate: u32,
    reference_secs: u32,
    query_anchor_secs: u32,
    query_duration_secs: u32,
) -> (PathBuf, PathBuf) {
    let path_a = dir.join("a.wav");
    let path_b = dir.join("b.wav");
    let sweep_secs = f64::from(reference_secs);
    let reference_samples = u64::from(sample_rate) * u64::from(reference_secs);
    write_mono_wav(
        &path_a,
        sample_rate,
        (0..reference_samples).map(|index| bounded_chirp_sample(sample_rate, index, sweep_secs)),
    );
    let start_index = u64::from(sample_rate) * u64::from(query_anchor_secs);
    let query_samples = u64::from(sample_rate) * u64::from(query_duration_secs);
    write_mono_wav(
        &path_b,
        sample_rate,
        (0..query_samples).map(|offset| bounded_chirp_sample(sample_rate, start_index + offset, sweep_secs)),
    );
    (path_a, path_b)
}

/// Short query A (slice from B at `query_anchor_secs`) + long reference B (full chirp).
///
/// Mirror of [`write_query_reference_chirp_pair`] for the B-longer repair scenario.
pub fn write_query_reference_b_longer_chirp_pair(
    dir: &Path,
    sample_rate: u32,
    reference_secs: u32,
    query_anchor_secs: u32,
    query_duration_secs: u32,
) -> (PathBuf, PathBuf) {
    let path_a = dir.join("a.wav");
    let path_b = dir.join("b.wav");
    let sweep_secs = f64::from(reference_secs);
    let reference_samples = u64::from(sample_rate) * u64::from(reference_secs);
    write_mono_wav(
        &path_b,
        sample_rate,
        (0..reference_samples).map(|index| bounded_chirp_sample(sample_rate, index, sweep_secs)),
    );
    let start_index = u64::from(sample_rate) * u64::from(query_anchor_secs);
    let query_samples = u64::from(sample_rate) * u64::from(query_duration_secs);
    write_mono_wav(
        &path_a,
        sample_rate,
        (0..query_samples).map(|offset| bounded_chirp_sample(sample_rate, start_index + offset, sweep_secs)),
    );
    (path_a, path_b)
}

/// Short excerpt **A** + long master **B** for symmetric anchored-end alignment tests.
///
/// - **A:** `shared_secs` of [`bounded_chirp_sample`] on `0..shared`.
/// - **B:** the same chirp through `shared_secs` (optional inter-file offset via [`ChirpDelayOn`]),
///   then silence through `long_secs`.
///
/// With `SharedTimeline` end anchoring, both end windows should land on `[shared − L, shared]`.
/// With legacy `FileTail` on B, B's end window sits at `[long − L, long]` (unrelated tail audio).
pub fn write_anchored_end_symmetric_pair(
    dir: &Path,
    sample_rate: u32,
    shared_secs: u32,
    long_secs: u32,
    offset_secs: u32,
) -> (PathBuf, PathBuf) {
    write_anchored_end_symmetric_pair_with_delay(
        dir,
        sample_rate,
        shared_secs,
        long_secs,
        offset_secs,
        ChirpDelayOn::B,
    )
}

/// [`write_anchored_end_symmetric_pair`] with explicit delay role (see [`ChirpDelayOn`]).
pub fn write_anchored_end_symmetric_pair_with_delay(
    dir: &Path,
    sample_rate: u32,
    shared_secs: u32,
    long_secs: u32,
    offset_secs: u32,
    delay_on: ChirpDelayOn,
) -> (PathBuf, PathBuf) {
    assert!(
        shared_secs <= long_secs,
        "shared_secs ({shared_secs}) must be <= long_secs ({long_secs})"
    );

    let path_a = dir.join("a.wav");
    let path_b = dir.join("b.wav");
    let sweep_secs = f64::from(long_secs);
    let rate = u64::from(sample_rate);
    let shared_samples = rate * u64::from(shared_secs);
    let long_samples = rate * u64::from(long_secs);
    let delay_samples = rate * u64::from(offset_secs);

    let a_chirp_start = match delay_on {
        ChirpDelayOn::B => 0,
        ChirpDelayOn::A => delay_samples,
    };
    write_mono_wav(
        &path_a,
        sample_rate,
        (0..shared_samples).map(|index| {
            if index < a_chirp_start {
                0
            } else {
                bounded_chirp_sample(sample_rate, index - a_chirp_start, sweep_secs)
            }
        }),
    );

    let b_chirp_start = match delay_on {
        ChirpDelayOn::B => delay_samples,
        ChirpDelayOn::A => 0,
    };
    let b_chirp_end = b_chirp_start.saturating_add(shared_samples);
    write_mono_wav(
        &path_b,
        sample_rate,
        (0..long_samples).map(|index| {
            if index < b_chirp_start {
                0
            } else if index < b_chirp_end {
                bounded_chirp_sample(sample_rate, index - b_chirp_start, sweep_secs)
            } else {
                0
            }
        }),
    );

    (path_a, path_b)
}

#[cfg(test)]
mod anchored_end_symmetric_tests {
    use super::*;
    use hound::WavReader;

    fn wav_duration_secs(path: &Path, sample_rate: u32) -> f64 {
        let reader = WavReader::open(path).expect("open wav");
        reader.len() as f64 / f64::from(sample_rate)
    }

    #[test]
    fn anchored_end_symmetric_pair_writes_expected_durations() {
        use crate::application::testing::anchored_end_oracles::{
            CI_LONG_SECS, CI_SAMPLE_RATE, CI_SHARED_SECS,
        };

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = write_anchored_end_symmetric_pair(
            temp.path(),
            CI_SAMPLE_RATE,
            CI_SHARED_SECS,
            CI_LONG_SECS,
            0,
        );
        assert!(
            (wav_duration_secs(&path_a, CI_SAMPLE_RATE) - f64::from(CI_SHARED_SECS)).abs() < 0.01
        );
        assert!(
            (wav_duration_secs(&path_b, CI_SAMPLE_RATE) - f64::from(CI_LONG_SECS)).abs() < 0.01
        );
    }

    #[test]
    fn anchored_end_symmetric_pair_b_delay_offsets_chirp_block() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sample_rate = 11_025;
        let offset_secs = 3;
        let (path_a, path_b) = write_anchored_end_symmetric_pair_with_delay(
            temp.path(),
            sample_rate,
            60,
            300,
            offset_secs,
            ChirpDelayOn::B,
        );
        let mut reader_a = WavReader::open(&path_a).expect("open a");
        let mut reader_b = WavReader::open(&path_b).expect("open b");
        let delay_samples = u64::from(sample_rate) * u64::from(offset_secs);

        let samples_a: Vec<i16> = reader_a
            .samples()
            .map(|s| s.expect("sample"))
            .collect();
        let samples_b: Vec<i16> = reader_b
            .samples()
            .map(|s| s.expect("sample"))
            .collect();

        let a_lead: i32 = samples_a.iter().take(100).map(|s| s.unsigned_abs() as i32).sum();
        let b_lead: i32 = samples_b
            .iter()
            .take(delay_samples as usize)
            .map(|s| s.unsigned_abs() as i32)
            .sum();
        assert!(a_lead > 0);
        assert_eq!(b_lead, 0);

        // A's chirp at t lines up with B's chirp at t + offset on B's timeline.
        let a_head = &samples_a[..100];
        let b_aligned = &samples_b[delay_samples as usize..delay_samples as usize + 100];
        assert_eq!(a_head, b_aligned);
    }
}
