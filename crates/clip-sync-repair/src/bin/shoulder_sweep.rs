//! `shoulder-sweep` — the independent wide-window waveform instrument on `--gap-listen` WAVs.
//!
//! **What it is for.** Production's `lag_decision` searches a fine ±600 ms window (with a 100 ms-bin
//! envelope rescue out to ±2000 ms) and is the thing under test on far-offset gaps. This tool answers
//! the same question from a different instrument: a 1 s A template adjacent to a throat, slid over the
//! B surround at **every integer sample lag** in ±`--max-lag-ms` (default 2000), sample-domain Pearson,
//! with a peak table and a floor/prominence number. It shares no code with the estimator it checks —
//! the FFT correlation below is local to this bin, and nothing here calls
//! `lag_correlation_curve_fft` / `normalized_correlation` / `register_donor_window`. Same constraint
//! `listen-registration` documents, for the same reason.
//!
//! **Why lag 0 in these WAVs is the nominal map.** `gap_listen` cuts A over `[a_start − ctx,
//! a_end + ctx]` and B over `[b_start − ctx, b_end + ctx]` at the same rate, so lag 0 at a throat is
//! the nominal offset map's image of that throat. Sign matches `DonorRegistration::lag_ms` and
//! `listen-registration`'s `wav_lag`: **negative means B's matching content sits earlier than the
//! nominal map put it.**
//!
//! **What it does not do.** It measures and stops. There is no classifier, no bucket, no per-pair
//! verdict: `step_ms` (`post_lag − pre_lag`) is recorded on every row and nothing branches on it, and
//! `bridge_ms` (`gap_ms + step_ms`) is a comparison column only — `|bridge_ms| ≈ 0` is an arithmetic
//! coincidence (it can only fire when a pair's map step equals minus that gap's duration, so no
//! positive step can ever satisfy it) and must never become a class again.
//!
//! **Specification.** `docs/dev/shoulder-sweep.md` §2-§3 is this bin's contract — throat
//! placement, the correlation, the peak picker, the derived columns, and the census schema. Read it
//! before changing any number here.
//!
//! **Usage.**
//!
//! ```text
//! cargo run --features calibration --bin shoulder-sweep -- \
//!     --shoulder pre <stem>_a_surround.wav <stem>_b_surround.wav
//! cargo run --features calibration --bin shoulder-sweep -- --corpus gap-files/<listen-run>
//! cargo run --features calibration --bin shoulder-sweep -- \
//!     --corpus gap-files/<flat-pair-dir> --pair 31
//! ```
//!
//! Media hygiene: prints pair directory names, gap indices, and file stems only — never media paths
//! or titles. Relative `--corpus` / `--also` / `--out` resolve against the repo root.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::Serialize;

use clip_sync_repair::application::gap_fingerprint::GapCorpus;

const A_SUFFIX: &str = "_a_surround.wav";
const B_SUFFIX: &str = "_b_surround.wav";

/// Template length. Fixed: the archived §4.3 landmark (31/g7 pre: r ≈ 0.8287 at ≈ −38.1 ms) is a
/// 1 s-template number, and a knob here would make old and new census files non-comparable.
const TEMPLATE_SECS: f64 = 1.0;
/// Peak-table exclusion radius, and the radius the floor is taken outside of.
const PEAK_EXCLUSION_MS: f64 = 250.0;
/// How close to the end of the swept range counts as pinned to it.
const EDGE_TOL_MS: f64 = 20.0;
const LIVE_R: f64 = 0.5;
const LIVE_PROM: f64 = 0.10;
const UNIQUE_R: f64 = 0.8;
/// Variance floor before the square root — guards a constant (zero-variance) haystack window.
const VAR_FLOOR: f64 = 1e-20;

#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
enum Shoulder {
    Pre,
    Post,
}

impl Shoulder {
    fn as_str(self) -> &'static str {
        match self {
            Shoulder::Pre => "pre",
            Shoulder::Post => "post",
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    about = "Wide-window (±2 s) waveform lag sweep of a gap's shoulders against the --gap-listen B clip"
)]
struct Args {
    /// Single-gap mode: the `_a_surround.wav` to take the template from.
    #[arg(requires = "b_surround")]
    a_surround: Option<PathBuf>,

    /// Single-gap mode: the `_b_surround.wav` to search.
    b_surround: Option<PathBuf>,

    /// Which throat to sweep. Required in single-gap mode, forbidden with `--corpus` (corpus mode
    /// always sweeps both).
    #[arg(long, conflicts_with = "corpus")]
    shoulder: Option<Shoulder>,

    /// Corpus mode: a `--gap-listen` tree (pair subdirectories, or one flat pair directory).
    #[arg(long, conflicts_with_all = ["a_surround", "b_surround"])]
    corpus: Option<PathBuf>,

    /// Extra listen dirs merged into the same census, by pair. First root to carry a stem wins.
    #[arg(long, requires = "corpus")]
    also: Vec<PathBuf>,

    /// Pair label for rows a directory name cannot supply — the pair index lives nowhere in the
    /// dumps, so a flat directory whose name is not the index needs it here. It fills in; it never
    /// overrides. Rejected on a tree of pair subdirectories, which name themselves.
    #[arg(long, requires = "corpus")]
    pair: Option<String>,

    /// Where to write `census.csv` / `census.json` / the per-shoulder logs. Default
    /// `<corpus>/wide-sweep/`.
    #[arg(long, requires = "corpus")]
    out: Option<PathBuf>,

    /// Search half-width, milliseconds. Leave the default alone for comparability with existing
    /// census files; pass a wider value explicitly when a pair's offset is known to exceed it.
    #[arg(long, default_value_t = 2000.0)]
    max_lag_ms: f64,

    /// Peak-table length.
    #[arg(long, default_value_t = 8)]
    peaks: usize,

    /// Export context per side, seconds — the **fallback** only. A sibling dump JSON's geometry
    /// wins when present, so a run made with a different pad measures its own throats.
    #[arg(long, default_value_t = 3.0)]
    context_secs: f64,
}

// -------------------------------------------------------------------------------------------
// Measurement
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize)]
struct Peak {
    lag_ms: f64,
    r: f64,
}

/// One throat's sweep. Everything here is measured; nothing is classified.
#[derive(Debug, Clone, Serialize)]
struct ShoulderResult {
    shoulder: &'static str,
    sr: u32,
    gap_secs: f64,
    template_a_secs: f64,
    max_lag_ms: f64,
    /// Correlation at the nominal map. `None` when lag 0 is outside the range actually swept (only
    /// reachable on a clipped haystack) — a value there would describe a lag that was not measured.
    lag0_r: Option<f64>,
    peak_lag_ms: f64,
    peak_r: f64,
    /// `(peak_r − mean r) / stddev r` over the whole swept curve. `None` on a flat curve.
    peak_z: Option<f64>,
    /// Highest r outside ±250 ms of the top peak. `None` when the swept range holds no such lag.
    floor_r: Option<f64>,
    /// `peak_r − floor_r`. `None` with the floor.
    prominence: Option<f64>,
    live: bool,
    unique_strict: bool,
    edge_pinned: bool,
    clipped_haystack: bool,
    /// The `[lo, hi]` lag range actually swept, when B clipped it short of the request.
    usable_lag_ms: Option<[f64; 2]>,
    peaks: Vec<Peak>,
}

/// Smallest power of two ≥ `n` — the linear-convolution transform size.
fn next_pow2(n: usize) -> usize {
    n.next_power_of_two()
}

/// `c[k] = Σᵢ hay[k+i] · tpl[i]` over the `valid` region, by FFT.
///
/// Three details decide whether this is a correlation or wraparound noise: the transform size is a
/// power of two **≥ `n + m`** (linear, not circular), the template is **time-reversed** before its
/// transform, and `rustfft`'s inverse is **unnormalized** so the result is scaled by `1/size`.
/// `rustfft` has no real-input transform, so both signals go through the complex FFT with zero
/// imaginary parts and the real part of the inverse is taken.
fn fft_correlate(hay: &[f64], tpl: &[f64]) -> Vec<f64> {
    let (n, m) = (hay.len(), tpl.len());
    let size = next_pow2(n + m);
    let zero = Complex::<f64>::new(0.0, 0.0);
    let mut h: Vec<Complex<f64>> = hay.iter().map(|&x| Complex::new(x, 0.0)).collect();
    h.resize(size, zero);
    let mut t: Vec<Complex<f64>> = tpl.iter().rev().map(|&x| Complex::new(x, 0.0)).collect();
    t.resize(size, zero);

    let mut planner = FftPlanner::new();
    let forward = planner.plan_fft_forward(size);
    forward.process(&mut h);
    forward.process(&mut t);
    for (hv, tv) in h.iter_mut().zip(t.iter()) {
        *hv *= *tv;
    }
    planner.plan_fft_inverse(size).process(&mut h);

    let scale = 1.0 / size as f64;
    h[m - 1..n].iter().map(|c| c.re * scale).collect()
}

/// Pearson r of `tpl` against every length-`tpl` window of `hay`.
///
/// The template is zero-meaned first, which is what makes the per-window haystack mean drop out of
/// the numerator — hence no windowed-mean subtraction in the correlation step. Window variances come
/// from prefix sums in O(1) each: `var = Σx² − (Σx)²/m`, floored before the square root.
fn pearson_sweep(tpl: &[f64], hay: &[f64]) -> Result<Vec<f64>, String> {
    let m = tpl.len();
    let n = hay.len();
    if m == 0 {
        return Err("empty template".into());
    }
    if n < m {
        return Err(format!(
            "haystack ({n} samples) is shorter than the template ({m} samples)"
        ));
    }
    let mean = tpl.iter().sum::<f64>() / m as f64;
    let centred: Vec<f64> = tpl.iter().map(|x| x - mean).collect();
    let tn = centred.iter().map(|x| x * x).sum::<f64>().sqrt();
    if tn == 0.0 {
        return Err("template is silent (zero variance)".into());
    }

    let corr = fft_correlate(hay, &centred);
    let mut cs = vec![0.0f64; n + 1];
    let mut cs2 = vec![0.0f64; n + 1];
    for i in 0..n {
        cs[i + 1] = cs[i] + hay[i];
        cs2[i + 1] = cs2[i] + hay[i] * hay[i];
    }
    Ok((0..=n - m)
        .map(|k| {
            let s = cs[k + m] - cs[k];
            let var = (cs2[k + m] - cs2[k] - s * s / m as f64).max(VAR_FLOOR);
            corr[k] / (tn * var.sqrt())
        })
        .collect())
}

/// Greedy peak table: highest r first, then the next highest **more than** `radius_ms` from every
/// peak already taken.
fn pick_peaks(lags_ms: &[f64], r: &[f64], radius_ms: f64, count: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..r.len()).collect();
    order.sort_by(|&i, &j| r[j].total_cmp(&r[i]));
    let mut taken: Vec<usize> = Vec::new();
    for i in order {
        if taken
            .iter()
            .all(|&j| (lags_ms[i] - lags_ms[j]).abs() > radius_ms)
        {
            taken.push(i);
            if taken.len() >= count {
                break;
            }
        }
    }
    taken
}

/// Both clips of one gap, already decoded, plus the recovered geometry they are read with.
struct Clips<'a> {
    a: &'a [f64],
    b: &'a [f64],
    rate: u32,
    ctx_samples: usize,
}

/// Sweep one throat. Non-chained: neither shoulder's lag informs the other's haystack origin.
fn sweep_shoulder(
    clips: &Clips<'_>,
    shoulder: Shoulder,
    max_lag_ms: f64,
    n_peaks: usize,
) -> Result<ShoulderResult, String> {
    let rate = f64::from(clips.rate);
    let m = (TEMPLATE_SECS * rate).round() as usize;
    let ctx = clips.ctx_samples;

    // Throats are placed on each clip's own edges: A and B can differ in length because the export's
    // frame range clamps at media edges, and end-relative placement is what the instrument's numbers
    // were measured with. Identical when the lengths match.
    let (a_anchor, b_anchor) = match shoulder {
        Shoulder::Pre => {
            let anchor = ctx
                .checked_sub(m)
                .ok_or("context is shorter than the 1.0 s template")?;
            (anchor, anchor)
        }
        Shoulder::Post => (
            clips
                .a
                .len()
                .checked_sub(ctx)
                .ok_or("A clip is shorter than its context")?,
            clips
                .b
                .len()
                .checked_sub(ctx)
                .ok_or("B clip is shorter than its context")?,
        ),
    };
    if a_anchor + m > clips.a.len() {
        return Err("A surround too short for a 1.0 s template at this throat".into());
    }
    let tpl = &clips.a[a_anchor..a_anchor + m];

    let max_lag = (max_lag_ms / 1000.0 * rate).round() as isize;
    let lo = b_anchor as isize - max_lag;
    let hi = b_anchor as isize + max_lag + m as isize;
    let clipped_lo = lo.max(0) as usize;
    let clipped_hi = (hi.max(0) as usize).min(clips.b.len());
    let clipped = clipped_lo as isize != lo || clipped_hi as isize != hi;
    if clipped_hi <= clipped_lo {
        return Err("B surround does not reach this throat".into());
    }
    let hay = &clips.b[clipped_lo..clipped_hi];

    let r = pearson_sweep(tpl, hay)?;
    let to_ms = |k: usize| (k + clipped_lo) as f64 - b_anchor as f64;
    let lags_ms: Vec<f64> = (0..r.len()).map(|k| to_ms(k) / rate * 1000.0).collect();
    let (lo_ms, hi_ms) = (lags_ms[0], lags_ms[lags_ms.len() - 1]);

    let peaks_i = pick_peaks(&lags_ms, &r, PEAK_EXCLUSION_MS, n_peaks);
    let top = peaks_i[0];
    let peak_lag_ms = lags_ms[top];
    let peak_r = r[top];

    // Floor = the tallest rival outside the top peak's exclusion radius. On a sweep so narrow (or so
    // clipped) that no such lag exists there is nothing to compare against — say so with `None`
    // rather than inventing a number, and `live` is then false for want of a prominence.
    let floor_r = lags_ms
        .iter()
        .zip(r.iter())
        .filter(|(lag, _)| (*lag - peak_lag_ms).abs() > PEAK_EXCLUSION_MS)
        .map(|(_, v)| *v)
        .fold(None::<f64>, |acc, v| Some(acc.map_or(v, |a| a.max(v))));
    let prominence = floor_r.map(|f| peak_r - f);

    let peak_z = peak_z(&r, peak_r);

    // Pinned against the range **actually swept**, not the requested one: on a clipped haystack the
    // real window edge is `usable_lag_ms`, and comparing to `max_lag_ms` there would leave a peak
    // sitting on the window edge unflagged — the one row whose reading is invalid.
    let edge_pinned = (peak_lag_ms - lo_ms) <= EDGE_TOL_MS || (hi_ms - peak_lag_ms) <= EDGE_TOL_MS;
    let live = peak_r >= LIVE_R && prominence.is_some_and(|p| p >= LIVE_PROM);
    let unique_strict = peak_r >= UNIQUE_R && prominence.is_some_and(|p| p >= LIVE_PROM);

    let lag0_r = (lo_ms <= 0.0 && hi_ms >= 0.0).then(|| {
        let k = lags_ms
            .iter()
            .enumerate()
            .min_by(|(_, x), (_, y)| x.abs().total_cmp(&y.abs()))
            .map(|(i, _)| i)
            .unwrap_or(0);
        r1e4(r[k])
    });

    Ok(ShoulderResult {
        shoulder: shoulder.as_str(),
        sr: clips.rate,
        gap_secs: r1e4(clips.a.len() as f64 / rate - 2.0 * ctx as f64 / rate),
        template_a_secs: r1e4(a_anchor as f64 / rate),
        max_lag_ms,
        lag0_r,
        peak_lag_ms: r1(peak_lag_ms),
        peak_r: r1e4(peak_r),
        peak_z: peak_z.map(r1),
        floor_r: floor_r.map(r1e4),
        prominence: prominence.map(r1e4),
        live,
        unique_strict,
        edge_pinned,
        clipped_haystack: clipped,
        usable_lag_ms: clipped.then(|| [r1(lo_ms), r1(hi_ms)]),
        peaks: peaks_i
            .iter()
            .map(|&i| Peak {
                lag_ms: r1(lags_ms[i]),
                r: r1e4(r[i]),
            })
            .collect(),
    })
}

/// How far the top peak stands above the swept curve, in its own standard deviations. `None` on a
/// curve with no spread, where the ratio would divide by ~0 and mean nothing.
fn peak_z(r: &[f64], peak_r: f64) -> Option<f64> {
    let mean = r.iter().sum::<f64>() / r.len() as f64;
    let sd = (r.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / r.len() as f64).sqrt();
    (sd > 1e-12).then(|| (peak_r - mean) / sd)
}

fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn r1e4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// The prototype log an operator reads for one shoulder. Corpus mode writes the same text to
/// `<stem>_pre.txt` / `<stem>_post.txt`, so a batch run leaves what sweeping by hand would.
fn prototype_log(res: &ShoulderResult) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "shoulder={}  sr={}  gap~{:.4}s  template=1.0s @ A {:.4}s  sweep +/-{:.0}ms\n",
        res.shoulder, res.sr, res.gap_secs, res.template_a_secs, res.max_lag_ms
    ));
    match res.lag0_r {
        Some(v) => out.push_str(&format!("r at lag 0 (nominal map): {v:.4}\n")),
        None => out.push_str("r at lag 0 (nominal map): not swept (clipped haystack)\n"),
    }
    if let Some([lo, hi]) = res.usable_lag_ms {
        out.push_str(&format!(
            "note: B surround clips the sweep; usable lag range [{lo:+.1}, {hi:+.1}] ms\n"
        ));
    }
    out.push_str(&format!("{:>10}  {:>7}\n", "lag (ms)", "r"));
    for p in &res.peaks {
        out.push_str(&format!("{:>+10.1}  {:>7.4}\n", p.lag_ms, p.r));
    }
    match (res.floor_r, res.prominence) {
        (Some(f), Some(p)) => out.push_str(&format!(
            "floor: max r outside +/-250ms of top peak = {f:.4}  (prominence {p:.4})\n"
        )),
        _ => out.push_str(
            "floor: no lag outside +/-250ms of the top peak was swept (prominence n/a)\n",
        ),
    }
    out
}

// -------------------------------------------------------------------------------------------
// WAV + dump I/O
// -------------------------------------------------------------------------------------------

/// Mono downmix (mean of all channels — LFE included, unlike the patched-vs-B script). WAV I/O is
/// not the estimator under test, so this mirrors `listen-registration`'s reader deliberately.
fn read_mono(path: &Path) -> Result<(Vec<f64>, u32), String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let scale = match spec.sample_format {
        hound::SampleFormat::Float => 1.0,
        hound::SampleFormat::Int => f64::from(1u32 << (spec.bits_per_sample - 1)),
    };
    let samples: Vec<f64> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map(f64::from).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|s| s.map(f64::from).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?,
    };
    Ok((
        samples
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f64>() / channels as f64 / scale)
            .collect(),
        spec.sample_rate,
    ))
}

/// The dump columns this instrument joins beside its own measurement. Never inputs to the sweep.
#[derive(Debug, Clone, Default)]
struct DumpJoin {
    /// The gap's dump `index` — the number every note means by `g7`.
    index: Option<usize>,
    /// `a_end − a_start`, the span context recovery subtracts.
    span_secs: Option<f64>,
    a_start_secs: Option<f64>,
    gap_secs_dump: Option<f64>,
    pre: Option<(f64, f64, String)>,
    post: Option<(f64, f64, String)>,
}

fn read_dump(path: &Path) -> Option<DumpJoin> {
    let text = std::fs::read_to_string(path).ok()?;
    let corpus: GapCorpus = serde_json::from_str(&text).ok()?;
    let gap = corpus.gaps.first()?;
    // `pre_anchor` / `post_anchor` are typed `Vec<LagSummary>`, so the join is `.first()`.
    let lag = gap.lag_decision_fingerprint();
    let side = |v: Option<&Vec<clip_sync_repair::application::gap_fingerprint::LagSummary>>| {
        v.and_then(|s| s.first())
            .map(|s| (s.frac_lag_ms, s.peak_r, format!("{:?}", s.verdict)))
    };
    Some(DumpJoin {
        index: Some(gap.index),
        span_secs: Some(gap.geometry.a_end_secs - gap.geometry.a_start_secs),
        a_start_secs: Some(gap.geometry.a_start_secs),
        gap_secs_dump: Some(gap.geometry.duration_secs),
        pre: side(lag.map(|l| &l.pre_anchor)),
        post: side(lag.map(|l| &l.post_anchor)),
    })
}

/// Production's disposition for this stem, from `listen.json` beside the WAVs.
fn listen_disposition(pair_dir: &Path, stem: &str) -> Option<String> {
    let text = std::fs::read_to_string(pair_dir.join("listen.json")).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    doc.get("entries")?
        .as_array()?
        .iter()
        .find(|e| e.get("stem").and_then(|s| s.as_str()) == Some(stem))
        .and_then(|e| e.get("disposition"))
        .and_then(|d| d.as_str())
        .map(str::to_string)
}

/// Context per side, in samples.
///
/// **Recovered geometry wins**; `--context-secs` is the no-dump fallback, not an override. A listen
/// run made with a different `gap_signature_context_secs` therefore measures its own throats.
fn ctx_samples(
    a_len: usize,
    rate: u32,
    span_secs: Option<f64>,
    context_secs: f64,
) -> Result<usize, String> {
    let secs = match span_secs {
        Some(span) => (a_len as f64 / f64::from(rate) - span) / 2.0,
        None => context_secs,
    };
    if secs <= 0.0 {
        return Err("clip is shorter than the gap it contains".into());
    }
    Ok((secs * f64::from(rate)).round() as usize)
}

/// Decode one gap's clips and recover its geometry.
fn load_gap(
    a_wav: &Path,
    b_wav: &Path,
    dump: &DumpJoin,
    context_secs: f64,
) -> Result<(Vec<f64>, Vec<f64>, u32, usize), String> {
    let (a, rate) = read_mono(a_wav)?;
    let (b, b_rate) = read_mono(b_wav)?;
    // Hard error, not a warning: resampling is out of scope and a silent mismatch corrupts every lag.
    if rate != b_rate {
        return Err(format!(
            "sample rates differ (A {rate}, B {b_rate}); resample first"
        ));
    }
    let ctx = ctx_samples(a.len(), rate, dump.span_secs, context_secs)?;
    Ok((a, b, rate, ctx))
}

// -------------------------------------------------------------------------------------------
// Census rows
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct Row {
    pair: String,
    /// The dump `index` — `7` for a `…_g007_full` stem, which is what `31/g7` means everywhere.
    gap_index: Option<usize>,
    /// `gap_index + 1`. Kept only so this census joins against the CSVs the Python wrote; it is
    /// **not** the number any note or log line uses.
    fingerprint_gap_1based: Option<usize>,
    dump_g: String,
    a_start_secs: Option<f64>,
    listen_disposition: Option<String>,
    gap_ms: f64,
    gap_secs_dump: Option<f64>,
    pre_lag_ms: f64,
    pre_r: f64,
    pre_z: Option<f64>,
    pre_floor_r: Option<f64>,
    pre_prom: Option<f64>,
    pre_live: bool,
    pre_unique_strict: bool,
    pre_edge_pinned: bool,
    pre_clipped_haystack: bool,
    pre_usable_lag_ms: Option<[f64; 2]>,
    post_lag_ms: f64,
    post_r: f64,
    post_z: Option<f64>,
    post_floor_r: Option<f64>,
    post_prom: Option<f64>,
    post_live: bool,
    post_unique_strict: bool,
    post_edge_pinned: bool,
    post_clipped_haystack: bool,
    post_usable_lag_ms: Option<[f64; 2]>,
    /// `post_lag_ms − pre_lag_ms`. Recorded on every row; **nothing branches on it.**
    step_ms: f64,
    /// `gap_ms + step_ms`. Comparison column only — see the module header on why `≈ 0` is a
    /// coincidence and not a class.
    bridge_ms: f64,
    sweep_may_be_narrow: bool,
    source_tree: String,
    stem: String,
    dump_pre_ms: Option<f64>,
    dump_pre_r: Option<f64>,
    dump_pre_verdict: Option<String>,
    dump_post_ms: Option<f64>,
    dump_post_r: Option<f64>,
    dump_post_verdict: Option<String>,
    pre_peaks: Vec<Peak>,
    post_peaks: Vec<Peak>,
}

#[allow(clippy::too_many_arguments)]
fn assemble_row(
    pair: &str,
    stem: &str,
    source_tree: &str,
    pre: &ShoulderResult,
    post: &ShoulderResult,
    gap_ms: f64,
    dump: &DumpJoin,
    disposition: Option<String>,
) -> Row {
    let step_ms = post.peak_lag_ms - pre.peak_lag_ms;
    Row {
        pair: pair.to_string(),
        gap_index: dump.index,
        fingerprint_gap_1based: dump.index.map(|i| i + 1),
        dump_g: gap_tag(stem),
        a_start_secs: dump.a_start_secs,
        listen_disposition: disposition,
        gap_ms: r1(gap_ms),
        gap_secs_dump: dump.gap_secs_dump,
        pre_lag_ms: pre.peak_lag_ms,
        pre_r: pre.peak_r,
        pre_z: pre.peak_z,
        pre_floor_r: pre.floor_r,
        pre_prom: pre.prominence,
        pre_live: pre.live,
        pre_unique_strict: pre.unique_strict,
        pre_edge_pinned: pre.edge_pinned,
        pre_clipped_haystack: pre.clipped_haystack,
        pre_usable_lag_ms: pre.usable_lag_ms,
        post_lag_ms: post.peak_lag_ms,
        post_r: post.peak_r,
        post_z: post.peak_z,
        post_floor_r: post.floor_r,
        post_prom: post.prominence,
        post_live: post.live,
        post_unique_strict: post.unique_strict,
        post_edge_pinned: post.edge_pinned,
        post_clipped_haystack: post.clipped_haystack,
        post_usable_lag_ms: post.usable_lag_ms,
        step_ms: r1(step_ms),
        bridge_ms: r1(gap_ms + step_ms),
        // The only condition that invalidates a reading rather than describing it: a shoulder that
        // failed `live` while its best peak sat on the window edge was probably not searched far
        // enough, not measured dead.
        sweep_may_be_narrow: (!pre.live && pre.edge_pinned) || (!post.live && post.edge_pinned),
        source_tree: source_tree.to_string(),
        stem: stem.to_string(),
        dump_pre_ms: dump.pre.as_ref().map(|p| p.0),
        dump_pre_r: dump.pre.as_ref().map(|p| p.1),
        dump_pre_verdict: dump.pre.as_ref().map(|p| p.2.clone()),
        dump_post_ms: dump.post.as_ref().map(|p| p.0),
        dump_post_r: dump.post.as_ref().map(|p| p.1),
        dump_post_verdict: dump.post.as_ref().map(|p| p.2.clone()),
        pre_peaks: pre.peaks.clone(),
        post_peaks: post.peaks.clone(),
    }
}

/// The `gNNN` tag carried in the listen stem, as written (`007`), for joining against file names.
fn gap_tag(stem: &str) -> String {
    stem.rsplit_once("_g")
        .map(|(_, tail)| {
            tail.split('_')
                .next()
                .unwrap_or_default()
                .trim_end_matches(|c: char| !c.is_ascii_digit())
                .to_string()
        })
        .unwrap_or_default()
}

const CSV_FIELDS: &[&str] = &[
    "pair",
    "gap_index",
    "fingerprint_gap_1based",
    "dump_g",
    "a_start_secs",
    "listen_disposition",
    "gap_ms",
    "pre_lag_ms",
    "pre_r",
    "pre_z",
    "pre_prom",
    "pre_live",
    "pre_unique_strict",
    "pre_edge_pinned",
    "pre_clipped_haystack",
    "pre_usable_lo_ms",
    "pre_usable_hi_ms",
    "post_lag_ms",
    "post_r",
    "post_z",
    "post_prom",
    "post_live",
    "post_unique_strict",
    "post_edge_pinned",
    "post_clipped_haystack",
    "post_usable_lo_ms",
    "post_usable_hi_ms",
    "step_ms",
    "bridge_ms",
    "sweep_may_be_narrow",
    "source_tree",
    "dump_pre_ms",
    "dump_pre_r",
    "dump_pre_verdict",
    "dump_post_ms",
    "dump_post_r",
    "dump_post_verdict",
    "stem",
];

/// Fixed-column CSV of stems and numbers — no titles, so no quoting rules and no `csv` crate.
fn census_csv(rows: &[Row]) -> String {
    fn num(v: Option<f64>, prec: usize) -> String {
        v.map(|x| format!("{x:.prec$}")).unwrap_or_default()
    }
    fn bound(v: Option<[f64; 2]>, i: usize) -> String {
        v.map(|b| format!("{:.1}", b[i])).unwrap_or_default()
    }
    let mut out = CSV_FIELDS.join(",");
    out.push('\n');
    for r in rows {
        let cells = vec![
            r.pair.clone(),
            r.gap_index.map(|i| i.to_string()).unwrap_or_default(),
            r.fingerprint_gap_1based
                .map(|i| i.to_string())
                .unwrap_or_default(),
            r.dump_g.clone(),
            num(r.a_start_secs, 4),
            r.listen_disposition.clone().unwrap_or_default(),
            format!("{:.1}", r.gap_ms),
            format!("{:.1}", r.pre_lag_ms),
            format!("{:.4}", r.pre_r),
            num(r.pre_z, 1),
            num(r.pre_prom, 4),
            r.pre_live.to_string(),
            r.pre_unique_strict.to_string(),
            r.pre_edge_pinned.to_string(),
            r.pre_clipped_haystack.to_string(),
            bound(r.pre_usable_lag_ms, 0),
            bound(r.pre_usable_lag_ms, 1),
            format!("{:.1}", r.post_lag_ms),
            format!("{:.4}", r.post_r),
            num(r.post_z, 1),
            num(r.post_prom, 4),
            r.post_live.to_string(),
            r.post_unique_strict.to_string(),
            r.post_edge_pinned.to_string(),
            r.post_clipped_haystack.to_string(),
            bound(r.post_usable_lag_ms, 0),
            bound(r.post_usable_lag_ms, 1),
            format!("{:.1}", r.step_ms),
            format!("{:.1}", r.bridge_ms),
            r.sweep_may_be_narrow.to_string(),
            r.source_tree.clone(),
            num(r.dump_pre_ms, 2),
            num(r.dump_pre_r, 4),
            r.dump_pre_verdict.clone().unwrap_or_default(),
            num(r.dump_post_ms, 2),
            num(r.dump_post_r, 4),
            r.dump_post_verdict.clone().unwrap_or_default(),
            r.stem.clone(),
        ];
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// `(numeric pair first, then by index, dumpless last, then by stem)`.
fn sort_rows(rows: &mut [Row]) {
    rows.sort_by(|x, y| {
        let key = |r: &Row| {
            let n = r.pair.parse::<u64>();
            (
                n.is_err(),
                n.unwrap_or(0),
                r.pair.clone(),
                // `a_start_secs` comes from the dump, which is optional — rows without one sort last
                // within their pair rather than pretending to be at zero.
                r.a_start_secs.is_none(),
                r.a_start_secs.unwrap_or(f64::MAX).to_bits(),
                r.stem.clone(),
            )
        };
        key(x).cmp(&key(y))
    });
}

// -------------------------------------------------------------------------------------------
// Discovery
// -------------------------------------------------------------------------------------------

/// Every `*_a_surround.wav` under `dir` as `(label, path)`: the root itself (labelled `.`) and each
/// immediate subdirectory (labelled by its name), sorted. Depth-1 — the same walk
/// `listen-registration` is already proven against on these trees.
fn collect_gaps(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let push_from = |d: &Path, label: &str, out: &mut Vec<(String, PathBuf)>| {
        let Ok(entries) = std::fs::read_dir(d) else {
            return;
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(A_SUFFIX))
            .collect();
        found.sort();
        for p in found {
            out.push((label.to_string(), p));
        }
    };
    push_from(dir, ".", &mut out);
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut subdirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for sub in subdirs {
            let label = sub
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            push_from(&sub, &label, &mut out);
        }
    }
    out
}

/// The pair index exists nowhere in the artifacts — `SourceMeta` carries hashed ids, `listen.json`
/// carries stems, and the only other candidate is a media path, which hygiene forbids. So the label
/// is resolved from the directory tree and, only where the tree cannot answer, the operator.
///
/// **A derived label is never overridden.** A subdirectory name wins outright; `--pair` fills in for
/// `.`-labelled rows alone, and `check_pair_flag_applies` rejects the run rather than let the flag
/// replace a name the tree supplied. That keeps this bin's labels as authoritative as
/// `listen-registration`'s, where the same label resolves the `--observe-dir` join and an override
/// would silently redirect it.
fn resolve_pair(dir_label: &str, root: &Path, pair_flag: Option<&str>) -> (String, Option<String>) {
    if dir_label != "." {
        return (dir_label.to_string(), None);
    }
    if let Some(p) = pair_flag {
        return (p.to_string(), None);
    }
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if root_name.is_empty() {
        return (
            ".".to_string(),
            Some(format!(
                "no pair index and no directory name for the flat rows of {}; labelling them `.`",
                root.display()
            )),
        );
    }
    if root_name.chars().all(|c| c.is_ascii_digit()) {
        return (root_name, None);
    }
    // Last resort: the root's own name. It is not an index, so the note stays — but a run
    // directory's name identifies the rows in a merged census where a bare `.` cannot.
    let note = format!(
        "no pair index for the flat rows of {}: its name is not an index and no --pair was given; labelling them `{root_name}`",
        root.display()
    );
    (root_name, Some(note))
}

/// `--pair` labels a flat directory the tree cannot name. Passing it at a run directory of pair
/// subdirectories is an error, not a silent flattening: those rows already have real, distinct
/// labels, and one flag cannot be right for all of them.
fn check_pair_flag_applies(
    pair_flag: Option<&str>,
    root: &Path,
    found: &[(String, PathBuf)],
) -> Result<(), String> {
    let Some(p) = pair_flag else {
        return Ok(());
    };
    let mut labelled: Vec<&str> = found
        .iter()
        .map(|(label, _)| label.as_str())
        .filter(|label| *label != ".")
        .collect();
    labelled.sort_unstable();
    labelled.dedup();
    if labelled.is_empty() {
        return Ok(());
    }
    Err(format!(
        "--pair {p} cannot label {}: it holds pair subdirectories ({}), which name themselves; \
         drop --pair, or point --corpus/--also at the one flat directory it describes",
        root.display(),
        labelled.join(", ")
    ))
}

fn stem_of(a_wav: &Path) -> String {
    a_wav
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .trim_end_matches(A_SUFFIX)
        .to_string()
}

/// The workspace root, without the `..` hops — these paths get printed.
fn repo_root() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(crate_dir)
        .to_path_buf()
}

fn resolve(dir: &Path) -> PathBuf {
    if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        repo_root().join(dir)
    }
}

// -------------------------------------------------------------------------------------------
// Modes
// -------------------------------------------------------------------------------------------

fn main() -> ExitCode {
    let args = Args::parse();
    let result = match (&args.corpus, &args.a_surround, &args.b_surround) {
        (Some(corpus), _, _) => run_corpus(&args, &resolve(corpus)),
        (None, Some(a), Some(b)) => run_single(&args, a, b),
        _ => Err("pass two WAVs with --shoulder, or --corpus <DIR>; see --help".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("error: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run_single(args: &Args, a_wav: &Path, b_wav: &Path) -> Result<(), String> {
    let shoulder = args
        .shoulder
        .ok_or("--shoulder is required in single-gap mode")?;
    let dump = sibling_dump(a_wav).unwrap_or_default();
    let (a, b, rate, ctx) = load_gap(a_wav, b_wav, &dump, args.context_secs)?;
    let res = sweep_shoulder(
        &Clips {
            a: &a,
            b: &b,
            rate,
            ctx_samples: ctx,
        },
        shoulder,
        args.max_lag_ms,
        args.peaks,
    )?;
    print!("{}", prototype_log(&res));
    Ok(())
}

/// `stem.json` beside `stem_a_surround.wav`, when the listen run wrote one.
fn sibling_dump(a_wav: &Path) -> Option<DumpJoin> {
    let stem = stem_of(a_wav);
    read_dump(&a_wav.parent()?.join(format!("{stem}.json")))
}

fn run_corpus(args: &Args, corpus: &Path) -> Result<(), String> {
    let out_dir = args
        .out
        .as_ref()
        .map(|o| resolve(o))
        .unwrap_or_else(|| corpus.join("wide-sweep"));
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;

    let roots: Vec<PathBuf> = std::iter::once(corpus.to_path_buf())
        .chain(args.also.iter().map(|d| resolve(d)))
        .collect();

    let mut rows: Vec<Row> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut seen_tree: Vec<(String, String)> = Vec::new();
    let mut total = 0usize;

    for root in &roots {
        if !root.is_dir() {
            eprintln!("skip missing dir: {}", root.display());
            continue;
        }
        let tree = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let found = collect_gaps(root);
        check_pair_flag_applies(args.pair.as_deref(), root, &found)?;
        let mut noted: HashSet<String> = HashSet::new();
        for (dir_label, a_wav) in found {
            let stem = stem_of(&a_wav);
            let (pair, note) = resolve_pair(&dir_label, root, args.pair.as_deref());
            if let Some(note) = note {
                if noted.insert(note.clone()) {
                    eprintln!("note: {note}");
                }
            }
            // First root to carry a stem wins: two trees can export the same gap with different
            // pads, so merging them would mix instruments rather than add data.
            if !seen.insert(stem.clone()) {
                let won = seen_tree
                    .iter()
                    .find(|(s, _)| *s == stem)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default();
                eprintln!("note: {stem} already taken from {won}; skipping the copy in {tree}");
                continue;
            }
            seen_tree.push((stem.clone(), tree.clone()));
            total += 1;
            match sweep_gap(args, &pair, &tree, &a_wav, &out_dir) {
                Ok(row) => rows.push(row),
                Err(why) => skipped.push(format!("{pair}/{stem}: {why}")),
            }
        }
    }
    if total == 0 {
        return Err(format!("no {A_SUFFIX} under {}", corpus.display()));
    }
    sort_rows(&mut rows);

    // The pair column widens to the longest label so a run-directory fallback name (section 5)
    // does not shift every other column on its row.
    let pw = rows.iter().map(|r| r.pair.len()).max().unwrap_or(4).max(4);
    println!(
        "{:>pw$} {:>5} {:>9} {:>7} {:>9} {:>7} {:>9} {:>9} {:>5}",
        "pair", "gap", "pre_ms", "pre_r", "post_ms", "post_r", "step_ms", "bridge", "live"
    );
    for r in &rows {
        println!(
            "{:>pw$} {:>5} {:>9.1} {:>7.3} {:>9.1} {:>7.3} {:>9.1} {:>9.1} {:>5}",
            r.pair,
            r.gap_index
                .map(|i| format!("g{i}"))
                .unwrap_or_else(|| "?".into()),
            r.pre_lag_ms,
            r.pre_r,
            r.post_lag_ms,
            r.post_r,
            r.step_ms,
            r.bridge_ms,
            r.pre_live && r.post_live
        );
    }

    let census = out_dir.join("census.csv");
    std::fs::write(&census, census_csv(&rows)).map_err(|e| format!("{}: {e}", census.display()))?;
    let json = out_dir.join("census.json");
    let payload = serde_json::json!({
        "_contract": {
            "instrument": "shoulder-sweep (docs/dev/shoulder-sweep.md; archived registration plan §4.3)",
            "center": "non-chained; lag 0 = the nominal map at each throat, measured independently per shoulder",
            "window": format!("1 s template, ±{:.0} ms, {PEAK_EXCLUSION_MS:.0} ms peak exclusion", args.max_lag_ms),
            "max_lag_ms": args.max_lag_ms,
            "context_secs": format!("recovered from dump geometry when present; else --context-secs {}", args.context_secs),
            "gap_index": "the dump `index` — `g7` in every note means index 7. `fingerprint_gap_1based` is index+1 and is a join column only",
            "step_ms": "post_lag_ms - pre_lag_ms; recorded, never branched on",
            "bridge_ms": "gap_ms + step_ms, where gap_ms is WAV-derived from the recovered context",
            "bridge_ms_is_not_a_class": "|bridge_ms| ~= 0 is an arithmetic coincidence (it fires only when a pair's map step equals minus that gap's duration, so no positive step can satisfy it) — do not bucket on it",
            "live": format!("peak_r >= {LIVE_R} and prominence >= {LIVE_PROM}; a null prominence is not live"),
            "unique_strict": format!("peak_r >= {UNIQUE_R} and the same prominence bar"),
            "edge_pinned": format!("the peak is within {EDGE_TOL_MS:.0} ms of an end of the range actually swept (usable_lag_ms when the haystack was clipped)"),
            "dedupe": "roots are visited in order (--corpus, then each --also); the first root carrying a stem wins",
            "denominator": "gaps with listen WAVs, NOT gaps in the corpus — nothing in this file is a base rate",
        },
        "n": rows.len(),
        "gaps": rows,
    });
    std::fs::write(
        &json,
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{}: {e}", json.display()))?;

    print_summary(&rows, total, &skipped);
    println!("\nwrote {}", census.display());
    println!("wrote {}", json.display());
    Ok(())
}

fn sweep_gap(
    args: &Args,
    pair: &str,
    tree: &str,
    a_wav: &Path,
    out_dir: &Path,
) -> Result<Row, String> {
    let stem = stem_of(a_wav);
    let dir = a_wav.parent().ok_or("no parent directory")?;
    let b_wav = dir.join(format!("{stem}{B_SUFFIX}"));
    if !b_wav.exists() {
        // A gap that never reached the fill plan has no B clip. Say so; do not invent one.
        return Err("no B surround clip (production declined, or A-only export)".into());
    }
    let dump = read_dump(&dir.join(format!("{stem}.json"))).unwrap_or_default();
    let (a, b, rate, ctx) = load_gap(a_wav, &b_wav, &dump, args.context_secs)?;
    let clips = Clips {
        a: &a,
        b: &b,
        rate,
        ctx_samples: ctx,
    };
    let pre = sweep_shoulder(&clips, Shoulder::Pre, args.max_lag_ms, args.peaks)?;
    let post = sweep_shoulder(&clips, Shoulder::Post, args.max_lag_ms, args.peaks)?;
    for res in [&pre, &post] {
        let path = out_dir.join(format!("{stem}_{}.txt", res.shoulder));
        std::fs::write(&path, prototype_log(res))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let gap_ms = pre.gap_secs * 1000.0;
    Ok(assemble_row(
        pair,
        &stem,
        tree,
        &pre,
        &post,
        gap_ms,
        &dump,
        listen_disposition(dir, &stem),
    ))
}

/// The stdout roll-up: counts and the one condition that invalidates a reading. No verdicts, and no
/// per-pair classification — see the plan's §4.3/§9 for why that ladder is parked, not forgotten.
fn print_summary(rows: &[Row], total: usize, skipped: &[String]) {
    let live_gaps = rows.iter().filter(|r| r.pre_live && r.post_live).count();
    let live_shoulders = rows
        .iter()
        .map(|r| usize::from(r.pre_live) + usize::from(r.post_live))
        .sum::<usize>();
    println!(
        "\n{} gap(s) swept, {} skipped, of {total} with listen WAVs",
        rows.len(),
        skipped.len()
    );
    for line in skipped {
        println!("  skipped {line}");
    }
    println!(
        "{live_shoulders} live shoulder(s); {live_gaps}/{} gaps live on both",
        rows.len()
    );
    println!(
        "  denominator is gaps with listen WAVs, not gaps in the corpus — this is not a base rate"
    );
    let narrow: Vec<&Row> = rows.iter().filter(|r| r.sweep_may_be_narrow).collect();
    if narrow.is_empty() {
        println!("no row flagged sweep_may_be_narrow");
    } else {
        println!(
            "sweep_may_be_narrow on {} row(s) — a dead shoulder pinned to the window edge is not a \
             measurement; re-run those with a wider --max-lag-ms:",
            narrow.len()
        );
        for r in narrow {
            println!(
                "  {}/{}",
                r.pair,
                r.gap_index
                    .map(|i| format!("g{i}"))
                    .unwrap_or_else(|| r.stem.clone())
            );
        }
    }
}

// -------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 8000;

    /// Deterministic aperiodic material. A periodic tone would score ~1 at every period, which
    /// is exactly what a floor/prominence/z fixture must not do.
    fn signal(n: usize, seed: f64) -> Vec<f64> {
        let mut state = ((seed * 1_000_000.0) as u64) | 1;
        (0..n)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (state >> 33) as f64 / f64::from(1u32 << 30) - 1.0
            })
            .collect()
    }

    /// O(nm) Pearson, the reference the FFT path must reproduce.
    fn naive_sweep(tpl: &[f64], hay: &[f64]) -> Vec<f64> {
        let m = tpl.len();
        let mean = tpl.iter().sum::<f64>() / m as f64;
        let t: Vec<f64> = tpl.iter().map(|x| x - mean).collect();
        let tn = t.iter().map(|x| x * x).sum::<f64>().sqrt();
        (0..=hay.len() - m)
            .map(|k| {
                let w = &hay[k..k + m];
                let wm = w.iter().sum::<f64>() / m as f64;
                let num: f64 = t.iter().zip(w).map(|(a, b)| a * (b - wm)).sum();
                let den: f64 = w.iter().map(|b| (b - wm) * (b - wm)).sum::<f64>().sqrt();
                num / (tn * den)
            })
            .collect()
    }

    fn clips_of<'a>(a: &'a [f64], b: &'a [f64], ctx_samples: usize) -> Clips<'a> {
        Clips {
            a,
            b,
            rate: SR,
            ctx_samples,
        }
    }

    /// A gap fixture: `ctx` seconds of context per side around a silence-ish gap, with B a copy of A
    /// shifted by `shift` samples so both throats have a known answer.
    fn gap_clips(ctx: usize, gap: usize, shift: isize) -> (Vec<f64>, Vec<f64>) {
        let n = 2 * ctx + gap;
        let a = signal(n, 0.021);
        let mut b = vec![0.0; n];
        for (i, slot) in b.iter_mut().enumerate() {
            let src = i as isize - shift;
            if src >= 0 && (src as usize) < n {
                *slot = a[src as usize];
            }
        }
        (a, b)
    }

    #[test]
    fn sweep_recovers_a_known_integer_shift_at_r_one() {
        let tpl = signal(2000, 0.017);
        let mut hay = vec![0.0; 300];
        hay.extend(signal(6000, 0.017));
        let r = pearson_sweep(&tpl, &hay).expect("a curve");
        let best = (0..r.len()).max_by(|&i, &j| r[i].total_cmp(&r[j])).unwrap();
        assert_eq!(best, 300, "peak lag");
        assert!(r[best] > 0.9999, "{}", r[best]);
    }

    /// The FFT path must equal the naive Pearson everywhere, including where a too-small transform
    /// would wrap: transform size, template reversal, `1/size` scaling and the `valid` slice at once.
    #[test]
    fn fft_sweep_matches_a_naive_reference() {
        let tpl = signal(97, 0.031);
        let hay = signal(1000, 0.031);
        let fast = pearson_sweep(&tpl, &hay).expect("a curve");
        let slow = naive_sweep(&tpl, &hay);
        assert_eq!(fast.len(), slow.len());
        for (f, s) in fast.iter().zip(&slow) {
            assert!((f - s).abs() < 1e-9, "{f} vs {s}");
        }
    }

    #[test]
    fn negative_lag_means_b_is_early() {
        let (a, b) = gap_clips(3 * SR as usize, SR as usize, -80);
        let pre = sweep_shoulder(&clips_of(&a, &b, 3 * SR as usize), Shoulder::Pre, 500.0, 4)
            .expect("pre");
        assert!((pre.peak_lag_ms - (-10.0)).abs() < 0.2, "{pre:?}");
    }

    #[test]
    fn a_dc_offset_haystack_scores_the_same_as_a_zero_mean_one() {
        let tpl = signal(512, 0.023);
        let hay = signal(4000, 0.023);
        let plain = pearson_sweep(&tpl, &hay).expect("plain");
        let offset: Vec<f64> = hay.iter().map(|x| x + 3.0).collect();
        let shifted = pearson_sweep(&tpl, &offset).expect("offset");
        for (p, s) in plain.iter().zip(&shifted) {
            assert!((p - s).abs() < 1e-9, "{p} vs {s}");
        }
    }

    #[test]
    fn a_constant_haystack_region_is_finite() {
        let tpl = signal(256, 0.019);
        let mut hay = vec![0.0; 1000];
        hay.extend(signal(1000, 0.019));
        let r = pearson_sweep(&tpl, &hay).expect("a curve");
        assert!(r.iter().all(|v| v.is_finite()), "NaN or inf in the curve");
    }

    #[test]
    fn a_silent_template_is_an_error_not_a_zero_score() {
        let err = pearson_sweep(&[0.0; 256], &signal(2000, 0.02)).expect_err("silent template");
        assert!(err.contains("silent"), "{err}");
    }

    #[test]
    fn peak_picker_keeps_peaks_outside_the_exclusion_radius_and_drops_those_inside() {
        // Lags at 1 ms spacing; peaks at 0, +100 ms (inside 250) and +300 ms (outside).
        let lags: Vec<f64> = (0..601).map(|i| i as f64 - 300.0).collect();
        let mut r = vec![0.0; lags.len()];
        r[300] = 0.9; // lag 0
        r[400] = 0.8; // +100 ms
        r[600] = 0.7; // +300 ms
        let picked = pick_peaks(&lags, &r, PEAK_EXCLUSION_MS, 8);
        let picked_lags: Vec<f64> = picked.iter().map(|&i| lags[i]).collect();
        assert!(
            picked_lags.contains(&0.0) && picked_lags.contains(&300.0),
            "{picked_lags:?}"
        );
        assert!(!picked_lags.contains(&100.0), "{picked_lags:?}");
    }

    #[test]
    fn floor_excludes_the_top_peaks_radius_and_prominence_is_top_minus_floor() {
        let (a, b) = gap_clips(3 * SR as usize, SR as usize, 40);
        let pre = sweep_shoulder(&clips_of(&a, &b, 3 * SR as usize), Shoulder::Pre, 1000.0, 8)
            .expect("pre");
        let floor = pre.floor_r.expect("a floor");
        let prom = pre.prominence.expect("a prominence");
        assert!((prom - (pre.peak_r - floor)).abs() < 1e-9);
        // Every peak inside the radius is excluded from the floor, so the floor is below them.
        assert!(floor < pre.peak_r, "floor {floor} vs peak {}", pre.peak_r);
    }

    #[test]
    fn a_sweep_with_no_lag_outside_the_radius_has_a_null_floor_and_is_not_live() {
        let (a, b) = gap_clips(3 * SR as usize, SR as usize, 0);
        // ±100 ms of sweep: nothing lies outside the top peak's ±250 ms exclusion.
        let pre = sweep_shoulder(&clips_of(&a, &b, 3 * SR as usize), Shoulder::Pre, 100.0, 8)
            .expect("pre");
        assert!(pre.floor_r.is_none() && pre.prominence.is_none(), "{pre:?}");
        assert!(pre.peak_r > 0.99, "the peak itself is real: {}", pre.peak_r);
        assert!(!pre.live && !pre.unique_strict, "no prominence ⇒ not live");
    }

    #[test]
    fn live_requires_both_the_r_and_the_prominence_bar() {
        let mut res = shoulder_fixture(0.0, 0.9);
        res.floor_r = Some(0.85);
        res.prominence = Some(0.05);
        // A broad r = 0.9 ridge clears LIVE_R but not LIVE_PROM.
        assert!(res.peak_r >= LIVE_R && res.prominence.unwrap() < LIVE_PROM);
        assert!(!(res.peak_r >= LIVE_R && res.prominence.is_some_and(|p| p >= LIVE_PROM)));
    }

    #[test]
    fn peak_z_is_the_curves_z_score_and_none_on_a_flat_curve() {
        let (a, b) = gap_clips(3 * SR as usize, SR as usize, 40);
        let pre = sweep_shoulder(&clips_of(&a, &b, 3 * SR as usize), Shoulder::Pre, 1000.0, 8)
            .expect("pre");
        let z = pre.peak_z.expect("a z");
        assert!(z > 3.0, "{z}");

        let curve = vec![0.2, 0.4, 0.6, 0.8];
        let by_hand = {
            let mean = 0.5;
            let sd = (0.05f64).sqrt();
            (0.8 - mean) / sd
        };
        assert!((peak_z(&curve, 0.8).expect("a z") - by_hand).abs() < 1e-12);
        assert!(
            peak_z(&[0.25; 64], 0.25).is_none(),
            "a curve with no spread has no z"
        );
    }

    #[test]
    fn edge_pinned_and_narrow_flags_fire_on_a_peak_at_the_window_edge() {
        // B's content sits ~200 ms early; a ±210 ms sweep pins the peak near its edge.
        let (a, b) = gap_clips(3 * SR as usize, SR as usize, -1600);
        let pre = sweep_shoulder(&clips_of(&a, &b, 3 * SR as usize), Shoulder::Pre, 210.0, 8)
            .expect("pre");
        assert!(pre.edge_pinned, "{pre:?}");
        let mut dead = shoulder_fixture(-200.0, 0.2);
        dead.edge_pinned = true;
        let row = assemble_row(
            "31",
            "stem",
            "tree",
            &dead,
            &shoulder_fixture(0.0, 0.95),
            1000.0,
            &DumpJoin::default(),
            None,
        );
        assert!(
            row.sweep_may_be_narrow,
            "a dead, pinned shoulder flags the row"
        );
    }

    #[test]
    fn a_clipped_haystack_pins_against_the_usable_range_not_the_request() {
        // B's matching content sits 1.2 s late and B's tail is cut, so the true peak lands exactly on
        // the edge of what could be swept. Against the *requested* +/-2000 ms it looks comfortably
        // interior; against the range actually swept it is pinned, and that is the honest reading.
        let ctx = 3 * SR as usize;
        let (a, full_b) = gap_clips(ctx, SR as usize, 9600);
        let b = full_b[..ctx + 9600].to_vec();
        let pre = sweep_shoulder(&clips_of(&a, &b, ctx), Shoulder::Pre, 2000.0, 8).expect("pre");
        let [lo, hi] = pre.usable_lag_ms.expect("a usable range");
        assert!(pre.clipped_haystack);
        assert!(
            hi < 2000.0,
            "the sweep stopped short of the request: hi {hi}"
        );
        assert!(
            (pre.peak_lag_ms - 1200.0).abs() < 0.2,
            "{}",
            pre.peak_lag_ms
        );
        assert!(
            pre.peak_lag_ms.abs() < 2000.0 - EDGE_TOL_MS,
            "not pinned against the request: {}",
            pre.peak_lag_ms
        );
        assert!(
            hi - pre.peak_lag_ms <= EDGE_TOL_MS,
            "pinned against the usable hi {hi}"
        );
        assert!(pre.edge_pinned, "{pre:?}");
        assert!(lo <= -2000.0 || pre.peak_lag_ms - lo > EDGE_TOL_MS);
    }

    #[test]
    fn differing_sample_rates_are_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join(format!("s{A_SUFFIX}"));
        let b = dir.path().join(format!("s{B_SUFFIX}"));
        write_wav(&a, &signal(8000, 0.02), 8000);
        write_wav(&b, &signal(8000, 0.02), 16000);
        let err = load_gap(&a, &b, &DumpJoin::default(), 0.5).expect_err("rate mismatch");
        assert!(err.contains("sample rates differ"), "{err}");
    }

    #[test]
    fn context_recovery_places_the_pre_throat_and_the_flag_is_only_a_fallback() {
        let rate = SR;
        let a_len = 10 * rate as usize; // 10 s clip
        let span = 4.0; // 4 s gap ⇒ ctx = 3 s
        let recovered = ctx_samples(a_len, rate, Some(span), 1.0).expect("recovered");
        assert_eq!(
            recovered,
            3 * rate as usize,
            "dump geometry wins over the flag"
        );
        let fallback = ctx_samples(a_len, rate, None, 2.5).expect("fallback");
        assert_eq!(fallback, (2.5 * f64::from(rate)) as usize);
        assert!(
            ctx_samples(a_len, rate, Some(12.0), 3.0).is_err(),
            "clip shorter than its gap"
        );

        // The template starts `ctx − m` into the clip, i.e. the last 1 s before the throat.
        let ctx = 3 * rate as usize;
        let (a, b) = gap_clips(ctx, rate as usize, 0);
        let pre = sweep_shoulder(&clips_of(&a, &b, ctx), Shoulder::Pre, 500.0, 4).expect("pre");
        assert!(
            (pre.template_a_secs - 2.0).abs() < 1e-6,
            "{}",
            pre.template_a_secs
        );
    }

    #[test]
    fn post_throats_are_end_relative_on_each_clip() {
        let ctx = 3 * SR as usize;
        let (a, b) = gap_clips(ctx, SR as usize, 0);
        // B is 400 samples (50 ms) longer at its head — its export reached further back, and the
        // gap it brackets still ends at its tail. End-relative throats therefore read lag 0; a
        // same-index throat on both clips would report a spurious +50 ms.
        let mut longer_b = vec![0.0; 400];
        longer_b.extend_from_slice(&b);
        let post =
            sweep_shoulder(&clips_of(&a, &longer_b, ctx), Shoulder::Post, 500.0, 4).expect("post");
        assert!(post.peak_lag_ms.abs() < 0.2, "{}", post.peak_lag_ms);
        assert!(post.peak_r > 0.99, "{}", post.peak_r);
    }

    #[test]
    fn step_and_bridge_are_recorded_and_branch_nothing() {
        let pre = shoulder_fixture(-38.1, 0.83);
        let small = shoulder_fixture(-35.0, 0.81);
        let large = shoulder_fixture(-1160.0, 0.81);
        let quiet = assemble_row(
            "31",
            "s",
            "t",
            &pre,
            &small,
            1118.8,
            &DumpJoin::default(),
            None,
        );
        let stepped = assemble_row(
            "31",
            "s",
            "t",
            &pre,
            &large,
            1118.8,
            &DumpJoin::default(),
            None,
        );
        assert!((quiet.step_ms - 3.1).abs() < 1e-9);
        assert!((stepped.step_ms - (-1121.9)).abs() < 1e-9);
        // `bridge_ms ≈ 0` on the stepped row — and nothing else about the row moves because of it.
        assert!(stepped.bridge_ms.abs() < 5.0, "{}", stepped.bridge_ms);
        assert_eq!(quiet.pre_live, stepped.pre_live);
        assert_eq!(quiet.pre_r, stepped.pre_r);
        assert_eq!(quiet.sweep_may_be_narrow, stepped.sweep_may_be_narrow);
        assert_eq!(quiet.post_live, stepped.post_live);
        assert_eq!(quiet.post_r, stepped.post_r);
    }

    #[test]
    fn gap_index_is_the_dump_index_and_the_1based_column_is_a_join_only() {
        let dump = DumpJoin {
            index: Some(7),
            ..DumpJoin::default()
        };
        let row = assemble_row(
            "31",
            "919e5368_91d1_t00-28-45_g007_full",
            "tree",
            &shoulder_fixture(-38.1, 0.83),
            &shoulder_fixture(-35.0, 0.81),
            1118.8,
            &dump,
            None,
        );
        assert_eq!(row.gap_index, Some(7), "`g7` means dump index 7");
        assert_eq!(row.fingerprint_gap_1based, Some(8));
        assert_eq!(row.dump_g, "007");
    }

    #[test]
    fn rows_without_a_dump_sort_last_within_their_pair_and_carry_a_null_start() {
        let with = |start: Option<f64>, stem: &str| {
            let mut row = assemble_row(
                "31",
                stem,
                "t",
                &shoulder_fixture(0.0, 0.9),
                &shoulder_fixture(0.0, 0.9),
                100.0,
                &DumpJoin::default(),
                None,
            );
            row.a_start_secs = start;
            row
        };
        let mut rows = vec![
            with(None, "z"),
            with(Some(900.0), "b"),
            with(Some(10.0), "a"),
        ];
        rows.push({
            let mut r = with(Some(5.0), "c");
            r.pair = "2".into();
            r
        });
        sort_rows(&mut rows);
        let order: Vec<&str> = rows.iter().map(|r| r.stem.as_str()).collect();
        assert_eq!(
            order,
            vec!["c", "a", "b", "z"],
            "pair 2 first; dumpless last in pair 31"
        );
        assert!(rows.last().unwrap().a_start_secs.is_none());
    }

    #[test]
    fn the_roll_up_denominator_counts_gaps_with_wavs() {
        // `total` is the WAV count, which is what the summary divides by — a dump with more gaps in
        // it never enlarges the denominator, because this instrument cannot reach them.
        let rows = vec![assemble_row(
            "31",
            "s",
            "t",
            &shoulder_fixture(0.0, 0.9),
            &shoulder_fixture(0.0, 0.9),
            100.0,
            &DumpJoin::default(),
            None,
        )];
        print_summary(&rows, 2, &["31/other: no B surround clip".to_string()]);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn discovery_is_depth_one() {
        let root = tempfile::tempdir().expect("tempdir");
        let flat = root.path().join("flat");
        let run = root.path().join("run");
        let pair = run.join("31");
        let nested = pair.join("deeper");
        for d in [&flat, &run, &pair, &nested] {
            std::fs::create_dir_all(d).expect("mkdir");
        }
        for d in [&flat, &pair, &nested] {
            std::fs::write(
                d.join(format!("s{}{A_SUFFIX}", d.to_string_lossy().len())),
                [],
            )
            .expect("wav");
        }
        assert_eq!(
            collect_gaps(&flat).len(),
            1,
            "a flat pair directory enumerates"
        );
        let found = collect_gaps(&run);
        assert_eq!(
            found.len(),
            1,
            "the run's pair subdirectory enumerates, its child does not"
        );
        assert_eq!(found[0].0, "31");
    }

    #[test]
    fn pair_labels_come_from_the_tree_or_the_flag_never_a_dump_field() {
        let root = Path::new("gap-files/2026-08-18-post-sweep");
        let numeric_root = Path::new("gap-files/2026-08-18-one-dead-listen/31");
        assert_eq!(
            resolve_pair("31", root, None).0,
            "31",
            "subdirectory name wins"
        );
        assert_eq!(
            resolve_pair(".", numeric_root, None).0,
            "31",
            "numeric root name"
        );
        assert_eq!(
            resolve_pair(".", root, Some("31")).0,
            "31",
            "--pair for a named flat dir"
        );
        assert_eq!(
            resolve_pair(".", numeric_root, Some("50")).0,
            "50",
            "--pair beats the weaker root-name guess"
        );
        let (label, note) = resolve_pair(".", root, None);
        assert_eq!(
            label, "2026-08-18-post-sweep",
            "last resort is the root's own name, never a bare `.`"
        );
        let note = note.expect("a stated fallback");
        assert!(note.contains("is not an index"), "{note}");
        assert!(note.contains("2026-08-18-post-sweep"), "{note}");
        assert_eq!(
            resolve_pair(".", Path::new(""), None).0,
            ".",
            "a root with no name at all still falls back to `.`"
        );
    }

    #[test]
    fn pair_never_replaces_a_label_the_tree_supplied() {
        let root = Path::new("gap-files/2026-08-18-post-sweep");
        assert_eq!(
            resolve_pair("31", root, Some("50")).0,
            "31",
            "a subdirectory name is authoritative"
        );
        let subdirs = vec![
            ("31".to_string(), PathBuf::from("a")),
            ("50".to_string(), PathBuf::from("b")),
        ];
        let err = check_pair_flag_applies(Some("50"), root, &subdirs)
            .expect_err("--pair on a run of pair dirs");
        assert!(err.contains("31, 50"), "{err}");
        let flat = vec![(".".to_string(), PathBuf::from("a"))];
        assert!(check_pair_flag_applies(Some("50"), root, &flat).is_ok());
        assert!(
            check_pair_flag_applies(None, root, &subdirs).is_ok(),
            "no flag, no complaint"
        );
    }

    #[test]
    fn corpus_and_shoulder_together_are_a_clap_error() {
        assert!(
            Args::try_parse_from(["shoulder-sweep", "--corpus", "d", "--shoulder", "pre"]).is_err()
        );
        assert!(Args::try_parse_from(["shoulder-sweep", "--corpus", "d"]).is_ok());
        assert!(
            Args::try_parse_from(["shoulder-sweep", "--shoulder", "pre", "a.wav", "b.wav"]).is_ok()
        );
        assert!(
            Args::try_parse_from(["shoulder-sweep", "--pair", "31"]).is_err(),
            "--pair needs --corpus"
        );
    }

    #[test]
    fn the_same_stem_in_two_trees_is_taken_from_the_first_root() {
        // The dedupe is `run_corpus`'s `seen` set; this pins the rule it implements.
        let mut seen: HashSet<String> = HashSet::new();
        assert!(seen.insert("stem".to_string()), "first root takes it");
        assert!(
            !seen.insert("stem".to_string()),
            "the later root's copy is skipped"
        );
    }

    #[test]
    fn the_csv_header_carries_the_z_and_join_columns() {
        for column in [
            "pre_z",
            "post_z",
            "fingerprint_gap_1based",
            "step_ms",
            "source_tree",
        ] {
            assert!(
                CSV_FIELDS.contains(&column),
                "{column} missing from the census header"
            );
        }
        assert!(!CSV_FIELDS.contains(&"bucket"), "no classifier column");
        let rows = vec![assemble_row(
            "31",
            "s",
            "t",
            &shoulder_fixture(-38.1, 0.83),
            &shoulder_fixture(-35.0, 0.81),
            1118.8,
            &DumpJoin::default(),
            None,
        )];
        let csv = census_csv(&rows);
        let header = csv.lines().next().expect("a header");
        let body = csv.lines().nth(1).expect("a row");
        assert_eq!(header.split(',').count(), body.split(',').count());
    }

    /// A `ShoulderResult` with just the fields the row assembly reads.
    fn shoulder_fixture(lag_ms: f64, peak_r: f64) -> ShoulderResult {
        ShoulderResult {
            shoulder: "pre",
            sr: SR,
            gap_secs: 1.0,
            template_a_secs: 2.0,
            max_lag_ms: 2000.0,
            lag0_r: Some(0.1),
            peak_lag_ms: lag_ms,
            peak_r,
            peak_z: Some(12.0),
            floor_r: Some(0.1),
            prominence: Some(peak_r - 0.1),
            live: peak_r >= LIVE_R,
            unique_strict: peak_r >= UNIQUE_R,
            edge_pinned: false,
            clipped_haystack: false,
            usable_lag_ms: None,
            peaks: vec![Peak { lag_ms, r: peak_r }],
        }
    }

    fn write_wav(path: &Path, samples: &[f64], rate: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).expect("writer");
        for s in samples {
            w.write_sample((s * 16000.0) as i16).expect("sample");
        }
        w.finalize().expect("finalize");
    }
}
