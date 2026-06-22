//! Repair speed/quality profiles and fit-mode boundary search policy.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Copy, Default)]
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

pub fn format_repair_profile_verbose(
    profile: RepairProfile,
    fit_boundary_search: FitBoundarySearch,
    fill_border_search_secs: f64,
) -> String {
    format!(
        "repair profile: {profile} (fit_boundary_search={fit_boundary_search}, fill_border_search_secs={fill_border_search_secs:.1})"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
