//! `listen-registration` — check the in-engine donor registration against the audio itself.
//!
//! **What it is for.** `DonorRegistration` places B's window by correlating 100 ms dB *envelopes*
//! (`domain::gap_equivalence::register_donor_window`) — cheap enough for a pre-decode gate, and
//! quantized to the scan block. This tool answers "is that lag right?" from a completely different
//! instrument: full-waveform Pearson on the pre-gap shoulder of the `--gap-listen` WAVs, at 1 ms
//! steps. No shared code with the thing it checks, and no dependence on any dump field except the
//! gap's geometry — so a corpus whose *detail* fields came from the projection path can still be
//! cross-checked (which is exactly the 2026-08-03 `extended` case).
//!
//! **Why lag 0 in these WAVs is the nominal map.** `gap_listen` cuts A over
//! `[a_start − ctx, a_end + ctx]` and B over `[b_start − ctx, b_end + ctx]` with the same context and
//! the same rate (B is resampled to A's during decode, never remixed). So sample 0 of
//! `_b_surround.wav` is the nominal offset map's image of sample 0 of `_a_surround.wav`, and the lag
//! between the two clips *is* the local registration error, in the same units and sign as
//! `donor_registration.lag_ms`. Nothing here re-derives the map; it measures the residual.
//!
//! **What it reports, and which column matters.** Three things per gap:
//!
//! - `wav_lag` vs `engine`, in bins. The registration is a bin-quantized estimator, so agreement
//!   means *within one bin*, not equality. On the 2026-08-03 twenty-gap set the worst was 0.83 bins.
//! - `r` — the waveform correlation behind `wav_lag`. **Read this before trusting the row.** On very
//!   quiet shoulders it falls to 0.2–0.4, and there the WAV lag is the weaker of the two estimates,
//!   not the reference. A large `bins` on a low-`r` row is a statement about this tool.
//! - `B−A`, the in-gap level difference at the registered lag, eroded one bin per edge. This is the
//!   column the registration work exists to protect: it must stay near zero on `shared_silence` and
//!   stay large and positive on `repairable_dropout`. A registration that moved a dropout onto B
//!   silence would show up here as a collapsed delta, whatever the lag columns said.
//!
//! **Usage.** Point it at a `--gap-listen` run directory (pair subdirectories or a flat directory of
//! WAVs + dumps). `--observe-dir` supplies the `donor_registration` side when the listen run predates
//! the scan wiring and a later `Observe` run covers the same gaps:
//!
//! ```text
//! cargo run --features calibration --bin listen-registration -- \
//!     gap-files/<listen-run> --observe-dir gap-files/<observe-run>
//! ```
//!
//! Media hygiene: prints pair directory names and gap indices only, never paths or titles.

use std::path::{Path, PathBuf};

use clap::Parser;

use clip_sync_repair::application::gap_fingerprint::GapCorpus;

const A_SUFFIX: &str = "_a_surround.wav";
const B_SUFFIX: &str = "_b_surround.wav";

#[derive(Parser)]
#[command(
    about = "Check the in-engine donor registration against the --gap-listen WAVs (waveform Pearson)"
)]
struct Args {
    /// A `--gap-listen` run directory: pair subdirectories, or one flat directory of WAVs + dumps.
    listen_dir: PathBuf,

    /// Where to read `equivalence_production.donor_registration` from, when the listen run itself predates
    /// the scan wiring. Matched by pair directory + file stem. Defaults to the listen run.
    #[arg(long)]
    observe_dir: Option<PathBuf>,

    /// Seconds of pre-gap A content to match. Shortened automatically at the head of a clip.
    #[arg(long, default_value_t = 2.0)]
    shoulder_secs: f64,

    /// Search half-width, seconds.
    #[arg(long, default_value_t = 1.0)]
    max_lag_secs: f64,

    /// Search step, milliseconds.
    #[arg(long, default_value_t = 1.0)]
    step_ms: f64,

    /// Bin width the `bins` column is expressed in. The scan block, i.e. the registration's own
    /// quantum — leave it alone unless the corpus was scanned at a different block.
    #[arg(long, default_value_t = 100.0)]
    bin_ms: f64,
}

/// One gap's comparison.
struct Row {
    pair: String,
    index: usize,
    class: String,
    wav_lag_ms: f64,
    r: f64,
    engine_lag_ms: Option<f64>,
    a_core_db: f64,
    b_nominal_db: f64,
    b_registered_db: f64,
}

fn main() {
    let args = Args::parse();
    let mut rows = Vec::new();
    let mut skipped = Vec::new();

    for (pair, a_wav) in collect_gaps(&args.listen_dir) {
        match measure(&args, &pair, &a_wav) {
            Ok(row) => rows.push(row),
            Err(why) => skipped.push(format!("{}/{}: {why}", pair, stem_of(&a_wav))),
        }
    }
    rows.sort_by(|x, y| (&x.pair, x.index).cmp(&(&y.pair, y.index)));

    println!(
        "{:>9} {:>18} {:>8} {:>6} {:>8} {:>6} {:>8} {:>8} {:>8} {:>8}",
        "gap", "class", "wav_lag", "r", "engine", "bins", "A core", "B nom", "B reg", "B−A"
    );
    let mut worst: Option<(String, f64)> = None;
    let mut compared = 0usize;
    for row in &rows {
        let (engine, bins) = match row.engine_lag_ms {
            Some(e) => {
                compared += 1;
                let b = (row.wav_lag_ms - e).abs() / args.bin_ms;
                if worst.as_ref().is_none_or(|(_, w)| b > *w) {
                    worst = Some((format!("{}/{}", row.pair, row.index), b));
                }
                (format!("{e:8.0}"), format!("{b:6.2}"))
            }
            None => ("       -".to_string(), "     -".to_string()),
        };
        println!(
            "{:>9} {:>18} {:8.0} {:6.3} {engine} {bins} {:8.1} {:8.1} {:8.1} {:8.1}",
            format!("{}/{}", row.pair, row.index),
            row.class,
            row.wav_lag_ms,
            row.r,
            row.a_core_db,
            row.b_nominal_db,
            row.b_registered_db,
            row.b_registered_db - row.a_core_db,
        );
    }

    println!(
        "\n{} gaps, {compared} with an engine lag to compare",
        rows.len()
    );
    if let Some((gap, bins)) = worst {
        println!("worst disagreement: {gap} at {bins:.2} bins");
    }
    for line in &skipped {
        println!("skipped {line}");
    }
}

/// Every `*_a_surround.wav` under `dir`, as `(pair label, path)`. Accepts both a run directory of
/// numbered pair subdirectories and a single flat directory (labelled `.`).
fn collect_gaps(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let push_from = |d: &Path, label: &str, out: &mut Vec<(String, PathBuf)>| {
        let Ok(entries) = std::fs::read_dir(d) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.to_string_lossy().ends_with(A_SUFFIX) {
                out.push((label.to_string(), p));
            }
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

fn stem_of(a_wav: &Path) -> String {
    a_wav
        .to_string_lossy()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(A_SUFFIX)
        .to_string()
}

fn measure(args: &Args, pair: &str, a_wav: &Path) -> Result<Row, String> {
    let stem = stem_of(a_wav);
    let dir = a_wav.parent().ok_or("no parent directory")?;
    let b_wav = dir.join(format!("{stem}{B_SUFFIX}"));
    if !b_wav.exists() {
        return Err("no B surround clip (production declined, or A-only export)".into());
    }
    let dump: GapCorpus = load(&dir.join(format!("{stem}.json")))?;
    let gap = dump.gaps.first().ok_or("dump carries no gap")?;

    let (a, rate) = read_mono(a_wav)?;
    let (b, b_rate) = read_mono(&b_wav)?;
    if rate != b_rate {
        return Err(format!("rate mismatch: A {rate}, B {b_rate}"));
    }

    // Recover the export context from the clip rather than assuming the 3.0 s default, so a run made
    // with a different `gap_signature_context_secs` measures its own shoulder instead of straddling
    // the gap edge.
    let span = gap.geometry.a_end_secs - gap.geometry.a_start_secs;
    let ctx = (a.len() as f64 / f64::from(rate) - span) / 2.0;
    if ctx <= 0.0 {
        return Err("clip is shorter than the gap it contains".into());
    }
    let gap_start = (ctx * f64::from(rate)).round() as usize;
    let shoulder = (args.shoulder_secs * f64::from(rate)).round() as usize;
    let w_lo = gap_start.saturating_sub(shoulder);
    if gap_start <= w_lo {
        return Err("no pre-gap shoulder to match on".into());
    }

    let half = (args.max_lag_secs * f64::from(rate)).round() as usize;
    let step = ((args.step_ms * f64::from(rate)) / 1000.0).round().max(1.0) as usize;
    let (lag_samples, r) = best_lag(&a[w_lo..gap_start], &b, w_lo, half, step)
        .ok_or("no lag in range covered a full window")?;

    // Erode one bin per edge, the way `DonorRegistration`'s interiors are — without it the window
    // edges import the gap's own fade shoulders.
    let erode = (args.bin_ms * f64::from(rate) / 1000.0).round() as usize;
    let core_lo = gap_start + erode;
    let core_hi = (gap_start + (span * f64::from(rate)).round() as usize).saturating_sub(erode);
    if core_hi <= core_lo {
        return Err("gap core vanishes under erosion".into());
    }

    let engine_lag_ms = engine_lag(args, pair, &stem, gap.index);
    let shift = engine_lag_ms.map_or(0, |ms| (ms / 1000.0 * f64::from(rate)).round() as isize);
    Ok(Row {
        pair: pair.to_string(),
        index: gap.index,
        class: gap
            .equivalence_production_verdict()
            .map(|v| format!("{:?}", v.class))
            .unwrap_or_else(|| "-".into()),
        wav_lag_ms: lag_samples as f64 / f64::from(rate) * 1000.0,
        r,
        engine_lag_ms,
        a_core_db: dbfs(&a[core_lo.min(a.len())..core_hi.min(a.len())]),
        b_nominal_db: dbfs(window(&b, core_lo as isize, core_hi as isize)),
        b_registered_db: dbfs(window(
            &b,
            core_lo as isize + shift,
            core_hi as isize + shift,
        )),
    })
}

/// `donor_registration.lag_ms` for this gap, from `--observe-dir` when given (matched by pair
/// directory and file stem) and from the listen dump otherwise.
fn engine_lag(args: &Args, pair: &str, stem: &str, index: usize) -> Option<f64> {
    let root = args.observe_dir.as_ref()?;
    let dir = if pair == "." {
        root.clone()
    } else {
        root.join(pair)
    };
    let corpus: GapCorpus = load(&dir.join(format!("{stem}.json"))).ok()?;
    let gap = corpus.gaps.iter().find(|g| g.index == index)?;
    Some(
        gap.equivalence_production_verdict()?
            .donor_registration
            .as_ref()?
            .lag_ms,
    )
}

fn load<T: for<'de> serde::Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Mono downmix of a WAV, as normalized `f64`. The clips are whatever layout the source had, and the
/// registration question is about timing, not layout — a plain channel mean is the right reduction
/// and matches what the offline analysis this tool replaces did.
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
    let mono = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f64>() / channels as f64 / scale)
        .collect();
    Ok((mono, spec.sample_rate))
}

/// Slice `b` over `[lo, hi)`, clamped — an out-of-range window yields fewer samples rather than an
/// error, because a lag that runs off the clip head is a normal outcome of the search.
fn window(b: &[f64], lo: isize, hi: isize) -> &[f64] {
    let lo = lo.max(0) as usize;
    let hi = (hi.max(0) as usize).min(b.len());
    &b[lo.min(hi)..hi]
}

/// Pearson of `a` against `b` slid over `±half` samples in `step`s, returning the best `(lag, r)`.
/// `b_center` is where lag 0 sits in `b` — the same index the window came from in `a`, because the
/// two clips are cut on the nominal map.
fn best_lag(
    a: &[f64],
    b: &[f64],
    b_center: usize,
    half: usize,
    step: usize,
) -> Option<(isize, f64)> {
    let mean_a = a.iter().sum::<f64>() / a.len() as f64;
    let centred: Vec<f64> = a.iter().map(|x| x - mean_a).collect();
    let denom_a = centred.iter().map(|x| x * x).sum::<f64>().sqrt();
    if denom_a == 0.0 {
        return None;
    }
    let mut best: Option<(isize, f64)> = None;
    let mut lag = -(half as isize);
    while lag <= half as isize {
        let lo = b_center as isize + lag;
        if lo >= 0 && (lo as usize) + a.len() <= b.len() {
            let seg = &b[lo as usize..lo as usize + a.len()];
            let mean_b = seg.iter().sum::<f64>() / seg.len() as f64;
            let mut num = 0.0;
            let mut den = 0.0;
            for (x, y) in centred.iter().zip(seg) {
                let y = y - mean_b;
                num += x * y;
                den += y * y;
            }
            let den = denom_a * den.sqrt();
            let r = if den > 0.0 { num / den } else { 0.0 };
            if best.is_none_or(|(_, br)| r > br) {
                best = Some((lag, r));
            }
        }
        lag += step as isize;
    }
    best
}

/// RMS in dBFS. Digital silence reports the same `-140.0` sentinel for every empty or dead window,
/// which is well below any real 16-bit floor and so cannot be mistaken for a measurement.
fn dbfs(x: &[f64]) -> f64 {
    if x.is_empty() {
        return -140.0;
    }
    let rms = (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
    if rms > 0.0 {
        20.0 * rms.log10()
    } else {
        -140.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A known shift must come back as that shift, at r = 1 — the tool's own calibration.
    #[test]
    fn best_lag_recovers_a_known_shift() {
        let n = 4000;
        let signal: Vec<f64> = (0..n)
            .map(|i| (i as f64 * 0.017).sin() + (i as f64 * 0.113).sin())
            .collect();
        // B is A delayed by 25 samples.
        let mut b = vec![0.0; 25];
        b.extend_from_slice(&signal);
        let (lag, r) = best_lag(&signal[1000..3000], &b, 1000, 200, 1).expect("a lag");
        assert_eq!(lag, 25);
        assert!(r > 0.999, "{r}");
    }

    /// Sign convention: a *negative* lag means B's content sits earlier than the nominal map put it,
    /// matching `DonorRegistration::lag_blocks` (positive ⇒ later in B).
    #[test]
    fn negative_lag_means_b_is_early() {
        let n = 4000;
        let signal: Vec<f64> = (0..n).map(|i| (i as f64 * 0.031).sin()).collect();
        let b = signal[40..].to_vec();
        let (lag, _) = best_lag(&signal[1000..3000], &b, 1000, 200, 1).expect("a lag");
        assert_eq!(lag, -40);
    }

    #[test]
    fn dbfs_reports_the_sentinel_for_dead_and_empty_windows() {
        assert_eq!(dbfs(&[]), -140.0);
        assert_eq!(dbfs(&[0.0; 16]), -140.0);
        assert!((dbfs(&[1.0; 16]) - 0.0).abs() < 1e-9);
    }
}
