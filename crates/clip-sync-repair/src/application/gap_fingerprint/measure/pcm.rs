//! `pcm` slice of the gap_fingerprint measure folder split.

use super::config::*;

use crate::application::gap_fingerprint::schema::*;

use crate::domain::gap_equivalence::{ChannelReduction, NoiseFloorProbe};

use crate::domain::gap_structure::StructureMatchParams;
use crate::domain::policies::{rms_interleaved, GapBorderSpec, RefinedGapFrames};

/// Span args for [`level_profile`]: the gap window and the surrounding context window (in frames).
pub(in crate::application::gap_fingerprint::measure) struct LevelProfileSpan {
    pub(in crate::application::gap_fingerprint::measure) gap_start: usize,
    pub(in crate::application::gap_fingerprint::measure) gap_end: usize,
    pub(in crate::application::gap_fingerprint::measure) context_start: usize,
    pub(in crate::application::gap_fingerprint::measure) context_end: usize,
}
/// Build a [`LevelProfile`] over `[context_start, context_end)` (binned), with `noise_floor` taken from the
/// context bins *outside* `[gap_start, gap_end)` and `gap_floor` the loudest bin *inside* it. `bin_rms(f,
/// end)` returns the mono RMS (linear) over `[f, end)`. Shared by the A-side (interleaved downmix) and the
/// symmetric B-side (mono) paths so the two profiles are computed by *identical* logic (D11) and cannot drift.
///
/// Also returns the **context bin count** behind `noise_floor_db`. It is not a [`LevelProfile`] field
/// because that type is serialized into every dumped gap; returning it keeps the noise-floor probes
/// (F15) from having to re-derive the bin walk and drift from the thing they characterize.
pub(in crate::application::gap_fingerprint::measure) fn level_profile(
    bin_rms: impl Fn(usize, usize) -> f32,
    span: LevelProfileSpan,
    bin_frames: usize,
    bin_ms: u32,
) -> (LevelProfile, usize) {
    let mut profile_db = Vec::new();
    let mut context_bins_db = Vec::new();
    let mut f = span.context_start;
    while f < span.context_end {
        let end = (f + bin_frames).min(span.context_end);
        let db = to_db(bin_rms(f, end));
        profile_db.push(db);
        if f < span.gap_start || f >= span.gap_end {
            context_bins_db.push(db);
        }
        f = end;
    }
    let gap_floor_db = {
        let mut mx = SILENCE_FLOOR_DB;
        let mut g = span.gap_start;
        while g < span.gap_end {
            let end = (g + bin_frames).min(span.gap_end);
            mx = mx.max(to_db(bin_rms(g, end)));
            g = end;
        }
        mx
    };
    let context_bins = context_bins_db.len();
    (
        LevelProfile {
            bin_ms: Some(bin_ms),
            speech_peak_db: Some(profile_db.iter().copied().fold(SILENCE_FLOOR_DB, f32::max)),
            noise_floor_db: median(context_bins_db),
            gap_floor_db,
            floor_db: Some(SILENCE_FLOOR_DB),
            profile_db,
        },
        context_bins,
    )
}
/// One candidate noise floor over A's gap context at `(context_secs, bin_ms, reduction)`, measured by
/// [`level_profile`] itself so the probe cannot drift from the measurement it characterizes.
///
/// The two reductions are taken from the functions the two front-ends actually use — `mono_rms` for
/// [`ChannelReduction::Downmix`], `rms_interleaved` (what the scan's `block_rms_db` calls) for
/// [`ChannelReduction::Interleaved`] — rather than reimplemented here, for the same reason.
///
/// **Provenance only** — see [`NoiseFloorProbe`]. An empty context (zero window, or a gap that fills
/// the track) yields `floor_db: None` rather than `median()`'s −120 placeholder, so "no context" stays
/// distinguishable from "silent context".
pub(in crate::application::gap_fingerprint::measure) fn noise_floor_probe(
    a_samples: &[f32],
    channels: usize,
    gap_frames: std::ops::Range<usize>,
    sample_rate: u32,
    context_secs: f64,
    bin_ms: u64,
    reduction: ChannelReduction,
) -> NoiseFloorProbe {
    let ch = channels.max(1);
    let rate = f64::from(sample_rate).max(1.0);
    let context_frames = (context_secs.max(0.0) * rate).round() as usize;
    let bin_frames = (((bin_ms as f64) / 1000.0) * rate).round().max(1.0) as usize;
    let frames = a_samples.len() / ch;
    let accessor = |f: usize, end: usize| match reduction {
        ChannelReduction::Downmix => mono_rms(a_samples, ch, f, end),
        ChannelReduction::Interleaved => {
            let end = end.min(frames);
            if f >= end {
                0.0
            } else {
                rms_interleaved(&a_samples[f * ch..end * ch])
            }
        }
    };
    let (lp, context_bins) = level_profile(
        accessor,
        LevelProfileSpan {
            gap_start: gap_frames.start,
            gap_end: gap_frames.end,
            context_start: gap_frames.start.saturating_sub(context_frames),
            context_end: (gap_frames.end + context_frames).min(a_samples.len() / ch),
        },
        bin_frames,
        bin_ms as u32,
    );
    NoiseFloorProbe {
        context_secs,
        bin_ms,
        reduction,
        floor_db: (context_bins > 0).then(|| f64::from(lp.noise_floor_db)),
        context_bins,
    }
}
/// The `{context window} × {bin size} × {channel reduction}` grid of [`noise_floor_probe`] reads,
/// deduped.
///
/// Deduping matters for interpretation, not cost: when the two front-ends' recipes coincide the grid
/// collapses, and a repeated row would read as corroboration rather than as the same measurement twice.
/// Mono material makes the two reductions identical *numerically* but they stay separate rows — they
/// are different recipes, and collapsing them would hide that the run had nothing to say about the axis.
pub(in crate::application::gap_fingerprint::measure) fn noise_floor_probe_grid(
    a_samples: &[f32],
    channels: usize,
    gap_frames: std::ops::Range<usize>,
    sample_rate: u32,
    context_secs: &[f64],
    bin_ms: &[u64],
    reductions: &[ChannelReduction],
) -> Vec<NoiseFloorProbe> {
    let mut out: Vec<NoiseFloorProbe> = Vec::new();
    for &secs in context_secs {
        for &bin in bin_ms {
            for &reduction in reductions {
                if out
                    .iter()
                    .any(|p| p.context_secs == secs && p.bin_ms == bin && p.reduction == reduction)
                {
                    continue;
                }
                out.push(noise_floor_probe(
                    a_samples,
                    channels,
                    gap_frames.clone(),
                    sample_rate,
                    secs,
                    bin,
                    reduction,
                ));
            }
        }
    }
    out
}
/// Mono RMS (linear) over `b_mono[start..end]` — the B-side accessor for [`level_profile`] / nominal donor.
pub(in crate::application::gap_fingerprint::measure) fn mono_slice_rms(
    b_mono: &[f64],
    start: usize,
    end: usize,
) -> f32 {
    let end = end.min(b_mono.len());
    if start >= end {
        return 0.0;
    }
    let s = &b_mono[start..end];
    ((s.iter().map(|v| v * v).sum::<f64>() / s.len() as f64).sqrt()) as f32
}
pub(in crate::application::gap_fingerprint::measure) fn to_db(rms: f32) -> f32 {
    if rms <= 1e-9 {
        SILENCE_FLOOR_DB
    } else {
        20.0 * rms.log10()
    }
}
/// Replace a non-finite correlation (`normalized_correlation` of a constant/silent window ⇒ `NaN`) with
/// `0.0` — a NaN "no signal" reads as zero correlation. Keeps the JSON finite (no `null` ⇒ no silent
/// whole-gap drop in strict consumers).
pub(in crate::application::gap_fingerprint::measure) fn finite_corr(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}
/// Mono-downmix RMS over a frame range.
pub(in crate::application::gap_fingerprint::measure) fn mono_rms(
    samples: &[f32],
    channels: usize,
    start: usize,
    end: usize,
) -> f32 {
    let ch = channels.max(1);
    let total = samples.len() / ch;
    let start = start.min(total);
    let end = end.min(total);
    if end <= start {
        return 0.0;
    }
    let mut sum_sq = 0f64;
    for f in start..end {
        let mut acc = 0f64;
        for c in 0..ch {
            acc += samples[f * ch + c] as f64;
        }
        let v = acc / ch as f64;
        sum_sq += v * v;
    }
    (sum_sq / (end - start) as f64).sqrt() as f32
}
pub(in crate::application::gap_fingerprint::measure) fn mono_peak(
    samples: &[f32],
    channels: usize,
    start: usize,
    end: usize,
) -> f32 {
    let ch = channels.max(1);
    let total = samples.len() / ch;
    let (start, end) = (start.min(total), end.min(total));
    let mut peak = 0f32;
    for f in start..end {
        let mut acc = 0f64;
        for c in 0..ch {
            acc += samples[f * ch + c] as f64;
        }
        peak = peak.max((acc / ch as f64).abs() as f32);
    }
    peak
}
pub(in crate::application::gap_fingerprint::measure) fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return SILENCE_FLOOR_DB;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}
/// `1.0 - peak-normalized range` of a gated-energy envelope: 1.0 = perfectly flat, →0 = strong contour.
pub(in crate::application::gap_fingerprint::measure) fn flatness(bins: &[f32]) -> f32 {
    if bins.is_empty() {
        return 1.0;
    }
    let peak = bins.iter().copied().fold(0.0f32, f32::max);
    if peak <= f32::EPSILON {
        return 1.0;
    }
    let min = bins.iter().copied().fold(f32::INFINITY, f32::min);
    (1.0 - (peak - min) / peak).clamp(0.0, 1.0)
}

/// §3.6a frozen wide-envelope confirmer: 100 ms RMS bins, 2 s seam window, ±400 ms lag on the envelope.
/// The bin is [`crate::domain::lag_envelope::ENVELOPE_BIN_MS`] — one const for every envelope in the
/// codebase; re-exported under the local name so this module's call sites keep their spelling.
pub(in crate::application::gap_fingerprint::measure) use crate::domain::lag_envelope::ENVELOPE_BIN_MS as WIDE_ENV_BIN_MS;
pub(in crate::application::gap_fingerprint::measure) const WIDE_ENV_WINDOW_SECS: f64 = 2.0;
pub(in crate::application::gap_fingerprint::measure) const WIDE_ENV_MAX_LAG_MS: f64 = 400.0;

/// `~10 ms`-bin RMS envelope — moved to [`crate::domain::lag_envelope`] with the rest of the coarse
/// envelope primitives; re-exported here so existing call sites keep their paths.
pub(in crate::application::gap_fingerprint::measure) use crate::domain::lag_envelope::fine_rms_envelope;
pub(in crate::application::gap_fingerprint::measure) fn structure_params_for(
    cfg: &FingerprintConfig,
    gap_frames: usize,
    bin_frames: usize,
    search_radius_frames: usize,
    slack: usize,
) -> StructureMatchParams {
    StructureMatchParams {
        gap_frames,
        bin_frames: bin_frames.max(1),
        search_radius_frames,
        fill_length_slack_frames: slack,
        // Bounded sample polish — NOT `slack`/`bin_frames`: a large value makes the unified search's
        // fine-polish loop run multi-second exhaustive scans per candidate (see gap_structure docs).
        max_fine_adjustment_frames: crate::domain::gap_structure::structure_fine_polish_frames(
            bin_frames,
        ),
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_silence_rms: cfg.absolute_silence_rms,
    }
}

/// Border standoff applied to **every** `GapBorderSpec` this probe builds — deliberately 0, and
/// deliberately **not** `RepairConfig.border_standoff_secs` (0.35).
///
/// The standoff exists to hold production's search templates away from the dropout; this probe
/// measures *registration*, so adopting it would push the template a third of a second off the edge
/// and every recorded `frac_lag_ms` with it. The residual walk-off/trim displacement that remains is
/// measured, not assumed (`border_edge_displacement_frames` → [`LagSummary::border_displacement_ms`]).
/// See `docs/dev/archive/TEMP-fill-placement-registration-plan.md` § "Step 5 result".
///
/// One constant, shared by [`probe_border_spec`] (every probe `GapBorderSpec`) and by
/// [`probe_border_standoff_secs`] / [`CorpusProbeRecipe::from_config`], so a corpus can never
/// advertise a standoff the probe didn't apply.
pub(crate) const PROBE_BORDER_STANDOFF_FRAMES: usize = 0;
/// Seconds form of [`PROBE_BORDER_STANDOFF_FRAMES`] at the rate the probe applied it. Shared by
/// [`CorpusProbeRecipe::from_config`] so a corpus cannot advertise a standoff the specs did not use.
pub(in crate::application::gap_fingerprint::measure) fn probe_border_standoff_secs(
    sample_rate: u32,
) -> f64 {
    PROBE_BORDER_STANDOFF_FRAMES as f64 / f64::from(sample_rate).max(1.0)
}
/// Every probe `GapBorderSpec` is built here so a fifth site cannot write a standoff the recipe
/// would not report. `border_frames` still varies per measurement; the standoff does not.
pub(in crate::application::gap_fingerprint::measure) fn probe_border_spec(
    refined: &RefinedGapFrames,
    border_frames: usize,
    cfg: &FingerprintConfig,
) -> GapBorderSpec {
    GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames,
        border_standoff_frames: PROBE_BORDER_STANDOFF_FRAMES,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gap_equivalence::ChannelReduction;

    #[test]
    fn noise_floor_probe_reads_the_context_not_the_gap() {
        // 1 s context @ ~−40 dBFS, 1 s digital-silence gap, 1 s context.
        let ctx = 0.01f32; // −40 dBFS constant ⇒ RMS = peak
        let mut a = vec![ctx; 48_000];
        a.extend(std::iter::repeat_n(0.0f32, 48_000));
        a.extend(std::iter::repeat_n(ctx, 48_000));
        let p = noise_floor_probe(
            &a,
            1,
            48_000..96_000,
            48_000,
            1.0,
            50,
            ChannelReduction::Downmix,
        );
        assert_eq!(p.context_bins, 40, "±1 s at 50 ms ⇒ 40 context bins");
        let db = p.floor_db.expect("context present ⇒ a floor");
        assert!(
            (db - -40.0).abs() < 0.5,
            "context floor should read ≈−40, got {db}"
        );
    }

    /// The two variables move the read independently, which is the whole point of probing a grid:
    /// each `(context_secs, bin_ms)` pair is a distinct row and none is silently reused.
    #[test]
    fn noise_floor_probe_grid_covers_the_cross_product() {
        let a = vec![0.01f32; 144_000];
        let g = noise_floor_probe_grid(
            &a,
            1,
            48_000..96_000,
            48_000,
            &[2.0, 3.0],
            &[100, 50],
            &[ChannelReduction::Downmix],
        );
        assert_eq!(g.len(), 4, "2 windows × 2 bin sizes");
        let mut seen: Vec<(u64, u64)> = g
            .iter()
            .map(|p| (p.context_secs.to_bits(), p.bin_ms))
            .collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 4, "every row must be a distinct combination");
        // Halving the bin size doubles the population behind the median at a fixed window.
        for secs in [2.0f64, 3.0] {
            let at = |bin| {
                g.iter()
                    .find(|p| p.context_secs == secs && p.bin_ms == bin)
                    .unwrap()
                    .context_bins
            };
            assert!(
                at(50) > at(100),
                "50 ms must bin finer than 100 ms at {secs}s"
            );
        }
    }

    /// When the two front-ends' recipes coincide the grid collapses instead of emitting the same
    /// measurement twice — a duplicate row would read as corroboration.
    #[test]
    fn noise_floor_probe_grid_dedupes_coinciding_recipes() {
        let a = vec![0.01f32; 144_000];
        let g = noise_floor_probe_grid(
            &a,
            1,
            48_000..96_000,
            48_000,
            &[2.0, 2.0],
            &[50, 50],
            &[ChannelReduction::Downmix],
        );
        assert_eq!(g.len(), 1);
    }

    /// No context at all (zero window) ⇒ `None`, not `median()`'s −120 placeholder: "no context" must
    /// stay distinguishable from "silent context".
    #[test]
    fn noise_floor_probe_without_context_reports_none() {
        let a = vec![0.0f32; 96_000];
        let p = noise_floor_probe(&a, 1, 0..96_000, 48_000, 0.0, 50, ChannelReduction::Downmix);
        assert_eq!(p.context_bins, 0);
        assert_eq!(p.floor_db, None);
    }

    /// A genuinely silent context does read a floor — the counterpart to the case above.
    #[test]
    fn noise_floor_probe_with_silent_context_reports_the_floor() {
        let a = vec![0.0f32; 144_000];
        let p = noise_floor_probe(
            &a,
            1,
            48_000..96_000,
            48_000,
            1.0,
            50,
            ChannelReduction::Downmix,
        );
        assert!(p.context_bins > 0);
        assert_eq!(p.floor_db, Some(f64::from(SILENCE_FLOOR_DB)));
    }

    // --- noise-floor probes: the channel-reduction dimension (F15 third variable) -------------------

    /// `channels`-channel interleaved tone bed, 3 s @ 48 kHz. `amps[c]` scales channel `c`'s sinusoid
    /// at `freqs[c]` Hz. All frequencies are multiples of 20 Hz, so every one completes a whole number
    /// of periods in a 50 ms bin and distinct channels are *exactly* orthogonal over each bin — the
    /// decorrelated case is deterministic here, with no PRNG and no statistical tolerance.
    fn tone_bed(freqs: &[f64], amps: &[f64]) -> Vec<f32> {
        let ch = freqs.len();
        let mut out = Vec::with_capacity(144_000 * ch);
        for f in 0..144_000usize {
            for c in 0..ch {
                let t = f as f64 / 48_000.0;
                out.push((amps[c] * (std::f64::consts::TAU * freqs[c] * t).sin()) as f32);
            }
        }
        out
    }

    fn nf_db(samples: &[f32], ch: usize, reduction: ChannelReduction) -> f64 {
        noise_floor_probe(samples, ch, 48_000..96_000, 48_000, 1.0, 50, reduction)
            .floor_db
            .expect("context present")
    }

    /// **ρ̄ = 1 ⇒ 0 dB.** Six channels carrying the *identical* waveform: the downmix returns exactly
    /// that waveform, so the amplitude mean and the power mean coincide. This is the only configuration
    /// where they do — equality in Cauchy–Schwarz requires pointwise-identical channels, not merely
    /// similar ones.
    #[test]
    fn reduction_agrees_only_when_the_channels_are_identical() {
        let a = tone_bed(&[440.0; 6], &[0.05; 6]);
        let (i, d) = (
            nf_db(&a, 6, ChannelReduction::Interleaved),
            nf_db(&a, 6, ChannelReduction::Downmix),
        );
        assert!(
            (i - d).abs() < 0.01,
            "identical channels must read the same both ways, got {i} vs {d}"
        );
    }

    /// **ρ̄ = 0 ⇒ 10·log10(N).** Six equal-power channels, mutually orthogonal over every bin. The
    /// coherent sum grows as √N against a divisor of N, so the downmix under-reads by exactly 7.78 dB
    /// at six channels. This is the ceiling the *measured* corpus penalties (3.13–7.96 dB) straddle.
    #[test]
    fn reduction_differs_by_ten_log_n_when_the_channels_are_decorrelated() {
        let a = tone_bed(&[200.0, 400.0, 600.0, 800.0, 1000.0, 1200.0], &[0.05; 6]);
        let delta =
            nf_db(&a, 6, ChannelReduction::Interleaved) - nf_db(&a, 6, ChannelReduction::Downmix);
        let expect = 10.0 * 6.0f64.log10();
        assert!(
            (delta - expect).abs() < 0.05,
            "decorrelated 6ch must differ by 10·log10(6) = {expect:.2} dB, got {delta:.2}"
        );
    }

    /// **Decorrelation is not the only route to 7.78 dB.** One active channel over five digitally
    /// silent ones hits the *same* `1/N` ratio. The penalty measures coherent-sum gain, which power
    /// concentration moves just as correlation does — so a large reduction term must not be read as
    /// evidence that the content is decorrelated.
    #[test]
    fn reduction_penalty_is_also_reached_by_a_single_active_channel() {
        let a = tone_bed(&[440.0; 6], &[0.05, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let delta =
            nf_db(&a, 6, ChannelReduction::Interleaved) - nf_db(&a, 6, ChannelReduction::Downmix);
        let expect = 10.0 * 6.0f64.log10();
        assert!(
            (delta - expect).abs() < 0.05,
            "one-of-six active must also differ by {expect:.2} dB, got {delta:.2}"
        );
    }

    /// **The sign is forced, not observed.** `(Σ x_c)² ≤ N·Σ x_c²` pointwise, so the downmix can never
    /// read *above* the interleaved power mean whatever the content. Recorded as a test because the
    /// corpus's "uniformly signed" result was briefly treated as evidence *for* the reduction
    /// hypothesis; it is a theorem, and carries no evidential weight.
    #[test]
    fn downmix_never_reads_above_interleaved() {
        for freqs in [
            [440.0, 440.0, 440.0, 440.0, 440.0, 440.0],
            [200.0, 400.0, 600.0, 800.0, 1000.0, 1200.0],
            [200.0, 200.0, 600.0, 600.0, 1000.0, 1000.0],
            [1200.0, 200.0, 800.0, 400.0, 600.0, 1000.0],
        ] {
            for amps in [[0.05; 6], [0.09, 0.01, 0.05, 0.02, 0.07, 0.03]] {
                let a = tone_bed(&freqs, &amps);
                let (i, d) = (
                    nf_db(&a, 6, ChannelReduction::Interleaved),
                    nf_db(&a, 6, ChannelReduction::Downmix),
                );
                assert!(
                    d <= i + 0.01,
                    "downmix {d} exceeded interleaved {i} at {freqs:?} / {amps:?}"
                );
            }
        }
    }

    /// The reduction is a third grid dimension, not a replacement for either of the first two: the
    /// cross-product is emitted whole.
    #[test]
    fn noise_floor_probe_grid_covers_the_reduction_dimension() {
        let a = tone_bed(&[200.0, 400.0, 600.0, 800.0, 1000.0, 1200.0], &[0.05; 6]);
        let g = noise_floor_probe_grid(
            &a,
            6,
            48_000..96_000,
            48_000,
            &[2.0, 3.0],
            &[100, 50],
            &[ChannelReduction::Interleaved, ChannelReduction::Downmix],
        );
        assert_eq!(g.len(), 8, "2 windows × 2 bin sizes × 2 reductions");
        for &secs in &[2.0f64, 3.0] {
            for &bin in &[100u64, 50] {
                let at = |r| {
                    g.iter()
                        .find(|p| p.context_secs == secs && p.bin_ms == bin && p.reduction == r)
                        .unwrap_or_else(|| panic!("missing row {secs}/{bin}/{r:?}"))
                        .floor_db
                        .unwrap()
                };
                assert!(
                    at(ChannelReduction::Downmix) < at(ChannelReduction::Interleaved),
                    "the reduction must move the read at every ({secs}, {bin})"
                );
            }
        }
    }

    /// Mono makes the two reductions numerically identical, but they stay **separate rows**. Collapsing
    /// them would hide that a mono run had nothing to say about the axis, which is exactly the case a
    /// reader of the dump needs to distinguish from "measured, and the axis was flat".
    #[test]
    fn mono_keeps_both_reduction_rows_despite_reading_the_same() {
        let a = tone_bed(&[440.0], &[0.05]);
        let g = noise_floor_probe_grid(
            &a,
            1,
            48_000..96_000,
            48_000,
            &[2.0],
            &[50],
            &[ChannelReduction::Interleaved, ChannelReduction::Downmix],
        );
        assert_eq!(g.len(), 2, "mono must not collapse the reduction dimension");
        assert_eq!(g[0].floor_db, g[1].floor_db);
    }
}
