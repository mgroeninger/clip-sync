//! PCM measurement + fingerprint/corpus builders (M-MOD `measure` slice).
//!
//! Owns the lag summarize/classify path, the placement / seam-probe / dual-fit / wide-envelope
//! builders, [`build_gap_fingerprint`], [`compute_region_measurements`], `characterize_gaps*`, and
//! [`write_corpus_dir`]. Depends on [`super::schema`] (types) and [`super::project`]
//! (`tags_from_fields`) — never the reverse.

use super::contract::*;
use super::project::*;
use super::schema::*;

use clip_sync::normalized_correlation;

use crate::domain::gap_equivalence::{
    ChannelReduction, EquivalenceMeasurement, NoiseFloorProbe, SpanKind,
};

use crate::domain::gap_anchor_seam::{
    list_anchor_candidates_a, list_feasible_anchor_brackets, AnchorSeamParams, AnchorSource,
};
use crate::domain::gap_energy::energy_bins;
use crate::domain::gap_fill_fit::{
    match_gap_fill_unified_in_b, UnifiedFillSearchInput, UnifiedFitWeights, WaveformSeamContext,
};
use crate::domain::gap_signature::{build_gap_signature, GapSignatureMode};
use crate::domain::gap_structure::StructureMatchParams;
use crate::domain::patch_result::GapPatchSkipReason;
use crate::domain::pcm::{interleaved_to_channels, interleaved_to_mono};
use crate::domain::policies::{
    border_templates_for_gap, border_templates_per_channel_for_gap, refine_gap_frames,
    rms_interleaved, seam_channel_diagnostics, GapBorderSpec, RefinedGapFrames, SeamPlacement,
    SeamTemplates,
};

/// Span args for [`level_profile`]: the gap window and the surrounding context window (in frames).
struct LevelProfileSpan {
    gap_start: usize,
    gap_end: usize,
    context_start: usize,
    context_end: usize,
}

/// Build a [`LevelProfile`] over `[context_start, context_end)` (binned), with `noise_floor` taken from the
/// context bins *outside* `[gap_start, gap_end)` and `gap_floor` the loudest bin *inside* it. `bin_rms(f,
/// end)` returns the mono RMS (linear) over `[f, end)`. Shared by the A-side (interleaved downmix) and the
/// symmetric B-side (mono) paths so the two profiles are computed by *identical* logic (D11) and cannot drift.
///
/// Also returns the **context bin count** behind `noise_floor_db`. It is not a [`LevelProfile`] field
/// because that type is serialized into every dumped gap; returning it keeps the noise-floor probes
/// (F15) from having to re-derive the bin walk and drift from the thing they characterize.
fn level_profile(
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
fn noise_floor_probe(
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
fn noise_floor_probe_grid(
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
fn mono_slice_rms(b_mono: &[f64], start: usize, end: usize) -> f32 {
    let end = end.min(b_mono.len());
    if start >= end {
        return 0.0;
    }
    let s = &b_mono[start..end];
    ((s.iter().map(|v| v * v).sum::<f64>() / s.len() as f64).sqrt()) as f32
}

/// A lag peak within this many ms of the searched boundary is treated as edge-pinned (search-exhausted):
/// the parabolic vertex can't be trusted and a larger lag was never scored. See [`LagSummary::edge_pinned`].
const LAG_EDGE_TOL_MS: f64 = 2.0;

/// Search knobs for a per-shoulder lag correlation sweep (`lag_side_sweep` / [`lag_pair`]).
#[derive(Debug, Clone, Copy)]
struct LagSweepParams {
    window: usize,
    max_lag: i64,
    sample_rate: u32,
    channel: LagChannel,
}

impl LagSweepParams {
    fn win_ms(self) -> u32 {
        ((self.window as f64) * 1000.0 / f64::from(self.sample_rate.max(1))) as u32
    }

    fn max_lag_ms(self) -> u32 {
        let ml = self.max_lag.max(0) as usize;
        ((ml as f64) * 1000.0 / f64::from(self.sample_rate.max(1))) as u32
    }

    fn ml(self) -> usize {
        self.max_lag.max(0) as usize
    }
}

/// One shoulder of a [`lag_pair`] sweep: an A border template vs the B haystack around `anchor_frame`.
#[derive(Debug, Clone, Copy)]
struct LagSideSweep<'a> {
    a_border: &'a [f64],
    b_signal: &'a [f64],
    anchor_frame: usize,
    /// `true` for the pre shoulder (last `window` samples of `a_border`); `false` for post (first `window`).
    pre_shoulder: bool,
    /// Samples added to each lag bin in the returned summary (sequential post: gross map to `b_mapped_end`).
    gross_lag_shift: i64,
}

/// ± lag sweep (ms) used to gauge each dual-fit seam's peak uniqueness (the periodicity/alias guard).
const DUALFIT_SEAM_UNIQ_LAG_MS: f64 = 30.0;

/// Half-width (ms) of the per-shoulder **seam-local** lag search in [`splice_dualfit_at`], anchored on the
/// **nominal `b_mapped`** (not the gross 1 s `lag_decision`). The seam defines its own placement, so the
/// search must cover the full registration range: a gross lag that locked onto distant content (7·g3: 1 s
/// pre lag −319 ms but the seam is at +18 ms) would otherwise clip a live seam. Set to the `lag_decision`
/// range so anything the 1 s sweep could register, the seam sweep can too. `peak_z` on the seam curve is the
/// alias guard against a wide search locking onto a far periodic peak. Calibrate at ledger A5/C6.
const SEAM_LOCAL_SEARCH_MS: f64 = 600.0;

/// Prominence of a seam's placement peak over its tallest rival within ±`max_lag`. `b_ctx` must be laid
/// out so lag 0 aligns `a_win` at the placement (`b_ctx[max_lag .. max_lag + a_win.len()]`). Low prominence
/// ⇒ the seam matches at many lags (periodic) ⇒ the dual-fit placement is not a unique registration.
fn seam_prominence(a_win: &[f64], b_ctx: &[f64], max_lag: i64, sample_rate: u32) -> Option<f64> {
    let curve = lag_correlation_curve(a_win, b_ctx, max_lag);
    summarize_lag_curve(&curve, sample_rate, 0, 0, LagChannel::Mono).and_then(|s| s.prominence)
}

/// Build the D/R tag payload from the shared [`RegionMeasurements`] (computed from decode) + the A-side levels
/// (8g.3b). Mirrors [`tags_from_fingerprint`] via [`tags_from_fields`]; `structure`/`seams` are omitted
/// (`None`) to match the `skip_baseline_placement` summary the dump uses — so from-decode tags equal the
/// oracle's on that path by construction. Used by the from-decode dump ([`characterize_gaps_from_decode`]).
fn tags_from_measurements(
    m: &RegionMeasurements,
    levels: Option<crate::domain::gap_repair_spec::LevelTags>,
) -> crate::domain::gap_repair_spec::GapRepairTags {
    tags_from_fields(
        m.lag_decision.as_ref(),
        m.lag_editorial.as_ref(),
        m.splice.as_ref(),
        m.splice_dualfit,
        &m.brackets,
        None,
        None,
        m.residual,
        m.donor_interior,
        m.donor_interior_nominal,
        levels,
    )
}

// ---------------------------------------------------------------------------------------------
// Lag-correlation probe
// ---------------------------------------------------------------------------------------------

/// `lag_correlation_curve` + `seam_local_peak` moved to the shared `domain::seam_local` so the production
/// dual-fit repair (A3) and this diagnostic scan use one implementation (no drift). Re-exported here so the
/// existing call sites / tests keep their paths.
pub use crate::domain::seam_local::{
    lag_correlation_curve, lag_correlation_curve_auto, seam_local_peak,
};

/// Summarize a lag curve: lag-0 value, integer peak, parabolic-interpolated (fractional) peak, and a
/// [`LagVerdict`]. `None` for an empty curve.
pub fn summarize_lag_curve(
    curve: &[(i64, f64)],
    sample_rate: u32,
    window_ms: u32,
    max_lag_ms: u32,
    channel: LagChannel,
) -> Option<LagSummary> {
    if curve.is_empty() {
        return None;
    }
    let lag0_r = curve
        .iter()
        .find(|(l, _)| *l == 0)
        .map(|(_, r)| *r)
        .unwrap_or(f64::NAN);
    let (pi, &(peak_lag, peak_r)) = curve
        .iter()
        .enumerate()
        .max_by(|(_, (_, x)), (_, (_, y))| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))?;

    // Parabolic vertex through the integer peak and its two neighbours (when interior).
    let (frac_lag, frac_r) = if pi > 0 && pi < curve.len() - 1 {
        let y0 = curve[pi - 1].1;
        let y1 = peak_r;
        let y2 = curve[pi + 1].1;
        let denom = y0 - 2.0 * y1 + y2;
        if denom.abs() > 1e-12 {
            let xstar = (y0 - y2) / (2.0 * denom);
            let peak = y1 - (y2 - y0) * (y2 - y0) / (8.0 * denom);
            (peak_lag as f64 + xstar, peak)
        } else {
            (peak_lag as f64, peak_r)
        }
    } else {
        (peak_lag as f64, peak_r)
    };

    let rate = f64::from(sample_rate).max(1.0);
    // Robust uniqueness from curve shape: prominence + spacing to the tallest rival, and the peak's
    // z-score over the whole curve (§3.6a — the metric that separates registration from periodicity).
    let second = secondary_peak(curve, pi);
    let n = curve.len() as f64;
    let mean = curve.iter().map(|(_, r)| *r).sum::<f64>() / n.max(1.0);
    let std = (curve.iter().map(|(_, r)| (r - mean).powi(2)).sum::<f64>() / n.max(1.0)).sqrt();
    // Edge-pin: the peak lands at (or within tol of) the boundary of the *searched* lags. Read the
    // boundary from the curve itself (`lag_correlation_curve` masks high-side lags where the window runs
    // past B), so this is the true searched extent, not the nominal `±max_lag`.
    let tol = ((LAG_EDGE_TOL_MS / 1000.0) * rate).round() as i64;
    let lo_lag = curve.first().map(|(l, _)| *l).unwrap_or(peak_lag);
    let hi_lag = curve.last().map(|(l, _)| *l).unwrap_or(peak_lag);
    let edge_pinned = (peak_lag - lo_lag) <= tol || (hi_lag - peak_lag) <= tol;
    Some(LagSummary {
        window_ms,
        max_lag_ms,
        channel,
        lag0_r: finite_corr(lag0_r),
        peak_r: finite_corr(peak_r),
        second_peak_r: second.map(|(_, r)| r),
        peak_z: (std > 1e-9).then(|| (peak_r - mean) / std),
        prominence: second.map(|(_, r)| peak_r - r),
        top2_spacing_ms: second
            .map(|(lag, _)| (peak_lag - lag).unsigned_abs() as f64 * 1000.0 / rate),
        peak_lag_samples: peak_lag,
        frac_lag_samples: frac_lag,
        frac_lag_ms: frac_lag * 1000.0 / rate,
        edge_pinned: Some(edge_pinned),
        verdict: classify_lag(lag0_r, frac_r, peak_lag),
    })
}

/// The tallest **competing** local maximum `(lag, r)` that is not the main peak at `peak_index`. Within
/// the main peak's lobe the curve falls off monotonically (no local maxima), so a separate local maximum
/// is a genuine rival lag — the periodicity / ambiguity signal. `None` when the peak is unrivalled.
fn secondary_peak(curve: &[(i64, f64)], peak_index: usize) -> Option<(i64, f64)> {
    let mut best: Option<(i64, f64)> = None;
    for i in 1..curve.len().saturating_sub(1) {
        if i == peak_index {
            continue;
        }
        let (lag, r) = curve[i];
        // Local maximum: ≥ both neighbours (≥ tolerates plateaus without double-counting the main lobe).
        if r >= curve[i - 1].1 && r >= curve[i + 1].1 {
            best = Some(match best {
                Some((_, br)) if br >= r => best.unwrap(),
                _ => (lag, r),
            });
        }
    }
    best
}

/// Verdict thresholds (see plan §4). `peak` is the parabolic-interpolated peak correlation.
fn classify_lag(lag0_r: f64, peak: f64, peak_lag: i64) -> LagVerdict {
    let recoverable = peak >= 0.5;
    let offset_away = peak_lag.abs() > 1 || (peak - lag0_r) > 0.2;
    if recoverable && offset_away {
        LagVerdict::TimingOffset
    } else if peak < 0.3 {
        LagVerdict::Decorrelated
    } else {
        LagVerdict::Ambiguous
    }
}

// ---------------------------------------------------------------------------------------------
// Builder — assembles a fingerprint from decoded windows using public domain functions only
// (no patch-path coupling). Mirrors `tests/diag_anchor_quiet_gap.rs`.
// ---------------------------------------------------------------------------------------------

/// Thresholds + window sizes the builder needs (a media-free subset of `RepairConfig`).
#[derive(Debug, Clone, Copy)]
pub struct FingerprintConfig {
    pub silence_peak_fraction: f32,
    pub absolute_silence_rms: f32,
    pub gap_signature_context_secs: f64,
    pub gap_signature_bin_ms: u64,
    pub fill_seam_search_secs: f64,
    pub fill_border_search_secs: f64,
    pub fill_align_margin_secs: f64,
    pub fill_length_slack_secs: f64,
    /// B haystack tail slack (extract / `pad_tail`); see `RepairConfig.fill_extract_tail_slack_secs`.
    pub fill_extract_tail_slack_secs: f64,
    pub border_secs: f64,
    pub border_standoff_secs: f64,
    pub max_anchor_bracket_secs: f64,
    pub max_anchors_per_side: usize,
    pub anchor_seam_min_prominence: f32,
    pub min_structure_match_score: f64,
    pub min_fill_correlation: f32,
    pub fill_marginal_margin: f32,
    pub fill_absolute_floor: f32,
    pub max_refine_secs: f64,
    pub lag_max_lag_ms: u32,
    /// Window (seconds) for the lag-uniqueness measurement. §3.6a froze this at **1 s** — it separates
    /// real registration from periodic ambiguity (250 ms is ambiguous on quiet produced audio, where a
    /// rival local max is nearly as tall; 1 s breaks the tie). Drives `peak_z` / `prominence`.
    pub lag_window_secs: f64,
}

impl Default for FingerprintConfig {
    fn default() -> Self {
        Self {
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
            gap_signature_context_secs: 3.0,
            gap_signature_bin_ms: 50,
            fill_seam_search_secs: 0.25,
            fill_border_search_secs: 10.0,
            fill_align_margin_secs: 1.0,
            fill_length_slack_secs: 0.05,
            fill_extract_tail_slack_secs: 0.05,
            border_secs: 1.0,
            border_standoff_secs: 0.35,
            max_anchor_bracket_secs: 5.0,
            max_anchors_per_side: 5,
            anchor_seam_min_prominence: 0.0,
            min_structure_match_score: 0.55,
            min_fill_correlation: 0.35,
            fill_marginal_margin: 0.08,
            fill_absolute_floor: 0.12,
            max_refine_secs: 0.75,
            lag_max_lag_ms: 600,
            lag_window_secs: 1.0,
        }
    }
}

/// Decoded windows + geometry for one gap. `b_haystack` absent ⇒ A-only (no pairwise fields).
pub struct GapInputs<'a> {
    pub a_samples: &'a [f32],
    pub b_haystack: Option<&'a [f32]>,
    pub channels: usize,
    pub sample_rate: u32,
    pub reported_start_frame: usize,
    pub reported_end_frame: usize,
    /// Start of the decoded B haystack (seconds); `0.0` if the full B track is passed as the
    /// haystack. Used to map A-relative B times into haystack frame coordinates.
    pub b_extract_start_secs: f64,
    /// B-minus-A offset (seconds) for this gap, from the report mapping / resolved alignment.
    /// The builder refines A internally and maps each A boundary to B via this offset.
    pub gap_offset_secs: f64,
    pub config: FingerprintConfig,
}

fn to_db(rms: f32) -> f32 {
    if rms <= 1e-9 {
        SILENCE_FLOOR_DB
    } else {
        20.0 * rms.log10()
    }
}

/// Replace a non-finite correlation (`normalized_correlation` of a constant/silent window ⇒ `NaN`) with
/// `0.0` — a NaN "no signal" reads as zero correlation. Keeps the JSON finite (no `null` ⇒ no silent
/// whole-gap drop in strict consumers).
fn finite_corr(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

/// Mono-downmix RMS over a frame range.
fn mono_rms(samples: &[f32], channels: usize, start: usize, end: usize) -> f32 {
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

fn mono_peak(samples: &[f32], channels: usize, start: usize, end: usize) -> f32 {
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

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return SILENCE_FLOOR_DB;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// `1.0 - peak-normalized range` of a gated-energy envelope: 1.0 = perfectly flat, →0 = strong contour.
fn flatness(bins: &[f32]) -> f32 {
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

fn structure_params_for(
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

/// Owned result of a structure+waveform placement on B at one A bracket.
struct PlacementScores {
    start_frame: usize,
    /// B-derived fill length the end sweep chose. Differs from `gap_frames` by up to
    /// `fill_length_slack_frames`; the only observable of the end search's decision.
    fill_frames: usize,
    structure_pre: f64,
    structure_post: f64,
    seam_pre: f64,
    seam_post: f64,
    per_channel: Vec<(f64, f64)>,
    selected_channels: Vec<usize>,
    mono_pre: f64,
    mono_post: f64,
}

/// Inputs for [`place_on_b`] — structure+waveform unified search on the B haystack.
struct PlaceOnBInput<'a> {
    a_samples: &'a [f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_haystack: &'a [f32],
    b_mono: &'a [f64],
    b_ch: &'a [Vec<f64>],
    nominal_fill_start: usize,
    context_frames: usize,
    bin_frames: usize,
    search_radius_frames: usize,
    cfg: &'a FingerprintConfig,
}

/// Structure-best placement of the A bracket `refined` on the B haystack, plus the seam there.
/// Structure-dominant weights so the placement locks to the energy-best (mirrors the production
/// "structure aligns, seam read there" story). `None` if the match degenerates.
fn place_on_b(input: &PlaceOnBInput<'_>) -> Option<PlacementScores> {
    let PlaceOnBInput {
        a_samples,
        channels,
        refined,
        b_haystack,
        b_mono,
        b_ch,
        nominal_fill_start,
        context_frames,
        bin_frames,
        search_radius_frames,
        cfg,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.checked_sub(refined.start_frame)?;
    if gap_frames == 0 {
        return None;
    }
    let border_frames = bin_frames * 3;
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames,
        border_standoff_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, ch, &border_spec);
    let pre_window = a_pre.len().max(1);
    let post_window = a_post.len().max(1);
    let templates = SeamTemplates {
        a_pre: &a_pre,
        a_post: &a_post,
        a_pre_ch: &a_pre_ch,
        a_post_ch: &a_post_ch,
        b_mono,
        b_ch,
    };
    let waveform = WaveformSeamContext {
        templates: &templates,
        gap_frames,
        pre_window,
        post_window,
        b_total_frames: b_mono.len(),
        repeat_window_frames: bin_frames.max(1),
        repeat_penalty_weight: 0.0,
    };
    let structure_params = structure_params_for(
        cfg,
        gap_frames,
        bin_frames,
        search_radius_frames,
        bin_frames,
    );
    let signature = build_gap_signature(
        a_samples,
        ch,
        refined.start_frame,
        refined.end_frame,
        context_frames,
        &structure_params,
        GapSignatureMode::Energy,
    );
    let nominal_fill_end = nominal_fill_start + gap_frames;
    // Structure-only placement, deliberately — NOT production's weights.
    //
    // `seam_pre`/`seam_post` recorded here feed `classify_bracket_stage`, and those two fields ship
    // with no prominence or z companion. Read at a structure-chosen placement they mean "structure
    // found a placement; does the waveform corroborate?", which is what makes `waveform_floor` a
    // meaningful failure stage. Let the seam influence the placement and they become an unguarded
    // argmax over the search radius — max-of-noise, and the stage stops distinguishing anything.
    //
    // This is an argument about *these two fields*, not about seam-chosen placement in general:
    // `splice_dualfit_at` places each shoulder at its own seam peak unconditionally, and answers the
    // same bias concern by publishing validators (`*_seam_prom`, `*_seam_z`, `post_seam_global_r`)
    // rather than by abstaining. A production-weights placement is therefore fine to add — as
    // *additional* fields carrying their own prominence, never by flipping this weight in place.
    // See docs/dev/archive/TEMP-fill-placement-axis-plan.md, Phase B.
    let weights = UnifiedFitWeights {
        structure_weight: 1.0,
        waveform_weight: 0.0,
        nominal_bias_scale: 0.0,
        late_start_penalty_scale: 0.0,
    };
    let matched = match_gap_fill_unified_in_b(
        &UnifiedFillSearchInput {
            signature: &signature,
            b_samples: b_haystack,
            channels,
            waveform: &waveform,
            nominal_fill_start,
            nominal_fill_end,
        },
        &structure_params,
        weights,
    )?;
    let start = matched.alignment.start_frame;
    let diag = seam_channel_diagnostics(
        &templates,
        SeamPlacement {
            start,
            gap_frames,
            pre_window,
            post_window,
        },
    );
    Some(PlacementScores {
        start_frame: start,
        fill_frames: matched.alignment.fill_frames,
        structure_pre: matched.structure_pre,
        structure_post: matched.structure_post,
        seam_pre: matched.alignment.pre_correlation,
        seam_post: matched.alignment.post_correlation,
        per_channel: diag.per_channel,
        selected_channels: diag.selected,
        mono_pre: diag.mono.0,
        mono_post: diag.mono.1,
    })
}

/// B-frame index in the decoded B haystack for the gross A→B map at `a_start_frame`
/// (`geometry.b_mapped_*` — ledger A2 / §3.7).
fn b_mapped_frame_in_haystack(
    a_start_frame: usize,
    sample_rate: f64,
    gap_offset_secs: f64,
    b_extract_start_secs: f64,
) -> usize {
    (((a_start_frame as f64 / sample_rate + gap_offset_secs - b_extract_start_secs) * sample_rate)
        .round()
        .max(0.0)) as usize
}

/// Correlate one A border shoulder against B over ±`params.max_lag` around `side.anchor_frame`.
fn lag_side_sweep(side: LagSideSweep<'_>, params: LagSweepParams) -> Option<LagSummary> {
    let ml = params.ml();
    let w = params.window.min(side.a_border.len());
    let in_range = if side.pre_shoulder {
        w >= 8 && side.anchor_frame >= w + ml
    } else {
        w >= 8 && side.anchor_frame >= ml
    };
    if !in_range {
        return None;
    }
    let (lo, hi) = if side.pre_shoulder {
        let lo = side.anchor_frame - w - ml;
        let hi = (side.anchor_frame + ml).min(side.b_signal.len());
        (lo, hi)
    } else {
        let lo = side.anchor_frame - ml;
        let hi = (side.anchor_frame + w + ml).min(side.b_signal.len());
        (lo, hi)
    };
    if hi <= lo {
        return None;
    }
    let a_win = if side.pre_shoulder {
        &side.a_border[side.a_border.len() - w..]
    } else {
        &side.a_border[..w]
    };
    let curve = lag_correlation_curve_auto(a_win, &side.b_signal[lo..hi], params.max_lag);
    if side.gross_lag_shift == 0 {
        summarize_lag_curve(
            &curve,
            params.sample_rate,
            params.win_ms(),
            params.max_lag_ms(),
            params.channel,
        )
    } else {
        let gross_curve: Vec<(i64, f64)> = curve
            .into_iter()
            .map(|(l, r)| (l + side.gross_lag_shift, r))
            .collect();
        summarize_lag_curve(
            &gross_curve,
            params.sample_rate,
            params.win_ms(),
            params.max_lag_ms(),
            params.channel,
        )
    }
}

fn lag_pair(
    a_pre: &[f64],
    a_post: &[f64],
    b_signal: &[f64],
    start_frame: usize,
    gap_frames: usize,
    params: LagSweepParams,
) -> (Option<LagSummary>, Option<LagSummary>) {
    let pre = lag_side_sweep(
        LagSideSweep {
            a_border: a_pre,
            b_signal,
            anchor_frame: start_frame,
            pre_shoulder: true,
            gross_lag_shift: 0,
        },
        params,
    );
    // Sequential per-shoulder registration (ledger A2 sequential registration): center the post
    // search on `start_frame + gap_frames + round(L_pre)`, not the naive `start_frame + gap_frames`.
    // Un-shifted centering forces `|L_pre + (D_B - D_A)|` (clip offset stacked with bridge-length
    // mismatch) into one ±max_lag window; shifting by the measured pre lag isolates the bridge mismatch
    // alone in the post search. The returned post lags are then shifted back so callers keep receiving
    // "gross" lags relative to `start_frame + gap_frames` (`b_mapped_end`) — i.e. `L_post_gross =
    // L_pre + L_post_fine` — for compatibility with existing consumers (`splice_summary_from_lag`,
    // `b_mapped_end`-relative alignment).
    let pre_shift = pre.map(|p| p.frac_lag_samples.round() as i64).unwrap_or(0);
    let post_base = (start_frame as i64 + gap_frames as i64 + pre_shift).max(0) as usize;
    let post = lag_side_sweep(
        LagSideSweep {
            a_border: a_post,
            b_signal,
            anchor_frame: post_base,
            pre_shoulder: false,
            gross_lag_shift: pre_shift,
        },
        params,
    );
    (pre, post)
}

/// Inputs for [`lag_at_placement`] — A/B windows + gate placement for a single lag fingerprint.
struct LagAtPlacementInput<'a> {
    a_samples: &'a [f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_mono: &'a [f64],
    b_ch: &'a [Vec<f64>],
    selected: Option<usize>,
    start_frame: usize,
    cfg: &'a FingerprintConfig,
    sample_rate: u32,
}

/// Lag fingerprint for a placement: A's kept border vs the B haystack swept over ±`max_lag`, on the
/// mono downmix **and** the gate-selected channel (where a multichannel failure lives).
fn lag_at_placement(input: &LagAtPlacementInput<'_>) -> LagFingerprint {
    let LagAtPlacementInput {
        a_samples,
        channels,
        refined,
        b_mono,
        b_ch,
        selected,
        start_frame,
        cfg,
        sample_rate,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    // §3.6a: uniqueness needs a ~1 s window. Discover that much border (plus the lag-search slack) so the
    // template isn't capped to the ~150 ms `bin_frames*3` used for the structure probe.
    let window = ((cfg.lag_window_secs * f64::from(sample_rate)).round() as usize).max(8);
    let max_lag = ((cfg.lag_max_lag_ms as f64 / 1000.0) * f64::from(sample_rate)).round() as i64;
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: window + max_lag.max(0) as usize,
        border_standoff_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, ch, &border_spec);

    let sweep = LagSweepParams {
        window,
        max_lag,
        sample_rate,
        channel: LagChannel::Mono,
    };

    let mut out = LagFingerprint::default();
    let mut add = |pre: Option<LagSummary>, post: Option<LagSummary>| {
        if let Some(p) = pre {
            out.pre_anchor.push(p);
        }
        if let Some(p) = post {
            out.post_anchor.push(p);
        }
    };

    let (pre, post) = lag_pair(&a_pre, &a_post, b_mono, start_frame, gap_frames, sweep);
    add(pre, post);

    if let Some(sel) = selected {
        if sel < b_ch.len()
            && sel < a_pre_ch.len()
            && sel < a_post_ch.len()
            && !a_pre_ch[sel].is_empty()
        {
            let (pre, post) = lag_pair(
                &a_pre_ch[sel],
                &a_post_ch[sel],
                &b_ch[sel],
                start_frame,
                gap_frames,
                LagSweepParams {
                    channel: LagChannel::Selected(sel),
                    ..sweep
                },
            );
            add(pre, post);
        }
    }
    out
}

/// `~10 ms`-bin RMS envelope — the encoding-/small-shift-robust representation for the seam probe.
fn fine_rms_envelope(x: &[f64], bin: usize) -> Vec<f64> {
    let bin = bin.max(1);
    x.chunks(bin)
        .map(|c| (c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).sqrt())
        .collect()
}

/// RMS of the **energy-weighted** downmix over interleaved frames `[lo, hi)`. Each channel is weighted by
/// its own energy, so a loud center isn't diluted by quiet surrounds/LFE the way a straight `1/N` mix is
/// (which buried these 5.1 seams 13–15 dB and over-flagged them "quiet" — §3.6a froze level on this mix).
/// Correlation stays on mono; only the level/SNR uses this. `0.0` for an empty/over-range span.
fn weighted_downmix_rms(samples: &[f32], channels: usize, lo: usize, hi: usize) -> f64 {
    let ch = channels.max(1);
    let total_frames = samples.len() / ch;
    let hi = hi.min(total_frames);
    if hi <= lo {
        return 0.0;
    }
    let n = (hi - lo) as f64;
    let mut ms = vec![0.0f64; ch];
    for f in lo..hi {
        let base = f * ch;
        for (c, m) in ms.iter_mut().enumerate() {
            let s = samples.get(base + c).copied().unwrap_or(0.0) as f64;
            *m += s * s;
        }
    }
    let total: f64 = ms.iter().sum();
    if total <= f64::EPSILON {
        return 0.0;
    }
    let weights: Vec<f64> = ms.iter().map(|e| e / total).collect();
    let mut acc = 0.0;
    for f in lo..hi {
        let base = f * ch;
        let y: f64 = weights
            .iter()
            .enumerate()
            .map(|(c, w)| samples.get(base + c).copied().unwrap_or(0.0) as f64 * w)
            .sum();
        acc += y * y;
    }
    (acc / n).sqrt()
}

/// One side's [`SeamProbe`]: waveform Pearson at lag 0, recovery over ±`fine_max_lag`, encoding-robust
/// envelope correlation, and level/SNR. Correlation is on the mono `a_win`; the level/SNR is taken from
/// `level_rms` (the energy-weighted-downmix RMS at the raw seam, computed by the caller). `b_ctx` spans
/// `±fine_max_lag` around the seam (lag 0 ⇒ `b_ctx[fine_max_lag .. + a_win.len()]`).
fn seam_probe_side(
    a_win: &[f64],
    b_ctx: &[f64],
    level_rms: f64,
    fine_max_lag: i64,
    sample_rate: u32,
    fine_bin: usize,
    gap_floor_db: f64,
) -> Option<SeamProbe> {
    let w = a_win.len();
    let ml = fine_max_lag.max(0) as usize;
    let rate = f64::from(sample_rate).max(1.0);
    if w < 8 || ml + w > b_ctx.len() {
        return None;
    }
    let curve = lag_correlation_curve(a_win, b_ctx, fine_max_lag);
    if curve.is_empty() {
        return None;
    }
    let waveform_r = curve
        .iter()
        .find(|(l, _)| *l == 0)
        .map(|(_, r)| *r)
        .unwrap_or(f64::NAN);
    let &(peak_lag, recovered_r) = curve
        .iter()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
    let b0 = &b_ctx[ml..ml + w];
    let envelope_r = normalized_correlation(
        &fine_rms_envelope(a_win, fine_bin),
        &fine_rms_envelope(b0, fine_bin),
    );
    let bandlimited_r = crate::domain::seam_robust::bandlimited_pearson(
        a_win,
        b0,
        sample_rate,
        crate::domain::seam_robust::BANDLIMITED_CUTOFF_HZ,
    );
    let spectrum_r = crate::domain::seam_robust::spectrum_correlation(a_win, b0);
    let rms_db = f64::from(to_db(level_rms as f32));
    Some(SeamProbe {
        waveform_r: finite_corr(waveform_r),
        recovered_r: finite_corr(recovered_r),
        recovered_lag_ms: peak_lag as f64 * 1000.0 / rate,
        bandlimited_r: finite_corr(bandlimited_r),
        spectrum_r: finite_corr(spectrum_r),
        envelope_r: finite_corr(envelope_r),
        rms_db,
        snr_db: rms_db - gap_floor_db,
    })
}

const SEAM_PROBE_FINE_LAG_MS: f64 = 25.0;
const SEAM_PROBE_ENV_BIN_MS: f64 = 10.0;

/// Pre/post [`SeamProbe`]s at a placement (mono). Built at **`b_mapped`** registration to diagnose a dead
/// waveform seam: recovery (mis-alignment) vs encoding-robust envelope (cross-encoding) vs level/SNR.
/// `post_shift_frames` is the measured pre-side lag (rounded, from `lag_decision`'s mono pre entry) —
/// the same sequential-registration shift `lag_pair` applies (ledger A2 sequential registration),
/// so the post probe isn't centered on the un-shifted `start_frame + gap_frames` while `lag_decision`'s
/// post search is. The post fine-lag half-width is also raised to `cfg.lag_max_lag_ms` (from the ±25 ms
/// `SEAM_PROBE_FINE_LAG_MS`) since, even after shifting, the residual search still needs to cover the
/// bridge-length mismatch (`D_B - D_A`), not just fine sub-frame jitter. `recovered_lag_ms` is reported
/// gross-relative (shifted back by `post_shift_frames`) to stay comparable with `lag_decision`.
struct SeamProbeAtPlacementInput<'a> {
    a_samples: &'a [f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_mono: &'a [f64],
    start_frame: usize,
    post_shift_frames: i64,
    bin_frames: usize,
    cfg: &'a FingerprintConfig,
    sample_rate: u32,
    gap_floor_db: f64,
}

fn seam_probe_at_placement(input: &SeamProbeAtPlacementInput<'_>) -> SeamProbeFingerprint {
    let SeamProbeAtPlacementInput {
        a_samples,
        channels,
        refined,
        b_mono,
        start_frame,
        post_shift_frames,
        bin_frames,
        cfg,
        sample_rate,
        gap_floor_db,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let rate = f64::from(sample_rate).max(1.0);
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: bin_frames * 3,
        border_standoff_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);
    let window = ((cfg.fill_seam_search_secs * rate).round() as usize).max(8);
    let fine_max_lag = ((SEAM_PROBE_FINE_LAG_MS / 1000.0) * rate).round() as i64;
    let fine_bin = ((SEAM_PROBE_ENV_BIN_MS / 1000.0) * rate).round().max(1.0) as usize;
    let ml = fine_max_lag.max(0) as usize;
    let post_max_lag = ((cfg.lag_max_lag_ms as f64 / 1000.0) * rate).round() as i64;
    let post_ml = post_max_lag.max(0) as usize;

    let pre = (|| {
        let w = window.min(a_pre.len());
        if w < 8 || start_frame < w + ml {
            return None;
        }
        let lo = start_frame - w - ml;
        let hi = (start_frame + ml).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        // Level over the raw seam span (the w frames ending at the gap), on the energy-weighted downmix.
        let level_rms = weighted_downmix_rms(a_samples, ch, start_frame - w, start_frame);
        seam_probe_side(
            &a_pre[a_pre.len() - w..],
            &b_mono[lo..hi],
            level_rms,
            fine_max_lag,
            sample_rate,
            fine_bin,
            gap_floor_db,
        )
    })();
    let post = (|| {
        let w = window.min(a_post.len());
        let post_base =
            (start_frame as i64 + gap_frames as i64 + post_shift_frames).max(0) as usize;
        if w < 8 || post_base < post_ml {
            return None;
        }
        let lo = post_base - post_ml;
        let hi = (post_base + w + post_ml).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        // Level over the raw seam span (the w frames starting at the gap end), energy-weighted downmix.
        let level_rms = weighted_downmix_rms(a_samples, ch, post_base, post_base + w);
        seam_probe_side(
            &a_post[..w],
            &b_mono[lo..hi],
            level_rms,
            post_max_lag,
            sample_rate,
            fine_bin,
            gap_floor_db,
        )
    })();
    let post_shift_ms = post_shift_frames as f64 * 1000.0 / rate;
    let post = post.map(|mut sp| {
        sp.recovered_lag_ms += post_shift_ms;
        sp
    });
    SeamProbeFingerprint { pre, post }
}

/// Inputs for [`splice_dualfit_at`] — the A/B PCM plus the **nominal** `b_mapped` gap-start frame. The seam
/// search re-anchors on nominal (not the gross `lag_decision`), so it needs only the geometry anchor; the
/// pre shoulder butts at `b_mapped_start`, the post at `b_mapped_start + gap_frames`.
struct SpliceDualfitInput<'a> {
    a_samples: &'a [f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_mono: &'a [f64],
    /// Nominal geometry `b_mapped` gap-start frame (no lag adjustment).
    b_mapped_start: usize,
    cfg: &'a FingerprintConfig,
    sample_rate: u32,
}

/// Dual-fit viability: score the pre/post seams at each shoulder's own seam-local lag, over the gate's
/// `fill_seam_search_secs` window, and gate on `min(pre, post)`. Mirrors production `try_dual_fit`'s
/// shoulder search so [`dual_fit_rescue_flag`] can predict rescue from dump measurements (F14).
///
/// **A borders are raw `mono(refined ± w)`** — the same construction `build_dual_fit_input` /
/// `try_dual_fit` use. Do **not** route through [`border_templates_for_gap`]: silence-skip + energy
/// trim moves the pre template, which moves `pre_lag` and therefore `post_seam_global_r`, flipping
/// the step-real term on gaps production rescues.
///
/// `None` when either seam window falls out of range.
fn splice_dualfit_at(input: &SpliceDualfitInput<'_>) -> Option<SpliceDualfit> {
    let SpliceDualfitInput {
        a_samples,
        channels,
        refined,
        b_mono,
        b_mapped_start,
        cfg,
        sample_rate,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let rate = f64::from(sample_rate).max(1.0);
    let window = ((cfg.fill_seam_search_secs * rate).round() as usize).max(8);
    let a_frames = a_samples.len() / ch;
    // Same range guard as `build_dual_fit_input` — insufficient shoulder ⇒ no dual-fit claim.
    if refined.start_frame < window || refined.end_frame + window > a_frames || gap_frames == 0 {
        return None;
    }
    let a_pre = interleaved_to_mono(
        &a_samples[(refined.start_frame - window) * ch..refined.start_frame * ch],
        ch,
    );
    let a_post = interleaved_to_mono(
        &a_samples[refined.end_frame * ch..(refined.end_frame + window) * ch],
        ch,
    );
    debug_assert_eq!(a_pre.len(), window);
    debug_assert_eq!(a_post.len(), window);

    let max_lag = (((SEAM_LOCAL_SEARCH_MS / 1000.0) * rate).round() as usize).max(1);

    // Seam-local viability, **re-anchored on nominal `b_mapped`**: the fill places each shoulder at the lag
    // that maximizes ITS own seam, so search ±`max_lag` around the nominal shoulder (pre butts at
    // `b_mapped_start`, post at `b_mapped_start + gap_frames`) and take the peak. Anchoring on the gross 1 s
    // `lag_decision` (the prior behavior) clipped seams whose lag diverges far from the 1 s peak — e.g. 7·g3,
    // pre 1 s lag −319 ms but the seam is at +18 ms, outside any narrow window around the gross placement.
    // Production `try_dual_fit` uses the same nominal centers (`dual_fit.rs`).
    let b_pre_nominal = b_mapped_start;
    let b_post_nominal = b_mapped_start + gap_frames;
    let pre_start = b_pre_nominal.checked_sub(window)?;
    let (pre_seam_r, pre_lag, pre_seam_z) = seam_local_peak(&a_pre, b_mono, pre_start, max_lag)?;
    let (post_seam_r, post_lag, post_seam_z) =
        seam_local_peak(&a_post, b_mono, b_post_nominal, max_lag)?;

    // Keep raw correlations — including NaN from zero-variance (digital silence) windows. Do **not**
    // `finite_corr` here: mapping NaN→0.0 inverts both of production's NaN decisions (gate_pass would
    // fail where `NaN < floor` does not; step-real would pass where `partial_cmp(NaN)` declines). F14.
    // The seam-local shoulder placements (nominal ± the per-seam search) define the bridge + step.
    let b_pre_seam = (b_pre_nominal as i64 + pre_lag).max(0) as usize;
    let b_post_seam = (b_post_nominal as i64 + post_lag).max(0) as usize;
    let bridge_frames = b_post_seam as i64 - b_pre_seam as i64;
    let smin = pre_seam_r.min(post_seam_r);
    // Same form as `try_dual_fit`: decline only when `smin < floor` (NaN < x is false ⇒ gate passes).
    let gate_pass =
        !(smin < f64::from(cfg.min_fill_correlation) || smin < f64::from(cfg.fill_absolute_floor));

    // Validator 1 — is the step necessary? Post seam at the PRE shoulder's seam-local lag (step forced 0):
    // if the post seam also clears there, one constant shift fixes both ⇒ registration artifact, not a splice.
    let b_post_global = b_pre_seam + gap_frames;
    let post_seam_global_r = if window >= 8 && b_post_global + window <= b_mono.len() {
        normalized_correlation(&a_post, &b_mono[b_post_global..b_post_global + window])
    } else {
        f64::NAN
    };

    // Validator 2 — is each seam a unique (non-periodic) match? Prominence of the placement peak over its
    // best rival within ±30 ms.
    let ml = ((DUALFIT_SEAM_UNIQ_LAG_MS / 1000.0) * rate).round() as i64;
    let mlu = ml.max(0) as usize;
    let pre_seam_prom =
        (window >= 8 && b_pre_seam >= window + mlu && b_pre_seam + mlu <= b_mono.len())
            .then(|| {
                seam_prominence(
                    &a_pre,
                    &b_mono[b_pre_seam - window - mlu..b_pre_seam + mlu],
                    ml,
                    sample_rate,
                )
            })
            .flatten();
    let post_seam_prom =
        (window >= 8 && b_post_seam >= mlu && b_post_seam + window + mlu <= b_mono.len())
            .then(|| {
                seam_prominence(
                    &a_post,
                    &b_mono[b_post_seam - mlu..b_post_seam + window + mlu],
                    ml,
                    sample_rate,
                )
            })
            .flatten();

    Some(SpliceDualfit {
        pre_seam_r,
        post_seam_r,
        gap_frames,
        bridge_frames,
        trim_frames: bridge_frames - gap_frames as i64,
        gate_pass,
        post_seam_global_r,
        pre_seam_prom,
        post_seam_prom,
        pre_seam_z,
        post_seam_z,
    })
}

/// §3.6a frozen wide-envelope confirmer: 100 ms RMS bins, 2 s seam window, ±400 ms lag on the envelope.
const WIDE_ENV_BIN_MS: f64 = 100.0;
const WIDE_ENV_WINDOW_SECS: f64 = 2.0;
const WIDE_ENV_MAX_LAG_MS: f64 = 400.0;

/// Mono [`LagSummary`] for one side of a [`LagFingerprint`].
fn mono_lag_side(lag: &LagFingerprint, pre: bool) -> Option<&LagSummary> {
    let entries = if pre {
        &lag.pre_anchor
    } else {
        &lag.post_anchor
    };
    entries.iter().find(|s| s.channel == LagChannel::Mono)
}

/// First-class splice summary from decision-seam `lag_decision` (mono): step + per-side peaks / `peak_z`.
pub fn splice_summary_from_lag(lag: &LagFingerprint) -> Option<SpliceSummary> {
    let pre = mono_lag_side(lag, true)?;
    let post = mono_lag_side(lag, false)?;
    // `step` spans both shoulders, so either shoulder being search-exhausted taints it. `None` only when
    // neither side recorded the flag (pre-edge-pin fingerprints).
    let edge_pinned = match (pre.edge_pinned, post.edge_pinned) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(false) || b.unwrap_or(false)),
    };
    Some(SpliceSummary {
        step_ms: post.frac_lag_ms - pre.frac_lag_ms,
        pre_peak_r: pre.peak_r,
        post_peak_r: post.peak_r,
        pre_peak_z: pre.peak_z,
        post_peak_z: post.peak_z,
        edge_pinned,
    })
}

/// One side of the wide-envelope segment-identity confirmer: bucketed RMS envelope lag peak + prominence.
fn wide_envelope_side(
    a_win: &[f64],
    b_wave_ctx: &[f64],
    sample_rate: u32,
    env_bin: usize,
    wide_lag_samples: usize,
) -> Option<EnvPeak> {
    if a_win.len() < env_bin.max(8) {
        return None;
    }
    let rate = f64::from(sample_rate).max(1.0);
    let ea = fine_rms_envelope(a_win, env_bin);
    let eb = fine_rms_envelope(b_wave_ctx, env_bin);
    if ea.len() < 3 {
        return None;
    }
    let env_max_lag = (wide_lag_samples / env_bin.max(1)) as i64;
    if env_max_lag < 1 {
        return None;
    }
    let curve = lag_correlation_curve(&ea, &eb, env_max_lag);
    if curve.is_empty() {
        return None;
    }
    let (pi, &(peak_lag_bin, peak_r)) = curve
        .iter()
        .enumerate()
        .max_by(|(_, (_, x)), (_, (_, y))| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))?;
    let prominence = secondary_peak(&curve, pi)
        .map(|(_, r)| peak_r - r)
        .unwrap_or(0.0);
    let peak_lag_ms = peak_lag_bin as f64 * env_bin as f64 * 1000.0 / rate;
    Some(EnvPeak {
        peak_r: finite_corr(peak_r),
        peak_lag_ms,
        prominence: finite_corr(prominence),
    })
}

/// Pre/post wide-envelope confirmers at **`b_mapped`** registration — cross-scale check vs `lag_decision`.
/// `post_shift_frames` mirrors `lag_pair`'s sequential centering (ledger A2): the post window is centered on `start_frame + gap_frames + post_shift_frames`, and its
/// search half-width is raised to `cfg.lag_max_lag_ms` (aligned with `lag_decision`, not the frozen
/// ±400 ms `WIDE_ENV_MAX_LAG_MS`) so it can still resolve the bridge-length mismatch after shifting.
/// `peak_lag_ms` is reported gross-relative for comparability with `lag_decision`.
struct WideEnvelopeAtPlacementInput<'a> {
    a_samples: &'a [f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_mono: &'a [f64],
    start_frame: usize,
    post_shift_frames: i64,
    cfg: &'a FingerprintConfig,
    sample_rate: u32,
}

fn wide_envelope_at_placement(input: &WideEnvelopeAtPlacementInput<'_>) -> WideEnvelopeFingerprint {
    let WideEnvelopeAtPlacementInput {
        a_samples,
        channels,
        refined,
        b_mono,
        start_frame,
        post_shift_frames,
        cfg,
        sample_rate,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let rate = f64::from(sample_rate).max(1.0);
    let window = ((WIDE_ENV_WINDOW_SECS * rate).round() as usize).max(8);
    let wide_lag = ((WIDE_ENV_MAX_LAG_MS / 1000.0) * rate).round() as usize;
    let post_wide_lag = ((cfg.lag_max_lag_ms as f64 / 1000.0) * rate).round() as usize;
    let env_bin = ((WIDE_ENV_BIN_MS / 1000.0) * rate).round().max(1.0) as usize;
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: (window + wide_lag).max(window + post_wide_lag),
        border_standoff_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);

    let pre = (|| {
        let w = window.min(a_pre.len());
        if w < env_bin || start_frame < w + wide_lag {
            return None;
        }
        let lo = start_frame.saturating_sub(w + wide_lag);
        let hi = (start_frame + wide_lag).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        wide_envelope_side(
            &a_pre[a_pre.len() - w..],
            &b_mono[lo..hi],
            sample_rate,
            env_bin,
            wide_lag,
        )
    })();
    let post = (|| {
        let w = window.min(a_post.len());
        let post_base =
            (start_frame as i64 + gap_frames as i64 + post_shift_frames).max(0) as usize;
        if w < env_bin || post_base < post_wide_lag {
            return None;
        }
        let lo = post_base.saturating_sub(post_wide_lag);
        let hi = (post_base + w + post_wide_lag).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        wide_envelope_side(
            &a_post[..w],
            &b_mono[lo..hi],
            sample_rate,
            env_bin,
            post_wide_lag,
        )
    })();
    let post_shift_ms = post_shift_frames as f64 * 1000.0 / rate;
    let post = post.map(|mut ep| {
        ep.peak_lag_ms += post_shift_ms;
        ep
    });
    WideEnvelopeFingerprint { pre, post }
}

fn classify_bracket_stage(
    structure_pre: f64,
    structure_post: f64,
    seam_pre: f64,
    seam_post: f64,
    cfg: &FingerprintConfig,
) -> Option<FailureStage> {
    if structure_pre.min(structure_post) < cfg.min_structure_match_score {
        return Some(FailureStage::StructureFloor);
    }
    if seam_pre.min(seam_post) < f64::from(cfg.fill_absolute_floor) {
        return Some(FailureStage::WaveformFloor);
    }
    None
}

/// Build a fingerprint for one gap from decoded windows.
///
/// When `skip_baseline_placement` is true (the pre-gate summary pass before the gate overlay in
/// [`characterize_gaps_from_decode`]), the expensive unified `place_on_b` search is omitted — the gate
/// overlay supplies authoritative seams and brackets (D12 §3 step 1).
pub fn build_gap_fingerprint(
    index: usize,
    inputs: &GapInputs<'_>,
    tier: DetailTier,
    skip_baseline_placement: bool,
) -> GapFingerprint {
    let cfg = &inputs.config;
    let ch = inputs.channels.max(1);
    let rate = f64::from(inputs.sample_rate).max(1.0);
    let total_a = inputs.a_samples.len() / ch;

    let bin_frames = ((cfg.gap_signature_bin_ms as f64 / 1000.0) * rate)
        .round()
        .max(1.0) as usize;
    let context_frames = (cfg.gap_signature_context_secs * rate).round() as usize;
    let border_frames = (cfg.border_secs * rate).round() as usize;
    let max_refine_frames = (cfg.max_refine_secs * rate).round() as usize;

    let refined = refine_gap_frames(
        inputs.a_samples,
        ch,
        inputs.reported_start_frame.min(total_a),
        inputs.reported_end_frame.min(total_a),
        cfg.silence_peak_fraction,
        cfg.absolute_silence_rms,
        max_refine_frames,
    );
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);

    let geometry = GapGeometry {
        a_start_secs: inputs.reported_start_frame as f64 / rate,
        a_end_secs: inputs.reported_end_frame as f64 / rate,
        a_refined_start_secs: refined.start_frame as f64 / rate,
        a_refined_end_secs: refined.end_frame as f64 / rate,
        duration_secs: gap_frames as f64 / rate,
        b_mapped_start_secs: inputs
            .b_haystack
            .map(|_| refined.start_frame as f64 / rate + inputs.gap_offset_secs),
        b_mapped_end_secs: inputs
            .b_haystack
            .map(|_| refined.end_frame as f64 / rate + inputs.gap_offset_secs),
        fill_offset_secs: inputs.b_haystack.map(|_| inputs.gap_offset_secs),
    };

    // --- intrinsic (A-side) ---
    let pre_start = refined.start_frame.saturating_sub(context_frames);
    let post_end = (refined.end_frame + context_frames).min(total_a);
    let (levels, _context_bins) = level_profile(
        |f, end| mono_rms(inputs.a_samples, ch, f, end),
        LevelProfileSpan {
            gap_start: refined.start_frame,
            gap_end: refined.end_frame,
            context_start: pre_start,
            context_end: post_end,
        },
        bin_frames,
        cfg.gap_signature_bin_ms as u32,
    );

    let collar_start = refined.start_frame.saturating_sub(border_frames);
    let collar_rms = mono_rms(inputs.a_samples, ch, collar_start, refined.start_frame);
    let collar_peak = mono_peak(inputs.a_samples, ch, collar_start, refined.start_frame);
    let collar_ratio = if collar_peak > 0.0 {
        collar_rms / collar_peak
    } else {
        0.0
    };
    let silence = Some(SilenceProfile {
        collar_rms_peak_ratio: collar_ratio,
        collar_above_relative_floor: collar_ratio >= cfg.silence_peak_fraction,
        silence_peak_fraction: cfg.silence_peak_fraction,
    });

    let sig_params = structure_params_for(cfg, gap_frames.max(1), bin_frames, 0, 0);
    let signature = build_gap_signature(
        inputs.a_samples,
        ch,
        refined.start_frame,
        refined.end_frame,
        context_frames,
        &sig_params,
        GapSignatureMode::Energy,
    );
    let pre_env = energy_bins(
        inputs.a_samples,
        ch,
        pre_start,
        refined.start_frame,
        bin_frames.max(1),
        cfg.silence_peak_fraction,
        cfg.absolute_silence_rms,
    );
    let post_env = energy_bins(
        inputs.a_samples,
        ch,
        refined.end_frame,
        post_end,
        bin_frames.max(1),
        cfg.silence_peak_fraction,
        cfg.absolute_silence_rms,
    );
    let contour = Some(ContourInfo {
        has_anchor_seam_contour: signature.has_anchor_seam_contour(),
        pre_flatness: flatness(&pre_env),
        post_flatness: flatness(&post_env),
    });

    let bracket_params = AnchorSeamParams {
        context_frames,
        max_anchors_per_side: cfg.max_anchors_per_side,
        max_bracket_frames: (cfg.max_anchor_bracket_secs * rate).round().max(1.0) as usize,
        min_prominence: cfg.anchor_seam_min_prominence,
        structure: structure_params_for(cfg, gap_frames.max(1), bin_frames, 0, 0),
    };
    let candidates = list_anchor_candidates_a(inputs.a_samples, ch, refined, &bracket_params);
    let map_anchor = |c: &crate::domain::gap_anchor_seam::AnchorCandidate| AnchorPoint {
        time_secs: c.frame as f64 / rate,
        source: match c.source {
            AnchorSource::ScanRefined => AnchorSourceKind::ScanRefined,
            AnchorSource::EnergyPeak => AnchorSourceKind::EnergyPeak,
            AnchorSource::BoolTransition => AnchorSourceKind::BoolTransition,
        },
        prominence: c.prominence,
        rms_db: to_db((c.rms.exp() - 1.0).max(0.0)),
    };
    let anchors = Some(AnchorSet {
        pre: candidates.pre.iter().map(map_anchor).collect(),
        post: candidates.post.iter().map(map_anchor).collect(),
    });
    let raw_brackets = list_feasible_anchor_brackets(&candidates, refined, &bracket_params);

    // --- pairwise (B present) ---
    let mut structure = None;
    let mut seams = None;
    let mut lag_editorial = None;
    let mut lag_decision = None;
    // Per-bracket scoring in Full tier; Summary tier only needs baseline structure/seam at `b_mapped`.
    let mut brackets: Vec<BracketInfo> = raw_brackets
        .iter()
        .map(|b| BracketInfo {
            pre_time_secs: b.pre.frame as f64 / rate,
            post_time_secs: b.post.frame as f64 / rate,
            span_secs: (b.post.frame.saturating_sub(b.pre.frame)) as f64 / rate,
            move_frames: b.move_frames,
            structure_pre: None,
            structure_post: None,
            seam_pre: None,
            seam_post: None,
            start_frame: None,
            fill_frames: None,
            failure_stage: None,
            residual_margin_db: None,
        })
        .collect();

    if let Some(b_haystack) = inputs.b_haystack {
        let b_mono = interleaved_to_mono(b_haystack, ch);
        let b_ch = interleaved_to_channels(b_haystack, ch);
        let search_radius_frames =
            ((cfg.fill_border_search_secs.max(cfg.fill_align_margin_secs)) * rate).round() as usize;
        // Each A boundary maps to its own B nominal: a_time + gap_offset, in haystack frame coords.
        let gap_offset_secs = inputs.gap_offset_secs;
        let b_mapped_start = |a_start_frame: usize| -> usize {
            b_mapped_frame_in_haystack(
                a_start_frame,
                rate,
                gap_offset_secs,
                inputs.b_extract_start_secs,
            )
        };

        if !skip_baseline_placement {
            if let Some(base) = place_on_b(&PlaceOnBInput {
                a_samples: inputs.a_samples,
                channels: ch,
                refined,
                b_haystack,
                b_mono: &b_mono,
                b_ch: &b_ch,
                nominal_fill_start: b_mapped_start(refined.start_frame),
                context_frames,
                bin_frames,
                search_radius_frames,
                cfg,
            }) {
                structure = Some(StructureScores {
                    baseline_pre: base.structure_pre,
                    baseline_post: base.structure_post,
                });
                seams = Some(SeamScores {
                    baseline_pre: base.seam_pre,
                    baseline_post: base.seam_post,
                    selected_channels: base.selected_channels,
                    per_channel: base.per_channel,
                    mono_pre: base.mono_pre,
                    mono_post: base.mono_post,
                });
            }
        }

        if tier == DetailTier::Full {
            // Decision-seam lag at `b_mapped` nominal + ±600 ms sweep (ledger A2) — not structure throat.
            lag_decision = Some(lag_at_placement(&LagAtPlacementInput {
                a_samples: inputs.a_samples,
                channels: ch,
                refined,
                b_mono: &b_mono,
                b_ch: &b_ch,
                selected: None,
                start_frame: b_mapped_start(refined.start_frame),
                cfg,
                sample_rate: inputs.sample_rate,
            }));
            // Per-bracket scoring; remember the best-structure energy-peak bracket's placement for lag.
            let mut best: Option<(f64, usize, RefinedGapFrames, Option<usize>)> = None;
            for (i, br) in raw_brackets.iter().enumerate() {
                let refined_b = br.refined;
                if let Some(p) = place_on_b(&PlaceOnBInput {
                    a_samples: inputs.a_samples,
                    channels: ch,
                    refined: refined_b,
                    b_haystack,
                    b_mono: &b_mono,
                    b_ch: &b_ch,
                    nominal_fill_start: b_mapped_start(refined_b.start_frame),
                    context_frames,
                    bin_frames,
                    search_radius_frames,
                    cfg,
                }) {
                    brackets[i].structure_pre = Some(p.structure_pre);
                    brackets[i].structure_post = Some(p.structure_post);
                    brackets[i].seam_pre = Some(p.seam_pre);
                    brackets[i].seam_post = Some(p.seam_post);
                    brackets[i].start_frame = Some(p.start_frame);
                    brackets[i].fill_frames = Some(p.fill_frames);
                    brackets[i].failure_stage = classify_bracket_stage(
                        p.structure_pre,
                        p.structure_post,
                        p.seam_pre,
                        p.seam_post,
                        cfg,
                    );
                    let energy_pair = br.pre.source == AnchorSource::EnergyPeak
                        && br.post.source == AnchorSource::EnergyPeak;
                    let smin = p.structure_pre.min(p.structure_post);
                    if energy_pair && best.is_none_or(|(bs, ..)| smin > bs) {
                        best = Some((
                            smin,
                            p.start_frame,
                            refined_b,
                            p.selected_channels.first().copied(),
                        ));
                    }
                } else {
                    brackets[i].failure_stage = Some(FailureStage::StructureAlign);
                }
            }
            if let Some((_, start_frame, refined_b, selected)) = best {
                lag_editorial = Some(lag_at_placement(&LagAtPlacementInput {
                    a_samples: inputs.a_samples,
                    channels: ch,
                    refined: refined_b,
                    b_mono: &b_mono,
                    b_ch: &b_ch,
                    selected,
                    start_frame,
                    cfg,
                    sample_rate: inputs.sample_rate,
                }));
            }
        }
    }

    if tier != DetailTier::Full {
        brackets.clear();
    }

    GapFingerprint {
        index,
        tier,
        sample_rate: inputs.sample_rate,
        channels: inputs.channels as u16,
        geometry,
        levels,
        silence,
        contour,
        anchors,
        brackets,
        structure,
        seams,
        lag_editorial,
        lag_decision,
        residual: None,
        seam_probe: None,
        donor_interior: None,
        donor_interior_nominal: None,
        b_levels: None,
        splice: None,
        wide_envelope: None,
        splice_dualfit: None,
        outcome: None,
        equivalence_diagnostic: None,
        equivalence_production: None,
    }
}

impl FingerprintConfig {
    /// Mirror the run's patch request so the builder enumerates the **same** brackets the gate does
    /// (matching `bin_frames` / `context` / `max_bracket` / `min_prominence` is what lets the oracle
    /// pass align per-bracket by frame).
    pub(crate) fn from_request(
        request: &crate::application::PatchAudioRequest,
        silence_peak_fraction: f32,
    ) -> Self {
        const GAP_EDGE_REFINE_SECS: f64 = 0.75;
        Self {
            silence_peak_fraction,
            absolute_silence_rms: request.absolute_silence_rms,
            gap_signature_context_secs: request.gap_signature_context_secs,
            gap_signature_bin_ms: request.gap_signature_bin_ms,
            fill_seam_search_secs: request.fill_seam_search_secs,
            fill_border_search_secs: request.fill_border_search_secs,
            fill_align_margin_secs: request.fill_align_margin_secs,
            fill_length_slack_secs: request.fill_length_slack_secs,
            fill_extract_tail_slack_secs: request.fill_extract_tail_slack_secs,
            border_secs: request.normalize_window_secs,
            border_standoff_secs: request.border_standoff_secs,
            max_anchor_bracket_secs: request.max_anchor_bracket_secs,
            max_anchors_per_side: request.max_anchors_per_side,
            anchor_seam_min_prominence: request.anchor_seam_min_prominence,
            min_structure_match_score: f64::from(request.min_structure_match_score),
            min_fill_correlation: request.min_fill_correlation,
            fill_marginal_margin: request.fill_marginal_margin,
            fill_absolute_floor: request.fill_absolute_floor,
            max_refine_secs: GAP_EDGE_REFINE_SECS,
            // `b_mapped_start`/`b_mapped_end` share a single rigid per-gap offset (gap_offset_secs) —
            // pre/post are NOT independently registered. The full-corpus rescan showed real steps up
            // to 322 ms from that nominal, so ±200 ms clips genuine peaks off the search edge and
            // manufactures false one-sided-dead/decorrelated verdicts. 600 ms matches the width already
            // validated offline (SPLICE_EXP_FINE_LAG_MS=600, §1b-i/§3.6).
            lag_max_lag_ms: 600,
            lag_window_secs: 1.0,
        }
    }
}

/// Scores + stage extracted from a gate failure — keeps structure and waveform channels separate.
struct BracketStageDetail {
    stage: FailureStage,
    structure_pre: Option<f64>,
    structure_post: Option<f64>,
    seam_pre: Option<f64>,
    seam_post: Option<f64>,
    residual_margin_db: Option<f64>,
}

/// Map a gate failure to the fingerprint's `failure_stage` and the scores that belong on each field.
/// Structure floors land on `structure_*`; waveform/residual on `seam_*`; residual also carries
/// `residual_margin_db`. Never overload structure correlations onto `seam_*`.
fn stage_of(failure: &crate::application::patch_region::SeamGateFailure) -> BracketStageDetail {
    use crate::application::patch_region::SeamGateFailure as F;
    match failure {
        F::StructureAlignmentFailed => BracketStageDetail {
            stage: FailureStage::StructureAlign,
            structure_pre: None,
            structure_post: None,
            seam_pre: None,
            seam_post: None,
            residual_margin_db: None,
        },
        F::StructureBelowThreshold { pre, post } => BracketStageDetail {
            stage: FailureStage::StructureFloor,
            structure_pre: Some(*pre),
            structure_post: Some(*post),
            seam_pre: None,
            seam_post: None,
            residual_margin_db: None,
        },
        F::WaveformBelowThreshold { pre, post, .. } => BracketStageDetail {
            stage: FailureStage::WaveformFloor,
            structure_pre: None,
            structure_post: None,
            seam_pre: Some(*pre),
            seam_post: Some(*post),
            residual_margin_db: None,
        },
        F::ResidualHeadroomExceeded {
            pre,
            post,
            margin_db,
            ..
        } => BracketStageDetail {
            stage: FailureStage::Residual,
            structure_pre: None,
            structure_post: None,
            seam_pre: Some(*pre),
            seam_post: Some(*post),
            residual_margin_db: Some(*margin_db),
        },
    }
}

fn anchor_params_from_gate(
    settings: &crate::application::PatchRequestSettings,
    derived: &crate::application::patch_region::SeamGateDerived,
    baseline: RefinedGapFrames,
) -> AnchorSeamParams {
    let gap_frames = baseline.end_frame.saturating_sub(baseline.start_frame);
    AnchorSeamParams {
        context_frames: derived.context_frames,
        max_anchors_per_side: settings.max_anchors_per_side,
        max_bracket_frames: (settings.max_anchor_bracket_secs * f64::from(derived.sample_rate))
            .round()
            .max(1.0) as usize,
        min_prominence: settings.anchor_seam_min_prominence,
        structure: StructureMatchParams {
            gap_frames,
            bin_frames: derived.bin_frames.max(1),
            search_radius_frames: derived.search_radius_frames,
            fill_length_slack_frames: derived.fill_length_slack_frames,
            max_fine_adjustment_frames: crate::domain::gap_structure::structure_fine_polish_frames(
                derived.bin_frames,
            ),
            silence_peak_fraction: derived.silence_peak_fraction,
            absolute_silence_rms: settings.absolute_silence_rms,
        },
    }
}

/// The gate-overlay measurements for one gap (Fingerprint-unification 8g.3a) — the shared measurement site the
/// from-decode dump ([`characterize_gaps_from_decode`]) projects into a `GapFingerprint` via
/// [`tags_from_measurements`]. `structure`/`seams` are deliberately omitted (from-decode runs under
/// `skip_baseline_placement`; populating `fp.seams` from the throat seam is the deferred F1 fix).
struct RegionMeasurements {
    brackets: Vec<BracketInfo>,
    outcome: GateOutcome,
    lag_decision: Option<LagFingerprint>,
    splice: Option<SpliceSummary>,
    seam_probe: Option<SeamProbeFingerprint>,
    donor_interior: Option<DonorInterior>,
    donor_interior_nominal: Option<DonorInterior>,
    b_levels: Option<LevelProfile>,
    splice_dualfit: Option<SpliceDualfit>,
    wide_envelope: Option<WideEnvelopeFingerprint>,
    residual: Option<ResidualInfo>,
    lag_editorial: Option<LagFingerprint>,
}

/// Per-gap inputs for [`compute_region_measurements`] — the geometry the caller resolved (refined throat, B
/// extract window/offset) plus the shared config/frame params. `gap_floor_db` is `fp.levels.gap_floor_db` from
/// the summary pass (the donor/seam-probe floor).
struct RegionMeasureInput<'a> {
    a_pcm: &'a clip_sync::MultiChannelPcm,
    ch: usize,
    b_slice: &'a [f32],
    b_extract_start_secs: f64,
    gap_offset: f64,
    refined: RefinedGapFrames,
    gap_frames: usize,
    gate_settings: &'a crate::application::PatchRequestSettings,
    gate_derived: crate::application::patch_region::SeamGateDerived,
    cfg: &'a FingerprintConfig,
    gap_floor_db: f32,
    include_diagnostics: bool,
    context_frames: usize,
    bin_frames: usize,
    search_radius_frames: usize,
    rate: f64,
    sample_rate: u32,
    progress: &'a dyn clip_sync::ProgressReporter,
}

/// Compute one gap's gate-overlay measurements from decode — the shared per-gap measurement site the
/// from-decode dump ([`characterize_gaps_from_decode`]) projects via [`tags_from_measurements`] (8g.3a).
fn compute_region_measurements(inp: RegionMeasureInput<'_>) -> RegionMeasurements {
    use crate::application::patch_region::{
        derive_seam_gate_geometry, oracle_build_fit_cache, oracle_measure_residual,
        oracle_score_fit_candidate, oracle_throat_structure_frame, SeamGateParams,
    };
    let RegionMeasureInput {
        a_pcm,
        ch,
        b_slice,
        b_extract_start_secs,
        gap_offset,
        refined,
        gap_frames,
        gate_settings,
        gate_derived,
        cfg,
        gap_floor_db,
        include_diagnostics,
        context_frames,
        bin_frames,
        search_radius_frames,
        rate,
        sample_rate,
        progress,
    } = inp;

    let geom = derive_seam_gate_geometry(
        gate_settings,
        &gate_derived,
        a_pcm,
        b_slice,
        b_extract_start_secs,
        refined.start_frame as f64 / rate + gap_offset,
        refined.end_frame as f64 / rate + gap_offset,
        gap_frames,
        None,
    );
    let params = SeamGateParams {
        settings: gate_settings,
        derived: gate_derived,
        geom,
    };
    let cache = oracle_build_fit_cache(&params);

    // Per-bracket authoritative seam + failure_stage (gate enumeration). The zero-move bracket is the throat;
    // its score becomes the baseline seam (consistent with the brackets and ~the production throat).
    let anchor_params = anchor_params_from_gate(gate_settings, &gate_derived, refined);
    let candidates = list_anchor_candidates_a(&a_pcm.samples, ch, refined, &anchor_params);
    let brackets = list_feasible_anchor_brackets(&candidates, refined, &anchor_params);
    let mut any_ok = false;
    let mut best_energy: Option<(f64, RefinedGapFrames)> = None;
    // Populated when the zero-move (throat) bracket scores `Ok` — the structure placement is already computed
    // inside that gate call, so the residual read below reuses it instead of a second `gate_structure_align`.
    let mut throat_structure_frame: Option<usize> = None;
    let mut infos = Vec::with_capacity(brackets.len());
    let bracket_total = brackets.len() as u64;
    for (bn, br) in brackets.iter().enumerate() {
        progress.progress("fingerprint-scoring", bn as u64 + 1, bracket_total);
        // `placement` is the gate's own chosen alignment at **production weights** — the only
        // placement in the dump that can observe an end-search scoring change. (The `place_on_b`
        // placement recorded by `characterize_gap_region` runs at `waveform_weight: 0.0` and is
        // blind to those terms by construction.) It costs nothing here: the gate already computed
        // it. `None` on a gate failure — there is no chosen placement to report.
        let (
            structure_pre,
            structure_post,
            seam_pre,
            seam_post,
            stage,
            residual_margin_db,
            placement,
        ) = match oracle_score_fit_candidate(&params, &cache, br.refined, refined, true) {
            Ok(sc) => {
                any_ok = true;
                if br.refined == refined {
                    throat_structure_frame = Some(sc.structure_start_frame);
                }
                (
                    None,
                    None,
                    Some(sc.report_pre),
                    Some(sc.report_post),
                    None,
                    None,
                    Some(sc.alignment),
                )
            }
            Err(f) => {
                let d = stage_of(&f);
                (
                    d.structure_pre,
                    d.structure_post,
                    d.seam_pre,
                    d.seam_post,
                    Some(d.stage),
                    d.residual_margin_db,
                    None,
                )
            }
        };
        infos.push(BracketInfo {
            pre_time_secs: br.pre.frame as f64 / rate,
            post_time_secs: br.post.frame as f64 / rate,
            span_secs: br.post.frame.saturating_sub(br.pre.frame) as f64 / rate,
            move_frames: br.move_frames,
            structure_pre,
            structure_post,
            seam_pre,
            seam_post,
            start_frame: placement.map(|a| a.start_frame),
            fill_frames: placement.map(|a| a.fill_frames),
            failure_stage: stage,
            residual_margin_db,
        });
        if include_diagnostics
            && br.pre.source == AnchorSource::EnergyPeak
            && br.post.source == AnchorSource::EnergyPeak
        {
            let smin = match (seam_pre, seam_post) {
                (Some(a), Some(b)) => a.min(b),
                _ => f64::NEG_INFINITY,
            };
            if best_energy.is_none_or(|(bs, _)| smin > bs) {
                best_energy = Some((smin, br.refined));
            }
        }
    }
    let patched = any_ok;
    let mut outcome = GateOutcome {
        plan_kind: "fillable".into(),
        tier: if patched {
            "patch".into()
        } else {
            "skip".into()
        },
        seam_shape: None,
        fit_path: None,
        signature_mode: None,
        // Fingerprint-native: closest failing bracket's FailureStage (same as analyzer
        // `closest_failure_stage`). Projection may refine from the final bracket list.
        skip_reason: (!patched)
            .then(|| closest_bracket_failure_stage(&infos).map(|s| s.as_str().to_string()))
            .flatten(),
        // Set below, once `splice_dualfit_at` has run — `gate_pass` is not available yet.
        dual_fit_rescue: None,
    };

    // Lag fingerprints — `b_mono`/`b_ch` shared by both placements.
    let b_mono = interleaved_to_mono(b_slice, ch);
    let b_ch = interleaved_to_channels(b_slice, ch);
    let b_mapped_start =
        b_mapped_frame_in_haystack(refined.start_frame, rate, gap_offset, b_extract_start_secs);
    let b_mapped_bracket = |refined_b: RefinedGapFrames| {
        b_mapped_frame_in_haystack(
            refined_b.start_frame,
            rate,
            gap_offset,
            b_extract_start_secs,
        )
    };

    // Registration metrics at `b_mapped` nominal (ledger A2 / §3.7) — stable gross map + ±600 ms lag sweep.
    let lag_decision = Some(lag_at_placement(&LagAtPlacementInput {
        a_samples: &a_pcm.samples,
        channels: ch,
        refined,
        b_mono: &b_mono,
        b_ch: &b_ch,
        selected: None,
        start_frame: b_mapped_start,
        cfg,
        sample_rate,
    }));
    let pre_shift_frames = lag_decision
        .as_ref()
        .and_then(|l| mono_lag_side(l, true))
        .map(|s| s.frac_lag_samples.round() as i64)
        .unwrap_or(0);
    let post_gross_frames = lag_decision
        .as_ref()
        .and_then(|l| mono_lag_side(l, false))
        .map(|s| s.frac_lag_samples.round() as i64);
    let seam_probe = if include_diagnostics {
        Some(seam_probe_at_placement(&SeamProbeAtPlacementInput {
            a_samples: &a_pcm.samples,
            channels: ch,
            refined,
            b_mono: &b_mono,
            start_frame: b_mapped_start,
            post_shift_frames: pre_shift_frames,
            bin_frames,
            cfg,
            sample_rate,
            gap_floor_db: f64::from(gap_floor_db),
        }))
    } else {
        None
    };
    let b_pre_aligned = (b_mapped_start as i64 + pre_shift_frames).max(0) as usize;
    let b_post_aligned = post_gross_frames
        .map(|g| (b_mapped_start as i64 + gap_frames as i64 + g).max(0) as usize)
        .unwrap_or(b_mapped_start + gap_frames);
    let donor_interior = donor_interior_at(
        &b_mono,
        b_pre_aligned,
        b_post_aligned,
        f64::from(gap_floor_db),
        sample_rate,
    );
    let b_gap_end = b_mapped_start + gap_frames;
    let donor_interior_nominal = donor_interior_at(
        &b_mono,
        b_mapped_start,
        b_gap_end,
        f64::from(gap_floor_db),
        sample_rate,
    );
    let b_levels = if include_diagnostics {
        Some(
            level_profile(
                |f, end| mono_slice_rms(&b_mono, f, end),
                LevelProfileSpan {
                    gap_start: b_mapped_start,
                    gap_end: b_gap_end,
                    context_start: b_mapped_start.saturating_sub(context_frames),
                    context_end: (b_gap_end + context_frames).min(b_mono.len()),
                },
                bin_frames,
                cfg.gap_signature_bin_ms as u32,
            )
            .0,
        )
    } else {
        None
    };
    let splice_dualfit = splice_dualfit_at(&SpliceDualfitInput {
        a_samples: &a_pcm.samples,
        channels: ch,
        refined,
        b_mono: &b_mono,
        b_mapped_start,
        cfg,
        sample_rate,
    });
    // F14: now that `splice_dualfit` exists, record whether production's dual-fit would rescue this
    // skip. Deliberately the same `dual_fit_rescue_flag` the projection calls — the two paths are
    // compared axis-by-axis by `decode_path_projection`, so a second inline copy would be a drift bug
    // waiting to happen.
    outcome.dual_fit_rescue = dual_fit_rescue_flag(&DualFitRescueInput {
        patched,
        brackets: &infos,
        splice_dualfit: splice_dualfit.as_ref(),
        donor_aligned: donor_interior.as_ref(),
        donor_nominal: donor_interior_nominal.as_ref(),
    });
    let wide_envelope = if include_diagnostics {
        Some(wide_envelope_at_placement(&WideEnvelopeAtPlacementInput {
            a_samples: &a_pcm.samples,
            channels: ch,
            refined,
            b_mono: &b_mono,
            start_frame: b_mapped_start,
            post_shift_frames: pre_shift_frames,
            cfg,
            sample_rate,
        }))
    } else {
        None
    };
    let splice = lag_decision.as_ref().and_then(splice_summary_from_lag);

    // Residual stays at the gate's structure throat. Reuse the throat placement from the bracket loop when the
    // throat bracket scored `Ok`; else a fresh `gate_structure_align` call.
    let throat_frame =
        throat_structure_frame.or_else(|| oracle_throat_structure_frame(&params, &cache, refined));
    let residual = throat_frame.and_then(|throat_frame| {
        oracle_measure_residual(&params, &cache, refined, throat_frame).map(|v| ResidualInfo {
            chosen_pre_db: residual_db_opt(v.chosen_pre_db),
            chosen_post_db: residual_db_opt(v.chosen_post_db),
            floor_pre_db: residual_db_opt(v.floor_pre_db),
            floor_post_db: residual_db_opt(v.floor_post_db),
            // See the `project.rs` twin: the source is what disambiguates an absent `floor_*_db`.
            floor_source_pre: Some(v.floor_source_pre),
            floor_source_post: Some(v.floor_source_post),
            informative: v.informative,
            // See the `project.rs` twin: recorded as measured, and the reach is carried so a replayed
            // verdict abstains where production did.
            uninformative_pre: v.uninformative_pre,
            uninformative_post: v.uninformative_post,
            placement_slide_frames: Some(v.placement_slide_frames),
            max_lag_frames: Some(v.max_lag_frames),
        })
    });

    // Diagnostic lag (Tier-3): one placement search at the best (highest-seam) speech bracket.
    let lag_editorial = if include_diagnostics {
        best_energy.and_then(|(_, refined_b)| {
            place_on_b(&PlaceOnBInput {
                a_samples: &a_pcm.samples,
                channels: ch,
                refined: refined_b,
                b_haystack: b_slice,
                b_mono: &b_mono,
                b_ch: &b_ch,
                nominal_fill_start: b_mapped_bracket(refined_b),
                context_frames,
                bin_frames,
                search_radius_frames,
                cfg,
            })
            .map(|p| {
                lag_at_placement(&LagAtPlacementInput {
                    a_samples: &a_pcm.samples,
                    channels: ch,
                    refined: refined_b,
                    b_mono: &b_mono,
                    b_ch: &b_ch,
                    selected: p.selected_channels.first().copied(),
                    start_frame: p.start_frame,
                    cfg,
                    sample_rate,
                })
            })
        })
    } else {
        None
    };

    RegionMeasurements {
        brackets: infos,
        outcome,
        lag_decision,
        splice,
        seam_probe,
        donor_interior,
        donor_interior_nominal,
        b_levels,
        splice_dualfit,
        wide_envelope,
        residual,
        lag_editorial,
    }
}

/// Corpus envelope when A/B `native_channels` disagree: provenance filled, pairwise gaps refused.
///
/// B's `channels` / `duration_secs` / `id` use **B's** layout (not A's). Rate may still be A's after
/// `decode_ab` resample. Callers must not index `b_samples` with `a_channels`.
///
/// This is the **opposite** rule from the non-refused path, where both sides are described at A's
/// layout because that is the layout everything was measured at. The split is deliberate: there is no
/// common measurement here to describe B against, so B is described honestly against itself. Note the
/// consequence — `b_source.id` digests B's own channel count, so a refused corpus and a normal one over
/// the same media carry different `b_source.id`s. Harmless today (`gaps` is empty, so `entry_stem`
/// never runs), but a `(a_id, b_id)` join across the two would not match. Both halves of the rule are
/// asserted: `characterize_gaps_refuses_channel_layout_mismatch` and the from-decode threading test.
fn refused_channel_mismatch_corpus(
    report: &crate::domain::GapReport,
    a_samples: &[f32],
    b_samples: &[f32],
    sample_rate: u32,
    a_channels: u16,
    sources: &crate::application::patch_audio::AbSources,
) -> GapCorpus {
    GapCorpus {
        source: SourceMeta {
            a_source: file_source(a_samples, sample_rate, a_channels, Some(&sources.a)),
            b_source: file_source(
                b_samples,
                sample_rate,
                sources.b.native_channels,
                Some(&sources.b),
            ),
            scan_recipe: CorpusScanRecipe::from_report(report),
            gap_count: report.gaps.len(),
            incomparable: Some(IncomparableReason::ChannelLayoutMismatch),
            gate_recipe: None,
            // Refusal ⇒ `gaps` is empty, so there is nothing to qualify.
            not_measured: Vec::new(),
        },
        gaps: vec![],
    }
}

fn channel_layout_mismatch(sources: &crate::application::patch_audio::AbSources) -> bool {
    sources.a.native_channels != sources.b.native_channels
}

/// Full-timeline A/B PCM for corpus characterization, plus optional source provenance.
///
/// Groups the media trio that already travels together from `decode_ab` (`a_pcm`, resampled B,
/// `sources`) so [`characterize_gaps`] / [`characterize_gaps_from_decode`] take one argument instead of
/// three. `sources` is `None` for media-free callers (synthetic fixtures), which leaves provenance fields
/// absent rather than guessed.
#[derive(Clone, Copy)]
pub struct CharacterizeAbPcm<'a> {
    pub a_pcm: &'a clip_sync::MultiChannelPcm,
    /// B interleaved samples at A's sample rate (same layout contract as decode's `b_samples_full`).
    pub b_samples: &'a [f32],
    pub sources: Option<&'a crate::application::patch_audio::AbSources>,
}

/// Fingerprint dump computed **from decode via the shared projection** (Fingerprint-unification 8g.4a/8g.4b) —
/// the `--gap-fingerprints` dump path (the old per-bracket-oracle inline build was removed at 8g.6). Per
/// gap: the summary (geometry/levels, already in `corpus`) + [`compute_region_measurements`] (8g.3a) → `m`,
/// then a spec (verdict from `m.outcome.tier` = the fingerprint **`any_ok`** semantics; tags from
/// [`tags_from_measurements`], 8g.3b) → [`spec_to_fingerprint_summary`]. Keeps fingerprint semantics — does NOT
/// run the production patch gate (pre-flip review Finding 1). Gaps whose overlay setup is skipped (no B start /
/// zero-length / empty window) keep their summary fingerprint, exactly as the oracle leaves them.
///
/// When `pcm.sources` reports disagreeing `native_channels`, returns an empty-gap corpus with
/// [`IncomparableReason::ChannelLayoutMismatch`] and does not index B at A's channel count.
///
/// **SHADOW at 8g.4a** — validated by the old-vs-new decode differential (`decode_path_projection`), lean +
/// diagnostics; the dump flips to it at 8g.4b. Lossy-by-projection on `silence`/`contour`/`anchors` (X, not
/// read by `golden_baseline`) — a fidelity item for the diagnostics path (8g.5), not a decision change.
pub fn characterize_gaps_from_decode(
    report: &crate::domain::GapReport,
    pcm: &CharacterizeAbPcm<'_>,
    request: &crate::application::PatchAudioRequest,
    select: &[usize],
    include_diagnostics: bool,
    progress: &dyn clip_sync::ProgressReporter,
) -> GapCorpus {
    use crate::domain::gap_repair_spec::{
        BExtractWindow, GapRepairSpec, GapRepairStrategy, GapRepairVerdict, LevelTags,
    };

    let CharacterizeAbPcm {
        a_pcm,
        b_samples: b_samples_full,
        sources,
    } = *pcm;
    let sample_rate = a_pcm.sample_rate;
    let channels = a_pcm.channels as usize;
    if let Some(src) = sources {
        if channel_layout_mismatch(src) {
            return refused_channel_mismatch_corpus(
                report,
                &a_pcm.samples,
                b_samples_full,
                sample_rate,
                a_pcm.channels,
                src,
            );
        }
    }
    let cfg = FingerprintConfig::from_request(request, report.recipe.silence_peak_fraction());
    let mut corpus = characterize_gaps(report, pcm, &cfg, select);
    // Seam-gate floors used for every bracket `failure_stage` on this dump — stamped once at corpus
    // level (same idea as equivalence `thresholds`). Summary-only `characterize_gaps` leaves this None.
    corpus.source.gate_recipe = Some(CorpusGateRecipe::from_settings(&request.settings));
    // Gaps whose `lag_decision` block came from `projected_lag_entry` rather than a real sweep —
    // the population `PROJECTED_LAG_DECISION_FIELDS` exists to disown. Zero on a dump where every
    // measured gap carried its sweep through, which is the normal from-decode case.
    let mut projected_lag_gaps = 0usize;

    let mut gate_derived = crate::application::patch_region::SeamGateDerived::from_repair(
        request,
        sample_rate,
        channels,
        report.recipe.silence_peak_fraction(),
    );
    gate_derived.measure_residual = true;
    let rate = f64::from(sample_rate).max(1.0);
    let ch = channels.max(1);
    let b_total = b_samples_full.len() / ch;
    let max_refine_frames = (cfg.max_refine_secs * rate).round() as usize;
    let context_frames = (cfg.gap_signature_context_secs * rate).round() as usize;
    let bin_frames = ((cfg.gap_signature_bin_ms as f64 / 1000.0) * rate)
        .round()
        .max(1.0) as usize;
    let search_radius_frames =
        ((cfg.fill_border_search_secs.max(cfg.fill_align_margin_secs)) * rate).round() as usize;
    let pad_lead =
        cfg.gap_signature_context_secs + cfg.fill_border_search_secs + cfg.fill_align_margin_secs;
    let pad_tail = cfg.gap_signature_context_secs
        + cfg
            .fill_extract_tail_slack_secs
            .max(cfg.fill_align_margin_secs)
        + cfg.fill_border_search_secs
        + cfg.fill_align_margin_secs;

    let total_gaps = corpus.gaps.len() as u64;
    for (gn, fp) in corpus.gaps.iter_mut().enumerate() {
        progress.progress("fingerprint-gap", gn as u64 + 1, total_gaps);
        let i = fp.index;
        let gap = &report.gaps[i];
        // Guarded span: half-mapped / degenerate / negative B coords are not fingerprintable.
        let Some((b_start, b_end)) = gap.mapped_b_span() else {
            continue;
        };
        let refined = refine_gap_frames(
            &a_pcm.samples,
            ch,
            (gap.video_a_start_secs * rate) as usize,
            (gap.video_a_end_secs * rate) as usize,
            cfg.silence_peak_fraction,
            cfg.absolute_silence_rms,
            max_refine_frames,
        );
        let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
        if gap_frames == 0 {
            continue;
        }
        let gap_offset = b_start - gap.video_a_start_secs;
        let lo = (((b_start - pad_lead).max(0.0) * rate) as usize).min(b_total);
        let hi = (((b_end + pad_tail) * rate).ceil() as usize).min(b_total);
        if hi <= lo {
            continue;
        }
        let b_slice = &b_samples_full[lo * ch..hi * ch];
        let b_extract_start_secs = lo as f64 / rate;

        let m = compute_region_measurements(RegionMeasureInput {
            a_pcm,
            ch,
            b_slice,
            b_extract_start_secs,
            gap_offset,
            refined,
            gap_frames,
            gate_settings: &request.settings,
            gate_derived,
            cfg: &cfg,
            gap_floor_db: fp.levels.gap_floor_db,
            include_diagnostics,
            context_frames,
            bin_frames,
            search_radius_frames,
            rate,
            sample_rate,
            progress,
        });

        // Decision spec: geometry + levels from the summary fp; verdict from `m.outcome` (any_ok); tags from
        // the shared measurements. Placeholder strategy/reason carries only the `patch`/`skip` + cell
        // distinction the reader's `tier` axis needs — dump `outcome.skip_reason` is the closest
        // `FailureStage` from `m.brackets`, not this production enum.
        let levels = LevelTags {
            a_gap_floor_db: f64::from(fp.levels.gap_floor_db),
            a_noise_floor_db: f64::from(fp.levels.noise_floor_db),
        };
        let verdict = if m.outcome.tier == "skip" {
            // Inert reason for `cell` only (`Decorrelated` default); wire skip_reason comes from brackets.
            GapRepairVerdict::skip(GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: 0.0,
                post_correlation: 0.0,
                min_correlation: 0.0,
                best_attempt: None,
            })
        } else {
            GapRepairVerdict::Patch(GapRepairStrategy::SilenceSplice {
                fill: Vec::new(),
                pre_seam_r: 0.0,
                post_seam_r: 0.0,
                pre_lag: 0,
                post_lag: 0,
                trim_frames: 0,
                residual: None,
                confidence: crate::domain::gap_fill_fit::FillConfidence::High,
            })
        };
        let spec = GapRepairSpec {
            gap_index: fp.index,
            a_start_secs: fp.geometry.a_start_secs,
            a_end_secs: fp.geometry.a_end_secs,
            gap_offset_secs: fp.geometry.fill_offset_secs.unwrap_or(gap_offset),
            refined: RefinedGapFrames {
                start_frame: (fp.geometry.a_refined_start_secs * rate).round() as usize,
                end_frame: (fp.geometry.a_refined_end_secs * rate).round() as usize,
            },
            b_extract: BExtractWindow {
                start_frame: 0,
                end_frame: 0,
                b_mapped_start_frame: 0,
            },
            crossfade_secs: 0.0,
            verdict,
            tags_ctx: tags_from_measurements(&m, Some(levels)),
        };
        let x = FingerprintXSet {
            seam_probe: m.seam_probe,
            wide_envelope: m.wide_envelope,
            b_levels: m.b_levels,
            lag_editorial: m.lag_editorial,
        };
        // Carry the REAL per-bracket rows (8g.4b) so the flipped dump is byte-faithful to the oracle's
        // `brackets` in both modes — the oracle enumerates them unconditionally, so from-decode must too.
        //
        // And the REAL `lag_decision`: `lag_at_placement` already swept ±`lag_max_lag_ms` at `b_mapped`
        // above, and the projection used to throw that away and rebuild a row from four stored scalars —
        // zeroed search parameters, `lag0_r` a copy of `peak_r`, `verdict` hardcoded `TimingOffset`. The
        // copy is the damaging one: it reads as "this shoulder peaks exactly at zero lag" on every gap,
        // which is the opposite of what a registration study wants to know. The measurement exists; it
        // just was not plumbed. The oracle path (spec only, no PCM) still cannot recover it and still
        // projects — which is why the declaration below is conditional rather than a constant.
        let measured = MeasuredDetail {
            brackets: Some(m.brackets),
            lag_decision: m.lag_decision,
        };
        let lag_supplied = measured.lag_decision.is_some();
        *fp = spec_to_fingerprint_summary(&spec, sample_rate, channels as u16, Some(x), measured);
        // Count only gaps that ended up with a *fabricated* block. A gap with no sweep and no scalars
        // has no `lag_decision` at all, and absence needs no disowning.
        if !lag_supplied && fp.lag_decision.is_some() {
            projected_lag_gaps += 1;
        }

        // Gap-equivalence classification overlay (gap-equivalence plan §7.4) — emitted for tuning/categorizing.
        // Silence-character signals: A gap RMS vs the recording's noise floor + donor silence at nominal.
        // `enabled: true` here so the dump always classifies (it never drops gaps — that's the v1 plan-time gate).
        // F15 fixes 1–3. The diagnostic equivalence read now owns its sensors instead of borrowing
        // `fp.levels.*`: those are amplitude-mean downmixes over the refined span with no silence
        // predicate, and all three properties bias this path toward `drop`. They cannot simply be fixed
        // in `level_profile` — `levels.gap_floor_db` / `levels.noise_floor_db` have other consumers
        // (`snr_db`, dual-fit's `a_gap_floor_db`) that must not move. See
        // `docs/dev/archive/TEMP-equivalence-divergence-findings.md` § *The three F15 fixes*.
        //
        // The span is the **silent core** — `equivalence_production.a_span_secs`, taken straight off the
        // index-parallel scan verdict — not `refined` and not the raw gap. Scan's block grid is
        // media-absolute and selects blocks by centre-containment, which is what fix 3 adopted and which
        // held (the grid reproduced scan's lattice on 802/802 gaps of the 39-pair corpus). The *interval*
        // did not: fix 3 read `derive_gap_equivalence`'s then-parameter names `a_start_secs`/`a_end_secs`
        // and bound `geometry.a_*`, the raw hold-bridged run, while every caller of that function passes
        // `SilentRun::core_*`. The core is narrower by 1–2 blocks on 66.9 % of gaps, and the extra blocks
        // are fade shoulders — non-silent, so they drag `donor_silence_fraction` under 0.5 and flip
        // `shared_silence` to `repairable_dropout`. That is the *dangerous* direction (10 such gaps, where
        // the 17-pair predecessor had 0), and it is why the fallback below is the raw span rather than a
        // refusal: raw is what this path measured for months, so absent scan provenance costs no accuracy
        // it previously had — but `a_span` then says `nominal`, because it is.
        //
        // Taking the interval from scan rather than recomputing it is deliberate: a second derivation
        // would have to re-run silent-run detection at this path's bin width and could disagree again.
        // Reading the number scan used makes convergence structural. See `docs/gap-scan.md` and
        // `crate::domain::gap_equivalence::GapEquivalenceVerdict::a_span_secs`.
        //
        // The noise floor is read under the same interleaved reduction as A, since a level-dependent
        // reduction error does not cancel between the two sides of `a < nf − margin`. No downmix
        // fallback when the context is empty — that would un-apply fix 2 on exactly the gaps too thin to
        // measure; `None` classifies `NotEvaluated` ⇒ keep.
        //
        // I1 (2026-07-30): the whole equivalence overlay now bins at **`scan_block_ms`**, not
        // `gap_signature_bin_ms`. That parameter is documented as the bin width for *active/silent
        // structure signatures* and every other consumer uses it for exactly that — a binary seam pattern
        // match, where 50 ms is well chosen because finer bins discriminate syllable-scale structure. This
        // overlay is a different job (level + threshold estimation for comparison against scan) and had
        // inherited the value by proximity, never by choice. The property inverts between the two jobs:
        // for `gap_floor_db` (a **max**) and `donor_silence_fraction` (a **threshold-crossing fraction**)
        // finer bins are upward-biased, measured at max ≥ coarser on 10/10 gaps and the donor fraction
        // biased up on 5/6. `gap_signature_bin_ms` itself is untouched — it has production consumers in
        // `patch_audio::geometry` / `::region`. The context window (I2) was split for that same reason
        // and is **closed 2026-08-01 by removal** — see the `noise_floor_probe` call below.
        // See `docs/dev/archive/TEMP-equivalence-instrument-convergence.md` § I1.
        let equiv_bin_ms = report.recipe.scan_block_ms();
        //
        // The donor goes in as **PCM at the nominal `b_mapped` span**, not as
        // `donor_interior_nominal.silence_fraction`. That fraction is a mono-downmix read thresholded
        // against `levels.gap_floor_db` — the unfiltered whole-span peak — so passing it here would leave
        // fix 1 half-applied: A's floor would move while the predicate that actually reaches the class
        // still tested against the old one. On the band-donor mechanism that predicate *is* the flip.
        //
        // A/donor move together or not at all. Converging A onto the core while leaving the donor on
        // `geometry.b_mapped_*` would fix one end of a comparison whose whole content is the two ends
        // agreeing, so a scan verdict that carries a core but no donor (B unscanned) falls back on both.
        let scan_verdict = report.gap_equivalence.get(fp.index);
        let scan_core = scan_verdict
            .and_then(|v| v.a_span_secs)
            .filter(|(s, e)| e > s);
        let (a_span_secs, b_span_secs, span_kind) = match scan_core {
            Some(core) => (
                core,
                scan_verdict.and_then(|v| v.donor_span_secs),
                SpanKind::Core,
            ),
            None => (
                (fp.geometry.a_start_secs, fp.geometry.a_end_secs),
                fp.geometry
                    .b_mapped_start_secs
                    .zip(fp.geometry.b_mapped_end_secs),
                SpanKind::Nominal,
            ),
        };
        let gap_frames = (a_span_secs.0 * rate).round().max(0.0) as usize
            ..(a_span_secs.1 * rate).round().max(0.0) as usize;
        let equiv_nf = noise_floor_probe(
            &a_pcm.samples,
            ch,
            gap_frames.clone(),
            sample_rate,
            // I2 closed 2026-08-01 by **removal, not convergence**: this overlay now estimates the
            // noise floor over scan's window. Note this is the *argument*, not the field —
            // `gap_signature_context_secs` keeps its 3.0 s for `build_gap_fingerprint`'s context
            // frames, the B-extract padding below, and its production consumers in
            // `patch_audio::geometry` / `::region`. Only the equivalence floor moves.
            //
            // Why removal rather than a converged value: 3.0 s was never chosen for *this* job. It is
            // the sibling field of `gap_signature_bin_ms`, which I1 found had been "inherited by
            // proximity, never by choice" for the same reason — so the split was one considered value
            // (scan's, named and documented) against one accident, not two judgements in tension. The
            // header here claimed "both values encode a real judgement"; that was true of scan's only.
            //
            // The 3.0 s reading is **not lost** — `noise_floor_probe_grid` below still carries its row,
            // which is where a context-sensitivity question belongs: a labelled axis in the provenance,
            // not an unlabelled difference inside the verdict being compared.
            crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS,
            equiv_bin_ms,
            ChannelReduction::Interleaved,
        );
        // Refuse a donor window B does not contain, rather than measuring whatever part of it exists.
        //
        // `measure_gap_equivalence` clamps its frame range to the samples available, so a window running
        // past B's end silently becomes a *shorter* window — and the truncated remainder of a tail gap is
        // digital silence, which scored 99.3–100 % silent on all 20 such gaps of the 39-pair corpus and
        // produced `shared_silence`/drop from audio that does not exist. Scan already fails closed here
        // (`b_range_fully_scanned`), which is why those gaps showed up as scan-vs-diagnostic divergences:
        // the disagreement was never about silence, only about whether to answer.
        //
        // `None` ⇒ no donor fraction ⇒ `NotEvaluated` ⇒ keep, the same stated refusal scan makes. The
        // `--check` warning stays: it reports the geometry, which is still worth seeing, and now nothing
        // downstream acts on it.
        let donor_span = b_span_secs
            .filter(|&(s, e)| {
                let lo = (s * rate).round().max(0.0) as usize;
                let hi = (e * rate).round().max(0.0) as usize;
                hi <= b_total && lo < hi
            })
            .map(|(s, e)| crate::application::gap_equivalence::DonorSpan {
                samples: b_samples_full,
                frames: (s * rate).round().max(0.0) as usize..(e * rate).round().max(0.0) as usize,
            });
        // Captured before `donor_span` is moved into the call below — the provenance token has to
        // report what was measured, and after the move there is nothing left to ask.
        let donor_measured = donor_span.is_some();
        let equiv = crate::application::gap_equivalence::measure_gap_equivalence(
            &a_pcm.samples,
            ch,
            gap_frames.clone(),
            equiv_nf.floor_db,
            donor_span,
            &crate::application::gap_equivalence::SilentCoreConfig {
                bin_frames: (((equiv_bin_ms as f64) / 1000.0) * rate).round().max(1.0) as usize,
                silence_peak_fraction: cfg.silence_peak_fraction,
                absolute_silence_rms: cfg.absolute_silence_rms,
            },
            &crate::domain::gap_equivalence::GapEquivalenceParams {
                enabled: true,
                ..Default::default()
            },
        );
        // Measurement recipe — attached here, not inside `measure_gap_equivalence`: that function never
        // sees `context_secs` / `bin_ms` (noise floor arrives precomputed; `SilentCoreConfig` has frames
        // only). See `docs/dev/archive/TEMP-fingerprint-provenance-plan.md` §3a.
        //
        // Both tokens are `span_kind`, reported rather than asserted. They read `core`/`nominal` as
        // literals until 2026-08-01 — `core` was simply wrong (this path measured the raw span), and a
        // provenance field that states the thing it exists to let you check is worse than no field: the
        // calibration diff printed `core` on both sides of every one of the 10 dangerous divergences the
        // span mismatch caused. A hardcoded token cannot report a fallback, so it does not get to be one.
        let measurement = EquivalenceMeasurement {
            // Must track the `noise_floor_probe` argument above, not `cfg` — a token reporting the
            // config field after the call site stopped reading it would be the third instance this
            // month of provenance describing an intent instead of a measurement (`a_span: core` on a
            // raw-span read; `donor_span: core` with no donor). Both sides now read 2.0.
            context_secs: crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS,
            bin_ms: equiv_bin_ms,
            reduction: ChannelReduction::Interleaved,
            a_span: span_kind,
            // Taken from the donor that was actually built, not from `b_span_secs`: the EOF filter
            // above can reject a mapped window, and the token has to report the measurement, not the
            // intent. `None` when nothing was measured.
            donor_span: donor_measured.then_some(span_kind),
        };

        // Candidate noise floors (F15, second axis) over the {context window} × {bin size} × {channel
        // reduction} grid the two front-ends straddle — also provenance, also classified on by nothing.
        // Built by calling `level_profile` itself rather than re-deriving the bin walk, so a probe
        // cannot drift from the measurement it characterizes. **Retained** for I2 attribution.
        //
        // The `(EQUIVALENCE_CONTEXT_SECS, scan_block_ms, Interleaved)` row is the anchor: it matches
        // scan's recipe on all three variables and so should reproduce `equivalence_production.noise_floor_db`.
        // Its `Downmix` twin was the anchor before the reduction dimension existed, and undershot by
        // 3.13–7.96 dB on every gap of the first run — the two rows now sit side by side, and their
        // difference *is* the reduction term.
        let nf_probes = noise_floor_probe_grid(
            &a_pcm.samples,
            ch,
            // **`gap_frames`, the span the live measurement used** — not `refined`, which is this
            // path's own edge-refined interval and a *third* window distinct from both the scan core
            // and the nominal span. The grid varies three axes so the rows differ by those axes and
            // nothing else; holding a different interval than the measurement it characterizes makes
            // every row carry an unlabelled fourth term. Measured cost of the mismatch on the
            // 2026-08-01 4-pair run: the anchor row (which matches scan's recipe on all three axes
            // and should therefore reproduce `equivalence_production.noise_floor_db`) missed it on 33/33
            // gaps, median 0.24 dB and max 7.49 dB, and the `gap_signature_context_secs` row missed
            // this path's own live floor by up to 1.63 dB. Both are the interval, not the axes.
            //
            // Was `refined` until 2026-08-01 and correct while it lasted: the live measurement used
            // the raw span too, and the A-span convergence moved the measurement onto the scan core
            // without bringing the probes along. I2 cannot be attributed from a grid that measures a
            // window nothing else does.
            gap_frames.clone(),
            sample_rate,
            &[
                crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS,
                cfg.gap_signature_context_secs,
            ],
            &[report.recipe.scan_block_ms(), cfg.gap_signature_bin_ms],
            &[ChannelReduction::Interleaved, ChannelReduction::Downmix],
        );

        // No `with_gap_floor_db` here any more: `equivalence.gap_floor_db` is now the **silent-core**
        // floor this path measured (fix 1), carried by `with_scan_provenance` alongside the silent-bin
        // count behind it. Re-attaching `levels.gap_floor_db` would overwrite the fix with the whole-span
        // content peak it exists to replace. `levels.gap_floor_db` is still dumped in its own block for
        // anyone who wants the old number.
        fp.equivalence_diagnostic = Some(Contracted::new(
            equiv
                .with_measurement(measurement)
                .with_noise_floor_probes(nf_probes),
            EQUIVALENCE_DIAGNOSTIC_CONTRACT,
        ));
    }

    // Copy in the coarse scan-block verdict (block size = the `scan_block_ms` recipe knob, not a
    // constant — see `default_scan_block_ms`; this comment said "250 ms" until 2026-07-30, long
    // after the default moved to 100), index-parallel to report gaps, so the corpus holds both
    // verdicts per gap and the calibration diff reads them from `corpus.json` alone. They are two
    // differently-*defined* measurements, not two granularities of one — see the table in
    // `bin/equivalence_calibration.rs` before treating either as the reference.
    //
    // **A separate pass, deliberately.** This ran inside the loop above until 2026-08-01, below the
    // `mapped_b_span` / `gap_frames` / `hi <= lo` guards — so a gap that tripped any of them lost
    // scan's verdict too, even though `derive_gap_equivalence` had already produced one and
    // `report.gap_equivalence` is pushed unconditionally (`scan_gaps.rs`, index-parallel). On the
    // 39-pair v0.5.0 corpus that silently discarded 27 real `NotEvaluated` verdicts — head gaps whose
    // negative offset maps B before zero. `NotEvaluated` is a *stated* refusal ("no donor, so no
    // judgement"); dropping it degrades that to an absence, which is precisely the unreadable-null
    // defect the provenance work exists to prevent. The diagnostic side genuinely cannot measure
    // those gaps and stays `None`; scan's answer is real and is now always carried.
    //
    // It cannot simply move to the top of the loop: `*fp = spec_to_fingerprint_summary(..)` rebuilds
    // the fingerprint wholesale mid-body and would clobber an early assignment.
    for fp in corpus.gaps.iter_mut() {
        fp.equivalence_production = report
            .gap_equivalence
            .get(fp.index)
            .cloned()
            .map(|v| Contracted::new(v, EQUIVALENCE_PRODUCTION_CONTRACT));
    }

    // Declare fabricated stand-ins this path still writes — stamped **here**, not in
    // `characterize_gaps`, because it is this function that rebuilds via
    // `spec_to_fingerprint_summary`. Envelope / silence / contour / anchors / seam_shape are now
    // omitted (`None`) rather than zeroed, so [`NOT_MEASURED_BY_PROJECTION`] is empty; only
    // `lag_decision.*` may still need a declaration when the sweep was projected rather than measured.
    //
    // Scoped to `DetailTier::Full` — gaps that never reached the rebuild keep `characterize_gaps`'s
    // real values and stay `Summary`. Only claim anything if the rebuild actually ran.
    //
    // `lag_decision.*` is appended only when some gap actually got the fabricated row. This path
    // normally threads the real sweep through `MeasuredDetail`, so the usual answer is "not declared,
    // because measured" — the declaration tracks what happened, not what the code is capable of.
    if corpus.gaps.iter().any(|g| g.tier == DetailTier::Full) {
        corpus.source.not_measured = NOT_MEASURED_BY_PROJECTION
            .iter()
            .chain(if projected_lag_gaps > 0 {
                PROJECTED_LAG_DECISION_FIELDS
            } else {
                &[]
            })
            .map(|s| (*s).to_string())
            .collect();
    }
    corpus
}

/// Build A-side **summary** fingerprints for the gaps in `select` (empty ⇒ all) against decoded
/// full A/B PCM: geometry + levels + contour + anchors + a baseline structure/seam per gap. The
/// authoritative gate detail (brackets / `failure_stage` / lag / outcome) is layered on by
/// [`characterize_gaps_from_decode`]. A gap with no B mapping is characterized A-only.
///
/// When `pcm.sources` reports disagreeing `native_channels`, returns
/// [`IncomparableReason::ChannelLayoutMismatch`] with no gaps — B must not be sliced at A's channel
/// count.
pub fn characterize_gaps(
    report: &crate::domain::GapReport,
    pcm: &CharacterizeAbPcm<'_>,
    cfg: &FingerprintConfig,
    select: &[usize],
) -> GapCorpus {
    let CharacterizeAbPcm {
        a_pcm,
        b_samples,
        sources,
    } = *pcm;
    let a_samples = a_pcm.samples.as_slice();
    let sample_rate = a_pcm.sample_rate;
    let channels = a_pcm.channels as usize;
    if let Some(src) = sources {
        if channel_layout_mismatch(src) {
            return refused_channel_mismatch_corpus(
                report,
                a_samples,
                b_samples,
                sample_rate,
                a_pcm.channels,
                src,
            );
        }
    }
    let rate = f64::from(sample_rate).max(1.0);
    let ch = channels.max(1);
    let b_total = b_samples.len() / ch;
    // Per-gap B haystack pad: context + border search + margin/slack on each side (mirrors
    // `prepare_region_patch`). Bounds the unified search so it does not build a timeline over all of B.
    let pad_lead =
        cfg.gap_signature_context_secs + cfg.fill_border_search_secs + cfg.fill_align_margin_secs;
    let pad_tail = cfg.gap_signature_context_secs
        + cfg.fill_border_search_secs
        + cfg
            .fill_extract_tail_slack_secs
            .max(cfg.fill_align_margin_secs)
        + cfg.fill_align_margin_secs;

    let take_all = select.is_empty();
    let gaps = report
        .gaps
        .iter()
        .enumerate()
        .filter(|(i, _)| take_all || select.contains(i))
        .map(|(i, gap)| {
            // One guarded read of the mapped span: the reported offset and the extract window
            // must come from the same validated coordinates (F10).
            let mapped_b = gap.mapped_b_span();
            let gap_offset_secs = mapped_b
                .map(|(b0, _)| b0 - gap.video_a_start_secs)
                .unwrap_or(0.0);

            let (b_haystack, b_extract_start_secs) = match mapped_b {
                Some((b_start, b_end)) => {
                    let extract_start = (b_start - pad_lead).max(0.0);
                    let extract_end = b_end + pad_tail;
                    let lo = ((extract_start * rate) as usize).min(b_total);
                    let hi = ((extract_end * rate).ceil() as usize).min(b_total);
                    if hi > lo {
                        (Some(&b_samples[lo * ch..hi * ch]), lo as f64 / rate)
                    } else {
                        (None, 0.0)
                    }
                }
                None => (None, 0.0),
            };

            let inputs = GapInputs {
                a_samples,
                b_haystack,
                channels,
                sample_rate,
                reported_start_frame: (gap.video_a_start_secs * rate) as usize,
                reported_end_frame: (gap.video_a_end_secs * rate) as usize,
                b_extract_start_secs,
                gap_offset_secs,
                config: *cfg,
            };
            build_gap_fingerprint(i, &inputs, DetailTier::Summary, true)
        })
        .collect();

    GapCorpus {
        source: SourceMeta {
            a_source: file_source(
                a_samples,
                sample_rate,
                channels as u16,
                sources.map(|s| &s.a),
            ),
            b_source: file_source(
                b_samples,
                sample_rate,
                channels as u16,
                sources.map(|s| &s.b),
            ),
            scan_recipe: CorpusScanRecipe::from_report(report),
            gap_count: report.gaps.len(),
            incomparable: None,
            // Summary-only path does not run the seam gate; from-decode stamps `gate_recipe`.
            gate_recipe: None,
            // **No `not_measured` here.** This path measures every one of those fields —
            // `build_gap_fingerprint` below fills levels/silence/contour/anchors for real. The
            // declaration belongs to `characterize_gaps_from_decode`, which *replaces* these gaps with
            // `spec_to_fingerprint_summary` output and strips them. Stamping it here (as this function
            // did briefly on 2026-08-01) makes a standalone `characterize_gaps` corpus disown data it
            // actually holds — the same unreadable-field defect the declaration exists to fix, pointed
            // the other way, and worse: an absent declaration invites a misread, a false one licenses
            // discarding real measurements.
            not_measured: Vec::new(),
        },
        gaps,
    }
}

#[cfg(any(feature = "calibration", test))]
fn detail_tier_str(t: DetailTier) -> &'static str {
    match t {
        DetailTier::Summary => "summary",
        DetailTier::Full => "full",
    }
}

#[cfg(any(feature = "calibration", test))]
fn lag_verdict_str(v: LagVerdict) -> &'static str {
    match v {
        LagVerdict::TimingOffset => "timing_offset",
        LagVerdict::Decorrelated => "decorrelated",
        LagVerdict::Ambiguous => "ambiguous",
    }
}

#[cfg(any(feature = "calibration", test))]
fn hms(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}-{:02}-{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Headline tag for a gap's filename: the lag verdict if measured, else the gate outcome, else `na`.
#[cfg(any(feature = "calibration", test))]
fn entry_verdict(gap: &GapFingerprint) -> String {
    gap.lag_editorial
        .as_ref()
        .and_then(|l| l.pre_anchor.first().or_else(|| l.post_anchor.first()))
        .map(|s| lag_verdict_str(s.verdict).to_string())
        .or_else(|| gap.outcome.as_ref().map(|o| o.tier.clone()))
        .unwrap_or_else(|| "na".to_string())
}

/// `<a8>_<b4>_t<hh-mm-ss>_g<idx>_<tier>_<verdict>` — non-leaking, sortable, classifiable.
///
/// **Extension-free on purpose.** This is the single naming authority for everything a gap emits:
/// `write_corpus_dir` appends `.json`, and the `--gap-listen` export appends `_a_surround.wav` /
/// `_b_surround.wav` / `_a_patched.wav`. That shared stem is the ears ↔ JSON join, so a WAV can
/// always be traced back to the fingerprint that describes it. Baking `.json` in here (as this did
/// before 2026-08-02) would force the exporter to reconstruct the format string and let the two
/// namings drift apart silently.
///
/// Note the stem is a function of the **built** fingerprint (`tier`, `verdict`, refined start), not
/// of the gap alone — so an exporter must look its gap up in the corpus by `index`.
#[cfg(any(feature = "calibration", test))]
pub(crate) fn entry_stem(source: &SourceMeta, gap: &GapFingerprint) -> String {
    let a8: String = source.a_source.id.chars().take(8).collect();
    let b4: String = source.b_source.id.chars().take(4).collect();
    format!(
        "{a8}_{b4}_t{}_g{:03}_{}_{}",
        hms(gap.geometry.a_refined_start_secs),
        gap.index,
        detail_tier_str(gap.tier),
        entry_verdict(gap),
    )
}

#[cfg(any(feature = "calibration", test))]
#[derive(serde::Serialize)]
struct ManifestEntry {
    file: String,
    index: usize,
    a_start_secs: f64,
    tier: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lag_verdict: Option<String>,
}

#[cfg(any(feature = "calibration", test))]
#[derive(serde::Serialize)]
struct Manifest<'a> {
    a_id: &'a str,
    b_id: &'a str,
    scan_recipe: &'a CorpusScanRecipe,
    gap_count: usize,
    entries: Vec<ManifestEntry>,
}

/// Write a self-contained corpus directory: the combined `corpus.json` (all gaps), one single-gap
/// [`GapCorpus`] JSON **per gap**, and a non-identifying `manifest.json`. Returns the gap count. No
/// titles/paths anywhere.
#[cfg(any(feature = "calibration", test))]
pub(crate) fn write_corpus_dir(
    corpus: &GapCorpus,
    dir: &std::path::Path,
) -> std::io::Result<usize> {
    let to_io = |e: serde_json::Error| std::io::Error::other(e);
    std::fs::create_dir_all(dir)?;
    // Combined corpus (all gaps) for quick inspection / scripting.
    let combined = std::fs::File::create(dir.join("corpus.json"))?;
    serde_json::to_writer_pretty(combined, corpus).map_err(to_io)?;

    let mut entries = Vec::with_capacity(corpus.gaps.len());
    for gap in &corpus.gaps {
        let file = format!("{}.json", entry_stem(&corpus.source, gap));
        let single = GapCorpus {
            source: corpus.source.clone(),
            gaps: vec![gap.clone()],
        };
        let f = std::fs::File::create(dir.join(&file))?;
        serde_json::to_writer_pretty(f, &single).map_err(to_io)?;
        entries.push(ManifestEntry {
            file,
            index: gap.index,
            a_start_secs: gap.geometry.a_refined_start_secs,
            tier: detail_tier_str(gap.tier),
            outcome: gap.outcome.as_ref().map(|o| o.tier.clone()),
            lag_verdict: gap
                .lag_editorial
                .as_ref()
                .and_then(|l| l.pre_anchor.first().or_else(|| l.post_anchor.first()))
                .map(|s| lag_verdict_str(s.verdict).to_string()),
        });
    }
    let manifest = Manifest {
        a_id: &corpus.source.a_source.id,
        b_id: &corpus.source.b_source.id,
        scan_recipe: &corpus.source.scan_recipe,
        gap_count: corpus.gaps.len(),
        entries,
    };
    let mf = std::fs::File::create(dir.join("manifest.json"))?;
    serde_json::to_writer_pretty(mf, &manifest).map_err(to_io)?;
    Ok(corpus.gaps.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- noise-floor probes (F15 second axis; provenance only) -------------------------------------

    /// A gap flanked by −40 dBFS context: the probe reads that context, not the silent gap it excludes.
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

    /// **8g.3b — `tags_from_measurements` reads the shared measurements correctly.** Builds a
    /// `RegionMeasurements` with distinctive values and asserts the D/R tags map the expected fields (brackets
    /// counts, seam_local from splice_dualfit, per-side donor, registration from splice, residual, levels), and
    /// that `structure`/`seams` are omitted (`None`) — matching the `skip_baseline_placement` dump so
    /// from-decode tags equal the oracle's on that path. (The shared `tags_from_fields` core is validated
    /// end-to-end for `tags_from_fingerprint` by the 8g.1 / media / C4 differentials.)
    #[test]
    fn tags_from_measurements_maps_the_shared_fields() {
        use crate::domain::donor::DonorInterior;
        use crate::domain::gap_repair_spec::LevelTags;

        let donor = |silence: f64, cont: bool| DonorInterior {
            rms_db: -20.0,
            silence_fraction: silence,
            longest_silence_ms: 0.0,
            continuous: cont,
            basis: None,
        };
        let bracket = |stage: Option<FailureStage>, seam: Option<f64>| BracketInfo {
            pre_time_secs: 0.0,
            post_time_secs: 0.0,
            span_secs: 0.0,
            move_frames: 0,
            structure_pre: None,
            structure_post: None,
            seam_pre: seam,
            seam_post: seam,
            start_frame: None,
            fill_frames: None,
            failure_stage: stage,
            residual_margin_db: None,
        };
        let m = RegionMeasurements {
            brackets: vec![
                bracket(None, Some(0.7)),
                bracket(Some(FailureStage::WaveformFloor), Some(0.4)),
            ],
            outcome: GateOutcome {
                plan_kind: "fillable".into(),
                tier: "patch".into(),
                seam_shape: None,
                fit_path: None,
                signature_mode: None,
                skip_reason: None,
                // Patched by the bracket gate ⇒ dual-fit is never consulted.
                dual_fit_rescue: None,
            },
            lag_decision: None,
            splice: Some(SpliceSummary {
                step_ms: 12.5,
                pre_peak_r: 0.93,
                post_peak_r: 0.91,
                pre_peak_z: Some(14.0),
                post_peak_z: Some(13.0),
                edge_pinned: Some(false),
            }),
            seam_probe: None,
            donor_interior: Some(donor(0.05, true)),
            donor_interior_nominal: Some(donor(0.10, false)),
            b_levels: None,
            splice_dualfit: Some(SpliceDualfit {
                pre_seam_r: 0.97,
                post_seam_r: 0.95,
                gap_frames: 24_000,
                bridge_frames: 24_480,
                trim_frames: 480,
                gate_pass: true,
                post_seam_global_r: 0.40,
                pre_seam_prom: None,
                post_seam_prom: None,
                pre_seam_z: None,
                post_seam_z: None,
            }),
            wide_envelope: None,
            residual: Some(ResidualInfo {
                chosen_pre_db: Some(-42.0),
                chosen_post_db: Some(-41.0),
                floor_pre_db: Some(-40.0),
                floor_post_db: Some(-40.0),
                floor_source_pre: Some(SeamFloorSource::Border),
                floor_source_post: Some(SeamFloorSource::Border),
                informative: true,
                uninformative_pre: None,
                uninformative_post: None,
                placement_slide_frames: Some(0),
                max_lag_frames: Some(0),
            }),
            lag_editorial: None,
        };
        let levels = LevelTags {
            a_gap_floor_db: -70.0,
            a_noise_floor_db: -60.0,
        };

        let tags = tags_from_measurements(&m, Some(levels));

        // Gate counts from the bracket list; structure/seams omitted (skip_baseline).
        assert_eq!(tags.gate.brackets_total, 2);
        assert_eq!(tags.gate.brackets_passing, 1);
        assert_eq!(tags.gate.structure_min, None, "structure omitted");
        assert_eq!(tags.gate.seam_min, None, "seams omitted");
        assert!(tags.gate.residual.is_some(), "residual mapped");
        // seam_local from splice_dualfit (single-source).
        let sl = tags.seam_local.expect("seam_local");
        assert_eq!(sl.pre_seam_r, 0.97);
        assert_eq!(sl.post_seam_r, 0.95);
        assert_eq!(sl.post_seam_global_r, 0.40);
        assert!(sl.gate_pass);
        // registration from splice; per-side donor mapped (nominal vs aligned).
        assert_eq!(tags.registration.pre_peak_r, Some(0.93));
        assert_eq!(tags.registration.step_ms, Some(12.5));
        assert_eq!(tags.donor_aligned.unwrap().silence_fraction, 0.05);
        assert_eq!(tags.donor_nominal.unwrap().silence_fraction, 0.10);
        assert_eq!(tags.levels.unwrap().a_gap_floor_db, -70.0);
    }

    /// splitmix64 finalizer → deterministic noise in [-1, 1).
    fn noise(seed: u64, i: usize) -> f64 {
        let mut z = ((seed << 32) | (i as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 40) as f64 / (1u64 << 24) as f64) * 2.0 - 1.0
    }

    fn base_noise(seed: u64, len: usize) -> Vec<f64> {
        (0..len).map(|i| noise(seed, i)).collect()
    }

    fn lerp(base: &[f64], x: f64) -> f64 {
        if x <= 0.0 {
            return base[0];
        }
        let i = x.floor() as usize;
        if i + 1 >= base.len() {
            return base[base.len() - 1];
        }
        let f = x - i as f64;
        base[i] * (1.0 - f) + base[i + 1] * f
    }

    /// Build (a, b_ctx) where B is the shared broadband signal delayed by `offset` samples (possibly
    /// fractional), so the lag curve should peak at lag ≈ `offset`.
    fn shifted_pair(offset: f64, n: usize, max_lag: i64) -> (Vec<f64>, Vec<f64>) {
        let ml = max_lag as f64;
        let a0 = (ml + offset.abs()).ceil() as usize + 4;
        let base = base_noise(1, a0 + n + max_lag as usize + 8);
        let a: Vec<f64> = (0..n).map(|i| base[a0 + i]).collect();
        let ctx_len = n + 2 * max_lag as usize;
        let b_ctx: Vec<f64> = (0..ctx_len)
            .map(|k| lerp(&base, a0 as f64 + (k as f64 - ml - offset)))
            .collect();
        (a, b_ctx)
    }

    #[test]
    fn lag_curve_finds_integer_offset() {
        let (a, b_ctx) = shifted_pair(17.0, 4000, 64);
        let curve = lag_correlation_curve(&a, &b_ctx, 64);
        let s = summarize_lag_curve(&curve, 48_000, 83, 1, LagChannel::Mono).expect("summary");
        assert_eq!(s.peak_lag_samples, 17, "peak at the true integer lag");
        assert!(
            s.peak_r > 0.95,
            "shared signal correlates ~1 at the lag, got {}",
            s.peak_r
        );
        assert!(
            s.lag0_r < 0.5,
            "lag-0 is depressed by the offset, got {}",
            s.lag0_r
        );
        assert_eq!(s.verdict, LagVerdict::TimingOffset);
    }

    #[test]
    fn lag_curve_resolves_fractional_offset() {
        let (a, b_ctx) = shifted_pair(17.4, 4000, 64);
        let curve = lag_correlation_curve(&a, &b_ctx, 64);
        let s = summarize_lag_curve(&curve, 48_000, 83, 1, LagChannel::Mono).expect("summary");
        assert!(
            (s.frac_lag_samples - 17.4).abs() < 0.3,
            "parabolic lag {} should land near 17.4",
            s.frac_lag_samples
        );
        assert_eq!(s.verdict, LagVerdict::TimingOffset);
    }

    #[test]
    fn lag_curve_flat_for_decorrelated_sources() {
        let n = 4000;
        let max_lag = 64i64;
        let a = base_noise(1, n);
        let b_ctx = base_noise(2, n + 2 * max_lag as usize); // independent stream
        let curve = lag_correlation_curve(&a, &b_ctx, max_lag);
        let s = summarize_lag_curve(&curve, 48_000, 83, 1, LagChannel::Mono).expect("summary");
        assert!(
            s.peak_r < 0.3,
            "independent noise should not correlate at any lag, peak {}",
            s.peak_r
        );
        assert_eq!(s.verdict, LagVerdict::Decorrelated);
    }

    #[test]
    fn lag_summary_flags_competing_peak_for_periodic_curve() {
        // Unique peak: one hump, monotonic falloff → no rival local maximum.
        let unique: Vec<(i64, f64)> = (-5..=5)
            .map(|l| (l, 1.0 - 0.1 * (l as f64).abs()))
            .collect();
        let s = summarize_lag_curve(&unique, 48_000, 10, 1, LagChannel::Mono).expect("summary");
        assert!(
            s.second_peak_r.is_none_or(|r| r < s.peak_r - 0.3),
            "a unique peak should have no strong rival: {:?}",
            s.second_peak_r
        );

        // Periodic-like: two humps of similar height → a rival near peak_r (low uniqueness margin).
        let periodic = vec![
            (-4, 0.2),
            (-3, 0.9),
            (-2, 0.5),
            (-1, 0.3),
            (0, 0.85),
            (1, 0.4),
            (2, 0.2),
            (3, 0.1),
            (4, 0.05),
        ];
        let s2 = summarize_lag_curve(&periodic, 48_000, 10, 1, LagChannel::Mono).expect("summary");
        assert!(
            s2.second_peak_r.is_some_and(|r| r > 0.8),
            "a periodic curve should expose a competing peak near peak_r: {:?}",
            s2.second_peak_r
        );

        // Robust uniqueness fields: the unique (monotonic) curve has NO rival → prominence `None` (best
        // case); the periodic one has a rival near the peak → low prominence. peak_z stands out further for
        // the unique curve. Spacing is the lag gap to the rival.
        assert!(
            s.prominence.is_none(),
            "a unrivalled peak has no prominence value: {:?}",
            s.prominence
        );
        assert!(
            s2.prominence.is_some_and(|p| p < 0.1),
            "periodic: low prominence {:?}",
            s2.prominence
        );
        // peak_z is computed (finite, positive) whenever the curve has spread; its discriminating power is
        // validated on real flat-floor curves in the §3.6a experiment, not these broad toy humps.
        assert!(
            s.peak_z.is_some_and(|z| z.is_finite() && z > 0.0),
            "peak_z computed: {:?}",
            s.peak_z
        );
        assert!(
            s2.peak_z.is_some_and(|z| z.is_finite() && z > 0.0),
            "peak_z computed: {:?}",
            s2.peak_z
        );
        // periodic peak at lag 0, rival at lag −3 → spacing 3 samples = 62.5 µs at 48 kHz.
        assert!(
            s2.top2_spacing_ms
                .is_some_and(|ms| (ms - 3.0 * 1000.0 / 48_000.0).abs() < 1e-6),
            "spacing to the rival lag: {:?}",
            s2.top2_spacing_ms
        );
    }

    #[test]
    fn lag_curve_truncates_when_context_too_short() {
        // b_ctx shorter than a.len() + 2*max_lag → fewer than the full set of lags, no panic.
        let a = base_noise(1, 100);
        let b_ctx = base_noise(1, 120);
        let curve = lag_correlation_curve(&a, &b_ctx, 64);
        assert!(curve.len() < (2 * 64 + 1));
        assert!(curve.iter().all(|(_, r)| r.is_finite()));
    }

    /// Pre-fix post search: center on `start_frame + gap_frames` with no pre-shift — stacks
    /// `L_pre + (D_B - D_A)` into one ±max_lag window (ledger A2).
    fn naive_lag_pair_post(
        a_post: &[f64],
        b_signal: &[f64],
        start_frame: usize,
        gap_frames: usize,
        params: LagSweepParams,
    ) -> Option<LagSummary> {
        lag_side_sweep(
            LagSideSweep {
                a_border: a_post,
                b_signal,
                anchor_frame: start_frame + gap_frames,
                pre_shoulder: false,
                gross_lag_shift: 0,
            },
            params,
        )
    }

    /// Regression: when `|L_pre + (D_B - D_A)|` exceeds ±max_lag but `|D_B - D_A|` alone fits,
    /// sequential `lag_pair` finds the post shoulder while naive centering does not.
    #[test]
    fn lag_pair_sequential_decouples_pre_offset_from_bridge_mismatch() {
        const RATE: u32 = 48_000;
        let w = 4_000usize;
        let max_lag = 4_000i64;
        let ml = max_lag as usize;
        let start_frame = 30_000usize;
        let gap_frames = 20_000usize; // D_A — keep post burst out of the pre search window
        let l_pre_true = 3_000i64;
        let bridge_delta = 1_500i64; // |L_pre + bridge| = 4500 > max_lag; |bridge| = 1500 <= max_lag

        let b_post_match = start_frame as i64 + l_pre_true + gap_frames as i64 + bridge_delta;
        let naive_lag_needed = b_post_match - (start_frame + gap_frames) as i64;
        assert_eq!(naive_lag_needed, l_pre_true + bridge_delta);
        assert!(naive_lag_needed.unsigned_abs() > max_lag as u64);

        let mut b_signal = vec![0.0f64; 100_000];

        let (a_pre, pre_ctx) = shifted_pair(l_pre_true as f64, w, max_lag);
        let pre_lo = start_frame.saturating_sub(w + ml);
        for (i, &v) in pre_ctx.iter().enumerate() {
            if pre_lo + i < b_signal.len() {
                b_signal[pre_lo + i] = v;
            }
        }

        let (a_post, post_ctx) = shifted_pair(bridge_delta as f64, w, max_lag);
        let seq_post_base = start_frame as i64 + gap_frames as i64 + l_pre_true;
        let post_lo = (seq_post_base - ml as i64).max(0) as usize;
        for (i, &v) in post_ctx.iter().enumerate() {
            if post_lo + i < b_signal.len() {
                b_signal[post_lo + i] = v;
            }
        }

        let sweep = LagSweepParams {
            window: w,
            max_lag,
            sample_rate: RATE,
            channel: LagChannel::Mono,
        };

        let (pre, post_seq) = lag_pair(&a_pre, &a_post, &b_signal, start_frame, gap_frames, sweep);
        let pre = pre.expect("pre shoulder");
        let post_seq = post_seq.expect("sequential post shoulder");
        assert!(
            (pre.frac_lag_samples - l_pre_true as f64).abs() < 5.0,
            "pre lag {} should land near {l_pre_true}",
            pre.frac_lag_samples
        );
        assert!(pre.peak_r > 0.95, "pre peak_r {}", pre.peak_r);

        let gross_post_expected = l_pre_true + bridge_delta;
        assert!(
            (post_seq.frac_lag_samples - gross_post_expected as f64).abs() < 5.0,
            "gross post lag {} should land near {gross_post_expected}",
            post_seq.frac_lag_samples
        );
        assert!(
            post_seq.peak_r > 0.95,
            "sequential post peak_r {}",
            post_seq.peak_r
        );

        let post_naive = naive_lag_pair_post(&a_post, &b_signal, start_frame, gap_frames, sweep)
            .expect("naive post summary");
        assert!(
            post_naive.peak_r < post_seq.peak_r - 0.15,
            "naive post should be worse than sequential (naive r {} at lag {} vs sequential r {})",
            post_naive.peak_r,
            post_naive.frac_lag_samples,
            post_seq.peak_r
        );
    }

    #[test]
    fn weighted_downmix_recovers_center_dominant_level() {
        // 6ch center-dominant 5.1 seam: loud center (idx 2), quiet L/R, silent surrounds/LFE.
        let ch = 6;
        let n = 480;
        let mut s = vec![0.0f32; n * ch];
        for f in 0..n {
            s[f * ch + 2] =
                (std::f64::consts::TAU * 200.0 * f as f64 / 48_000.0).sin() as f32 * 0.5;
            s[f * ch] = 0.005;
            s[f * ch + 1] = -0.005;
        }
        let weighted = weighted_downmix_rms(&s, ch, 0, n);
        let mono: Vec<f64> = s
            .chunks(ch)
            .map(|fr| fr.iter().map(|&x| x as f64).sum::<f64>() / ch as f64)
            .collect();
        let mono_rms = (mono.iter().map(|v| v * v).sum::<f64>() / mono.len() as f64).sqrt();
        // The straight 1/6 mix buries the center; the energy-weighted mix keeps it (~0.5/√2 ≈ 0.35).
        assert!(
            weighted > 0.2,
            "weighted preserves center level: {weighted}"
        );
        assert!(
            weighted > mono_rms * 3.0,
            "weighted {weighted} ≫ straight mono {mono_rms}"
        );
        // Over-range / empty spans are guarded.
        assert_eq!(weighted_downmix_rms(&s, ch, 10, 10), 0.0);
        assert_eq!(
            weighted_downmix_rms(&s, ch, n - 5, n + 100),
            weighted_downmix_rms(&s, ch, n - 5, n)
        );
    }

    #[test]
    fn splice_summary_from_lag_decision_mono() {
        let lag = LagFingerprint {
            pre_anchor: vec![LagSummary {
                window_ms: 1000,
                max_lag_ms: 200,
                channel: LagChannel::Mono,
                lag0_r: 0.1,
                peak_r: 0.99,
                second_peak_r: Some(0.2),
                peak_z: Some(16.0),
                prominence: Some(0.79),
                top2_spacing_ms: Some(40.0),
                peak_lag_samples: -500,
                frac_lag_samples: -500.0,
                frac_lag_ms: -10.5,
                edge_pinned: Some(false),
                verdict: LagVerdict::TimingOffset,
            }],
            post_anchor: vec![LagSummary {
                window_ms: 1000,
                max_lag_ms: 200,
                channel: LagChannel::Mono,
                lag0_r: 0.2,
                peak_r: 0.96,
                second_peak_r: Some(0.3),
                peak_z: Some(14.0),
                prominence: Some(0.66),
                top2_spacing_ms: Some(50.0),
                peak_lag_samples: -300,
                frac_lag_samples: -300.0,
                frac_lag_ms: -6.2,
                edge_pinned: Some(false),
                verdict: LagVerdict::TimingOffset,
            }],
        };
        let s = splice_summary_from_lag(&lag).expect("splice");
        assert!((s.step_ms - 4.3).abs() < 0.01, "step {}", s.step_ms);
        assert!((s.pre_peak_r - 0.99).abs() < 1e-6);
        assert!((s.post_peak_r - 0.96).abs() < 1e-6);
        assert_eq!(s.pre_peak_z, Some(16.0));
        assert_eq!(s.post_peak_z, Some(14.0));
        // Neither shoulder search-exhausted → step is trustworthy.
        assert_eq!(s.edge_pinned, Some(false));

        // One edge-pinned shoulder taints the step (either side ⇒ true).
        let mut pinned = lag.clone();
        pinned.post_anchor[0].edge_pinned = Some(true);
        assert_eq!(
            splice_summary_from_lag(&pinned)
                .expect("splice")
                .edge_pinned,
            Some(true),
        );

        // No shoulder carries the flag (old fingerprint) → unknown, not a false negative.
        let mut legacy = lag.clone();
        legacy.pre_anchor[0].edge_pinned = None;
        legacy.post_anchor[0].edge_pinned = None;
        assert_eq!(
            splice_summary_from_lag(&legacy)
                .expect("splice")
                .edge_pinned,
            None,
        );
    }

    #[test]
    fn edge_pinned_flags_boundary_peak() {
        // A curve whose maximum sits at the top boundary lag is search-exhausted (true optimum ≥ edge).
        let sr = 48_000u32;
        let curve: Vec<(i64, f64)> = (-4800..=4800).map(|l| (l, l as f64 / 4800.0)).collect();
        let s = summarize_lag_curve(&curve, sr, 1000, 100, LagChannel::Mono).expect("summary");
        assert_eq!(s.peak_lag_samples, 4800, "peak at the boundary");
        assert_eq!(s.edge_pinned, Some(true), "boundary peak is edge-pinned");

        // A curve peaking in the interior is not edge-pinned.
        let interior: Vec<(i64, f64)> = (-4800..=4800)
            .map(|l| (l, -((l as f64) / 4800.0).powi(2)))
            .collect();
        let s2 = summarize_lag_curve(&interior, sr, 1000, 100, LagChannel::Mono).expect("summary");
        assert_eq!(s2.peak_lag_samples, 0, "peak in the interior");
        assert_eq!(s2.edge_pinned, Some(false), "interior peak not edge-pinned");
    }

    #[test]
    fn wide_envelope_finds_shifted_segment_peak() {
        let rate = 48_000u32;
        let r = f64::from(rate);
        let max_lag = ((WIDE_ENV_MAX_LAG_MS / 1000.0) * r).round() as i64;
        let window = (WIDE_ENV_WINDOW_SECS * r).round() as usize;
        let env_bin = ((WIDE_ENV_BIN_MS / 1000.0) * r).round() as usize;
        // Offset must be visible at 100 ms-bin resolution (~1 bin = 100 ms).
        let offset_samples = env_bin as f64;
        let (a, b_ctx) = shifted_pair(offset_samples, window, max_lag);
        let wide_lag = max_lag as usize;
        let peak = wide_envelope_side(&a, &b_ctx, rate, env_bin, wide_lag).expect("env peak");
        assert!(peak.peak_r > 0.9, "segment match {}", peak.peak_r);
        let expected_ms = offset_samples * 1000.0 / r;
        assert!(
            (peak.peak_lag_ms - expected_ms).abs() < 50.0,
            "env peak lag {} should land near {expected_ms} ms",
            peak.peak_lag_ms
        );
        // Non-vacuous: `finite_corr` guarantees finite outputs even on a flat/degenerate envelope.
        assert!(
            peak.prominence.is_finite() && peak.peak_r.is_finite(),
            "finite: {peak:?}"
        );
    }

    fn write_speech(buf: &mut [f32], start: usize, end: usize, freq: f64, amp: f32) {
        let n = (end - start) as f64;
        for (f, slot) in buf.iter_mut().enumerate().take(end).skip(start) {
            let t = (f - start) as f64;
            let env = 0.5 - 0.5 * (std::f64::consts::TAU * t / n).cos();
            let s = (std::f64::consts::TAU * freq * t / 48_000.0).sin();
            *slot = (env * s) as f32 * amp;
        }
    }

    fn write_noise(buf: &mut [f32], start: usize, end: usize, seed: u64, amp: f32) {
        for (f, slot) in buf.iter_mut().enumerate().take(end).skip(start) {
            *slot = noise(seed, f) as f32 * amp;
        }
    }

    #[test]
    fn builder_characterizes_noise_collar_gap() {
        let rate = 48_000u32;
        let ch = 1usize;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let total = secs(5.0);
        let (sp1, n1, gap, n2, sp2) = (
            (secs(0.50), secs(0.85)),
            (secs(0.85), secs(1.85)),
            (secs(1.85), secs(3.35)),
            (secs(3.35), secs(4.35)),
            (secs(4.35), secs(4.70)),
        );
        let mut a = vec![0f32; total];
        let mut b = vec![0f32; total];
        // Same-master speech bursts (identical), decorrelated collars, B fill in the gap.
        write_speech(&mut a, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut b, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut a, sp2.0, sp2.1, 440.0, 0.079);
        write_speech(&mut b, sp2.0, sp2.1, 440.0, 0.079);
        write_noise(&mut a, n1.0, n1.1, 1, 0.0056);
        write_noise(&mut b, n1.0, n1.1, 11, 0.0056);
        write_noise(&mut a, n2.0, n2.1, 3, 0.0056);
        write_noise(&mut b, n2.0, n2.1, 13, 0.0056);
        write_noise(&mut b, gap.0, gap.1, 5, 0.0056);

        let config = FingerprintConfig {
            gap_signature_context_secs: 1.5,
            fill_border_search_secs: 1.0,
            border_secs: 0.3,
            border_standoff_secs: 0.0,
            ..Default::default()
        };
        let inputs = GapInputs {
            a_samples: &a,
            b_haystack: Some(&b),
            channels: ch,
            sample_rate: rate,
            reported_start_frame: gap.0,
            reported_end_frame: gap.1,
            b_extract_start_secs: 0.0,
            gap_offset_secs: 0.0,
            config,
        };
        let fp = build_gap_fingerprint(0, &inputs, DetailTier::Full, false);

        assert!(
            (fp.geometry.duration_secs - 1.5).abs() < 0.05,
            "duration {}",
            fp.geometry.duration_secs
        );
        assert!(
            fp.anchors
                .as_ref()
                .expect("measured anchors")
                .pre
                .iter()
                .any(|p| p.source == AnchorSourceKind::EnergyPeak),
            "expected a pre energy-peak anchor: {:?}",
            fp.anchors.as_ref().map(|a| &a.pre)
        );
        assert!(
            fp.contour
                .as_ref()
                .expect("measured contour")
                .has_anchor_seam_contour,
            "speech bursts give contour"
        );

        let s = fp.structure.expect("structure present with B");
        assert!(
            s.baseline_pre > 0.5,
            "structure aligns at the throat: {}",
            s.baseline_pre
        );
        let seam = fp.seams.expect("seams present with B");
        assert!(
            seam.baseline_pre < 0.2 && seam.baseline_post < 0.2,
            "throat seam collapses in decorrelated noise: pre={} post={}",
            seam.baseline_pre,
            seam.baseline_post
        );
        assert!(
            fp.brackets.iter().any(
                |b| b.seam_pre.is_some_and(|v| v > 0.5) && b.seam_post.is_some_and(|v| v > 0.5)
            ),
            "a speech-anchored bracket should score a strong seam where the throat cannot"
        );
        assert!(
            fp.lag_editorial.is_some(),
            "lag computed at the best speech bracket"
        );
    }

    /// One 2.5 s gap with speech shoulders on both sides and low-level noise filling B's donor window:
    /// enough for the full projection path to produce a `DetailTier::Full` gap with a real donor.
    fn equivalence_overlay_fixture() -> (
        crate::domain::GapReport,
        clip_sync::MultiChannelPcm,
        Vec<f32>,
    ) {
        use crate::domain::gap::Gap;
        use crate::domain::{GapReport, ScanAlignment};
        use clip_sync::MultiChannelPcm;

        let rate = 48_000u32;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let mut a = vec![0f32; secs(6.0)];
        let mut b = vec![0f32; secs(6.0)];
        write_speech(&mut a, secs(0.5), secs(1.5), 330.0, 0.063);
        write_speech(&mut b, secs(0.5), secs(1.5), 330.0, 0.063);
        write_speech(&mut a, secs(4.0), secs(5.0), 440.0, 0.079);
        write_speech(&mut b, secs(4.0), secs(5.0), 440.0, 0.079);
        write_noise(&mut b, secs(1.5), secs(4.0), 5, 0.0056);
        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: 1,
            samples: a,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        let report = GapReport {
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            gaps: vec![Gap {
                video_a_start_secs: 1.5,
                video_a_end_secs: 4.0,
                video_b_start_secs: Some(1.5),
                video_b_end_secs: Some(4.0),
                b_has_energy: true,
            }],
            gap_equivalence: vec![Default::default()],
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
        };
        (report, a_pcm, b)
    }

    /// `characterize_gaps` alone measures those fields, so it must **not** declare them unmeasured.
    ///
    /// The declaration is owned by the projection rebuild in `characterize_gaps_from_decode`, not by
    /// the summary builder that feeds it. Stamped one function too early it would make this corpus
    /// disown levels/silence/contour/anchors it genuinely holds — and `--check` treats a false
    /// declaration as an error precisely because it licenses discarding real data.
    #[test]
    fn summary_characterize_does_not_declare_fields_it_measured() {
        let (report, a_pcm, b) = equivalence_overlay_fixture();
        let corpus = characterize_gaps(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &FingerprintConfig::default(),
            &[],
        );
        assert!(
            corpus.source.not_measured.is_empty(),
            "summary path measures these fields; declaring them unmeasured is a false claim: {:?}",
            corpus.source.not_measured
        );
        // Not vacuous: prove the path really does fill the envelope it used to declare unmeasured.
        assert!(
            corpus
                .gaps
                .iter()
                .any(|g| g.levels.bin_ms.is_some_and(|b| b != 0) || !g.levels.profile_db.is_empty()),
            "fixture must exercise a measured levels envelope field"
        );
    }

    /// The probe row matching the live recipe **reproduces the live floor exactly**.
    ///
    /// That identity is what makes the grid an attribution instrument: rows differ by the three axes
    /// they vary and by nothing else, so a row-to-row delta *is* that axis's term. It held until the
    /// A-span convergence moved the live measurement onto the scan core and left the grid on this
    /// path's `refined` span — an unlabelled fourth term worth up to 1.63 dB against the live floor
    /// and 7.49 dB against scan's, which is the size of the I2 effect the grid exists to measure.
    #[test]
    fn noise_floor_probes_measure_the_span_the_equivalence_measurement_used() {
        let (report, a_pcm, b) = equivalence_overlay_fixture();
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &crate::infrastructure::config::RepairConfig::default()
                .patch_settings()
                .into_request(report.clone())
                .expect("default All gap selection"),
            &[],
            false,
            &clip_sync_repair_fixtures::NoOpProgressReporter,
        );
        let eq = corpus.gaps[0]
            .equivalence_diagnostic_verdict()
            .expect("overlay ran");
        let m = eq
            .measurement
            .as_ref()
            .expect("measurement recipe recorded");
        let live = eq.noise_floor_db.expect("a floor was measured");
        let row = eq
            .noise_floor_probes
            .iter()
            .find(|p| {
                p.context_secs == m.context_secs
                    && p.bin_ms == m.bin_ms
                    && p.reduction == m.reduction
            })
            .expect("the grid must contain the live recipe's own row");
        assert!(
            (row.floor_db.expect("row measured") - live).abs() < 1e-9,
            "probe row {:?} reads {:?} but the live measurement reads {live} — the grid is measuring \
             a different span, so every row carries an unlabelled term",
            (row.context_secs, row.bin_ms, row.reduction),
            row.floor_db,
        );
    }

    /// A donor window running past B's end is **refused**, not measured against a clamped remainder.
    ///
    /// `measure_gap_equivalence` clamps its frame range to what exists, so without the guard a tail
    /// gap measures a *shorter* window whose missing part reads as digital silence — 20 gaps of the
    /// 39-pair corpus scored 99.3–100 % silent that way and drew `shared_silence`/drop from audio
    /// B does not contain. Scan already fails closed here, so the divergence was never about silence,
    /// only about whether to answer. Absent donor ⇒ no fraction ⇒ keep.
    #[test]
    fn diagnostic_equivalence_refuses_a_donor_window_past_b_end() {
        let (report, a_pcm, b) = equivalence_overlay_fixture();
        // B stops at 3.0 s; the gap's donor window is 1.5–4.0 s, so 1 s of it does not exist.
        let truncated: Vec<f32> = b[..(3.0 * 48_000.0) as usize].to_vec();
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &truncated,
                sources: None,
            },
            &crate::infrastructure::config::RepairConfig::default()
                .patch_settings()
                .into_request(report.clone())
                .expect("default All gap selection"),
            &[],
            false,
            &clip_sync_repair_fixtures::NoOpProgressReporter,
        );
        let eq = corpus.gaps[0]
            .equivalence_diagnostic_verdict()
            .expect("diagnostic overlay still runs — the refusal is about the donor, not the gap");
        assert_eq!(
            eq.measurement.as_ref().and_then(|m| m.donor_span),
            None,
            "the provenance token must report the refusal, not the intent"
        );
        assert_eq!(
            eq.donor_silence_fraction, None,
            "nothing may be measured against samples B does not have"
        );
        assert!(
            !eq.class.drops(),
            "a refusal keeps the gap; dropping on absent audio is the defect this guards"
        );
    }

    /// Production from-decode dump omits unmeasured Option fields and leaves `not_measured` empty
    /// when the real `lag_decision` sweep was threaded through (no fabricated lag stand-ins).
    #[test]
    fn production_dump_omits_unmeasured_option_fields() {
        use crate::application::PatchAudioRequest;
        use crate::infrastructure::config::RepairConfig;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let (report, a_pcm, b) = equivalence_overlay_fixture();
        let repair = RepairConfig {
            fill_border_search_secs: 0.05,
            fill_align_margin_secs: 0.02,
            fill_length_slack_secs: 0.1,
            fill_extract_tail_slack_secs: 0.1,
            fill_seam_search_secs: 0.05,
            border_standoff_secs: 0.0,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            ..RepairConfig::default()
        };
        let request: PatchAudioRequest = repair
            .patch_settings()
            .into_request(report.clone())
            .expect("default All gap selection");
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &request,
            &[],
            false,
            &NoOpProgressReporter,
        );
        assert_eq!(
            corpus.source.not_measured, NOT_MEASURED_BY_PROJECTION,
            "the production dump must leave not_measured empty when it threads the real \
             `lag_decision` through (no `PROJECTED_LAG_DECISION_FIELDS`)"
        );
        let recipe = corpus
            .source
            .gate_recipe
            .as_ref()
            .expect("from-decode dump stamps the seam-gate recipe");
        assert_eq!(
            recipe.min_fill_correlation, request.min_fill_correlation,
            "gate_recipe must echo the settings used to score brackets"
        );
        assert_eq!(
            recipe.fill_absolute_floor, request.fill_absolute_floor,
            "gate_recipe must echo the settings used to score brackets"
        );
        assert_eq!(
            recipe.residual_headroom_margin_db,
            request.residual_headroom_margin_db
        );
        let full: Vec<_> = corpus
            .gaps
            .iter()
            .filter(|g| g.tier == DetailTier::Full)
            .collect();
        assert!(!full.is_empty(), "fixture must produce a measured gap");
        for g in full {
            assert!(g.levels.bin_ms.is_none());
            assert!(g.levels.profile_db.is_empty());
            assert!(g.levels.floor_db.is_none());
            assert!(g.levels.speech_peak_db.is_none());
            assert!(g.silence.is_none());
            assert!(g.contour.is_none());
            assert!(g.anchors.is_none());
            assert!(
                g.outcome.as_ref().is_some_and(|o| o.seam_shape.is_none()),
                "seam_shape must be omitted, not hardcoded empty"
            );
            // This path threads the real sweep through `MeasuredDetail`, so the rows must NOT carry
            // `projected_lag_entry`'s signature: a real sweep reports its search width, and `lag0_r`
            // is an independent read rather than a copy of `peak_r`. Guarding the shape here is what
            // keeps the conditional declaration below honest in the other direction — if the
            // pass-through ever regresses to projection, this fires before the declaration does.
            for e in g
                .lag_decision
                .iter()
                .flat_map(|bl| bl.pre_anchor.iter().chain(bl.post_anchor.iter()))
            {
                assert_ne!(e.max_lag_ms, 0, "real sweep reports its search width");
                assert_ne!(e.window_ms, 0, "real sweep reports its window");
            }
        }
        assert!(
            corpus.gaps.iter().any(|g| g.lag_decision.is_some()),
            "fixture must produce a lag_decision to make the assertions above non-vacuous"
        );
    }

    /// The conditional list must not overlap the unconditional one.
    ///
    /// If a `lag_decision.*` path appeared in both, the declaration would be emitted even on dumps
    /// that measured the sweep — claiming a field is unmeasured when it is real, which `--check`
    /// treats as worse than no declaration at all (it licenses discarding good data). The
    /// exact-equality assertion in the test above is what proves the from-decode dump omits them;
    /// this proves the constants can't make that impossible.
    #[test]
    fn conditional_lag_fields_stay_out_of_the_unconditional_list() {
        for f in PROJECTED_LAG_DECISION_FIELDS {
            assert!(
                !NOT_MEASURED_BY_PROJECTION.contains(f),
                "{f} is conditional; it must not sit in the unconditional list too"
            );
        }
    }

    /// Structure-floor scores land on `structure_*`; residual failures keep `residual_margin_db`.
    /// Never overload structure correlations onto `seam_*` (the pre-2026-08-03 dump bug).
    #[test]
    fn stage_of_keeps_structure_and_residual_channels_separate() {
        use crate::application::patch_region::SeamGateFailure;
        use crate::domain::policies::{SeamFloorProbe, SeamFloorSource, SeamResidualVerdict};

        let structure = stage_of(&SeamGateFailure::StructureBelowThreshold {
            pre: 0.41,
            post: 0.52,
        });
        assert_eq!(structure.stage, FailureStage::StructureFloor);
        assert_eq!(structure.structure_pre, Some(0.41));
        assert_eq!(structure.structure_post, Some(0.52));
        assert_eq!(structure.seam_pre, None);
        assert_eq!(structure.seam_post, None);
        assert_eq!(structure.residual_margin_db, None);

        let waveform = stage_of(&SeamGateFailure::WaveformBelowThreshold {
            pre: 0.11,
            post: 0.12,
            min: 0.12,
            best_attempt: None,
        });
        assert_eq!(waveform.stage, FailureStage::WaveformFloor);
        assert_eq!(waveform.structure_pre, None);
        assert_eq!(waveform.seam_pre, Some(0.11));
        assert_eq!(waveform.seam_post, Some(0.12));
        assert_eq!(waveform.residual_margin_db, None);

        let residual = SeamResidualVerdict::from_parts_with_placement(
            &SeamFloorProbe {
                residual_db: -10.0,
                source: SeamFloorSource::None,
                best_lag: 0,
                gain: 1.0,
            },
            &SeamFloorProbe {
                residual_db: -10.0,
                source: SeamFloorSource::None,
                best_lag: 0,
                gain: 1.0,
            },
            &SeamFloorProbe {
                residual_db: -40.0,
                source: SeamFloorSource::None,
                best_lag: 0,
                gain: 1.0,
            },
            &SeamFloorProbe {
                residual_db: -40.0,
                source: SeamFloorSource::None,
                best_lag: 0,
                gain: 1.0,
            },
            -15.0,
            0,
            0,
        );
        let residual_fail = stage_of(&SeamGateFailure::ResidualHeadroomExceeded {
            pre: 0.9,
            post: 0.91,
            residual,
            margin_db: 6.0,
        });
        assert_eq!(residual_fail.stage, FailureStage::Residual);
        assert_eq!(residual_fail.seam_pre, Some(0.9));
        assert_eq!(residual_fail.seam_post, Some(0.91));
        assert_eq!(residual_fail.structure_pre, None);
        assert_eq!(residual_fail.residual_margin_db, Some(6.0));
    }

    /// A gap the fingerprint cannot measure still carries scan's verdict.
    ///
    /// The head-gap shape: A silent from t=0 in a pair with a negative offset, so B's window maps before
    /// zero, `Gap::mapped_b_span` fails closed and the per-gap loop `continue`s. Scan does **not** fail
    /// here — `scan_gaps.rs` pushes a verdict unconditionally and yields `NotEvaluated`, a *stated*
    /// refusal to classify (⇒ keep). The copy that carries it into the dump used to sit at the bottom of
    /// that loop body, below the `continue`, so 27 of 829 gaps on the 39-pair v0.5.0 corpus reached
    /// `equivalence-calibration` with neither verdict and were dropped from the comparison population
    /// silently. Degrading a stated refusal into an absent key is the provenance plan's §1.1 defect.
    #[test]
    fn scan_verdict_survives_a_gap_the_fingerprint_cannot_map() {
        use crate::application::PatchAudioRequest;
        use crate::domain::gap::Gap;
        use crate::domain::gap_equivalence::{GapEquivalenceClass, GapEquivalenceVerdict};
        use crate::domain::{GapReport, ScanAlignment};
        use crate::infrastructure::config::RepairConfig;
        use clip_sync::MultiChannelPcm;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let rate = 48_000u32;
        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: 1,
            samples: vec![0f32; rate as usize * 2],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        let report = GapReport {
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            // The head gap: A from 0.0, B mapped negative by the pair's offset ⇒ `mapped_b_span` = None.
            gaps: vec![Gap {
                video_a_start_secs: 0.0,
                video_a_end_secs: 0.8,
                video_b_start_secs: Some(-0.4),
                video_b_end_secs: Some(0.4),
                b_has_energy: true,
            }],
            gap_equivalence: vec![GapEquivalenceVerdict {
                class: GapEquivalenceClass::NotEvaluated,
                ..Default::default()
            }],
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
        };
        let repair = RepairConfig::default();
        let request: PatchAudioRequest = repair
            .patch_settings()
            .into_request(report.clone())
            .expect("default All gap selection");
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &vec![0f32; rate as usize * 2],
                sources: None,
            },
            &request,
            &[],
            false,
            &NoOpProgressReporter,
        );
        let fp = &corpus.gaps[0];
        assert!(
            fp.equivalence_diagnostic.is_none(),
            "unmappable gap: the diagnostic path genuinely cannot measure it"
        );
        assert_eq!(
            fp.equivalence_production_verdict().map(|v| v.class),
            Some(GapEquivalenceClass::NotEvaluated),
            "scan's stated refusal must reach the dump; an absent key would read as 'never asked'"
        );
    }

    /// C1: both equivalence verdicts ship a co-located `_contract`, and the two carry **different**
    /// text — that per-field difference is the reason the wrapper exists rather than a field on the
    /// shared `GapEquivalenceVerdict`. Contracts are reader metadata: their presence must never
    /// change a class, so this asserts alongside, not instead of, the verdict itself.
    #[test]
    fn from_decode_dump_stamps_a_contract_on_both_equivalence_verdicts() {
        let (report, a_pcm, b) = equivalence_overlay_fixture();
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &crate::infrastructure::config::RepairConfig::default()
                .patch_settings()
                .into_request(report.clone())
                .expect("default All gap selection"),
            &[],
            false,
            &clip_sync_repair_fixtures::NoOpProgressReporter,
        );
        let fp = &corpus.gaps[0];

        let diag = fp
            .equivalence_diagnostic
            .as_ref()
            .expect("overlay ran")
            .contract
            .as_ref()
            .expect("diagnostic contract stamped on write");
        let prod = fp
            .equivalence_production
            .as_ref()
            .expect("scan verdict copied in")
            .contract
            .as_ref()
            .expect("production contract stamped on write");

        assert_ne!(
            diag, prod,
            "the pair shares one type; contract text must still distinguish them"
        );
        assert!(
            prod.not
                .as_deref()
                .is_some_and(|s| s.contains("equivalence_diagnostic")),
            "the authoritative verdict must name its confusable sibling: {prod:?}"
        );
        assert!(
            diag.not
                .as_deref()
                .is_some_and(|s| s.contains("equivalence_production")),
            "the diagnostic verdict must point at the authoritative one: {diag:?}"
        );

        // `_contract` sits beside the values, not around them (flatten), and does not displace them.
        let json = serde_json::to_value(fp).expect("serialize");
        assert!(json["equivalence_production"]["_contract"]["placement"].is_string());
        assert!(json["equivalence_production"]["class"].is_string());
    }

    /// The diagnostic equivalence overlay measures the **silent core**, taken off the
    /// index-parallel scan verdict, and says so in `measurement.a_span` / `donor_span`.
    ///
    /// This is the convergence half of the 2026-08-01 fix. The overlay used to window on
    /// `geometry.a_start_secs`/`a_end_secs` — the raw hold-bridged run — while scan windows on
    /// `SilentRun::core_*`; on the 39-pair v0.5.0 corpus the two disagreed on `a_gap_total_blocks` in
    /// 66.9 % of gaps, and the extra blocks (fade shoulders, non-silent) dragged `donor_silence_fraction`
    /// under the 0.5 threshold on 10 gaps, flipping scan's `shared_silence`/drop to the diagnostic's
    /// `repairable_dropout`/keep. Both verdicts printed `a_span: core` throughout, so the provenance
    /// field concealed the divergence it existed to expose.
    ///
    /// Asserted against the *same* fixture with an empty `gap_equivalence` (the fallback arm, pinned in
    /// `characterize_gaps_from_decode_include_diagnostics_toggles_x_set`), so the block counts move for
    /// one reason only: which interval was binned.
    #[test]
    fn diagnostic_equivalence_adopts_the_scan_verdicts_core_span() {
        use crate::application::PatchAudioRequest;
        use crate::domain::gap::Gap;
        use crate::domain::gap_equivalence::{GapEquivalenceClass, GapEquivalenceVerdict};
        use crate::domain::{GapReport, GapSignatureMode, ScanAlignment};
        use crate::infrastructure::config::RepairConfig;
        use clip_sync::MultiChannelPcm;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let rate = 48_000u32;
        let ch = 1usize;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let total = secs(5.0);
        let (sp1, n1, gap, n2, sp2) = (
            (secs(0.50), secs(0.85)),
            (secs(0.85), secs(1.85)),
            (secs(1.85), secs(3.35)),
            (secs(3.35), secs(4.35)),
            (secs(4.35), secs(4.70)),
        );
        let mut a = vec![0f32; total];
        let mut b = vec![0f32; total];
        write_speech(&mut a, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut b, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut a, sp2.0, sp2.1, 440.0, 0.079);
        write_speech(&mut b, sp2.0, sp2.1, 440.0, 0.079);
        write_noise(&mut a, n1.0, n1.1, 1, 0.0056);
        write_noise(&mut b, n1.0, n1.1, 11, 0.0056);
        write_noise(&mut a, n2.0, n2.1, 3, 0.0056);
        write_noise(&mut b, n2.0, n2.1, 13, 0.0056);
        write_noise(&mut b, gap.0, gap.1, 5, 0.0056);

        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: ch as u16,
            samples: a,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        let f = |x: usize| x as f64 / f64::from(rate);
        let report = |gap_equivalence: Vec<GapEquivalenceVerdict>| GapReport {
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            gaps: vec![Gap {
                video_a_start_secs: f(gap.0),
                video_a_end_secs: f(gap.1),
                video_b_start_secs: Some(f(gap.0)),
                video_b_end_secs: Some(f(gap.1)),
                b_has_energy: true,
            }],
            gap_equivalence,
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
        };
        // Same search-radius trimming as the C3 test, and for the same reason: the 5 s fixture is far
        // smaller than the production defaults assume.
        let repair = RepairConfig {
            gap_signature_mode: GapSignatureMode::Energy,
            gap_signature_context_secs: 1.5,
            fill_border_search_secs: 0.05,
            fill_align_margin_secs: 0.02,
            fill_length_slack_secs: 0.1,
            fill_extract_tail_slack_secs: 0.1,
            fill_seam_search_secs: 0.05,
            border_standoff_secs: 0.0,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            ..RepairConfig::default()
        };
        let run = |rep: &GapReport| {
            let request: PatchAudioRequest = repair
                .patch_settings()
                .into_request(rep.clone())
                .expect("default All gap selection");
            characterize_gaps_from_decode(
                rep,
                &CharacterizeAbPcm {
                    a_pcm: &a_pcm,
                    b_samples: &b,
                    sources: None,
                },
                &request,
                &[],
                false,
                &NoOpProgressReporter,
            )
            .gaps
            .remove(0)
            .equivalence_diagnostic
            .expect("diagnostic equivalence")
        };

        let nominal = run(&report(Vec::new()));
        assert_eq!(
            nominal.measurement.as_ref().map(|m| m.a_span),
            Some(SpanKind::Nominal)
        );

        // A core inset half a second at each end of the 1.5 s gap — the shape of the real residual
        // (shoulders trimmed), exaggerated so the block count has to move at 100 ms bins.
        let core = (f(gap.0) + 0.5, f(gap.1) - 0.5);
        let mut verdict = GapEquivalenceVerdict {
            class: GapEquivalenceClass::NotEvaluated,
            ..Default::default()
        };
        verdict.a_span_secs = Some(core);
        verdict.donor_span_secs = Some(core);
        let converged = run(&report(vec![verdict]));

        let m = converged.measurement.as_ref().expect("measurement");
        assert_eq!(
            (m.a_span, m.donor_span),
            (SpanKind::Core, Some(SpanKind::Core)),
            "adopting scan's span must be reported, not silently assumed"
        );
        assert!(
            converged.a_gap_total_blocks < nominal.a_gap_total_blocks,
            "the core is inset, so fewer A blocks fall in it: core {:?} vs nominal {:?}",
            converged.a_gap_total_blocks,
            nominal.a_gap_total_blocks
        );
        assert!(
            converged.donor_total_blocks < nominal.donor_total_blocks,
            "the donor must follow A onto the core, not stay on `b_mapped`: core {:?} vs nominal {:?}",
            converged.donor_total_blocks,
            nominal.donor_total_blocks
        );
    }

    /// **C3 — `fingerprint_diagnostics` gates the X-set:** off, the diagnostic-only fields
    /// (`seam_probe`, `wide_envelope`, `b_levels`) are absent; on, they're populated. Closes
    /// perf-plan `docs/dev/archive/TEMP-pipeline-perf-redesign-plan.md` §4.7 backlog item **C3** — the flag
    /// exists (`RepairConfig.fingerprint_diagnostics`, `characterize_gaps_from_decode`'s
    /// `include_diagnostics`) but had no regression test pinning what it actually gates.
    #[test]
    fn characterize_gaps_from_decode_include_diagnostics_toggles_x_set() {
        use crate::application::PatchAudioRequest;
        use crate::domain::gap::Gap;
        use crate::domain::{GapReport, GapSignatureMode, ScanAlignment};
        use crate::infrastructure::config::RepairConfig;
        use clip_sync::MultiChannelPcm;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let rate = 48_000u32;
        let ch = 1usize;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let total = secs(5.0);
        let (sp1, n1, gap, n2, sp2) = (
            (secs(0.50), secs(0.85)),
            (secs(0.85), secs(1.85)),
            (secs(1.85), secs(3.35)),
            (secs(3.35), secs(4.35)),
            (secs(4.35), secs(4.70)),
        );
        let mut a = vec![0f32; total];
        let mut b = vec![0f32; total];
        write_speech(&mut a, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut b, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut a, sp2.0, sp2.1, 440.0, 0.079);
        write_speech(&mut b, sp2.0, sp2.1, 440.0, 0.079);
        write_noise(&mut a, n1.0, n1.1, 1, 0.0056);
        write_noise(&mut b, n1.0, n1.1, 11, 0.0056);
        write_noise(&mut a, n2.0, n2.1, 3, 0.0056);
        write_noise(&mut b, n2.0, n2.1, 13, 0.0056);
        write_noise(&mut b, gap.0, gap.1, 5, 0.0056);

        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: ch as u16,
            samples: a,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };

        let report = GapReport {
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            gaps: vec![Gap {
                video_a_start_secs: gap.0 as f64 / f64::from(rate),
                video_a_end_secs: gap.1 as f64 / f64::from(rate),
                video_b_start_secs: Some(gap.0 as f64 / f64::from(rate)),
                video_b_end_secs: Some(gap.1 as f64 / f64::from(rate)),
                b_has_energy: true,
            }],
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
        };

        let repair = RepairConfig {
            gap_signature_mode: GapSignatureMode::Energy,
            gap_signature_context_secs: 1.5,
            // The 5 s synthetic fixture is far smaller than the production defaults assume; every
            // search radius (border slide, anchor bracket span) must stay inside the fixture's own
            // timeline or the unified fit search's O(radius * window) slide becomes a multi-minute
            // brute force over a handful of anchor-bracket combinations.
            fill_border_search_secs: 0.05,
            fill_align_margin_secs: 0.02,
            fill_length_slack_secs: 0.1,
            fill_extract_tail_slack_secs: 0.1,
            fill_seam_search_secs: 0.05,
            border_standoff_secs: 0.0,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            ..RepairConfig::default()
        };
        let request: PatchAudioRequest = repair
            .patch_settings()
            .into_request(report.clone())
            .expect("default All gap selection");
        let progress = NoOpProgressReporter;

        // Threading check: the descriptors must land on the matching side of `source`, and the two sides
        // must stay distinguishable — a swap or a copy would pass every `file_source` unit test.
        //
        // **Do not give the two sides different `native_channels` to strengthen this.** Equal counts are
        // the refuse gate's precondition (`channel_layout_mismatch`); making them differ turns this into
        // a refuse test — `gaps` empties and every assertion below stops exercising the from-decode path.
        // Per-side channel mapping is asserted where it is observable, in
        // `characterize_gaps_refuses_channel_layout_mismatch`. Here the sides are distinguished by
        // codec / bit_depth / native_sample_rate / bitrate instead.
        let sources = crate::application::AbSources {
            a: crate::application::SourceDescriptor {
                codec: "flac".into(),
                bit_depth: Some(clip_sync::BitDepth::Int24),
                native_sample_rate: rate,
                native_channels: ch as u16,
                bitrate_bps: None,
            },
            b: crate::application::SourceDescriptor {
                codec: "aac".into(),
                bit_depth: None,
                native_sample_rate: 44_100,
                native_channels: ch as u16,
                bitrate_bps: Some(192_000),
            },
        };
        let with_sources = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: Some(&sources),
            },
            &request,
            &[],
            false,
            &progress,
        );
        assert_eq!(with_sources.source.a_source.codec.as_deref(), Some("flac"));
        assert_eq!(with_sources.source.b_source.codec.as_deref(), Some("aac"));
        assert_eq!(
            with_sources.source.a_source.bit_depth.as_deref(),
            Some("s24")
        );
        assert_eq!(
            with_sources.source.b_source.native_channels,
            Some(ch as u16)
        );
        // The gate's precondition, asserted so the reason the counts match is explicit rather than
        // incidental — see the comment above before changing either side.
        assert_eq!(
            with_sources.source.a_source.native_channels,
            with_sources.source.b_source.native_channels
        );
        assert_eq!(with_sources.source.a_source.was_resampled(), Some(false));
        assert_eq!(with_sources.source.b_source.was_resampled(), Some(true));
        // Bitrate is the one per-side field that is *asymmetric* on the positive path, so it is the
        // strongest available side-mapping check here: a swap flips both of these.
        assert_eq!(with_sources.source.a_source.source_audio_bitrate_bps, None);
        assert_eq!(
            with_sources.source.b_source.source_audio_bitrate_bps,
            Some(192_000)
        );
        // Counterpart to the refuse path's B-uses-B's-layout rule: when the layouts agree, **both**
        // sides are described at the decoded/analysis layout (A's), because that is what was measured.
        // The two rules are deliberate and differ; asserting only the refuse half made this look like a
        // quirk of that path. See `refused_channel_mismatch_corpus`'s doc comment.
        assert_eq!(with_sources.source.a_source.channels, ch as u16);
        assert_eq!(with_sources.source.b_source.channels, ch as u16);
        assert_eq!(
            with_sources.source.a_source.sample_rate, with_sources.source.b_source.sample_rate,
            "both sides are described at the measurement rate (A's), not their native rates"
        );
        assert_eq!(with_sources.source.incomparable, None);
        assert!(
            !with_sources.gaps.is_empty(),
            "matched channel layout must still characterize"
        );
        // Track B fine-path attach: pin the recipe on the dumped verdict, not only on the builder.
        //
        // `report.gap_equivalence` is empty here, so this is the **fallback** arm: no scan verdict means
        // no exported silent core, and the path measures the raw span — `nominal` on both sides. Both
        // tokens said `core`/`nominal` unconditionally until 2026-08-01; `core` was false and this test
        // pinned the falsehood. The core arm is covered by
        // `diagnostic_equivalence_adopts_the_scan_verdicts_core_span`.
        {
            let equiv = with_sources.gaps[0]
                .equivalence_diagnostic
                .as_ref()
                .expect("diagnostic equivalence");
            let m = equiv
                .measurement
                .as_ref()
                .expect("measurement attached at caller");
            // Scan's window, not `repair.gap_signature_context_secs` — I2 closed by removal
            // 2026-08-01. Asserted against the constant so this fails if the call site drifts back
            // onto the config field while the token keeps saying otherwise.
            assert!(
                (m.context_secs - crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS).abs()
                    < f64::EPSILON,
                "the equivalence floor must be measured over scan's context window"
            );
            assert!(
                (repair.gap_signature_context_secs
                    - crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS)
                    .abs()
                    > f64::EPSILON,
                "fixture must keep the two values distinct or the assertion above is vacuous"
            );
            assert_eq!(m.bin_ms, report.recipe.scan_block_ms());
            assert_eq!(m.reduction, ChannelReduction::Interleaved);
            assert_eq!(
                (m.a_span, m.donor_span),
                (SpanKind::Nominal, Some(SpanKind::Nominal)),
                "no scan verdict ⇒ no core to adopt ⇒ both ends stay on the raw span"
            );
        }

        let off = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &request,
            &[],
            false,
            &progress,
        );
        // The descriptor-less call must leave provenance absent, not defaulted.
        assert_eq!(off.source.a_source.codec, None);
        assert_eq!(off.source.b_source.was_resampled(), None);
        let on = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &request,
            &[],
            true,
            &progress,
        );

        let fp_off = off.gaps.first().expect("one gap (off)");
        assert!(
            fp_off.seam_probe.is_none(),
            "diagnostics off: seam_probe must be absent"
        );
        assert!(
            fp_off.wide_envelope.is_none(),
            "diagnostics off: wide_envelope must be absent"
        );
        assert!(
            fp_off.b_levels.is_none(),
            "diagnostics off: b_levels must be absent"
        );

        let fp_on = on.gaps.first().expect("one gap (on)");
        assert!(
            fp_on.seam_probe.is_some(),
            "diagnostics on: seam_probe must be populated"
        );
        assert!(
            fp_on.wide_envelope.is_some(),
            "diagnostics on: wide_envelope must be populated"
        );
        assert!(
            fp_on.b_levels.is_some(),
            "diagnostics on: b_levels must be populated"
        );
    }

    /// Stereo A + mono B: refuse pairwise characterize rather than indexing B at A's channel count.
    #[test]
    fn characterize_gaps_refuses_channel_layout_mismatch() {
        use crate::application::PatchAudioRequest;
        use crate::domain::gap::Gap;
        use crate::domain::{GapReport, GapSignatureMode, ScanAlignment};
        use crate::infrastructure::config::RepairConfig;
        use clip_sync::MultiChannelPcm;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let rate = 48_000u32;
        let a_ch = 2u16;
        let b_ch = 1u16;
        // 1.0 s of frames on each side — duration must use each side's own layout.
        let a_frames = rate as usize;
        let b_frames = rate as usize;
        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: a_ch,
            samples: vec![0.01f32; a_frames * a_ch as usize],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        let b = vec![0.01f32; b_frames * b_ch as usize];
        let report = GapReport {
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            gaps: vec![Gap {
                video_a_start_secs: 0.25,
                video_a_end_secs: 0.50,
                video_b_start_secs: Some(0.25),
                video_b_end_secs: Some(0.50),
                b_has_energy: true,
            }],
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
        };
        let request: PatchAudioRequest = RepairConfig {
            gap_signature_mode: GapSignatureMode::Energy,
            ..RepairConfig::default()
        }
        .patch_settings()
        .into_request(report.clone())
        .expect("default All gap selection");

        let sources = crate::application::AbSources {
            a: crate::application::SourceDescriptor {
                codec: "flac".into(),
                bit_depth: Some(clip_sync::BitDepth::Int16),
                native_sample_rate: rate,
                native_channels: a_ch,
                bitrate_bps: None,
            },
            b: crate::application::SourceDescriptor {
                codec: "aac".into(),
                bit_depth: None,
                native_sample_rate: rate,
                native_channels: b_ch,
                bitrate_bps: Some(128_000),
            },
        };

        let pcm = CharacterizeAbPcm {
            a_pcm: &a_pcm,
            b_samples: &b,
            sources: Some(&sources),
        };
        let corpus = characterize_gaps_from_decode(
            &report,
            &pcm,
            &request,
            &[],
            false,
            &NoOpProgressReporter,
        );
        assert_eq!(
            corpus.source.incomparable,
            Some(IncomparableReason::ChannelLayoutMismatch)
        );
        assert!(
            corpus.gaps.is_empty(),
            "mismatch must not emit pairwise gap fingerprints"
        );
        assert_eq!(corpus.source.gap_count, 1);
        assert_eq!(corpus.source.a_source.native_channels, Some(a_ch));
        assert_eq!(corpus.source.b_source.native_channels, Some(b_ch));
        // Honest B layout: 1.0 s mono, not the false 0.5 s that A's channel count would invent.
        assert_eq!(corpus.source.b_source.channels, b_ch);
        assert!(
            (corpus.source.b_source.duration_secs - 1.0).abs() < 1e-9,
            "B duration must use B channels, got {}",
            corpus.source.b_source.duration_secs
        );
        assert_eq!(corpus.source.a_source.channels, a_ch);
        assert!(
            (corpus.source.a_source.duration_secs - 1.0).abs() < 1e-9,
            "A duration {}",
            corpus.source.a_source.duration_secs
        );

        // Direct characterize path must refuse the same way (defense in depth).
        let summary = characterize_gaps(&report, &pcm, &FingerprintConfig::default(), &[]);
        assert_eq!(
            summary.source.incomparable,
            Some(IncomparableReason::ChannelLayoutMismatch)
        );
        assert!(summary.gaps.is_empty());
    }

    fn mk_fp(index: usize, full: bool) -> GapFingerprint {
        GapFingerprint {
            index,
            tier: if full {
                DetailTier::Full
            } else {
                DetailTier::Summary
            },
            sample_rate: 48_000,
            channels: 2,
            geometry: GapGeometry {
                a_start_secs: 832.5,
                a_end_secs: 834.0,
                a_refined_start_secs: 832.3,
                a_refined_end_secs: 834.1,
                duration_secs: 1.8,
                b_mapped_start_secs: None,
                b_mapped_end_secs: None,
                fill_offset_secs: None,
            },
            levels: LevelProfile {
                bin_ms: Some(50),
                profile_db: vec![],
                floor_db: Some(-120.0),
                speech_peak_db: Some(-40.0),
                noise_floor_db: -53.0,
                gap_floor_db: -98.0,
            },
            silence: Some(SilenceProfile {
                collar_rms_peak_ratio: 0.1,
                collar_above_relative_floor: true,
                silence_peak_fraction: 0.01,
            }),
            contour: Some(ContourInfo {
                has_anchor_seam_contour: true,
                pre_flatness: 0.0,
                post_flatness: 0.0,
            }),
            anchors: Some(AnchorSet::default()),
            brackets: vec![],
            structure: None,
            seams: None,
            lag_editorial: full.then(|| LagFingerprint {
                pre_anchor: vec![LagSummary {
                    window_ms: 250,
                    max_lag_ms: 200,
                    channel: LagChannel::Mono,
                    lag0_r: 0.02,
                    peak_r: 0.99,
                    second_peak_r: Some(0.20),
                    peak_z: Some(12.5),
                    prominence: Some(0.79),
                    top2_spacing_ms: Some(40.0),
                    peak_lag_samples: -778,
                    frac_lag_samples: -778.0,
                    frac_lag_ms: -16.2,
                    edge_pinned: Some(false),
                    verdict: LagVerdict::TimingOffset,
                }],
                post_anchor: vec![],
            }),
            lag_decision: None,
            residual: None,
            seam_probe: None,
            donor_interior: None,
            splice: None,
            wide_envelope: None,
            splice_dualfit: None,
            donor_interior_nominal: None,
            b_levels: None,
            outcome: None,
            equivalence_diagnostic: None,
            equivalence_production: None,
        }
    }

    #[test]
    fn write_corpus_library_emits_named_files_and_manifest() {
        let corpus = GapCorpus {
            source: SourceMeta {
                a_source: FileSource {
                    id: "aaaaaaaaaaaaaaaa".into(),
                    container: None,
                    codec: None,
                    bit_depth: None,
                    native_sample_rate: None,
                    native_channels: None,
                    source_audio_bitrate_bps: None,
                    sample_rate: 48_000,
                    channels: 2,
                    duration_secs: 1000.0,
                },
                b_source: FileSource {
                    id: "bbbbbbbbbbbbbbbb".into(),
                    container: None,
                    codec: None,
                    bit_depth: None,
                    native_sample_rate: None,
                    native_channels: None,
                    source_audio_bitrate_bps: None,
                    sample_rate: 48_000,
                    channels: 2,
                    duration_secs: 1000.0,
                },
                scan_recipe: CorpusScanRecipe::default(),
                gap_count: 2,
                incomparable: None,
                gate_recipe: None,
                not_measured: Vec::new(),
            },
            gaps: vec![mk_fp(0, false), mk_fp(3, true)],
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let n = write_corpus_dir(&corpus, dir.path()).expect("write library");
        assert_eq!(n, 2);
        assert!(dir.path().join("manifest.json").exists());
        assert!(dir.path().join("corpus.json").exists());

        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|f| f.contains("g003_full_timing_offset")),
            "full gap named by verdict: {names:?}"
        );
        assert!(
            names.iter().any(|f| f.contains("g000_summary_na")),
            "summary gap named na: {names:?}"
        );
        // No leaking identifiers; per-gap files are opaque-id prefixed.
        assert!(names.iter().all(|f| f == "manifest.json"
            || f == "corpus.json"
            || f.starts_with("aaaaaaaa_bbbb_")));

        // Each per-gap file is a self-contained single-gap corpus.
        let full = names.iter().find(|f| f.contains("g003")).unwrap();
        let parsed: GapCorpus =
            serde_json::from_reader(std::fs::File::open(dir.path().join(full)).unwrap()).unwrap();
        assert_eq!(parsed.gaps.len(), 1);
        assert_eq!(parsed.gaps[0].index, 3);
        assert_eq!(parsed.source.a_source.id, "aaaaaaaaaaaaaaaa");
    }
}
