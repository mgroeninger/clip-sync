use std::collections::HashSet;

use crate::domain::gap::GapReport;
use crate::domain::patch_result::GapFillSkipReason;
use crate::domain::track_match::CompatibilityVerdict;

/// Resolved after scan, before fill plan. **0-based** — same base as [`FillRegion::gap_index`].
#[derive(Debug, Clone)]
pub struct GapSelection {
    /// 0-based indices into `GapReport.gaps` (chronological). May be empty when the report has no
    /// gaps (`GapSelectionMode::All` / skip-nothing on an empty report). A non-empty report never
    /// yields an empty set after a successful [`resolve_gap_selection`] — that case errors first.
    selected: HashSet<usize>,
    /// Present when `--only-gaps` / `--skip-gaps` (or TOML peers) were in effect — drives the
    /// stderr filter note.
    filter: Option<GapSelectionFilter>,
}

#[derive(Debug, Clone)]
struct GapSelectionFilter {
    /// `"only-gaps"` or `"skip-gaps"`.
    kind: &'static str,
    /// User tokens as provided (trimmed), for the filter note.
    tokens: Vec<String>,
}

impl GapSelection {
    /// Every gap selected — the default when no selection flag is given.
    pub fn all(gap_count: usize) -> Self {
        Self {
            selected: (0..gap_count).collect(),
            filter: None,
        }
    }

    pub fn is_selected(&self, gap_index: usize) -> bool {
        self.selected.contains(&gap_index)
    }

    /// True when a selection flag was in effect and did not name every gap.
    pub fn is_filtered(&self, gap_count: usize) -> bool {
        self.filter.is_some() && self.selected.len() != gap_count
    }
}

/// Unresolved user intent; tokens are still strings (v1 parses integers at resolve time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GapSelectionMode {
    All,
    Only(Vec<String>),
    Skip(Vec<String>),
}

/// Parse CLI/TOML selection tokens against `report.gaps` (1-based gap numbers → 0-based indices).
///
/// Error strings for `0` / out-of-range match `resolve_fingerprint_gap_select` verbatim.
pub fn resolve_gap_selection(
    mode: &GapSelectionMode,
    report: &GapReport,
) -> Result<GapSelection, String> {
    let gap_count = report.gaps.len();
    match mode {
        GapSelectionMode::All => Ok(GapSelection::all(gap_count)),
        GapSelectionMode::Only(tokens) => {
            let selected = parse_gap_number_tokens(tokens, gap_count)?;
            if selected.is_empty() {
                // Empty report: the empty set *is* the full selection — do not fail write/preview.
                // Non-empty report + empty only-list: user named nothing → error (§3).
                if gap_count == 0 {
                    return Ok(GapSelection {
                        selected,
                        filter: Some(GapSelectionFilter {
                            kind: "only-gaps",
                            tokens: tokens.iter().map(|t| t.trim().to_string()).collect(),
                        }),
                    });
                }
                return Err("gap selection matched no gaps (only-gaps listed nothing)".to_string());
            }
            Ok(GapSelection {
                selected,
                filter: Some(GapSelectionFilter {
                    kind: "only-gaps",
                    tokens: tokens.iter().map(|t| t.trim().to_string()).collect(),
                }),
            })
        }
        GapSelectionMode::Skip(tokens) => {
            let skip = parse_gap_number_tokens(tokens, gap_count)?;
            let selected: HashSet<usize> = (0..gap_count).filter(|i| !skip.contains(i)).collect();
            if selected.is_empty() {
                // Empty report + skip-nothing: same as All — succeed. Non-empty report where every
                // gap was excluded: user error (§3).
                if gap_count == 0 {
                    return Ok(GapSelection {
                        selected,
                        filter: Some(GapSelectionFilter {
                            kind: "skip-gaps",
                            tokens: tokens.iter().map(|t| t.trim().to_string()).collect(),
                        }),
                    });
                }
                return Err(
                    "skip-gaps excluded every detected gap (nothing left to select)".to_string(),
                );
            }
            Ok(GapSelection {
                selected,
                filter: Some(GapSelectionFilter {
                    kind: "skip-gaps",
                    tokens: tokens.iter().map(|t| t.trim().to_string()).collect(),
                }),
            })
        }
    }
}

fn parse_gap_number_tokens(tokens: &[String], gap_count: usize) -> Result<HashSet<usize>, String> {
    let mut selected = HashSet::new();
    let mut seen_numbers = HashSet::new();
    for raw in tokens {
        let token = raw.trim();
        if token.is_empty() {
            return Err(format!(
                "invalid gap selection token {raw:?}: expected a gap number"
            ));
        }
        let n: usize = token
            .parse()
            .map_err(|_| format!("invalid gap selection token {token:?}: expected a gap number"))?;
        // Bounds before duplicate: `--only-gaps 9,9` on a 3-gap report must say out-of-range, not
        // "duplicate 9".
        let index = match n {
            0 => {
                return Err("gap number 0 is invalid (gap numbers are 1-based)".to_string());
            }
            n if n > gap_count => {
                return Err(format!(
                    "gap number {n} out of range ({gap_count} gaps detected)"
                ));
            }
            n => n - 1,
        };
        if !seen_numbers.insert(n) {
            return Err(format!("duplicate gap number {n} in selection"));
        }
        selected.insert(index);
    }
    Ok(selected)
}

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
    selection: &GapSelection,
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

        // Equivalence gate (after fillable + coverage; selection is lower still — plan §5.2): drop
        // gaps whose silence is already equivalent to B's (mutual/ambient silence), so the
        // decode/patch path is never entered for them. Only when `skip_equivalent_gaps`; the
        // classification is advisory otherwise.
        if skip_equivalent_gaps && report.gap_equivalence_at(index).is_some_and(|v| v.drop) {
            skipped.push(GapFillSkipped {
                gap_index: index,
                a_start_secs: g.video_a_start_secs,
                a_end_secs: g.video_a_end_secs,
                reason: GapFillSkipReason::AlreadyMatchesReference,
            });
            continue;
        }

        if !selection.is_selected(index) {
            skipped.push(GapFillSkipped {
                gap_index: index,
                a_start_secs: g.video_a_start_secs,
                a_end_secs: g.video_a_end_secs,
                reason: GapFillSkipReason::GapNotSelected,
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

/// Stderr note when `--only-gaps` / `--skip-gaps` narrows the fill plan.
pub(crate) fn format_gap_selection_filter_note(
    selection: &GapSelection,
    gap_count: usize,
) -> Option<String> {
    if !selection.is_filtered(gap_count) {
        return None;
    }
    let filter = selection.filter.as_ref()?;
    let n = selection.selected.len();
    let token_list = filter.tokens.join(",");
    Some(format!(
        "Gap filter: selected {n} of {gap_count} detected gaps ({}: {token_list})",
        filter.kind
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
        let plan = build_gap_fill_plan(&report, 10, false, &GapSelection::all(report.gaps.len()));
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
        let plan = build_gap_fill_plan(&report, 10, false, &GapSelection::all(report.gaps.len()));
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
        let plan = build_gap_fill_plan(&report, 10, false, &GapSelection::all(report.gaps.len()));
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
        let plan = build_gap_fill_plan(&report, 0, false, &GapSelection::all(report.gaps.len()));
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

        let plan = build_gap_fill_plan(&report, 0, false, &GapSelection::all(report.gaps.len()));
        assert_eq!(plan.regions.len(), 1);
        assert!((plan.regions[0].a_start_secs - 1.0).abs() < 0.001);
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(
            plan.skipped[0].reason,
            GapFillSkipReason::OutsideReferenceCoverage
        );

        // With the mapped-region gate disabled, both gaps are planned.
        report.limit_fill_to_mapped_region = false;
        let plan_all =
            build_gap_fill_plan(&report, 0, false, &GapSelection::all(report.gaps.len()));
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
        let plan_off =
            build_gap_fill_plan(&report, 0, false, &GapSelection::all(report.gaps.len()));
        assert_eq!(plan_off.regions.len(), 2);
        assert!(plan_off.skipped.is_empty());

        // Flag on: the dropping gap is skipped as AlreadyMatchesReference; the other is still planned.
        let plan_on = build_gap_fill_plan(&report, 0, true, &GapSelection::all(report.gaps.len()));
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
        let plan = build_gap_fill_plan(&report, 0, true, &GapSelection::all(report.gaps.len()));
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
        let plan = build_gap_fill_plan(&report, 0, false, &GapSelection::all(report.gaps.len()));
        let line = super::format_align_fill_regions_phase(&plan);
        assert!(line.contains("Skipping 1 gap(s) at fill plan (1 unfillable)"));
        assert!(line.contains("aligning 1 fill region(s)"));
    }

    #[test]
    fn resolve_only_gaps_is_order_insensitive_and_one_based() {
        let report = base_report(
            Some(stereo_identical()),
            vec![
                fillable_gap(0.0, 1.0),
                fillable_gap(2.0, 3.0),
                fillable_gap(4.0, 5.0),
            ],
        );
        let a = resolve_gap_selection(
            &GapSelectionMode::Only(vec!["5".into(), "2".into()]),
            &report,
        );
        // 5 is out of range for 3 gaps
        assert!(a.is_err());

        let sel = resolve_gap_selection(
            &GapSelectionMode::Only(vec!["3".into(), "1".into()]),
            &report,
        )
        .expect("in range");
        assert!(sel.is_selected(0));
        assert!(!sel.is_selected(1));
        assert!(sel.is_selected(2));
        assert!(sel.is_filtered(3));

        let sel2 = resolve_gap_selection(
            &GapSelectionMode::Only(vec!["1".into(), "3".into()]),
            &report,
        )
        .expect("same set");
        assert_eq!(sel.selected, sel2.selected);
    }

    #[test]
    fn resolve_rejects_zero_out_of_range_duplicates_and_empty_only() {
        let report = base_report(Some(stereo_identical()), vec![fillable_gap(0.0, 1.0)]);
        assert_eq!(
            resolve_gap_selection(&GapSelectionMode::Only(vec!["0".into()]), &report).unwrap_err(),
            "gap number 0 is invalid (gap numbers are 1-based)"
        );
        assert_eq!(
            resolve_gap_selection(&GapSelectionMode::Only(vec!["2".into()]), &report).unwrap_err(),
            "gap number 2 out of range (1 gaps detected)"
        );
        assert_eq!(
            resolve_gap_selection(
                &GapSelectionMode::Only(vec!["1".into(), "1".into()]),
                &report
            )
            .unwrap_err(),
            "duplicate gap number 1 in selection"
        );
        assert!(resolve_gap_selection(&GapSelectionMode::Only(vec![]), &report).is_err());
    }

    #[test]
    fn empty_report_allows_vacuous_only_and_skip_selection() {
        let report = base_report(Some(stereo_identical()), vec![]);
        let only = resolve_gap_selection(&GapSelectionMode::Only(vec![]), &report)
            .expect("empty only-list on empty report");
        assert!(!only.is_filtered(0));
        let skip = resolve_gap_selection(&GapSelectionMode::Skip(vec![]), &report)
            .expect("empty skip-list on empty report");
        assert!(!skip.is_filtered(0));
        // Named tokens still fail bounds against zero gaps.
        assert_eq!(
            resolve_gap_selection(&GapSelectionMode::Skip(vec!["1".into()]), &report).unwrap_err(),
            "gap number 1 out of range (0 gaps detected)"
        );
    }

    #[test]
    fn selection_skips_unselected_after_equivalence_and_fillability() {
        use crate::domain::gap_equivalence::{classify_gap_equivalence, GapEquivalenceParams};

        let on = GapEquivalenceParams {
            enabled: true,
            ..Default::default()
        };
        let mut report = base_report(
            Some(stereo_identical()),
            vec![
                fillable_gap(0.0, 1.0),
                fillable_gap(2.0, 3.0),
                fillable_gap(4.0, 5.0),
            ],
        );
        report.gap_equivalence = vec![
            classify_gap_equivalence(Some(-108.0), Some(-46.0), Some(1.0), &on), // drop
            classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.0), &on), // keep
            classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.0), &on), // keep
        ];

        let selection = resolve_gap_selection(
            &GapSelectionMode::Only(vec!["1".into(), "2".into()]),
            &report,
        )
        .unwrap();
        // Equivalence on + drop on gap 0: AlreadyMatchesReference beats GapNotSelected.
        let plan = build_gap_fill_plan(&report, 0, true, &selection);
        assert_eq!(plan.regions.len(), 1);
        assert_eq!(plan.regions[0].gap_index, 1);
        assert!(plan
            .skipped
            .iter()
            .any(|s| s.gap_index == 0 && s.reason == GapFillSkipReason::AlreadyMatchesReference));
        assert!(plan
            .skipped
            .iter()
            .any(|s| s.gap_index == 2 && s.reason == GapFillSkipReason::GapNotSelected));
    }

    #[test]
    fn plan_block_arm_ignores_selection() {
        let report = base_report(Some(stereo_mismatch()), vec![fillable_gap(0.0, 1.0)]);
        let selection =
            resolve_gap_selection(&GapSelectionMode::Only(vec!["1".into()]), &report).unwrap();
        let plan = build_gap_fill_plan(&report, 0, false, &selection);
        assert!(plan.regions.is_empty());
        assert_eq!(
            plan.skipped[0].reason,
            GapFillSkipReason::TrackLayoutMismatch
        );
    }

    #[test]
    fn resolve_skip_gaps_is_complement_of_only() {
        let report = base_report(
            Some(stereo_identical()),
            vec![
                fillable_gap(0.0, 1.0),
                fillable_gap(2.0, 3.0),
                fillable_gap(4.0, 5.0),
            ],
        );
        let only = resolve_gap_selection(
            &GapSelectionMode::Only(vec!["1".into(), "3".into()]),
            &report,
        )
        .expect("only");
        let skip = resolve_gap_selection(&GapSelectionMode::Skip(vec!["2".into()]), &report)
            .expect("skip");
        for i in 0..3 {
            assert_eq!(
                only.is_selected(i),
                skip.is_selected(i),
                "index {i} must match between only and skip complement"
            );
        }
        assert!(skip.is_selected(0));
        assert!(!skip.is_selected(1));
        assert!(skip.is_selected(2));
    }

    #[test]
    fn empty_only_errors_but_empty_skip_selects_all() {
        let report = base_report(
            Some(stereo_identical()),
            vec![fillable_gap(0.0, 1.0), fillable_gap(2.0, 3.0)],
        );
        assert_eq!(
            resolve_gap_selection(&GapSelectionMode::Only(vec![]), &report).unwrap_err(),
            "gap selection matched no gaps (only-gaps listed nothing)"
        );
        let skip_nothing =
            resolve_gap_selection(&GapSelectionMode::Skip(vec![]), &report).expect("skip empty");
        assert!(skip_nothing.is_selected(0));
        assert!(skip_nothing.is_selected(1));
        assert!(!skip_nothing.is_filtered(2));
    }

    #[test]
    fn skip_all_gaps_errors_with_exclusion_wording() {
        let report = base_report(
            Some(stereo_identical()),
            vec![
                fillable_gap(0.0, 1.0),
                fillable_gap(2.0, 3.0),
                fillable_gap(4.0, 5.0),
            ],
        );
        assert_eq!(
            resolve_gap_selection(
                &GapSelectionMode::Skip(vec!["1".into(), "2".into(), "3".into()]),
                &report
            )
            .unwrap_err(),
            "skip-gaps excluded every detected gap (nothing left to select)"
        );
    }

    #[test]
    fn out_of_range_duplicate_prefers_bounds_error() {
        let report = base_report(
            Some(stereo_identical()),
            vec![
                fillable_gap(0.0, 1.0),
                fillable_gap(2.0, 3.0),
                fillable_gap(4.0, 5.0),
            ],
        );
        assert_eq!(
            resolve_gap_selection(
                &GapSelectionMode::Only(vec!["9".into(), "9".into()]),
                &report
            )
            .unwrap_err(),
            "gap number 9 out of range (3 gaps detected)"
        );
    }

    #[test]
    fn filter_note_fires_when_subset_and_silent_when_all() {
        let report = base_report(
            Some(stereo_identical()),
            vec![
                fillable_gap(0.0, 1.0),
                fillable_gap(2.0, 3.0),
                fillable_gap(4.0, 5.0),
            ],
        );
        let subset =
            resolve_gap_selection(&GapSelectionMode::Only(vec!["2".into()]), &report).unwrap();
        let note = format_gap_selection_filter_note(&subset, 3).expect("filtered");
        assert!(note.contains("selected 1 of 3"), "{note}");
        assert!(note.contains("only-gaps: 2"), "{note}");

        let all_named = resolve_gap_selection(
            &GapSelectionMode::Only(vec!["1".into(), "2".into(), "3".into()]),
            &report,
        )
        .unwrap();
        assert!(format_gap_selection_filter_note(&all_named, 3).is_none());

        let skip_nothing = resolve_gap_selection(&GapSelectionMode::Skip(vec![]), &report).unwrap();
        assert!(format_gap_selection_filter_note(&skip_nothing, 3).is_none());
    }
}
