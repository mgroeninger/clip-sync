//! Report side of the corpus scan: the `CorpusReport` aggregation plus its
//! summary / mechanism / gate / splice / dual-fit / CSV / golden formatters.
//!
//! Consumes analyzed `GapRow`s (built by `super::analysis`); does no JSON I/O.

use std::collections::BTreeMap;

use clip_sync_repair::domain::donor::PROGRAM_QUIET_SILENCE_FRAC;

use super::schema::{
    GapKind, GapRow, SkewClass, SpliceDiag, LOW_UNIQUENESS_MARGIN, SEAM_ROBUST_R,
    SPLICE_MIN_PEAK_R, SPLICE_MIN_PEAK_Z, SPLICE_MIN_PROMINENCE,
};

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

/// Below this `|seam step|` (ms) the gap is a **clean constant-offset** dropout (one shift aligns both
/// seams); above it the A↔B timeline steps at the gap. Heuristic; calibrate against the corpus.
const CLEAN_STEP_MS: f64 = 2.0;

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

/// Least-squares line `y = slope·x + b`; returns `(slope, residual_std)`. `None` for < 2 points or no
/// x-spread. Used to test the offset for **clip drift** (a real drift is a consistent slope vs gap time
/// with small residual; a per-gap-local offset has a large residual around any line).
fn linfit(pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let nf = n as f64;
    let (sx, sy) = pts
        .iter()
        .fold((0.0, 0.0), |(ax, ay), &(x, y)| (ax + x, ay + y));
    let (mx, my) = (sx / nf, sy / nf);
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for &(x, y) in pts {
        sxx += (x - mx) * (x - mx);
        sxy += (x - mx) * (y - my);
    }
    if sxx <= f64::EPSILON {
        return None;
    }
    let slope = sxy / sxx;
    let b = my - slope * mx;
    let resid = (pts
        .iter()
        .map(|&(x, y)| (y - (slope * x + b)).powi(2))
        .sum::<f64>()
        / nf)
        .sqrt();
    Some((slope, resid))
}

/// RMS of each value folded into `[−q/2, q/2]` — small ⇒ the values cluster near multiples of `q` (a
/// dropped/duplicated buffer or video frame leaves the step quantized to a block size).
fn quantization_residual(values: &[f64], q: f64) -> f64 {
    if values.is_empty() || q <= 0.0 {
        return f64::INFINITY;
    }
    let folded: f64 = values
        .iter()
        .map(|&v| {
            let r = v.rem_euclid(q);
            let r = if r > q / 2.0 { r - q } else { r };
            r * r
        })
        .sum();
    (folded / values.len() as f64).sqrt()
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
        self.rows
            .iter()
            .filter(|r| r.kind == GapKind::Matched)
            .collect()
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

        // C-harness-3: warn loudly when the corpus mixes pre-/post-A2 schemas — the legacy `lag` fallback
        // sits at a different placement than `baseline_lag`, so the two aren't comparable in one run.
        let legacy = self.count(|r| r.registration_from_legacy_lag);
        if legacy > 0 {
            let _ = writeln!(
                s,
                "  ⚠ registration schema mix: {legacy}/{n} row(s) fell back to the legacy diagnostic `lag` \
                 (pre-A2, structure-throat placement) — do NOT compare their lags with `baseline_lag` rows."
            );
        }

        // Plan kind (vocabulary validation — today the fingerprint only emits `fillable`).
        let mut plan: BTreeMap<String, usize> = BTreeMap::new();
        for r in &self.rows {
            *plan
                .entry(r.plan_kind.clone().unwrap_or_else(|| "(none)".into()))
                .or_default() += 1;
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
            *verdicts
                .entry(r.verdict.as_deref().unwrap_or("(none)"))
                .or_default() += 1;
        }
        let vstr: Vec<String> = verdicts
            .iter()
            .map(|(v, c)| format!("{v} {c} ({})", pct(*c, m)))
            .collect();
        let _ = writeln!(s, "── among matched ({m}) ──");
        let _ = writeln!(s, "  verdict: {}", vstr.join(" · "));

        // Uniqueness — how trustworthy the same-source verdicts are (low margin ⇒ a competing lag peak
        // nearly as tall ⇒ possible periodic false positive). Guards the "all same-source" headline.
        let margins: Vec<f64> = matched.iter().filter_map(|r| r.uniqueness_margin).collect();
        if margins.is_empty() {
            let _ = writeln!(
                s,
                "  uniqueness: (no second_peak_r — re-fingerprint with the current binary to populate)"
            );
        } else if let Some((mn, md, _)) = stats(margins.clone()) {
            let suspect = margins
                .iter()
                .filter(|&&v| v < LOW_UNIQUENESS_MARGIN)
                .count();
            let _ = writeln!(
                s,
                "  uniqueness: {suspect}/{} periodicity-suspect (margin < {:.2}); margin min {mn:.2} / median {md:.2}",
                margins.len(),
                LOW_UNIQUENESS_MARGIN
            );
        }

        // Residual — the strong same-source confirm. Cross-checks the lag verdict: does B actually
        // cancel A to the noise floor (informative & headroom ≤ 0)?
        let with_resid: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.residual_headroom_db.is_some())
            .collect();
        if with_resid.is_empty() {
            let _ = writeln!(
                s,
                "  residual: (no residual probe — re-fingerprint with the current binary to populate)"
            );
        } else {
            let confirmed = with_resid
                .iter()
                .filter(|r| {
                    r.residual_informative == Some(true)
                        && r.residual_headroom_db.is_some_and(|h| h <= 0.0)
                })
                .count();
            let headrooms: Vec<f64> = with_resid
                .iter()
                .filter_map(|r| r.residual_headroom_db)
                .collect();
            if let Some((mn, md, mx)) = stats(headrooms) {
                let _ = writeln!(
                    s,
                    "  residual: {confirmed}/{} same-source-confirmed (informative & headroom ≤ 0); headroom dB min {mn:.1} / median {md:.1} / max {mx:.1}",
                    with_resid.len()
                );
            }
        }

        // Outcome among matched.
        let mpatched = matched.iter().filter(|r| r.patched()).count();
        let mskipped = matched
            .iter()
            .filter(|r| r.outcome_tier.as_deref() == Some("skip"))
            .count();
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
        let constant = addr
            .iter()
            .filter(|r| r.skew == SkewClass::Constant)
            .count();
        let drift = addr.iter().filter(|r| r.skew == SkewClass::Drift).count();
        let _ = writeln!(
            s,
            "  addressable (timing_offset AND skipped): {} ({} of matched)",
            addr.len(),
            pct(addr.len(), m)
        );
        let _ = writeln!(
            s,
            "    constant (single shift): {constant}   drift (needs time-warp): {drift}"
        );
        if let Some((mn, md, mx)) = stats(addr.iter().filter_map(|r| r.drift_ms()).collect()) {
            let _ = writeln!(
                s,
                "    pre↔post drift ms  min {mn:.2} / median {md:.2} / max {mx:.2}"
            );
        }
        if let Some((mn, md, mx)) = stats(addr.iter().filter_map(|r| r.max_offset_ms()).collect()) {
            let _ = writeln!(
                s,
                "    seam offset ms     min {mn:.2} / median {md:.2} / max {mx:.2}"
            );
        }
        if let Some((mn, md, _)) = stats(
            addr.iter()
                .filter_map(|r| match (r.peak_r_pre, r.peak_r_post) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    _ => None,
                })
                .collect(),
        ) {
            let _ = writeln!(
                s,
                "    min(peak_r) pre/post  worst {mn:.3} / median {md:.3} (recover floor 0.5)"
            );
        }

        // Contrast: decorrelated skips (genuinely unfixable).
        let decorr_skipped = matched
            .iter()
            .filter(|r| {
                r.verdict.as_deref() == Some("decorrelated")
                    && r.outcome_tier.as_deref() == Some("skip")
            })
            .count();
        let _ = writeln!(
            s,
            "    (contrast) decorrelated AND skipped = {decorr_skipped}"
        );

        // Registration decomposition (decision seam): offset (where in B — a shiftable clip-drift
        // residual) vs step (the pre↔post discontinuity). `step ≈ 0` ⇒ clean constant offset; large
        // `step` ⇒ the A↔B timeline steps at the gap (an edit/length divergence, not shift- or
        // warp-recoverable). NOTE: a large step alone does not prove real divergence vs a spurious lag
        // lock — `uniqueness_margin` decides that (needs the current binary's `second_peak_r`).
        let steps_abs: Vec<f64> = matched
            .iter()
            .filter_map(|r| r.seam_step_ms().map(f64::abs))
            .collect();
        if !steps_abs.is_empty() {
            let clean = steps_abs.iter().filter(|&&v| v < CLEAN_STEP_MS).count();
            let _ = writeln!(
                s,
                "  registration: {clean}/{} clean (|step| < {:.0} ms, ≈ constant offset); {} stepped",
                steps_abs.len(),
                CLEAN_STEP_MS,
                steps_abs.len() - clean
            );
            if let Some((mn, md, mx)) = stats(
                matched
                    .iter()
                    .filter_map(|r| r.seam_mid_ms().map(f64::abs))
                    .collect(),
            ) {
                let _ = writeln!(
                    s,
                    "    |offset| ms (shiftable): min {mn:.1} / median {md:.1} / max {mx:.1}"
                );
            }
            if let Some((mn, md, mx)) = stats(steps_abs.clone()) {
                let _ = writeln!(
                    s,
                    "    |step| ms (divergence) : min {mn:.1} / median {md:.1} / max {mx:.1}"
                );
            }
        }

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

    /// **Mechanism checks** — is the pre↔post step *clip drift* or a *local discontinuity* (dropped
    /// buffer / frame)? Two tests: (1) is the offset clip drift (a consistent slope vs gap time per
    /// file)? (2) does the step cluster near a sample-block / video-frame size (the buffer-drop tell)?
    pub fn mechanism_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "=== mechanism: is the step clip drift, or a local discontinuity? ==="
        );

        // (1) Offset trend per pair — clip drift ⇒ a consistent slope with small residual.
        let _ = writeln!(
            s,
            "offset (mid) vs gap time, per pair  [drift ⇒ residual ≪ offset-spread]:"
        );
        let mut by_pair: BTreeMap<&str, Vec<(f64, f64)>> = BTreeMap::new();
        for r in self.matched() {
            if let (Some(t), Some(mid)) = (r.a_start_secs, r.seam_mid_ms()) {
                by_pair.entry(r.pair.as_str()).or_default().push((t, mid));
            }
        }
        for (pair, pts) in &by_pair {
            let spread = stats(pts.iter().map(|&(_, y)| y).collect())
                .map(|(mn, _, mx)| mx - mn)
                .unwrap_or(0.0);
            let xspan = {
                let xs: Vec<f64> = pts.iter().map(|&(x, _)| x).collect();
                stats(xs).map(|(mn, _, mx)| mx - mn).unwrap_or(0.0)
            };
            match linfit(pts) {
                // Drift only if the line's rise over the file (|slope|·span) explains most of the
                // offset spread. A ~0 slope (offset scattered, no time trend) is *not* drift. Fewer
                // than 4 gaps can't tell a line from scatter — don't render a verdict.
                Some((slope, resid)) => {
                    let explained = slope.abs() * xspan;
                    let verdict = if pts.len() < 4 {
                        "too few to judge"
                    } else if explained > 0.5 * spread && spread > 4.0 {
                        "drift-like"
                    } else {
                        "local (not drift)"
                    };
                    let _ = writeln!(
                        s,
                        "  {pair:<10} {} gaps: slope {slope:+.3} ms/s → {explained:.1} ms over file, residual {resid:.1} ms, spread {spread:.1} ms  → {verdict}",
                        pts.len()
                    );
                }
                None => {
                    let _ = writeln!(s, "  {pair:<10} (too few gaps)");
                }
            }
        }

        // (2) Step quantization — a dropped/duplicated buffer or frame leaves |step| near a block size.
        let steps: Vec<f64> = self
            .matched()
            .iter()
            .filter_map(|r| r.seam_step_ms().map(f64::abs))
            .filter(|&v| v > CLEAN_STEP_MS)
            .collect();
        let _ = writeln!(
            s,
            "step quantization over {} stepped gaps  [ratio = residual / chance (q/√12); ≪ 1 ⇒ quantized]:",
            steps.len()
        );
        let candidates: [(&str, f64); 7] = [
            ("512 smp", 512.0 / 48.0),
            ("1024 smp", 1024.0 / 48.0),
            ("2048 smp", 2048.0 / 48.0),
            ("20ms/50fps", 20.0),
            ("33.4ms/30fps", 1001.0 / 30.0),
            ("40ms/25fps", 40.0),
            ("41.7ms/24fps", 1001.0 / 24.0),
        ];
        // A small *residual* is meaningless on its own (a tiny q always fits). Compare to the chance
        // level for uniform values, `q/√12`: a real drop event sits well below it (ratio ≪ 1).
        let mut best: Option<(&str, f64)> = None;
        for (label, q) in candidates {
            let resid = quantization_residual(&steps, q);
            let ratio = resid / (q / 12.0_f64.sqrt());
            let _ = writeln!(
                s,
                "    {label:<13} (q={q:5.1} ms): residual {resid:4.1} ms  ({ratio:.2}× chance)"
            );
            if best.is_none_or(|(_, br)| ratio < br) {
                best = Some((label, ratio));
            }
        }
        match best {
            Some((label, ratio)) if ratio < 0.6 => {
                let _ = writeln!(s, "  → quantized to {label} ({ratio:.2}× chance) — supports a discrete drop event");
            }
            Some((_, ratio)) => {
                let _ = writeln!(s, "  → no quantization (best {ratio:.2}× chance ≈ random) — steps are continuous, NOT a clean block-drop (or measurements are periodicity-corrupted; needs uniqueness)");
            }
            None => {}
        }
        s
    }

    /// **Trustworthy funnel** — does *any* clean recoverable timing offset survive once we demand the
    /// match be unique (not periodicity-suspect)? Filters matched gaps by `uniqueness_margin`, then
    /// residual-confirmed same-source, then clean step, and lists the high-uniqueness survivors so they
    /// can be eyeballed. If nothing clean survives, the timing-offset rescue is settled no-go.
    pub fn trustworthy_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let matched = self.matched();
        let unique: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| {
                r.uniqueness_margin
                    .is_some_and(|m| m >= LOW_UNIQUENESS_MARGIN)
            })
            .collect();
        let confirmed: Vec<&&&GapRow> = unique
            .iter()
            .filter(|r| {
                r.residual_informative == Some(true)
                    && r.residual_headroom_db.is_some_and(|h| h <= 0.0)
            })
            .collect();
        let clean: usize = confirmed
            .iter()
            .filter(|r| r.seam_step_ms().is_some_and(|st| st.abs() < CLEAN_STEP_MS))
            .count();

        let _ = writeln!(
            s,
            "=== trustworthy funnel (is any clean recoverable offset left?) ==="
        );
        let _ = writeln!(
            s,
            "  matched {} → unique (margin ≥ {:.2}) {} → +residual-confirmed {} → +clean step (<{:.0} ms) {}",
            matched.len(),
            LOW_UNIQUENESS_MARGIN,
            unique.len(),
            confirmed.len(),
            CLEAN_STEP_MS,
            clean
        );
        if unique.is_empty() {
            let _ = writeln!(
                s,
                "  → no unique-peak gaps: every match is periodicity-suspect. Rescue NO-GO."
            );
            return s;
        }
        let _ = writeln!(
            s,
            "  high-uniqueness survivors (pair idx | offset step uniq | residual | outcome):"
        );
        for r in &unique {
            let _ = writeln!(
                s,
                "    {:<12} g{:<3} | off {:>7.1} step {:>7.1} uniq {:.2} | resid {:>5.1} dB {} | {}",
                r.pair,
                r.index,
                r.seam_mid_ms().unwrap_or(f64::NAN),
                r.seam_step_ms().unwrap_or(f64::NAN),
                r.uniqueness_margin.unwrap_or(f64::NAN),
                r.residual_headroom_db.unwrap_or(f64::NAN),
                if r.residual_informative == Some(true) { "inf" } else { "—" },
                if r.patched() { "patched" } else { "skip" },
            );
        }
        s
    }

    /// **Gate decision** — what actually drives patch vs skip (as opposed to the lag overlay, which is
    /// orthogonal to it). Compares the gate's own scores — **structure** (envelope placement) vs
    /// **seam** (sample-level waveform Pearson) — between patched and skipped gaps, and tallies the
    /// `failure_stage` of the closest bracket on skips. The diagnostic test of "placed by envelope but
    /// rejected by the waveform seam": skipped gaps with structure OK yet seam below the floor.
    pub fn gate_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let matched = self.matched();
        let patched: Vec<&&GapRow> = matched.iter().filter(|r| r.patched()).collect();
        let skipped: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.outcome_tier.as_deref() == Some("skip"))
            .collect();
        let _ = writeln!(
            s,
            "=== gate decision: structure (placement) vs seam (waveform) ==="
        );

        let group = |label: &str, g: &[&&GapRow], out: &mut String| {
            let st = stats(g.iter().filter_map(|r| r.structure_min).collect());
            let sm = stats(g.iter().filter_map(|r| r.seam_min).collect());
            let bb = stats(g.iter().filter_map(|r| r.best_bracket_seam).collect());
            let f = |o: Option<(f64, f64, f64)>| {
                o.map(|(mn, md, mx)| format!("min {mn:.2} / med {md:.2} / max {mx:.2}"))
                    .unwrap_or_else(|| "—".into())
            };
            let _ = writeln!(out, "  {label} ({}):", g.len());
            let _ = writeln!(out, "    structure (envelope) min : {}", f(st));
            let _ = writeln!(out, "    seam (waveform) @throat  : {}", f(sm));
            let _ = writeln!(out, "    best bracket seam        : {}", f(bb));
        };
        group("patched", &patched, &mut s);
        group("skipped", &skipped, &mut s);

        // Skip reasons (closest bracket's failure stage).
        let mut stages: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &skipped {
            *stages
                .entry(r.closest_failure_stage.as_deref().unwrap_or("(none)"))
                .or_default() += 1;
        }
        let _ = writeln!(s, "  skipped failure stages (closest bracket): {stages:?}");

        // The hypothesis test: structure passed (≥ 0.5) but waveform seam below the High floor (0.35).
        let placed_but_rejected = skipped
            .iter()
            .filter(|r| {
                r.structure_min.is_some_and(|v| v >= 0.5) && r.seam_min.is_some_and(|v| v < 0.35)
            })
            .count();
        let _ = writeln!(
            s,
            "  → skipped with structure ≥ 0.5 but waveform seam < 0.35 (placed, waveform-rejected): {}/{}",
            placed_but_rejected,
            skipped.len()
        );
        s
    }

    /// **Seam diagnosis** — the missing measurement. For skipped gaps (dead waveform seam), classify
    /// *why* from the seam probe: `misaligned` (recovers under fine lag → finer alignment), `cross-
    /// encoding` (envelope matches, waveform won't align → encoding-robust metric), `quiet`, or
    /// `unresolved`. This is the data that decides the fix.
    pub fn seam_probe_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(s, "=== seam diagnosis: why is the waveform seam dead? ===");
        let _ = writeln!(
            s,
            "  (NOTE: recov/cross-codec here use the seam_probe (±25 ms pre / ±600 ms sequentially-centered \
             post) — see the silence-splice view for the ±600 ms baseline_lag truth)"
        );
        let matched = self.matched();
        let skipped: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.outcome_tier.as_deref() == Some("skip"))
            .collect();
        if skipped.iter().all(|r| r.seam_diag.is_none()) {
            let _ = writeln!(
                s,
                "  (no seam probe — re-fingerprint with the current binary to populate)"
            );
            return s;
        }
        let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &skipped {
            if let Some(d) = r.seam_diag {
                *tally.entry(d.as_str()).or_default() += 1;
            }
        }
        let _ = writeln!(s, "  skipped seam diagnosis: {tally:?}");

        // The cross-codec hypothesis (plan §3): robust (R2 or R4) ≥ 0.5 while waveform seam < 0.35.
        let with_diag = skipped.iter().filter(|r| r.seam_diag.is_some()).count();
        let robust_high = skipped
            .iter()
            .filter(|r| {
                r.seam_min.is_some_and(|w| w < 0.35)
                    && r.seam_bandlimited_r
                        .unwrap_or(0.0)
                        .max(r.seam_spectrum_r.unwrap_or(0.0))
                        >= SEAM_ROBUST_R
            })
            .count();
        let _ = writeln!(
            s,
            "  hypothesis (robust ≥ {:.2} while waveform < 0.35): {robust_high}/{with_diag}",
            SEAM_ROBUST_R
        );

        for r in &skipped {
            if let Some(d) = r.seam_diag {
                let _ = writeln!(
                    s,
                    "    {:<12} g{:<3} | wav {:.2} R2 {:.2} R4 {:.2} env {:.2} recov {:.2} snr {:>5.1} → {}",
                    r.pair,
                    r.index,
                    r.seam_min.unwrap_or(f64::NAN),
                    r.seam_bandlimited_r.unwrap_or(f64::NAN),
                    r.seam_spectrum_r.unwrap_or(f64::NAN),
                    r.seam_envelope_r.unwrap_or(f64::NAN),
                    r.seam_recovered_r.unwrap_or(f64::NAN),
                    r.seam_snr_db.unwrap_or(f64::NAN),
                    d.as_str()
                );
            }
        }
        s
    }

    /// **Silence-splice view** — the authoritative seam read, from the ±600 ms per-side `baseline_lag`
    /// peaks, sequentially centered (ledger A2) so pre offset and
    /// bridge-length mismatch don't stack into one search window (the ±25 ms `seam_probe.recovered_r`
    /// underlying `seam_probe_text` mislabels any step > 25 ms as "cross-codec"). Classifies every matched
    /// gap: `splice` (both shoulders clean & unique at their own lag, separated by a step — addressable by
    /// independent fit + length reconciliation), `alias-suspect` (a thin uniqueness margin), or
    /// `one-sided-dead` (a shoulder aligns at no lag — the only genuine cross-encoding candidate). Lists
    /// the skipped gaps with both per-side peaks + the step.
    pub fn splice_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "=== silence-splice view (±600 ms sequentially-centered baseline per-side; supersedes the ±25 ms recov diagnosis) ==="
        );
        let matched = self.matched();

        // Tally over all matched gaps (the mechanism is shared patch/skip).
        let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
        let mut classified = 0usize;
        for r in &matched {
            if let Some(d) = r.splice_diag() {
                *tally.entry(d.as_str()).or_default() += 1;
                classified += 1;
            }
        }
        if classified == 0 {
            let _ = writeln!(
                s,
                "  (no per-side baseline_lag peaks — nothing to classify)"
            );
            return s;
        }
        let tstr: Vec<String> = tally.iter().map(|(k, c)| format!("{k} {c}")).collect();
        let _ = writeln!(s, "  among matched ({classified}): {}", tstr.join(" · "));
        let recoverable = matched
            .iter()
            .filter(|r| r.both_sides_recoverable())
            .count();
        let _ = writeln!(
            s,
            "  both-sides-recoverable (peak_r ≥ {:.2} each AND uniqueness ≥ {:.2} or peak_z ≥ {:.0}/prom ≥ {:.2}): {}/{}",
            SPLICE_MIN_PEAK_R, LOW_UNIQUENESS_MARGIN, SPLICE_MIN_PEAK_Z, SPLICE_MIN_PROMINENCE, recoverable, classified
        );
        // Search-exhausted steps: a shoulder peak was clipped at ±max_lag, so `step` is GIGO and the gap is
        // excluded from `dualfit_candidate` (ledger A5/C6). A nonzero count here means widen the lag sweep.
        let edge_pinned = matched.iter().filter(|r| r.step_edge_pinned()).count();
        let edge_pinned_known = matched
            .iter()
            .filter(|r| r.splice_edge_pinned.is_some())
            .count();
        if edge_pinned_known == 0 {
            let _ = writeln!(
                s,
                "  edge-pinned steps: — (pre-flag fingerprint; re-scan to populate)"
            );
        } else {
            let _ = writeln!(
                s,
                "  edge-pinned steps (search-exhausted ⇒ step GIGO, excluded from dual-fit): {}/{}",
                edge_pinned, edge_pinned_known
            );
        }

        // The skipped gaps in detail — these are the repair targets.
        let skipped: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.outcome_tier.as_deref() == Some("skip"))
            .collect();
        if !skipped.is_empty() {
            let _ = writeln!(
                s,
                "  skipped gaps (pre peak@lag | post peak@lag | step | uniq → class):"
            );
            for r in &skipped {
                let base_cls = r.splice_diag().map(|d| d.as_str()).unwrap_or("—");
                let cls = if r.step_edge_pinned() {
                    format!("{base_cls} [edge-pinned]")
                } else {
                    base_cls.to_string()
                };
                let z = r
                    .uniqueness_z
                    .map(|v| format!("z {v:.1}"))
                    .unwrap_or_else(|| "z —".into());
                let _ = writeln!(
                    s,
                    "    {:<12} g{:<3} | pre {:.3}@{:>7.1} | post {:.3}@{:>7.1} | step {:>7.1} | uniq {:.2} {} → {}",
                    r.pair,
                    r.index,
                    r.peak_r_pre.unwrap_or(f64::NAN),
                    r.frac_lag_pre_ms.unwrap_or(f64::NAN),
                    r.peak_r_post.unwrap_or(f64::NAN),
                    r.frac_lag_post_ms.unwrap_or(f64::NAN),
                    r.seam_step_ms().unwrap_or(f64::NAN),
                    r.uniqueness_margin.unwrap_or(f64::NAN),
                    z,
                    cls,
                );
            }
        }

        // One-sided-dead anywhere is the signal that would revive a cross-encoding validator — call it out.
        let dead: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.splice_diag() == Some(SpliceDiag::OneSidedDead))
            .collect();
        let _ = writeln!(
            s,
            "  one-sided-dead (a shoulder aligns at NO lag — would revive cross-encoding): {}",
            dead.len()
        );
        for r in &dead {
            let _ = writeln!(
                s,
                "    {:<12} g{:<3} | pre {:.3} | post {:.3} | {}",
                r.pair,
                r.index,
                r.peak_r_pre.unwrap_or(f64::NAN),
                r.peak_r_post.unwrap_or(f64::NAN),
                if r.patched() { "patched" } else { "skip" },
            );
        }
        s
    }

    /// **Occupancy — dropout vs program-quiet (D11).** Uses the registration-independent nominal-span B
    /// occupancy (`donor_interior_nominal`): a "gap" where B is *also* silent at the same program time is a
    /// program-quiet passage, not a fillable dropout — correctly skipped, and NOT a repair failure. Also
    /// flags nominal-vs-aligned donor disagreement (the per-shoulder registration moved onto other content).
    pub fn occupancy_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "=== occupancy: fillable dropout vs program-quiet (nominal-span B silence, registration-independent) ==="
        );
        let matched = self.matched();
        let have: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.donor_nominal_silence.is_some())
            .collect();
        if have.is_empty() {
            let _ = writeln!(s, "  (no donor_interior_nominal in corpus — re-scan with the current binary to populate)");
            return s;
        }
        let quiet = have
            .iter()
            .filter(|r| r.program_quiet() == Some(true))
            .count();
        let _ = writeln!(
            s,
            "  among matched with nominal donor ({}): dropout {} · program-quiet {} (B ≥ {:.0}% silent at the same program time)",
            have.len(),
            have.len() - quiet,
            quiet,
            PROGRAM_QUIET_SILENCE_FRAC * 100.0
        );
        let skip_quiet: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| {
                r.outcome_tier.as_deref() == Some("skip") && r.program_quiet() == Some(true)
            })
            .collect();
        let _ = writeln!(
            s,
            "  → skipped-and-program-quiet: {} — correctly skipped, NOT fill misses (drop from the addressable denominator)",
            skip_quiet.len()
        );
        let disagree = have
            .iter()
            .filter(|r| r.donor_span_disagrees() == Some(true))
            .count();
        let _ = writeln!(
            s,
            "  nominal-vs-aligned donor disagreement (registration moved span onto other content): {}/{}",
            disagree,
            have.len()
        );
        if !skip_quiet.is_empty() {
            let _ = writeln!(
                s,
                "  program-quiet skips (A gapflr/nflr | B gapflr/nflr | Bsil nom/aln | step):"
            );
            for r in &skip_quiet {
                let o = |v: Option<f64>| {
                    v.map(|x| format!("{x:6.1}"))
                        .unwrap_or_else(|| "   —  ".into())
                };
                let _ = writeln!(
                    s,
                    "    {:<12} g{:<3} | A {}/{} | B {}/{} | {:.2}/{:.2} | step {:>7.1}",
                    r.pair,
                    r.index,
                    o(r.a_gap_floor_db),
                    o(r.a_noise_floor_db),
                    o(r.b_gap_floor_db),
                    o(r.b_noise_floor_db),
                    r.donor_nominal_silence.unwrap_or(f64::NAN),
                    r.donor_aligned_silence.unwrap_or(f64::NAN),
                    r.seam_step_ms().unwrap_or(f64::NAN),
                );
            }
        }
        s
    }

    /// **Dual-fit viability (scan-native `splice_dualfit`; C3/C7).** The offline `diag_splice_dualfit`
    /// simulation promoted into the scan: each shoulder placed at its own baseline lag, seams scored at
    /// lag 0 vs the gate thresholds — "would a length-reconciled fill pass?" — on the scan's own decode
    /// (no separate ffmpeg path). Reports the pass rate among the bracket-exhausted skips and lists each.
    pub fn dualfit_viability_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "=== dual-fit viability (scan-native splice_dualfit; per-shoulder placement, gate-equiv seam @ lag 0) ==="
        );
        let matched = self.matched();
        let measured: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.dualfit_pass.is_some())
            .collect();
        if measured.is_empty() {
            let _ = writeln!(
                s,
                "  (no splice_dualfit in corpus — re-scan with the current binary to populate)"
            );
            return s;
        }
        let pass = measured
            .iter()
            .filter(|r| r.dualfit_pass == Some(true))
            .count();
        let _ = writeln!(
            s,
            "  among matched with splice_dualfit ({}): {pass} would pass a length-reconciled fill",
            measured.len()
        );

        // The decision cohort: bracket-exhausted skips (0 brackets pass) — the dual-fit targets.
        let skips: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.outcome_tier.as_deref() == Some("skip") && r.brackets_passing == 0)
            .collect();
        let skip_pass = skips
            .iter()
            .filter(|r| r.dualfit_pass == Some(true))
            .count();
        let _ = writeln!(
            s,
            "  bracket-exhausted skips: {}/{} would pass dual-fit (the C3 answer)",
            skip_pass,
            skips.len()
        );
        // Validators: is the step necessary (materially improves post vs a constant offset), unique (prom)?
        let step_spurious = skips
            .iter()
            .filter(|r| r.dualfit_pass == Some(true) && !r.step_is_real())
            .count();
        let _ = writeln!(
            s,
            "  validators — of the {skip_pass} dual-fit passes: {} also pass a single global shift (step SPURIOUS ⇒ registration miss, not a splice); {} need the step (real splice)",
            step_spurious,
            skip_pass.saturating_sub(step_spurious)
        );
        // The A3 scope: gate_pass ∧ step-real ∧ donor-continuous ∧ ¬program-quiet (dualfit_target).
        let targets: Vec<String> = matched
            .iter()
            .filter(|r| r.dualfit_target())
            .map(|r| format!("{}·g{}", r.pair, r.index))
            .collect();
        let _ = writeln!(
            s,
            "  ⇒ A3 repair targets (gate_pass ∧ step-real ∧ donor-continuous ∧ ¬program-quiet): {} — {}",
            targets.len(),
            if targets.is_empty() { "(none)".into() } else { targets.join(", ") }
        );
        if !skips.is_empty() {
            let _ = writeln!(
                s,
                "  skipped gaps (dualfit pre | post | post@pre-off | seam-prom | donor | step | br → verdict):"
            );
            for r in &skips {
                let glob = r
                    .dualfit_post_global_r
                    .map(|v| format!("{v:>6.3}"))
                    .unwrap_or_else(|| "  —  ".into());
                let prom = r
                    .dualfit_seam_prom
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| " — ".into());
                let donor = match r.donor_continuous {
                    Some(true) => "cont",
                    Some(false) => "BROKEN",
                    None => "—",
                };
                let verdict = match r.dualfit_pass {
                    Some(true) if r.step_is_real() => "PASS (step real)",
                    Some(true) => "PASS (step spurious)",
                    Some(false) => "fail",
                    None => "—",
                };
                let _ = writeln!(
                    s,
                    "    {:<12} g{:<3} | pre {:>6.3} | post {:>6.3} | {} | prom {} | {:<6} | step {:>7.1} | {}/{} → {}",
                    r.pair,
                    r.index,
                    r.dualfit_pre_r.unwrap_or(f64::NAN),
                    r.dualfit_post_r.unwrap_or(f64::NAN),
                    glob,
                    prom,
                    donor,
                    r.seam_step_ms().unwrap_or(f64::NAN),
                    r.brackets_passing,
                    r.brackets_total,
                    verdict,
                );
            }
            let _ = writeln!(
                s,
                "  (post@pre-off ≥ 0.35 ⇒ a constant shift also fixes it — the \"step\" is a registration artifact.\n   \
                 seam-prom low ⇒ periodic/alias match, PASS not trustworthy.  donor BROKEN ⇒ nothing clean to fill with.)"
            );
        }
        s
    }

    /// **Dual-fit scope (review C1/S4)** — proves patch/skip is *bracket-search success*, not step
    /// magnitude, and narrows the dual-fit target to bracket-exhausted-yet-recoverable skips. Answerable
    /// from `brackets[]` + `baseline_lag` on the *current* corpora (no re-scan needed).
    pub fn dualfit_scope_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let matched = self.matched();
        let patched: Vec<&&GapRow> = matched.iter().filter(|r| r.patched()).collect();
        let skipped: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.outcome_tier.as_deref() == Some("skip"))
            .collect();
        let _ = writeln!(
            s,
            "=== dual-fit scope: patch/skip is bracket success, NOT step magnitude (C1) ==="
        );

        // Bracket-pass vs outcome (they should coincide).
        let patched_with_pass = patched.iter().filter(|r| r.brackets_passing > 0).count();
        let skipped_exhausted = skipped.iter().filter(|r| r.bracket_exhausted()).count();
        let _ = writeln!(
            s,
            "  patched {} ({} have ≥1 passing bracket) · skipped {} ({} bracket-exhausted, 0 passing)",
            patched.len(), patched_with_pass, skipped.len(), skipped_exhausted
        );

        // Step does NOT separate patch from skip — show the overlapping ranges.
        let step_abs = |g: &[&&GapRow]| {
            stats(
                g.iter()
                    .filter_map(|r| r.seam_step_ms().map(f64::abs))
                    .collect(),
            )
        };
        if let (Some((pn, pm, px)), Some((sn, sm, sx))) = (step_abs(&patched), step_abs(&skipped)) {
            let _ = writeln!(s, "  |step| ms  patched: {pn:.1}/{pm:.1}/{px:.1}  ·  skipped: {sn:.1}/{sm:.1}/{sx:.1}  (overlap ⇒ step is not the discriminator)");
        }
        let bb = |g: &[&&GapRow]| stats(g.iter().filter_map(|r| r.best_bracket_seam).collect());
        if let Some((_, pmed, _)) = bb(&patched) {
            let smax = bb(&skipped).map(|(_, _, mx)| mx).unwrap_or(f64::NAN);
            let _ = writeln!(s, "  best-bracket seam  patched median {pmed:.2}  ·  skipped max {smax:.2}  (the real separator)");
        }

        // D11: program-quiet skips (B silent at the same program time) leave the addressable denominator —
        // there is nothing to fill, so they are not dual-fit misses. Report the rate over addressable skips.
        let program_quiet: Vec<&&GapRow> = skipped
            .iter()
            .filter(|r| r.program_quiet_skip())
            .copied()
            .collect();
        let addressable: Vec<&&GapRow> = skipped
            .iter()
            .filter(|r| !r.program_quiet_skip())
            .copied()
            .collect();
        let cand = addressable.iter().filter(|r| r.dualfit_candidate()).count();
        let _ = writeln!(
            s,
            "  dual-fit candidates (skip + bracket-exhausted + both-sides-recoverable): {cand}/{} addressable skips ({} program-quiet dropped, D11)",
            addressable.len(),
            program_quiet.len(),
        );
        let _ = writeln!(
            s,
            "  skipped gaps (step | brackets pass/total | best-seam | recoverable → candidate?):"
        );
        for r in &skipped {
            let verdict = if r.program_quiet_skip() {
                "program-quiet".to_string()
            } else if r.dualfit_candidate() {
                "DUAL-FIT".to_string()
            } else {
                "—".to_string()
            };
            let _ = writeln!(
                s,
                "    {:<12} g{:<3} | step {:>7.1} | {:>2}/{:<2} | best {:.2} | {} → {}",
                r.pair,
                r.index,
                r.seam_step_ms().unwrap_or(f64::NAN),
                r.brackets_passing,
                r.brackets_total,
                r.best_bracket_seam.unwrap_or(f64::NAN),
                if r.both_sides_recoverable() {
                    "recoverable"
                } else {
                    "not-recov  "
                },
                verdict,
            );
        }
        let _ = writeln!(s, "  → dual-fit targets the bracket-exhausted recoverable skips, NOT high-step patches (e.g. 5·g3 patches with a +72 ms step).");
        s
    }

    /// Provenance legend — what each surfaced measurement actually is (representation · window ·
    /// placement), so the numbers are never read without their definition.
    pub fn legend_text(&self) -> String {
        "=== measurement provenance ===\n\
         structure (envelope) : bucketed 50 ms-bin envelope correlation · ~3 s context each side · at the structure/envelope placement\n\
         seam (waveform)      : sample-level Pearson · ~250 ms seam border · at the throat placement · scored at lag 0\n\
         baseline_lag         : waveform correlation sweep · 1 s border · ±600 ms search, post sequentially\n\
                                 centered on pre lag (S + D_A + round(L_pre), not the naive S + D_A) · at\n\
                                 b_mapped (not the throat) · mono; reported gross-relative to b_mapped_end\n\
         splice               : first-class step + per-side peak_r / peak_z from baseline_lag (post − pre);\n\
                                 ≈ bridge-length mismatch (D_B − D_A) once sequential centering is in effect\n\
         wide_envelope        : 100 ms-bin RMS envelope · 2 s window · ±400 ms pre / ±600 ms sequentially-\n\
                                 centered post · segment-identity confirmer · at b_mapped\n\
         offset/step          : (pre+post)/2 and post−pre of baseline_lag (or splice.step_ms), ms\n\
         residual headroom    : sample-level least-squares cancellation (dB vs floor) · ~250 ms seam · at the throat\n\
         uniqueness           : peak_z + prominence at 1 s window (≥12 / ≥0.45); legacy margin = peak_r − 2nd peak\n\
         donor_interior       : B RMS / continuity over the sequentially-aligned bridge span [S + L_pre,\n\
                                 b_mapped_end + L_post_gross) (bridges the hole?) · at b_mapped\n\
         seam probe           : at b_mapped (not the throat) — wav (Pearson@0) · R2 · R4 · env (10 ms-bin) ·\n\
                                 recov (±25 ms pre / ±600 ms sequentially-centered post) · snr (energy-weighted downmix)\n"
            .to_string()
    }

    /// One CSV row per gap for drill-down (RFC 4180 via the `csv` crate).
    pub fn csv(&self) -> String {
        let mut wtr = csv::Writer::from_writer(Vec::new());
        wtr.write_record([
            "pair",
            "a_id",
            "b_id",
            "index",
            "duration_secs",
            "plan_kind",
            "kind",
            "verdict",
            "outcome_tier",
            "patched",
            "frac_lag_pre_ms",
            "frac_lag_post_ms",
            "seam_mid_ms",
            "seam_step_ms",
            "drift_ms",
            "peak_r_pre",
            "peak_r_post",
            "uniqueness_margin",
            "residual_headroom_db",
            "residual_informative",
            "skew",
        ])
        .expect("csv header");
        let opt = |v: Option<f64>| v.map(|x| format!("{x:.4}")).unwrap_or_default();
        let optb = |v: Option<bool>| v.map(|x| x.to_string()).unwrap_or_default();
        for r in &self.rows {
            wtr.write_record([
                r.pair.as_str(),
                r.a_id.as_str(),
                r.b_id.as_str(),
                &r.index.to_string(),
                &opt(r.duration_secs),
                r.plan_kind.as_deref().unwrap_or(""),
                r.kind.as_str(),
                r.verdict.as_deref().unwrap_or(""),
                r.outcome_tier.as_deref().unwrap_or(""),
                &r.patched().to_string(),
                &opt(r.frac_lag_pre_ms),
                &opt(r.frac_lag_post_ms),
                &opt(r.seam_mid_ms()),
                &opt(r.seam_step_ms()),
                &opt(r.drift_ms()),
                &opt(r.peak_r_pre),
                &opt(r.peak_r_post),
                &opt(r.uniqueness_margin),
                &opt(r.residual_headroom_db),
                &optb(r.residual_informative),
                &format!("{:?}", r.skew),
            ])
            .expect("csv row");
        }
        let bytes = wtr.into_inner().expect("csv flush");
        String::from_utf8(bytes).expect("csv utf8")
    }

    /// **Golden baseline for the perf §4 decision-invariance harness.** See [`crate::golden_baseline`].
    pub fn golden_baseline(&self) -> crate::golden_baseline::GoldenBaseline {
        crate::golden_baseline::baseline_from_report(self)
    }

    /// JSON serialization of [`Self::golden_baseline`].
    pub fn golden_json(&self) -> String {
        crate::golden_baseline::golden_json(self)
    }
}
