//! `placement` slice of the gap_fingerprint measure folder split.

use crate::application::gap_fingerprint::schema::*;



use crate::domain::gap_fill_fit::{
    match_gap_fill_unified_in_b, UnifiedFillSearchInput, UnifiedFitWeights, WaveformSeamContext,
};
use crate::domain::gap_signature::{build_gap_signature, GapSignatureMode};
use crate::domain::policies::{
    border_templates_for_gap,
    border_templates_per_channel_for_gap,
    seam_channel_diagnostics, RefinedGapFrames, SeamPlacement, SeamTemplates,
};

use super::pcm::*;
use super::config::*;

/// Owned result of a structure+waveform placement on B at one A bracket.
pub(in crate::application::gap_fingerprint::measure) struct PlacementScores {
    pub(in crate::application::gap_fingerprint::measure) start_frame: usize,
    /// B-derived fill length the end sweep chose. Differs from `gap_frames` by up to
    /// `fill_length_slack_frames`; the only observable of the end search's decision.
    pub(in crate::application::gap_fingerprint::measure) fill_frames: usize,
    pub(in crate::application::gap_fingerprint::measure) structure_pre: f64,
    pub(in crate::application::gap_fingerprint::measure) structure_post: f64,
    pub(in crate::application::gap_fingerprint::measure) seam_pre: f64,
    pub(in crate::application::gap_fingerprint::measure) seam_post: f64,
    pub(in crate::application::gap_fingerprint::measure) per_channel: Vec<(f64, f64)>,
    pub(in crate::application::gap_fingerprint::measure) selected_channels: Vec<usize>,
    pub(in crate::application::gap_fingerprint::measure) mono_pre: f64,
    pub(in crate::application::gap_fingerprint::measure) mono_post: f64,
}
/// Inputs for [`place_on_b`] — structure+waveform unified search on the B haystack.
pub(in crate::application::gap_fingerprint::measure) struct PlaceOnBInput<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_samples: &'a [f32],
    pub(in crate::application::gap_fingerprint::measure) channels: usize,
    pub(in crate::application::gap_fingerprint::measure) refined: RefinedGapFrames,
    pub(in crate::application::gap_fingerprint::measure) b_haystack: &'a [f32],
    pub(in crate::application::gap_fingerprint::measure) b_mono: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) b_ch: &'a [Vec<f64>],
    pub(in crate::application::gap_fingerprint::measure) nominal_fill_start: usize,
    pub(in crate::application::gap_fingerprint::measure) context_frames: usize,
    pub(in crate::application::gap_fingerprint::measure) bin_frames: usize,
    pub(in crate::application::gap_fingerprint::measure) search_radius_frames: usize,
    pub(in crate::application::gap_fingerprint::measure) cfg: &'a FingerprintConfig,
}
impl CorpusProbeRecipe {
    /// Build from the config **and the gaps that were actually built**, so each window is reported
    /// only where the corpus can show the measurement it sized.
    ///
    /// The config alone is not evidence. [`characterize_gaps`] builds every gap at
    /// `DetailTier::Summary` with `skip_baseline_placement`, which calls neither [`lag_at_placement`]
    /// nor [`place_on_b`] — yet its `FingerprintConfig` still carries `lag_window_secs` /
    /// `lag_max_lag_ms` / `fill_seam_search_secs`. Reading those straight off the config would
    /// advertise sweeps that never ran: the deleted `border_standoff_secs` defect, moved up one level.
    /// From-decode corpora are mixed (selected gaps rebuilt at `DetailTier::Full`, the rest left at
    /// summary), so the test is "did *any* gap carry this measurement" — the same check a reader would
    /// make against the gaps in front of them.
    ///
    /// `sample_rate` is required, not defaulted: the standoff the probe actually applied is a frame
    /// count, and the only honest way to report it in seconds is to divide by the rate it was applied
    /// at. Hardcoding `0.0` here would re-create the defect this type exists to close the moment
    /// [`PROBE_BORDER_STANDOFF_FRAMES`] stops being zero.
    pub fn for_corpus(cfg: &FingerprintConfig, sample_rate: u32, gaps: &[GapFingerprint]) -> Self {
        let measured_lag = gaps.iter().any(|g| g.lag_decision.is_some());
        let measured_seams = gaps.iter().any(|g| g.seams.is_some());
        Self {
            noise_collar_secs: cfg.noise_collar_secs,
            lag_window_secs: measured_lag.then_some(cfg.lag_window_secs),
            lag_max_lag_ms: measured_lag.then_some(cfg.lag_max_lag_ms),
            // Include `0` so an off-arm dump is distinguishable from a pre-field corpus.
            lag_envelope_max_lag_ms: measured_lag.then_some(cfg.lag_envelope_max_lag_ms),
            seam_window_secs: measured_seams.then_some(cfg.fill_seam_search_secs),
            border_standoff_secs: probe_border_standoff_secs(sample_rate),
        }
    }
}
/// Structure-best placement of the A bracket `refined` on the B haystack, plus the seam there.
/// Structure-dominant weights so the placement locks to the energy-best (mirrors the production
/// "structure aligns, seam read there" story). `None` if the match degenerates.
pub(in crate::application::gap_fingerprint::measure) fn place_on_b(input: &PlaceOnBInput<'_>) -> Option<PlacementScores> {
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
    let border_spec = probe_border_spec(&refined, border_frames, cfg);
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
        // Unbiased placement: every distance-from-nominal prior is off here, backward included.
        early_start_penalty_scale: 0.0,
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
pub(in crate::application::gap_fingerprint::measure) fn b_mapped_frame_in_haystack(
    a_start_frame: usize,
    sample_rate: f64,
    gap_offset_secs: f64,
    b_extract_start_secs: f64,
) -> usize {
    (((a_start_frame as f64 / sample_rate + gap_offset_secs - b_extract_start_secs) * sample_rate)
        .round()
        .max(0.0)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::build::*;

    #[test]
    fn probe_recipe_reports_the_collar_width_that_moved_the_measurement() {
        let rate = 48_000u32;
        let frames = |s: f64| (s * f64::from(rate)) as usize;
        let (gap_start, gap_end) = (frames(1.0), frames(1.5));
        let mut a = vec![0f32; frames(2.0)];
        // 100 ms of tone hard against the gap edge; everything before it is silent, so a collar wider
        // than 100 ms averages in that silence and the RMS/peak ratio falls.
        for i in frames(0.9)..gap_start {
            a[i] = (i as f32 * 0.05).sin() * 0.5;
        }

        let measure = |collar_secs: f64| {
            let config = FingerprintConfig {
                noise_collar_secs: collar_secs,
                ..Default::default()
            };
            let inputs = GapInputs {
                a_samples: &a,
                b_haystack: None,
                channels: 1,
                sample_rate: rate,
                reported_start_frame: gap_start,
                reported_end_frame: gap_end,
                b_extract_start_secs: 0.0,
                gap_offset_secs: 0.0,
                config,
            };
            let fp = build_gap_fingerprint(0, &inputs, DetailTier::Full, false);
            let ratio = fp
                .silence
                .as_ref()
                .expect("silence profile measured")
                .collar_rms_peak_ratio;
            let recipe = CorpusProbeRecipe::for_corpus(&config, rate, std::slice::from_ref(&fp));
            (ratio, recipe)
        };

        let (narrow, narrow_recipe) = measure(0.1);
        let (wide, wide_recipe) = measure(1.0);
        assert!(
            narrow > wide * 2.0,
            "collar width must move the measurement it is reported for: {narrow} vs {wide}"
        );
        assert_eq!(narrow_recipe.noise_collar_secs, 0.1);
        assert_eq!(wide_recipe.noise_collar_secs, 1.0);
        // Not a restatement of the constant: this asserts the recipe *derives* the standoff it reports
        // from the same constant the specs are built with, at the rate it was applied at. If someone
        // wires a non-zero standoff into the probe, this reads it back instead of silently claiming 0.
        assert_eq!(
            narrow_recipe.border_standoff_secs,
            probe_border_standoff_secs(48_000),
            "the recipe must report the standoff the probe's border specs were built with, never \
             production's 0.35"
        );
    }

    /// A summary-only corpus must not advertise the lag/seam sweeps it never ran. `characterize_gaps`
    /// builds every gap at `DetailTier::Summary` with `skip_baseline_placement` — no
    /// `lag_at_placement`, no `place_on_b` — while its `FingerprintConfig` still carries
    /// `lag_window_secs` / `lag_max_lag_ms` / `fill_seam_search_secs`. Quoting them from the config is
    /// the deleted `border_standoff_secs` defect one level up, so the recipe reads the gaps instead.
    #[test]
    fn probe_recipe_omits_windows_the_summary_path_never_swept() {
        let rate = 48_000u32;
        let frames = |s: f64| (s * f64::from(rate)) as usize;
        let mut a = vec![0f32; frames(2.0)];
        for i in frames(0.9)..frames(1.0) {
            a[i] = (i as f32 * 0.05).sin() * 0.5;
        }
        let config = FingerprintConfig::default();
        let inputs = GapInputs {
            a_samples: &a,
            b_haystack: None,
            channels: 1,
            sample_rate: rate,
            reported_start_frame: frames(1.0),
            reported_end_frame: frames(1.5),
            b_extract_start_secs: 0.0,
            gap_offset_secs: 0.0,
            config,
        };

        let summary = build_gap_fingerprint(0, &inputs, DetailTier::Summary, true);
        assert!(summary.lag_decision.is_none() && summary.seams.is_none());
        let recipe = CorpusProbeRecipe::for_corpus(&config, rate, std::slice::from_ref(&summary));
        assert_eq!(
            recipe.noise_collar_secs, config.noise_collar_secs,
            "levels runs on every path, so the collar is always reportable"
        );
        assert_eq!(
            (recipe.lag_window_secs, recipe.lag_max_lag_ms, recipe.lag_envelope_max_lag_ms, recipe.seam_window_secs),
            (None, None, None, None),
            "a summary-only corpus has no lag_decision and no seams — reporting their windows would \
             describe sweeps that never ran"
        );

        // The other direction — a corpus that DID sweep must report the windows — is asserted where a
        // real B side exists to sweep against: `production_dump_omits_unmeasured_option_fields`.
    }


}
