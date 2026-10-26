//! `edge-onset` — the independent soft-edge instrument on `--gap-listen` WAVs.
//!
//! **What it is for.** A gap's scan boundary is where every channel first stays below −60 dBFS. A
//! dropout whose onset is a fast fall to a band *above* that floor (50/g7: −20 dB in 2 ms to −51 …
//! −65 dBFS, ~26 ms before digital silence) leaves the band outside the gap: the scanner and the
//! patch-time `refine_gap_frames` walk both stop at the last above-floor frame, and the band plays
//! before the pre seam. This bin measures, per gap edge, where the dropout's onset is, where the
//! floor edge is, and where the patch actually writes, so the soft band's size and exposure are known.
//!
//! **Independence.** It does not call the scanner, `is_silent_interleaved`, `is_silent_frame` or
//! `refine_gap_frames`; the −60 dBFS comparison below is this bin's own. WAV I/O mirrors the sibling
//! bins'. The dump's `a_refined_*` edges are joined **beside** the measurement as a cross-check, never
//! used as an input.
//!
//! **What it does not do.** It measures and stops: no classifier, no verdict per edge. Two onsets are
//! recorded because the rule has a known false-positive shape (programme decay into quiet programme,
//! then the dropout) and the reader, not this bin, decides which one a row means.
//!
//! **Specification.** `docs/dev/TEMP-soft-edge-census-plan.md` § 3 is this bin's contract — the
//! onset rule, the floor edge, the write-start interval and its bias estimate, and the columns. Read
//! it before changing any number here.
//!
//! **Usage.**
//!
//! ```text
//! cargo run --features calibration --bin edge-onset -- --corpus gap-files/<listen-run>
//! cargo run --features calibration --bin edge-onset -- \
//!     --corpus gap-files/<listen-run>/dual --also gap-files/<listen-run>/single
//! ```
//!
//! Media hygiene: prints pair labels, gap indices, relative directories and file stems only — never
//! media paths or titles. Relative `--corpus` / `--also` / `--out` resolve against the repo root.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use serde::Serialize;

use clip_sync_repair::application::gap_fingerprint::GapCorpus;

const A_SUFFIX: &str = "_a_surround.wav";
const PATCHED_SUFFIX: &str = "_a_patched.wav";
const B_SUFFIX: &str = "_b_surround.wav";

/// Level-bin width. Short enough to resolve a 2 ms fall; the onset is then refined to the sample.
const BIN_MS: f64 = 2.0;
/// How far outward from each boundary the onset and the floor edge are searched. Wider than the
/// backlog row's 100 ms so a band longer than the one known 26 ms case is not clipped.
const WINDOW_MS: f64 = 200.0;
/// A fall (lead) or rise (trail) this large, sustained to the boundary, is a dropout onset.
const FALL_DB: f64 = 15.0;
/// The fall is measured from the loudest of this many bins before it (the rise to the loudest of
/// this many from it), so a fall that straddles a bin edge — half its drop in each of two bins, as
/// on 50/g7 (−22 → −30 → −45 dBFS) — still reads as one 15 dB step. With one bin the rule depends
/// on where the bin grid happens to fall.
const REF_BINS: usize = 2;
/// −60 dBFS. On 16-bit WAVs `|x| >= 0.001` is `|int| >= 33`, the scan's own floor; the comparison
/// is this bin's code, not the scanner's.
const FLOOR_AMP: f64 = 0.001;
/// Level of an all-zero bin, so digital silence orders below everything without `-inf`.
const ZERO_DB: f64 = -140.0;
/// A write this far past the prediction is over programme, not a mis-measured seam: listed apart.
const WIDE_WRITE_MS: f64 = 50.0;
/// Two arms' writes within this many ms (one 48 kHz sample, plus rounding) are the same write.
const SAME_WRITE_MS: f64 = 0.05;
/// How deep below the gap dir the discovery walk goes (`<run>/<arm>/<pair>` is depth 2).
const MAX_DEPTH: usize = 3;

#[derive(Parser, Debug)]
#[command(
    about = "Soft-edge census of --gap-listen clips: dropout onset, floor edge and measured write start per gap edge"
)]
struct Args {
    /// A `--gap-listen` tree: any directory holding `*_a_surround.wav`, up to three levels down.
    #[arg(long)]
    corpus: PathBuf,

    /// Extra trees measured into the same census. Unlike `shoulder-sweep`, a stem seen in two trees
    /// is **kept** in both: two arms of one listen run share raw A but not the write, and comparing
    /// their writes is the point.
    #[arg(long)]
    also: Vec<PathBuf>,

    /// Pair label for rows the directory tree cannot name. Fills in; never overrides.
    #[arg(long)]
    pair: Option<String>,

    /// Where to write `census.csv` / `census.json`. Default `<corpus>/edge-onset/`.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Export context per side, seconds — the **fallback** only, used when no sibling dump JSON
    /// gives the gap's span.
    #[arg(long, default_value_t = 3.0)]
    context_secs: f64,

    /// The patch's seam crossfade, milliseconds: the hard width of the write-start interval and the
    /// `cf` of the bias estimate. A config value, not a reading of the seam code.
    #[arg(long, default_value_t = 10.0)]
    crossfade_ms: f64,
}

// -------------------------------------------------------------------------------------------
// WAV
// -------------------------------------------------------------------------------------------

/// Interleaved, normalized to `[-1, 1]`, all channels kept: the floor test is per channel.
#[derive(Debug, Clone)]
struct Wav {
    channels: usize,
    rate: u32,
    /// One quantization step in normalized units (`2^-(bits-1)`; `2^-24` for float WAVs).
    lsb: f64,
    samples: Vec<f64>,
}

impl Wav {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }

    fn frame(&self, f: usize) -> &[f64] {
        &self.samples[f * self.channels..(f + 1) * self.channels]
    }

    /// Max `|x|` across channels.
    fn frame_peak(&self, f: usize) -> f64 {
        self.frame(f).iter().fold(0.0, |m, x| m.max(x.abs()))
    }

    fn frames_to_ms(&self, frames: f64) -> f64 {
        frames / f64::from(self.rate) * 1000.0
    }

    fn ms_to_frames(&self, ms: f64) -> usize {
        (ms / 1000.0 * f64::from(self.rate)).round().max(0.0) as usize
    }
}

fn read_wav(path: &Path) -> Result<Wav, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let (samples, lsb) = match spec.sample_format {
        hound::SampleFormat::Float => (
            reader
                .samples::<f32>()
                .map(|s| s.map(f64::from).map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?,
            2f64.powi(-24),
        ),
        hound::SampleFormat::Int => {
            let scale = f64::from(1u32 << (spec.bits_per_sample - 1));
            (
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| f64::from(v) / scale).map_err(|e| e.to_string()))
                    .collect::<Result<Vec<_>, _>>()?,
                1.0 / scale,
            )
        }
    };
    Ok(Wav {
        channels,
        rate: spec.sample_rate,
        lsb,
        samples,
    })
}

// -------------------------------------------------------------------------------------------
// Measurement
// -------------------------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Edge {
    Lead,
    Trail,
}

impl Edge {
    fn as_str(self) -> &'static str {
        match self {
            Edge::Lead => "lead",
            Edge::Trail => "trail",
        }
    }
}

fn db(amp: f64) -> f64 {
    if amp <= 0.0 {
        ZERO_DB
    } else {
        (20.0 * amp.log10()).max(ZERO_DB)
    }
}

fn amp(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Per-bin peak dBFS (max across channels), `nbins` chronological bins of `bin` frames from `lo`.
fn bin_levels(w: &Wav, lo: usize, nbins: usize, bin: usize) -> Vec<f64> {
    (0..nbins)
        .map(|k| {
            let s = lo + k * bin;
            db((s..s + bin).map(|f| w.frame_peak(f)).fold(0.0, f64::max))
        })
        .collect()
}

/// Lead rule on bins ending at the boundary: every `i` whose level, and every level after it up to
/// the boundary, is at least [`FALL_DB`] below the loudest of the [`REF_BINS`] bins before `i`.
/// Returns `(i, reference dB)`, ascending.
fn lead_falls(l: &[f64]) -> Vec<(usize, f64)> {
    let n = l.len();
    let mut suffix_max = vec![f64::NEG_INFINITY; n + 1];
    for i in (0..n).rev() {
        suffix_max[i] = suffix_max[i + 1].max(l[i]);
    }
    (1..n)
        .filter_map(|i| {
            let reference = l[i.saturating_sub(REF_BINS)..i]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            (suffix_max[i] <= reference - FALL_DB).then_some((i, reference))
        })
        .collect()
}

/// Trail rule on bins starting at the boundary: every `j` whose [`REF_BINS`]-bin level from `j` is
/// at least [`FALL_DB`] above every bin before it back to the boundary. Returns `(j, level dB)`,
/// ascending.
fn trail_rises(l: &[f64]) -> Vec<(usize, f64)> {
    let mut prefix_max = f64::NEG_INFINITY;
    let mut out = Vec::new();
    for j in 0..l.len() {
        let level = l[j..(j + REF_BINS).min(l.len())]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        if j >= 1 && prefix_max <= level - FALL_DB {
            out.push((j, level));
        }
        prefix_max = prefix_max.max(l[j]);
    }
    out
}

/// Collapse onsets (outward distances, frames) closer than `merge` into one, represented by its
/// outermost member — where the fall begins. Two bins of one decay can both clear [`FALL_DB`] by a
/// fraction of a dB (50/g7: 25.9 and 23.8 ms) and must not read as the room-tone shape, whose falls
/// are tens of ms apart. Returns distances ascending, nearest first.
fn merge_onsets(mut outward: Vec<usize>, merge: usize) -> Vec<usize> {
    outward.sort_unstable();
    let mut out: Vec<usize> = Vec::new();
    for d in outward {
        match out.last_mut() {
            Some(last) if d - *last <= merge => *last = d,
            _ => out.push(d),
        }
    }
    out
}

/// One edge's onset and floor reading on raw A. Distances are milliseconds **outward** from the scan
/// boundary into programme: before it on the lead edge, after it on the trail edge.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
struct OnsetReading {
    /// The window actually searched; under [`WINDOW_MS`] only when the clip edge clipped it.
    window_ms: f64,
    /// Distinct onsets satisfying the rule (onsets within [`REF_BINS`] bins of each other count
    /// once) outward of the floor edge, so a reader can see a multi-step decay.
    n_falls: usize,
    /// Onsets nearer the boundary than the floor edge: falls *inside* the silent run (a sub-floor
    /// tail stepping to digital zero). Already covered by refinement, so never an onset column.
    n_inside_floor: usize,
    /// The farthest qualifying fall (lead: earliest; trail: latest).
    onset_far_ms: Option<f64>,
    /// The nearest qualifying fall.
    onset_near_ms: Option<f64>,
    /// The last frame with any channel at or above −60 dBFS before the lead boundary (the first one
    /// after the trail boundary), as the edge of the silent run it bounds. `None` when the whole
    /// window is below the floor.
    floor_ms: Option<f64>,
    /// Peak across channels over the near band (near onset to the boundary).
    band_peak_dbfs: Option<f64>,
    /// Whether that band holds any sample at or above −60 dBFS.
    band_above_floor: Option<bool>,
}

/// The onset rule (§ 3), refined to the sample: the lead onset is the frame after the last one in the
/// reference bins that exceeds the reference level less [`FALL_DB`] — every later frame up to the
/// boundary is at or below it by construction. The trail onset mirrors it.
fn measure_onset(a: &Wav, boundary: usize, edge: Edge) -> OnsetReading {
    let bin = a.ms_to_frames(BIN_MS).max(1);
    let want = a.ms_to_frames(WINDOW_MS);
    let boundary = boundary.min(a.frames());
    // Each arm yields the searched bin count, raw onsets and the floor edge, all as outward
    // distances in frames; the band (boundary to an outward distance) is read the same way after.
    let (nbins, raw, floor) = match edge {
        Edge::Lead => {
            let nbins = want.min(boundary) / bin;
            let lo = boundary - nbins * bin;
            let l = bin_levels(a, lo, nbins, bin);
            let raw: Vec<usize> = lead_falls(&l)
                .into_iter()
                .map(|(i, reference)| {
                    let thr = amp(reference - FALL_DB);
                    let s = lo + i.saturating_sub(REF_BINS) * bin;
                    let f = (s..lo + i * bin)
                        .rev()
                        .find(|&f| a.frame_peak(f) > thr)
                        .map_or(s, |f| f + 1);
                    boundary - f
                })
                .collect();
            let floor = (lo..boundary)
                .rev()
                .find(|&f| a.frame_peak(f) >= FLOOR_AMP)
                .map(|f| boundary - (f + 1));
            (nbins, raw, floor)
        }
        Edge::Trail => {
            let nbins = want.min(a.frames() - boundary) / bin;
            let lo = boundary;
            let l = bin_levels(a, lo, nbins, bin);
            let raw: Vec<usize> = trail_rises(&l)
                .into_iter()
                .map(|(j, level)| {
                    let thr = amp(level - FALL_DB);
                    let s = lo + j * bin;
                    let f = (s..lo + (j + REF_BINS).min(nbins) * bin)
                        .find(|&f| a.frame_peak(f) > thr)
                        .unwrap_or(s);
                    f - boundary
                })
                .collect();
            let floor = (boundary..lo + nbins * bin)
                .find(|&f| a.frame_peak(f) >= FLOOR_AMP)
                .map(|f| f - boundary);
            (nbins, raw, floor)
        }
    };
    // An onset is a fall *into* the silent run, so it sits at or outward of the floor edge; one bin
    // of slack keeps the bucket-edge case (onset ≈ floor edge). No floor edge in the window means the
    // whole window is the silent run.
    let (onsets, inside): (Vec<usize>, Vec<usize>) = merge_onsets(raw, REF_BINS * bin)
        .into_iter()
        .partition(|&d| floor.is_some_and(|fl| d + bin >= fl));
    let near = onsets.first().copied();
    let band_peak = near.map(|n| {
        let range = match edge {
            Edge::Lead => boundary - n..boundary,
            Edge::Trail => boundary..boundary + n,
        };
        range.map(|f| a.frame_peak(f)).fold(0.0, f64::max)
    });
    let ms = |d: usize| a.frames_to_ms(d as f64);
    OnsetReading {
        window_ms: a.frames_to_ms((nbins * bin) as f64),
        n_falls: onsets.len(),
        n_inside_floor: inside.len(),
        onset_far_ms: onsets.last().map(|&d| ms(d)),
        onset_near_ms: near.map(ms),
        floor_ms: floor.map(ms),
        band_peak_dbfs: band_peak.map(db),
        band_above_floor: band_peak.map(|p| p >= FLOOR_AMP),
    }
}

/// First and last frame where any channel of `patched` differs from `surround` by at least one LSB
/// (any change, on an integer WAV). `None` when the patch left the clip untouched.
fn diff_span(surround: &Wav, patched: &Wav) -> Result<Option<(usize, usize)>, String> {
    if surround.channels != patched.channels || surround.frames() != patched.frames() {
        return Err(format!(
            "patched clip shape differs from surround ({}ch × {} vs {}ch × {})",
            patched.channels,
            patched.frames(),
            surround.channels,
            surround.frames()
        ));
    }
    let tol = 0.5 * surround.lsb.min(patched.lsb);
    let differs = |f: usize| {
        surround
            .frame(f)
            .iter()
            .zip(patched.frame(f))
            .any(|(a, p)| (a - p).abs() >= tol)
    };
    let n = surround.frames();
    let Some(first) = (0..n).find(|&f| differs(f)) else {
        return Ok(None);
    };
    let last = (first..n).rev().find(|&f| differs(f)).unwrap_or(first);
    Ok(Some((first, last)))
}

/// Max-across-channels RMS over `[lo, hi)`. `None` on an empty range.
fn rms_max_channel(w: &Wav, lo: usize, hi: usize) -> Option<f64> {
    let hi = hi.min(w.frames());
    if lo >= hi {
        return None;
    }
    (0..w.channels)
        .map(|ch| {
            let ss: f64 = (lo..hi)
                .map(|f| w.samples[f * w.channels + ch].powi(2))
                .sum();
            (ss / (hi - lo) as f64).sqrt()
        })
        .reduce(f64::max)
}

/// How late `write_first_diff` reads against the true ramp start (§ 3): over the ramp the difference
/// is about `sin θ · b`, so it first clears one LSB at `θ = asin(LSB / b)`. An **estimate** — `b`
/// oscillates, so the true lateness can exceed it by up to a period of B's dominant content — and
/// never more than one crossfade, the hard bound.
fn write_bias_ms(lsb: f64, b_rms: Option<f64>, crossfade_ms: f64) -> f64 {
    match b_rms {
        Some(b) if b > 0.0 => {
            crossfade_ms * std::f64::consts::FRAC_2_PI * (lsb / b).min(1.0).asin()
        }
        _ => crossfade_ms,
    }
}

/// One edge's write reading: where the patch's write meets this edge, outward from the boundary.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
struct WriteReading {
    /// Lead: `write_first_diff`; trail: one past the last differing frame. Positive = outward of the
    /// scan boundary (the write covers some of the programme side).
    write_ms: Option<f64>,
    /// B's max-channel RMS over the crossfade outward of the measured write edge.
    b_level_dbfs: Option<f64>,
    write_bias_ms: Option<f64>,
}

fn measure_write(
    surround: &Wav,
    span: Option<(usize, usize)>,
    b: Option<&Wav>,
    boundary: usize,
    edge: Edge,
    crossfade_ms: f64,
) -> WriteReading {
    let Some((first, last)) = span else {
        return WriteReading::default();
    };
    let cf = surround.ms_to_frames(crossfade_ms);
    let (write_ms, b_lo, b_hi) = match edge {
        Edge::Lead => (
            surround.frames_to_ms(boundary as f64 - first as f64),
            first.saturating_sub(cf),
            first,
        ),
        Edge::Trail => (
            surround.frames_to_ms((last + 1) as f64 - boundary as f64),
            last + 1,
            last + 1 + cf,
        ),
    };
    let b_rms = b.and_then(|b| rms_max_channel(b, b_lo, b_hi));
    WriteReading {
        write_ms: Some(write_ms),
        b_level_dbfs: b_rms.map(db),
        write_bias_ms: Some(write_bias_ms(surround.lsb, b_rms, crossfade_ms)),
    }
}

// -------------------------------------------------------------------------------------------
// Geometry and joins
// -------------------------------------------------------------------------------------------

/// The dump columns joined beside the measurement. Geometry is used only to place the boundaries.
#[derive(Debug, Clone, Default)]
struct DumpJoin {
    index: Option<usize>,
    a_start_secs: Option<f64>,
    a_end_secs: Option<f64>,
    a_refined_start_secs: Option<f64>,
    a_refined_end_secs: Option<f64>,
    codec: Option<String>,
}

fn read_dump(path: &Path) -> Option<DumpJoin> {
    let text = std::fs::read_to_string(path).ok()?;
    let corpus: GapCorpus = serde_json::from_str(&text).ok()?;
    let gap = corpus.gaps.first()?;
    Some(DumpJoin {
        index: Some(gap.index),
        a_start_secs: Some(gap.geometry.a_start_secs),
        a_end_secs: Some(gap.geometry.a_end_secs),
        a_refined_start_secs: Some(gap.geometry.a_refined_start_secs),
        a_refined_end_secs: Some(gap.geometry.a_refined_end_secs),
        codec: corpus.source.a_source.codec.clone(),
    })
}

/// Scan boundaries as clip frames, plus the context they imply.
///
/// With a dump, the context is recovered from clip length and span (the `shoulder-sweep` § 2.1 rule),
/// snapped to the millisecond when within two samples of one, and the boundaries reproduce the
/// export's own arithmetic: the clip starts at `floor((a_start − ctx)·rate)` and production places
/// the scan edge at `trunc(a_start·rate)`. Without a dump, `--context-secs` sets both.
fn boundaries(
    frames: usize,
    rate: u32,
    dump: &DumpJoin,
    context_secs: f64,
) -> Result<(usize, usize, f64), String> {
    let r = f64::from(rate);
    let (Some(a_start), Some(a_end)) = (dump.a_start_secs, dump.a_end_secs) else {
        let ctx = (context_secs * r).round() as usize;
        if 2 * ctx >= frames {
            return Err("clip is shorter than its context".into());
        }
        return Ok((ctx, frames - ctx, context_secs));
    };
    let est = (frames as f64 / r - (a_end - a_start)) / 2.0;
    let snapped = (est * 1000.0).round() / 1000.0;
    let ctx = if ((snapped - est) * r).abs() <= 2.0 {
        snapped
    } else {
        est
    };
    if ctx <= 0.0 {
        return Err("clip is shorter than the gap it contains".into());
    }
    if a_start - ctx < 0.0 {
        return Err("clip clamped at media start; boundary not recoverable".into());
    }
    let clip_start = ((a_start - ctx) * r).floor() as usize;
    let lead = ((a_start * r) as usize).saturating_sub(clip_start);
    let trail = ((a_end * r) as usize).saturating_sub(clip_start);
    if trail > frames || lead >= trail {
        return Err("recovered boundaries fall outside the clip".into());
    }
    Ok((lead, trail, ctx))
}

/// Production's disposition for this stem, from `listen.json` beside the WAVs.
fn listen_disposition(dir: &Path, stem: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("listen.json")).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    doc.get("entries")?
        .as_array()?
        .iter()
        .find(|e| e.get("stem").and_then(|s| s.as_str()) == Some(stem))
        .and_then(|e| e.get("disposition"))
        .and_then(|d| d.as_str())
        .map(str::to_string)
}

/// The fill path for 0-based gap `index`, from the run log's status table (`#` is 1-based, prefixed
/// `-` / `>` when not repaired). On a patched row: `dual_fit` (`dual-fit a→b`), `anchor_seam`
/// (`anchor a→b` — the rescue that writes the wide A region by design) or `single_lag`;
/// `not_patched` otherwise. The last matching row wins, so a log with a retry table reports the
/// final pass.
fn fill_path_from_log(text: &str, index: usize) -> Option<&'static str> {
    let want = (index + 1).to_string();
    let mut found = None;
    for line in text.lines() {
        let mut tokens = line.split_whitespace();
        let (Some(num), Some(range)) = (tokens.next(), tokens.next()) else {
            continue;
        };
        if num.trim_start_matches(['-', '>']) != want || !range.contains(':') {
            continue;
        }
        let rest = line.to_ascii_lowercase();
        found = Some(
            if !rest.contains("patched") || rest.contains("not planned") {
                "not_patched"
            } else if rest.contains("dual-fit") || rest.contains("dual_fit") {
                "dual_fit"
            } else if rest.contains("(anchor") {
                "anchor_seam"
            } else {
                "single_lag"
            },
        );
    }
    found
}

/// Look for the run log from the gap directory up to the tree root: in each ancestor `d`, both `d/`
/// and the sibling `d-out/` (the `measure-gap-fingerprints.ps1` layout), as `<pair>.log` or
/// `<pair>_g<index>.log`.
fn find_fill_path(gap_dir: &Path, root: &Path, pair: &str, index: usize) -> Option<&'static str> {
    let names = [format!("{pair}.log"), format!("{pair}_g{index}.log")];
    let mut dir = gap_dir;
    loop {
        let mut candidates = vec![dir.to_path_buf()];
        if let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) {
            candidates.push(parent.join(format!("{}-out", name.to_string_lossy())));
        }
        for d in &candidates {
            for name in &names {
                if let Ok(text) = std::fs::read_to_string(d.join(name)) {
                    if let Some(path) = fill_path_from_log(&text, index) {
                        return Some(path);
                    }
                }
            }
        }
        if dir == root {
            return None;
        }
        dir = dir.parent()?;
    }
}

/// The pair label from the directory names between the tree root and the gap, nearest first: an
/// all-digit name, or the `<pair>` of a `<pair>_g<n>` name.
fn pair_label(components: &[String]) -> Option<String> {
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    components.iter().rev().find_map(|c| {
        if digits(c) {
            return Some(c.clone());
        }
        c.split_once("_g")
            .filter(|(p, g)| digits(p) && digits(g))
            .map(|(p, _)| p.to_string())
    })
}

/// Every `*_a_surround.wav` under `dir`, to [`MAX_DEPTH`] levels, sorted.
fn collect_gaps(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            if depth < MAX_DEPTH {
                collect_gaps(&p, depth + 1, out);
            }
        } else if p.to_string_lossy().ends_with(A_SUFFIX) {
            out.push(p);
        }
    }
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
// Census rows
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct Row {
    pair: String,
    /// The dump `index` — `7` for a `…_g007_full` stem, which is what `50/g7` means.
    gap_index: Option<usize>,
    edge: &'static str,
    /// The gap's directory relative to its tree, `/`-separated (`dual/10`, `50_g7/dual-xf`): the arm.
    rel_dir: String,
    tree: String,
    stem: String,
    codec: Option<String>,
    disposition: Option<String>,
    fill_path: Option<&'static str>,
    context_secs: f64,
    #[serde(flatten)]
    onset: OnsetReading,
    /// `true` when the far and near onsets are different falls — the § 3 room-tone shape, flagged,
    /// not resolved.
    onsets_disagree: bool,
    /// Production's refined edge from the dump, outward from the scan edge. A cross-check only.
    dump_refined_ms: Option<f64>,
    /// `floor_ms − dump_refined_ms`: ~0 when this bin's floor edge is production's refined edge.
    floor_vs_refined_ms: Option<f64>,
    /// Near onset to floor edge: the band refinement cannot remove.
    onset_near_to_floor_ms: Option<f64>,
    #[serde(flatten)]
    write: WriteReading,
    /// `floor_ms + cf − write_ms`: reads in `[0, write_bias_ms]` when the write is the floor edge
    /// less one crossfade.
    write_vs_predicted_ms: Option<f64>,
    /// Near onset to measured write: the exposed band, upper end (the measured write is late by up
    /// to the bias).
    exposure_hi_ms: Option<f64>,
    /// `exposure_hi_ms − write_bias_ms`: the estimated lower end.
    exposure_lo_ms: Option<f64>,
    /// `exposure_hi_ms − cf`: the hard lower end.
    exposure_lo_hard_ms: Option<f64>,
    /// Far onset to measured write, upper end — the room-tone reading of the same edge.
    exposure_far_hi_ms: Option<f64>,
}

struct Site<'a> {
    pair: &'a str,
    rel_dir: &'a str,
    tree: &'a str,
    stem: &'a str,
    disposition: Option<String>,
    fill_path: Option<&'static str>,
    context_secs: f64,
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn assemble_row(
    site: &Site<'_>,
    dump: &DumpJoin,
    edge: Edge,
    onset: OnsetReading,
    write: WriteReading,
    crossfade_ms: f64,
) -> Row {
    let dump_refined_ms = match edge {
        Edge::Lead => dump
            .a_start_secs
            .zip(dump.a_refined_start_secs)
            .map(|(s, r)| (s - r) * 1000.0),
        Edge::Trail => dump
            .a_end_secs
            .zip(dump.a_refined_end_secs)
            .map(|(e, r)| (r - e) * 1000.0),
    };
    let exposure_hi = onset.onset_near_ms.zip(write.write_ms).map(|(o, w)| o - w);
    let row = Row {
        pair: site.pair.to_string(),
        gap_index: dump.index,
        edge: edge.as_str(),
        rel_dir: site.rel_dir.to_string(),
        tree: site.tree.to_string(),
        stem: site.stem.to_string(),
        codec: dump.codec.clone(),
        disposition: site.disposition.clone(),
        fill_path: site.fill_path,
        context_secs: site.context_secs,
        onsets_disagree: onset.n_falls > 1 && onset.onset_far_ms != onset.onset_near_ms,
        dump_refined_ms,
        floor_vs_refined_ms: onset.floor_ms.zip(dump_refined_ms).map(|(f, r)| f - r),
        onset_near_to_floor_ms: onset.onset_near_ms.zip(onset.floor_ms).map(|(o, f)| o - f),
        write_vs_predicted_ms: onset
            .floor_ms
            .zip(write.write_ms)
            .map(|(f, w)| f + crossfade_ms - w),
        exposure_hi_ms: exposure_hi,
        exposure_lo_ms: exposure_hi.zip(write.write_bias_ms).map(|(h, b)| h - b),
        exposure_lo_hard_ms: exposure_hi.map(|h| h - crossfade_ms),
        exposure_far_hi_ms: onset.onset_far_ms.zip(write.write_ms).map(|(o, w)| o - w),
        onset,
        write,
    };
    round_row(row)
}

/// Two decimals on every millisecond / dB column — sample resolution at 48 kHz is 0.02 ms.
fn round_row(mut r: Row) -> Row {
    let o = &mut r.onset;
    for v in [
        &mut o.onset_far_ms,
        &mut o.onset_near_ms,
        &mut o.floor_ms,
        &mut o.band_peak_dbfs,
        &mut r.write.write_ms,
        &mut r.write.b_level_dbfs,
        &mut r.write.write_bias_ms,
        &mut r.dump_refined_ms,
        &mut r.floor_vs_refined_ms,
        &mut r.onset_near_to_floor_ms,
        &mut r.write_vs_predicted_ms,
        &mut r.exposure_hi_ms,
        &mut r.exposure_lo_ms,
        &mut r.exposure_lo_hard_ms,
        &mut r.exposure_far_hi_ms,
    ] {
        *v = v.map(r2);
    }
    o.window_ms = r2(o.window_ms);
    r
}

const CSV_FIELDS: &[&str] = &[
    "pair",
    "gap_index",
    "edge",
    "rel_dir",
    "codec",
    "disposition",
    "fill_path",
    "window_ms",
    "n_falls",
    "n_inside_floor",
    "onset_far_ms",
    "onset_near_ms",
    "onsets_disagree",
    "floor_ms",
    "dump_refined_ms",
    "floor_vs_refined_ms",
    "onset_near_to_floor_ms",
    "band_peak_dbfs",
    "band_above_floor",
    "write_ms",
    "b_level_dbfs",
    "write_bias_ms",
    "write_vs_predicted_ms",
    "exposure_hi_ms",
    "exposure_lo_ms",
    "exposure_lo_hard_ms",
    "exposure_far_hi_ms",
    "context_secs",
    "tree",
    "stem",
];

/// Fixed-column CSV of stems and numbers — no titles, so no quoting rules and no `csv` crate.
fn census_csv(rows: &[Row]) -> String {
    fn num(v: Option<f64>) -> String {
        v.map(|x| format!("{x:.2}")).unwrap_or_default()
    }
    fn opt<T: ToString>(v: Option<T>) -> String {
        v.map(|x| x.to_string()).unwrap_or_default()
    }
    let mut out = CSV_FIELDS.join(",");
    out.push('\n');
    for r in rows {
        let o = &r.onset;
        let w = &r.write;
        let cells = [
            r.pair.clone(),
            opt(r.gap_index),
            r.edge.to_string(),
            r.rel_dir.clone(),
            opt(r.codec.clone()),
            opt(r.disposition.clone()),
            opt(r.fill_path),
            format!("{:.2}", o.window_ms),
            o.n_falls.to_string(),
            o.n_inside_floor.to_string(),
            num(o.onset_far_ms),
            num(o.onset_near_ms),
            r.onsets_disagree.to_string(),
            num(o.floor_ms),
            num(r.dump_refined_ms),
            num(r.floor_vs_refined_ms),
            num(r.onset_near_to_floor_ms),
            num(o.band_peak_dbfs),
            opt(o.band_above_floor),
            num(w.write_ms),
            num(w.b_level_dbfs),
            num(w.write_bias_ms),
            num(r.write_vs_predicted_ms),
            num(r.exposure_hi_ms),
            num(r.exposure_lo_ms),
            num(r.exposure_lo_hard_ms),
            num(r.exposure_far_hi_ms),
            format!("{:.3}", r.context_secs),
            r.tree.clone(),
            r.stem.clone(),
        ];
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// `(numeric pair first, then gap index, stem, edge lead-first, rel_dir)`, so a stem's arms sit
/// together.
fn sort_rows(rows: &mut [Row]) {
    rows.sort_by(|x, y| {
        let key = |r: &Row| {
            let n = r.pair.parse::<u64>();
            (
                n.is_err(),
                n.unwrap_or(0),
                r.pair.clone(),
                r.gap_index.unwrap_or(usize::MAX),
                r.stem.clone(),
                r.edge != "lead",
                r.rel_dir.clone(),
            )
        };
        key(x).cmp(&key(y))
    });
}

// -------------------------------------------------------------------------------------------
// Modes
// -------------------------------------------------------------------------------------------

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("error: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let corpus = resolve(&args.corpus);
    let out_dir = args
        .out
        .as_ref()
        .map(|o| resolve(o))
        .unwrap_or_else(|| corpus.join("edge-onset"));
    let roots: Vec<PathBuf> = std::iter::once(corpus.clone())
        .chain(args.also.iter().map(|d| resolve(d)))
        .collect();

    let mut rows: Vec<Row> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut total = 0usize;
    for root in &roots {
        if !root.is_dir() {
            eprintln!("skip missing dir: {}", root.display());
            continue;
        }
        let tree = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut found = Vec::new();
        collect_gaps(root, 0, &mut found);
        for a_wav in found {
            total += 1;
            let stem = stem_of(&a_wav);
            match measure_gap(args, root, &tree, roots.len() > 1, &a_wav) {
                Ok(mut gap_rows) => rows.append(&mut gap_rows),
                Err(why) => skipped.push(format!("{stem}: {why}")),
            }
        }
    }
    if total == 0 {
        return Err(format!("no {A_SUFFIX} under {}", corpus.display()));
    }
    sort_rows(&mut rows);
    print_table(&rows);

    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let census = out_dir.join("census.csv");
    std::fs::write(&census, census_csv(&rows)).map_err(|e| format!("{}: {e}", census.display()))?;
    let json = out_dir.join("census.json");
    let payload = serde_json::json!({
        "_contract": {
            "instrument": "edge-onset (docs/dev/TEMP-soft-edge-census-plan.md § 3)",
            "distances": "milliseconds OUTWARD from the scan boundary into programme: before it on `lead`, after it on `trail`",
            "onset": format!("{BIN_MS} ms peak bins over {WINDOW_MS} ms; a >= {FALL_DB} dB fall (lead) / rise (trail) sustained to the boundary; refined to the sample"),
            "onset_far_vs_near": format!("far = earliest lead fall / latest trail rise; near = closest to the boundary; onsets within {REF_BINS} bins merge to the outermost. They disagree on the room-tone shape; flagged, not resolved"),
            "n_inside_floor": "falls nearer the boundary than the floor edge (inside the silent run); counted, never an onset",
            "floor": format!("first non-silent frame outward of the boundary, any channel |x| >= {FLOOR_AMP}; this bin's own comparison"),
            "write": "first (lead) / one past last (trail) frame where the patched clip differs from the surround clip by >= 1 LSB",
            "write_bias_ms": format!("cf*(2/pi)*asin(LSB/b_rms) with cf = --crossfade-ms {}; an estimate (B oscillates), hard bound cf", args.crossfade_ms),
            "dump_refined_ms": "production's refined edge from the dump, joined as a cross-check — never an input",
            "arms": "a stem in several trees/dirs is kept in each: raw A is shared, the write is not",
            "denominator": "gap edges with listen WAVs, NOT gaps in the corpus — nothing in this file is a base rate",
        },
        "n": rows.len(),
        "edges": rows,
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

/// `prefix_tree`: several trees were given, so `rel_dir` leads with the tree name (`dual/10`) — each
/// tree is an arm, and the arm must survive into every printed label.
fn measure_gap(
    args: &Args,
    root: &Path,
    tree: &str,
    prefix_tree: bool,
    a_wav: &Path,
) -> Result<Vec<Row>, String> {
    let stem = stem_of(a_wav);
    let dir = a_wav.parent().ok_or("no parent directory")?;
    let components: Vec<String> = dir
        .strip_prefix(root)
        .unwrap_or(Path::new(""))
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let rel_dir = match (prefix_tree, components.is_empty()) {
        (true, _) => std::iter::once(tree.to_string())
            .chain(components.iter().cloned())
            .collect::<Vec<_>>()
            .join("/"),
        (false, true) => ".".to_string(),
        (false, false) => components.join("/"),
    };
    let root_name = vec![tree.to_string()];
    let pair = pair_label(&components)
        .or_else(|| pair_label(&root_name))
        .or_else(|| args.pair.clone())
        .unwrap_or_else(|| ".".to_string());

    let dump = read_dump(&dir.join(format!("{stem}.json"))).unwrap_or_default();
    let a = read_wav(a_wav)?;
    let (lead, trail, ctx) = boundaries(a.frames(), a.rate, &dump, args.context_secs)?;

    let patched_path = dir.join(format!("{stem}{PATCHED_SUFFIX}"));
    let span = if patched_path.exists() {
        diff_span(&a, &read_wav(&patched_path)?)?
    } else {
        None
    };
    let b_path = dir.join(format!("{stem}{B_SUFFIX}"));
    let b = if span.is_some() && b_path.exists() {
        Some(read_wav(&b_path)?)
    } else {
        None
    };
    if let Some(b) = &b {
        if b.rate != a.rate {
            return Err(format!(
                "sample rates differ (A {}, B {}); resample first",
                a.rate, b.rate
            ));
        }
    }

    let site = Site {
        pair: &pair,
        rel_dir: &rel_dir,
        tree,
        stem: &stem,
        disposition: listen_disposition(dir, &stem),
        fill_path: dump.index.and_then(|i| find_fill_path(dir, root, &pair, i)),
        context_secs: ctx,
    };
    Ok([(Edge::Lead, lead), (Edge::Trail, trail)]
        .into_iter()
        .map(|(edge, boundary)| {
            assemble_row(
                &site,
                &dump,
                edge,
                measure_onset(&a, boundary, edge),
                measure_write(&a, span, b.as_ref(), boundary, edge, args.crossfade_ms),
                args.crossfade_ms,
            )
        })
        .collect())
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1}")).unwrap_or_else(|| "-".into())
}

fn print_table(rows: &[Row]) {
    let pw = rows.iter().map(|r| r.pair.len()).max().unwrap_or(4).max(4);
    let dw = rows
        .iter()
        .map(|r| r.rel_dir.len())
        .max()
        .unwrap_or(3)
        .max(3);
    println!(
        "{:>pw$} {:>5} {:<5} {:<dw$} {:>7} {:>7} {:>6} {:>7} {:>7} {:>5} {:>7} {:<11}",
        "pair",
        "gap",
        "edge",
        "dir",
        "near",
        "far",
        "floor",
        "refine",
        "write",
        "bias",
        "expo_hi",
        "fill"
    );
    for r in rows {
        println!(
            "{:>pw$} {:>5} {:<5} {:<dw$} {:>7} {:>7} {:>6} {:>7} {:>7} {:>5} {:>7} {:<11}",
            r.pair,
            r.gap_index
                .map(|i| format!("g{i}"))
                .unwrap_or_else(|| "?".into()),
            r.edge,
            r.rel_dir,
            fmt_opt(r.onset.onset_near_ms),
            fmt_opt(r.onset.onset_far_ms),
            fmt_opt(r.onset.floor_ms),
            fmt_opt(r.dump_refined_ms),
            fmt_opt(r.write.write_ms),
            fmt_opt(r.write.write_bias_ms),
            fmt_opt(r.exposure_hi_ms),
            r.fill_path.unwrap_or("-")
        );
    }
}

fn gap_label(r: &Row) -> String {
    format!(
        "{}/{} {} ({})",
        r.pair,
        r.gap_index
            .map(|i| format!("g{i}"))
            .unwrap_or_else(|| r.stem.clone()),
        r.edge,
        r.rel_dir
    )
}

/// Counts and the Phase 1 checks (§ 4) as listings. No verdicts.
fn print_summary(rows: &[Row], total: usize, skipped: &[String]) {
    println!(
        "\n{} edge(s) from {} gap clip set(s), {} skipped",
        rows.len(),
        total,
        skipped.len()
    );
    for line in skipped {
        println!("  skipped {line}");
    }
    let with_onset = rows
        .iter()
        .filter(|r| r.onset.onset_near_ms.is_some())
        .count();
    let disagree = rows.iter().filter(|r| r.onsets_disagree).count();
    println!(
        "{with_onset} edge(s) with an onset; {disagree} with far ≠ near (room-tone shape, flagged)"
    );

    // Raw A is shared across arms: the onset reading must be identical wherever a stem repeats.
    let mut by_edge: BTreeMap<(String, &str), Vec<&Row>> = BTreeMap::new();
    for r in rows {
        by_edge.entry((r.stem.clone(), r.edge)).or_default().push(r);
    }
    let repeated: Vec<&Vec<&Row>> = by_edge.values().filter(|v| v.len() > 1).collect();
    let inconsistent: Vec<&&Vec<&Row>> = repeated
        .iter()
        .filter(|v| v.iter().any(|r| r.onset != v[0].onset))
        .collect();
    println!(
        "raw-A consistency: {} edge(s) measured in more than one dir; {} differ on onset/floor",
        repeated.len(),
        inconsistent.len()
    );
    for v in inconsistent {
        println!("  differs: {}", gap_label(v[0]));
    }

    let mismatched: Vec<&Row> = rows
        .iter()
        .filter(|r| r.floor_vs_refined_ms.is_some_and(|d| d.abs() > 1.0))
        .collect();
    println!(
        "floor edge vs production's refined edge: {} edge(s) more than 1 ms apart",
        mismatched.len()
    );
    for r in mismatched {
        println!(
            "  {}: floor {} vs refined {}",
            gap_label(r),
            fmt_opt(r.onset.floor_ms),
            fmt_opt(r.dump_refined_ms)
        );
    }

    let written: Vec<&Row> = rows.iter().filter(|r| r.write.write_ms.is_some()).collect();
    let tight = written
        .iter()
        .filter(|r| r.write.write_bias_ms.is_some_and(|b| b <= 1.0))
        .count();
    println!(
        "write_bias_ms <= 1 ms on {tight} of {} written edge(s)",
        written.len()
    );
    let (wide, off): (Vec<&Row>, Vec<&Row>) = written
        .iter()
        .copied()
        .filter(|r| {
            r.write_vs_predicted_ms
                .zip(r.write.write_bias_ms)
                .is_some_and(|(d, b)| d < -1.0 || d > b + 1.0)
        })
        .partition(|r| {
            r.fill_path == Some("anchor_seam")
                && r.write_vs_predicted_ms.is_some_and(|d| d < -WIDE_WRITE_MS)
        });
    println!(
        "write_vs_predicted_ms outside [-1, bias + 1] on {} edge(s):",
        wide.len() + off.len()
    );
    println!(
        "  {} anchor-seam wide writes (> {WIDE_WRITE_MS} ms past the prediction, over programme — \
         the rescue's A region, by design; the band is covered):",
        wide.len()
    );
    for r in &wide {
        println!(
            "    {}: {} ms (fill {})",
            gap_label(r),
            fmt_opt(r.write_vs_predicted_ms),
            r.fill_path.unwrap_or("-")
        );
    }
    println!("  {} other — each needs a named cause:", off.len());
    for r in off {
        println!(
            "    {}: {} ms (bias {}, fill {})",
            gap_label(r),
            fmt_opt(r.write_vs_predicted_ms),
            fmt_opt(r.write.write_bias_ms),
            r.fill_path.unwrap_or("-")
        );
    }

    // Arms of one stem: does the write move between them? Within one sample is the same write.
    let tol = SAME_WRITE_MS;
    let mut arm_diffs = 0usize;
    for v in &repeated {
        let writes: Vec<Option<f64>> = v.iter().map(|r| r.write.write_ms).collect();
        let moved = writes.windows(2).any(|w| match (w[0], w[1]) {
            (Some(x), Some(y)) => (x - y).abs() > tol,
            (x, y) => x != y,
        });
        if moved {
            arm_diffs += 1;
            let parts: Vec<String> = v
                .iter()
                .map(|r| {
                    format!(
                        "{} {} ({})",
                        r.rel_dir,
                        fmt_opt(r.write.write_ms),
                        r.fill_path.unwrap_or("-")
                    )
                })
                .collect();
            println!(
                "  write moves across dirs: {}/{} {}: {}",
                v[0].pair,
                v[0].gap_index.map(|i| format!("g{i}")).unwrap_or_default(),
                v[0].edge,
                parts.join(", ")
            );
        }
    }
    println!(
        "{arm_diffs} of {} repeated edge(s) write from a different frame across dirs",
        repeated.len()
    );
    println!(
        "  denominator is gap edges with listen WAVs, not gaps in the corpus — this is not a base rate"
    );
}

// -------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;
    const LSB16: f64 = 1.0 / 32768.0;

    fn ms(v: f64) -> usize {
        (v / 1000.0 * f64::from(SR)).round() as usize
    }

    /// Stereo WAV from a per-frame mono function, both channels equal.
    fn wav(frames: usize, f: impl Fn(usize) -> f64) -> Wav {
        Wav {
            channels: 2,
            rate: SR,
            lsb: LSB16,
            samples: (0..frames).flat_map(|i| [f(i), f(i)]).collect(),
        }
    }

    fn sine(i: usize, amp: f64, hz: f64) -> f64 {
        amp * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(SR)).sin()
    }

    fn quantize(x: f64) -> f64 {
        (x / LSB16).round() * LSB16
    }

    /// 50/g7's shape: programme, a fast fall to a −50 dBFS band 25.3 ms before the boundary, then
    /// silence from the boundary. The band is above the floor, so the floor edge is the boundary.
    #[test]
    fn soft_lead_band_reports_onset_before_the_floor_edge() {
        let boundary = ms(500.0);
        let onset = boundary - ms(25.3);
        let a = wav(ms(1000.0), |i| match i {
            i if i < onset => sine(i, 0.1, 440.0),
            i if i < boundary => sine(i, 0.003, 1000.0),
            _ => 0.0,
        });
        let r = measure_onset(&a, boundary, Edge::Lead);
        let near = r.onset_near_ms.expect("an onset");
        assert!((near - 25.3).abs() < 1.0, "{r:?}");
        assert_eq!(r.onset_far_ms, r.onset_near_ms, "{r:?}");
        assert!(r.floor_ms.expect("a floor edge") < 1.0, "{r:?}");
        assert_eq!(r.band_above_floor, Some(true));
        let peak = r.band_peak_dbfs.unwrap();
        assert!((peak - db(0.003)).abs() < 0.5, "{peak}");
    }

    /// 50/g7's real trace: the fall spreads over the bin that straddles it (−22 → −30 → −45 dBFS)
    /// and the band's own peaks sit near the straddling bin's −15 dB level. A one-bin reference
    /// misses it; the [`REF_BINS`] reference does not.
    #[test]
    fn fall_straddling_a_bin_edge_still_reads_as_one_onset() {
        let boundary = ms(500.0);
        let bin = ms(BIN_MS);
        let onset = boundary - ms(25.0) - bin / 2;
        let a = wav(ms(1000.0), |i| match i {
            i if i < onset => sine(i, 0.08, 440.0),
            i if i < onset + bin => sine(i, 0.03, 440.0),
            i if i < boundary => sine(i, 0.0057, 1000.0),
            _ => 0.0,
        });
        let r = measure_onset(&a, boundary, Edge::Lead);
        let near = r.onset_near_ms.expect("an onset");
        assert!((near - 25.0).abs() < 2.5, "{r:?}");
        assert_eq!(r.n_falls, 1, "{r:?}");
        assert_eq!(r.band_above_floor, Some(true));
    }

    /// 83/g7's shape: the dropout falls to a sub-floor tail 90 ms out, which then steps to digital
    /// zero 65 ms out. The second fall is inside the silent run, so it is counted apart and the
    /// onset is the first.
    #[test]
    fn fall_inside_the_silent_run_is_not_an_onset() {
        let boundary = ms(500.0);
        let a = wav(ms(1000.0), |i| match i {
            i if i < boundary - ms(90.0) => sine(i, 0.1, 440.0),
            i if i < boundary - ms(65.0) => sine(i, 0.0003, 1000.0),
            _ => 0.0,
        });
        let r = measure_onset(&a, boundary, Edge::Lead);
        assert_eq!((r.n_falls, r.n_inside_floor), (1, 1), "{r:?}");
        assert!((r.onset_near_ms.unwrap() - 90.0).abs() < 1.0, "{r:?}");
        assert_eq!(r.onset_far_ms, r.onset_near_ms);
        assert!((r.floor_ms.unwrap() - 90.0).abs() < 1.0, "{r:?}");
    }

    #[test]
    fn merge_keeps_the_outermost_of_each_close_run() {
        assert_eq!(
            merge_onsets(vec![1243, 1142, 7200, 480], 192),
            vec![480, 1243, 7200]
        );
        assert_eq!(merge_onsets(vec![], 192), Vec::<usize>::new());
    }

    /// The known false-positive shape: a word ends 150 ms out, room tone runs to 10 ms out, then the
    /// dropout. Far finds the word's end; near finds the dropout.
    #[test]
    fn room_tone_after_a_word_splits_far_from_near() {
        let boundary = ms(500.0);
        let word_end = boundary - ms(150.0);
        let drop = boundary - ms(10.0);
        let a = wav(ms(1000.0), |i| match i {
            i if i < word_end => sine(i, 0.1, 440.0),
            i if i < drop => sine(i, 0.003, 700.0),
            _ => 0.0,
        });
        let r = measure_onset(&a, boundary, Edge::Lead);
        assert!((r.onset_far_ms.unwrap() - 150.0).abs() < 1.0, "{r:?}");
        assert!((r.onset_near_ms.unwrap() - 10.0).abs() < 1.0, "{r:?}");
        assert!(r.n_falls >= 2);
    }

    /// A dip that comes back up is not sustained to the boundary; a hard edge has no fall at all.
    #[test]
    fn recovering_dip_and_hard_edge_report_no_onset() {
        let boundary = ms(500.0);
        let dip = boundary - ms(100.0);
        let a = wav(ms(1000.0), |i| match i {
            i if (dip..dip + ms(6.0)).contains(&i) => 0.0,
            i if i < boundary => sine(i, 0.1, 440.0),
            _ => 0.0,
        });
        let r = measure_onset(&a, boundary, Edge::Lead);
        assert_eq!(r.onset_near_ms, None, "{r:?}");
        assert_eq!(r.n_falls, 0);
        assert!(r.floor_ms.unwrap() < 1.0, "{r:?}");
    }

    /// Bucket-edge band: below the floor throughout, so the floor edge sits at the onset and nothing
    /// is left for refinement to miss.
    #[test]
    fn sub_floor_band_puts_the_floor_edge_at_the_onset() {
        let boundary = ms(500.0);
        let onset = boundary - ms(40.0);
        let a = wav(ms(1000.0), |i| match i {
            i if i < onset => sine(i, 0.1, 440.0),
            i if i < boundary => sine(i, 0.0003, 1000.0),
            _ => 0.0,
        });
        let r = measure_onset(&a, boundary, Edge::Lead);
        let near = r.onset_near_ms.unwrap();
        let floor = r.floor_ms.unwrap();
        assert!((near - 40.0).abs() < 1.0, "{r:?}");
        assert!((near - floor).abs() < 1.0, "{r:?}");
        assert_eq!(r.band_above_floor, Some(false));
    }

    /// The trail mirror: a −50 dBFS pre-echo for 20 ms after the boundary, then programme.
    #[test]
    fn soft_trail_band_reports_the_rise() {
        let boundary = ms(500.0);
        let rise = boundary + ms(20.0);
        let a = wav(ms(1000.0), |i| match i {
            i if i < boundary => 0.0,
            i if i < rise => sine(i, 0.003, 1000.0),
            _ => sine(i, 0.1, 440.0),
        });
        let r = measure_onset(&a, boundary, Edge::Trail);
        assert!((r.onset_near_ms.unwrap() - 20.0).abs() < 1.0, "{r:?}");
        assert_eq!(r.onset_far_ms, r.onset_near_ms);
        assert!(r.floor_ms.unwrap() < 1.0, "{r:?}");
        assert_eq!(r.band_above_floor, Some(true));
    }

    /// The window clips at the clip edge rather than reading out of bounds.
    #[test]
    fn window_clips_at_the_clip_start() {
        let a = wav(ms(300.0), |i| sine(i, 0.1, 440.0));
        let r = measure_onset(&a, ms(50.0), Edge::Lead);
        assert!((r.window_ms - 50.0).abs() < 0.1, "{r:?}");
    }

    /// A quantized equal-power crossfade from A (a quiet band) to B starting at a known frame:
    /// `write_first_diff` never precedes the ramp, stays inside it, and lands within the bias
    /// estimate plus one B period of its start.
    fn crossfade_case(b_amp: f64) -> (usize, usize, f64) {
        let n = ms(400.0);
        let ramp = ms(200.0);
        let cf = ms(10.0);
        let a_f = |i: usize| sine(i, 0.003, 1000.0);
        let b_f = |i: usize| sine(i, b_amp, 440.0);
        let surround = wav(n, |i| quantize(a_f(i)));
        let b = wav(n, |i| quantize(b_f(i)));
        let patched = wav(n, |i| {
            let x = if i < ramp {
                a_f(i)
            } else if i < ramp + cf {
                let t = (i - ramp) as f64 / cf as f64 * std::f64::consts::FRAC_PI_2;
                t.cos() * a_f(i) + t.sin() * b_f(i)
            } else {
                b_f(i)
            };
            quantize(x)
        });
        let span = diff_span(&surround, &patched).unwrap();
        let (first, _) = span.expect("a write");
        let w = measure_write(&surround, span, Some(&b), ramp + cf, Edge::Lead, 10.0);
        (first, ramp, w.write_bias_ms.unwrap())
    }

    #[test]
    fn first_diff_sits_within_the_bias_estimate_of_the_ramp_start() {
        let period = f64::from(SR) / 440.0;
        for b_amp in [0.1, 0.000_05] {
            let (first, ramp, bias_ms) = crossfade_case(b_amp);
            let late = (first - ramp) as f64;
            assert!(first >= ramp, "b {b_amp}: first {first} before ramp {ramp}");
            assert!(
                late < ms(10.0) as f64,
                "b {b_amp}: {late} frames, past the crossfade"
            );
            let bound = bias_ms / 1000.0 * f64::from(SR) + period;
            assert!(
                late <= bound,
                "b {b_amp}: {late} frames late, bound {bound}"
            );
        }
        let (_, _, loud) = crossfade_case(0.1);
        let (_, _, quiet) = crossfade_case(0.000_05);
        assert!(loud < 0.01, "{loud}");
        assert!(quiet > 3.0, "{quiet}");
    }

    #[test]
    fn bias_is_the_full_crossfade_when_b_is_silent() {
        assert_eq!(write_bias_ms(LSB16, None, 10.0), 10.0);
        assert_eq!(write_bias_ms(LSB16, Some(0.0), 10.0), 10.0);
        assert_eq!(write_bias_ms(LSB16, Some(LSB16 / 2.0), 10.0), 10.0);
    }

    #[test]
    fn untouched_clip_has_no_write() {
        let a = wav(1000, |i| quantize(sine(i, 0.1, 440.0)));
        assert_eq!(diff_span(&a, &a.clone()).unwrap(), None);
        let w = measure_write(&a, None, None, 500, Edge::Lead, 10.0);
        assert_eq!(w, WriteReading::default());
    }

    #[test]
    fn row_derives_exposure_and_prediction_from_the_readings() {
        let site = Site {
            pair: "50",
            rel_dir: "50_g7/dual-xf",
            tree: "run",
            stem: "s",
            disposition: None,
            fill_path: Some("dual_fit"),
            context_secs: 3.0,
        };
        let dump = DumpJoin {
            index: Some(7),
            a_start_secs: Some(10.0),
            a_refined_start_secs: Some(10.0),
            ..DumpJoin::default()
        };
        let onset = OnsetReading {
            onset_near_ms: Some(26.0),
            onset_far_ms: Some(26.0),
            n_falls: 1,
            floor_ms: Some(0.0),
            ..OnsetReading::default()
        };
        let write = WriteReading {
            write_ms: Some(10.0),
            b_level_dbfs: Some(-20.0),
            write_bias_ms: Some(0.5),
        };
        let r = assemble_row(&site, &dump, Edge::Lead, onset, write, 10.0);
        assert_eq!(r.exposure_hi_ms, Some(16.0));
        assert_eq!(r.exposure_lo_ms, Some(15.5));
        assert_eq!(r.exposure_lo_hard_ms, Some(6.0));
        assert_eq!(r.write_vs_predicted_ms, Some(0.0));
        assert_eq!(r.dump_refined_ms, Some(0.0));
        assert_eq!(r.floor_vs_refined_ms, Some(0.0));
        assert!(!r.onsets_disagree);
    }

    #[test]
    fn boundaries_reproduce_the_export_arithmetic() {
        let r = f64::from(SR);
        let (a_start, a_end, ctx) = (2176.0751458333334, 2178.627916666667, 3.0);
        let clip_start = ((a_start - ctx) * r).floor() as usize;
        let frames = ((a_end + ctx) * r).ceil() as usize - clip_start;
        let dump = DumpJoin {
            a_start_secs: Some(a_start),
            a_end_secs: Some(a_end),
            ..DumpJoin::default()
        };
        let (lead, trail, got_ctx) = boundaries(frames, SR, &dump, 99.0).unwrap();
        assert_eq!(got_ctx, 3.0);
        assert_eq!(lead, (a_start * r) as usize - clip_start);
        assert_eq!(trail, (a_end * r) as usize - clip_start);
        let none = boundaries(frames, SR, &DumpJoin::default(), 3.0).unwrap();
        assert_eq!(none.0, ms(3000.0));
    }

    #[test]
    fn clamped_clip_is_refused() {
        let dump = DumpJoin {
            a_start_secs: Some(1.0),
            a_end_secs: Some(2.0),
            ..DumpJoin::default()
        };
        // Clamped at media start: 1 s + 1 s gap + 3 s, so the symmetric recovery says ctx = 2 s > 1 s.
        assert!(boundaries(ms(5000.0), SR, &dump, 3.0).is_err());
    }

    #[test]
    fn log_status_rows_give_the_fill_path() {
        let log = "Gap scan: 13 silent run(s) ≥500ms — block 100ms\n\
                   \x20 #   Range                Dur      Status\n\
                   \x20 -2  14:14 – 14:15        0.6s     not planned: gap not selected\n\
                   \x20 3   0:10 – 0:11          0.6s     patched (0.91/0.88) [high]\n\
                   \x20 9   1:44:22 – 1:44:23    1.8s     patched (dual-fit 0.95→0.96) [high · balanced]\n\
                   \x20 11  1:46:12 – 1:46:14    1.9s     ! patched (anchor 0.31→0.34) [marginal · balanced]\n";
        assert_eq!(fill_path_from_log(log, 10), Some("anchor_seam"));
        assert_eq!(fill_path_from_log(log, 8), Some("dual_fit"));
        assert_eq!(fill_path_from_log(log, 2), Some("single_lag"));
        assert_eq!(fill_path_from_log(log, 1), Some("not_patched"));
        assert_eq!(fill_path_from_log(log, 12), None);
    }

    #[test]
    fn pair_label_reads_both_listen_layouts() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(pair_label(&s(&["dual", "10"])), Some("10".into()));
        assert_eq!(pair_label(&s(&["50_g7", "dual-xf"])), Some("50".into()));
        assert_eq!(pair_label(&s(&["dual-xf"])), None);
    }
}
