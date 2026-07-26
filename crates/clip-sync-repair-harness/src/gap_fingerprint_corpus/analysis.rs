//! Corpus discovery + JSON projection: walk pair dirs, parse the minimal `corpus.json` projection, and
//! flatten each gap into a [`GapRow`] ([`analyze_dirs`]). Owns the private `Deserialize` types (their only
//! consumer is `gap_row` here) and the curated-fixture / env-knob entrypoints. Builds the report shell
//! ([`super::CorpusReport`]); the formatters that render it live in [`super::report`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::schema::{
    GapKind, GapRow, SeamDiag, SkewClass, SEAM_QUIET_SNR_DB, SEAM_RECOVER_R, SEAM_ROBUST_R,
};
use super::CorpusReport;

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
    /// A-side level profile (summary levels only) — dropout depth vs program-quiet (D11).
    #[serde(default)]
    levels: Option<BLevels>,
    /// Diagnostic lag at the best editorial bracket (may sit far from the decision seam).
    #[serde(default)]
    lag: Option<Lag>,
    /// Lag at the **decision** placement (structure-slid throat) — the registration-relevant one (#2).
    #[serde(default)]
    baseline_lag: Option<Lag>,
    /// Residual cancellation at the decision seam (the strong same-source confirm).
    #[serde(default)]
    residual: Option<Residual>,
    /// Envelope/structure match at the baseline placement (bucketed, 50 ms bins, ~3 s context).
    #[serde(default)]
    structure: Option<PairScore>,
    /// Waveform seam Pearson at the throat (sample-level, ~250 ms) — the gate's actual decision seam.
    #[serde(default)]
    seams: Option<PairScore>,
    /// Per-bracket gate scores + which stage rejected each (the authoritative skip reason).
    #[serde(default)]
    brackets: Vec<Bracket>,
    /// Seam recovery / encoding-robust envelope / level at the decision seam (diagnoses dead seams).
    #[serde(default)]
    seam_probe: Option<SeamProbeFp>,
    /// Donor B energy across the gap-mapped span (bridges the hole?).
    #[serde(default)]
    donor_interior: Option<DonorInterior>,
    /// Donor occupancy at the **nominal** geometry span (no lag) — registration-independent (D11).
    #[serde(default)]
    donor_interior_nominal: Option<DonorInterior>,
    /// Symmetric B-side level profile over the nominal span (counterpart to A `levels`; D11).
    #[serde(default)]
    b_levels: Option<BLevels>,
    /// First-class splice summary (step + per-side peaks / peak_z).
    #[serde(default)]
    splice: Option<Splice>,
    /// Dual-fit repair viability at the per-shoulder placement (gate-equivalent seam score; C3/C7).
    #[serde(default)]
    splice_dualfit: Option<SpliceDualfit>,
    /// Wide (100 ms-bin) envelope segment-identity confirmer at the decision seam.
    #[serde(default)]
    wide_envelope: Option<WideEnvelopeFp>,
    #[serde(default)]
    outcome: Option<Outcome>,
}

#[derive(Deserialize)]
struct DonorInterior {
    rms_db: f64,
    silence_fraction: f64,
    continuous: bool,
}

/// Projection of the symmetric B-side `LevelProfile` (only the summary levels the analyzer reads).
#[derive(Deserialize)]
struct BLevels {
    gap_floor_db: f64,
    noise_floor_db: f64,
    #[allow(dead_code)]
    speech_peak_db: f64,
}

#[derive(Deserialize)]
struct Splice {
    step_ms: f64,
    pre_peak_r: f64,
    post_peak_r: f64,
    #[serde(default)]
    pre_peak_z: Option<f64>,
    #[serde(default)]
    post_peak_z: Option<f64>,
    /// Either shoulder's `baseline_lag` peak was search-exhausted (clipped at ±max_lag) ⇒ `step_ms` is
    /// GIGO (ledger A5/C6). `None` for fingerprints predating the edge-pin flag.
    #[serde(default)]
    edge_pinned: Option<bool>,
}

#[derive(Deserialize, Clone, Copy)]
struct SpliceDualfit {
    pre_seam_r: f64,
    post_seam_r: f64,
    #[allow(dead_code)]
    trim_frames: i64,
    gate_pass: bool,
    #[serde(default)]
    post_seam_global_r: f64,
    #[serde(default)]
    pre_seam_prom: Option<f64>,
    #[serde(default)]
    post_seam_prom: Option<f64>,
}

#[derive(Deserialize, Clone, Copy)]
struct EnvPeak {
    #[allow(dead_code)]
    peak_r: f64,
    peak_lag_ms: f64,
    #[allow(dead_code)]
    prominence: f64,
}

#[derive(Deserialize)]
struct WideEnvelopeFp {
    #[serde(default)]
    pre: Option<EnvPeak>,
    #[serde(default)]
    post: Option<EnvPeak>,
}

#[derive(Deserialize)]
struct SeamProbeFp {
    #[serde(default)]
    pre: Option<SeamProbe>,
    #[serde(default)]
    post: Option<SeamProbe>,
}

#[derive(Deserialize, Clone, Copy)]
struct SeamProbe {
    waveform_r: f64,
    recovered_r: f64,
    #[allow(dead_code)]
    recovered_lag_ms: f64,
    #[serde(default)]
    bandlimited_r: f64,
    #[serde(default)]
    spectrum_r: f64,
    envelope_r: f64,
    #[allow(dead_code)]
    rms_db: f64,
    snr_db: f64,
}

#[derive(Deserialize)]
struct PairScore {
    #[serde(default)]
    baseline_pre: Option<f64>,
    #[serde(default)]
    baseline_post: Option<f64>,
}

#[derive(Deserialize)]
struct Bracket {
    #[serde(default)]
    seam_pre: Option<f64>,
    #[serde(default)]
    seam_post: Option<f64>,
    /// Chosen placement — `None` on pre-2026-07-25 dumps and on projected (non-measured) brackets.
    #[serde(default)]
    start_frame: Option<usize>,
    #[serde(default)]
    fill_frames: Option<usize>,
    #[serde(default)]
    failure_stage: Option<String>,
}

/// The bracket with the highest min-seam — the same "closest to chosen" rule `best_bracket_seam`
/// and `closest_failure_stage` already use. Brackets with an incomplete seam pair are ineligible.
fn best_seam_bracket(brackets: &[Bracket]) -> Option<&Bracket> {
    brackets
        .iter()
        .filter_map(|b| match (b.seam_pre, b.seam_post) {
            (Some(a), Some(c)) => Some((a.min(c), b)),
            _ => None,
        })
        .fold(None, |acc: Option<(f64, &Bracket)>, (v, b)| match acc {
            Some((best, _)) if best >= v => acc,
            _ => Some((v, b)),
        })
        .map(|(_, b)| b)
}

#[derive(Deserialize)]
struct Residual {
    // `Option` because the writer emits non-finite dB (a silent gap cancels to ~0 ⇒ `to_db(0) = -inf`) as
    // JSON `null`; a required `f64` here would fail the **whole** `corpus.json` parse and silently drop
    // every measured gap in the pair (it did — see the residual-null bug). `None` ⇒ residual unavailable.
    #[serde(default)]
    chosen_pre_db: Option<f64>,
    #[serde(default)]
    chosen_post_db: Option<f64>,
    #[serde(default)]
    floor_pre_db: Option<f64>,
    #[serde(default)]
    floor_post_db: Option<f64>,
    #[serde(default)]
    informative: bool,
}

#[derive(Deserialize)]
struct Geometry {
    #[serde(default)]
    duration_secs: Option<f64>,
    #[serde(default)]
    a_refined_start_secs: Option<f64>,
}

#[derive(Deserialize)]
struct Lag {
    #[serde(default)]
    pre_anchor: Vec<LagEntry>,
    #[serde(default)]
    post_anchor: Vec<LagEntry>,
}

/// Mirrors `gap_fingerprint::LagChannel`'s externally-tagged repr (`"mono"` / `{"selected":N}`).
#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum LagChannelTag {
    Mono,
    Selected(usize),
}

#[derive(Deserialize)]
struct LagEntry {
    /// Absent in pre-A2 fingerprints (single-entry mono-only arrays); `.first()`-style fallback below
    /// treats a missing channel as mono so old data keeps working.
    #[serde(default)]
    channel: Option<LagChannelTag>,
    peak_r: f64,
    #[serde(default)]
    second_peak_r: Option<f64>,
    /// Robust uniqueness (post-rescan): z-score of the peak over the whole lag curve. `None` for older
    /// fingerprints. Once populated this is the addressability gate (§3.6a: unique ≥ ~12 at the 1 s window).
    /// (`prominence` is also written to the fingerprint but the analyzer gates on `peak_z`.)
    #[serde(default)]
    peak_z: Option<f64>,
    /// `peak_r − second_peak_r` — prominence of the chosen lag over the tallest rival.
    #[serde(default)]
    prominence: Option<f64>,
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
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            eprintln!(
                "warning: failed to read corpus {}: {err}",
                path.display()
            );
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(file) => Some(file),
        Err(err) => {
            // Do not silently fall back to stale per-gap files when schema drifts.
            eprintln!(
                "warning: failed to parse corpus {}: {err}",
                path.display()
            );
            None
        }
    }
}

fn gap_row(pair: &str, source: &SourceMeta, gap: &GapEntry, eps: f64, tail_secs: f64) -> GapRow {
    // Registration is read at the **decision** seam (`baseline_lag`, #2); fall back to the diagnostic
    // best-bracket `lag` for older fingerprints that predate it. Select the mono entry explicitly (P2-1):
    // capture pushes mono first today, but `.first()` silently picks whatever is first if channel order
    // ever changes — match on `channel` instead, falling back to `.first()` only when `channel` is absent
    // (pre-A2 fingerprints, which only ever wrote one mono entry anyway).
    fn mono_entry(v: &[LagEntry]) -> Option<&LagEntry> {
        v.iter()
            .find(|e| e.channel == Some(LagChannelTag::Mono))
            .or_else(|| v.first())
    }
    // Registration prefers the decision-seam `baseline_lag` (b_mapped, ledger A2); the diagnostic
    // best-bracket `lag` is only a **legacy fallback** for pre-A2 fingerprints and sits at a *different*
    // placement (structure throat). Flag when a row falls back so a run mixing pre-/post-A2 corpora is
    // visible instead of silently conflating placements (C-harness-3).
    let registration_from_legacy_lag = gap.baseline_lag.is_none() && gap.lag.is_some();
    let lag = gap.baseline_lag.as_ref().or(gap.lag.as_ref());
    let pre = lag.and_then(|l| mono_entry(&l.pre_anchor));
    let post = lag.and_then(|l| mono_entry(&l.post_anchor));
    let verdict = pre.or(post).map(|s| s.verdict.clone());
    let frac_lag_pre_ms = pre.map(|s| s.frac_lag_ms);
    let frac_lag_post_ms = post.map(|s| s.frac_lag_ms);
    let duration_secs = gap.geometry.as_ref().and_then(|g| g.duration_secs);
    let has_lag = lag.is_some();
    let seam = gap.seam_probe.as_ref().and_then(worst_seam_side);

    // Uniqueness margin: peak_r − second_peak_r per seam; the worst (min) over pre/post is the gap's.
    let margin = |s: &LagEntry| s.second_peak_r.map(|sp| s.peak_r - sp);
    let worst = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    let uniqueness_margin = worst(pre.and_then(margin), post.and_then(margin));
    // **Robust uniqueness is a both-shoulders test** (C-harness-1): a splice is only trustworthy if *both*
    // seams stand out, so take the min over pre/post only when BOTH sides carry the metric. Falling back to
    // a single present side (the old `worst()`) over-states uniqueness for a gap missing a shoulder z.
    let both = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        _ => None,
    };
    let base_uniqueness_z = both(pre.and_then(|s| s.peak_z), post.and_then(|s| s.peak_z));
    let uniqueness_prom = worst(pre.and_then(|s| s.prominence), post.and_then(|s| s.prominence));

    // First-class splice / donor / wide-envelope when present (post-rescan).
    let splice = gap.splice.as_ref();
    let peak_r_pre = splice.map(|s| s.pre_peak_r).or_else(|| pre.map(|s| s.peak_r));
    let peak_r_post = splice.map(|s| s.post_peak_r).or_else(|| post.map(|s| s.peak_r));
    let splice_step_ms = splice.map(|s| s.step_ms);
    let splice_edge_pinned = splice.and_then(|s| s.edge_pinned);
    // Prefer the splice's per-shoulder z (same `baseline_lag` source), but stay two-sided: fall back to the
    // lag-entry both-sided z only when the splice carries neither shoulder.
    let uniqueness_z = match splice {
        Some(s) => both(s.pre_peak_z, s.post_peak_z).or(base_uniqueness_z),
        None => base_uniqueness_z,
    };

    // **Skew is a two-sided classification** (C-harness-2): constant-vs-drift needs *both* shoulders, and
    // both must read `timing_offset`. The headline `verdict` is pre-preferred (one-sided) and only labels
    // the gap; don't let it drive skew when the shoulders disagree or one is missing.
    let both_timing = pre.is_some_and(|s| s.verdict == "timing_offset")
        && post.is_some_and(|s| s.verdict == "timing_offset");
    let skew = match (both_timing, frac_lag_pre_ms, frac_lag_post_ms) {
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
        a_start_secs: gap.geometry.as_ref().and_then(|g| g.a_refined_start_secs),
        plan_kind: gap.outcome.as_ref().and_then(|o| o.plan_kind.clone()),
        kind,
        verdict,
        outcome_tier: gap.outcome.as_ref().map(|o| o.tier.clone()),
        skip_reason: gap.outcome.as_ref().and_then(|o| o.skip_reason.clone()),
        frac_lag_pre_ms,
        frac_lag_post_ms,
        peak_r_pre,
        peak_r_post,
        uniqueness_margin,
        uniqueness_z,
        uniqueness_prom,
        splice_step_ms,
        splice_edge_pinned,
        registration_from_legacy_lag,
        donor_continuous: gap.donor_interior.as_ref().map(|d| d.continuous),
        donor_rms_db: gap.donor_interior.as_ref().map(|d| d.rms_db),
        wide_env_pre_lag_ms: gap.wide_envelope.as_ref().and_then(|w| w.pre.map(|p| p.peak_lag_ms)),
        wide_env_post_lag_ms: gap.wide_envelope.as_ref().and_then(|w| w.post.map(|p| p.peak_lag_ms)),
        // Worst (least-cancelling) side: max of the two headrooms. `None` if any dB was null (non-finite).
        residual_headroom_db: gap.residual.as_ref().and_then(|r| {
            match (r.chosen_pre_db, r.floor_pre_db, r.chosen_post_db, r.floor_post_db) {
                (Some(cp), Some(fp), Some(cq), Some(fq)) => Some((cp - fp).max(cq - fq)),
                _ => None,
            }
        }),
        residual_informative: gap.residual.as_ref().map(|r| r.informative),
        structure_min: gap.structure.as_ref().and_then(pair_min),
        seam_min: gap.seams.as_ref().and_then(pair_min),
        best_bracket_seam: gap
            .brackets
            .iter()
            .filter_map(|b| match (b.seam_pre, b.seam_post) {
                (Some(a), Some(c)) => Some(a.min(c)),
                _ => None,
            })
            .fold(None, |acc, v| Some(acc.map_or(v, |a: f64| a.max(v)))),
        // Placement of that same best-min-seam bracket — the fill the end sweep actually chose.
        // Nothing else in the corpus records it; see docs/dev/TEMP-fill-placement-axis-plan.md.
        best_bracket_start_frame: best_seam_bracket(&gap.brackets).and_then(|b| b.start_frame),
        best_bracket_fill_frames: best_seam_bracket(&gap.brackets).and_then(|b| b.fill_frames),
        best_bracket_seam_pre: best_seam_bracket(&gap.brackets).and_then(|b| b.seam_pre),
        best_bracket_seam_post: best_seam_bracket(&gap.brackets).and_then(|b| b.seam_post),
        brackets_total: gap.brackets.len(),
        brackets_passing: gap.brackets.iter().filter(|b| b.failure_stage.is_none()).count(),
        // Failure stage of the bracket with the highest min-seam (the closest to passing).
        closest_failure_stage: gap
            .brackets
            .iter()
            .filter(|b| b.failure_stage.is_some())
            .max_by(|x, y| {
                let mn = |b: &Bracket| match (b.seam_pre, b.seam_post) {
                    (Some(a), Some(c)) => a.min(c),
                    _ => f64::NEG_INFINITY,
                };
                mn(x).partial_cmp(&mn(y)).unwrap_or(std::cmp::Ordering::Equal)
            })
            .and_then(|b| b.failure_stage.clone()),
        seam_recovered_r: seam.map(|p| p.recovered_r),
        seam_bandlimited_r: seam.map(|p| p.bandlimited_r),
        seam_spectrum_r: seam.map(|p| p.spectrum_r),
        seam_envelope_r: seam.map(|p| p.envelope_r),
        seam_snr_db: seam.map(|p| p.snr_db),
        seam_diag: seam.map(seam_diag),
        skew,
        dualfit_pre_r: gap.splice_dualfit.as_ref().map(|d| d.pre_seam_r),
        dualfit_post_r: gap.splice_dualfit.as_ref().map(|d| d.post_seam_r),
        dualfit_pass: gap.splice_dualfit.as_ref().map(|d| d.gate_pass),
        dualfit_post_global_r: gap.splice_dualfit.as_ref().map(|d| d.post_seam_global_r),
        dualfit_seam_prom: gap.splice_dualfit.as_ref().and_then(|d| match (d.pre_seam_prom, d.post_seam_prom) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }),
        donor_nominal_silence: gap.donor_interior_nominal.as_ref().map(|d| d.silence_fraction),
        donor_nominal_cont: gap.donor_interior_nominal.as_ref().map(|d| d.continuous),
        donor_aligned_silence: gap.donor_interior.as_ref().map(|d| d.silence_fraction),
        b_gap_floor_db: gap.b_levels.as_ref().map(|l| l.gap_floor_db),
        b_noise_floor_db: gap.b_levels.as_ref().map(|l| l.noise_floor_db),
        a_gap_floor_db: gap.levels.as_ref().map(|l| l.gap_floor_db),
        a_noise_floor_db: gap.levels.as_ref().map(|l| l.noise_floor_db),
    }
}

/// The worst (most-blocking) side of a seam probe: the present side with the lower `waveform_r`.
fn worst_seam_side(p: &SeamProbeFp) -> Option<SeamProbe> {
    match (p.pre, p.post) {
        (Some(a), Some(b)) => Some(if a.waveform_r <= b.waveform_r { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Classify a dead waveform seam from the probe — the measurement that was missing. Simplest-fix-first:
/// quiet → a sub-bin shift recovers the raw waveform (R3) → a cross-codec-robust metric agrees (R2/R4)
/// → none.
fn seam_diag(p: SeamProbe) -> SeamDiag {
    if p.snr_db < SEAM_QUIET_SNR_DB {
        SeamDiag::Quiet
    } else if p.recovered_r >= SEAM_RECOVER_R {
        SeamDiag::Misaligned
    } else if p.bandlimited_r.max(p.spectrum_r) >= SEAM_ROBUST_R {
        SeamDiag::CrossCodec
    } else {
        SeamDiag::Unresolved
    }
}

fn pair_min(p: &PairScore) -> Option<f64> {
    match (p.baseline_pre, p.baseline_post) {
        (Some(a), Some(b)) => Some(a.min(b)),
        _ => None,
    }
}

/// Build the analyzed [`GapRow`]s from one corpus's JSON bytes — the same minimal-projection parse
/// `analyze_dirs` uses, exposed for per-gap fixture tests that hold a single `corpus.json` (or a curated
/// single-gap fixture) rather than a directory tree. `pair` labels the rows (diagnostics only).
pub fn gap_rows_from_corpus_json(
    bytes: &[u8],
    pair: &str,
    drift_eps_ms: f64,
    tail_secs: f64,
) -> Result<Vec<GapRow>, serde_json::Error> {
    let cf: CorpusFile = serde_json::from_slice(bytes)?;
    Ok(cf
        .gaps
        .iter()
        .map(|g| gap_row(pair, &cf.source, g, drift_eps_ms, tail_secs))
        .collect())
}

/// Analyze one curated fixture's JSON bytes into its single [`GapRow`], labelled `pair`.
fn curated_single_row(bytes: &[u8], pair: &str) -> GapRow {
    let mut rows = gap_rows_from_corpus_json(bytes, pair, DEFAULT_DRIFT_EPS_MS, DEFAULT_TAIL_SECS)
        .expect("parse curated fixture into rows");
    assert_eq!(rows.len(), 1, "curated fixture {pair} must be single-gap");
    rows.pop().unwrap()
}

/// The golden row label for a fixture: its filename stem (e.g. `01_bracket_patch_clean`). Unique by
/// construction, so keys stay distinct even when several fixtures share a cell type *and* a source-gap index
/// (the Phase-5 multiple-per-type case) — a bare cell-type label would collide there.
fn curated_row_label(file: &str) -> &str {
    file.strip_suffix(".json").unwrap_or(file)
}

/// Analyzed rows for the committed curated per-gap-**type** fixtures (gap cells: `docs/dev/gap-vocabulary.md`).
/// Each row is labelled by its fixture file stem, so golden keys are unique despite colliding source-gap
/// indices. Media-free — reads the committed fixture bytes directly.
pub fn curated_gap_cell_rows() -> Vec<GapRow> {
    use clip_sync_repair_fixtures::gap_cell_fixtures::{curated_fixtures_dir, load_gap_cell_fixtures};
    let dir = curated_fixtures_dir();
    load_gap_cell_fixtures()
        .into_iter()
        .map(|fx| {
            let path = dir.join(&fx.file);
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            curated_single_row(&bytes, curated_row_label(&fx.file))
        })
        .collect()
}

/// As [`curated_gap_cell_rows`] but each fixture is first pushed through the tags→fingerprint projection
/// ([`crate::corpus_projection::project_corpus`]) — the projection-fidelity input (Phase 3 spec-diff).
pub fn curated_gap_cell_projected_rows() -> Vec<GapRow> {
    use clip_sync_repair_fixtures::gap_cell_fixtures::load_gap_cell_fixtures;
    load_gap_cell_fixtures()
        .into_iter()
        .map(|fx| {
            let projected = crate::corpus_projection::project_corpus(&fx.corpus);
            let bytes = serde_json::to_vec(&projected).expect("serialize projected curated corpus");
            curated_single_row(&bytes, curated_row_label(&fx.file))
        })
        .collect()
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
