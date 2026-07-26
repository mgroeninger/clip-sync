//! Perf §4 decision-invariance golden baseline — emit, parse, and compare D/R axis snapshots.
//! See `docs/dev/archive/TEMP-pipeline-perf-redesign-plan.md` §4 and `golden/README.md`.

use serde::{Deserialize, Serialize};

use crate::gap_fingerprint_corpus::{CorpusReport, GapRow};

/// Tier-2 continuous-field tolerance. FFT refactors may drift at ~1e-10; identical corpus replays are exact.
pub const TIER2_ABS_EPS: f64 = 1e-9;

/// Top-level golden-baseline document (see [`CorpusReport::golden_baseline`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenBaseline {
    pub schema: String,
    pub gap_count: usize,
    /// Fix-list additions — gaps dual-fit would newly patch (perf §4.1).
    pub dualfit_targets: Vec<String>,
    pub gaps: Vec<GoldenRecord>,
}

/// One gap's decision/repair (D/R) coordinates. Nulls preserve shape for clean diffs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenRecord {
    pub pair: String,
    pub index: usize,
    // --- Tier 1: bit-exact ---
    pub tier: Option<String>,
    pub patched: bool,
    pub dualfit_target: bool,
    pub program_quiet_skip: bool,
    pub bracket_exhausted: bool,
    pub gate_pass: Option<bool>,
    pub step_edge_pinned: bool,
    pub aligned_donor_continuous: Option<bool>,
    pub nominal_donor_continuous: Option<bool>,
    pub brackets_total: usize,
    pub brackets_passing: usize,
    /// Fill placement of the closest-to-chosen bracket. **Tier 1** — these are frame indices, exact
    /// integers, not continuous scores, so any drift is a real decision change. Added 2026-07-25 to
    /// close the blind spot where an end-search change could move every fill length on the corpus
    /// and leave this diff green (docs/dev/TEMP-fill-placement-axis-plan.md, Phase A).
    pub fill_start_frame: Option<usize>,
    pub fill_frames: Option<usize>,
    // --- Tier 2: within ε ---
    /// Seam correlations at that placement, unaggregated — a placement change that trades one
    /// shoulder for the other moves these while `throat_structure_min` stays put.
    pub fill_pre_r: Option<f64>,
    pub fill_post_r: Option<f64>,
    pub gross_frac_lag_pre_ms: Option<f64>,
    pub gross_frac_lag_post_ms: Option<f64>,
    pub gross_peak_r_pre: Option<f64>,
    pub gross_peak_r_post: Option<f64>,
    pub gross_step_ms: Option<f64>,
    pub gross_uniqueness_z: Option<f64>,
    pub gross_uniqueness_prom: Option<f64>,
    pub seamlocal_pre_seam_r: Option<f64>,
    pub seamlocal_post_seam_r: Option<f64>,
    pub seamlocal_post_global_r: Option<f64>,
    pub seamlocal_seam_prom: Option<f64>,
    pub nominal_donor_silence: Option<f64>,
    pub aligned_donor_silence: Option<f64>,
    pub aligned_donor_rms_db: Option<f64>,
    pub throat_residual_headroom_db: Option<f64>,
    pub throat_structure_min: Option<f64>,
    pub a_gap_floor_db: Option<f64>,
    pub a_noise_floor_db: Option<f64>,
    pub b_gap_floor_db: Option<f64>,
    pub b_noise_floor_db: Option<f64>,
}

/// One gap's golden record, from its analyzed [`GapRow`] (reuses the analyzer predicates).
fn record_from_row(r: &GapRow) -> GoldenRecord {
    GoldenRecord {
        pair: r.pair.clone(),
        index: r.index,
        tier: r.outcome_tier.clone(),
        patched: r.patched(),
        dualfit_target: r.dualfit_target(),
        program_quiet_skip: r.program_quiet_skip(),
        bracket_exhausted: r.bracket_exhausted(),
        gate_pass: r.dualfit_pass,
        step_edge_pinned: r.step_edge_pinned(),
        aligned_donor_continuous: r.donor_continuous,
        nominal_donor_continuous: r.donor_nominal_cont,
        brackets_total: r.brackets_total,
        brackets_passing: r.brackets_passing,
        fill_start_frame: r.best_bracket_start_frame,
        fill_frames: r.best_bracket_fill_frames,
        fill_pre_r: r.best_bracket_seam_pre,
        fill_post_r: r.best_bracket_seam_post,
        gross_frac_lag_pre_ms: r.frac_lag_pre_ms,
        gross_frac_lag_post_ms: r.frac_lag_post_ms,
        gross_peak_r_pre: r.peak_r_pre,
        gross_peak_r_post: r.peak_r_post,
        gross_step_ms: r.seam_step_ms(),
        gross_uniqueness_z: r.uniqueness_z,
        gross_uniqueness_prom: r.uniqueness_prom,
        seamlocal_pre_seam_r: r.dualfit_pre_r,
        seamlocal_post_seam_r: r.dualfit_post_r,
        seamlocal_post_global_r: r.dualfit_post_global_r,
        seamlocal_seam_prom: r.dualfit_seam_prom,
        nominal_donor_silence: r.donor_nominal_silence,
        aligned_donor_silence: r.donor_aligned_silence,
        aligned_donor_rms_db: r.donor_rms_db,
        throat_residual_headroom_db: r.residual_headroom_db,
        throat_structure_min: r.structure_min,
        a_gap_floor_db: r.a_gap_floor_db,
        a_noise_floor_db: r.a_noise_floor_db,
        b_gap_floor_db: r.b_gap_floor_db,
        b_noise_floor_db: r.b_noise_floor_db,
    }
}

/// Build a golden snapshot from an explicit set of analyzed rows (every row included, in order). The
/// curated per-gap-type fixture path uses this to snapshot a set whose rows carry unique `pair` labels;
/// [`baseline_from_report`] feeds it the corpus's matched rows.
pub fn baseline_from_rows<'a>(rows: impl IntoIterator<Item = &'a GapRow>) -> GoldenBaseline {
    let gaps: Vec<GoldenRecord> = rows.into_iter().map(record_from_row).collect();
    GoldenBaseline {
        schema: "perf §4 golden baseline. Tier-1 (tier/patched/*target/*quiet/*exhausted/gate_pass/\
                 *continuous/edge_pinned/brackets_*/fill_start_frame/fill_frames) assert BIT-EXACT. \
                 Tier-2 (gross_*/seamlocal_*/nominal_*/aligned_*/throat_*/fill_*_r continuous) assert \
                 WITHIN ε. \
                 Tier-3 diagnostics (seam_probe/wide_envelope/b_levels/fp.lag) omitted. Field prefix = \
                 measurement placement. fill_* are frame indices (integers) — Tier-1, not Tier-2."
            .into(),
        gap_count: gaps.len(),
        dualfit_targets: gaps
            .iter()
            .filter(|g| g.dualfit_target)
            .map(|g| format!("{}·g{}", g.pair, g.index))
            .collect(),
        gaps,
    }
}

impl GoldenBaseline {
    /// This snapshot with the `fill_*` placement axes nulled on every record.
    ///
    /// Placement (`fill_start_frame` / `fill_frames` and the seam correlations read at it) is an
    /// **output of the bracket search**, not a decision input: no gate, tier, or verdict reads it.
    /// `GapRepairTags` therefore does not carry it, and the media-free projection differential —
    /// whose contract is *tags are a complete decision carrier* — must not assert on it. The
    /// tripwire for placement lives on the **media corpus** golden ([`baseline_from_report`]),
    /// where the values are measured rather than reconstructed.
    pub fn without_placement(mut self) -> Self {
        for g in &mut self.gaps {
            g.fill_start_frame = None;
            g.fill_frames = None;
            g.fill_pre_r = None;
            g.fill_post_r = None;
        }
        self
    }
}

/// Build the golden snapshot from an analyzed corpus (its matched rows).
pub fn baseline_from_report(report: &CorpusReport) -> GoldenBaseline {
    baseline_from_rows(report.matched())
}

pub fn parse_golden_baseline(json: &str) -> Result<GoldenBaseline, serde_json::Error> {
    serde_json::from_str(json)
}

pub fn golden_json(report: &CorpusReport) -> String {
    serde_json::to_string_pretty(&baseline_from_report(report))
        .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
}

fn gap_key(pair: &str, index: usize) -> String {
    format!("{pair}·g{index}")
}

/// Compare `actual` to `expected`. Returns all mismatches (tier-1 first, then tier-2).
pub fn diff_baselines(expected: &GoldenBaseline, actual: &GoldenBaseline, eps: f64) -> Vec<String> {
    let mut errs = Vec::new();
    if expected.gap_count != actual.gap_count {
        errs.push(format!(
            "gap_count: expected {} actual {}",
            expected.gap_count, actual.gap_count
        ));
    }
    if expected.dualfit_targets != actual.dualfit_targets {
        errs.push(format!(
            "dualfit_targets: expected {:?} actual {:?}",
            expected.dualfit_targets, actual.dualfit_targets
        ));
    }

    let mut exp_by_key: std::collections::BTreeMap<String, &GoldenRecord> = std::collections::BTreeMap::new();
    for g in &expected.gaps {
        exp_by_key.insert(gap_key(&g.pair, g.index), g);
    }
    let mut act_by_key: std::collections::BTreeMap<String, &GoldenRecord> = std::collections::BTreeMap::new();
    for g in &actual.gaps {
        act_by_key.insert(gap_key(&g.pair, g.index), g);
    }

    for (key, exp) in &exp_by_key {
        let Some(act) = act_by_key.get(key) else {
            errs.push(format!("missing gap {key} in actual"));
            continue;
        };
        diff_record(key, exp, act, eps, &mut errs);
    }
    for key in act_by_key.keys() {
        if !exp_by_key.contains_key(key) {
            errs.push(format!("unexpected gap {key} in actual"));
        }
    }
    errs
}

fn diff_record(key: &str, exp: &GoldenRecord, act: &GoldenRecord, eps: f64, errs: &mut Vec<String>) {
    macro_rules! tier1 {
        ($field:ident) => {
            if exp.$field != act.$field {
                errs.push(format!("{key}.{}: expected {:?} actual {:?}", stringify!($field), exp.$field, act.$field));
            }
        };
    }
    tier1!(tier);
    tier1!(patched);
    tier1!(dualfit_target);
    tier1!(program_quiet_skip);
    tier1!(bracket_exhausted);
    tier1!(gate_pass);
    tier1!(step_edge_pinned);
    tier1!(aligned_donor_continuous);
    tier1!(nominal_donor_continuous);
    tier1!(brackets_total);
    tier1!(brackets_passing);
    tier1!(fill_start_frame);
    tier1!(fill_frames);

    macro_rules! tier2 {
        ($field:ident) => {
            diff_f64_opt(key, stringify!($field), exp.$field, act.$field, eps, errs);
        };
    }
    tier2!(gross_frac_lag_pre_ms);
    tier2!(gross_frac_lag_post_ms);
    tier2!(gross_peak_r_pre);
    tier2!(gross_peak_r_post);
    tier2!(gross_step_ms);
    tier2!(gross_uniqueness_z);
    tier2!(gross_uniqueness_prom);
    tier2!(seamlocal_pre_seam_r);
    tier2!(seamlocal_post_seam_r);
    tier2!(seamlocal_post_global_r);
    tier2!(seamlocal_seam_prom);
    tier2!(nominal_donor_silence);
    tier2!(aligned_donor_silence);
    tier2!(aligned_donor_rms_db);
    tier2!(throat_residual_headroom_db);
    tier2!(fill_pre_r);
    tier2!(fill_post_r);
    tier2!(throat_structure_min);
    tier2!(a_gap_floor_db);
    tier2!(a_noise_floor_db);
    tier2!(b_gap_floor_db);
    tier2!(b_noise_floor_db);
}

fn diff_f64_opt(key: &str, field: &str, exp: Option<f64>, act: Option<f64>, eps: f64, errs: &mut Vec<String>) {
    match (exp, act) {
        (None, None) => {}
        (Some(e), Some(a)) if (e - a).abs() <= eps => {}
        (e, a) => errs.push(format!("{key}.{field}: expected {e:?} actual {a:?} (eps {eps})")),
    }
}

// The §4.4 footgun guards that lived here (`assert_footguns`, keyed to specific re-anchor gaps) were
// retired in Phase 4 of the gap-fixture-corpus plan: their semantics now live as per-**type** assertions on
// committed, media-independent fixtures in `clip-sync-repair/tests/gap_cell_fixtures.rs` (silence-splice IS a
// dual-fit target; program-quiet with passing seams is NOT), and the frozen target set is pinned by
// `curated.golden.json` via `golden_baseline_invariance`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_catches_tier1_flip() {
        let exp = GoldenBaseline {
            schema: String::new(),
            gap_count: 1,
            dualfit_targets: vec![],
            gaps: vec![GoldenRecord {
                pair: "p/1".into(),
                index: 0,
                tier: Some("skip".into()),
                patched: false,
                dualfit_target: false,
                program_quiet_skip: false,
                bracket_exhausted: true,
                gate_pass: Some(true),
                step_edge_pinned: false,
                aligned_donor_continuous: Some(true),
                nominal_donor_continuous: Some(true),
                brackets_total: 4,
                brackets_passing: 0,
                fill_start_frame: Some(1000),
                fill_frames: Some(4410),
                gross_frac_lag_pre_ms: None,
                gross_frac_lag_post_ms: None,
                gross_peak_r_pre: None,
                gross_peak_r_post: None,
                gross_step_ms: None,
                gross_uniqueness_z: None,
                gross_uniqueness_prom: None,
                seamlocal_pre_seam_r: None,
                seamlocal_post_seam_r: None,
                seamlocal_post_global_r: None,
                seamlocal_seam_prom: None,
                nominal_donor_silence: None,
                aligned_donor_silence: None,
                aligned_donor_rms_db: None,
                throat_residual_headroom_db: None,
                fill_pre_r: Some(0.8),
                fill_post_r: Some(0.7),
                throat_structure_min: None,
                a_gap_floor_db: None,
                a_noise_floor_db: None,
                b_gap_floor_db: None,
                b_noise_floor_db: None,
            }],
        };
        let mut act = exp.clone();
        act.gaps[0].dualfit_target = true;
        let errs = diff_baselines(&exp, &act, TIER2_ABS_EPS);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("dualfit_target"));

        // Phase A exit criterion: a fill-placement change must turn the diff red. Before these two
        // fields existed, an end-search change could move every fill length on the corpus and leave
        // this diff green — the blind spot TEMP-fill-placement-axis-plan.md was written to close.
        // A one-frame move is enough; these are Tier 1, so there is no epsilon to hide in.
        let mut act = exp.clone();
        act.gaps[0].fill_frames = Some(4411);
        let errs = diff_baselines(&exp, &act, TIER2_ABS_EPS);
        assert_eq!(errs.len(), 1, "a 1-frame fill-length change must be caught: {errs:?}");
        assert!(errs[0].contains("fill_frames"));

        let mut act = exp.clone();
        act.gaps[0].fill_start_frame = Some(1001);
        let errs = diff_baselines(&exp, &act, TIER2_ABS_EPS);
        assert_eq!(errs.len(), 1, "a 1-frame placement move must be caught: {errs:?}");
        assert!(errs[0].contains("fill_start_frame"));
    }
}
