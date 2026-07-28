//! Characterization: the FFT resample path's group delay, and the fact that rubato reports it
//! exactly (M-RESAMPLE, measured 2026-07-27).
//!
//! Nothing in production compensates for this delay today. That is currently harmless because the
//! only way to reach an *asymmetric* resample is a pair whose two sources differ in sample rate,
//! which the corpus never exercises (every recorded track is 48 kHz). These tests exist so that
//! stays a deliberate, measured position rather than an assumption:
//!
//! * `fft_group_delay_matches_rubato_reported_output_delay` locks the property that makes the fix
//!   a two-line trim if a cross-rate pair ever shows up — `Resampler::output_delay()` is not an
//!   estimate here, it is the exact measured lag.
//! * `fft_and_linear_paths_disagree_on_group_delay` pins the *known* discrepancy between the FFT
//!   path and its linear fallback, so a rubato bump or a fallback rewrite cannot silently change
//!   which of the two a run gets.
//!
//! See `docs/dev/archive/TEMP-rust-review-findings.md` § M-RESAMPLE (archived closed; that
//! section lists the conditions under which this finding should be re-opened).

use clip_sync::Resampler as _;
use clip_sync::{MonoPcmClip, RubatoResampler};

/// Rate pairs spanning up/down conversion and both of the common broadcast/consumer rates.
const RATE_PAIRS: [(u32, u32); 4] = [
    (44_100, 48_000),
    (48_000, 44_100),
    (32_000, 48_000),
    (48_000, 96_000),
];

/// Group delay is small but not negligible; a value outside this band means the engine or its
/// configuration changed materially and the M-RESAMPLE analysis needs redoing.
const DELAY_BAND_MS: (f64, f64) = (1.0, 10.0);

/// Linear interpolation to `to_rate`, mirroring `linear_resample_fallback`'s math.
/// Linear interpolation has no group delay, so this doubles as the zero-delay reference.
fn linear_reference(input: &[i16], from_rate: u32, to_rate: u32) -> Vec<f64> {
    let out_len =
        ((input.len() as u64 * u64::from(to_rate)) / u64::from(from_rate)).max(1) as usize;
    (0..out_len)
        .map(|i| {
            let src = (i as f64 * f64::from(from_rate)) / f64::from(to_rate);
            let left = src.floor() as usize;
            let right = (left + 1).min(input.len() - 1);
            let frac = src - left as f64;
            let left_sample = f64::from(input[left.min(input.len() - 1)]);
            let right_sample = f64::from(input[right]);
            left_sample + (right_sample - left_sample) * frac
        })
        .collect()
}

/// Linear chirp 200 Hz -> 4 kHz. Broadband and non-repeating, so the correlation peak is unique.
fn chirp(rate: u32, secs: f64) -> Vec<i16> {
    let n = (f64::from(rate) * secs) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            let f = 200.0 + (4000.0 - 200.0) * (t / secs);
            (12000.0 * (std::f64::consts::TAU * f * t).sin()) as i16
        })
        .collect()
}

fn normalized_correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    let (mut num, mut den_a, mut den_b) = (0.0, 0.0, 0.0);
    for i in 0..n {
        num += a[i] * b[i];
        den_a += a[i] * a[i];
        den_b += b[i] * b[i];
    }
    if den_a == 0.0 || den_b == 0.0 {
        return 0.0;
    }
    num / (den_a.sqrt() * den_b.sqrt())
}

/// Integer lag of `probe` relative to `reference`, in samples. Positive = probe is delayed.
fn best_lag(reference: &[f64], probe: &[f64], max_lag: i64) -> (i64, f64) {
    let mut best = (0i64, f64::NEG_INFINITY);
    for lag in -max_lag..=max_lag {
        let (ref_start, probe_start) = if lag >= 0 {
            (0usize, lag as usize)
        } else {
            ((-lag) as usize, 0usize)
        };
        if ref_start >= reference.len() || probe_start >= probe.len() {
            continue;
        }
        let n = (reference.len() - ref_start).min(probe.len() - probe_start);
        if n < 1000 {
            continue;
        }
        let corr = normalized_correlation(
            &reference[ref_start..ref_start + n],
            &probe[probe_start..probe_start + n],
        );
        if corr > best.1 {
            best = (lag, corr);
        }
    }
    best
}

/// Measured FFT-path lag against the zero-delay linear reference, in output samples.
fn measured_delay_samples(from_rate: u32, to_rate: u32) -> (i64, f64) {
    let samples = chirp(from_rate, 2.0);
    let clip = MonoPcmClip {
        sample_rate: from_rate,
        samples: samples.clone(),
        decode_error_skips: 0,
        decoded_sample_count: None,
    };
    let fft = RubatoResampler.resample_mono(&clip, to_rate);
    let fft_f64: Vec<f64> = fft.samples.iter().map(|s| f64::from(*s)).collect();
    let reference = linear_reference(&samples, from_rate, to_rate);
    best_lag(&reference, &fft_f64, 4096)
}

/// Rubato's own reported delay for an engine built exactly as production builds it.
/// Mirrors `RESAMPLE_CHUNK_SIZE` / `RESAMPLE_SUB_CHUNKS` in `infrastructure/resample/rubato.rs`.
fn reported_delay_samples(from_rate: u32, to_rate: u32) -> usize {
    use rubato::Resampler as _;
    rubato::FftFixedIn::<f32>::new(from_rate as usize, to_rate as usize, 1024, 4, 1)
        .expect("production rate pairs must construct")
        .output_delay()
}

/// **M-RESAMPLE — `output_delay()` is exact, not an estimate.**
///
/// This is the load-bearing fact for the proposed fix: if a cross-rate pair ever needs
/// compensating, dropping `output_delay()` leading frames removes the lag completely. If this
/// test ever fails, the trim would be wrong and the fix must be re-derived from measurement.
#[test]
fn fft_group_delay_matches_rubato_reported_output_delay() {
    for (from_rate, to_rate) in RATE_PAIRS {
        let (measured, corr) = measured_delay_samples(from_rate, to_rate);
        let reported = reported_delay_samples(from_rate, to_rate);
        let delay_ms = measured as f64 / f64::from(to_rate) * 1000.0;

        println!(
            "{from_rate} -> {to_rate}: measured {measured} samples ({delay_ms:.3} ms, \
             peak corr {corr:.4}); rubato output_delay() = {reported}"
        );

        assert!(
            corr > 0.9,
            "{from_rate}->{to_rate}: correlation peak {corr:.4} too weak to trust the lag"
        );
        assert_eq!(
            measured, reported as i64,
            "{from_rate}->{to_rate}: measured group delay must equal rubato's reported \
             output_delay(); the M-RESAMPLE fix (drop output_delay() leading frames) depends on it"
        );
        assert!(
            (DELAY_BAND_MS.0..=DELAY_BAND_MS.1).contains(&delay_ms),
            "{from_rate}->{to_rate}: group delay {delay_ms:.3} ms left the characterized band \
             {DELAY_BAND_MS:?}; re-run the M-RESAMPLE analysis"
        );
    }
}

/// **M-RESAMPLE — the FFT path and its linear fallback do not agree on delay.**
///
/// `linear_resample_fallback` presents itself (via `warn!`) as a transparent substitute when the
/// rubato engine fails, but it has no group delay while the FFT path has several milliseconds. A
/// fallback therefore shifts the signal relative to a normal run. Pinned here so the discrepancy
/// is a known quantity rather than a surprise; compensating the FFT path to zero delay would make
/// the two agree and should flip this test's expectation.
#[test]
fn fft_and_linear_paths_disagree_on_group_delay() {
    let (from_rate, to_rate) = (44_100, 48_000);
    let (measured, _) = measured_delay_samples(from_rate, to_rate);
    assert!(
        measured > 0,
        "FFT path is expected to lag the linear fallback; measured {measured} samples. \
         If this is now 0, the delay was compensated — update M-RESAMPLE in the ledger."
    );
}
