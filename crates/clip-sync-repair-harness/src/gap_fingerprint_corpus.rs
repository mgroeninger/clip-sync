//! Cross-corpus aggregation of gap-fingerprint output (the **P0 prevalence scan**, see
//! `docs/TEMP-w5-timing-offset-rescue-plan.md` §5 P0).
//!
//! `--gap-fingerprints DIR` writes one `corpus.json` per A/B pair (all that pair's gaps) plus per-gap
//! library files. Point this at the parent of several such runs (e.g. `gap-files/`, holding `1/`..`6/`)
//! and it tallies, **across every pair**, the lag verdicts and gate outcomes — the numbers P0 needs:
//! how many gaps are `timing_offset` (a recoverable seam the gate skipped), split **constant** vs
//! **drift**, vs genuinely `decorrelated` skips.
//!
//! Parses a **minimal** projection of the schema (ids, index, geometry duration, lag, outcome) so it is
//! robust to unrelated `GapCorpus` schema drift. Prefers each pair dir's `corpus.json` (authoritative,
//! all gaps, no per-gap-file accumulation); falls back to globbing per-gap `*.json` and de-duping by
//! gap index when `corpus.json` is absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

// ── minimal schema projection ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct CorpusFile {
    source: SourceMeta,
    #[serde(default)]
    gaps: Vec<GapEntry>,
}

#[derive(Deserialize)]
struct SourceMeta {
    a_source: FileId,
    b_source: FileId,
}

#[derive(Deserialize)]
struct FileId {
    id: String,
}

#[derive(Deserialize)]
struct GapEntry {
    index: usize,
    #[serde(default)]
    geometry: Option<Geometry>,
    /// Diagnostic lag at the best editorial bracket (may sit far from the decision seam).
    #[serde(default)]
    lag: Option<Lag>,
    /// Lag at the **decision** placement (structure-slid throat) — the registration-relevant one (#2).
    #[serde(default)]
    baseline_lag: Option<Lag>,
    #[serde(default)]
    outcome: Option<Outcome>,
}

#[derive(Deserialize)]
struct Geometry {
    #[serde(default)]
    duration_secs: Option<f64>,
}

#[derive(Deserialize)]
struct Lag {
    #[serde(default)]
    pre_anchor: Vec<LagEntry>,
    #[serde(default)]
    post_anchor: Vec<LagEntry>,
}

#[derive(Deserialize)]
struct LagEntry {
    peak_r: f64,
    frac_lag_ms: f64,
    verdict: String,
}

#[derive(Deserialize)]
struct Outcome {
    #[serde(default)]
    plan_kind: Option<String>,
    tier: String,
    #[serde(default)]
    skip_reason: Option<String>,
}

// ── analysis result ─────────────────────────────────────────────────────────────

/// Constant vs drift classification for a `timing_offset` gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkewClass {
    /// Pre/post seam lags within `eps` → a single shift aligns both seams.
    Constant,
    /// Pre/post seam lags differ by more than `eps` → needs a time-warp (g003-like).
    Drift,
    /// Not a `timing_offset` gap, or lag missing on one side.
    NotApplicable,
}

/// Honest "what is this region" bucket — used so the addressable rate isn't diluted by track tails or
/// gaps with no matchable B seam. Maps onto the gap-repair-guide Layer-1 types where it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapKind {
    /// Long / end-of-file region (guide **P6**): `fillable` but duration ≥ the tail threshold. These
    /// are track-length mismatches / huge silences, not seam-repairable content — excluded from rates.
    Tail,
    /// No lag fingerprint: structure never found a matchable B bracket (genuine content gap, or
    /// structure-align fail — guide **W6/C5**). B carries nothing to time-align at the seam.
    NoLag,
    /// Has a seam lag fingerprint: B carries matchable content at the seam. **The analysis denominator.**
    Matched,
}

impl GapKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GapKind::Tail => "tail",
            GapKind::NoLag => "no-lag",
            GapKind::Matched => "matched",
        }
    }
}

/// One gap's aggregated row.
#[derive(Debug, Clone)]
pub struct GapRow {
    pub pair: String,
    pub a_id: String,
    pub b_id: String,
    pub index: usize,
    pub duration_secs: Option<f64>,
    /// Plan classification from the fingerprint outcome (`fillable` / …). Today the fingerprint only
    /// characterizes `fillable` gaps, so this is uniform — surfaced for validation + future-proofing.
    pub plan_kind: Option<String>,
    /// Region bucket (tail / no-lag / matched) — the honest denominator selector.
    pub kind: GapKind,
    /// Lag verdict (`timing_offset` / `decorrelated` / `ambiguous`), or `None` if no lag fingerprint.
    pub verdict: Option<String>,
    /// Gate outcome tier (`skip`, or a patch tier). `None` if the gap carries no outcome.
    pub outcome_tier: Option<String>,
    pub skip_reason: Option<String>,
    pub frac_lag_pre_ms: Option<f64>,
    pub frac_lag_post_ms: Option<f64>,
    pub peak_r_pre: Option<f64>,
    pub peak_r_post: Option<f64>,
    pub skew: SkewClass,
}

impl GapRow {
    /// Patched = the gate produced a patch tier (anything but `skip`). `None` outcome → not patched.
    pub fn patched(&self) -> bool {
        matches!(&self.outcome_tier, Some(t) if t != "skip")
    }

    /// `|frac_lag_pre − frac_lag_post|` in ms when both are present.
    pub fn drift_ms(&self) -> Option<f64> {
        Some((self.frac_lag_pre_ms? - self.frac_lag_post_ms?).abs())
    }

    /// Largest seam-offset magnitude (ms) seen on either side — recoverability needs this within the
    /// lag search window.
    pub fn max_offset_ms(&self) -> Option<f64> {
        match (self.frac_lag_pre_ms, self.frac_lag_post_ms) {
            (Some(a), Some(b)) => Some(a.abs().max(b.abs())),
            (Some(a), None) => Some(a.abs()),
            (None, Some(b)) => Some(b.abs()),
            (None, None) => None,
        }
    }
}

/// Full aggregation across every pair dir found.
#[derive(Debug, Clone, Default)]
pub struct CorpusReport {
    pub rows: Vec<GapRow>,
    pub pairs: Vec<String>,
    /// `eps` (ms) used for the constant/drift split.
    pub drift_eps_ms: f64,
    /// Duration (s) at/above which a `fillable` gap is bucketed as a `Tail` (guide P6).
    pub tail_secs: f64,
}

const DEFAULT_DRIFT_EPS_MS: f64 = 1.0;
const DEFAULT_TAIL_SECS: f64 = 30.0;

/// Aggregate every A/B-pair `corpus.json` found under `roots` (each root is scanned for its own
/// `corpus.json` and for immediate subdirs that have one). `drift_eps_ms` splits constant vs drift;
/// `tail_secs` is the long-tail (P6) duration cutoff.
pub fn analyze_dirs(roots: &[PathBuf], drift_eps_ms: f64, tail_secs: f64) -> CorpusReport {
    let mut report = CorpusReport {
        drift_eps_ms,
        tail_secs,
        ..Default::default()
    };
    for root in roots {
        for pair_dir in pair_dirs(root) {
            let label = pair_label(root, &pair_dir);
            match read_pair(&pair_dir) {
                Some(file) => {
                    report.pairs.push(label.clone());
                    for gap in &file.gaps {
                        report
                            .rows
                            .push(gap_row(&label, &file.source, gap, drift_eps_ms, tail_secs));
                    }
                }
                None => eprintln!("warn: no parseable corpus in {}", pair_dir.display()),
            }
        }
    }
    report
}

/// A pair dir holds either a `corpus.json` or loose per-gap `*.json` library files. `root` itself
/// counts, as do its immediate subdirs (so `gap-files/` with `1/`..`6/` resolves to six pairs in one
/// call).
fn pair_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if is_pair_dir(root) {
        out.push(root.to_path_buf());
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        let mut subs: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && is_pair_dir(p))
            .collect();
        subs.sort();
        out.extend(subs);
    }
    out
}

/// True if `dir` holds a `corpus.json`, or any non-`manifest` `*.json` (per-gap library files).
fn is_pair_dir(dir: &Path) -> bool {
    if dir.join("corpus.json").is_file() {
        return true;
    }
    std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let p = e.path();
            p.extension().and_then(|x| x.to_str()) == Some("json")
                && p.file_name().and_then(|n| n.to_str()) != Some("manifest.json")
        })
}

fn pair_label(root: &Path, pair_dir: &Path) -> String {
    pair_dir
        .strip_prefix(root.parent().unwrap_or(root))
        .unwrap_or(pair_dir)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Read a pair dir: prefer `corpus.json`; else merge per-gap `*.json`, de-duping by gap index.
fn read_pair(dir: &Path) -> Option<CorpusFile> {
    if let Some(file) = read_corpus_json(&dir.join("corpus.json")) {
        return Some(file);
    }
    // Fallback: per-gap library files (each a single-gap corpus). Dedup by index.
    let mut source: Option<SourceMeta> = None;
    let mut seen: BTreeMap<usize, GapEntry> = BTreeMap::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if p.file_name().and_then(|n| n.to_str()) == Some("manifest.json") {
            continue;
        }
        if let Some(file) = read_corpus_json(&p) {
            source.get_or_insert(file.source);
            for gap in file.gaps {
                seen.entry(gap.index).or_insert(gap);
            }
        }
    }
    Some(CorpusFile {
        source: source?,
        gaps: seen.into_values().collect(),
    })
}

fn read_corpus_json(path: &Path) -> Option<CorpusFile> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn gap_row(pair: &str, source: &SourceMeta, gap: &GapEntry, eps: f64, tail_secs: f64) -> GapRow {
    // Registration is read at the **decision** seam (`baseline_lag`, #2); fall back to the diagnostic
    // best-bracket `lag` for older fingerprints that predate it. Mono summary is written first.
    let lag = gap.baseline_lag.as_ref().or(gap.lag.as_ref());
    let pre = lag.and_then(|l| l.pre_anchor.first());
    let post = lag.and_then(|l| l.post_anchor.first());
    let verdict = pre.or(post).map(|s| s.verdict.clone());
    let frac_lag_pre_ms = pre.map(|s| s.frac_lag_ms);
    let frac_lag_post_ms = post.map(|s| s.frac_lag_ms);
    let duration_secs = gap.geometry.as_ref().and_then(|g| g.duration_secs);
    let has_lag = lag.is_some();

    let is_timing = verdict.as_deref() == Some("timing_offset");
    let skew = match (is_timing, frac_lag_pre_ms, frac_lag_post_ms) {
        (true, Some(a), Some(b)) if (a - b).abs() <= eps => SkewClass::Constant,
        (true, Some(_), Some(_)) => SkewClass::Drift,
        _ => SkewClass::NotApplicable,
    };

    // Tail by duration takes precedence (a P6 region isn't seam-repairable regardless of a lag read).
    let kind = if duration_secs.is_some_and(|d| d >= tail_secs) {
        GapKind::Tail
    } else if has_lag {
        GapKind::Matched
    } else {
        GapKind::NoLag
    };

    GapRow {
        pair: pair.to_string(),
        a_id: source.a_source.id.clone(),
        b_id: source.b_source.id.clone(),
        index: gap.index,
        duration_secs,
        plan_kind: gap.outcome.as_ref().and_then(|o| o.plan_kind.clone()),
        kind,
        verdict,
        outcome_tier: gap.outcome.as_ref().map(|o| o.tier.clone()),
        skip_reason: gap.outcome.as_ref().and_then(|o| o.skip_reason.clone()),
        frac_lag_pre_ms,
        frac_lag_post_ms,
        peak_r_pre: pre.map(|s| s.peak_r),
        peak_r_post: post.map(|s| s.peak_r),
        skew,
    }
}

// ── reporting ───────────────────────────────────────────────────────────────────

fn pct(n: usize, total: usize) -> String {
    if total == 0 {
        "  0.0%".to_string()
    } else {
        format!("{:5.1}%", 100.0 * n as f64 / total as f64)
    }
}

fn stats(mut v: Vec<f64>) -> Option<(f64, f64, f64)> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min = v[0];
    let max = v[v.len() - 1];
    let med = v[v.len() / 2];
    Some((min, med, max))
}

impl CorpusReport {
    pub fn total_gaps(&self) -> usize {
        self.rows.len()
    }

    fn count<F: Fn(&GapRow) -> bool>(&self, f: F) -> usize {
        self.rows.iter().filter(|r| f(r)).count()
    }

    /// Rows with a matchable B seam (the honest analysis denominator — excludes tails and no-lag gaps).
    pub fn matched(&self) -> Vec<&GapRow> {
        self.rows.iter().filter(|r| r.kind == GapKind::Matched).collect()
    }

    /// Human summary aimed at the P0 go/no-go. The denominator is **matched** gaps (B carries seam
    /// content); tails (P6) and no-lag gaps are bucketed out so the addressable rate isn't diluted.
    pub fn summary_text(&self) -> String {
        use std::fmt::Write;
        let n = self.total_gaps();
        let mut s = String::new();
        let _ = writeln!(
            s,
            "=== gap-fingerprint corpus: {} pair(s), {n} gap(s) (tail ≥ {:.0}s, drift eps {:.1} ms) ===",
            self.pairs.len(),
            self.tail_secs,
            self.drift_eps_ms
        );

        // Plan kind (vocabulary validation — today the fingerprint only emits `fillable`).
        let mut plan: BTreeMap<String, usize> = BTreeMap::new();
        for r in &self.rows {
            *plan.entry(r.plan_kind.clone().unwrap_or_else(|| "(none)".into())).or_default() += 1;
        }
        let plan_str: Vec<String> = plan.iter().map(|(k, c)| format!("{k} {c}")).collect();
        let _ = writeln!(s, "plan_kind: {}", plan_str.join(" · "));

        // Region kind — the denominator selector.
        let tail = self.count(|r| r.kind == GapKind::Tail);
        let nolag = self.count(|r| r.kind == GapKind::NoLag);
        let matched = self.matched();
        let m = matched.len();
        let _ = writeln!(
            s,
            "gap kind:  matched {m} · no-lag {nolag} · tail {tail}   [analysis denominator = matched]"
        );

        // Verdict mix among matched.
        let mut verdicts: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &matched {
            *verdicts.entry(r.verdict.as_deref().unwrap_or("(none)")).or_default() += 1;
        }
        let vstr: Vec<String> = verdicts.iter().map(|(v, c)| format!("{v} {c} ({})", pct(*c, m))).collect();
        let _ = writeln!(s, "── among matched ({m}) ──");
        let _ = writeln!(s, "  verdict: {}", vstr.join(" · "));

        // Outcome among matched.
        let mpatched = matched.iter().filter(|r| r.patched()).count();
        let mskipped = matched.iter().filter(|r| r.outcome_tier.as_deref() == Some("skip")).count();
        let _ = writeln!(
            s,
            "  outcome: patched {mpatched} ({}) · skipped {mskipped} ({})",
            pct(mpatched, m),
            pct(mskipped, m)
        );

        // Headline: addressable = matched + timing_offset + skipped.
        let addr: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| {
                r.verdict.as_deref() == Some("timing_offset")
                    && r.outcome_tier.as_deref() == Some("skip")
            })
            .collect();
        let constant = addr.iter().filter(|r| r.skew == SkewClass::Constant).count();
        let drift = addr.iter().filter(|r| r.skew == SkewClass::Drift).count();
        let _ = writeln!(
            s,
            "  addressable (timing_offset AND skipped): {} ({} of matched)",
            addr.len(),
            pct(addr.len(), m)
        );
        let _ = writeln!(s, "    constant (single shift): {constant}   drift (needs time-warp): {drift}");
        if let Some((mn, md, mx)) = stats(addr.iter().filter_map(|r| r.drift_ms()).collect()) {
            let _ = writeln!(s, "    pre↔post drift ms  min {mn:.2} / median {md:.2} / max {mx:.2}");
        }
        if let Some((mn, md, mx)) = stats(addr.iter().filter_map(|r| r.max_offset_ms()).collect()) {
            let _ = writeln!(s, "    seam offset ms     min {mn:.2} / median {md:.2} / max {mx:.2}");
        }
        if let Some((mn, md, _)) = stats(
            addr.iter()
                .filter_map(|r| match (r.peak_r_pre, r.peak_r_post) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    _ => None,
                })
                .collect(),
        ) {
            let _ = writeln!(s, "    min(peak_r) pre/post  worst {mn:.3} / median {md:.3} (recover floor 0.5)");
        }

        // Contrast: decorrelated skips (genuinely unfixable).
        let decorr_skipped = matched
            .iter()
            .filter(|r| {
                r.verdict.as_deref() == Some("decorrelated")
                    && r.outcome_tier.as_deref() == Some("skip")
            })
            .count();
        let _ = writeln!(s, "    (contrast) decorrelated AND skipped = {decorr_skipped}");

        // Per-pair breakdown (matched / skipped-matched).
        let _ = writeln!(s, "per pair (matched / skipped-matched):");
        let mut by_pair: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for r in &matched {
            let e = by_pair.entry(r.pair.as_str()).or_default();
            e.0 += 1;
            if r.outcome_tier.as_deref() == Some("skip") {
                e.1 += 1;
            }
        }
        for (pair, (mm, sk)) in &by_pair {
            let _ = writeln!(s, "  {pair:<20} {mm:>3} / {sk:>3}");
        }
        s
    }

    /// One CSV row per gap for drill-down.
    pub fn csv(&self) -> String {
        use std::fmt::Write;
        let mut s = String::from(
            "pair,a_id,b_id,index,duration_secs,plan_kind,kind,verdict,outcome_tier,patched,\
frac_lag_pre_ms,frac_lag_post_ms,drift_ms,peak_r_pre,peak_r_post,skew\n",
        );
        let opt = |v: Option<f64>| v.map(|x| format!("{x:.4}")).unwrap_or_default();
        for r in &self.rows {
            let _ = writeln!(
                s,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:?}",
                r.pair,
                r.a_id,
                r.b_id,
                r.index,
                opt(r.duration_secs),
                r.plan_kind.clone().unwrap_or_default(),
                r.kind.as_str(),
                r.verdict.clone().unwrap_or_default(),
                r.outcome_tier.clone().unwrap_or_default(),
                r.patched(),
                opt(r.frac_lag_pre_ms),
                opt(r.frac_lag_post_ms),
                opt(r.drift_ms()),
                opt(r.peak_r_pre),
                opt(r.peak_r_post),
                r.skew,
            );
        }
        s
    }
}

/// Convenience: env knob for the constant/drift split (`GAP_FP_DRIFT_EPS_MS`, default 1.0 ms).
pub fn drift_eps_from_env() -> f64 {
    std::env::var("GAP_FP_DRIFT_EPS_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_DRIFT_EPS_MS)
}

/// Convenience: env knob for the long-tail (P6) duration cutoff (`GAP_FP_TAIL_SECS`, default 30 s).
pub fn tail_secs_from_env() -> f64 {
    std::env::var("GAP_FP_TAIL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TAIL_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_corpus(dir: &Path, a_id: &str, b_id: &str, gaps_json: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let json = format!(
            r#"{{"source":{{"a_source":{{"id":"{a_id}","sample_rate":48000,"channels":2,"duration_secs":60.0}},
            "b_source":{{"id":"{b_id}","sample_rate":48000,"channels":2,"duration_secs":60.0}},
            "scan_recipe":{{"silence_peak_fraction":0.01}},"gap_count":1}},"gaps":{gaps_json}}}"#
        );
        std::fs::write(dir.join("corpus.json"), json).unwrap();
    }

    fn gap(index: usize, verdict: &str, tier: &str, pre_ms: f64, post_ms: f64) -> String {
        format!(
            r#"{{"index":{index},"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{{"a_start_secs":0,"a_end_secs":0,"a_refined_start_secs":0,"a_refined_end_secs":0,"duration_secs":1.8}},
            "levels":{{"bin_ms":50,"profile_db":[],"floor_db":-120,"speech_peak_db":-40,"noise_floor_db":-50,"gap_floor_db":-90}},
            "silence":{{"collar_rms_peak_ratio":0.1,"collar_above_relative_floor":true,"silence_peak_fraction":0.01}},
            "contour":{{"has_anchor_seam_contour":true,"pre_flatness":0,"post_flatness":0}},
            "anchors":{{"pre":[],"post":[]}},
            "lag":{{"pre_anchor":[{{"window_ms":250,"max_lag_ms":200,"channel":"mono","lag0_r":-0.1,"peak_r":0.95,"peak_lag_samples":-100,"frac_lag_samples":-100,"frac_lag_ms":{pre_ms},"verdict":"{verdict}"}}],
                    "post_anchor":[{{"window_ms":250,"max_lag_ms":200,"channel":"mono","lag0_r":-0.1,"peak_r":0.95,"peak_lag_samples":-50,"frac_lag_samples":-50,"frac_lag_ms":{post_ms},"verdict":"{verdict}"}}]}},
            "outcome":{{"plan_kind":"fillable","tier":"{tier}","seam_shape":""}}}}"#
        )
    }

    /// A gap with **no** lag block, at a given duration — exercises the `NoLag` / `Tail` kinds.
    fn gap_nolag(index: usize, duration_secs: f64, tier: &str) -> String {
        format!(
            r#"{{"index":{index},"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{{"a_start_secs":0,"a_end_secs":0,"a_refined_start_secs":0,"a_refined_end_secs":0,"duration_secs":{duration_secs}}},
            "levels":{{"bin_ms":50,"profile_db":[],"floor_db":-120,"speech_peak_db":-40,"noise_floor_db":-50,"gap_floor_db":-90}},
            "silence":{{"collar_rms_peak_ratio":0.1,"collar_above_relative_floor":true,"silence_peak_fraction":0.01}},
            "contour":{{"has_anchor_seam_contour":true,"pre_flatness":0,"post_flatness":0}},
            "anchors":{{"pre":[],"post":[]}},
            "outcome":{{"plan_kind":"fillable","tier":"{tier}","seam_shape":""}}}}"#
        )
    }

    #[test]
    fn aggregates_verdicts_and_skew_across_pairs() {
        let root = tempfile::tempdir().unwrap();
        // pair 1: drifting timing_offset skip (−16 vs −8), decorrelated skip, and a 200 s tail.
        write_corpus(
            &root.path().join("1"),
            "aaaa",
            "bbbb",
            &format!(
                "[{},{},{}]",
                gap(0, "timing_offset", "skip", -16.0, -8.0),
                gap(1, "decorrelated", "skip", 2.0, 3.0),
                gap_nolag(2, 200.0, "skip"),
            ),
        );
        // pair 2: constant timing_offset skip (−5 vs −5.2), a patched gap, and a short no-lag skip.
        write_corpus(
            &root.path().join("2"),
            "cccc",
            "dddd",
            &format!(
                "[{},{},{}]",
                gap(0, "timing_offset", "skip", -5.0, -5.2),
                gap(1, "timing_offset", "patch", -5.0, -5.0),
                gap_nolag(2, 3.0, "skip"),
            ),
        );

        let report = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0);
        assert_eq!(report.pairs.len(), 2, "two pair dirs");
        assert_eq!(report.total_gaps(), 6);

        // Region kinds: 4 matched (the lag-bearing gaps), 1 tail (200 s), 1 no-lag (3 s).
        assert_eq!(report.matched().len(), 4, "four lag-bearing gaps are matched");
        assert_eq!(report.rows.iter().filter(|r| r.kind == GapKind::Tail).count(), 1);
        assert_eq!(report.rows.iter().filter(|r| r.kind == GapKind::NoLag).count(), 1);

        // Addressable = matched + timing_offset + skipped (the patched and decorrelated excluded).
        let matched = report.matched();
        let addr: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.verdict.as_deref() == Some("timing_offset") && !r.patched())
            .collect();
        assert_eq!(addr.len(), 2, "two timing_offset skips");
        assert_eq!(addr.iter().filter(|r| r.skew == SkewClass::Drift).count(), 1, "−16/−8 is drift");
        assert_eq!(addr.iter().filter(|r| r.skew == SkewClass::Constant).count(), 1, "−5/−5.2 is constant");

        // Plan kind surfaced; summary + CSV render and carry the new columns.
        assert!(report.rows.iter().all(|r| r.plan_kind.as_deref() == Some("fillable")));
        let summary = report.summary_text();
        assert!(summary.contains("plan_kind: fillable"));
        assert!(summary.contains("gap kind:"));
        assert!(report.csv().lines().next().unwrap().contains("plan_kind,kind,"));
        assert_eq!(report.csv().lines().count(), 7); // header + 6 gaps
    }
}
