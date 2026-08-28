//! `overlay` slice of the gap_fingerprint measure folder split.

use crate::application::gap_fingerprint::project::*;
use crate::application::gap_fingerprint::schema::*;

use crate::domain::gap_anchor_seam::{
    list_anchor_candidates_a, list_feasible_anchor_brackets, AnchorSeamParams, AnchorSource,
};
use crate::domain::gap_structure::StructureMatchParams;
use crate::domain::pcm::{interleaved_to_channels, interleaved_to_mono};
use crate::domain::policies::RefinedGapFrames;

use super::config::*;
use super::lag::*;
use super::pcm::*;
use super::placement::*;
use super::probes::*;

/// Build the D/R tag payload from the shared [`RegionMeasurements`] (computed from decode) + the A-side levels
/// (8g.3b). Mirrors [`tags_from_fingerprint`] via [`tags_from_fields`]; `structure`/`seams` are omitted
/// (`None`) to match the `skip_baseline_placement` summary the dump uses — so from-decode tags equal the
/// oracle's on that path by construction. Used by the from-decode dump ([`characterize_gaps_from_decode`]).
pub(in crate::application::gap_fingerprint::measure) fn tags_from_measurements(
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
        m.donor_interior_aligned,
        m.donor_interior_nominal,
        levels,
    )
}

// ---------------------------------------------------------------------------------------------
// Lag-correlation probe
// ---------------------------------------------------------------------------------------------
/// Scores + stage extracted from a gate failure — keeps structure and waveform channels separate.
pub(in crate::application::gap_fingerprint::measure) struct BracketStageDetail {
    pub(in crate::application::gap_fingerprint::measure) stage: FailureStage,
    pub(in crate::application::gap_fingerprint::measure) structure_pre: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) structure_post: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) seam_pre: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) seam_post: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) residual_margin_db: Option<f64>,
}
/// Map a gate failure to the fingerprint's `failure_stage` and the scores that belong on each field.
/// Structure floors land on `structure_*`; waveform/residual on `seam_*`; residual also carries
/// `residual_margin_db`. Never overload structure correlations onto `seam_*`.
pub(in crate::application::gap_fingerprint::measure) fn stage_of(
    failure: &crate::application::patch_region::SeamGateFailure,
) -> BracketStageDetail {
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
pub(in crate::application::gap_fingerprint::measure) fn anchor_params_from_gate(
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
pub(in crate::application::gap_fingerprint::measure) struct RegionMeasurements {
    pub(in crate::application::gap_fingerprint::measure) brackets: Vec<BracketInfo>,
    pub(in crate::application::gap_fingerprint::measure) outcome: GateOutcome,
    pub(in crate::application::gap_fingerprint::measure) lag_decision: Option<LagFingerprint>,
    pub(in crate::application::gap_fingerprint::measure) splice: Option<SpliceSummary>,
    pub(in crate::application::gap_fingerprint::measure) seam_probe: Option<SeamProbeFingerprint>,
    pub(in crate::application::gap_fingerprint::measure) donor_interior_aligned:
        Option<DonorInterior>,
    pub(in crate::application::gap_fingerprint::measure) donor_interior_nominal:
        Option<DonorInterior>,
    pub(in crate::application::gap_fingerprint::measure) b_levels: Option<LevelProfile>,
    pub(in crate::application::gap_fingerprint::measure) splice_dualfit: Option<SpliceDualfit>,
    pub(in crate::application::gap_fingerprint::measure) wide_envelope:
        Option<WideEnvelopeFingerprint>,
    pub(in crate::application::gap_fingerprint::measure) residual: Option<ResidualInfo>,
    pub(in crate::application::gap_fingerprint::measure) lag_editorial: Option<LagFingerprint>,
}
/// Per-gap inputs for [`compute_region_measurements`] — the geometry the caller resolved (refined throat, B
/// extract window/offset) plus the shared config/frame params. `gap_floor_db` is `fp.levels.gap_floor_db` from
/// the summary pass (the donor/seam-probe floor).
pub(in crate::application::gap_fingerprint::measure) struct RegionMeasureInput<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_pcm: &'a clip_sync::MultiChannelPcm,
    pub(in crate::application::gap_fingerprint::measure) ch: usize,
    pub(in crate::application::gap_fingerprint::measure) b_slice: &'a [f32],
    pub(in crate::application::gap_fingerprint::measure) b_extract_start_secs: f64,
    pub(in crate::application::gap_fingerprint::measure) gap_offset: f64,
    pub(in crate::application::gap_fingerprint::measure) refined: RefinedGapFrames,
    pub(in crate::application::gap_fingerprint::measure) gap_frames: usize,
    pub(in crate::application::gap_fingerprint::measure) gate_settings:
        &'a crate::application::PatchRequestSettings,
    pub(in crate::application::gap_fingerprint::measure) gate_derived:
        crate::application::patch_region::SeamGateDerived,
    pub(in crate::application::gap_fingerprint::measure) cfg: &'a FingerprintConfig,
    pub(in crate::application::gap_fingerprint::measure) gap_floor_db: f32,
    pub(in crate::application::gap_fingerprint::measure) include_diagnostics: bool,
    pub(in crate::application::gap_fingerprint::measure) context_frames: usize,
    pub(in crate::application::gap_fingerprint::measure) bin_frames: usize,
    pub(in crate::application::gap_fingerprint::measure) search_radius_frames: usize,
    pub(in crate::application::gap_fingerprint::measure) rate: f64,
    pub(in crate::application::gap_fingerprint::measure) sample_rate: u32,
    pub(in crate::application::gap_fingerprint::measure) progress:
        &'a dyn clip_sync::ProgressReporter,
}
/// Compute one gap's gate-overlay measurements from decode — the shared per-gap measurement site the
/// from-decode dump ([`characterize_gaps_from_decode`]) projects via [`tags_from_measurements`] (8g.3a).
pub(in crate::application::gap_fingerprint::measure) fn compute_region_measurements(
    inp: RegionMeasureInput<'_>,
) -> RegionMeasurements {
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
    let donor_interior_aligned = donor_interior_at(
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
        donor_aligned: donor_interior_aligned.as_ref(),
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
        donor_interior_aligned,
        donor_interior_nominal,
        b_levels,
        splice_dualfit,
        wide_envelope,
        residual,
        lag_editorial,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            donor_interior_aligned: Some(donor(0.05, true)),
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
                pre_seam_rescued: None,
                post_seam_rescued: None,
                pre_seam_edge_pinned: None,
                post_seam_edge_pinned: None,
                seams_clamped: None,
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
}
