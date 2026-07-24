//! Mirror per-gap geometry from `application/patch_audio.rs` for fixture diagnostics.

use clip_sync_repair::domain::align::ScanAlignment;

use clip_sync_repair::domain::fill_offset::{resolve_gap_offset_secs, AnchoredRetryPass, FillOffsetMode};
use clip_sync_repair::domain::gap_fill_fit::{match_gap_fill_unified_in_b, UnifiedFillSearchInput, UnifiedFitWeights};
use clip_sync_repair::domain::gap_signature::{build_gap_signature, GapSignatureMode};
use clip_sync_repair::domain::gap_structure::{self, StructureMatchParams};
use clip_sync_repair::domain::repair_profile::gap_extension_slack_secs;
use clip_sync_repair::domain::pcm::{interleaved_to_channels, interleaved_to_mono};
use clip_sync_repair::domain::policies::{
    self, border_templates_for_gap, border_templates_per_channel_for_gap, GapBorderSpec,
    RefinedGapFrames, SeamTemplates,
};
use clip_sync_repair::domain::gap_fill_fit::WaveformSeamContext;

use super::energy_signature_fixtures::EnergySignatureFixture;

/// Matches `GAP_EDGE_REFINE_SECS` in `application/patch_audio.rs`.
const GAP_EDGE_REFINE_SECS: f64 = 0.75;

/// Subset of `PatchAudioRequest` / `PatchTestOptions` that affects haystack geometry.
#[derive(Debug, Clone)]
pub struct PatchGeometryParams {
    pub fill_border_search_secs: f64,
    pub fill_align_margin_secs: f64,
    pub gap_signature_context_secs: f64,
    pub fill_length_slack_secs: f64,
    pub gap_end_extend_max_ms: u64,
    pub gap_end_extend_on_post_seam_fail: bool,
    pub gap_start_extend_on_pre_seam_fail: bool,
    pub fit_boundary_search: clip_sync_repair::domain::FitBoundarySearch,
    pub fill_offset_mode: FillOffsetMode,
    pub fill_mode_fit: bool,
    pub gap_signature_bin_ms: u32,
}

/// Computed placement windows for one gap (domain vs patch-path comparison).
#[derive(Debug, Clone)]
pub struct PatchGeometryPreview {
    pub fixture_gap_start: usize,
    pub fixture_gap_end: usize,
    pub fixture_true_fill_start: usize,
    pub fixture_nominal_fill_start: usize,
    pub shift_frames: i64,
    pub report_a_start_secs: f64,
    pub report_a_end_secs: f64,
    pub report_b_start_secs: f64,
    pub report_b_end_secs: f64,
    pub gap_offset_secs: f64,
    pub gap_offset_from_alignment: bool,
    pub reported_start_frame: usize,
    pub reported_end_frame: usize,
    pub refined: RefinedGapFrames,
    pub fixture_gap_frames: usize,
    pub refined_b_start_secs: f64,
    pub refined_b_end_secs: f64,
    pub b_extract_start_secs: f64,
    pub b_extract_end_secs: f64,
    pub search_radius_frames: usize,
    pub context_frames: usize,
    pub offset_nominal_start: usize,
    pub gap_end_in_haystack: usize,
    pub haystack_frames: usize,
    pub true_fill_in_haystack: usize,
    pub true_fill_in_full_b: usize,
    pub true_within_haystack: bool,
    pub true_within_search_radius: bool,
    pub patch_structure_params: StructureMatchParams,
}

impl PatchGeometryPreview {
    /// Run unified structure search on the same B haystack slice the patch path would decode.
    pub fn unified_match_on_haystack(
        &self,
        fixture: &EnergySignatureFixture,
        mode: GapSignatureMode,
        weights: UnifiedFitWeights,
    ) -> Option<clip_sync_repair::domain::gap_fill_fit::UnifiedFillMatch> {
        let haystack = slice_b_interleaved(
            &fixture.b_samples,
            fixture.channels,
            fixture.sample_rate,
            self.b_extract_start_secs,
            self.b_extract_end_secs,
        );
        if haystack.is_empty() {
            return None;
        }

        let ch = fixture.channels.max(1);
        let gap_frames = refined_gap_frames(&self.refined);
        let border_frames = self.patch_structure_params.bin_frames * 3;
        let border_spec = GapBorderSpec {
            gap_start_frame: self.refined.start_frame,
            gap_end_frame: self.refined.end_frame,
            border_frames,
            border_standoff_frames: 0,
            silence_peak_fraction: self.patch_structure_params.silence_peak_fraction,
            absolute_rms_floor: self.patch_structure_params.absolute_silence_rms,
        };
        let (a_pre, a_post) =
            border_templates_for_gap(&fixture.a_samples, ch, &border_spec);
        let (a_pre_ch, a_post_ch) =
            border_templates_per_channel_for_gap(&fixture.a_samples, ch, &border_spec);
        let b_mono = interleaved_to_mono(&haystack, ch);
        let b_ch = interleaved_to_channels(&haystack, ch);
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch: &b_ch,
        };
        let waveform = WaveformSeamContext {
            templates: &templates,
            gap_frames,
            pre_window: a_pre.len().max(1),
            post_window: a_post.len().max(1),
            b_total_frames: b_mono.len(),
            repeat_window_frames: self.patch_structure_params.bin_frames.max(1),
            repeat_penalty_weight: 0.0,
        };
        let signature = build_gap_signature(
            &fixture.a_samples,
            ch,
            self.refined.start_frame,
            self.refined.end_frame,
            self.context_frames,
            &self.patch_structure_params,
            mode,
        );
        match_gap_fill_unified_in_b(
            &UnifiedFillSearchInput {
                signature: &signature,
                b_samples: &haystack,
                channels: ch,
                waveform: &waveform,
                nominal_fill_start: self.offset_nominal_start,
                nominal_fill_end: self.gap_end_in_haystack,
            },
            &self.patch_structure_params,
            weights,
        )
    }

    pub fn format_diagnostic(&self, fixture: &EnergySignatureFixture) -> String {
        let bin = fixture.bin_frames();
        let mut lines = vec![
            format!("=== patch geometry diagnostic ({}) ===", fixture.id),
            format!(
                "fixture frames: gap [{}..{}] nominal_fill={} true_fill={} shift={}",
                self.fixture_gap_start,
                self.fixture_gap_end,
                self.fixture_nominal_fill_start,
                self.fixture_true_fill_start,
                self.shift_frames,
            ),
            format!(
                "gap report secs: A [{:.6}..{:.6}] B_nominal [{:.6}..{:.6}]",
                self.report_a_start_secs,
                self.report_a_end_secs,
                self.report_b_start_secs,
                self.report_b_end_secs,
            ),
            format!(
                "gap_offset_secs={:.6} (from_alignment={})",
                self.gap_offset_secs, self.gap_offset_from_alignment
            ),
            format!(
                "reported frames [{}, {}] → refined [{}, {}] (fixture gap_frames={} refined={})",
                self.reported_start_frame,
                self.reported_end_frame,
                self.refined.start_frame,
                self.refined.end_frame,
                self.fixture_gap_frames,
                refined_gap_frames(&self.refined),
            ),
            format!(
                "B mapped: [{:.6}..{:.6}] haystack [{:.6}..{:.6}] ({} frames)",
                self.refined_b_start_secs,
                self.refined_b_end_secs,
                self.b_extract_start_secs,
                self.b_extract_end_secs,
                self.haystack_frames,
            ),
            format!(
                "haystack nominal start={} end={} search_radius={} context={} bin={}",
                self.offset_nominal_start,
                self.gap_end_in_haystack,
                self.search_radius_frames,
                self.context_frames,
                bin,
            ),
            format!(
                "true fill: full_B={} haystack={} in_haystack={} within_search={}",
                self.true_fill_in_full_b,
                self.true_fill_in_haystack,
                self.true_within_haystack,
                self.true_within_search_radius,
            ),
        ];
        if self.refined.start_frame != self.fixture_gap_start
            || self.refined.end_frame != self.fixture_gap_end
        {
            lines.push(format!(
                "NOTE: refine_gap_frames moved edges by start Δ{} end Δ{} vs fixture",
                self.refined.start_frame as i64 - self.fixture_gap_start as i64,
                self.refined.end_frame as i64 - self.fixture_gap_end as i64,
            ));
        }
        if !self.true_within_search_radius {
            lines.push(
                "WARN: true fill site outside search radius from haystack nominal — expect BoundaryAlignmentFailed"
                    .into(),
            );
        }
        lines.join("\n")
    }
}

fn refined_gap_frames(refined: &RefinedGapFrames) -> usize {
    refined.end_frame.saturating_sub(refined.start_frame)
}

/// Recompute patch-path geometry without running `PatchAudio`.
pub fn preview_patch_geometry(
    fixture: &EnergySignatureFixture,
    alignment: &ScanAlignment,
    a_start_secs: f64,
    a_end_secs: f64,
    b_start_secs: f64,
    b_end_secs: f64,
    params: &PatchGeometryParams,
) -> PatchGeometryPreview {
    let rate = fixture.sample_rate;
    let channels = fixture.channels.max(1);
    let silence_peak_fraction = fixture.structure_params.silence_peak_fraction;
    let absolute_silence_rms = fixture.structure_params.absolute_silence_rms;

    let gap_time_on_a = (a_start_secs + a_end_secs) / 2.0;
    let gap_offset_from_alignment = resolve_gap_offset_secs(
        alignment,
        params.fill_offset_mode,
        gap_time_on_a,
        None,
        AnchoredRetryPass::First,
    );
    let gap_offset_secs = gap_offset_from_alignment.unwrap_or(b_start_secs - a_start_secs);
    let gap_offset_from_alignment = gap_offset_from_alignment.is_some();

    let reported_start_frame = (a_start_secs * rate as f64) as usize;
    let reported_end_frame = (a_end_secs * rate as f64) as usize;
    let max_refine_frames = (GAP_EDGE_REFINE_SECS * rate as f64).round() as usize;
    let refined = policies::refine_gap_frames(
        &fixture.a_samples,
        channels,
        reported_start_frame,
        reported_end_frame,
        silence_peak_fraction,
        absolute_silence_rms,
        max_refine_frames,
    );

    let refined_a_start_secs = refined.start_frame as f64 / rate as f64;
    let refined_a_end_secs = refined.end_frame as f64 / rate as f64;
    let refined_b_start_secs = refined_a_start_secs + gap_offset_secs;
    let refined_b_end_secs = refined_a_end_secs + gap_offset_secs;

    let context_frames = (params.gap_signature_context_secs * rate as f64).round() as usize;
    let bin_frames =
        ((params.gap_signature_bin_ms as f64 / 1000.0) * rate as f64).round() as usize;
    let margin_secs = params.fill_align_margin_secs;
    let border_search_secs = params.fill_border_search_secs;
    let search_radius_secs = border_search_secs.max(margin_secs);
    let extend_slack_secs = gap_extension_slack_secs(clip_sync_repair::domain::RepairPatchConfigView {
        fill_mode: if params.fill_mode_fit {
            clip_sync_repair::domain::FillMode::Fit
        } else {
            clip_sync_repair::domain::FillMode::Gate
        },
        fit_boundary_search: params.fit_boundary_search,
        gap_end_extend_on_post_seam_fail: params.gap_end_extend_on_post_seam_fail,
        gap_start_extend_on_pre_seam_fail: params.gap_start_extend_on_pre_seam_fail,
        gap_end_extend_max_ms: params.gap_end_extend_max_ms,
        disable_structure_trust: false,
        short_gap_one_strong_seam_fallback: true,
        fill_anchor_search_prior_weight: 0.0,
        fill_anchor_retry_marginal: false,
        fill_offset_mode: params.fill_offset_mode,
        anchor_seam_mode: clip_sync_repair::domain::AnchorSeamMode::Off,
    });
    let b_extract_start_secs = (refined_b_start_secs
        - params.gap_signature_context_secs
        - search_radius_secs
        - margin_secs
        - extend_slack_secs)
        .max(0.0);
    let length_slack_secs = params.fill_length_slack_secs.max(margin_secs);
    let b_extract_end_secs = refined_b_end_secs
        + params.gap_signature_context_secs
        + search_radius_secs
        + length_slack_secs
        + margin_secs
        + extend_slack_secs;

    let search_radius_frames = (border_search_secs * rate as f64).round() as usize;
    let fill_length_slack_frames = (params.fill_length_slack_secs * rate as f64).round() as usize;
    let gap_frames = refined_gap_frames(&refined);
    let patch_structure_params = StructureMatchParams {
        gap_frames,
        bin_frames: bin_frames.max(1),
        search_radius_frames,
        fill_length_slack_frames,
        max_fine_adjustment_frames: gap_structure::structure_fine_polish_frames(bin_frames),
        silence_peak_fraction,
        absolute_silence_rms,
    };

    let offset_nominal_start = ((refined_b_start_secs - b_extract_start_secs) * rate as f64)
        .round() as usize;
    let gap_end_in_haystack = ((refined_b_end_secs - b_extract_start_secs) * rate as f64)
        .round() as usize;

    let haystack_frames = slice_b_interleaved(
        &fixture.b_samples,
        channels,
        rate,
        b_extract_start_secs,
        b_extract_end_secs,
    )
    .len()
        / channels;

    let extract_start_frame = (b_extract_start_secs * rate as f64).round() as usize;
    let true_fill_in_haystack = fixture.true_fill_start.saturating_sub(extract_start_frame);
    let true_within_haystack =
        fixture.true_fill_start >= extract_start_frame && true_fill_in_haystack < haystack_frames;
    let true_within_search_radius = true_within_haystack
        && offset_nominal_start.abs_diff(true_fill_in_haystack) <= search_radius_frames;

    PatchGeometryPreview {
        fixture_gap_start: fixture.gap_start,
        fixture_gap_end: fixture.gap_end,
        fixture_true_fill_start: fixture.true_fill_start,
        fixture_nominal_fill_start: fixture.nominal_fill_start,
        shift_frames: fixture.true_fill_start as i64 - fixture.nominal_fill_start as i64,
        report_a_start_secs: a_start_secs,
        report_a_end_secs: a_end_secs,
        report_b_start_secs: b_start_secs,
        report_b_end_secs: b_end_secs,
        gap_offset_secs,
        gap_offset_from_alignment,
        reported_start_frame,
        reported_end_frame,
        refined,
        fixture_gap_frames: fixture.gap_frames(),
        refined_b_start_secs,
        refined_b_end_secs,
        b_extract_start_secs,
        b_extract_end_secs,
        search_radius_frames,
        context_frames,
        offset_nominal_start,
        gap_end_in_haystack,
        haystack_frames,
        true_fill_in_haystack,
        true_fill_in_full_b: fixture.true_fill_start,
        true_within_haystack,
        true_within_search_radius,
        patch_structure_params,
    }
}

pub(crate) fn slice_b_interleaved(
    b_samples: &[f32],
    channels: usize,
    sample_rate: u32,
    start_secs: f64,
    end_secs: f64,
) -> Vec<f32> {
    let channels = channels.max(1);
    let start_frame = (start_secs * sample_rate as f64).round() as usize;
    let end_frame =
        ((end_secs * sample_rate as f64).round() as usize).min(b_samples.len() / channels);
    if start_frame >= end_frame {
        return Vec::new();
    }
    b_samples[start_frame * channels..end_frame * channels].to_vec()
}
