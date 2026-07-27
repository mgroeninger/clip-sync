//! Repair speed/quality profiles and fit-mode boundary search policy.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::domain::fill_mode::FillMode;
use crate::domain::gap_anchor_seam::AnchorSeamMode;
use crate::domain::FillOffsetMode;

/// Named repair preset: bundles haystack size, extension flags, and fit boundary search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepairProfile {
    /// Interactive default: marginal baseline patches without boundary grid.
    #[default]
    Default,
    /// Draft mux: smaller haystack, no extension, baseline-only fit path.
    Quick,
    /// Quality pass: full boundary grid and extension retries.
    Full,
}

impl RepairProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            RepairProfile::Default => "default",
            RepairProfile::Quick => "quick",
            RepairProfile::Full => "full",
        }
    }
}

impl std::fmt::Display for RepairProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RepairProfile {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "default" => Ok(RepairProfile::Default),
            "quick" => Ok(RepairProfile::Quick),
            "full" => Ok(RepairProfile::Full),
            _ => Err(format!(
                "invalid repair profile: {s} (expected default, quick, or full)"
            )),
        }
    }
}

/// Whether fit mode runs the joint A-boundary grid after baseline unified search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitBoundarySearch {
    /// Accept High or Marginal baseline; skip boundary grid.
    #[default]
    BaselineOnly,
    /// Run full boundary grid when baseline is not High (legacy behavior).
    FullGrid,
}

impl FitBoundarySearch {
    pub fn as_str(self) -> &'static str {
        match self {
            FitBoundarySearch::BaselineOnly => "baseline_only",
            FitBoundarySearch::FullGrid => "full_grid",
        }
    }
}

impl std::fmt::Display for FitBoundarySearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FitBoundarySearch {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "baseline_only" | "baseline-only" => Ok(FitBoundarySearch::BaselineOnly),
            "full_grid" | "full-grid" | "full" => Ok(FitBoundarySearch::FullGrid),
            _ => Err(format!(
                "invalid fit_boundary_search: {s} (expected baseline_only or full_grid)"
            )),
        }
    }
}

/// Which profile-bundle fields were set explicitly (TOML or CLI) and must not be overwritten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RepairProfileFieldMask {
    pub fill_border_search_secs: bool,
    pub gap_end_extend_on_post_seam_fail: bool,
    pub gap_start_extend_on_pre_seam_fail: bool,
    pub fit_boundary_search: bool,
}

impl RepairProfileFieldMask {
    pub fn all() -> Self {
        Self {
            fill_border_search_secs: true,
            gap_end_extend_on_post_seam_fail: true,
            gap_start_extend_on_pre_seam_fail: true,
            fit_boundary_search: true,
        }
    }
}

/// Values applied by each profile before per-field overrides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepairProfileBundle {
    pub fill_border_search_secs: f64,
    pub gap_end_extend_on_post_seam_fail: bool,
    pub gap_start_extend_on_pre_seam_fail: bool,
    pub fit_boundary_search: FitBoundarySearch,
}

impl RepairProfile {
    pub fn bundle(self) -> RepairProfileBundle {
        match self {
            RepairProfile::Default => RepairProfileBundle {
                fill_border_search_secs: 10.0,
                gap_end_extend_on_post_seam_fail: true,
                gap_start_extend_on_pre_seam_fail: true,
                fit_boundary_search: FitBoundarySearch::BaselineOnly,
            },
            RepairProfile::Quick => RepairProfileBundle {
                fill_border_search_secs: 5.0,
                gap_end_extend_on_post_seam_fail: false,
                gap_start_extend_on_pre_seam_fail: false,
                fit_boundary_search: FitBoundarySearch::BaselineOnly,
            },
            RepairProfile::Full => RepairProfileBundle {
                fill_border_search_secs: 10.0,
                gap_end_extend_on_post_seam_fail: true,
                gap_start_extend_on_pre_seam_fail: true,
                fit_boundary_search: FitBoundarySearch::FullGrid,
            },
        }
    }
}

pub fn repair_profile_override_notes(
    profile: RepairProfile,
    fill_border_search_secs: f64,
    gap_end_extend_on_post_seam_fail: bool,
    gap_start_extend_on_pre_seam_fail: bool,
    fit_boundary_search: FitBoundarySearch,
) -> Vec<String> {
    let bundle = profile.bundle();
    let mut notes = Vec::new();
    if (fill_border_search_secs - bundle.fill_border_search_secs).abs() > f64::EPSILON {
        notes.push(format!(
            "fill_border_search_secs={fill_border_search_secs:.1}"
        ));
    }
    if gap_end_extend_on_post_seam_fail != bundle.gap_end_extend_on_post_seam_fail {
        notes.push(format!(
            "gap_end_extend_on_post_seam_fail={gap_end_extend_on_post_seam_fail}"
        ));
    }
    if gap_start_extend_on_pre_seam_fail != bundle.gap_start_extend_on_pre_seam_fail {
        notes.push(format!(
            "gap_start_extend_on_pre_seam_fail={gap_start_extend_on_pre_seam_fail}"
        ));
    }
    if fit_boundary_search != bundle.fit_boundary_search {
        notes.push(format!("fit_boundary_search={fit_boundary_search}"));
    }
    notes
}

pub fn format_repair_profile_verbose(
    profile: RepairProfile,
    fit_boundary_search: FitBoundarySearch,
    fill_border_search_secs: f64,
    gap_end_extend_on_post_seam_fail: bool,
    gap_start_extend_on_pre_seam_fail: bool,
) -> String {
    let base = format!(
        "repair profile: {profile} (fit_boundary_search={fit_boundary_search}, fill_border_search_secs={fill_border_search_secs:.1})"
    );
    let overrides = repair_profile_override_notes(
        profile,
        fill_border_search_secs,
        gap_end_extend_on_post_seam_fail,
        gap_start_extend_on_pre_seam_fail,
        fit_boundary_search,
    );
    if overrides.is_empty() {
        base
    } else {
        format!("{base} (+ override: {})", overrides.join(", "))
    }
}

/// Subset of patch config used to explain flags that are stored but inactive.
#[derive(Debug, Clone, Copy)]
pub struct RepairPatchConfigView {
    pub fill_mode: FillMode,
    pub fit_boundary_search: FitBoundarySearch,
    pub gap_end_extend_on_post_seam_fail: bool,
    pub gap_start_extend_on_pre_seam_fail: bool,
    pub gap_end_extend_max_ms: u64,
    pub disable_structure_trust: bool,
    pub short_gap_one_strong_seam_fallback: bool,
    pub fill_anchor_search_prior_weight: f64,
    pub fill_anchor_retry_marginal: bool,
    pub fill_offset_mode: FillOffsetMode,
    pub anchor_seam_mode: AnchorSeamMode,
}

/// Whether fit mode may run the joint A-boundary grid (extension axes).
pub fn boundary_grid_may_run(view: RepairPatchConfigView) -> bool {
    view.fill_mode == FillMode::Fit
        && view.fit_boundary_search == FitBoundarySearch::FullGrid
        && (view.gap_end_extend_on_post_seam_fail || view.gap_start_extend_on_pre_seam_fail)
}

/// Extra B extract padding when A-boundary extension can shift the mapped bracket.
pub fn gap_extension_slack_secs(view: RepairPatchConfigView) -> f64 {
    if view.gap_end_extend_max_ms == 0 {
        return 0.0;
    }
    let extension_enabled =
        view.gap_end_extend_on_post_seam_fail || view.gap_start_extend_on_pre_seam_fail;
    if !extension_enabled {
        return 0.0;
    }
    match view.fill_mode {
        FillMode::Fit => {
            if view.fit_boundary_search == FitBoundarySearch::FullGrid {
                view.gap_end_extend_max_ms as f64 / 1000.0
            } else {
                0.0
            }
        }
        FillMode::Gate => view.gap_end_extend_max_ms as f64 / 1000.0,
    }
}

/// Human-readable notes for `-v` when stored flags do not affect this run.
pub fn inactive_repair_flag_notes(view: RepairPatchConfigView) -> Vec<String> {
    let mut notes = Vec::new();

    if view.fill_mode == FillMode::Fit {
        if view.disable_structure_trust {
            notes.push(
                "no-structure-trust: no effect with fill_mode=fit (use --fill-mode gate)".into(),
            );
        }
        if !view.short_gap_one_strong_seam_fallback {
            notes.push("no-short-gap-one-strong-seam: no effect with fill_mode=fit".into());
        }

        let grid_may_run = boundary_grid_may_run(view);
        let extension_any =
            view.gap_end_extend_on_post_seam_fail || view.gap_start_extend_on_pre_seam_fail;

        if !grid_may_run {
            if extension_any || view.gap_end_extend_max_ms > 0 {
                notes.push(
                    "gap_end/start_extend_*: boundary grid and haystack slack inactive \
                     (fit_boundary_search=baseline_only); use --full or fit_boundary_search=full_grid"
                        .into(),
                );
            }
        } else if !extension_any {
            notes.push(
                "boundary grid: disabled (--no-gap-end-extend / --no-gap-start-extend); \
                 only baseline bracket evaluated under --full"
                    .into(),
            );
        }

        if view.fill_anchor_retry_marginal && view.fill_offset_mode != FillOffsetMode::AnchoredRetry
        {
            notes.push(
                "fill-anchor-retry-marginal: only applies with --fill-offset anchored-retry".into(),
            );
        }
        if view.fill_anchor_search_prior_weight > 0.0
            && view.fill_offset_mode != FillOffsetMode::AnchoredRetry
        {
            notes.push(
                "fill-anchor-search-prior-weight: only applies with --fill-offset anchored-retry"
                    .into(),
            );
        }
        if view.anchor_seam_mode == AnchorSeamMode::Off {
            notes.push(
                "anchor_seam_mode=off: editorial anchor search inactive; use --anchor-seam-mode auto|force"
                    .into(),
            );
        }
    }

    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_view() -> RepairPatchConfigView {
        RepairPatchConfigView {
            fill_mode: FillMode::Fit,
            fit_boundary_search: FitBoundarySearch::BaselineOnly,
            gap_end_extend_on_post_seam_fail: true,
            gap_start_extend_on_pre_seam_fail: true,
            gap_end_extend_max_ms: 500,
            disable_structure_trust: false,
            short_gap_one_strong_seam_fallback: true,
            fill_anchor_search_prior_weight: 0.0,
            fill_anchor_retry_marginal: false,
            fill_offset_mode: FillOffsetMode::Recommended,
            anchor_seam_mode: AnchorSeamMode::default(),
        }
    }

    #[test]
    fn extension_slack_zero_under_baseline_only_fit() {
        assert!((gap_extension_slack_secs(default_view()) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn extension_slack_applies_under_full_grid_fit() {
        let mut view = default_view();
        view.fit_boundary_search = FitBoundarySearch::FullGrid;
        assert!((gap_extension_slack_secs(view) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn extension_slack_applies_under_gate_retries() {
        let mut view = default_view();
        view.fill_mode = FillMode::Gate;
        assert!((gap_extension_slack_secs(view) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn inactive_notes_warn_extension_latent_under_baseline_only() {
        let notes = inactive_repair_flag_notes(default_view());
        assert!(notes.iter().any(|n| n.contains("baseline_only")));
    }

    #[test]
    fn inactive_notes_gate_only_flags_under_fit() {
        let mut view = default_view();
        view.disable_structure_trust = true;
        let notes = inactive_repair_flag_notes(view);
        assert!(notes.iter().any(|n| n.contains("no-structure-trust")));
    }

    #[test]
    fn profile_bundle_quick_sets_border_five_and_no_extension() {
        let b = RepairProfile::Quick.bundle();
        assert!((b.fill_border_search_secs - 5.0).abs() < f64::EPSILON);
        assert!(!b.gap_end_extend_on_post_seam_fail);
        assert!(!b.gap_start_extend_on_pre_seam_fail);
        assert_eq!(b.fit_boundary_search, FitBoundarySearch::BaselineOnly);
    }

    #[test]
    fn profile_bundle_full_enables_grid() {
        let b = RepairProfile::Full.bundle();
        assert_eq!(b.fit_boundary_search, FitBoundarySearch::FullGrid);
    }

    #[test]
    fn repair_profile_from_str() {
        assert_eq!(
            "QUICK".parse::<RepairProfile>().unwrap(),
            RepairProfile::Quick
        );
    }

    #[test]
    fn format_repair_profile_verbose_lists_overrides() {
        let line = format_repair_profile_verbose(
            RepairProfile::Quick,
            FitBoundarySearch::BaselineOnly,
            8.0,
            false,
            false,
        );
        assert!(line.contains("repair profile: quick"));
        assert!(line.contains("+ override: fill_border_search_secs=8.0"));
    }

    #[test]
    fn repair_profile_override_notes_empty_when_bundle_matches() {
        let bundle = RepairProfile::Default.bundle();
        assert!(repair_profile_override_notes(
            RepairProfile::Default,
            bundle.fill_border_search_secs,
            bundle.gap_end_extend_on_post_seam_fail,
            bundle.gap_start_extend_on_pre_seam_fail,
            bundle.fit_boundary_search,
        )
        .is_empty());
    }
}
