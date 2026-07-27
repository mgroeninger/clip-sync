//! **Offline timescale / uniqueness / downmix experiment** for the seam-splice repair direction
//! (`docs/dev/archive/TEMP-seam-splice-dualfit-plan.md` §3.6). The assembly point where every candidate schema
//! metric is validated on the *source media* before it is frozen into the capture binary.
//!
//! Tier: **diagnostic** (`diagnostic-tests`) — and it shells out to **ffmpeg** (must be on PATH). It does
//! NOT run the repair pipeline: it reads a pair's `corpus.json` for geometry only, decodes the few
//! seconds of A/B around each chosen gap, reconstructs the seam windows, and sweeps:
//!   * **level** — straight mono vs per-channel vs energy-weighted downmix vs loudest channel (the
//!     cancellation-vs-quiet question, plan §3.3);
//!   * **uniqueness** — fine-waveform lag curve → top-K peaks, prominence (#1−#2), `peak_z`
//!     (= (peak−mean)/std), top-2 spacing — across window sizes (250 ms / 500 ms / 1 s / 2 s);
//!   * **wide-envelope segment uniqueness** — bucketed RMS envelope lag curve at a few bin sizes.
//!
//! **Spot-check `b_mapped` registration (ledger C2)** — the `[fine uniqueness]` block already anchors B at
//! `geometry.b_mapped_*` (same frame as capture after A2). Compare pre/post `peak@lag` and `peak_z` at
//! 1000 ms against `baseline_lag` / `splice` in a fresh fingerprint scan.
//!
//! Pair-6 one-sided-dead (done): `SPLICE_EXP_GAPS=2,6,7,9,10` on `gap-files/6/corpus.json`.
//! Pair-7 confirm (ledger C2): gaps that were dead at F1 throat — typically `SPLICE_EXP_GAPS=3,4`:
//! ```powershell
//! $env:SPLICE_EXP_CORPUS = "gap-files/7/corpus.json"
//! $env:SPLICE_EXP_A = "path\to\pair7-A.mkv"
//! $env:SPLICE_EXP_B = "path\to\pair7-B.m4v"
//! $env:SPLICE_EXP_GAPS = "3,4"
//! cargo test -p clip-sync-repair --features diagnostic-tests --test diag_splice_timescale -- --nocapture
//! ```
//! Pass criteria: both shoulders `peak_r` ≥ 0.9 at 1000 ms with a stable `peak@lag` (not pinned at ±200 ms
//! edge). Optional: widen `SPLICE_EXP_FINE_LAG_MS` if a shoulder still pins.
//!
//! Nothing here writes the schema — it prints a report so the *winning* timescales/representations are
//! chosen from data. Run (paths are yours; `blah.*` are placeholders):
//! ```powershell
//! $env:SPLICE_EXP_CORPUS = "gap-files/1/corpus.json"
//! $env:SPLICE_EXP_A = "F:\Video\A.mkv"
//! $env:SPLICE_EXP_B = "F:\Video\B.m4v"
//! # optional: $env:SPLICE_EXP_GAPS = "3,19,22"   (default: gaps that carry baseline_lag)
//! # optional: $env:SPLICE_EXP_SR = "48000"
//! cargo test -p clip-sync-repair --features diagnostic-tests --test diag_splice_timescale -- --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use clip_sync::normalized_correlation;
use serde::Deserialize;

// ── geometry projection from corpus.json ─────────────────────────────────────────

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
    /// Present only on gaps that found a matchable B placement — our experiment targets these.
    #[serde(default)]
    baseline_lag: Option<serde_json::Value>,
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

// ── tunables ─────────────────────────────────────────────────────────────────────

/// Window sizes swept for level + fine-uniqueness (the timescale question).
const WINDOW_MS: [f64; 4] = [250.0, 500.0, 1000.0, 2000.0];
/// Bin sizes swept for the wide bucketed-envelope segment curve.
const ENV_BIN_MS: [f64; 3] = [20.0, 50.0, 100.0];
/// ± lag search for the fine waveform curve (matches `baseline_lag`).
/// ± lag search for the fine waveform curve (matches `baseline_lag` at 200 ms). Override with
/// `SPLICE_EXP_FINE_LAG_MS` to probe large offsets — e.g. a post lag pinned at the ±200 ms edge (6·g6):
/// re-run with `SPLICE_EXP_FINE_LAG_MS=600` to see whether it peaks beyond 200 ms (clipped offset) or
/// stays low everywhere (truly decorrelated).
fn fine_max_lag_ms() -> f64 {
    std::env::var("SPLICE_EXP_FINE_LAG_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200.0)
}
/// ± lag search (wider) for the wide-envelope segment curve.
const WIDE_MAX_LAG_MS: f64 = 400.0;
/// Top peaks reported from each curve.
const TOP_K: usize = 5;

/// **Outward-anchor search** — for a quiet shoulder that can't establish uniqueness at the gap edge, scan
/// outward (away from the gap) to the loudest window within `ANCHOR_MAX_OUT_MS` and lag-search *there*
/// instead. A distant loud feature registers with high uniqueness; the same-master rigid content carries
/// that lag back to the quiet seam. Window `ANCHOR_WIN_MS`, stepped by `ANCHOR_STEP_MS`.
const ANCHOR_WIN_MS: f64 = 500.0;
const ANCHOR_STEP_MS: f64 = 50.0;
/// How far outward the anchor search reaches; override with `SPLICE_EXP_ANCHOR_OUT_MS` for long quiet
/// pockets (7·g3's quiet section runs > 2 s, so the loudest feature is further out). The decode pad scales.
fn anchor_max_out_ms() -> f64 {
    std::env::var("SPLICE_EXP_ANCHOR_OUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000.0)
}

fn sr() -> u32 {
    std::env::var("SPLICE_EXP_SR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(48_000)
}

/// Resolve a path: absolute as-is, relative against the **repo root** (cargo runs tests from the crate
/// dir, so a relative `gap-files/1/corpus.json` would otherwise miss).
fn resolve(p: &str) -> PathBuf {
    let p = Path::new(p);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(p)
    }
}

// ── ffmpeg decode of a single [start,end] span → interleaved f32 + channel count ──

struct Pcm {
    samples: Vec<f32>,
    channels: usize,
}

impl Pcm {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }
    /// One channel's samples, de-interleaved.
    fn channel(&self, ch: usize) -> Vec<f64> {
        self.samples
            .chunks(self.channels.max(1))
            .map(|f| *f.get(ch).unwrap_or(&0.0) as f64)
            .collect()
    }
    /// Straight unweighted mono downmix (sum/N) — today's `interleaved_to_mono`.
    fn mono(&self) -> Vec<f64> {
        let n = self.channels.max(1);
        self.samples
            .chunks(n)
            .map(|f| f.iter().map(|&s| s as f64).sum::<f64>() / n as f64)
            .collect()
    }
    /// Energy-weighted downmix: weight each channel by its RMS so a dominant channel isn't divided by N.
    fn weighted_mono(&self) -> Vec<f64> {
        let n = self.channels.max(1);
        let rms: Vec<f64> = (0..n).map(|c| rms(&self.channel(c))).collect();
        let total: f64 = rms.iter().sum();
        if total <= f64::EPSILON {
            return self.mono();
        }
        let w: Vec<f64> = rms.iter().map(|r| r / total).collect();
        self.samples
            .chunks(n)
            .map(|f| {
                f.iter()
                    .enumerate()
                    .map(|(c, &s)| s as f64 * w.get(c).copied().unwrap_or(0.0))
                    .sum()
            })
            .collect()
    }
}

/// Channels per audio stream, in audio-stream order (so the position = the `a:N` index). Empty on
/// failure. Used to match the pipeline's track selection (`select_track_for_reference`: same channel
/// count as A — e.g. B's 5.1, not its years-old stereo downmix at `a:0`). Reads JSON and falls back to
/// `channel_layout` because some demuxers (e.g. AC-3 in MP4) leave the numeric `channels` field empty.
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

/// Decode `[start_secs, start_secs+dur_secs]` of audio stream `a:{stream}` of `input` to interleaved f32
/// at `sample_rate`, native channel layout. Fast-seeks (`-ss` before `-i`). `None` on any failure.
fn decode_span(
    input: &Path,
    stream: usize,
    start_secs: f64,
    dur_secs: f64,
    sample_rate: u32,
) -> Option<Pcm> {
    let tmp = std::env::temp_dir().join(format!("splice_exp_{}.wav", std::process::id()));
    let status = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error"])
        .args(["-ss", &format!("{:.6}", start_secs.max(0.0))])
        .args(["-t", &format!("{:.6}", dur_secs.max(0.0))])
        .arg("-i")
        .arg(input)
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
        eprintln!(
            "ffmpeg decode failed: {} @ {start_secs:.2}s+{dur_secs:.2}s",
            input.display()
        );
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

// ── metric helpers (the candidates under test) ───────────────────────────────────

fn rms(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * (x.max(1e-12)).log10()
}

/// `~bin`-sample RMS envelope.
fn rms_envelope(x: &[f64], bin: usize) -> Vec<f64> {
    let bin = bin.max(1);
    x.chunks(bin).map(|c| rms(c)).collect()
}

/// Cross-correlation curve of `a` against `b_ctx` over integer lags `-max_lag..=max_lag`, where
/// `b_ctx[max_lag + lag .. + a.len()]` is the shifted comparison window (so `b_ctx` must span
/// `a.len() + 2*max_lag`). Returns `(lag_samples, r)`.
fn lag_curve(a: &[f64], b_ctx: &[f64], max_lag: i64) -> Vec<(i64, f64)> {
    let n = a.len();
    if n == 0 || max_lag < 0 {
        return Vec::new();
    }
    (-max_lag..=max_lag)
        .filter_map(|lag| {
            let base = (max_lag + lag) as usize;
            if base + n > b_ctx.len() {
                return None;
            }
            Some((lag, normalized_correlation(a, &b_ctx[base..base + n])))
        })
        .collect()
}

struct Peak {
    lag_ms: f64,
    r: f64,
}

/// Top-K local maxima of a curve, tallest first.
fn top_peaks(curve: &[(i64, f64)], sample_rate: u32, k: usize) -> Vec<Peak> {
    let rate = f64::from(sample_rate).max(1.0);
    let mut maxima: Vec<(i64, f64)> = Vec::new();
    for i in 1..curve.len().saturating_sub(1) {
        let r = curve[i].1;
        if r >= curve[i - 1].1 && r >= curve[i + 1].1 {
            maxima.push(curve[i]);
        }
    }
    maxima.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    maxima
        .into_iter()
        .take(k)
        .map(|(lag, r)| Peak {
            lag_ms: lag as f64 * 1000.0 / rate,
            r,
        })
        .collect()
}

/// `(prominence, peak_z, top2_spacing_ms)`: prominence = r1−r2; peak_z = (r1−mean)/std over the curve;
/// spacing = |lag1−lag2| ms. The robust-uniqueness candidates, all from one curve.
fn uniqueness_stats(curve: &[(i64, f64)], peaks: &[Peak]) -> (f64, f64, f64) {
    let rs: Vec<f64> = curve.iter().map(|&(_, r)| r).collect();
    let mean = rs.iter().sum::<f64>() / rs.len().max(1) as f64;
    let var = rs.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rs.len().max(1) as f64;
    let std = var.sqrt().max(1e-9);
    let r1 = peaks.first().map(|p| p.r).unwrap_or(f64::NAN);
    let r2 = peaks.get(1).map(|p| p.r).unwrap_or(0.0);
    let prominence = r1 - r2;
    let peak_z = (r1 - mean) / std;
    let spacing = match (peaks.first(), peaks.get(1)) {
        (Some(a), Some(b)) => (a.lag_ms - b.lag_ms).abs(),
        _ => f64::NAN,
    };
    (prominence, peak_z, spacing)
}

// ── window reconstruction ────────────────────────────────────────────────────────

/// A pre-border window of `w` samples ending at frame `end`, and the matching B context spanning
/// `±max_lag` around B frame `b_anchor` (B's position aligned to the gap edge). `None` if out of range.
fn seam_windows(
    a: &[f64],
    b: &[f64],
    a_edge: usize,
    b_anchor: usize,
    w: usize,
    max_lag: usize,
    pre: bool,
) -> Option<(Vec<f64>, Vec<f64>)> {
    if pre {
        if a_edge < w || b_anchor < w + max_lag || b_anchor + max_lag > b.len() {
            return None;
        }
        let a_win = a[a_edge - w..a_edge].to_vec();
        let b_ctx = b[b_anchor - w - max_lag..b_anchor + max_lag].to_vec();
        Some((a_win, b_ctx))
    } else {
        if a_edge + w > a.len() || b_anchor < max_lag || b_anchor + w + max_lag > b.len() {
            return None;
        }
        let a_win = a[a_edge..a_edge + w].to_vec();
        let b_ctx = b[b_anchor - max_lag..b_anchor + w + max_lag].to_vec();
        Some((a_win, b_ctx))
    }
}

/// Lag-search at one placement: `(rms_db, peak_r, peak_lag_ms, peak_z)`.
fn lag_probe(
    a: &[f64],
    b: &[f64],
    a_edge: usize,
    b_anchor: usize,
    w: usize,
    max_lag: usize,
    pre: bool,
    rate: f64,
) -> Option<(f64, f64, f64, f64)> {
    let (a_win, b_ctx) = seam_windows(a, b, a_edge, b_anchor, w, max_lag, pre)?;
    let curve = lag_curve(&a_win, &b_ctx, max_lag as i64);
    if curve.is_empty() {
        return None;
    }
    let peaks = top_peaks(&curve, rate as u32, TOP_K);
    let (_, z, _) = uniqueness_stats(&curve, &peaks);
    let p = peaks.first()?;
    Some((db(rms(&a_win)), p.r, p.lag_ms, z))
}

/// **Outward-anchor**: scan a shoulder outward (away from the gap) to the loudest `ANCHOR_WIN_MS` window
/// within `ANCHOR_MAX_OUT_MS`, then lag-search there. Returns `(offset_ms, rms_db, peak_r, peak_lag_ms,
/// peak_z)` — the distant loud feature's registration, which should be far more unique than the quiet edge.
fn outward_anchor(
    a: &[f64],
    b: &[f64],
    a_edge: usize,
    b_anchor: usize,
    w: usize,
    max_lag: usize,
    pre: bool,
    rate: f64,
) -> Option<(f64, f64, f64, f64, f64)> {
    let step = ((ANCHOR_STEP_MS / 1000.0 * rate).round() as usize).max(1);
    let max_out = (anchor_max_out_ms() / 1000.0 * rate).round() as usize;
    let mut best: Option<(usize, f64)> = None; // (offset frames, rms) of the loudest reachable window
    let mut off = 0usize;
    while off <= max_out {
        let (ae, ba) = if pre {
            (a_edge.checked_sub(off), b_anchor.checked_sub(off))
        } else {
            (Some(a_edge + off), Some(b_anchor + off))
        };
        if let (Some(ae), Some(ba)) = (ae, ba) {
            if let Some((a_win, _)) = seam_windows(a, b, ae, ba, w, max_lag, pre) {
                let r = rms(&a_win);
                if best.is_none_or(|(_, br)| r > br) {
                    best = Some((off, r));
                }
            }
        }
        off += step;
    }
    let (off, _) = best?;
    let (ae, ba) = if pre {
        (a_edge - off, b_anchor - off)
    } else {
        (a_edge + off, b_anchor + off)
    };
    let (rms_db, peak_r, peak_lag_ms, peak_z) = lag_probe(a, b, ae, ba, w, max_lag, pre, rate)?;
    Some((
        off as f64 * 1000.0 / rate,
        rms_db,
        peak_r,
        peak_lag_ms,
        peak_z,
    ))
}

// ── report ───────────────────────────────────────────────────────────────────────

const A_PAD_SECS: f64 = 2.3; // ≥ max window
const LAG_PAD_SECS: f64 = 0.5; // ≥ WIDE_MAX_LAG_MS

fn run_gap(
    geo: &Geometry,
    idx: usize,
    a_path: &Path,
    b_path: &Path,
    a_stream: usize,
    b_stream: usize,
    rate: u32,
) {
    let r = f64::from(rate);
    let dur = geo
        .duration_secs
        .unwrap_or(geo.a_refined_end_secs - geo.a_refined_start_secs);
    println!(
        "\n──────── gap {idx}  (A {:.3}..{:.3}s, dur {dur:.3}s → B {:.3}..{:.3}s) ────────",
        geo.a_refined_start_secs,
        geo.a_refined_end_secs,
        geo.b_mapped_start_secs,
        geo.b_mapped_end_secs
    );

    // Decode the few seconds of A and B that span the gap ± windows/lag. The pad must cover the widest
    // window, the (possibly widened) fine lag, the wide-envelope lag, AND the outward-anchor reach.
    let lag_pad = LAG_PAD_SECS
        .max(fine_max_lag_ms() / 1000.0 + 0.1)
        .max(WIDE_MAX_LAG_MS / 1000.0 + 0.1);
    let a_pad = A_PAD_SECS
        .max((anchor_max_out_ms() + ANCHOR_WIN_MS) / 1000.0 + fine_max_lag_ms() / 1000.0 + 0.1);
    let a_start = geo.a_refined_start_secs - a_pad;
    let a_span = (geo.a_refined_end_secs + a_pad) - a_start;
    let b_start = geo.b_mapped_start_secs - a_pad - lag_pad;
    let b_span = (geo.b_mapped_end_secs + a_pad + lag_pad) - b_start;
    let (Some(a_pcm), Some(b_pcm)) = (
        decode_span(a_path, a_stream, a_start, a_span, rate),
        decode_span(b_path, b_stream, b_start, b_span, rate),
    ) else {
        println!("  (decode failed — skipping)");
        return;
    };
    println!(
        "  A {} ch, {} frames | B {} ch, {} frames",
        a_pcm.channels,
        a_pcm.frames(),
        b_pcm.channels,
        b_pcm.frames()
    );

    // Edge frames within the decoded spans.
    let a_pre_edge = ((geo.a_refined_start_secs - a_start) * r).round() as usize;
    let a_post_edge = ((geo.a_refined_end_secs - a_start) * r).round() as usize;
    let max_lag = (fine_max_lag_ms() / 1000.0 * r).round() as usize;

    // (1) LEVEL — straight mono vs per-channel vs weighted vs loudest, over the pre border (each window).
    println!("  [level] pre-border RMS dBFS by representation × window:");
    println!("    win_ms |   mono | weighted | loudest |  per-channel");
    for &wm in &WINDOW_MS {
        let w = (wm / 1000.0 * r).round() as usize;
        if a_pre_edge < w {
            continue;
        }
        let lo = a_pre_edge - w;
        let mono = db(rms(&a_pcm.mono()[lo..a_pre_edge]));
        let weighted = db(rms(&a_pcm.weighted_mono()[lo..a_pre_edge]));
        let per: Vec<f64> = (0..a_pcm.channels)
            .map(|c| db(rms(&a_pcm.channel(c)[lo..a_pre_edge])))
            .collect();
        let loudest = per.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let per_str: Vec<String> = per.iter().map(|d| format!("{d:6.1}")).collect();
        println!(
            "    {wm:6.0} | {mono:6.1} | {weighted:8.1} | {loudest:7.1} | {}",
            per_str.join(" ")
        );
    }

    // (2) UNIQUENESS — fine waveform lag curve on mono, per window size, pre + post seams.
    let mono_a = a_pcm.mono();
    let mono_b = b_pcm.mono();
    let b_pre_anchor = ((geo.b_mapped_start_secs - b_start) * r).round() as usize;
    let b_post_anchor = ((geo.b_mapped_end_secs - b_start) * r).round() as usize;
    println!(
        "  [fine uniqueness] mono waveform, ±{:.0}ms:  peak@lag | prom | peak_z | top2_gap_ms",
        fine_max_lag_ms()
    );
    for (label, a_edge, b_anchor, pre) in [
        ("pre", a_pre_edge, b_pre_anchor, true),
        ("post", a_post_edge, b_post_anchor, false),
    ] {
        for &wm in &WINDOW_MS {
            let w = (wm / 1000.0 * r).round() as usize;
            let Some((a_win, b_ctx)) =
                seam_windows(&mono_a, &mono_b, a_edge, b_anchor, w, max_lag, pre)
            else {
                continue;
            };
            let curve = lag_curve(&a_win, &b_ctx, max_lag as i64);
            if curve.is_empty() {
                continue;
            }
            let peaks = top_peaks(&curve, rate, TOP_K);
            let (prom, z, gap) = uniqueness_stats(&curve, &peaks);
            let p0 = peaks.first();
            println!(
                "    {label:<4} {wm:6.0}ms | {:.3}@{:>7.1} | {prom:.3} | {z:6.2} | {gap:7.1}",
                p0.map(|p| p.r).unwrap_or(f64::NAN),
                p0.map(|p| p.lag_ms).unwrap_or(f64::NAN),
            );
        }
    }

    // (2b) DOWNMIX representation for CORRELATION at the decisive 1 s window: does emphasizing the loud
    // center (weighted / loudest channel) beat the straight mono A↓6 that dilutes it with quiet surrounds
    // B's stereo lacks? Each A representation is correlated against B-mono.
    let rep_w = r.round() as usize; // 1 s
    let loudest_ch = (0..a_pcm.channels)
        .filter(|_| a_pre_edge >= rep_w)
        .max_by(|&c1, &c2| {
            let e = |c: usize| rms(&a_pcm.channel(c)[a_pre_edge - rep_w..a_pre_edge]);
            e(c1)
                .partial_cmp(&e(c2))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);
    let reps: [(String, Vec<f64>); 3] = [
        ("mono".into(), mono_a.clone()),
        ("weighted".into(), a_pcm.weighted_mono()),
        (format!("loud-ch{loudest_ch}"), a_pcm.channel(loudest_ch)),
    ];
    let peak_prom =
        |sig: &[f64], a_edge: usize, b_anchor: usize, pre: bool| -> Option<(f64, f64)> {
            let (a_win, b_ctx) = seam_windows(sig, &mono_b, a_edge, b_anchor, rep_w, max_lag, pre)?;
            let curve = lag_curve(&a_win, &b_ctx, max_lag as i64);
            if curve.is_empty() {
                return None;
            }
            let pk = top_peaks(&curve, rate, TOP_K);
            let (prom, _, _) = uniqueness_stats(&curve, &pk);
            Some((pk.first().map(|p| p.r).unwrap_or(f64::NAN), prom))
        };
    println!("  [repr @1s] A-downmix vs B-mono:  repr     | pre peak/prom | post peak/prom");
    let fmt = |o: Option<(f64, f64)>| {
        o.map(|(r, p)| format!("{r:.3}/{p:.3}"))
            .unwrap_or_else(|| "   -   ".into())
    };
    for (label, sig) in &reps {
        let pre = peak_prom(sig, a_pre_edge, b_pre_anchor, true);
        let post = peak_prom(sig, a_post_edge, b_post_anchor, false);
        println!("    {label:<8} | {:>11} | {:>11}", fmt(pre), fmt(post));
    }

    // (2d) OUTWARD-ANCHOR — for a quiet shoulder, align on the nearest LOUD feature instead of the gap edge.
    // If the quiet edge has thin uniqueness but a distant loud window locks in sharply (high peak_z) at a
    // consistent lag, that lag registers the quiet seam — the fix for flat-envelope placement wander.
    let anch_w = (ANCHOR_WIN_MS / 1000.0 * r).round() as usize;
    println!(
        "  [outward-anchor] loudest {:.0}ms window within ±{:.0}ms of the shoulder — edge(off 0) vs anchor:",
        ANCHOR_WIN_MS, anchor_max_out_ms()
    );
    println!("    side | edge:  rms   peak@lag   z | anchor: off     rms   peak@lag   z");
    for (label, a_edge, b_anchor, pre) in [
        ("pre", a_pre_edge, b_pre_anchor, true),
        ("post", a_post_edge, b_post_anchor, false),
    ] {
        let edge = lag_probe(&mono_a, &mono_b, a_edge, b_anchor, anch_w, max_lag, pre, r);
        let anc = outward_anchor(&mono_a, &mono_b, a_edge, b_anchor, anch_w, max_lag, pre, r);
        let fe = edge
            .map(|(rms, pr, lag, z)| format!("{rms:6.1} {pr:.3}@{lag:+7.1} z{z:5.1}"))
            .unwrap_or_else(|| "        -        ".into());
        let fa = anc
            .map(|(off, rms, pr, lag, z)| {
                format!("{off:+5.0}ms {rms:6.1} {pr:.3}@{lag:+7.1} z{z:5.1}")
            })
            .unwrap_or_else(|| "           -           ".into());
        println!("    {label:<4} | {fe} | {fa}");
    }

    // (3) WIDE-ENVELOPE segment uniqueness — bucketed RMS env, widest window, per bin size.
    println!("  [wide-env segment] 2 s window, ±{WIDE_MAX_LAG_MS:.0}ms:  bin_ms | peak@lag | prom | peak_z");
    let wide_w = (2.0 * r).round() as usize;
    let wide_lag = (WIDE_MAX_LAG_MS / 1000.0 * r).round() as usize;
    if let Some((a_win, b_ctx)) = seam_windows(
        &mono_a,
        &mono_b,
        a_pre_edge,
        b_pre_anchor,
        wide_w,
        wide_lag,
        true,
    ) {
        for &bm in &ENV_BIN_MS {
            let bin = (bm / 1000.0 * r).round() as usize;
            let ea = rms_envelope(&a_win, bin);
            let eb = rms_envelope(&b_ctx, bin);
            let env_lag = (wide_lag / bin.max(1)) as i64;
            let curve = lag_curve(&ea, &eb, env_lag);
            if curve.is_empty() {
                continue;
            }
            // `top_peaks(.., 1, ..)` returns lag in *bins×1000*; real ms = lag_bins · bin_samples / sr · 1000
            // = (returned / 1000) · bin / sr · 1000 = returned · bin / sr.
            let peaks: Vec<Peak> = top_peaks(&curve, 1, TOP_K)
                .into_iter()
                .map(|p| Peak {
                    lag_ms: p.lag_ms * bin as f64 / r,
                    r: p.r,
                })
                .collect();
            let (prom, z, _) = uniqueness_stats(&curve, &peaks);
            let p0 = peaks.first();
            println!(
                "    {bm:6.0} | {:.3}@{:>7.1} | {prom:.3} | {z:6.2}",
                p0.map(|p| p.r).unwrap_or(f64::NAN),
                p0.map(|p| p.lag_ms).unwrap_or(f64::NAN),
            );
        }
    }
}

#[test]
fn diag_splice_timescale() {
    let (Ok(corpus), Ok(a), Ok(b)) = (
        std::env::var("SPLICE_EXP_CORPUS"),
        std::env::var("SPLICE_EXP_A"),
        std::env::var("SPLICE_EXP_B"),
    ) else {
        eprintln!(
            "skip: set SPLICE_EXP_CORPUS (a pair's corpus.json), SPLICE_EXP_A, SPLICE_EXP_B (media).\n\
             optional SPLICE_EXP_GAPS=3,19,22  SPLICE_EXP_SR=48000"
        );
        return;
    };
    let (a_path, b_path) = (resolve(&a), resolve(&b));
    let corpus_path = resolve(&corpus);
    let rate = sr();

    let text = std::fs::read_to_string(&corpus_path)
        .unwrap_or_else(|e| panic!("read SPLICE_EXP_CORPUS {}: {e}", corpus_path.display()));
    let file: CorpusFile = serde_json::from_str(&text).expect("parse corpus.json");

    let only: Option<Vec<usize>> = std::env::var("SPLICE_EXP_GAPS")
        .ok()
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect());

    // Match the pipeline's track selection: A = its highest-channel audio stream (the 5.1), B = the audio
    // stream with the same channel count (B's 5.1, NOT its years-old stereo downmix at a:0). Overridable.
    let a_ch = probe_audio_channels(&a_path);
    let b_ch = probe_audio_channels(&b_path);
    let env_stream = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<usize>().ok());
    let a_stream = env_stream("SPLICE_EXP_A_STREAM").unwrap_or_else(|| {
        a_ch.iter()
            .enumerate()
            .max_by_key(|(_, &c)| c)
            .map(|(i, _)| i)
            .unwrap_or(0)
    });
    let target_ch = a_ch.get(a_stream).copied().unwrap_or(0);
    let b_stream = env_stream("SPLICE_EXP_B_STREAM")
        .unwrap_or_else(|| b_ch.iter().position(|&c| c == target_ch).unwrap_or(0));

    println!("=== splice timescale experiment ===");
    println!(
        "A: {}\nB: {}\nsr: {rate}",
        a_path.display(),
        b_path.display()
    );
    println!(
        "A audio streams (ch): {a_ch:?} → using a:{a_stream} ({target_ch}ch)\n\
         B audio streams (ch): {b_ch:?} → using a:{b_stream} ({}ch){}",
        b_ch.get(b_stream).copied().unwrap_or(0),
        if b_ch.get(b_stream).copied() == Some(target_ch) {
            " [channel-matched]"
        } else {
            " [!! NO channel match — set SPLICE_EXP_B_STREAM]"
        }
    );
    let mut ran = 0;
    for g in &file.gaps {
        if let Some(ids) = &only {
            if !ids.contains(&g.index) {
                continue;
            }
        } else if g.baseline_lag.is_none() {
            continue; // default: only gaps with a matchable B placement
        }
        let Some(geo) = &g.geometry else { continue };
        run_gap(geo, g.index, &a_path, &b_path, a_stream, b_stream, rate);
        ran += 1;
    }
    println!("\n{ran} gap(s) processed.");
    assert!(ran > 0, "no target gaps found in {corpus}");
}
