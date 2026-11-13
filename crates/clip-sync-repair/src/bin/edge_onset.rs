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
//! **Specification.** `docs/dev/archive/TEMP-soft-edge-census-plan.md` § 3 is this bin's contract — the
//! onset rule, the floor edge, the write-start interval and its bias estimate, and the columns. Read
//! it before changing any number here.
//!
//! **Two modes.** Listen mode (`--corpus`, Phase 1) reads `--gap-listen` WAVs and also measures the
//! write. Media mode (`--report`, Phase 2) reads one `--patch-only --format json` report per pair,
//! decodes that pair's A with the scanner's own sequential bucket decode, keeps only ±250 ms around
//! each scan edge, and measures **every** gap. It has no write to measure, so it reports the § 4
//! exposure estimate instead. No B, no oracle, no full-PCM hold.
//!
//! **Usage.**
//!
//! ```text
//! cargo run --features calibration --bin edge-onset -- --corpus gap-files/<listen-run>
//! cargo run --features calibration --bin edge-onset -- \
//!     --corpus gap-files/<listen-run>/dual --also gap-files/<listen-run>/single
//! cargo run --release --features calibration,he-aac --bin edge-onset -- \
//!     --report gap-files/<census-run>/50.json --out gap-files/<census-run>/edge-onset
//! ```
//!
//! Media mode needs `he-aac` even for AAC-LC: it registers the only AAC decoder. Reports are decoded
//! one at a time, in order — never in parallel.
//!
//! Media hygiene: prints pair labels, gap indices, relative directories and file stems only — never
//! media paths or titles. A report's `video_a` path is opened but never printed; decode errors are
//! scrubbed of it. Relative `--corpus` / `--also` / `--report` / `--out` resolve against the repo
//! root.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use serde::{Deserialize, Serialize};

use clip_sync::{
    select_best_track, MediaReader, MediaSession, MediaSource, ProgressMode, ProgressReporter,
    StderrProgressReporter, SymphoniaMediaReader,
};
use clip_sync_repair::application::gap_fingerprint::GapCorpus;
use clip_sync_repair::domain::policies::{snap_gap_edges_to_onset, EdgeSnap, RefinedGapFrames};

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
/// Media mode keeps this much either side of each scan edge: the onset window plus margin.
const MEDIA_HALF_MS: f64 = 250.0;
/// Media mode's decode bucket. Any length works; only frame positions matter.
const MEDIA_BUCKET_SECS: f64 = 10.0;

#[derive(Parser, Debug)]
#[command(
    about = "Soft-edge census: dropout onset, floor edge and write start (listen clips) or exposure estimate (media) per gap edge"
)]
struct Args {
    /// Listen mode: a `--gap-listen` tree, any directory holding `*_a_surround.wav` up to three
    /// levels down.
    #[arg(long, required_unless_present = "report", conflicts_with = "report")]
    corpus: Option<PathBuf>,

    /// Media mode: a pair's `--patch-only --format json` report. Repeat for several pairs; each is
    /// decoded in turn. The pair label comes from the file or directory name (`50.json`, `50/…`),
    /// else `--pair` when only one report is given.
    #[arg(long)]
    report: Vec<PathBuf>,

    /// Media mode, one report only: open this A instead of the report's `video_a` (the corpus moved).
    #[arg(long)]
    media_a: Option<PathBuf>,

    /// Extra trees measured into the same census. Unlike `shoulder-sweep`, a stem seen in two trees
    /// is **kept** in both: two arms of one listen run share raw A but not the write, and comparing
    /// their writes is the point.
    #[arg(long)]
    also: Vec<PathBuf>,

    /// Pair label for rows the directory tree cannot name. Fills in; never overrides.
    #[arg(long)]
    pair: Option<String>,

    /// Where to write `census.csv` / `census.json`. Default `<corpus>/edge-onset/`, or
    /// `edge-onset/` beside the first report.
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

    /// Media mode, diagnostic: also run the production `snap_gap_edges_to_onset` at this cap (ms)
    /// on each edge window, with the scan boundary as the refined edge, and print one stderr line
    /// per edge where either reading has an onset. A cross-check of the Phase 4 port against this
    /// bin's reading; it changes no output file.
    #[arg(long)]
    snap_cross_check_ms: Option<f64>,

    /// Media mode, diagnostic: write `frame-grid.csv` to the out dir — per edge, the exact-zero
    /// run's edge on the decoded timeline (mod 1024 / 2048) and the envelope of the 1024 frames
    /// beside it against the AAC window halves. Tests whether lead bands are the decoder's
    /// overlap-add tail. Changes no other output.
    #[arg(long)]
    frame_grid: bool,

    /// `--frame-grid` only: the amplitude at or below which a frame counts as silent (every channel).
    #[arg(long, default_value_t = 0.0)]
    grid_zero_amp: f64,
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
    /// The refined edges before the edge snap, on dumps that record them.
    a_unsnapped_start_secs: Option<f64>,
    a_unsnapped_end_secs: Option<f64>,
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
        a_unsnapped_start_secs: gap.geometry.a_unsnapped_start_secs,
        a_unsnapped_end_secs: gap.geometry.a_unsnapped_end_secs,
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
    /// Production's refined edge from the dump (before the edge snap, when the dump records it),
    /// outward from the scan edge. A cross-check only.
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
    /// Media mode, patched edges: the write's move **outward** from the refined edge, from the
    /// report's adjust field (`−gap_start_adjust_frames` lead, `+gap_end_adjust_frames` trail).
    gap_adjust_ms: Option<f64>,
    /// Media mode: the edge snap's outward move from the report's `edge_snap.*.moved_frames`. Present
    /// on every edge of a report that carries a snap record (0 when the edge did not snap).
    snap_moved_ms: Option<f64>,
    /// Media mode: `onset_near_to_floor_ms − cf − gap_adjust_ms − snap_moved_ms`, the § 4 estimate
    /// less the band the snap already wrote over. Patched, non-anchor-seam edges with an onset
    /// only — an anchor-seam write's side is not on the record.
    exposure_est_ms: Option<f64>,
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
    // This bin's floor edge is the refine's edge, so compare it with the edge BEFORE the snap: a
    // snapped dump's `a_refined_*` already sits at the onset.
    let dump_refined_ms = match edge {
        Edge::Lead => dump
            .a_start_secs
            .zip(dump.a_unsnapped_start_secs.or(dump.a_refined_start_secs))
            .map(|(s, r)| (s - r) * 1000.0),
        Edge::Trail => dump
            .a_end_secs
            .zip(dump.a_unsnapped_end_secs.or(dump.a_refined_end_secs))
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
        gap_adjust_ms: None,
        snap_moved_ms: None,
        exposure_est_ms: None,
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
        &mut r.gap_adjust_ms,
        &mut r.snap_moved_ms,
        &mut r.exposure_est_ms,
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
    "gap_adjust_ms",
    "snap_moved_ms",
    "exposure_est_ms",
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
            num(r.gap_adjust_ms),
            num(r.snap_moved_ms),
            num(r.exposure_est_ms),
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
    match &args.corpus {
        Some(corpus) => run_listen(args, corpus),
        None => run_media(args),
    }
}

/// `census.csv` and `census.json` (rows under `edges`, `contract` as `_contract`) into `out_dir`.
fn write_census(
    out_dir: &Path,
    rows: &[Row],
    contract: serde_json::Value,
) -> Result<(PathBuf, PathBuf), String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let census = out_dir.join("census.csv");
    std::fs::write(&census, census_csv(rows)).map_err(|e| format!("{}: {e}", census.display()))?;
    let json = out_dir.join("census.json");
    let payload = serde_json::json!({
        "_contract": contract,
        "n": rows.len(),
        "edges": rows,
    });
    std::fs::write(
        &json,
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{}: {e}", json.display()))?;
    Ok((census, json))
}

fn run_listen(args: &Args, corpus: &Path) -> Result<(), String> {
    let corpus = resolve(corpus);
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

    let contract = serde_json::json!({
            "instrument": "edge-onset listen mode (docs/dev/archive/TEMP-soft-edge-census-plan.md § 3)",
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
    });
    let (census, json) = write_census(&out_dir, &rows, contract)?;

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
// Media mode (Phase 2)
// -------------------------------------------------------------------------------------------

/// The slice of a `--patch-only --format json` report media mode reads. `video_a` is opened, never
/// printed.
#[derive(Deserialize)]
struct PatchReportJson {
    scan: ScanJson,
    patch: Option<PatchJson>,
}

#[derive(Deserialize)]
struct ScanJson {
    video_a: PathBuf,
    gaps: Vec<ScanGapJson>,
}

#[derive(Deserialize)]
struct ScanGapJson {
    video_a_start_secs: f64,
    video_a_end_secs: f64,
}

#[derive(Deserialize)]
struct PatchJson {
    gaps: Vec<PatchGapJson>,
}

#[derive(Deserialize)]
struct PatchGapJson {
    a_start_secs: f64,
    a_end_secs: f64,
    /// Externally tagged `GapPatchStatus`, read as a value: only four fields of one arm are needed.
    status: serde_json::Value,
    /// The per-gap snap record, read as a value: only each edge's `moved_frames` is needed. Absent
    /// on reports from before the snap, or with it off.
    #[serde(default)]
    edge_snap: Option<serde_json::Value>,
}

/// One gap's patch outcome, from its status.
#[derive(Debug, Clone, PartialEq)]
struct Outcome {
    /// The status tag: `patched`, `skipped`, `not_planned`, `not_applied`.
    disposition: String,
    /// `dual_fit`, `anchor_seam`, `single_lag` or `not_patched` — the listen-mode vocabulary.
    fill_path: &'static str,
    start_adjust_frames: i64,
    end_adjust_frames: i64,
    /// The snap's outward move per edge (`edge_snap.{lead,trail}.moved_frames`); `None` when the
    /// report carries no snap record for this gap.
    snap_lead_frames: Option<usize>,
    snap_trail_frames: Option<usize>,
}

/// One edge's `moved_frames` from a gap's `edge_snap` record.
fn snap_moved(edge_snap: Option<&serde_json::Value>, side: &str) -> Option<usize> {
    edge_snap?
        .get(side)?
        .get("moved_frames")?
        .as_u64()
        .map(|v| v as usize)
}

fn outcome_from_status(status: &serde_json::Value) -> Result<Outcome, String> {
    let (tag, body) = status
        .as_object()
        .and_then(|o| o.iter().next())
        .ok_or("status is not an externally tagged object")?;
    let flag = |k: &str| body.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let frames = |k: &str| body.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
    // Dual-fit first, as in the log parser: a dual-fit rescue is never an anchor-seam write.
    let fill_path = match tag.as_str() {
        "patched" if flag("dual_fit_used") => "dual_fit",
        "patched" if flag("anchor_seam_used") => "anchor_seam",
        "patched" => "single_lag",
        _ => "not_patched",
    };
    Ok(Outcome {
        disposition: tag.clone(),
        fill_path,
        start_adjust_frames: frames("gap_start_adjust_frames"),
        end_adjust_frames: frames("gap_end_adjust_frames"),
        snap_lead_frames: None,
        snap_trail_frames: None,
    })
}

/// A gap's scan edges as decoded-timeline frames, and its outcome.
#[derive(Debug, Clone, PartialEq)]
struct MediaGap {
    index: usize,
    lead: usize,
    trail: usize,
    outcome: Outcome,
}

/// Scan edges at `trunc(secs · rate)` — the listen-mode boundary arithmetic — paired with
/// `patch.gaps`, which must be index-parallel to `scan.gaps` with the same bounds.
fn media_gaps(report: &PatchReportJson, rate: u32) -> Result<Vec<MediaGap>, String> {
    let patch = report
        .patch
        .as_ref()
        .ok_or("report has no `patch` object; it needs a `--patch-only --format json` run")?;
    if patch.gaps.len() != report.scan.gaps.len() {
        return Err(format!(
            "{} scan gaps but {} patch outcomes",
            report.scan.gaps.len(),
            patch.gaps.len()
        ));
    }
    let r = f64::from(rate);
    report
        .scan
        .gaps
        .iter()
        .zip(&patch.gaps)
        .enumerate()
        .map(|(index, (s, p))| {
            if (s.video_a_start_secs - p.a_start_secs).abs() > 1e-6
                || (s.video_a_end_secs - p.a_end_secs).abs() > 1e-6
            {
                return Err(format!("gap {index}: scan and patch bounds differ"));
            }
            let mut outcome =
                outcome_from_status(&p.status).map_err(|e| format!("gap {index}: {e}"))?;
            outcome.snap_lead_frames = snap_moved(p.edge_snap.as_ref(), "lead");
            outcome.snap_trail_frames = snap_moved(p.edge_snap.as_ref(), "trail");
            Ok(MediaGap {
                index,
                lead: (s.video_a_start_secs * r) as usize,
                trail: (s.video_a_end_secs * r) as usize,
                outcome,
            })
        })
        .collect()
}

/// Copies frame windows out of a sequential decode as it streams past, so only the windows are
/// held. Windows may overlap and need not be sorted.
struct WindowSink {
    windows: Vec<(usize, usize)>,
    bufs: Vec<Vec<f64>>,
    channels: Option<usize>,
    /// Decoded frames seen so far: the next bucket's first frame.
    pos: usize,
}

impl WindowSink {
    fn new(windows: Vec<(usize, usize)>) -> Self {
        let bufs = vec![Vec::new(); windows.len()];
        Self {
            windows,
            bufs,
            channels: None,
            pos: 0,
        }
    }

    fn push(&mut self, samples: &[f32], channels: usize) -> Result<(), String> {
        let ch = *self.channels.get_or_insert(channels);
        if ch != channels || ch == 0 {
            return Err(format!(
                "decoded channel count changed mid-stream ({ch} → {channels})"
            ));
        }
        let lo = self.pos;
        let hi = lo + samples.len() / ch;
        for (&(start, end), buf) in self.windows.iter().zip(&mut self.bufs) {
            let (s, e) = (start.max(lo), end.min(hi));
            if s < e {
                buf.extend(
                    samples[(s - lo) * ch..(e - lo) * ch]
                        .iter()
                        .map(|&x| f64::from(x)),
                );
            }
        }
        self.pos = hi;
        Ok(())
    }

    /// One WAV per window, in window order; short when the decode ended inside it.
    fn into_wavs(self, rate: u32) -> Vec<Wav> {
        let channels = self.channels.unwrap_or(1);
        self.bufs
            .into_iter()
            .map(|samples| Wav {
                channels,
                rate,
                lsb: 2f64.powi(-24),
                samples,
            })
            .collect()
    }
}

/// One edge's reading from the report: the write's adjust move, the snap's move and the § 4
/// exposure estimate — see the [`Row`] fields.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct MediaExposure {
    gap_adjust_ms: Option<f64>,
    snap_moved_ms: Option<f64>,
    exposure_est_ms: Option<f64>,
}

/// Per-edge [`MediaExposure`]. The snap move is read on every gap that carries the record; the
/// adjust and the estimate only on patched gaps. The adjust is measured from the SNAPPED edge, and
/// this bin's floor edge is the unsnapped one, so the estimate takes the snap move off as well.
fn media_exposure(
    onset_near_to_floor_ms: Option<f64>,
    outcome: &Outcome,
    edge: Edge,
    rate: u32,
    crossfade_ms: f64,
) -> MediaExposure {
    let ms = |frames: f64| frames / f64::from(rate) * 1000.0;
    let snap_frames = match edge {
        Edge::Lead => outcome.snap_lead_frames,
        Edge::Trail => outcome.snap_trail_frames,
    };
    let snap_moved_ms = snap_frames.map(|f| ms(f as f64));
    if outcome.disposition != "patched" {
        return MediaExposure {
            snap_moved_ms,
            ..MediaExposure::default()
        };
    }
    let outward_frames = match edge {
        Edge::Lead => -outcome.start_adjust_frames,
        Edge::Trail => outcome.end_adjust_frames,
    };
    let outward = ms(outward_frames as f64);
    let est = onset_near_to_floor_ms
        .filter(|_| outcome.fill_path != "anchor_seam")
        .map(|d| d - crossfade_ms - outward - snap_moved_ms.unwrap_or(0.0));
    MediaExposure {
        gap_adjust_ms: Some(outward),
        snap_moved_ms,
        exposure_est_ms: est,
    }
}

/// Every occurrence of the A path (as given, and its file name) replaced by `<A>`.
fn scrub(message: &str, a_path: &Path) -> String {
    let mut out = message.replace(&a_path.display().to_string(), "<A>");
    if let Some(name) = a_path.file_name().map(|n| n.to_string_lossy().into_owned()) {
        if !name.is_empty() {
            out = out.replace(&name, "<A>");
        }
    }
    out
}

/// The report's pair label: its file stem, else its directory name (see [`pair_label`]).
fn report_pair_label(report: &Path) -> Option<String> {
    let name = |p: Option<&std::ffi::OsStr>| p.map(|n| n.to_string_lossy().into_owned());
    let components: Vec<String> = [
        name(report.parent().and_then(Path::file_name)),
        name(report.file_stem()),
    ]
    .into_iter()
    .flatten()
    .collect();
    pair_label(&components)
}

/// Rows from the per-edge windows. `windows[2k]` / `windows[2k + 1]` are gap `k`'s lead / trail.
fn media_rows(
    gaps: &[MediaGap],
    windows: &[(usize, usize)],
    wavs: &[Wav],
    pair: &str,
    codec: &str,
    crossfade_ms: f64,
    snap_cross_check_ms: Option<f64>,
) -> Vec<Row> {
    let mut rows = Vec::with_capacity(2 * gaps.len());
    for (k, g) in gaps.iter().enumerate() {
        let stem = format!("{pair}_g{}", g.index);
        let site = Site {
            pair,
            rel_dir: ".",
            tree: "media",
            stem: &stem,
            disposition: Some(g.outcome.disposition.clone()),
            fill_path: Some(g.outcome.fill_path),
            context_secs: 0.0,
        };
        let dump = DumpJoin {
            index: Some(g.index),
            codec: Some(codec.to_string()),
            ..DumpJoin::default()
        };
        for (j, (edge, boundary)) in [(Edge::Lead, g.lead), (Edge::Trail, g.trail)]
            .into_iter()
            .enumerate()
        {
            let w = &wavs[2 * k + j];
            let local = boundary - windows[2 * k + j].0;
            let onset = measure_onset(w, local, edge);
            if let Some(cap) = snap_cross_check_ms {
                let prod = production_snap_ms(w, local, edge, cap);
                if onset.onset_near_ms.is_some() || prod.is_some_and(|d| d > 0.0) {
                    eprintln!(
                        "snap-cross-check {pair}/g{} {} disposition={} fill_path={} census_near_ms={} census_floor_ms={} prod_snap_ms={}",
                        g.index,
                        edge.as_str(),
                        g.outcome.disposition,
                        g.outcome.fill_path,
                        onset.onset_near_ms.map_or("-".into(), |v| format!("{v:.2}")),
                        onset.floor_ms.map_or("-".into(), |v| format!("{v:.2}")),
                        prod.map_or("-".into(), |v| format!("{v:.2}")),
                    );
                }
            }
            let mut row = assemble_row(
                &site,
                &dump,
                edge,
                onset,
                WriteReading::default(),
                crossfade_ms,
            );
            let exposure = media_exposure(
                row.onset_near_to_floor_ms,
                &g.outcome,
                edge,
                w.rate,
                crossfade_ms,
            );
            row.gap_adjust_ms = exposure.gap_adjust_ms.map(r2);
            row.snap_moved_ms = exposure.snap_moved_ms.map(r2);
            row.exposure_est_ms = exposure.exposure_est_ms.map(r2);
            rows.push(row);
        }
    }
    rows
}

/// The production snap's outward move (ms) on one edge window, the scan boundary standing in for
/// the refined edge. The other edge is the window's far end, so only this edge can move.
fn production_snap_ms(w: &Wav, boundary: usize, edge: Edge, cap_ms: f64) -> Option<f64> {
    let samples: Vec<f32> = w.samples.iter().map(|&x| x as f32).collect();
    let snap = EdgeSnap::from_ms(cap_ms, f64::from(w.rate));
    let refined = match edge {
        Edge::Lead => RefinedGapFrames {
            start_frame: boundary,
            end_frame: w.frames(),
        },
        Edge::Trail => RefinedGapFrames {
            start_frame: 0,
            end_frame: boundary,
        },
    };
    if refined.start_frame >= refined.end_frame {
        return None;
    }
    let out = snap_gap_edges_to_onset(&samples, w.channels, refined.start_frame, refined, snap);
    let moved = match edge {
        Edge::Lead => refined.start_frame - out.start_frame,
        Edge::Trail => out.end_frame - refined.end_frame,
    };
    Some(w.frames_to_ms(moved as f64))
}

/// AAC frame hop (samples): the MDCT overlap is one hop, so a decoder tail is at most this long.
const GRID_HOP: usize = 1024;
/// Envelope blocks per hop for the window-shape fit.
const GRID_BLOCKS: usize = 8;
/// How far past the scan boundary (frames) the zero run may open: the scan's floor is −60 dBFS,
/// so a decay can cross it well before reaching digital silence.
const GRID_SEARCH: usize = 4 * GRID_HOP;
/// Silence run (frames) the zero edge must open onto, so a zero crossing is never read as the gap.
const GRID_MIN_RUN: usize = 256;

/// Every channel at or below `amp` (`0.0`: exact digital zero).
fn frame_is_zero(w: &Wav, f: usize, amp: f64) -> bool {
    w.frame(f).iter().all(|&x| x.abs() <= amp)
}

/// What every frame-grid reading shares: the two window shapes and the silence amplitude.
struct GridCtx {
    sine: Vec<f64>,
    kbd: Vec<f64>,
    zero_amp: f64,
}

/// The zero run's programme-side edge near `boundary`, as a local frame: on `lead` the first
/// all-zero frame of the run, on `trail` the first non-zero frame after it. Searches up to
/// [`GRID_SEARCH`] into the gap when the boundary frame is not itself zero.
fn zero_edge(w: &Wav, boundary: usize, edge: Edge, amp: f64) -> Option<usize> {
    let n = w.frames();
    let run_from = |s: usize, dir: i64| -> usize {
        let mut k = 0usize;
        let mut f = s as i64;
        while f >= 0 && (f as usize) < n && frame_is_zero(w, f as usize, amp) {
            k += 1;
            f += dir;
        }
        k
    };
    match edge {
        // The first frame (walking into the gap) that opens a run of at least GRID_MIN_RUN, so an
        // isolated quiet frame inside the decay is never read as the silence.
        Edge::Lead => {
            let first = (boundary..n.min(boundary + GRID_SEARCH))
                .find(|&f| run_from(f, 1) >= GRID_MIN_RUN)?;
            let mut z = first;
            while z > 0 && frame_is_zero(w, z - 1, amp) {
                z -= 1;
            }
            Some(z)
        }
        Edge::Trail => {
            let last = (boundary.saturating_sub(GRID_SEARCH)..=boundary.min(n.checked_sub(1)?))
                .rev()
                .find(|&f| run_from(f, -1) >= GRID_MIN_RUN)?;
            let mut e = last;
            while e + 1 < n && frame_is_zero(w, e + 1, amp) {
                e += 1;
            }
            (e + 1 < n).then_some(e + 1)
        }
    }
}

/// Window `w(m)`, `m` in `0..2*GRID_HOP`: sine, or Kaiser-Bessel-derived with alpha 4 (AAC long).
fn aac_windows() -> (Vec<f64>, Vec<f64>) {
    let len = 2 * GRID_HOP;
    let sine: Vec<f64> = (0..len)
        .map(|m| (std::f64::consts::PI * (m as f64 + 0.5) / len as f64).sin())
        .collect();
    let i0 = |x: f64| {
        let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
        while term > 1e-12 * sum {
            term *= (x / (2.0 * k)).powi(2);
            sum += term;
            k += 1.0;
        }
        sum
    };
    let alpha = 4.0 * std::f64::consts::PI;
    let kaiser: Vec<f64> = (0..=GRID_HOP)
        .map(|j| {
            let t = 2.0 * j as f64 / GRID_HOP as f64 - 1.0;
            i0(alpha * (1.0 - t * t).max(0.0).sqrt())
        })
        .collect();
    let total: f64 = kaiser.iter().sum();
    let mut acc = 0.0;
    let mut kbd = vec![0.0; len];
    for m in 0..GRID_HOP {
        acc += kaiser[m];
        let v = (acc / total).sqrt();
        kbd[m] = v;
        kbd[len - 1 - m] = v;
    }
    (sine, kbd)
}

/// Peak dBFS over hops 2–5 inside the gap from `boundary` (clear of the transition): how silent
/// the silence is.
fn silence_peak_db(w: &Wav, boundary: usize, edge: Edge) -> Option<f64> {
    let (lo, hi) = match edge {
        Edge::Lead => (
            boundary + 2 * GRID_HOP,
            (boundary + 6 * GRID_HOP).min(w.frames()),
        ),
        Edge::Trail => (
            boundary.saturating_sub(6 * GRID_HOP),
            boundary.saturating_sub(2 * GRID_HOP),
        ),
    };
    (lo < hi).then(|| {
        let peak = (lo..hi).fold(0.0f64, |m, f| m.max(w.frame_peak(f)));
        20.0 * peak.max(1e-12).log10()
    })
}

/// RMS over all channels of local frames `lo..hi`; `None` if out of the window.
fn block_rms(w: &Wav, lo: i64, hi: i64) -> Option<f64> {
    if lo < 0 || hi as usize > w.frames() || hi <= lo {
        return None;
    }
    let s = &w.samples[lo as usize * w.channels..hi as usize * w.channels];
    Some((s.iter().map(|x| x * x).sum::<f64>() / s.len() as f64).sqrt())
}

/// Block levels (dB re the next hop out) over the hop beside `edge_at`, ordered programme → silence,
/// and the matching window-half levels. `lead`: hop `[at-HOP, at)`, falling half. `trail`: hop
/// `[at, at+HOP)`, rising half, reversed.
fn hop_envelope(w: &Wav, at: i64, edge: Edge, win: &[f64]) -> Option<(Vec<f64>, Vec<f64>)> {
    let hop = GRID_HOP as i64;
    let b = hop / GRID_BLOCKS as i64;
    let db = |x: f64| 20.0 * x.max(1e-12).log10();
    let (hop_lo, ref_lo, ref_hi) = match edge {
        Edge::Lead => (at - hop, at - 2 * hop, at - hop),
        Edge::Trail => (at, at + hop, at + 2 * hop),
    };
    let reference = block_rms(w, ref_lo, ref_hi)?;
    if reference <= 0.0 {
        return None;
    }
    let mut levels = Vec::with_capacity(GRID_BLOCKS);
    let mut model = Vec::with_capacity(GRID_BLOCKS);
    for k in 0..GRID_BLOCKS as i64 {
        let lo = hop_lo + k * b;
        levels.push(db(block_rms(w, lo, lo + b)? / reference));
        let m0 = match edge {
            Edge::Lead => (hop + k * b) as usize,
            Edge::Trail => (k * b) as usize,
        };
        let ms = &win[m0..m0 + b as usize];
        model.push(db((ms.iter().map(|x| x * x).sum::<f64>()
            / ms.len() as f64)
            .sqrt()));
    }
    if edge == Edge::Trail {
        levels.reverse();
        model.reverse();
    }
    Some((levels, model))
}

/// Offset-free misfit: sd of `levels - model` (dB), and its mean.
fn misfit(levels: &[f64], model: &[f64]) -> (f64, f64) {
    let d: Vec<f64> = levels.iter().zip(model).map(|(l, m)| l - m).collect();
    let mean = d.iter().sum::<f64>() / d.len() as f64;
    let var = d.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / d.len() as f64;
    (var.sqrt(), mean)
}

const FRAME_GRID_HEADER: &str = "pair,gap_index,edge,disposition,fill_path,rate,channels,onset_near_ms,silence_peak_db,zero_edge_vs_boundary,residue_1024,residue_2048,sd_flat_db,sd_sine_db,sd_kbd_db,mean_sine_db,mean_kbd_db,ctrl_sd_flat_db,ctrl_sd_sine_db,ctrl_sd_kbd_db,levels_db,env_dbfs";

/// One `frame-grid.csv` line per edge; empty fields where the edge has no zero run.
fn frame_grid_line(
    pair: &str,
    g: &MediaGap,
    edge: Edge,
    w: &Wav,
    (window_start, local_boundary): (usize, usize),
    onset_near_ms: Option<f64>,
    ctx: &GridCtx,
) -> String {
    let opt = |v: Option<f64>| v.map_or(String::new(), |x| format!("{x:.3}"));
    let head = format!(
        "{pair},{},{},{},{},{},{},{},{}",
        g.index,
        edge.as_str(),
        g.outcome.disposition,
        g.outcome.fill_path,
        w.rate,
        w.channels,
        opt(onset_near_ms),
        opt(silence_peak_db(w, local_boundary, edge)),
    );
    let Some(at) = zero_edge(w, local_boundary, edge, ctx.zero_amp) else {
        return format!("{head},,,,,,,,,,,,,,");
    };
    let abs = window_start + at;
    let fit = |at: i64| -> Option<(f64, f64, f64, f64, f64, Vec<f64>)> {
        let (lv, sine) = hop_envelope(w, at, edge, &ctx.sine)?;
        let (_, kbd) = hop_envelope(w, at, edge, &ctx.kbd)?;
        let flat = vec![0.0; lv.len()];
        let (sd_flat, _) = misfit(&lv, &flat);
        let (sd_sine, mean_sine) = misfit(&lv, &sine);
        let (sd_kbd, mean_kbd) = misfit(&lv, &kbd);
        Some((sd_flat, sd_sine, sd_kbd, mean_sine, mean_kbd, lv))
    };
    let at_i = at as i64;
    // Control: the same reading 4 hops into programme, where no window applies.
    let ctrl_at = match edge {
        Edge::Lead => at_i - 4 * GRID_HOP as i64,
        Edge::Trail => at_i + 4 * GRID_HOP as i64,
    };
    let main = fit(at_i);
    let ctrl = fit(ctrl_at);
    let f = |v: f64| format!("{v:.3}");
    let (m1, m2, m3, m4, m5, lv) = match &main {
        Some((a, b, c, d, e, lv)) => (
            f(*a),
            f(*b),
            f(*c),
            f(*d),
            f(*e),
            lv.iter()
                .map(|x| format!("{x:.1}"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
        None => Default::default(),
    };
    let (c1, c2, c3) = match &ctrl {
        Some((a, b, c, ..)) => (f(*a), f(*b), f(*c)),
        None => Default::default(),
    };
    // Absolute envelope (dBFS RMS, 128-frame blocks) over the three hops on the programme side,
    // ordered programme → silence, so the whole fall is visible.
    let blk = (GRID_HOP / GRID_BLOCKS) as i64;
    let env: Vec<String> = (0..3 * GRID_BLOCKS as i64)
        .filter_map(|k| {
            let lo = match edge {
                Edge::Lead => at_i - 3 * GRID_HOP as i64 + k * blk,
                Edge::Trail => at_i + 3 * GRID_HOP as i64 - (k + 1) * blk,
            };
            block_rms(w, lo, lo + blk).map(|r| format!("{:.0}", 20.0 * r.max(1e-12).log10()))
        })
        .collect();
    format!(
        "{head},{},{},{},{m1},{m2},{m3},{m4},{m5},{c1},{c2},{c3},{lv},{}",
        at as i64 - local_boundary as i64,
        abs % GRID_HOP,
        abs % (2 * GRID_HOP),
        env.join(" "),
    )
}

/// One report: open its A, decode it once, measure every gap edge. Returns rows, the gap count and
/// any `frame-grid.csv` lines.
fn measure_report(
    args: &Args,
    report_path: &Path,
    pair: &str,
) -> Result<(Vec<Row>, usize, Vec<String>), String> {
    let text = std::fs::read_to_string(report_path)
        .map_err(|e| format!("report unreadable ({})", e.kind()))?;
    let report: PatchReportJson =
        serde_json::from_str(&text).map_err(|e| format!("not a repair JSON report: {e}"))?;
    let a_path = args
        .media_a
        .clone()
        .unwrap_or_else(|| report.scan.video_a.clone());
    let hide = |e: String| scrub(&e, &a_path);

    let mut session = SymphoniaMediaReader
        .open(&MediaSource::new(a_path.clone()))
        .map_err(|e| hide(e.to_string()))?;
    let tracks = session.list_tracks().map_err(|e| hide(e.to_string()))?;
    let track = select_best_track(&tracks)
        .map_err(|e| hide(e.to_string()))?
        .clone();
    let gaps = media_gaps(&report, track.sample_rate)?;
    let half = (MEDIA_HALF_MS / 1000.0 * f64::from(track.sample_rate)).round() as usize;
    let windows: Vec<(usize, usize)> = gaps
        .iter()
        .flat_map(|g| [g.lead, g.trail])
        .map(|b| (b.saturating_sub(half), b + half))
        .collect();

    let mut sink = WindowSink::new(windows.clone());
    let mut sink_err: Option<String> = None;
    let mut rate_seen: Option<u32> = None;
    let mut skew = None;
    let progress = StderrProgressReporter::new(ProgressMode::Auto);
    session
        .scan_interleaved_buckets(
            &track,
            MEDIA_BUCKET_SECS,
            &progress,
            &format!("pair {pair} A"),
            &mut |bucket| {
                if sink_err.is_none() {
                    rate_seen.get_or_insert(bucket.pcm.sample_rate);
                    if let Err(e) = sink.push(&bucket.pcm.samples, usize::from(bucket.pcm.channels))
                    {
                        sink_err = Some(e);
                    }
                }
                Ok(())
            },
            &mut skew,
        )
        .map_err(|e| hide(e.to_string()))?;
    progress.flush_progress();
    if let Some(e) = sink_err {
        return Err(e);
    }
    if let Some(rate) = rate_seen.filter(|&r| r != track.sample_rate) {
        return Err(format!(
            "decoded rate {rate} Hz differs from the track's {} Hz",
            track.sample_rate
        ));
    }
    let wavs = sink.into_wavs(track.sample_rate);
    let rows = media_rows(
        &gaps,
        &windows,
        &wavs,
        pair,
        &track.codec,
        args.crossfade_ms,
        args.snap_cross_check_ms,
    );
    let mut grid = Vec::new();
    if args.frame_grid {
        let (sine, kbd) = aac_windows();
        let ctx = GridCtx {
            sine,
            kbd,
            zero_amp: args.grid_zero_amp,
        };
        for (k, g) in gaps.iter().enumerate() {
            for (j, (edge, boundary)) in [(Edge::Lead, g.lead), (Edge::Trail, g.trail)]
                .into_iter()
                .enumerate()
            {
                let start = windows[2 * k + j].0;
                let onset = rows
                    .iter()
                    .find(|r| r.gap_index == Some(g.index) && r.edge == edge.as_str())
                    .and_then(|r| r.onset.onset_near_ms);
                grid.push(frame_grid_line(
                    pair,
                    g,
                    edge,
                    &wavs[2 * k + j],
                    (start, boundary - start),
                    onset,
                    &ctx,
                ));
            }
        }
    }
    Ok((rows, gaps.len(), grid))
}

fn run_media(args: &Args) -> Result<(), String> {
    if args.media_a.is_some() && args.report.len() != 1 {
        return Err("--media-a names one A; pass exactly one --report with it".into());
    }
    if !cfg!(feature = "he-aac") {
        eprintln!(
            "warning: built without `he-aac` — AAC will not decode (use --features calibration,he-aac)"
        );
    }
    let reports: Vec<PathBuf> = args.report.iter().map(|r| resolve(r)).collect();
    let out_dir = args.out.as_ref().map(|o| resolve(o)).unwrap_or_else(|| {
        reports[0]
            .parent()
            .unwrap_or(Path::new("."))
            .join("edge-onset")
    });

    let mut rows: Vec<Row> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut n_gaps = 0usize;
    let mut grid: Vec<String> = Vec::new();
    for (k, report) in reports.iter().enumerate() {
        let pair =
            report_pair_label(report).or_else(|| args.pair.clone().filter(|_| reports.len() == 1));
        let Some(pair) = pair else {
            skipped.push(format!(
                "report #{}: no pair label in its file or directory name (pass --pair with one report)",
                k + 1
            ));
            continue;
        };
        eprintln!("pair {pair}: decoding A ({} of {})", k + 1, reports.len());
        match measure_report(args, report, &pair) {
            Ok((mut r, n, mut g)) => {
                n_gaps += n;
                rows.append(&mut r);
                grid.append(&mut g);
            }
            Err(why) => skipped.push(format!("pair {pair}: {why}")),
        }
    }
    if rows.is_empty() {
        return Err(format!("no edges measured; {}", skipped.join("; ")));
    }
    sort_rows(&mut rows);
    print_media_table(&rows);

    let contract = serde_json::json!({
        "instrument": "edge-onset media mode (docs/dev/archive/TEMP-soft-edge-census-plan.md § 3, § 4 Phase 2)",
        "distances": "milliseconds OUTWARD from the scan boundary into programme: before it on `lead`, after it on `trail`",
        "onset": format!("{BIN_MS} ms peak bins over {WINDOW_MS} ms; a >= {FALL_DB} dB fall (lead) / rise (trail) sustained to the boundary; refined to the sample"),
        "onset_far_vs_near": format!("far = earliest lead fall / latest trail rise; near = closest to the boundary; onsets within {REF_BINS} bins merge to the outermost. They disagree on the room-tone shape; flagged, not resolved"),
        "n_inside_floor": "falls nearer the boundary than the floor edge (inside the silent run); counted, never an onset",
        "floor": format!("first non-silent frame outward of the boundary, any channel |x| >= {FLOOR_AMP}; this bin's own comparison"),
        "decode": format!("A only, the scanner's sequential bucket decode; ±{MEDIA_HALF_MS} ms kept per scan edge at trunc(video_a_*_secs · rate)"),
        "write": "not measured in media mode (write_*, b_level_*, exposure_hi/lo/far columns are empty)",
        "gap_adjust_ms": "patched edges: the write's outward move from the refined edge (-gap_start_adjust_frames lead, +gap_end_adjust_frames trail)",
        "snap_moved_ms": "the edge snap's outward move (edge_snap.lead/trail.moved_frames); every edge of a report with a snap record, 0 when that edge did not snap",
        "exposure_est_ms": format!("onset_near_to_floor_ms - cf - gap_adjust_ms - snap_moved_ms with cf = --crossfade-ms {}; patched, non-anchor-seam edges with an onset only. gap_adjust is measured from the snapped edge, the floor edge here is the unsnapped one", args.crossfade_ms),
        "anchor_seam": "no estimate: the anchor seam writes 0-2.2 s past the prediction and the report does not say which side moved",
        "denominator": "every scan gap of each report's pair — a base rate for these pairs, split patched vs not",
    });
    let (census, json) = write_census(&out_dir, &rows, contract)?;
    if args.frame_grid {
        let path = out_dir.join("frame-grid.csv");
        let mut text = String::from(FRAME_GRID_HEADER);
        text.push('\n');
        for line in &grid {
            text.push_str(line);
            text.push('\n');
        }
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {}", path.display());
    }
    print_media_summary(&rows, n_gaps, reports.len(), &skipped);
    println!("\nwrote {}", census.display());
    println!("wrote {}", json.display());
    Ok(())
}

/// Edges with an onset only; the full set is in the census.
fn print_media_table(rows: &[Row]) {
    let pw = rows.iter().map(|r| r.pair.len()).max().unwrap_or(4).max(4);
    println!(
        "{:>pw$} {:>5} {:<5} {:<11} {:>7} {:>7} {:>6} {:>7} {:>6} {:>7} {:<11}",
        "pair", "gap", "edge", "disp", "near", "far", "floor", "n→fl", "adj", "est", "fill"
    );
    for r in rows.iter().filter(|r| r.onset.onset_near_ms.is_some()) {
        println!(
            "{:>pw$} {:>5} {:<5} {:<11} {:>7} {:>7} {:>6} {:>7} {:>6} {:>7} {:<11}",
            r.pair,
            r.gap_index
                .map(|i| format!("g{i}"))
                .unwrap_or_else(|| "?".into()),
            r.edge,
            r.disposition.as_deref().unwrap_or("-"),
            fmt_opt(r.onset.onset_near_ms),
            fmt_opt(r.onset.onset_far_ms),
            fmt_opt(r.onset.floor_ms),
            fmt_opt(r.onset_near_to_floor_ms),
            fmt_opt(r.gap_adjust_ms),
            fmt_opt(r.exposure_est_ms),
            r.fill_path.unwrap_or("-")
        );
    }
}

/// One line per value of `key`: edges, with an onset, band above floor, estimate > 0 and > 5 ms.
fn print_split(title: &str, rows: &[&Row], key: impl Fn(&Row) -> String) {
    let mut groups: BTreeMap<String, [usize; 5]> = BTreeMap::new();
    for r in rows {
        let g = groups.entry(key(r)).or_default();
        let est = r.exposure_est_ms;
        g[0] += 1;
        g[1] += usize::from(r.onset.onset_near_ms.is_some());
        g[2] += usize::from(r.onset.band_above_floor == Some(true));
        g[3] += usize::from(est.is_some_and(|e| e > 0.0));
        g[4] += usize::from(est.is_some_and(|e| e > 5.0));
    }
    println!("{title}:");
    for (k, g) in groups {
        println!(
            "  {k:<14} edges {:>5}  onset {:>4}  above-floor {:>4}  est>0 {:>4}  est>5ms {:>4}",
            g[0], g[1], g[2], g[3], g[4]
        );
    }
}

fn print_media_summary(rows: &[Row], n_gaps: usize, n_reports: usize, skipped: &[String]) {
    println!(
        "\n{} edge(s) from {n_gaps} gap(s) over {n_reports} report(s); {} skipped",
        rows.len(),
        skipped.len()
    );
    for line in skipped {
        println!("  skipped {line}");
    }
    let disagree = rows.iter().filter(|r| r.onsets_disagree).count();
    let inside = rows.iter().filter(|r| r.onset.n_inside_floor > 0).count();
    let clipped = rows
        .iter()
        .filter(|r| r.onset.window_ms + BIN_MS < WINDOW_MS)
        .count();
    println!(
        "{disagree} edge(s) with far ≠ near (room-tone shape, flagged); {inside} with a fall inside \
         the silent run; {clipped} window(s) clipped at a media edge"
    );

    let all: Vec<&Row> = rows.iter().collect();
    let patched: Vec<&Row> = rows
        .iter()
        .filter(|r| r.disposition.as_deref() == Some("patched"))
        .collect();
    print_split("by disposition (all edges)", &all, |r| {
        r.disposition.clone().unwrap_or_default()
    });
    print_split("by edge (all edges)", &all, |r| r.edge.to_string());
    print_split("by edge (patched)", &patched, |r| r.edge.to_string());
    print_split("by codec (patched)", &patched, |r| {
        r.codec.clone().unwrap_or_default()
    });
    print_split("by fill path (patched)", &patched, |r| {
        r.fill_path.unwrap_or("-").to_string()
    });

    let mut bands: Vec<f64> = patched
        .iter()
        .filter_map(|r| r.onset_near_to_floor_ms)
        .collect();
    bands.sort_by(f64::total_cmp);
    if !bands.is_empty() {
        let q = |p: f64| bands[((bands.len() - 1) as f64 * p).round() as usize];
        println!(
            "onset → floor on patched edges (n = {}): min {:.1}, median {:.1}, p90 {:.1}, max {:.1} ms",
            bands.len(),
            q(0.0),
            q(0.5),
            q(0.9),
            q(1.0)
        );
    }

    let anchor: Vec<&&Row> = patched
        .iter()
        .filter(|r| r.fill_path == Some("anchor_seam") && r.onset.onset_near_ms.is_some())
        .collect();
    println!(
        "{} anchor-seam patched edge(s) with an onset — exposure unknown, listen clip only:",
        anchor.len()
    );
    for r in anchor {
        println!(
            "  {}/g{} {}: onset → floor {} ms",
            r.pair,
            r.gap_index.unwrap_or_default(),
            r.edge,
            fmt_opt(r.onset_near_to_floor_ms)
        );
    }

    // § 4's early exit: no patched edge besides 50/g7 over ~5 ms.
    let mut exposed: Vec<&&Row> = patched
        .iter()
        .filter(|r| r.exposure_est_ms.is_some_and(|e| e > 5.0))
        .filter(|r| !(r.pair == "50" && r.gap_index == Some(7) && r.edge == "lead"))
        .collect();
    exposed.sort_by(|x, y| {
        y.exposure_est_ms
            .unwrap_or_default()
            .total_cmp(&x.exposure_est_ms.unwrap_or_default())
    });
    println!(
        "early exit (§ 4): {} patched edge(s) besides 50/g7 lead with est > 5 ms{}",
        exposed.len(),
        if exposed.is_empty() {
            " — the exit holds"
        } else {
            ""
        }
    );
    for r in exposed.iter().take(25) {
        println!(
            "  {}/g{} {}: est {} ms (onset → floor {}, adjust {}, band peak {} dBFS, fill {})",
            r.pair,
            r.gap_index.unwrap_or_default(),
            r.edge,
            fmt_opt(r.exposure_est_ms),
            fmt_opt(r.onset_near_to_floor_ms),
            fmt_opt(r.gap_adjust_ms),
            fmt_opt(r.onset.band_peak_dbfs),
            r.fill_path.unwrap_or("-")
        );
    }
    if exposed.len() > 25 {
        println!("  … {} more in the census", exposed.len() - 25);
    }
    println!(
        "  denominator is every scan gap of the given pairs; audibility applies to patched edges only"
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

    fn outcome(status: serde_json::Value) -> Outcome {
        outcome_from_status(&status).unwrap()
    }

    #[test]
    fn status_gives_disposition_fill_path_and_adjusts() {
        let o = outcome(serde_json::json!({"patched": {
            "dual_fit_used": true, "anchor_seam_used": true,
            "gap_start_adjust_frames": -96, "gap_end_adjust_frames": 48}}));
        assert_eq!(
            o,
            Outcome {
                disposition: "patched".into(),
                fill_path: "dual_fit",
                start_adjust_frames: -96,
                end_adjust_frames: 48,
                snap_lead_frames: None,
                snap_trail_frames: None,
            }
        );
        let anchor = outcome(serde_json::json!({"patched": {"anchor_seam_used": true}}));
        assert_eq!(anchor.fill_path, "anchor_seam");
        assert_eq!(anchor.start_adjust_frames, 0);
        let single = outcome(serde_json::json!({"patched": {"dual_fit_used": false}}));
        assert_eq!(single.fill_path, "single_lag");
        let skipped = outcome(serde_json::json!({"skipped": {"reason": "x"}}));
        assert_eq!(
            (skipped.disposition.as_str(), skipped.fill_path),
            ("skipped", "not_patched")
        );
        assert!(outcome_from_status(&serde_json::json!("patched")).is_err());
    }

    fn report(scan: &[(f64, f64)], patch: Option<&[(f64, f64)]>) -> PatchReportJson {
        PatchReportJson {
            scan: ScanJson {
                video_a: PathBuf::from("a.mkv"),
                gaps: scan
                    .iter()
                    .map(|&(s, e)| ScanGapJson {
                        video_a_start_secs: s,
                        video_a_end_secs: e,
                    })
                    .collect(),
            },
            patch: patch.map(|p| PatchJson {
                gaps: p
                    .iter()
                    .map(|&(s, e)| PatchGapJson {
                        a_start_secs: s,
                        a_end_secs: e,
                        status: serde_json::json!({"not_planned": {}}),
                        edge_snap: None,
                    })
                    .collect(),
            }),
        }
    }

    #[test]
    fn media_gaps_truncate_the_scan_bounds_and_refuse_bad_reports() {
        let bounds = [(1.000_01, 2.5), (10.0, 10.75)];
        let gaps = media_gaps(&report(&bounds, Some(&bounds)), SR).unwrap();
        assert_eq!((gaps[0].lead, gaps[0].trail), (48_000, 120_000));
        assert_eq!(
            (gaps[1].index, gaps[1].lead, gaps[1].trail),
            (1, 480_000, 516_000)
        );
        assert_eq!(gaps[1].outcome.disposition, "not_planned");
        assert_eq!(gaps[1].outcome.snap_lead_frames, None);
        assert!(media_gaps(&report(&bounds, None), SR)
            .unwrap_err()
            .contains("--patch-only"));
        assert!(media_gaps(&report(&bounds, Some(&bounds[..1])), SR).is_err());
        assert!(media_gaps(&report(&bounds, Some(&[(1.0, 2.5), (10.0, 10.8)])), SR).is_err());
    }

    #[test]
    fn sink_collects_overlapping_windows_across_buckets() {
        // Stereo, frame i = (i, -i); windows overlap, are unsorted, and one runs past the end.
        let frames: Vec<f32> = (0..100).flat_map(|i| [i as f32, -(i as f32)]).collect();
        let mut sink = WindowSink::new(vec![(30, 60), (5, 12), (50, 70), (95, 120)]);
        for bucket in frames.chunks(2 * 7) {
            sink.push(bucket, 2).unwrap();
        }
        let wavs = sink.into_wavs(SR);
        let firsts = |w: &Wav| w.samples.iter().step_by(2).copied().collect::<Vec<f64>>();
        let range = |a: usize, b: usize| (a..b).map(|i| i as f64).collect::<Vec<f64>>();
        assert_eq!(firsts(&wavs[0]), range(30, 60));
        assert_eq!(firsts(&wavs[1]), range(5, 12));
        assert_eq!(firsts(&wavs[2]), range(50, 70));
        assert_eq!(
            firsts(&wavs[3]),
            range(95, 100),
            "truncated at the end of the decode"
        );
        assert_eq!(wavs[0].samples[1], -30.0);
        assert_eq!((wavs[0].channels, wavs[0].rate), (2, SR));
    }

    #[test]
    fn sink_refuses_a_channel_change() {
        let mut sink = WindowSink::new(vec![(0, 10)]);
        sink.push(&[0.0; 8], 2).unwrap();
        assert!(sink.push(&[0.0; 8], 1).is_err());
    }

    #[test]
    fn exposure_estimate_subtracts_crossfade_and_the_outward_move() {
        let patched = |fill_path, start, end| Outcome {
            disposition: "patched".into(),
            fill_path,
            start_adjust_frames: start,
            end_adjust_frames: end,
            snap_lead_frames: None,
            snap_trail_frames: None,
        };
        let pair = |e: MediaExposure| (e.gap_adjust_ms, e.exposure_est_ms);
        // Lead moved 5 ms outward (start adjust −240 frames); trail moved 2 ms inward.
        let o = patched("single_lag", -240, -96);
        let (adj, est) = pair(media_exposure(Some(25.0), &o, Edge::Lead, SR, 10.0));
        assert_eq!(adj, Some(5.0));
        assert!((est.unwrap() - 10.0).abs() < 1e-9);
        let (adj, est) = pair(media_exposure(Some(25.0), &o, Edge::Trail, SR, 10.0));
        assert_eq!(adj, Some(-2.0));
        assert!((est.unwrap() - 17.0).abs() < 1e-9);
        // Anchor seam: the move is reported, the estimate is not.
        let (adj, est) = pair(media_exposure(
            Some(25.0),
            &patched("anchor_seam", -48, 0),
            Edge::Lead,
            SR,
            10.0,
        ));
        assert_eq!((adj, est), (Some(1.0), None));
        // No onset: no estimate. Not patched: nothing.
        assert_eq!(
            media_exposure(None, &o, Edge::Lead, SR, 10.0).exposure_est_ms,
            None
        );
        let skipped = Outcome {
            disposition: "skipped".into(),
            ..patched("not_patched", -240, 0)
        };
        assert_eq!(
            media_exposure(Some(25.0), &skipped, Edge::Lead, SR, 10.0),
            MediaExposure::default()
        );
    }

    #[test]
    fn a_snapped_edge_takes_its_move_off_the_exposure_estimate() {
        // Lead snapped 12 ms (576 frames) to its onset 25 ms out; the write then adjusted 3 ms
        // outward of the SNAPPED edge. Exposure = 25 − 10 − 3 − 12 = 0.
        let o = Outcome {
            disposition: "patched".into(),
            fill_path: "single_lag",
            start_adjust_frames: -144,
            end_adjust_frames: 0,
            snap_lead_frames: Some(576),
            snap_trail_frames: Some(0),
        };
        let lead = media_exposure(Some(25.0), &o, Edge::Lead, SR, 10.0);
        assert_eq!(lead.snap_moved_ms, Some(12.0));
        assert!(lead.exposure_est_ms.unwrap().abs() < 1e-9);
        let trail = media_exposure(Some(25.0), &o, Edge::Trail, SR, 10.0);
        assert_eq!(trail.snap_moved_ms, Some(0.0));
        assert!((trail.exposure_est_ms.unwrap() - 15.0).abs() < 1e-9);
        // A skipped gap still reports its snap move, with no adjust or estimate.
        let skipped = Outcome {
            disposition: "skipped".into(),
            fill_path: "not_patched",
            ..o
        };
        assert_eq!(
            media_exposure(Some(25.0), &skipped, Edge::Lead, SR, 10.0),
            MediaExposure {
                snap_moved_ms: Some(12.0),
                ..MediaExposure::default()
            }
        );
    }

    #[test]
    fn snap_moves_are_read_per_edge_from_the_record() {
        let rec = serde_json::json!({"lead": {"outcome": "snapped", "moved_frames": 576},
            "trail": {"outcome": "hard", "moved_frames": 0}});
        assert_eq!(snap_moved(Some(&rec), "lead"), Some(576));
        assert_eq!(snap_moved(Some(&rec), "trail"), Some(0));
        assert_eq!(snap_moved(None, "lead"), None);
    }

    #[test]
    fn scrub_hides_the_a_path_and_name() {
        let a = Path::new("media/some title.mkv");
        let msg = format!("cannot open {}; some title.mkv missing", a.display());
        assert_eq!(scrub(&msg, a), "cannot open <A>; <A> missing");
    }

    #[test]
    fn report_label_comes_from_the_stem_or_its_directory() {
        assert_eq!(
            report_pair_label(Path::new("x/reports/50.json")),
            Some("50".into())
        );
        assert_eq!(
            report_pair_label(Path::new("x/31/patch.json")),
            Some("31".into())
        );
        assert_eq!(report_pair_label(Path::new("x/reports/patch.json")), None);
    }

    /// The decode check in miniature: the 50/g7 lead shape streamed through the sink in buckets
    /// reads the same onset as the whole clip measured at the same boundary.
    #[test]
    fn bucketed_window_reads_the_same_onset_as_the_whole_clip() {
        let boundary = ms(3_000.0) + 17;
        let onset = boundary - ms(25.3);
        let f = |i: usize| match i {
            i if i < onset => sine(i, 0.1, 440.0),
            i if i < boundary + ms(800.0) => {
                if i < boundary {
                    sine(i, 0.003, 1000.0)
                } else {
                    0.0
                }
            }
            _ => sine(i, 0.1, 440.0),
        };
        let pcm: Vec<f32> = (0..ms(6_000.0))
            .flat_map(|i| [f(i) as f32, f(i) as f32])
            .collect();
        let whole = Wav {
            channels: 2,
            rate: SR,
            lsb: 2f64.powi(-24),
            samples: pcm.iter().map(|&x| f64::from(x)).collect(),
        };
        let half = ms(MEDIA_HALF_MS);
        let windows = vec![(boundary - half, boundary + half)];
        let mut sink = WindowSink::new(windows.clone());
        for bucket in pcm.chunks(2 * 4_801) {
            sink.push(bucket, 2).unwrap();
        }
        let wavs = sink.into_wavs(SR);
        let got = measure_onset(&wavs[0], boundary - windows[0].0, Edge::Lead);
        assert_eq!(got, measure_onset(&whole, boundary, Edge::Lead));
        assert!(
            (got.onset_near_ms.expect("an onset") - 25.3).abs() < 1.0,
            "{got:?}"
        );
    }
}
