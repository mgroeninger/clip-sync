//! **Offline dual-fit gate simulation** (`docs/TEMP-seam-splice-dualfit-plan.md` §4; ledger C3/C7).
//!
//! Tier: **diagnostic** (`diagnostic-tests`) — shells out to **ffmpeg**. Reads `corpus.json` for geometry
//! and `baseline_lag` / `splice`, decodes A/B around each gap, applies **independent per-side lags** at
//! `b_mapped`, reconciles interior length (`trim` / `pad` `|step|`), and scores pre/post seam Pearson with
//! the same window the production gate uses (`fill_seam_search_secs`).
//!
//! This is **not** the repair pipeline — it de-risks P1 (gate pass after reconciliation) and P2/C7 (trim ≈
//! `step_ms`) before wiring §4.
//!
//! ```powershell
//! $env:SPLICE_DUALFIT_CORPUS = "gap-files/1/corpus.json"   # or SPLICE_EXP_CORPUS
//! $env:SPLICE_DUALFIT_A = "F:\Video\A.mkv"                 # or SPLICE_EXP_A
//! $env:SPLICE_DUALFIT_B = "F:\Video\B.m4v"                 # or SPLICE_EXP_B
//! # optional: SPLICE_DUALFIT_GAPS=19,22   (default: skipped gaps with baseline_lag)
//! # optional: SPLICE_DUALFIT_MIN_CORR=0.35  SPLICE_DUALFIT_ABS_FLOOR=0.12
//! cargo test -p clip-sync-repair --features diagnostic-tests --test diag_splice_dualfit -- --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use clip_sync::normalized_correlation;
use serde::Deserialize;

// ── corpus projection ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CorpusFile {
    #[serde(default)]
    gaps: Vec<GapEntry>,
}

#[derive(Deserialize)]
struct GapEntry {
    index: usize,
    #[serde(default)]
    geometry: Option<Geometry>,
    #[serde(default)]
    baseline_lag: Option<BaselineLag>,
    #[serde(default)]
    splice: Option<SpliceSummary>,
    #[serde(default)]
    outcome: Option<Outcome>,
    #[serde(default)]
    brackets: Vec<BracketInfo>,
}

#[derive(Deserialize, Clone)]
struct Geometry {
    a_refined_start_secs: f64,
    a_refined_end_secs: f64,
    b_mapped_start_secs: f64,
    b_mapped_end_secs: f64,
    #[serde(default)]
    duration_secs: Option<f64>,
}

#[derive(Deserialize)]
struct BaselineLag {
    #[serde(default)]
    pre_anchor: Vec<LagEntry>,
    #[serde(default)]
    post_anchor: Vec<LagEntry>,
}

#[derive(Deserialize)]
struct LagEntry {
    window_ms: u32,
    frac_lag_ms: f64,
    peak_r: f64,
    #[serde(default)]
    peak_z: Option<f64>,
    channel: serde_json::Value,
}

#[derive(Deserialize)]
struct SpliceSummary {
    step_ms: f64,
    #[serde(default)]
    pre_peak_r: Option<f64>,
    #[serde(default)]
    post_peak_r: Option<f64>,
}

#[derive(Deserialize)]
struct Outcome {
    tier: String,
    #[serde(default)]
    skip_reason: Option<String>,
}

#[derive(Deserialize)]
struct BracketInfo {
    #[serde(default)]
    seam_pre: Option<f64>,
    #[serde(default)]
    seam_post: Option<f64>,
    #[serde(default)]
    failure_stage: Option<String>,
}

impl LagEntry {
    fn is_mono(&self) -> bool {
        self.channel.as_str().is_some_and(|c| c == "mono")
    }
}

fn mono_lag<'a>(entries: &'a [LagEntry]) -> Option<&'a LagEntry> {
    entries.iter().filter(|e| e.is_mono()).max_by_key(|e| e.window_ms)
}

// ── tunables ─────────────────────────────────────────────────────────────────────

const PAD_SECS: f64 = 2.5;

fn env_str(primary: &str, fallback: &str) -> Option<String> {
    std::env::var(primary)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var(fallback).ok().filter(|s| !s.trim().is_empty()))
}

fn sr() -> u32 {
    env_str("SPLICE_DUALFIT_SR", "SPLICE_EXP_SR")
        .and_then(|v| v.parse().ok())
        .unwrap_or(48_000)
}

fn seam_secs() -> f64 {
    env_str("SPLICE_DUALFIT_SEAM_SECS", "")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.25)
}

fn min_fill_correlation() -> f64 {
    env_str("SPLICE_DUALFIT_MIN_CORR", "")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.35)
}

fn fill_absolute_floor() -> f64 {
    env_str("SPLICE_DUALFIT_ABS_FLOOR", "")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.12)
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn resolve(p: &str) -> PathBuf {
    let p = Path::new(p);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root().join(p)
    }
}

// ── decode (same pattern as diag_splice_timescale) ───────────────────────────────

struct Pcm {
    samples: Vec<f32>,
    channels: usize,
}

impl Pcm {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    fn mono(&self) -> Vec<f64> {
        let n = self.channels.max(1);
        self.samples
            .chunks(n)
            .map(|f| f.iter().map(|&s| s as f64).sum::<f64>() / n as f64)
            .collect()
    }
}

fn probe_audio_channels(input: &Path) -> Vec<usize> {
    #[derive(Deserialize)]
    struct Stream {
        #[serde(default)]
        channels: Option<usize>,
        #[serde(default)]
        channel_layout: Option<String>,
    }
    #[derive(Deserialize)]
    struct Probe {
        #[serde(default)]
        streams: Vec<Stream>,
    }
    let layout_ch = |l: Option<&str>| match l.unwrap_or("") {
        x if x.starts_with("7.1") => 8,
        x if x.starts_with("6.1") => 7,
        x if x.starts_with("5.1") => 6,
        x if x.starts_with("quad") || x.starts_with("4.0") => 4,
        x if x.starts_with("stereo") || x.starts_with("downmix") => 2,
        "mono" => 1,
        _ => 0,
    };
    Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=channels,channel_layout",
            "-of",
            "json",
        ])
        .arg(input)
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice::<Probe>(&o.stdout).ok())
        .map(|p| {
            p.streams
                .into_iter()
                .map(|s| {
                    s.channels
                        .filter(|&c| c > 0)
                        .unwrap_or_else(|| layout_ch(s.channel_layout.as_deref()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn decode_span(input: &Path, stream: usize, start_secs: f64, dur_secs: f64, sample_rate: u32) -> Option<Pcm> {
    let tmp = std::env::temp_dir().join(format!("dualfit_{}.wav", std::process::id()));
    // Sample-accurate seek: a fast keyframe seek to `coarse` (before `-i`) followed by an accurate
    // seek of the `fine` remainder (after `-i`). Input-only `-ss` snaps to a keyframe and offsets the
    // whole decoded span by a codec-dependent constant (~75 ms observed), which reads as a dead seam
    // even when the corpus lag is correct. The coarse+fine combo decodes only ~`COARSE`+dur seconds
    // accurately, so it's both fast and exact.
    const COARSE_LEAD_SECS: f64 = 20.0;
    let start = start_secs.max(0.0);
    let coarse = (start - COARSE_LEAD_SECS).max(0.0);
    let fine = start - coarse;
    let status = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error"])
        .args(["-ss", &format!("{coarse:.6}")])
        .arg("-i")
        .arg(input)
        .args(["-ss", &format!("{fine:.6}")])
        .args(["-t", &format!("{:.6}", dur_secs.max(0.0))])
        .args([
            "-vn",
            "-map",
            &format!("0:a:{stream}"),
            "-c:a",
            "pcm_f32le",
            "-ar",
            &sample_rate.to_string(),
            "-f",
            "wav",
        ])
        .arg(&tmp)
        .status()
        .ok()?;
    if !status.success() {
        eprintln!("ffmpeg decode failed: {} @ {start_secs:.2}s+{dur_secs:.2}s", input.display());
        return None;
    }
    let mut reader = hound::WavReader::open(&tmp).ok()?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / max)
                .collect()
        }
    };
    let _ = std::fs::remove_file(&tmp);
    Some(Pcm {
        samples,
        channels: spec.channels as usize,
    })
}

fn rms(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
}

/// Pearson @ lag 0 on equal-length windows.
fn seam_r(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n < 8 {
        return f64::NAN;
    }
    normalized_correlation(&a[..n], &b[..n])
}

/// **Decode-bug detector.** Hold `a_win` fixed and slide the B seam window by ±`search` samples around
/// `b_anchor`, returning the best Pearson and the offset (samples) that produced it. `pre` selects the
/// window that *ends* at the anchor (pre shoulder) vs *starts* at it (post shoulder). If the fixed-lag
/// seam reads ~0 but a search here recovers a high `r` at a roughly constant offset across gaps, the
/// content is present and correctly registered — the sim's absolute decode indexing is just shifted
/// (an ffmpeg `-ss` input-seek artifact), not a genuine dual-fit failure.
fn seam_search(a_win: &[f64], b: &[f64], b_anchor: i64, pre: bool, seam_w: usize, search: i64) -> (f64, i64) {
    let mut best_r = f64::NAN;
    let mut best_off = 0i64;
    for d in -search..=search {
        let anchor = b_anchor + d;
        let (lo, hi) = if pre {
            (anchor - seam_w as i64, anchor)
        } else {
            (anchor, anchor + seam_w as i64)
        };
        if lo < 0 || hi as usize > b.len() {
            continue;
        }
        let r = seam_r(a_win, &b[lo as usize..hi as usize]);
        if r.is_finite() && (best_r.is_nan() || r > best_r) {
            best_r = r;
            best_off = d;
        }
    }
    (best_r, best_off)
}

// ── dual-fit simulation ────────────────────────────────────────────────────────────

struct SimResult {
    pre_lag_ms: f64,
    post_lag_ms: f64,
    pre_peak_r: f64,
    post_peak_r: f64,
    step_ms: f64,
    step_ms_fingerprint: Option<f64>,
    gap_frames: usize,
    bridge_frames: i64,
    trim_frames: i64,
    reconcile_delta: i64,
    pre_seam_r: f64,
    post_seam_r: f64,
    pre_search_r: f64,
    pre_search_off_ms: f64,
    post_search_r: f64,
    post_search_off_ms: f64,
    calib_off_ms: f64,
    calib_lock_r: f64,
    pre_seam_cal_r: f64,
    post_seam_cal_r: f64,
    gate_pass: bool,
    global_shift_ms: f64,
    post_seam_global_r: f64,
    global_pass: bool,
    pre_align1_r: f64,
    pre_align1_off_ms: f64,
    post_align1_r: f64,
    post_align1_off_ms: f64,
    best_bracket_seam: Option<f64>,
    brackets_passing: usize,
}

/// Reconcile bridge length by removing `trim` samples centered on the lowest-RMS interior frame.
fn trim_at_min_energy(bridge: &[f64], trim: usize) -> Vec<f64> {
    if trim == 0 || bridge.len() <= trim + 2 {
        return bridge.to_vec();
    }
    let interior_lo = 1;
    let interior_hi = bridge.len().saturating_sub(1);
    if interior_hi <= interior_lo {
        return bridge.to_vec();
    }
    let cut_at = (interior_lo..interior_hi)
        .min_by(|&i, &j| rms(&bridge[i..i + 1]).partial_cmp(&rms(&bridge[j..j + 1])).unwrap())
        .unwrap_or(interior_lo);
    let half = trim / 2;
    let lo = cut_at.saturating_sub(half);
    let hi = (lo + trim).min(bridge.len());
    let lo = hi.saturating_sub(trim);
    let mut out = Vec::with_capacity(bridge.len() - trim);
    out.extend_from_slice(&bridge[..lo]);
    out.extend_from_slice(&bridge[hi..]);
    out
}

fn pad_zeros(bridge: &[f64], pad: usize) -> Vec<f64> {
    if pad == 0 {
        return bridge.to_vec();
    }
    let mut out = bridge.to_vec();
    let insert = bridge
        .iter()
        .enumerate()
        .skip(1)
        .take(bridge.len().saturating_sub(2))
        .min_by(|(_, a), (_, b)| a.abs().partial_cmp(&b.abs()).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(bridge.len() / 2);
    out.splice(insert..insert, std::iter::repeat_n(0.0, pad));
    out
}

fn reconcile_bridge(bridge: &[f64], gap_frames: usize) -> (Vec<f64>, i64) {
    let delta = bridge.len() as i64 - gap_frames as i64;
    if delta > 0 {
        let trimmed = trim_at_min_energy(bridge, delta as usize);
        (trimmed, delta)
    } else if delta < 0 {
        let padded = pad_zeros(bridge, (-delta) as usize);
        (padded, delta)
    } else {
        (bridge.to_vec(), 0)
    }
}

fn run_gap(
    gap: &GapEntry,
    geo: &Geometry,
    a_path: &Path,
    b_path: &Path,
    a_stream: usize,
    b_stream: usize,
    rate: u32,
) -> Option<SimResult> {
    let bl = gap.baseline_lag.as_ref()?;
    let pre = mono_lag(&bl.pre_anchor)?;
    let post = mono_lag(&bl.post_anchor)?;

    let r = f64::from(rate);
    let seam_w = (seam_secs() * r).round() as usize;
    let pad = PAD_SECS;

    let a_start = geo.a_refined_start_secs - pad;
    let a_span = (geo.a_refined_end_secs + pad) - a_start;
    let b_start = geo.b_mapped_start_secs - pad;
    let b_span = (geo.b_mapped_end_secs + pad) - b_start;

    let a_pcm = decode_span(a_path, a_stream, a_start, a_span, rate)?;
    let b_pcm = decode_span(b_path, b_stream, b_start, b_span, rate)?;
    let a = a_pcm.mono();
    let b = b_pcm.mono();

    let a_pre = ((geo.a_refined_start_secs - a_start) * r).round() as usize;
    let a_post = ((geo.a_refined_end_secs - a_start) * r).round() as usize;
    let b_pre = ((geo.b_mapped_start_secs - b_start) * r).round() as usize;
    let b_post = ((geo.b_mapped_end_secs - b_start) * r).round() as usize;

    let lag_pre = (pre.frac_lag_ms / 1000.0 * r).round() as i64;
    let lag_post = (post.frac_lag_ms / 1000.0 * r).round() as i64;
    let b_pre_al = (b_pre as i64 + lag_pre).max(0) as usize;
    let b_post_al = (b_post as i64 + lag_post).max(0) as usize;

    if a_pre < seam_w || a_post + seam_w > a.len() {
        eprintln!("  gap {}: A window out of range", gap.index);
        return None;
    }
    if b_pre_al < seam_w || b_post_al + seam_w > b.len() {
        eprintln!("  gap {}: B window out of range (lags {lag_pre}/{lag_post})", gap.index);
        return None;
    }

    let pre_seam_r = seam_r(&a[a_pre - seam_w..a_pre], &b[b_pre_al - seam_w..b_pre_al]);
    let post_seam_r = seam_r(&a[a_post..a_post + seam_w], &b[b_post_al..b_post_al + seam_w]);

    // Decode-bug detector: re-search each seam ±400 ms around the corpus-lag placement.
    let search = (0.400 * r).round() as i64;
    let (pre_search_r, pre_search_off) =
        seam_search(&a[a_pre - seam_w..a_pre], &b, b_pre_al as i64, true, seam_w, search);
    let (post_search_r, post_search_off) =
        seam_search(&a[a_post..a_post + seam_w], &b, b_post_al as i64, false, seam_w, search);

    // Alias-vs-decode discriminator: re-search with a 1 s window (the SAME border the fingerprint
    // scan's `baseline_lag` uses) instead of the 250 ms seam. A 250 ms window can lock onto a periodic
    // false peak; a 1 s window rejects it. The offsets below are reported relative to the CORPUS lag, so:
    //   post 1 s-offset ≈ 0        ⇒ the 1 s search agrees with the scan's step (step is REAL; the 250 ms
    //                                 "common offset" was a short-window alias).
    //   post 1 s-offset ≈ −step    ⇒ the 1 s search still collapses to the common offset even at 1 s
    //                                 (the two decodes genuinely disagree — a decode mismatch, not alias).
    let w1 = rate as usize; // 1 s
    let (pre_align1_r, pre_align1_off) = if a_pre >= w1 && b_pre_al >= w1 + search as usize {
        seam_search(&a[a_pre - w1..a_pre], &b, b_pre_al as i64, true, w1, search)
    } else {
        (f64::NAN, 0)
    };
    let (post_align1_r, post_align1_off) = if a_post + w1 <= a.len() {
        seam_search(&a[a_post..a_post + w1], &b, b_post_al as i64, false, w1, search)
    } else {
        (f64::NAN, 0)
    };

    // Common-mode decode-delay calibration. The sim's independent ffmpeg decodes carry a constant
    // ~75 ms A/B priming/delay that accurate seeking does not remove. Measure it from the PRE shoulder
    // within a narrow ±150 ms window (alias-guarded, so a periodic post shoulder can't drag it), then
    // apply the SAME offset to BOTH seams and re-score at the corpus lag. This neutralizes the constant
    // decode delay WITHOUT independently re-fitting each shoulder — so a genuine step (or a shoulder that
    // only aligns at a different lag, i.e. alias/one-sided) still fails, exactly as the real gate would.
    let calib_search = (0.150 * r).round() as i64;
    let (calib_lock_r, calib_off) =
        seam_search(&a[a_pre - seam_w..a_pre], &b, b_pre_al as i64, true, seam_w, calib_search);
    let post_cal = b_post_al as i64 + calib_off;
    // Pre seam at the calibrated anchor is exactly the calibration lock by construction.
    let pre_seam_cal_r = calib_lock_r;
    let post_seam_cal_r = if post_cal >= seam_w as i64 && (post_cal + seam_w as i64) as usize <= b.len() {
        seam_r(
            &a[a_post..a_post + seam_w],
            &b[post_cal as usize..(post_cal + seam_w as i64) as usize],
        )
    } else {
        f64::NAN
    };

    // Global single-shift mode: force step = 0 by placing BOTH shoulders at the PRE offset
    // (`b_mapped + lag_pre + C`), ignoring the corpus post lag entirely. If the post seam cleans up
    // here, the corpus's step is spurious and the gap is a constant-offset registration miss — a single
    // global shift patches it, no dual-fit trim/pad needed. If it stays dead while the calibrated
    // (per-shoulder) post is clean, the step is real and dual-fit is the right tool.
    let post_global = b_post as i64 + lag_pre + calib_off;
    let post_seam_global_r = if post_global >= seam_w as i64 && (post_global + seam_w as i64) as usize <= b.len()
    {
        seam_r(
            &a[a_post..a_post + seam_w],
            &b[post_global as usize..(post_global + seam_w as i64) as usize],
        )
    } else {
        f64::NAN
    };

    let gap_frames = a_post.saturating_sub(a_pre);

    let (bridge_lo, bridge_hi) = if b_post_al >= b_pre_al {
        (b_pre_al, b_post_al)
    } else {
        (b_post_al, b_pre_al)
    };
    let bridge: Vec<f64> = if bridge_hi > bridge_lo {
        b[bridge_lo..bridge_hi].to_vec()
    } else {
        Vec::new()
    };
    let step_ms = post.frac_lag_ms - pre.frac_lag_ms;
    let (_reconciled, reconcile_delta) = reconcile_bridge(&bridge, gap_frames);

    let min_corr = min_fill_correlation();
    let abs_floor = fill_absolute_floor();
    // Gate on the common-mode-calibrated seams (the raw fixed-lag seams are still printed for reference).
    let smin = pre_seam_cal_r.min(post_seam_cal_r);
    let gate_pass = smin.is_finite() && smin >= min_corr && smin >= abs_floor;
    // Global single-shift gate: pre seam (== calib lock) + the step-0 post seam.
    let smin_global = pre_seam_cal_r.min(post_seam_global_r);
    let global_pass = smin_global.is_finite() && smin_global >= min_corr && smin_global >= abs_floor;

    let brackets_passing = gap
        .brackets
        .iter()
        .filter(|br| br.failure_stage.is_none())
        .count();
    let best_bracket_seam = gap.brackets.iter().filter_map(|br| match (br.seam_pre, br.seam_post) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        _ => None,
    }).max_by(|a, b| a.partial_cmp(b).unwrap());

    Some(SimResult {
        pre_lag_ms: pre.frac_lag_ms,
        post_lag_ms: post.frac_lag_ms,
        pre_peak_r: pre.peak_r,
        post_peak_r: post.peak_r,
        step_ms,
        step_ms_fingerprint: gap.splice.as_ref().map(|s| s.step_ms),
        gap_frames,
        bridge_frames: bridge.len() as i64,
        trim_frames: bridge.len() as i64 - gap_frames as i64,
        reconcile_delta,
        pre_seam_r,
        post_seam_r,
        pre_search_r,
        pre_search_off_ms: pre_search_off as f64 * 1000.0 / r,
        post_search_r,
        post_search_off_ms: post_search_off as f64 * 1000.0 / r,
        calib_off_ms: calib_off as f64 * 1000.0 / r,
        calib_lock_r,
        pre_seam_cal_r,
        post_seam_cal_r,
        gate_pass,
        global_shift_ms: (lag_pre + calib_off) as f64 * 1000.0 / r,
        post_seam_global_r,
        global_pass,
        pre_align1_r,
        pre_align1_off_ms: pre_align1_off as f64 * 1000.0 / r,
        post_align1_r,
        post_align1_off_ms: post_align1_off as f64 * 1000.0 / r,
        best_bracket_seam,
        brackets_passing,
    })
}

fn print_gap(gap: &GapEntry, geo: &Geometry, sim: &SimResult) {
    let dur = geo.duration_secs.unwrap_or(geo.a_refined_end_secs - geo.a_refined_start_secs);
    let tier = gap.outcome.as_ref().map(|o| o.tier.as_str()).unwrap_or("?");
    println!(
        "\n──────── gap {}  (A {:.3}..{:.3}s, dur {:.3}s, outcome {}) ────────",
        gap.index, geo.a_refined_start_secs, geo.a_refined_end_secs, dur, tier
    );
    println!(
        "  registration (baseline_lag mono):  pre {:+.1} ms (r {:.3})  post {:+.1} ms (r {:.3})  step {:+.1} ms{}",
        sim.pre_lag_ms,
        sim.pre_peak_r,
        sim.post_lag_ms,
        sim.post_peak_r,
        sim.step_ms,
        sim.step_ms_fingerprint
            .map(|s| format!("  (fingerprint splice {s:+.1} ms)"))
            .unwrap_or_default()
    );
    println!(
        "  length:  gap {} smp  bridge {} smp  Δtrim {} smp  (reconcile Δ {})",
        sim.gap_frames, sim.bridge_frames, sim.trim_frames, sim.reconcile_delta
    );
    println!(
        "  post-dual-fit seams @ lag 0 ({:.0} ms window):  pre {:.3}  post {:.3}  min {:.3}",
        seam_secs() * 1000.0,
        sim.pre_seam_r,
        sim.post_seam_r,
        sim.pre_seam_r.min(sim.post_seam_r)
    );
    println!(
        "  seam re-search (±400 ms around corpus lag):  pre {:.3} @ {:+.1} ms  post {:.3} @ {:+.1} ms  \
         → {}",
        sim.pre_search_r,
        sim.pre_search_off_ms,
        sim.post_search_r,
        sim.post_search_off_ms,
        if sim.pre_search_r.min(sim.post_search_r) >= 0.7 {
            "content present — absolute decode indexing shifted (harness decode bug)"
        } else {
            "no lag recovers the seam — genuine failure"
        }
    );
    // 1 s-window alignment (matches the scan's baseline_lag border) — the alias-vs-decode discriminator.
    // Offsets are relative to the corpus lag; post ≈ 0 ⇒ agrees with the scan's step (step real, 250 ms
    // was aliasing); post ≈ −(corpus step) ⇒ collapses to common offset even at 1 s (decode mismatch).
    let post_agrees_scan = sim.post_align1_off_ms.abs() <= 20.0;
    println!(
        "  1 s-window alignment (scan window; ±400 ms):  pre {:.3} @ {:+.1} ms  post {:.3} @ {:+.1} ms  → {}",
        sim.pre_align1_r,
        sim.pre_align1_off_ms,
        sim.post_align1_r,
        sim.post_align1_off_ms,
        if !sim.post_align1_r.is_finite() {
            "n/a"
        } else if post_agrees_scan {
            "AGREES with corpus step (step REAL; 250 ms was aliasing)"
        } else {
            "collapses off corpus lag (250 ms result persists at 1 s — decode mismatch vs scan)"
        }
    );
    println!(
        "  common-mode calibrated seams (decode delay {:+.1} ms, pre lock r {:.3}):  pre {:.3}  post {:.3}  min {:.3}",
        sim.calib_off_ms,
        sim.calib_lock_r,
        sim.pre_seam_cal_r,
        sim.post_seam_cal_r,
        sim.pre_seam_cal_r.min(sim.post_seam_cal_r)
    );
    println!(
        "  global single-shift (step forced 0, both shoulders @ {:+.1} ms):  pre {:.3}  post {:.3}  min {:.3}  →  {}",
        sim.global_shift_ms,
        sim.pre_seam_cal_r,
        sim.post_seam_global_r,
        sim.pre_seam_cal_r.min(sim.post_seam_global_r),
        if sim.global_pass { "GLOBAL-PASS (step spurious)" } else { "global-fail" }
    );
    println!(
        "  gate thresholds (dual-fit / calibrated seams):  min_corr {:.2}  abs_floor {:.2}  →  {}",
        min_fill_correlation(),
        fill_absolute_floor(),
        if sim.gate_pass { "PASS" } else { "FAIL" }
    );
    println!(
        "  fingerprint brackets:  {}/{} pass  best seam {:?}",
        sim.brackets_passing,
        gap.brackets.len(),
        sim.best_bracket_seam
    );
}

#[test]
fn diag_splice_dualfit() {
    let corpus = env_str("SPLICE_DUALFIT_CORPUS", "SPLICE_EXP_CORPUS").expect(
        "set SPLICE_DUALFIT_CORPUS or SPLICE_EXP_CORPUS to a pair corpus.json",
    );
    let a_media = env_str("SPLICE_DUALFIT_A", "SPLICE_EXP_A")
        .expect("set SPLICE_DUALFIT_A or SPLICE_EXP_A");
    let b_media = env_str("SPLICE_DUALFIT_B", "SPLICE_EXP_B")
        .expect("set SPLICE_DUALFIT_B or SPLICE_EXP_B");

    let corpus_path = resolve(&corpus);
    let a_path = resolve(&a_media);
    let b_path = resolve(&b_media);
    let rate = sr();

    let text = std::fs::read_to_string(&corpus_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", corpus_path.display()));
    let file: CorpusFile = serde_json::from_str(&text).expect("parse corpus.json");

    let only: Option<Vec<usize>> = std::env::var("SPLICE_DUALFIT_GAPS")
        .ok()
        .or_else(|| std::env::var("SPLICE_EXP_GAPS").ok())
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect());

    let skips_only = match std::env::var("SPLICE_DUALFIT_ALL").as_deref() {
        Ok("1") | Ok("true") => false,
        _ => true,
    };

    let a_ch = probe_audio_channels(&a_path);
    let b_ch = probe_audio_channels(&b_path);
    let env_stream = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<usize>().ok());
    let a_stream = env_stream("SPLICE_DUALFIT_A_STREAM")
        .or_else(|| env_stream("SPLICE_EXP_A_STREAM"))
        .unwrap_or_else(|| a_ch.iter().enumerate().max_by_key(|(_, &c)| c).map(|(i, _)| i).unwrap_or(0));
    let target_ch = a_ch.get(a_stream).copied().unwrap_or(0);
    let b_stream = env_stream("SPLICE_DUALFIT_B_STREAM")
        .or_else(|| env_stream("SPLICE_EXP_B_STREAM"))
        .unwrap_or_else(|| b_ch.iter().position(|&c| c == target_ch).unwrap_or(0));

    println!("=== dual-fit gate simulation ===");
    println!(
        "corpus: {}\nA: {}\nB: {}\nsr: {rate}",
        corpus_path.display(),
        a_path.display(),
        b_path.display()
    );
    println!(
        "A audio streams (ch): {a_ch:?} → a:{a_stream} ({target_ch}ch)\n\
         B audio streams (ch): {b_ch:?} → a:{b_stream} ({}ch)",
        b_ch.get(b_stream).copied().unwrap_or(0)
    );
    println!(
        "gate: seam {:.0} ms  min_corr {:.2}  abs_floor {:.2}",
        seam_secs() * 1000.0,
        min_fill_correlation(),
        fill_absolute_floor()
    );

    let mut ran = 0usize;
    let mut pass = 0usize;
    let mut global_pass = 0usize;
    for g in &file.gaps {
        if let Some(ids) = &only {
            if !ids.contains(&g.index) {
                continue;
            }
        } else if g.baseline_lag.is_none() {
            continue;
        } else if skips_only && g.outcome.as_ref().is_some_and(|o| o.tier != "skip") {
            continue;
        }
        let Some(geo) = &g.geometry else { continue };
        let Some(sim) = run_gap(g, geo, &a_path, &b_path, a_stream, b_stream, rate) else {
            continue;
        };
        print_gap(g, geo, &sim);
        ran += 1;
        if sim.gate_pass {
            pass += 1;
        }
        if sim.global_pass {
            global_pass += 1;
        }
    }

    println!("\n{pass}/{ran} pass dual-fit (per-shoulder) gate · {global_pass}/{ran} pass GLOBAL single-shift.");
    println!(
        "  (global-pass ⇒ corpus step is spurious, a constant shift fixes it — registration miss, not a splice.\n   \
         dual-fit-pass but global-fail ⇒ the step is real and needs per-shoulder reconciliation.\n   \
         CAVEAT: both modes use the sim's own ffmpeg decode; a decode mismatch vs the fingerprint scan\n   \
         would bias these — treat as strong evidence, not proof, until cross-checked against the scan.)"
    );
    assert!(ran > 0, "no target gaps in {corpus}");
}
