//! Self-validation for the **W5 timing-offset** fixture (`build_w5_timing_offset_seam`): assert it
//! reproduces the real g003 fingerprint signature — a seam that is **dead at lag 0** (the seam gate's
//! `waveform_floor` skip) yet recovers near-perfect correlation under a multi-millisecond shift (lag
//! verdict `timing_offset`), with the recovered offset **drifting** across the gap (pre ≠ post).
//!
//! Uses the same lag API the fingerprint records (`gap_fingerprint::{lag_correlation_curve,
//! summarize_lag_curve}`) so this characterizes the fixture against the production discriminator.
//! Default tier (one fixture, direct windows, no `PatchAudio`). See
//! `docs/dev/archive/TEMP-w5-timing-offset-diag-plan.md` §3/§5 Phase B/D and the curated real exemplar
//! `tests/gap_corpus/fingerprints/g003_timing_offset.json`.

use clip_sync_repair::application::gap_fingerprint::{GateOutcome, LagFingerprint, LagVerdict};
use clip_sync_repair_fixtures::energy_signature_fixtures::build_w5_timing_offset_seam;
use clip_sync_repair_fixtures::w5_timing_offset_diag::w5_timing_offset_seam_lag;
use serde::Deserialize;

const SR: u32 = 48_000;

/// The committed real g003 fingerprint (Phase D regression anchor) — a single **curated** exemplar
/// under the test tree, deliberately kept separate from the gitignored `gap-files/` scratch/output dir
/// (which `--gap-fingerprints` overwrites). Licensing-safe: numbers only, no samples; the identifying
/// `id → title` map stays in a gitignored `*.sources.local.toml`.
const G003_FINGERPRINT: &str = "tests/gap_corpus/fingerprints/g003_timing_offset.json";

/// The fixture reproduces the g003 signature: both seams are `timing_offset` (lag-0 dead, a
/// multi-ms shift recovers r≈1), and the recovered offset drifts pre→post (the seam skew).
#[test]
fn timing_offset_fixture_reproduces_g003_signature() {
    let seam_offset_ms = 16.0;
    let drift_ppm = -4_500.0; // g003-like skew; lag decreases with time → |pre| > |post| (−16 vs −8 ms)
    let fixture =
        build_w5_timing_offset_seam(SR, 1, 1.0, 0.3, seam_offset_ms, drift_ppm);

    // Probe inside the collar with a guard so the shifted B window never crosses into the throat fill
    // (which would dilute the recovered peak). A short 80 ms window keeps within-window lag drift well
    // under the collar's autocorrelation width.
    let lag = w5_timing_offset_seam_lag(&fixture, 0.08, 0.040).expect("seam lag in range");
    let (pre, post) = (lag.pre, lag.post);

    // 1. Dead at lag 0 — this is the `waveform_floor` skip the seam gate produces.
    assert!(
        pre.lag0_r < 0.3 && post.lag0_r < 0.3,
        "expected lag-0 seam dead: pre_lag0={:.3} post_lag0={:.3}",
        pre.lag0_r,
        post.lag0_r
    );

    // 2. A shift recovers near-perfect correlation → recoverable timing offset.
    assert_eq!(pre.verdict, LagVerdict::TimingOffset, "pre: {pre:?}");
    assert_eq!(post.verdict, LagVerdict::TimingOffset, "post: {post:?}");
    assert!(
        pre.peak_r > 0.9 && post.peak_r > 0.9,
        "expected strong recovered peak: pre_peak={:.3} post_peak={:.3}",
        pre.peak_r,
        post.peak_r
    );

    // 3. The recovered offset is multi-ms and positive (B delayed), bracketing the 16 ms anchor.
    assert!(
        (5.0..35.0).contains(&pre.frac_lag_ms) && (5.0..35.0).contains(&post.frac_lag_ms),
        "expected multi-ms recovered offsets near {seam_offset_ms} ms: pre={:.2} post={:.2}",
        pre.frac_lag_ms,
        post.frac_lag_ms
    );

    // 4. The offset DRIFTS across the gap (pre ≠ post) — the defining g003 asymmetry — and in the
    //    g003 direction (|pre| > |post|).
    assert!(
        (pre.frac_lag_ms - post.frac_lag_ms).abs() > 4.0,
        "expected pre↔post drift > 4 ms: pre={:.2} post={:.2}",
        pre.frac_lag_ms,
        post.frac_lag_ms
    );
    assert!(
        pre.frac_lag_ms.abs() > post.frac_lag_ms.abs(),
        "expected |pre| > |post| (g003 direction): pre={:.2} post={:.2}",
        pre.frac_lag_ms,
        post.frac_lag_ms
    );
}

/// Sanity: the envelope/structure is undisturbed by the sub-frame skew — a direct lag-0 Pearson on
/// the raw seam is dead, but the 50 ms-bin RMS envelope of A's collar matches B's (the fixture's whole
/// point: structure aligns while the waveform seam does not). Cheap proxy: collar RMS is comparable.
#[test]
fn timing_offset_fixture_envelope_is_preserved() {
    let fixture = build_w5_timing_offset_seam(SR, 1, 1.0, 0.3, 16.0, -8_000.0);
    let collar = (0.2 * f64::from(SR)) as usize;
    let a_rms = rms(&fixture.a_samples, fixture.gap_start - collar, fixture.gap_start);
    let b_rms = rms(&fixture.b_samples, fixture.gap_start - collar, fixture.gap_start);
    assert!(a_rms > 0.01, "collar should carry content: a_rms={a_rms:.4}");
    assert!(
        (a_rms - b_rms).abs() / a_rms < 0.2,
        "collar RMS should survive the resample: a={a_rms:.4} b={b_rms:.4}"
    );
}

fn rms(samples: &[f32], start: usize, end: usize) -> f64 {
    let slice = &samples[start..end];
    let sum: f64 = slice.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / slice.len().max(1) as f64).sqrt()
}

/// **Phase D** — the committed real g003 fingerprint is the canonical `timing_offset` exemplar the
/// synthetic fixture is calibrated against. Loads the curated copy from the test tree and asserts the
/// class signature: every measured seam `timing_offset` (lag-0 dead, strong recovered peak), the
/// recovered offset drifts pre→post (`|pre| > |post|`), and the gate **skipped** the gap (the bug the
/// class names).
#[test]
fn g003_real_fingerprint_is_timing_offset_exemplar() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(G003_FINGERPRINT);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read curated g003 fingerprint {}: {e}", path.display()));
    // Parse only the fields the timing-offset class is about (`index`, `lag`, `outcome`) via a minimal
    // struct, so the regression test is robust to unrelated schema drift in the full `GapCorpus` (the
    // committed exemplar predates a `seams.per_channel` schema change and no longer round-trips whole).
    #[derive(Deserialize)]
    struct G003Corpus {
        gaps: Vec<G003Gap>,
    }
    #[derive(Deserialize)]
    struct G003Gap {
        index: usize,
        lag: Option<LagFingerprint>,
        outcome: Option<GateOutcome>,
    }

    let corpus: G003Corpus = serde_json::from_str(&text).expect("parse g003 fingerprint corpus");

    assert_eq!(corpus.gaps.len(), 1, "single-gap library file");
    let gap = &corpus.gaps[0];
    assert_eq!(gap.index, 3, "g003");

    let lag = gap.lag.as_ref().expect("full-tier g003 carries a lag fingerprint");
    assert!(
        !lag.pre_anchor.is_empty() && !lag.post_anchor.is_empty(),
        "g003 lag fingerprint has pre and post seams"
    );
    // Every measured seam (mono + selected channel) recovers under a shift → timing_offset, not
    // decorrelated. This is the discriminator the synthetic fixture reproduces (see Phase B test).
    for s in lag.pre_anchor.iter().chain(lag.post_anchor.iter()) {
        assert_eq!(s.verdict, LagVerdict::TimingOffset, "g003 seam verdict: {s:?}");
        assert!(s.peak_r > 0.9, "g003 recovers strongly under a shift: {s:?}");
        assert!(s.lag0_r.abs() < 0.35, "g003 seam dead at lag 0: {s:?}");
    }

    // The recovered offset drifts across the gap, |pre| > |post| (−16 vs −8 ms) — the asymmetry the
    // synthetic `build_w5_timing_offset_seam` models with its negative `drift_ppm`.
    let pre_ms = lag.pre_anchor[0].frac_lag_ms.abs();
    let post_ms = lag.post_anchor[0].frac_lag_ms.abs();
    assert!(
        pre_ms > post_ms,
        "g003 seam skew: |pre|={pre_ms:.2} ms should exceed |post|={post_ms:.2} ms"
    );

    // The gate skipped it — the failure the class names (recoverable seam read as decorrelated).
    let outcome = gap.outcome.as_ref().expect("g003 carries a gate outcome");
    assert_eq!(outcome.tier, "skip", "g003 was skipped by the gate");
}
