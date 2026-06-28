//! Real-content anchor-prominence calibration (plan §8 Q1 follow-up).
//!
//! Measures the energy-peak **prominence distribution** of editorial anchor candidates on real
//! audio, so a sensible production default for `anchor_seam_min_prominence` (today `0.0`) can be
//! chosen with evidence rather than from the synthetic noise-collar fixture. Prominence is
//! `peak − local_median` in energy-bin (`ln(1+rms)`) units; the scan-fallback sentinel (`1.0`) is
//! excluded so only true energy peaks are counted.

use clip_sync_repair::domain::gap_anchor_seam::{
    list_anchor_candidates_a, AnchorSeamParams, AnchorSource,
};
use clip_sync_repair::domain::gap_structure::StructureMatchParams;
use clip_sync_repair::domain::policies::RefinedGapFrames;
use clip_sync_repair::infrastructure::config::RepairConfig;

/// Energy-peak anchor prominences (pre + post, scan-fallback excluded) for one injected gap on real
/// mono PCM. `min_prominence` is forced to 0 so the *full* distribution is returned — the caller
/// decides where the floor should sit.
pub fn measure_anchor_prominences(
    mono: &[f32],
    sample_rate: u32,
    gap_anchor_secs: f64,
    gap_secs: f64,
) -> Vec<f32> {
    let cfg = RepairConfig::default();
    let rate = f64::from(sample_rate);
    let total = mono.len();
    if total == 0 {
        return Vec::new();
    }
    let gap_start = ((gap_anchor_secs * rate) as usize).min(total.saturating_sub(1));
    let gap_end = (((gap_anchor_secs + gap_secs) * rate) as usize).min(total);
    let scan_hole = RefinedGapFrames {
        start_frame: gap_start,
        end_frame: gap_end.max(gap_start + 1),
    };

    let bin_frames = ((cfg.gap_signature_bin_ms as f64 / 1000.0) * rate).round() as usize;
    let context_frames = (cfg.gap_signature_context_secs * rate).round() as usize;
    let structure = StructureMatchParams {
        gap_frames: scan_hole.end_frame - scan_hole.start_frame,
        bin_frames: bin_frames.max(1),
        search_radius_frames: 0,
        fill_length_slack_frames: 0,
        max_fine_adjustment_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_silence_rms: cfg.absolute_silence_rms,
    };
    let params = AnchorSeamParams {
        context_frames,
        max_anchors_per_side: 100_000, // capture the full distribution (production caps at ~5)
        max_bracket_frames: total,
        min_prominence: 0.0, // measure everything; the floor is the thing being calibrated
        structure,
    };

    let set = list_anchor_candidates_a(mono, 1, scan_hole, &params);
    set.pre
        .iter()
        .chain(set.post.iter())
        .filter(|c| c.source == AnchorSource::EnergyPeak)
        .map(|c| c.prominence)
        .collect()
}

/// Summary of a prominence distribution.
#[derive(Debug, Clone)]
pub struct ProminenceStats {
    pub count: usize,
    pub min: f32,
    pub median: f32,
    pub p90: f32,
    pub p99: f32,
    pub max: f32,
}

impl ProminenceStats {
    pub fn from_values(mut values: Vec<f32>) -> Option<Self> {
        if values.is_empty() {
            return None;
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let pct = |p: f64| values[((values.len() - 1) as f64 * p).round() as usize];
        Some(ProminenceStats {
            count: values.len(),
            min: values[0],
            median: pct(0.5),
            p90: pct(0.9),
            p99: pct(0.99),
            max: values[values.len() - 1],
        })
    }
}

/// Fraction of values `>= theta` — i.e. the share of real anchor candidates a floor would keep.
pub fn fraction_kept(values: &[f32], theta: f32) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().filter(|&&v| v >= theta).count() as f64 / values.len() as f64
}
