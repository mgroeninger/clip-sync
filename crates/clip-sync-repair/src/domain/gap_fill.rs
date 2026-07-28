use crate::domain::gap::GapReport;
use crate::domain::patch_result::GapFillSkipReason;
use crate::domain::track_match::CompatibilityVerdict;

/// Describes one region where B audio will be spliced into A.
pub struct FillRegion {
    /// Index into [`GapReport::gaps`] this region was planned from. The identity used to join
    /// patch outcomes back onto the report — never re-derive it from `a_start_secs`, which the
    /// patch path refines locally.
    pub gap_index: usize,
    pub a_start_secs: f64,
    pub a_end_secs: f64,
    pub b_start_secs: f64,
    pub b_end_secs: f64,
    /// Loudness gain applied to B segment (1.0 = no change; updated by PatchAudio).
    pub gain: f32,
    pub crossfade_secs: f64,
}

/// A gap detected in A that will not be attempted during patching.
#[derive(Debug, Clone, PartialEq)]
pub struct GapFillSkipped {
    /// Index into [`GapReport::gaps`]; see [`FillRegion::gap_index`].
    pub gap_index: usize,
    pub a_start_secs: f64,
    pub a_end_secs: f64,
    pub reason: GapFillSkipReason,
}

/// Ordered list of regions to splice plus gaps excluded at plan time.
pub struct GapFillPlan {
    pub regions: Vec<FillRegion>,
    pub skipped: Vec<GapFillSkipped>,
}

/// Build a fill plan from a gap report.
///
/// Returns an empty plan when:
/// - `track_compatibility` is `None`
/// - the compatibility verdict is `Mismatch`
///
/// Only gaps for which [`Gap::is_fillable`] returns `true` are included in `regions`.
/// Other gaps are listed in `skipped` with a reason.
///
/// The query-reference mapped-region gate is read from [`GapReport::limit_fill_to_mapped_region`]
/// (the single source of truth, set when the report is built) so the plan can never disagree with
/// [`GapReport::repairable_count`].
pub fn build_gap_fill_plan(
    report: &GapReport,
    crossfade_ms: u64,
    skip_equivalent_gaps: bool,
) -> GapFillPlan {
    let crossfade_secs = crossfade_ms as f64 / 1000.0;
    let limit_fill_to_mapped_region = report.limit_fill_to_mapped_region;

    let plan_block_reason = match &report.track_compatibility {
        None => Some(GapFillSkipReason::TrackCompatibilityUnavailable),
        Some(tc) if tc.verdict == CompatibilityVerdict::Mismatch => {
            Some(GapFillSkipReason::TrackLayoutMismatch)
        }
        Some(_) => None,
    };

    if let Some(reason) = plan_block_reason {
        let skipped = report
            .gaps
            .iter()
            .enumerate()
            .map(|(index, g)| GapFillSkipped {
                gap_index: index,
                a_start_secs: g.video_a_start_secs,
                a_end_secs: g.video_a_end_secs,
                reason: if g.is_fillable() {
                    reason.clone()
                } else {
                    GapFillSkipReason::NotFillable
                },
            })
            .collect();
        return GapFillPlan {
            regions: vec![],
            skipped,
        };
    }

    let mut regions = Vec::new();
    let mut skipped = Vec::new();

    for (index, g) in report.gaps.iter().enumerate() {
        if !g.is_fillable() {
            skipped.push(GapFillSkipped {
                gap_index: index,
                a_start_secs: g.video_a_start_secs,
                a_end_secs: g.video_a_end_secs,
                reason: GapFillSkipReason::NotFillable,
            });
            continue;
        }

        if limit_fill_to_mapped_region && report.gap_outside_reference_coverage(g) {
            skipped.push(GapFillSkipped {
                gap_index: index,
                a_start_secs: g.video_a_start_secs,
                a_end_secs: g.video_a_end_secs,
                reason: GapFillSkipReason::OutsideReferenceCoverage,
            });
            continue;
        }

        // Equivalence gate (lowest precedence — after fillable + coverage, per plan §4): drop gaps whose
        // silence is already equivalent to B's (mutual/ambient silence), so the decode/patch path is never
        // entered for them. Only when `skip_equivalent_gaps`; the classification is advisory otherwise.
        if skip_equivalent_gaps && report.gap_equivalence_at(index).is_some_and(|v| v.drop) {
            skipped.push(GapFillSkipped {
                gap_index: index,
                a_start_secs: g.video_a_start_secs,
                a_end_secs: g.video_a_end_secs,
                reason: GapFillSkipReason::AlreadyMatchesReference,
            });
            continue;
        }

        regions.push(FillRegion {
            gap_index: index,
            a_start_secs: g.video_a_start_secs,
            a_end_secs: g.video_a_end_secs,
            b_start_secs: g.video_b_start_secs.unwrap(),
            b_end_secs: g.video_b_end_secs.unwrap(),
            gain: 1.0,
            crossfade_secs,
        });
    }

    GapFillPlan { regions, skipped }
}

/// Second stderr line after gap scan when some detected gaps are not repairable.
pub(crate) fn format_scan_fillable_followup(report: &GapReport) -> Option<String> {
    let found = report.gaps.len();
    let repairable = report.repairable_count();
    let omitted = found.saturating_sub(repairable);
    if omitted == 0 {
        return None;
    }

    let unfillable = report.gaps.iter().filter(|g| !g.is_fillable()).count();
    let outside = report
        .gaps
        .iter()
        .filter(|g| {
            g.is_fillable()
                && report.limit_fill_to_mapped_region
                && report.gap_outside_reference_coverage(g)
        })
        .count();
    let blocked = omitted.saturating_sub(unfillable + outside);

    let mut detail = Vec::new();
    if unfillable > 0 {
        detail.push(format!("{unfillable} unfillable"));
    }
    if outside > 0 {
        detail.push(format!("{outside} outside mapped region"));
    }
    if blocked > 0 {
        detail.push(format!("{blocked} blocked by track layout"));
    }

    Some(format!(
        "Gap fill: {repairable} of {found} repairable ({omitted} skipped — {})",
        detail.join(", ")
    ))
}

/// Patch-stage phase line: note plan-time skips before structure match / splice.
pub(crate) fn format_align_fill_regions_phase(plan: &GapFillPlan) -> String {
    let region_count = plan.regions.len();
    if plan.skipped.is_empty() {
        return format!("Aligning {region_count} fill region(s) (structure match + splice)...");
    }

    let skipped = plan.skipped.len();
    let unfillable = plan
        .skipped
        .iter()
        .filter(|entry| entry.reason == GapFillSkipReason::NotFillable)
        .count();
    let mut detail = Vec::new();
    if unfillable > 0 {
        detail.push(format!("{unfillable} unfillable"));
    }
    let other = skipped - unfillable;
    if other > 0 {
        detail.push(format!("{other} not planned"));
    }

    format!(
        "Skipping {skipped} gap(s) at fill plan ({detail}); aligning {region_count} fill region(s) (structure match + splice)...",
        detail = detail.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::domain::{
        align::{AlignedClip, ClipRole, ScanAlignment, TimelineOverlap},
        gap::{Gap, GapReport},
        patch_result::GapFillSkipReason,
        track_match::{CompatibilityVerdict, TrackCompatibility},
    };

    use super::*;

    fn make_alignment(offset: Option<f64>) -> ScanAlignment {
        ScanAlignment {
            clips: vec![AlignedClip {
                role: ClipRole::Start,
                window_start_secs: 0.0,
                window_end_secs: 60.0,
                aligned: offset.is_some(),
                offset_secs: offset,
                confidence: if offset.is_some() { 0.9 } else { 0.0 },
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            }],
            start_aligned: offset.is_some(),
            end_aligned: None,
            recommended_offset_secs: offset,
            offsets_consistent: true,
            offset_drift_secs: None,
            start_overlap: None,
            query_reference_mode: false,
        }
    }

    fn stereo_identical() -> TrackCompatibility {
        TrackCompatibility {
            a_channels: 2,
            b_channels: 2,
            a_sample_rate: 44_100,
            b_sample_rate: 44_100,
            channels_match: true,
            rate_match: true,
            verdict: CompatibilityVerdict::Identical,
        }
    }

    fn stereo_mismatch() -> TrackCompatibility {
        TrackCompatibility {
            a_channels: 2,
            b_channels: 6,
            a_sample_rate: 44_100,
            b_sample_rate: 44_100,
            channels_match: false,
            rate_match: true,
            verdict: CompatibilityVerdict::Mismatch,
        }
    }

    fn fillable_gap(a_start: f64, a_end: f64) -> Gap {
        Gap {
            video_a_start_secs: a_start,
            video_a_end_secs: a_end,
            video_b_start_secs: Some(a_start),
            video_b_end_secs: Some(a_end),
            b_has_energy: true,
        }
    }

    fn base_report(compat: Option<TrackCompatibility>, gaps: Vec<Gap>) -> GapReport {
        GapReport {
            video_a: PathBuf::from("a.wav"),
            video_b: PathBuf::from("b.wav"),
            track_compatibility: compat,
            alignment: make_alignment(Some(0.0)),
            gaps,
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 60,
            scan_block_ms: 250,
            silence_peak_fraction: 0.01,
            limit_fill_to_mapped_region: true,
            audio_timeline_skew: None,
        }
    }

    #[test]
    fn build_gap_fill_plan_empty_when_mismatch() {
        let report = base_report(Some(stereo_mismatch()), vec![fillable_gap(0.0, 3.0)]);
        assert_eq!(report.fillable_count(), 1);
        assert_eq!(report.repairable_count(), 0);
        assert!(!report.patch_allowed());
        let plan = build_gap_fill_plan(&report, 10, false);
        assert!(plan.regions.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(
            plan.skipped[0].reason,
            GapFillSkipReason::TrackLayoutMismatch
        );
    }

    #[test]
    fn build_gap_fill_plan_empty_when_no_compatibility() {
        let report = base_report(None, vec![fillable_gap(0.0, 3.0)]);
        let plan = build_gap_fill_plan(&report, 10, false);
        assert!(plan.regions.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(
            plan.skipped[0].reason,
            GapFillSkipReason::TrackCompatibilityUnavailable
        );
    }

    #[test]
    fn build_gap_fill_plan_includes_fillable_gaps() {
        let gaps = vec![
            fillable_gap(3.0, 6.0),
            Gap {
                video_a_start_secs: 10.0,
                video_a_end_secs: 13.0,
                video_b_start_secs: None,
                video_b_end_secs: None,
                b_has_energy: false,
            },
        ];
        let report = base_report(Some(stereo_identical()), gaps);
        let plan = build_gap_fill_plan(&report, 10, false);
        assert_eq!(plan.regions.len(), 1);
        assert!((plan.regions[0].a_start_secs - 3.0).abs() < 0.001);
        assert!((plan.regions[0].a_end_secs - 6.0).abs() < 0.001);
        assert!((plan.regions[0].crossfade_secs - 0.01).abs() < 0.0001);
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].reason, GapFillSkipReason::NotFillable);
    }

    #[test]
    fn build_gap_fill_plan_includes_gaps_outside_start_overlap() {
        let mut report = base_report(
            Some(stereo_identical()),
            vec![fillable_gap(1.0, 4.0), fillable_gap(5979.0, 6180.0)],
        );
        report.alignment.start_overlap = Some(TimelineOverlap {
            video_a_start_secs: 0.0,
            video_a_end_secs: 10.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 10.0,
            shared_length_secs: 10.0,
        });
        let plan = build_gap_fill_plan(&report, 0, false);
        assert_eq!(plan.regions.len(), 2);
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn build_gap_fill_plan_skips_gaps_outside_query_mapped_region() {
        let mut report = base_report(
            Some(stereo_identical()),
            vec![fillable_gap(1.0, 4.0), fillable_gap(5979.0, 6180.0)],
        );
        report.alignment.start_overlap = Some(TimelineOverlap {
            video_a_start_secs: 0.0,
            video_a_end_secs: 10.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 10.0,
            shared_length_secs: 10.0,
        });
        report.alignment.query_reference_mode = true;
        assert_eq!(report.repairable_count(), 1);

        let plan = build_gap_fill_plan(&report, 0, false);
        assert_eq!(plan.regions.len(), 1);
        assert!((plan.regions[0].a_start_secs - 1.0).abs() < 0.001);
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(
            plan.skipped[0].reason,
            GapFillSkipReason::OutsideReferenceCoverage
        );

        // With the mapped-region gate disabled, both gaps are planned.
        report.limit_fill_to_mapped_region = false;
        let plan_all = build_gap_fill_plan(&report, 0, false);
        assert_eq!(plan_all.regions.len(), 2);
    }

    #[test]
    fn equivalence_drops_gap_only_when_flag_enabled() {
        use crate::domain::gap_equivalence::{classify_gap_equivalence, GapEquivalenceParams};

        let on = GapEquivalenceParams {
            enabled: true,
            ..Default::default()
        };
        // Two fillable gaps; the first classifies as shared silence (drop), the second as a repairable
        // dropout (keep). Index-parallel to `gaps`.
        let mut report = base_report(
            Some(stereo_identical()),
            vec![fillable_gap(3.0, 6.0), fillable_gap(20.0, 23.0)],
        );
        report.gap_equivalence = vec![
            classify_gap_equivalence(Some(-108.0), Some(-46.0), Some(1.0), &on), // shared_silence → drop
            classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.0), &on), // repairable → keep
        ];
        assert!(report.gap_equivalence[0].drop && !report.gap_equivalence[1].drop);

        // Flag off: classification is advisory only — both gaps still planned.
        let plan_off = build_gap_fill_plan(&report, 0, false);
        assert_eq!(plan_off.regions.len(), 2);
        assert!(plan_off.skipped.is_empty());

        // Flag on: the dropping gap is skipped as AlreadyMatchesReference; the other is still planned.
        let plan_on = build_gap_fill_plan(&report, 0, true);
        assert_eq!(plan_on.regions.len(), 1);
        assert!((plan_on.regions[0].a_start_secs - 20.0).abs() < 1e-9);
        assert_eq!(plan_on.skipped.len(), 1);
        assert_eq!(
            plan_on.skipped[0].reason,
            GapFillSkipReason::AlreadyMatchesReference
        );
        assert!((plan_on.skipped[0].a_start_secs - 3.0).abs() < 1e-9);
    }

    #[test]
    fn equivalence_never_overrides_not_fillable() {
        use crate::domain::gap_equivalence::{classify_gap_equivalence, GapEquivalenceParams};

        // An unfillable gap (no B energy) whose (hypothetical) verdict says keep must still be NotFillable —
        // equivalence is lowest precedence and only ever *drops* fillable gaps.
        let on = GapEquivalenceParams {
            enabled: true,
            ..Default::default()
        };
        let mut report = base_report(
            Some(stereo_identical()),
            vec![Gap {
                video_a_start_secs: 10.0,
                video_a_end_secs: 13.0,
                video_b_start_secs: None,
                video_b_end_secs: None,
                b_has_energy: false,
            }],
        );
        report.gap_equivalence = vec![classify_gap_equivalence(
            Some(-106.0),
            Some(-47.0),
            Some(0.0),
            &on,
        )];
        let plan = build_gap_fill_plan(&report, 0, true);
        assert!(plan.regions.is_empty());
        assert_eq!(plan.skipped[0].reason, GapFillSkipReason::NotFillable);
    }

    #[test]
    fn format_scan_fillable_followup_omitted_gaps() {
        let report = base_report(
            Some(stereo_identical()),
            vec![
                fillable_gap(3.0, 6.0),
                Gap {
                    video_a_start_secs: 10.0,
                    video_a_end_secs: 13.0,
                    video_b_start_secs: None,
                    video_b_end_secs: None,
                    b_has_energy: false,
                },
            ],
        );
        let line = super::format_scan_fillable_followup(&report).expect("follow-up line");
        assert!(line.contains("1 of 2 repairable"));
        assert!(line.contains("1 skipped"));
        assert!(line.contains("1 unfillable"));
    }

    #[test]
    fn format_scan_fillable_followup_all_repairable_is_none() {
        let report = base_report(Some(stereo_identical()), vec![fillable_gap(1.0, 2.0)]);
        assert!(super::format_scan_fillable_followup(&report).is_none());
    }

    #[test]
    fn format_align_fill_regions_phase_notes_skipped_unfillable() {
        let report = base_report(
            Some(stereo_identical()),
            vec![
                fillable_gap(3.0, 6.0),
                Gap {
                    video_a_start_secs: 10.0,
                    video_a_end_secs: 13.0,
                    video_b_start_secs: None,
                    video_b_end_secs: None,
                    b_has_energy: false,
                },
            ],
        );
        let plan = build_gap_fill_plan(&report, 0, false);
        let line = super::format_align_fill_regions_phase(&plan);
        assert!(line.contains("Skipping 1 gap(s) at fill plan (1 unfillable)"));
        assert!(line.contains("aligning 1 fill region(s)"));
    }
}
