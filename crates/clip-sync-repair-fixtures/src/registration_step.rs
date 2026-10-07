//! Step-kind registration fixture for the dual-fit routing tests
//! (`docs/dev/TEMP-dual-fit-registration-routing-plan.md` § 5).
//!
//! A is white noise with a silent hole; B is the same noise whose lag *steps* at the hole's midpoint
//! (`pre_shift_secs` before it, `post_shift_secs` after), so no single lag registers both shoulders.

use std::path::Path;

use crate::energy_signature_fixtures::write_pcm_wav;

/// Geometry of a step fixture. Shifts are B-leads-A in seconds: `B[t] = A[t + shift]`.
#[derive(Debug, Clone, Copy)]
pub struct RegistrationStepSpec {
    pub sample_rate: u32,
    pub total_secs: f64,
    pub gap_start_secs: f64,
    pub gap_end_secs: f64,
    pub pre_shift_secs: f64,
    pub post_shift_secs: f64,
    pub seed: u32,
    pub amplitude: f32,
    /// `Some(f0)`: a harmonic tone at `f0` Hz under a ~25 ms-block random envelope instead of white noise.
    /// A step that is a whole number of periods leaves the waveform seam passing at the wrong lag (the gate
    /// passes a misregistered fill) while the envelope still marks the step.
    pub periodic_hz: Option<f64>,
    /// `Some((start, end))`: B is silent over that absolute-time span (seconds). Placed in the gap interior it
    /// leaves both shoulders intact — the single-lag seams still pass — but makes the donor bridge
    /// discontinuous, so dual-fit declines.
    pub b_dropout_secs: Option<(f64, f64)>,
}

fn noise(seed: u32, idx: i64, amplitude: f32) -> f32 {
    let mut x = idx.wrapping_add(i64::from(seed)) as u64;
    x = x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    let unit = (x >> 40) as f64 / f64::from(1u32 << 24);
    ((unit * 2.0 - 1.0) as f32) * amplitude
}

fn sample(spec: &RegistrationStepSpec, idx: i64) -> f32 {
    let Some(f0) = spec.periodic_hz else {
        return noise(spec.seed, idx, spec.amplitude);
    };
    let sr = f64::from(spec.sample_rate);
    let secs = idx as f64 / sr;
    let tone: f64 = (1..=12)
        .map(|h| (std::f64::consts::TAU * f0 * f64::from(h) * secs).sin() / f64::from(h))
        .sum();
    // Linear interpolation between per-block random gains (blocks of ~25 ms).
    let block = (0.025 * sr) as i64;
    let (k, frac) = (
        idx.div_euclid(block),
        idx.rem_euclid(block) as f32 / block as f32,
    );
    let g = |k: i64| 0.4 + 0.6 * (noise(spec.seed ^ 0x55AA, k, 1.0).abs());
    let env = g(k) * (1.0 - frac) + g(k + 1) * frac;
    (tone as f32) * 0.25 * env * spec.amplitude * 2.0
}

/// Write `a_path` (noise, silent hole) and `b_path` (noise with the stepped lag, no hole), stereo.
pub fn write_registration_step_pair(spec: &RegistrationStepSpec, a_path: &Path, b_path: &Path) {
    let sr = f64::from(spec.sample_rate);
    let total = (spec.total_secs * sr).round() as i64;
    let gap_start = (spec.gap_start_secs * sr).round() as i64;
    let gap_end = (spec.gap_end_secs * sr).round() as i64;
    let step_at = (gap_start + gap_end) / 2;
    let pre = (spec.pre_shift_secs * sr).round() as i64;
    let post = (spec.post_shift_secs * sr).round() as i64;

    let dropout = spec
        .b_dropout_secs
        .map(|(s, e)| ((s * sr).round() as i64, (e * sr).round() as i64));

    let mut a = Vec::with_capacity(total as usize * 2);
    let mut b = Vec::with_capacity(total as usize * 2);
    for t in 0..total {
        let in_gap = t >= gap_start && t < gap_end;
        let a_s = if in_gap { 0.0 } else { sample(spec, t) };
        let shift = if t < step_at { pre } else { post };
        let in_dropout = dropout.is_some_and(|(s, e)| t >= s && t < e);
        let b_s = if in_dropout {
            0.0
        } else {
            sample(spec, t + shift)
        };
        a.extend([a_s, a_s]);
        b.extend([b_s, b_s]);
    }
    write_pcm_wav(a_path, spec.sample_rate, 2, &a);
    write_pcm_wav(b_path, spec.sample_rate, 2, &b);
}
