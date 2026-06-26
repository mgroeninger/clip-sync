//! Editorial seam anchor candidates on A (anchor-based seam placement, P0).

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use clip_sync::PcmCorrelator;
use crate::domain::gap_energy::energy_bins;
use crate::domain::gap_structure::{activity_bins, StructureMatchParams};
use crate::domain::policies::{is_silent_frame, RefinedGapFrames};

/// When to search for matchable editorial boundaries instead of the scan throat alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorSeamMode {
    #[default]
    Off,
    /// Run when baseline throat `min(pre, post)` is below the marginal floor and energy contour exists.
    Auto,
    /// Always run anchor bracket search after baseline fails the marginal short-circuit.
    Force,
}

impl FromStr for AnchorSeamMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(AnchorSeamMode::Off),
            "auto" => Ok(AnchorSeamMode::Auto),
            "force" => Ok(AnchorSeamMode::Force),
            _ => Err(format!(
                "invalid anchor seam mode: {s} (expected off, auto, or force)"
            )),
        }
    }
}

/// Which editorial boundary on A.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorSeamSide {
    /// Last kept sample before the B fill (`A_pre`).
    Pre,
    /// First kept sample after the B fill (`A_post`).
    Post,
}

/// How an anchor frame was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorSource {
    ScanRefined,
    EnergyPeak,
    BoolTransition,
}

/// One candidate anchor frame on A.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchorCandidate {
    pub frame: usize,
    pub side: AnchorSeamSide,
    pub source: AnchorSource,
    /// Peak − local median (energy) or 1.0 for scan fallback.
    pub prominence: f32,
    /// Gated log-RMS in the anchor bin (A-side).
    pub rms: f32,
}

/// Anchor candidates on both sides of a scan hole.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnchorCandidateSet {
    pub pre: Vec<AnchorCandidate>,
    pub post: Vec<AnchorCandidate>,
}

/// One feasible editorial bracket; anchors contain the scan hole (default policy).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchorBracket {
    pub refined: RefinedGapFrames,
    pub pre: AnchorCandidate,
    pub post: AnchorCandidate,
    /// Total frame movement from scan-refined baseline (ranking penalty input).
    pub move_frames: usize,
    /// Distance between scan-hole center and bracket center on A (decoy-peak guard).
    pub center_drift_frames: usize,
}

/// Default minimum anchor-window Pearson for B-side matchability (below structure gate).
pub const DEFAULT_ANCHOR_MATCH_MIN_PEARSON: f32 = 0.12;
/// Default GCC-PHAT peak when Pearson is ambiguous at an anchor.
pub const DEFAULT_ANCHOR_MATCH_MIN_XCORR_PEAK: f32 = 0.5;
/// Pearson band below [`AnchorMatchabilityParams::min_pearson`] that triggers optional xcorr.
pub const DEFAULT_ANCHOR_MATCH_XCORR_AMBIGUOUS_BAND: f32 = 0.15;

/// Thresholds for per-anchor B-side waveform matchability (fit seam gate).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchorMatchabilityParams {
    pub min_pearson: f32,
    pub min_xcorr_peak: f32,
    pub xcorr_ambiguous_band: f32,
}

impl Default for AnchorMatchabilityParams {
    fn default() -> Self {
        Self {
            min_pearson: DEFAULT_ANCHOR_MATCH_MIN_PEARSON,
            min_xcorr_peak: DEFAULT_ANCHOR_MATCH_MIN_XCORR_PEAK,
            xcorr_ambiguous_band: DEFAULT_ANCHOR_MATCH_XCORR_AMBIGUOUS_BAND,
        }
    }
}

impl AnchorMatchabilityParams {
    pub fn from_repair_fields(
        min_match_pearson: f32,
        min_xcorr_peak: f32,
        xcorr_ambiguous_band: f32,
    ) -> Self {
        Self {
            min_pearson: min_match_pearson,
            min_xcorr_peak,
            xcorr_ambiguous_band,
        }
    }
}

/// Parameters for anchor candidate generation on A.
#[derive(Debug, Clone, Copy)]
pub struct AnchorSeamParams {
    pub context_frames: usize,
    pub max_anchors_per_side: usize,
    pub max_bracket_frames: usize,
    pub min_prominence: f32,
    pub structure: StructureMatchParams,
}

/// Whether anchor seam search should run for this gap.
pub fn should_run_anchor_seam(
    mode: AnchorSeamMode,
    baseline_pre: f64,
    baseline_post: f64,
    min_fill_correlation: f32,
    fill_marginal_margin: f32,
    signature_has_contour: bool,
) -> bool {
    match mode {
        AnchorSeamMode::Off => false,
        AnchorSeamMode::Force => true,
        AnchorSeamMode::Auto => {
            let min_score = baseline_pre.min(baseline_post);
            let marginal_floor = f64::from(min_fill_correlation - fill_marginal_margin);
            min_score < marginal_floor && signature_has_contour
        }
    }
}

/// Collect ≤K anchor frames per side around a scan hole on A.
pub fn list_anchor_candidates_a(
    samples: &[f32],
    channels: usize,
    scan_hole: RefinedGapFrames,
    params: &AnchorSeamParams,
) -> AnchorCandidateSet {
    let channels = channels.max(1);
    let bin_frames = params.structure.bin_frames.max(1);
    let total_frames = samples.len() / channels;

    let (pre_start, pre_end, post_start, post_end) =
        gap_context_frame_ranges(scan_hole, params.context_frames, total_frames);

    let mut pre = vec![scan_fallback_candidate(
        scan_hole.start_frame,
        AnchorSeamSide::Pre,
    )];
    let mut post = vec![scan_fallback_candidate(
        scan_hole.end_frame,
        AnchorSeamSide::Post,
    )];

    let pre_bins = energy_bins(
        samples,
        channels,
        pre_start,
        pre_end,
        bin_frames,
        params.structure.silence_peak_fraction,
        params.structure.absolute_silence_rms,
    );
    let post_bins = energy_bins(
        samples,
        channels,
        post_start,
        post_end,
        bin_frames,
        params.structure.silence_peak_fraction,
        params.structure.absolute_silence_rms,
    );
    pre.extend(energy_peak_candidates(
        &pre_bins,
        &AnchorSideScanCtx {
            bin_frames,
            origin_frame: pre_start,
            side: AnchorSeamSide::Pre,
            scan_edge: scan_hole.start_frame,
            params,
            samples,
            channels,
        },
    ));
    post.extend(energy_peak_candidates(
        &post_bins,
        &AnchorSideScanCtx {
            bin_frames,
            origin_frame: post_start,
            side: AnchorSeamSide::Post,
            scan_edge: scan_hole.end_frame,
            params,
            samples,
            channels,
        },
    ));

    let pre_activity = activity_bins(
        samples,
        channels,
        pre_start,
        pre_end,
        bin_frames,
        params.structure.silence_peak_fraction,
        params.structure.absolute_silence_rms,
    );
    let post_activity = activity_bins(
        samples,
        channels,
        post_start,
        post_end,
        bin_frames,
        params.structure.silence_peak_fraction,
        params.structure.absolute_silence_rms,
    );
    pre.extend(bool_transition_candidates(
        &pre_activity,
        &AnchorSideScanCtx {
            bin_frames,
            origin_frame: pre_start,
            side: AnchorSeamSide::Pre,
            scan_edge: scan_hole.start_frame,
            params,
            samples,
            channels,
        },
    ));
    post.extend(bool_transition_candidates(
        &post_activity,
        &AnchorSideScanCtx {
            bin_frames,
            origin_frame: post_start,
            side: AnchorSeamSide::Post,
            scan_edge: scan_hole.end_frame,
            params,
            samples,
            channels,
        },
    ));

    AnchorCandidateSet {
        pre: finalize_side_candidates(
            pre,
            AnchorSeamSide::Pre,
            scan_hole.start_frame,
            params,
        ),
        post: finalize_side_candidates(
            post,
            AnchorSeamSide::Post,
            scan_hole.end_frame,
            params,
        ),
    }
}

/// Enumerate feasible `(pre, post)` brackets that contain the scan hole.
pub fn list_feasible_anchor_brackets(
    candidates: &AnchorCandidateSet,
    scan_hole: RefinedGapFrames,
    params: &AnchorSeamParams,
) -> Vec<AnchorBracket> {
    let mut brackets = Vec::new();
    for pre in &candidates.pre {
        for post in &candidates.post {
            let start = pre.frame;
            let end = post.frame;
            if start > scan_hole.start_frame || end < scan_hole.end_frame {
                continue;
            }
            if end <= start || end - start > params.max_bracket_frames {
                continue;
            }
            brackets.push(AnchorBracket {
                refined: RefinedGapFrames {
                    start_frame: start,
                    end_frame: end,
                },
                pre: *pre,
                post: *post,
                move_frames: scan_hole.start_frame.abs_diff(start)
                    + scan_hole.end_frame.abs_diff(end),
                center_drift_frames: {
                    let scan_center = (scan_hole.start_frame + scan_hole.end_frame) / 2;
                    let bracket_center = (start + end) / 2;
                    scan_center.abs_diff(bracket_center)
                },
            });
        }
    }
    brackets.sort_by(|a, b| {
        a.move_frames
            .cmp(&b.move_frames)
            .then_with(|| a.center_drift_frames.cmp(&b.center_drift_frames))
            .then_with(|| {
                b.pre
                    .prominence
                    .partial_cmp(&a.pre.prominence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                b.post
                    .prominence
                    .partial_cmp(&a.post.prominence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    brackets
}

/// P0: A-side only — bin adjacent to the anchor carries signal above the silence floor.
pub fn anchor_matchable_on_a(
    samples: &[f32],
    channels: usize,
    frame: usize,
    side: AnchorSeamSide,
    params: &AnchorSeamParams,
) -> bool {
    let bin_frames = params.structure.bin_frames.max(1);
    let (start, end) = match side {
        AnchorSeamSide::Pre => (frame.saturating_sub(bin_frames), frame),
        AnchorSeamSide::Post => (frame, frame.saturating_add(bin_frames)),
    };
    if start >= end {
        return false;
    }
    (start..end).any(|f| {
        !is_silent_frame(
            samples,
            channels,
            f,
            params.structure.silence_peak_fraction,
            params.structure.absolute_silence_rms,
        )
    })
}

pub(crate) fn gap_context_frame_ranges(
    scan_hole: RefinedGapFrames,
    context_frames: usize,
    total_frames: usize,
) -> (usize, usize, usize, usize) {
    let pre_start = scan_hole.start_frame.saturating_sub(context_frames);
    let pre_end = scan_hole.start_frame.min(total_frames);
    let post_start = scan_hole.end_frame.min(total_frames);
    let post_end = (scan_hole.end_frame + context_frames).min(total_frames);
    (pre_start, pre_end, post_start, post_end)
}

fn scan_fallback_candidate(frame: usize, side: AnchorSeamSide) -> AnchorCandidate {
    AnchorCandidate {
        frame,
        side,
        source: AnchorSource::ScanRefined,
        prominence: 1.0,
        rms: 0.0,
    }
}

fn bin_to_anchor_frame(
    bin_idx: usize,
    bin_frames: usize,
    origin_frame: usize,
    side: AnchorSeamSide,
    scan_edge: usize,
) -> usize {
    match side {
        AnchorSeamSide::Pre => origin_frame
            .saturating_add((bin_idx + 1) * bin_frames)
            .min(scan_edge),
        AnchorSeamSide::Post => origin_frame
            .saturating_add(bin_idx * bin_frames)
            .max(scan_edge),
    }
}

struct AnchorSideScanCtx<'a> {
    bin_frames: usize,
    origin_frame: usize,
    side: AnchorSeamSide,
    scan_edge: usize,
    params: &'a AnchorSeamParams,
    samples: &'a [f32],
    channels: usize,
}

fn energy_peak_candidates(bins: &[f32], ctx: &AnchorSideScanCtx<'_>) -> Vec<AnchorCandidate> {
    let mut out = Vec::new();
    if bins.len() < 3 {
        return out;
    }
    for i in 1..bins.len() - 1 {
        let v = bins[i];
        if v <= bins[i - 1] || v <= bins[i + 1] {
            continue;
        }
        let local_median = median3(bins[i - 1], v, bins[i + 1]);
        let prominence = v - local_median;
        if prominence < ctx.params.min_prominence {
            continue;
        }
        let frame = bin_to_anchor_frame(
            i,
            ctx.bin_frames,
            ctx.origin_frame,
            ctx.side,
            ctx.scan_edge,
        );
        if !anchor_matchable_on_a(ctx.samples, ctx.channels, frame, ctx.side, ctx.params) {
            continue;
        }
        out.push(AnchorCandidate {
            frame,
            side: ctx.side,
            source: AnchorSource::EnergyPeak,
            prominence,
            rms: v,
        });
    }
    out
}

fn bool_transition_candidates(
    activity: &[bool],
    ctx: &AnchorSideScanCtx<'_>,
) -> Vec<AnchorCandidate> {
    let mut out = Vec::new();
    if activity.is_empty() {
        return out;
    }
    for i in 1..activity.len() {
        let prev = activity[i - 1];
        let curr = activity[i];
        if prev == curr {
            continue;
        }
        let direction_ok = match ctx.side {
            AnchorSeamSide::Pre => !prev && curr,
            AnchorSeamSide::Post => prev && !curr,
        };
        if !direction_ok {
            continue;
        }
        let frame = match ctx.side {
            AnchorSeamSide::Pre => bin_to_anchor_frame(
                i,
                ctx.bin_frames,
                ctx.origin_frame,
                ctx.side,
                ctx.scan_edge,
            ),
            AnchorSeamSide::Post => ctx
                .origin_frame
                .saturating_add(i * ctx.bin_frames)
                .saturating_sub(1)
                .max(ctx.scan_edge),
        };
        if !anchor_matchable_on_a(ctx.samples, ctx.channels, frame, ctx.side, ctx.params) {
            continue;
        }
        out.push(AnchorCandidate {
            frame,
            side: ctx.side,
            source: AnchorSource::BoolTransition,
            prominence: 0.5,
            rms: if curr { 1.0 } else { 0.5 },
        });
    }
    out
}

fn finalize_side_candidates(
    candidates: Vec<AnchorCandidate>,
    side: AnchorSeamSide,
    scan_edge: usize,
    params: &AnchorSeamParams,
) -> Vec<AnchorCandidate> {
    let bin_frames = params.structure.bin_frames.max(1);
    let scan_fallback = scan_fallback_candidate(scan_edge, side);

    let mut others: Vec<AnchorCandidate> = candidates
        .into_iter()
        .filter(|c| c.side == side && c.source != AnchorSource::ScanRefined)
        .collect();
    others.sort_by(|a, b| {
        b.prominence
            .partial_cmp(&a.prominence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.frame.cmp(&b.frame))
    });

    let mut out = Vec::new();
    let mut seen_bins = std::collections::BTreeSet::new();
    for candidate in others {
        let bin = candidate.frame / bin_frames;
        if !seen_bins.insert(bin) {
            continue;
        }
        out.push(candidate);
        if out.len() >= params.max_anchors_per_side.saturating_sub(1) {
            break;
        }
    }
    out.push(scan_fallback);
    out
}

fn median3(a: f32, b: f32, c: f32) -> f32 {
    let mut v = [a, b, c];
    v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    v[1]
}

/// B-side matchability at one anchor after unified placement is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchorMatchability {
    pub pearson: f64,
    pub xcorr_peak: Option<f64>,
    pub matchable: bool,
}

/// Inputs for [`matchability_at_anchor`].
#[derive(Clone, Copy)]
pub struct MatchabilityAtAnchorArgs<'a> {
    pub templates: &'a crate::domain::policies::SeamTemplates<'a>,
    pub placement: crate::domain::policies::SeamPlacement,
    pub side: AnchorSeamSide,
    pub pre_window: usize,
    pub post_window: usize,
    pub params: &'a AnchorMatchabilityParams,
    pub correlator: Option<&'a clip_sync::FftCorrelator>,
    pub max_lag_frames: i32,
}

/// Score waveform Pearson at one anchor; optional local xcorr when Pearson is ambiguous.
pub fn matchability_at_anchor(args: &MatchabilityAtAnchorArgs<'_>) -> AnchorMatchability {
    use crate::domain::policies::fill_seam_correlations;

    let MatchabilityAtAnchorArgs {
        templates,
        placement,
        side,
        pre_window,
        post_window,
        params,
        correlator,
        max_lag_frames,
    } = *args;

    let mut placement = placement;
    match side {
        AnchorSeamSide::Pre => placement.pre_window = pre_window,
        AnchorSeamSide::Post => placement.post_window = post_window,
    };
    let (pre, post) = fill_seam_correlations(templates, placement);
    let pearson = match side {
        AnchorSeamSide::Pre => pre,
        AnchorSeamSide::Post => post,
    };
    let min_pearson = f64::from(params.min_pearson);
    let ambiguous = pearson.is_finite()
        && pearson < min_pearson
        && pearson >= min_pearson - f64::from(params.xcorr_ambiguous_band);
    let xcorr_peak = if ambiguous {
        correlator.and_then(|c| {
            local_anchor_xcorr_peak(c, templates, placement, side, pre_window, post_window, max_lag_frames)
        })
    } else {
        None
    };
    let min_xcorr = f64::from(params.min_xcorr_peak);
    let matchable = pearson.is_finite()
        && (pearson >= min_pearson || xcorr_peak.is_some_and(|p| p >= min_xcorr));
    AnchorMatchability {
        pearson,
        xcorr_peak,
        matchable,
    }
}

/// Both anchor windows must pass independent waveform matchability on B.
pub fn anchor_bracket_both_matchable(
    templates: &crate::domain::policies::SeamTemplates<'_>,
    placement: crate::domain::policies::SeamPlacement,
    pre_window: usize,
    post_window: usize,
    params: &AnchorMatchabilityParams,
    correlator: Option<&clip_sync::FftCorrelator>,
    max_lag_frames: i32,
) -> bool {
    let pre = matchability_at_anchor(&MatchabilityAtAnchorArgs {
        templates,
        placement,
        side: AnchorSeamSide::Pre,
        pre_window,
        post_window,
        params,
        correlator,
        max_lag_frames,
    });
    let post = matchability_at_anchor(&MatchabilityAtAnchorArgs {
        templates,
        placement,
        side: AnchorSeamSide::Post,
        pre_window,
        post_window,
        params,
        correlator,
        max_lag_frames,
    });
    pre.matchable && post.matchable
}

/// Local GCC-PHAT peak at an anchor window (Tier 2).
pub fn local_anchor_xcorr_peak(
    correlator: &clip_sync::FftCorrelator,
    templates: &crate::domain::policies::SeamTemplates<'_>,
    placement: crate::domain::policies::SeamPlacement,
    side: AnchorSeamSide,
    pre_window: usize,
    post_window: usize,
    max_lag_frames: i32,
) -> Option<f64> {
    let (a_win, b_win) = anchor_windows(templates, placement, side, pre_window, post_window)?;
    if a_win.is_empty() || a_win.len() != b_win.len() {
        return None;
    }
    let mut best = None;
    let max_lag = max_lag_frames.max(0) as usize;
    for lag in 0..=max_lag {
        if lag >= b_win.len() {
            break;
        }
        let len = a_win.len().min(b_win.len() - lag);
        if len < 8 {
            continue;
        }
        let mag = correlator.segment_similarity(&a_win[..len], &b_win[lag..lag + len]);
        if mag.is_finite() && best.is_none_or(|(_, bmag)| mag > bmag) {
            best = Some((lag, mag));
        }
    }
    best.map(|(_, mag)| mag)
}

fn anchor_windows(
    templates: &crate::domain::policies::SeamTemplates<'_>,
    placement: crate::domain::policies::SeamPlacement,
    side: AnchorSeamSide,
    pre_window: usize,
    post_window: usize,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let gap_frames = placement.gap_frames;
    let start = placement.start;
    match side {
        AnchorSeamSide::Pre => {
            let w = pre_window.min(templates.a_pre.len());
            if w == 0 || start < w {
                return None;
            }
            Some((
                templates.a_pre[templates.a_pre.len() - w..].to_vec(),
                templates.b_mono[start - w..start].to_vec(),
            ))
        }
        AnchorSeamSide::Post => {
            let w = post_window.min(templates.a_post.len());
            let b_end = start + gap_frames + w;
            if w == 0 || b_end > templates.b_mono.len() {
                return None;
            }
            Some((
                templates.a_post[..w].to_vec(),
                templates.b_mono[start + gap_frames..b_end].to_vec(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_params() -> AnchorSeamParams {
        AnchorSeamParams {
            context_frames: 200,
            max_anchors_per_side: 5,
            max_bracket_frames: 500,
            min_prominence: 0.0,
            structure: StructureMatchParams {
                gap_frames: 50,
                bin_frames: 10,
                search_radius_frames: 50,
                fill_length_slack_frames: 5,
                max_fine_adjustment_frames: 2,
                silence_peak_fraction: 0.01,
                absolute_silence_rms: 0.0,
            },
        }
    }

    fn write_active(samples: &mut [f32], start: usize, end: usize, amp: f32) {
        for s in &mut samples[start..end] {
            *s = amp;
        }
    }

    #[test]
    fn flat_room_tone_only_scan_fallback() {
        let samples = vec![0.0f32; 600];
        let scan = RefinedGapFrames {
            start_frame: 300,
            end_frame: 350,
        };
        let set = list_anchor_candidates_a(&samples, 1, scan, &test_params());
        assert_eq!(set.pre.len(), 1);
        assert_eq!(set.post.len(), 1);
        assert_eq!(set.pre[0].source, AnchorSource::ScanRefined);
        assert_eq!(set.post[0].source, AnchorSource::ScanRefined);
    }

    #[test]
    fn speech_peak_before_throat_differs_from_scan_start() {
        let mut samples = vec![0.0f32; 600];
        // One full energy bin (bin_frames=10) louder than silent neighbors.
        write_active(&mut samples, 240, 250, 0.9);
        let scan = RefinedGapFrames {
            start_frame: 300,
            end_frame: 350,
        };
        let set = list_anchor_candidates_a(&samples, 1, scan, &test_params());
        assert!(
            set.pre
                .iter()
                .any(|c| c.source == AnchorSource::EnergyPeak && c.frame < scan.start_frame),
            "expected energy peak pre-anchor before throat: {:?}",
            set.pre
        );
    }

    #[test]
    fn bool_onset_near_gap_produces_transition_candidate() {
        let mut samples = vec![0.0f32; 600];
        write_active(&mut samples, 280, 295, 0.6);
        let scan = RefinedGapFrames {
            start_frame: 300,
            end_frame: 350,
        };
        let set = list_anchor_candidates_a(&samples, 1, scan, &test_params());
        assert!(
            set.pre.iter().any(|c| c.source == AnchorSource::BoolTransition),
            "expected bool transition: {:?}",
            set.pre
        );
    }

    #[test]
    fn bool_transition_direction_pre_rising_post_falling_only() {
        let scan = RefinedGapFrames {
            start_frame: 300,
            end_frame: 350,
        };
        // Wrong pre: only falling edge (continuous active then silent before gap).
        let mut falling_pre = vec![0.0f32; 800];
        write_active(&mut falling_pre, 0, 290, 0.6);
        let set = list_anchor_candidates_a(&falling_pre, 1, scan, &test_params());
        assert!(
            !set.pre.iter().any(|c| c.source == AnchorSource::BoolTransition),
            "falling pre edge must not produce bool transition: {:?}",
            set.pre
        );

        // Wrong post: only rising edge (silent after gap then active).
        let mut rising_post = vec![0.0f32; 800];
        write_active(&mut rising_post, 400, 700, 0.6);
        let set = list_anchor_candidates_a(&rising_post, 1, scan, &test_params());
        assert!(
            !set.post.iter().any(|c| c.source == AnchorSource::BoolTransition),
            "rising post edge must not produce bool transition: {:?}",
            set.post
        );

        // Correct directions: rising pre, falling post.
        let mut samples = vec![0.0f32; 800];
        write_active(&mut samples, 280, 299, 0.6);
        write_active(&mut samples, 350, 390, 0.6);
        let set = list_anchor_candidates_a(&samples, 1, scan, &test_params());
        assert!(
            set.pre.iter().any(|c| c.source == AnchorSource::BoolTransition),
            "expected rising pre transition: {:?}",
            set.pre
        );
        assert!(
            set.post.iter().any(|c| c.source == AnchorSource::BoolTransition),
            "expected falling post transition: {:?}",
            set.post
        );
    }

    #[test]
    fn matchability_requires_anchor_pearson_not_structure_recycle() {
        let params = AnchorMatchabilityParams {
            min_pearson: 0.12,
            ..AnchorMatchabilityParams::default()
        };
        let a_pre = vec![1.0, 1.0, 1.0, 1.0];
        let a_post = vec![1.0, 1.0, 1.0, 1.0];
        let b_mono = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let templates = crate::domain::policies::SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &[vec![1.0; 4]],
            a_post_ch: &[vec![1.0; 4]],
            b_mono: &b_mono,
            b_ch: std::slice::from_ref(&b_mono),
        };
        let placement = crate::domain::policies::SeamPlacement {
            start: 4,
            gap_frames: 2,
            pre_window: 4,
            post_window: 4,
        };
        let pre = matchability_at_anchor(&MatchabilityAtAnchorArgs {
            templates: &templates,
            placement,
            side: AnchorSeamSide::Pre,
            pre_window: 4,
            post_window: 4,
            params: &params,
            correlator: None,
            max_lag_frames: 0,
        });
        assert!(
            !pre.matchable,
            "low anchor Pearson must fail even when structure scores would pass"
        );
        assert!(pre.pearson < 0.12);
    }

    #[test]
    fn brackets_must_contain_scan_hole() {
        let scan = RefinedGapFrames {
            start_frame: 300,
            end_frame: 350,
        };
        let candidates = AnchorCandidateSet {
            pre: vec![AnchorCandidate {
                frame: 310,
                side: AnchorSeamSide::Pre,
                source: AnchorSource::ScanRefined,
                prominence: 1.0,
                rms: 0.0,
            }],
            post: vec![scan_fallback_candidate(350, AnchorSeamSide::Post)],
        };
        assert!(list_feasible_anchor_brackets(&candidates, scan, &test_params()).is_empty());
    }

    #[test]
    fn brackets_reject_span_over_max() {
        let scan = RefinedGapFrames {
            start_frame: 100,
            end_frame: 120,
        };
        let candidates = AnchorCandidateSet {
            pre: vec![scan_fallback_candidate(100, AnchorSeamSide::Pre)],
            post: vec![AnchorCandidate {
                frame: 700,
                side: AnchorSeamSide::Post,
                source: AnchorSource::EnergyPeak,
                prominence: 1.0,
                rms: 1.0,
            }],
        };
        assert!(list_feasible_anchor_brackets(&candidates, scan, &test_params()).is_empty());
    }

    #[test]
    fn should_run_auto_only_when_below_marginal_and_contour() {
        assert!(!should_run_anchor_seam(
            AnchorSeamMode::Off,
            0.0,
            0.0,
            0.35,
            0.08,
            true,
        ));
        assert!(should_run_anchor_seam(
            AnchorSeamMode::Force,
            0.5,
            0.5,
            0.35,
            0.08,
            false,
        ));
        assert!(should_run_anchor_seam(
            AnchorSeamMode::Auto,
            0.05,
            0.04,
            0.35,
            0.08,
            true,
        ));
        assert!(!should_run_anchor_seam(
            AnchorSeamMode::Auto,
            0.30,
            0.28,
            0.35,
            0.08,
            true,
        ));
        assert!(!should_run_anchor_seam(
            AnchorSeamMode::Auto,
            0.05,
            0.04,
            0.35,
            0.08,
            false,
        ));
    }

    fn test_tone(len: usize, freq_hz: f64, rate_hz: f64) -> Vec<f64> {
        use std::f64::consts::TAU;
        (0..len)
            .map(|i| (TAU * freq_hz * i as f64 / rate_hz).sin())
            .collect()
    }

    /// Pre window: Pearson fails at lag 0 but GCC-PHAT peak at the true lag clears Tier 2.
    fn misaligned_pre_b_window(a_ref: &[f64], lag: usize) -> Vec<f64> {
        let w = a_ref.len();
        let mut b = vec![0.0; w];
        b[lag..w].copy_from_slice(&a_ref[..w - lag]);
        for (i, sample) in b[..lag].iter_mut().enumerate() {
            *sample = ((i as f64) * 0.37).sin() * 0.05;
        }
        b
    }

    fn xcorr_rescue_templates(
        pre_window: usize,
        lag: usize,
    ) -> (
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        crate::domain::policies::SeamPlacement,
    ) {
        let gap_frames = 10;
        let start = pre_window;
        let a_pre = test_tone(pre_window, 440.0, 8_000.0);
        let a_post = test_tone(pre_window, 440.0, 8_000.0);
        let b_pre = misaligned_pre_b_window(&a_pre, lag);
        let mut b_mono = vec![0.0; start + gap_frames + pre_window];
        b_mono[..pre_window].copy_from_slice(&b_pre);
        b_mono[start + gap_frames..].copy_from_slice(&a_post);
        let placement = crate::domain::policies::SeamPlacement {
            start,
            gap_frames,
            pre_window,
            post_window: pre_window,
        };
        (a_pre, a_post, b_mono, placement)
    }

    #[test]
    fn local_anchor_xcorr_peak_finds_lag_alignment() {
        let correlator = clip_sync::FftCorrelator;
        let lag = 50;
        let w = 256;
        let (a_pre, a_post, b_mono, placement) = xcorr_rescue_templates(w, lag);
        let templates = crate::domain::policies::SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &[a_pre.clone()],
            a_post_ch: &[a_post.clone()],
            b_mono: &b_mono,
            b_ch: std::slice::from_ref(&b_mono),
        };
        let peak = local_anchor_xcorr_peak(
            &correlator,
            &templates,
            placement,
            AnchorSeamSide::Pre,
            w,
            w,
            lag as i32 + 5,
        )
        .expect("xcorr peak");
        assert!(
            peak >= f64::from(DEFAULT_ANCHOR_MATCH_MIN_XCORR_PEAK),
            "expected strong GCC-PHAT peak at lag {lag}, got {peak}"
        );
    }

    #[test]
    fn xcorr_rescues_ambiguous_pearson_pre_anchor() {
        let correlator = clip_sync::FftCorrelator;
        let lag = 50;
        let w = 256;
        let (a_pre, a_post, b_mono, placement) = xcorr_rescue_templates(w, lag);
        let templates = crate::domain::policies::SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &[a_pre.clone()],
            a_post_ch: &[a_post.clone()],
            b_mono: &b_mono,
            b_ch: std::slice::from_ref(&b_mono),
        };
        let params = AnchorMatchabilityParams::default();
        let without = matchability_at_anchor(&MatchabilityAtAnchorArgs {
            templates: &templates,
            placement,
            side: AnchorSeamSide::Pre,
            pre_window: w,
            post_window: w,
            params: &params,
            correlator: None,
            max_lag_frames: 0,
        });
        assert!(
            !without.matchable,
            "misaligned pre Pearson must fail without xcorr: pearson={}",
            without.pearson
        );
        assert!(
            without.pearson < f64::from(params.min_pearson),
            "pearson must sit in ambiguous band trigger range"
        );

        let with = matchability_at_anchor(&MatchabilityAtAnchorArgs {
            templates: &templates,
            placement,
            side: AnchorSeamSide::Pre,
            pre_window: w,
            post_window: w,
            params: &params,
            correlator: Some(&correlator),
            max_lag_frames: lag as i32 + 5,
        });
        assert!(
            with.matchable,
            "xcorr should rescue ambiguous pre anchor: pearson={} xcorr={:?}",
            with.pearson,
            with.xcorr_peak
        );
        assert!(with.xcorr_peak.is_some_and(|p| p >= f64::from(params.min_xcorr_peak)));
    }

    #[test]
    fn xcorr_not_run_when_pearson_deep_fail() {
        let correlator = clip_sync::FftCorrelator;
        let w = 256;
        let a_pre = test_tone(w, 440.0, 8_000.0);
        let a_post = a_pre.clone();
        let mut b_pre = a_pre.clone();
        for sample in &mut b_pre {
            *sample = -*sample;
        }
        let start = w;
        let gap_frames = 10;
        let mut b_mono = vec![0.0; start + gap_frames + w];
        b_mono[..w].copy_from_slice(&b_pre);
        b_mono[start + gap_frames..].copy_from_slice(&a_post);
        let templates = crate::domain::policies::SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &[a_pre.clone()],
            a_post_ch: &[a_post.clone()],
            b_mono: &b_mono,
            b_ch: std::slice::from_ref(&b_mono),
        };
        let placement = crate::domain::policies::SeamPlacement {
            start,
            gap_frames,
            pre_window: w,
            post_window: w,
        };
        let params = AnchorMatchabilityParams::default();
        let result = matchability_at_anchor(&MatchabilityAtAnchorArgs {
            templates: &templates,
            placement,
            side: AnchorSeamSide::Pre,
            pre_window: w,
            post_window: w,
            params: &params,
            correlator: Some(&correlator),
            max_lag_frames: 50,
        });
        assert!(!result.matchable);
        assert!(result.xcorr_peak.is_none());
        assert!(
            result.pearson < f64::from(params.min_pearson - params.xcorr_ambiguous_band),
            "inverted tone must fall outside ambiguous band, pearson={}",
            result.pearson
        );
    }
}
