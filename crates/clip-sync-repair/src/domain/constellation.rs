//! A/B constellation lag histogram — onset picker + `L`-histogram uniqueness probe.
//!
//! Fingerprint-diagnostic experiment (`docs/dev/TEMP-constellation-fingerprint-plan.md`). §0 of
//! that plan is normative; this module implements §2 rules 2–4 (the onset picker, `L` as a lag
//! not an interval, and the redraw-decoy uniqueness test) as pure, decode-free primitives so they
//! are unit-testable against the §3 synthetic fixture before any corpus wiring exists.
//!
//! Not `crate::domain::gap_anchor_seam::energy_peak_candidates` (3-point neighbor prominence on
//! full-band bins) and not `list_anchor_candidates_a` (A-only scan fallback) — this picker is
//! high-passed, uses a wider neighborhood-median prominence, and this module votes a two-sided
//! `L` lag, which those do not.

use crate::domain::pcm::interleaved_to_mono;

/// One picked onset: time from the start of the window it was picked in (seconds), and picker
/// prominence (peak minus neighborhood median — not the 3-point prominence in
/// `gap_anchor_seam::energy_peak_candidates`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnsetEvent {
    pub time_secs: f64,
    pub prominence: f32,
}

/// Onset-picker geometry (§2 rule 2). `first_candidate` values are the plan's first candidate —
/// reopen only from a failed §3 fixture, never from corpus taste.
#[derive(Debug, Clone, Copy)]
pub struct PickerParams {
    pub sample_rate: u32,
    /// One-pole high-pass corner (Hz).
    pub highpass_hz: f64,
    /// Bin width (ms) the log-RMS envelope is built at.
    pub bin_ms: f64,
    /// Neighborhood (ms) the prominence baseline is measured over, centered on the peak.
    pub neighborhood_ms: f64,
    /// Core (ms) excluded from the neighborhood median, centered on the peak.
    pub core_ms: f64,
    /// Kept events per window, prominence-desc. No write floor — cap is the only filter.
    pub cap: usize,
}

impl PickerParams {
    pub fn first_candidate(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            highpass_hz: 2_000.0,
            bin_ms: 10.0,
            neighborhood_ms: 200.0,
            core_ms: 30.0,
            cap: 12,
        }
    }
}

/// One-pole high-pass, corner at `corner_hz`. Broadband onsets (clicks, plosives) survive; slow
/// envelope drift and low-frequency bed ripple do not.
fn highpass(mono: &[f64], sample_rate: u32, corner_hz: f64) -> Vec<f64> {
    if mono.len() < 2 || corner_hz <= 0.0 {
        return mono.to_vec();
    }
    let dt = 1.0 / f64::from(sample_rate).max(1.0);
    let rc = 1.0 / (2.0 * std::f64::consts::PI * corner_hz);
    let alpha = rc / (rc + dt);
    let mut out = Vec::with_capacity(mono.len());
    out.push(0.0);
    let mut prev_in = mono[0];
    let mut prev_out = 0.0f64;
    for &x in &mono[1..] {
        let y = alpha * (prev_out + x - prev_in);
        out.push(y);
        prev_in = x;
        prev_out = y;
    }
    out
}

/// Per-bin gated-nothing log-RMS (no silence floor — the picker has no write floor).
fn log_rms_bins(x: &[f64], bin_frames: usize) -> Vec<f32> {
    let bin_frames = bin_frames.max(1);
    x.chunks(bin_frames)
        .map(|c| {
            let ms = c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64;
            (1.0 + ms.sqrt()).ln() as f32
        })
        .collect()
}

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values[values.len() / 2]
}

/// Pick onsets in one window: high-pass → 10 ms log-RMS → neighborhood-prominence local-max →
/// cap. `samples` is interleaved PCM for the window only (the A one-sided template, or a slice of
/// the B haystack) — the picker never sees anything outside what it is given.
pub fn pick_onsets(samples: &[f32], channels: usize, params: &PickerParams) -> Vec<OnsetEvent> {
    let mono = interleaved_to_mono(samples, channels);
    let filtered = highpass(&mono, params.sample_rate, params.highpass_hz);
    let bin_frames = ((params.bin_ms / 1000.0) * f64::from(params.sample_rate))
        .round()
        .max(1.0) as usize;
    let bins = log_rms_bins(&filtered, bin_frames);
    if bins.len() < 3 {
        return Vec::new();
    }

    let neighborhood_half_bins = ((params.neighborhood_ms / params.bin_ms) / 2.0)
        .round()
        .max(1.0) as usize;
    // `.floor()`, not `.round()`: at the spec defaults (30 ms core / 10 ms bin) `(30/10)/2 = 1.5`,
    // and rounding that up to 2 would widen the excluded core to 5 bins (50 ms) instead of the
    // specified 3 bins (30 ms).
    let core_half_bins = ((params.core_ms / params.bin_ms) / 2.0).floor().max(0.0) as usize;

    let mut candidates: Vec<(usize, f32)> = Vec::new();
    for i in 1..bins.len() - 1 {
        let v = bins[i];
        if v <= bins[i - 1] || v <= bins[i + 1] {
            continue;
        }
        let lo = i.saturating_sub(neighborhood_half_bins);
        let hi = (i + neighborhood_half_bins).min(bins.len() - 1);
        let core_lo = i.saturating_sub(core_half_bins);
        let core_hi = (i + core_half_bins).min(bins.len() - 1);
        let mut baseline: Vec<f32> = (lo..=hi)
            .filter(|&j| j < core_lo || j > core_hi)
            .map(|j| bins[j])
            .collect();
        if baseline.is_empty() {
            continue;
        }
        let prominence = v - median(&mut baseline);
        candidates.push((i, prominence));
    }

    // min-spacing = picker bin: adjacent bins can never both be strict local maxima, so no extra
    // suppression is needed beyond the local-max test above.
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    candidates.truncate(params.cap);

    let bin_secs = params.bin_ms / 1000.0;
    candidates
        .into_iter()
        .map(|(i, prominence)| OnsetEvent {
            time_secs: i as f64 * bin_secs,
            prominence,
        })
        .collect()
}

/// `L`-histogram + redraw-decoy geometry (§2 rules 3–4). `first_candidate`'s `vote_bin_ms`
/// mirrors `DUALFIT_SEAM_UNIQ_LAG_MS` (30 ms) — kept as its own literal here rather than importing
/// the private fingerprint-measure constant, since this is decode-free domain code.
#[derive(Debug, Clone, Copy)]
pub struct LagHistogramParams {
    pub vote_bin_ms: f64,
    pub redraw_trials: usize,
    pub redraw_percentile: f64,
    /// Minimum spacing (seconds) a legal picker output must respect — enforced only on the
    /// redraw's minted B times (§2 rule 4: "the null is a legal picker output"), never on real
    /// picked events.
    pub min_spacing_secs: f64,
}

impl LagHistogramParams {
    /// `min_spacing_secs` is derived from `picker.bin_ms` rather than duplicated as its own
    /// literal: "min-spacing = picker bin" is the picker's own invariant (see `pick_onsets`'s
    /// spacing comment), and the redraw null (§2 rule 4) has to respect the same floor a real
    /// picker output would — hardcoding a second copy would let the two silently desync under a
    /// future retune.
    pub fn first_candidate(picker: &PickerParams) -> Self {
        Self {
            vote_bin_ms: 30.0,
            redraw_trials: 100,
            redraw_percentile: 95.0,
            min_spacing_secs: picker.bin_ms / 1000.0,
        }
    }
}

/// Per-shoulder verdict (§5's `reason` field). `Unmeasured` (no B / projector-dropped) is not
/// produced here — that state belongs to the fingerprint-measure wiring, not this pure function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockReason {
    Locked,
    NoTrain,
    NotUnique,
    EdgePinned,
    /// The redraw null (§2 rule 4) has no legal arrangement at all: `b_span` is too narrow to fit
    /// `n_b` points respecting `min_spacing_secs` (`(n_b - 1) * min_spacing_secs > width`). Distinct
    /// from `NotUnique` — that means the null was built and didn't lose; this means the null could
    /// not be built.
    RedrawInfeasible,
}

#[derive(Debug, Clone)]
pub struct LagLockResult {
    pub n_a: usize,
    pub n_b: usize,
    pub hist_peak_lag_ms: Option<f64>,
    pub hist_peak_votes: Option<usize>,
    pub hist_second_votes: Option<usize>,
    pub redraw_p95_votes: Option<usize>,
    pub reason: LockReason,
}

/// All-pairs `L = t_B - t_A` vote histogram over `[lo_l, lo_l + num_bins * bin_secs)`.
///
/// Permutation-invariant by construction (every pair is visited regardless of order) — a caller
/// that permutes `b_times` gets the identical histogram, which is exactly why permuting detected
/// B times is not the decoy (§2 rule 4; see the `permuting_b_times_does_not_change_hist_peak_votes`
/// test below).
fn vote_histogram(
    a_times: &[f64],
    b_times: &[f64],
    lo_l: f64,
    bin_secs: f64,
    num_bins: usize,
) -> Vec<usize> {
    let mut hist = vec![0usize; num_bins];
    for &a in a_times {
        for &b in b_times {
            let idx = ((b - a - lo_l) / bin_secs).floor();
            if idx >= 0.0 && (idx as usize) < num_bins {
                hist[idx as usize] += 1;
            }
        }
    }
    hist
}

fn mode_and_second(hist: &[usize]) -> Option<(usize, usize, Option<usize>)> {
    let mut idx_counts: Vec<(usize, usize)> = hist.iter().copied().enumerate().collect();
    idx_counts.sort_by_key(|&(_, c)| std::cmp::Reverse(c));
    let &(mode_idx, mode_count) = idx_counts.first()?;
    let second = idx_counts.get(1).map(|&(_, c)| c);
    Some((mode_idx, mode_count, second))
}

fn percentile(sorted: &[usize], pct: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((pct / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

/// Deterministic small PRNG for the redraw null (no external `rand` dependency in domain code;
/// same LCG shape already used by this crate's synthetic-media unit tests, e.g. `domain::dual_fit`).
struct Lcg(u64);
impl Lcg {
    fn next_unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

/// Mint `n_b` new times uniformly inside `span`, conditioned on respecting `min_spacing_secs`
/// (§2 rule 4: "the null is a legal picker output").
///
/// Uses the direct spacings transform rather than reject-and-retry: draw `n_b` iid uniforms on
/// the slack `[0, width - (n_b-1)*min_spacing_secs]`, sort them, then re-inflate the `i`-th by
/// `i * min_spacing_secs`. That transform is a measure-preserving bijection onto the feasible
/// spaced-arrangement region, so it is a genuinely uniform sample with zero rejections — unlike
/// reject-and-retry, whose acceptance probability is `(1 - (n_b-1)*min_spacing_secs/width)^n_b`
/// and decays combinatorially fast as `width` approaches the feasibility floor (e.g. `n_b=12`,
/// `min_spacing_secs=0.01`: ~1-in-625 at `width=250ms`, ~1-in-3,000,000 at `width=150ms` — no
/// fixed retry budget covers that reliably, and a fixed-point fallback silently returns the same
/// non-random arrangement every trial when it doesn't, degrading the redraw's 100 "independent"
/// trials into one repeated sample).
///
/// Returns `None` when no legal arrangement exists at all (`(n_b-1)*min_spacing_secs > width`) —
/// the caller must treat that as "the null could not be constructed," not substitute something
/// that may itself violate the spacing floor.
fn redraw_b_times(
    rng: &mut Lcg,
    n_b: usize,
    span: (f64, f64),
    min_spacing_secs: f64,
) -> Option<Vec<f64>> {
    let (lo, hi) = span;
    let width = (hi - lo).max(0.0);
    if n_b == 0 {
        return Some(Vec::new());
    }
    let required = min_spacing_secs * (n_b - 1) as f64;
    if required > width {
        return None;
    }
    let slack = width - required;
    let mut offsets: Vec<f64> = (0..n_b).map(|_| rng.next_unit() * slack).collect();
    offsets.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(
        offsets
            .into_iter()
            .enumerate()
            .map(|(i, off)| lo + off + (i as f64) * min_spacing_secs)
            .collect(),
    )
}

/// Evaluate the `L` histogram for one shoulder and test it against the redraw decoy (§2 rules
/// 3–4). `b_span` is the searched B haystack span (§2 rule 7) — it bounds both the histogram
/// range and the redraw's draw range.
pub fn evaluate_l_histogram(
    a_times: &[f64],
    b_times: &[f64],
    b_span: (f64, f64),
    params: &LagHistogramParams,
    redraw_seed: u64,
) -> LagLockResult {
    let n_a = a_times.len();
    let n_b = b_times.len();
    if n_a < 2 || n_b < 2 {
        return LagLockResult {
            n_a,
            n_b,
            hist_peak_lag_ms: None,
            hist_peak_votes: None,
            hist_second_votes: None,
            redraw_p95_votes: None,
            reason: LockReason::NoTrain,
        };
    }

    let bin_secs = (params.vote_bin_ms / 1000.0).max(1e-9);
    let a_min = a_times.iter().copied().fold(f64::INFINITY, f64::min);
    let a_max = a_times.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let lo_l = b_span.0 - a_max;
    let hi_l = b_span.1 - a_min;
    let num_bins = (((hi_l - lo_l) / bin_secs).ceil().max(1.0)) as usize;

    let hist = vote_histogram(a_times, b_times, lo_l, bin_secs, num_bins);
    let Some((mode_idx, mode_count, second)) = mode_and_second(&hist) else {
        return LagLockResult {
            n_a,
            n_b,
            hist_peak_lag_ms: None,
            hist_peak_votes: None,
            hist_second_votes: None,
            redraw_p95_votes: None,
            reason: LockReason::NotUnique,
        };
    };

    // "Not within one vote_bin of the histogram edge" (§2 rule 4): valid interior bins are
    // [1, num_bins - 2]; anything else (including a too-narrow histogram) is edge-pinned. This is
    // a purely geometric fact about the mode's position — decided before, and independent of,
    // whether a redraw null can even be constructed below.
    let edge_pinned = num_bins < 3 || mode_idx == 0 || mode_idx == num_bins - 1;
    let hist_peak_lag_ms = (lo_l + (mode_idx as f64 + 0.5) * bin_secs) * 1000.0;
    if edge_pinned {
        return LagLockResult {
            n_a,
            n_b,
            hist_peak_lag_ms: Some(hist_peak_lag_ms),
            hist_peak_votes: Some(mode_count),
            hist_second_votes: second,
            redraw_p95_votes: None,
            reason: LockReason::EdgePinned,
        };
    }

    let mut rng = Lcg(redraw_seed | 1);
    let mut trial_maxes: Vec<usize> = Vec::with_capacity(params.redraw_trials);
    for _ in 0..params.redraw_trials {
        let Some(redrawn) = redraw_b_times(&mut rng, n_b, b_span, params.min_spacing_secs) else {
            return LagLockResult {
                n_a,
                n_b,
                hist_peak_lag_ms: Some(hist_peak_lag_ms),
                hist_peak_votes: Some(mode_count),
                hist_second_votes: second,
                redraw_p95_votes: None,
                reason: LockReason::RedrawInfeasible,
            };
        };
        let trial_hist = vote_histogram(a_times, &redrawn, lo_l, bin_secs, num_bins);
        trial_maxes.push(trial_hist.iter().copied().max().unwrap_or(0));
    }
    trial_maxes.sort_unstable();
    let redraw_p95_votes = percentile(&trial_maxes, params.redraw_percentile);

    let reason = if mode_count > redraw_p95_votes {
        LockReason::Locked
    } else {
        LockReason::NotUnique
    };

    LagLockResult {
        n_a,
        n_b,
        hist_peak_lag_ms: Some(hist_peak_lag_ms),
        hist_peak_votes: Some(mode_count),
        hist_second_votes: second,
        redraw_p95_votes: Some(redraw_p95_votes),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRng(u64);
    impl TestRng {
        fn next_signed(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (((self.0 >> 11) as f64) / ((1u64 << 53) as f64)) * 2.0 - 1.0
        }
    }

    /// Sub-highpass-corner bed ripple (a few Hz, well below the picker's 2 kHz corner) the picker
    /// must not mistake for onsets — unlike broadband noise, this is what the high-pass actually
    /// exists to reject.
    fn noise_bed(rng: &mut TestRng, len: usize, amp: f32, sample_rate: u32) -> Vec<f32> {
        let f1 = 3.0 + rng.next_signed().abs() * 2.0;
        let f2 = 7.0 + rng.next_signed().abs() * 3.0;
        let sr = f64::from(sample_rate);
        (0..len)
            .map(|i| {
                let t = i as f64 / sr;
                let v = (2.0 * std::f64::consts::PI * f1 * t).sin() * 0.6
                    + (2.0 * std::f64::consts::PI * f2 * t).sin() * 0.4;
                (v as f32) * amp
            })
            .collect()
    }

    fn write_click(samples: &mut [f32], at: usize, width: usize, amp: f32) {
        let end = (at + width).min(samples.len());
        for s in &mut samples[at..end] {
            *s = amp;
        }
    }

    /// Shared A/B click-train fixture used by both the lock test and the edge-pinned test, so the
    /// edge-pinned case is a genuine "same fixture, narrower search span" scenario rather than a
    /// hand-rolled setup that only resembles one.
    struct GolombFixture {
        a_times: Vec<f64>,
        b_times: Vec<f64>,
        b_len_secs: f64,
    }

    fn build_golomb_fixture(seed: u64, injected_lag_secs: f64) -> GolombFixture {
        let sr = 8_000u32;
        let mut rng = TestRng(seed);
        // Only the *matching* A/B pair at each onset votes the true lag; a mismatched pair votes
        // `true_lag + (onset_j - onset_i)`. Evenly spaced onsets alias those mismatched votes back
        // onto a handful of bins (the same periodic-ambiguity trap `lag_window_secs`'s doc comment
        // warns about) and can rival the true peak. An order-10 Golomb ruler has all pairwise
        // differences distinct, so every mismatched pair lands in its own bin and the true-lag bin
        // is the only one with more than one vote.
        const RULER_MARKS: [usize; 10] = [0, 1, 4, 9, 15, 22, 32, 34, 43, 60];
        const RULER_SCALE_FRAMES: usize = 100;
        const RULER_START_FRAMES: usize = 200;
        let injected_frames: Vec<usize> = RULER_MARKS
            .iter()
            .map(|&m| RULER_START_FRAMES + m * RULER_SCALE_FRAMES)
            .collect();
        let a_len = RULER_START_FRAMES + 60 * RULER_SCALE_FRAMES + 400;
        let mut a = noise_bed(&mut rng, a_len, 0.01, sr);
        for &f in &injected_frames {
            write_click(&mut a, f, 20, 0.9);
        }

        let lag_frames = (injected_lag_secs * f64::from(sr)).round() as usize;
        let b_len = a_len + lag_frames + 200;
        let mut b = noise_bed(&mut rng, b_len, 0.01, sr);
        for &f in &injected_frames {
            write_click(&mut b, f + lag_frames, 20, 0.9);
        }

        let params = PickerParams::first_candidate(sr);
        let a_times: Vec<f64> = pick_onsets(&a, 1, &params)
            .iter()
            .map(|e| e.time_secs)
            .collect();
        let b_times: Vec<f64> = pick_onsets(&b, 1, &params)
            .iter()
            .map(|e| e.time_secs)
            .collect();
        GolombFixture {
            a_times,
            b_times,
            b_len_secs: b_len as f64 / f64::from(sr),
        }
    }

    /// §3 case 1, picker half: injected onsets survive the cap and bed ripple does not displace
    /// them.
    #[test]
    fn picker_recovers_injected_onsets_with_bed_ripple() {
        let sr = 8_000u32;
        let mut rng = TestRng(0xC0FFEE);
        let len = 4_000usize; // 500 ms
        let mut samples = noise_bed(&mut rng, len, 0.01, sr);
        let injected_frames = [800usize, 1_800, 2_900];
        for &f in &injected_frames {
            write_click(&mut samples, f, 20, 0.9);
        }

        let params = PickerParams::first_candidate(sr);
        let picked = pick_onsets(&samples, 1, &params);
        assert!(!picked.is_empty(), "expected onsets, got none");

        let bin_secs = params.bin_ms / 1000.0;
        for &f in &injected_frames {
            let injected_secs = f as f64 / f64::from(sr);
            assert!(
                picked
                    .iter()
                    .any(|e| (e.time_secs - injected_secs).abs() <= bin_secs),
                "expected a picked onset within one bin of {injected_secs}s: {picked:?}"
            );
        }
    }

    /// §3 case 1, lock half: the `L` histogram unique-locks at the injected lag within one
    /// `vote_bin`.
    #[test]
    fn l_histogram_locks_at_injected_lag_within_one_vote_bin() {
        let injected_lag_secs = 0.35; // inside a generous max_lag
        let fixture = build_golomb_fixture(0xBEEF_1234, injected_lag_secs);

        let picker_params = PickerParams::first_candidate(8_000);
        let hist_params = LagHistogramParams::first_candidate(&picker_params);
        let b_span = (0.0, fixture.b_len_secs);
        let result =
            evaluate_l_histogram(&fixture.a_times, &fixture.b_times, b_span, &hist_params, 42);

        assert_eq!(result.reason, LockReason::Locked, "{result:?}");
        let peak_ms = result
            .hist_peak_lag_ms
            .expect("locked result carries a peak");
        assert!(
            (peak_ms - injected_lag_secs * 1000.0).abs() <= hist_params.vote_bin_ms,
            "expected peak near {}ms, got {peak_ms}ms",
            injected_lag_secs * 1000.0
        );
    }

    /// §3 case 2 (A-only onsets): B has no shared onsets — must not unique-lock.
    #[test]
    fn a_only_onsets_do_not_unique_lock() {
        let sr = 8_000u32;
        let mut rng = TestRng(0xA11CE);
        let a_len = 4_000usize;
        let mut a = noise_bed(&mut rng, a_len, 0.01, sr);
        for &f in &[800usize, 1_800, 2_900] {
            write_click(&mut a, f, 20, 0.9);
        }
        let b_len = 4_000usize;
        let b = noise_bed(&mut rng, b_len, 0.01, sr); // bed only, no clicks

        let params = PickerParams::first_candidate(sr);
        let a_times: Vec<f64> = pick_onsets(&a, 1, &params)
            .iter()
            .map(|e| e.time_secs)
            .collect();
        let b_times: Vec<f64> = pick_onsets(&b, 1, &params)
            .iter()
            .map(|e| e.time_secs)
            .collect();

        let hist_params = LagHistogramParams::first_candidate(&params);
        let b_span = (0.0, b_len as f64 / f64::from(sr));
        let result = evaluate_l_histogram(&a_times, &b_times, b_span, &hist_params, 7);
        assert_ne!(
            result.reason,
            LockReason::Locked,
            "bed-only B must not unique-lock: {result:?}"
        );
    }

    /// §3 case 3 (geometry): the true lag sits outside the searched B span, so the mode has no
    /// interior support — edge-pinned, not a lock.
    ///
    /// Same click-train fixture the lock test uses (confirmed to lock under its natural span,
    /// below), but re-evaluated against a search span deliberately shrunk so the true-lag bin
    /// falls on the histogram's edge (`mode_idx == 0`) instead of an interior bin. This is what
    /// "the true lag sits outside/at-the-edge-of the searched span" actually means for
    /// `evaluate_l_histogram`: the votes for genuine matching pairs below `lo_l` are dropped by
    /// `vote_histogram`'s range check, leaving no interior support for the true lag.
    #[test]
    fn out_of_reach_lag_is_edge_pinned_not_locked() {
        let injected_lag_secs = 0.35;
        let fixture = build_golomb_fixture(0xFACE, injected_lag_secs);
        let picker_params = PickerParams::first_candidate(8_000);
        let hist_params = LagHistogramParams::first_candidate(&picker_params);

        let natural_b_span = (0.0, fixture.b_len_secs);
        let natural = evaluate_l_histogram(
            &fixture.a_times,
            &fixture.b_times,
            natural_b_span,
            &hist_params,
            99,
        );
        assert_eq!(
            natural.reason,
            LockReason::Locked,
            "fixture sanity check (must lock under its natural span): {natural:?}"
        );
        let peak_secs = natural
            .hist_peak_lag_ms
            .expect("locked result carries a peak")
            / 1000.0;

        let a_max = fixture
            .a_times
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let bin_secs = hist_params.vote_bin_ms / 1000.0;
        // `lo_l = b_span.0 - a_max`, so this pins the true-lag vote into bin index 0: right at
        // the edge instead of interior.
        let shrunk_b_span = (a_max + peak_secs - bin_secs * 0.4, fixture.b_len_secs);
        let result = evaluate_l_histogram(
            &fixture.a_times,
            &fixture.b_times,
            shrunk_b_span,
            &hist_params,
            99,
        );
        assert_eq!(
            result.reason,
            LockReason::EdgePinned,
            "shrunk span must edge-pin the true lag, not lock: {result:?}"
        );
    }

    /// §3 case 1 (redraw half): feeding `redraw_b_times`'s own output as the literal B input must
    /// not unique-lock — the null model itself has to be a legal, non-locking picker output (§2
    /// rule 4), not just something the real fixture beats on average.
    #[test]
    fn redrawn_b_times_as_literal_input_do_not_unique_lock() {
        let injected_lag_secs = 0.35;
        let fixture = build_golomb_fixture(0xD0D0, injected_lag_secs);
        let picker_params = PickerParams::first_candidate(8_000);
        let hist_params = LagHistogramParams::first_candidate(&picker_params);
        let b_span = (0.0, fixture.b_len_secs);

        let mut rng = Lcg(0xBADC0FFE_u64 | 1);
        let redrawn_b_times = redraw_b_times(
            &mut rng,
            fixture.b_times.len(),
            b_span,
            hist_params.min_spacing_secs,
        )
        .expect("fixture's b_span is comfortably wider than the spacing floor");

        let result =
            evaluate_l_histogram(&fixture.a_times, &redrawn_b_times, b_span, &hist_params, 99);
        assert_ne!(
            result.reason,
            LockReason::Locked,
            "redrawn B times fed as literal input must not unique-lock: {result:?}"
        );
    }

    /// The direct spacings transform must produce a legal (min-spacing-respecting) arrangement
    /// whenever one exists, covering the geometry the old reject-and-retry loop only reached
    /// probabilistically: `n_b` points packed right up against the feasibility floor
    /// `(n_b-1)*min_spacing_secs == width`.
    #[test]
    fn redraw_b_times_at_the_spacing_floor_is_legal() {
        let n_b = 8usize;
        let min_spacing_secs = 0.010;
        let width = min_spacing_secs * (n_b - 1) as f64; // exactly the feasibility floor
        let span = (1.0, 1.0 + width);

        let mut rng = Lcg(0x5EED_u64 | 1);
        let times =
            redraw_b_times(&mut rng, n_b, span, min_spacing_secs).expect("floor is feasible");
        assert_eq!(times.len(), n_b);
        assert!(
            times
                .windows(2)
                .all(|w| w[1] - w[0] >= min_spacing_secs - 1e-9),
            "spacing floor violated: {times:?}"
        );
        assert!(
            times.iter().all(|&t| t >= span.0 && t <= span.1 + 1e-9),
            "times must stay inside the searched span: {times:?}"
        );
    }

    /// §3 case 1 (redraw half, infeasible geometry): when `b_span` is narrower than the spacing
    /// floor, no legal redraw exists — `evaluate_l_histogram` must report `RedrawInfeasible`, not
    /// silently substitute a spacing-violating arrangement or `Locked`/`NotUnique`.
    #[test]
    fn too_narrow_span_is_redraw_infeasible() {
        let a_times = vec![0.10, 0.40, 0.90, 1.30];
        let b_times = vec![0.55, 0.555, 0.56, 0.565];
        let picker_params = PickerParams::first_candidate(8_000);
        let hist_params = LagHistogramParams::first_candidate(&picker_params);
        // 4 B times need 3 internal gaps; at min_spacing_secs=0.010 the feasibility floor is
        // 0.030s. This span is 0.020s wide — narrower than the floor, so no legal redraw exists.
        let b_span = (0.5, 0.52);

        let result = evaluate_l_histogram(&a_times, &b_times, b_span, &hist_params, 1);
        assert_eq!(result.reason, LockReason::RedrawInfeasible, "{result:?}");
        assert!(result.redraw_p95_votes.is_none());
    }

    /// Checklist item 2's regression: permuting the detected B times must not change
    /// `hist_peak_votes` — the reason a literal shuffle cannot be the decoy (§0, §2 rule 4).
    #[test]
    fn permuting_b_times_does_not_change_hist_peak_votes() {
        let a_times = vec![0.10, 0.40, 0.90, 1.30];
        let b_times = vec![0.55, 0.85, 1.35, 1.75];
        let picker_params = PickerParams::first_candidate(8_000);
        let params = LagHistogramParams::first_candidate(&picker_params);
        let b_span = (0.0, 2.0);

        let original = evaluate_l_histogram(&a_times, &b_times, b_span, &params, 1);

        let mut permuted = b_times.clone();
        permuted.reverse();
        permuted.swap(0, 2);
        let after_permute = evaluate_l_histogram(&a_times, &permuted, b_span, &params, 1);

        assert_eq!(original.hist_peak_votes, after_permute.hist_peak_votes);
        assert_eq!(original.hist_peak_lag_ms, after_permute.hist_peak_lag_ms);
    }
}
