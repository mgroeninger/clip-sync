use std::path::{Path, PathBuf};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde::Deserialize;
use tempfile::TempDir;

use clip_sync::{AlignmentResult, ClipLabel, ClipMatch, SymphoniaMediaReader};
use clip_sync::testing::fakes::FakeProgressReporter;

use crate::application::ports::Aligner;
use crate::application::scan_gaps::{ScanGaps, ScanGapsRequest};
use crate::domain::gap::Gap;

const DEFAULT_SAMPLE_RATE: u32 = 11_025;
const DEFAULT_TOTAL_SECS: u32 = 30;

// ── manifest types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GapCorpusManifest {
    pub version: u32,
    #[serde(default)]
    pub defaults: GapCorpusDefaults,
    #[serde(default)]
    pub case: Vec<GapCorpusCase>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GapCorpusDefaults {
    #[serde(default = "default_scan_block_ms")]
    pub scan_block_ms: u64,
    #[serde(default = "default_silence_peak_fraction")]
    pub silence_peak_fraction: f32,
    #[serde(default = "default_absolute_silence_floor")]
    pub absolute_silence_floor: f32,
    #[serde(default = "default_min_gap_ms")]
    pub min_gap_ms: u64,
    #[serde(default = "default_silence_hold_ms")]
    pub silence_hold_ms: u64,
    #[serde(default = "default_tolerance_secs")]
    pub tolerance_secs: f64,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
}

impl Default for GapCorpusDefaults {
    fn default() -> Self {
        Self {
            scan_block_ms: default_scan_block_ms(),
            silence_peak_fraction: default_silence_peak_fraction(),
            absolute_silence_floor: default_absolute_silence_floor(),
            min_gap_ms: default_min_gap_ms(),
            silence_hold_ms: default_silence_hold_ms(),
            tolerance_secs: default_tolerance_secs(),
            sample_rate: default_sample_rate(),
        }
    }
}

fn default_scan_block_ms() -> u64 { 250 }
fn default_silence_peak_fraction() -> f32 { 0.01 }
fn default_absolute_silence_floor() -> f32 { 33.0 }
fn default_min_gap_ms() -> u64 { 1_000 }
fn default_silence_hold_ms() -> u64 { 500 }
fn default_tolerance_secs() -> f64 { 0.3 }
fn default_sample_rate() -> u32 { DEFAULT_SAMPLE_RATE }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GapCorpusTier {
    Committed,
    Generated,
    External,
}

/// A single silence segment to zero out when generating a case file.
#[derive(Debug, Clone, Deserialize)]
pub struct GapSegment {
    pub start_secs: f64,
    pub end_secs: f64,
}

/// A gap that the scanner is expected to produce for a case.
#[derive(Debug, Clone, Deserialize)]
pub struct ExpectedGap {
    pub start_secs: f64,
    pub end_secs: f64,
    /// Per-boundary tolerance override; falls back to case then defaults.
    #[serde(default)]
    pub tolerance_secs: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GapCorpusCase {
    pub id: String,
    pub tier: GapCorpusTier,
    /// Relative path to the A file (committed and external tiers).
    #[serde(default)]
    pub video_a: Option<String>,
    /// Generator name for the generated tier: "zeroed_chirp" or "quiet_chirp".
    #[serde(default)]
    pub generator: Option<String>,
    /// Total duration (seconds) for generated cases.
    #[serde(default)]
    pub total_secs: Option<u32>,
    /// Chirp amplitude fraction (0.0–1.0) for "quiet_chirp" generator.
    #[serde(default)]
    pub amplitude_fraction: Option<f32>,
    /// Segments to zero out in the generated A file.
    #[serde(default)]
    pub gap_segments: Vec<GapSegment>,
    /// Channel count for generated WAVs (default 1). Committed cases use the file on disk.
    #[serde(default)]
    pub channels: Option<u16>,
    /// For `asymmetric_channels`: index of the channel that carries the chirp (default 0).
    #[serde(default)]
    pub hot_channel: Option<u16>,
    /// For `partial_channel_gap`: index of the channel zeroed in `gap_segments` (default 1).
    #[serde(default)]
    pub gap_channel: Option<u16>,
    /// Ground truth: the gaps the scanner should detect.
    #[serde(default)]
    pub expected_gaps: Vec<ExpectedGap>,
    /// Per-case detector config overrides.
    #[serde(default)]
    pub min_gap_ms: Option<u64>,
    #[serde(default)]
    pub silence_hold_ms: Option<u64>,
    #[serde(default)]
    pub tolerance_secs: Option<f64>,
    #[serde(default)]
    pub ignore: bool,
}

pub struct GeneratedCasePaths {
    pub _temp: TempDir,
    pub video_a: PathBuf,
}

// ── path resolution ───────────────────────────────────────────────────────────

pub fn corpus_root() -> PathBuf {
    if let Ok(root) = std::env::var("CLIP_SYNC_WORKSPACE_ROOT") {
        return PathBuf::from(root)
            .join("tests/gap_corpus");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/gap_corpus")
}

pub fn load_manifest() -> GapCorpusManifest {
    let path = corpus_root().join("manifest.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    toml::from_str(&text)
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Resolve the A-file path and optional temp guard for a case.
/// Returns `(Option<TempDir>, PathBuf)` — the guard must stay alive for the test.
pub fn resolve_case_path(
    case: &GapCorpusCase,
    defaults: &GapCorpusDefaults,
) -> (Option<TempDir>, PathBuf) {
    match case.tier {
        GapCorpusTier::Committed => {
            let root = corpus_root();
            let video_a = root.join(case.video_a.as_ref().expect("committed case needs video_a"));
            (None, video_a)
        }
        GapCorpusTier::Generated => {
            let paths = generate_case_wav(case, defaults);
            let video_a = paths.video_a.clone();
            (Some(paths._temp), video_a)
        }
        GapCorpusTier::External => {
            let corpus_dir = std::env::var("CLIP_SYNC_GAP_CORPUS")
                .expect("CLIP_SYNC_GAP_CORPUS must be set for external tier");
            let video_a = PathBuf::from(corpus_dir)
                .join(case.video_a.as_ref().expect("external case needs video_a"));
            (None, video_a)
        }
    }
}

// ── WAV generation ────────────────────────────────────────────────────────────

fn chirp_sample(sample_rate: u32, index: u64) -> i16 {
    let rate = f64::from(sample_rate);
    let t = index as f64 / rate;
    let freq = 300.0 + 400.0 * t;
    (f64::sin(std::f64::consts::TAU * freq * t) * f64::from(i16::MAX) * 0.5).round() as i16
}

fn wav_spec(sample_rate: u32, channels: u16) -> WavSpec {
    WavSpec {
        channels: channels.max(1),
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    }
}

fn write_mono_wav(path: &Path, sample_rate: u32, samples: impl IntoIterator<Item = i16>) {
    write_interleaved_wav(path, sample_rate, 1, samples);
}

fn write_interleaved_wav(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    samples: impl IntoIterator<Item = i16>,
) {
    let mut writer = WavWriter::create(path, wav_spec(sample_rate, channels)).expect("create wav");
    for s in samples {
        writer.write_sample(s).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

fn symmetric_stereo_chirp_frames(
    sample_rate: u32,
    total_frames: u64,
    amplitude_fraction: f64,
) -> Vec<i16> {
    let mut samples = Vec::with_capacity(total_frames as usize * 2);
    for i in 0..total_frames {
        let sample = (chirp_sample(sample_rate, i) as f64 * amplitude_fraction).round() as i16;
        samples.push(sample);
        samples.push(sample);
    }
    samples
}

fn asymmetric_stereo_chirp_frames(
    sample_rate: u32,
    total_frames: u64,
    hot_channel: u16,
    amplitude_fraction: f64,
) -> Vec<i16> {
    let channels = 2u16;
    let hot = hot_channel.min(channels - 1) as usize;
    let mut samples = Vec::with_capacity(total_frames as usize * channels as usize);
    for i in 0..total_frames {
        let hot_sample = (chirp_sample(sample_rate, i) as f64 * amplitude_fraction).round() as i16;
        for ch in 0..channels as usize {
            samples.push(if ch == hot { hot_sample } else { 0 });
        }
    }
    samples
}

fn zero_wav_segments(path: &Path, sample_rate: u32, segments: &[GapSegment]) {
    if segments.is_empty() {
        return;
    }
    let ranges: Vec<(usize, usize)> = segments
        .iter()
        .map(|seg| {
            let start = (seg.start_secs * f64::from(sample_rate)).round() as usize;
            let end = (seg.end_secs * f64::from(sample_rate)).round() as usize;
            (start, end)
        })
        .collect();
    zero_wav_sample_ranges(path, sample_rate, &ranges);
}

/// Zero exact sample ranges. Unlike `zero_wav_segments`, no rounding from float seconds —
/// use this when segments must align to scanner block boundaries to avoid partial-block leakage.
fn zero_wav_sample_ranges(path: &Path, sample_rate: u32, ranges: &[(usize, usize)]) {
    zero_interleaved_frame_ranges(path, sample_rate, ranges, None);
}

/// Zero frame ranges in an interleaved WAV. When `channel` is `Some(ch)`, only that channel
/// is zeroed per frame; when `None`, every channel in the range is zeroed.
fn zero_interleaved_frame_ranges(
    path: &Path,
    sample_rate: u32,
    frame_ranges: &[(usize, usize)],
    channel: Option<u16>,
) {
    if frame_ranges.is_empty() {
        return;
    }
    let mut reader = WavReader::open(path).expect("open wav");
    let channels = reader.spec().channels.max(1);
    let mut samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|s| s.expect("read sample"))
        .collect();
    for &(start_frame, end_frame) in frame_ranges {
        for frame in start_frame..end_frame {
            if let Some(ch) = channel {
                let idx = frame * channels as usize + ch.min(channels - 1) as usize;
                if let Some(sample) = samples.get_mut(idx) {
                    *sample = 0;
                }
            } else {
                let base = frame * channels as usize;
                for sample in samples
                    .iter_mut()
                    .skip(base)
                    .take(channels as usize)
                {
                    *sample = 0;
                }
            }
        }
    }
    let mut writer =
        WavWriter::create(path, wav_spec(sample_rate, channels)).expect("create wav");
    for s in samples {
        writer.write_sample(s).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

fn zero_wav_segments_interleaved(
    path: &Path,
    sample_rate: u32,
    segments: &[GapSegment],
    channel: Option<u16>,
) {
    if segments.is_empty() {
        return;
    }
    let ranges: Vec<(usize, usize)> = segments
        .iter()
        .map(|seg| {
            let start = (seg.start_secs * f64::from(sample_rate)).round() as usize;
            let end = (seg.end_secs * f64::from(sample_rate)).round() as usize;
            (start, end)
        })
        .collect();
    zero_interleaved_frame_ranges(path, sample_rate, &ranges, channel);
}

fn case_channels(case: &GapCorpusCase) -> u16 {
    case.channels.unwrap_or(1).max(1)
}

pub fn generate_case_wav(case: &GapCorpusCase, defaults: &GapCorpusDefaults) -> GeneratedCasePaths {
    let temp = TempDir::new().expect("tempdir");
    let total_secs = case.total_secs.unwrap_or(DEFAULT_TOTAL_SECS);
    let sample_rate = defaults.sample_rate;
    let total_samples = u64::from(sample_rate) * u64::from(total_secs);
    let path = temp.path().join("a.wav");

    let channels = case_channels(case);
    let fraction = f64::from(case.amplitude_fraction.unwrap_or(1.0));

    match case.generator.as_deref().unwrap_or("zeroed_chirp") {
        "quiet_chirp" => {
            let amp = f64::from(case.amplitude_fraction.unwrap_or(0.05));
            if channels == 1 {
                let samples = (0..total_samples)
                    .map(|i| (chirp_sample(sample_rate, i) as f64 * amp).round() as i16);
                write_mono_wav(&path, sample_rate, samples);
            } else {
                write_interleaved_wav(
                    &path,
                    sample_rate,
                    channels,
                    symmetric_stereo_chirp_frames(sample_rate, total_samples, amp),
                );
            }
        }
        "asymmetric_channels" => {
            assert!(
                channels >= 2,
                "asymmetric_channels generator requires channels >= 2"
            );
            let hot = case.hot_channel.unwrap_or(0);
            write_interleaved_wav(
                &path,
                sample_rate,
                channels,
                asymmetric_stereo_chirp_frames(sample_rate, total_samples, hot, fraction),
            );
        }
        "partial_channel_gap" => {
            assert!(
                channels >= 2,
                "partial_channel_gap generator requires channels >= 2"
            );
            if channels == 2 {
                write_interleaved_wav(
                    &path,
                    sample_rate,
                    2,
                    symmetric_stereo_chirp_frames(sample_rate, total_samples, fraction),
                );
            } else {
                let samples = (0..total_samples).flat_map(|i| {
                    let s = (chirp_sample(sample_rate, i) as f64 * fraction).round() as i16;
                    std::iter::repeat_n(s, channels as usize)
                });
                write_interleaved_wav(&path, sample_rate, channels, samples);
            }
            let gap_ch = case.gap_channel.unwrap_or(1);
            zero_wav_segments_interleaved(&path, sample_rate, &case.gap_segments, Some(gap_ch));
        }
        "zeroed_chirp" if channels > 1 => {
            write_interleaved_wav(
                &path,
                sample_rate,
                channels,
                symmetric_stereo_chirp_frames(sample_rate, total_samples, fraction),
            );
            zero_wav_segments_interleaved(&path, sample_rate, &case.gap_segments, None);
        }
        _ => {
            let samples = (0..total_samples)
                .map(|i| (chirp_sample(sample_rate, i) as f64 * fraction).round() as i16);
            write_mono_wav(&path, sample_rate, samples);
            zero_wav_segments(&path, sample_rate, &case.gap_segments);
        }
    }

    GeneratedCasePaths { _temp: temp, video_a: path }
}

// ── committed fixture generation ──────────────────────────────────────────────

pub fn write_committed_wav_fixtures() {
    let wav_dir = corpus_root().join("wav");
    std::fs::create_dir_all(&wav_dir).expect("create wav dir");

    let rate = DEFAULT_SAMPLE_RATE;
    let total = DEFAULT_TOTAL_SECS;
    let total_samples = u64::from(rate) * u64::from(total);

    // 1. leading_16s_a.wav — 16s silence then chirp.
    let path = wav_dir.join("leading_16s_a.wav");
    let samples = (0..total_samples).map(|i| chirp_sample(rate, i));
    write_mono_wav(&path, rate, samples);
    zero_wav_segments(&path, rate, &[GapSegment { start_secs: 0.0, end_secs: 16.0 }]);

    // 2. mid_2s_a.wav — chirp with a 2-second gap in the middle.
    let path = wav_dir.join("mid_2s_a.wav");
    let samples = (0..total_samples).map(|i| chirp_sample(rate, i));
    write_mono_wav(&path, rate, samples);
    zero_wav_segments(&path, rate, &[GapSegment { start_secs: 14.0, end_secs: 16.0 }]);

    // 3. multi_gap_a.wav — three gaps at 3s, 13s, and 23s.
    // Silence regions are aligned to scanner block boundaries (block_samples = round(0.25 * rate))
    // so no chirp samples leak into the first or last block of each silent run. Without this
    // alignment, a 1-second gap starting at a non-block-aligned sample would have its first block
    // classified as loud (due to 1-3 chirp samples) and be detected as only ~0.75s — below the
    // 1s minimum.
    let path = wav_dir.join("multi_gap_a.wav");
    let samples = (0..total_samples).map(|i| chirp_sample(rate, i));
    write_mono_wav(&path, rate, samples);
    let block_samples = (0.25 * f64::from(rate)).round() as usize; // 2756 at 11025 Hz
    let block_floor = |secs: f64| -> usize {
        let sample = (secs * f64::from(rate)).round() as usize;
        (sample / block_samples) * block_samples
    };
    zero_wav_sample_ranges(&path, rate, &[
        (block_floor(3.0),  block_floor(5.0)),   // 2s gap, blocks 12–21
        (block_floor(13.0), block_floor(15.0)),  // 2s gap, blocks 52–59
        (block_floor(23.0), block_floor(25.0)),  // 2s gap, blocks 92–99
    ]);

    // 4. quiet_no_gap_a.wav — chirp at 5% amplitude; should produce no gaps.
    let path = wav_dir.join("quiet_no_gap_a.wav");
    let fraction = 0.05f64;
    let samples = (0..total_samples)
        .map(|i| (chirp_sample(rate, i) as f64 * fraction).round() as i16);
    write_mono_wav(&path, rate, samples);

    // 5. stereo_mid_gap_2s_a.wav — both channels share a 2s gap (mirror of mid_2s_a).
    let path = wav_dir.join("stereo_mid_gap_2s_a.wav");
    write_interleaved_wav(
        &path,
        rate,
        2,
        symmetric_stereo_chirp_frames(rate, total_samples, 1.0),
    );
    zero_wav_segments_interleaved(
        &path,
        rate,
        &[GapSegment {
            start_secs: 14.0,
            end_secs: 16.0,
        }],
        None,
    );

    // 6. stereo_hot_left_no_gap_a.wav — chirp on L only, R silent; must not report gaps.
    let path = wav_dir.join("stereo_hot_left_no_gap_a.wav");
    write_interleaved_wav(
        &path,
        rate,
        2,
        asymmetric_stereo_chirp_frames(rate, total_samples, 0, 1.0),
    );

    // 7. stereo_partial_gap_a.wav — chirp on L throughout; R zeroed in [14, 16) only.
    let path = wav_dir.join("stereo_partial_gap_a.wav");
    write_interleaved_wav(
        &path,
        rate,
        2,
        asymmetric_stereo_chirp_frames(rate, total_samples, 0, 1.0),
    );
    zero_wav_segments_interleaved(
        &path,
        rate,
        &[GapSegment {
            start_secs: 14.0,
            end_secs: 16.0,
        }],
        Some(1),
    );
}

// ── scan runner & assertions ──────────────────────────────────────────────────

fn no_op_alignment() -> AlignmentResult {
    AlignmentResult {
        clips: vec![ClipMatch {
            label: ClipLabel::Start,
            window_start_secs: 0.0,
            window_end_secs: 0.0,
            aligned: false,
            offset_secs: None,
            confidence: 0.0,
            video_a_decode_skips: 0,
            video_b_decode_skips: 0,
            repetition: None,
        }],
        start_aligned: false,
        end_aligned: None,
        recommended_offset_secs: None,
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap: None,
        high_rate_refinement: None,
    }
}

fn build_scan_request(
    video_a: PathBuf,
    case: &GapCorpusCase,
    defaults: &GapCorpusDefaults,
) -> ScanGapsRequest {
    let min_gap_ms = case.min_gap_ms.unwrap_or(defaults.min_gap_ms);
    let silence_hold_ms = case.silence_hold_ms.unwrap_or(defaults.silence_hold_ms);
    let scan_block_ms = defaults.scan_block_ms;
    let hold_blocks = if scan_block_ms == 0 {
        0
    } else {
        ((silence_hold_ms as f64 / scan_block_ms as f64).ceil()) as u32
    };

    ScanGapsRequest {
        video_a: video_a.clone(),
        video_b: video_a, // unused (no alignment offset)
        align: Default::default(),
        decode_chunk_secs: 10,
        scan_block_secs: scan_block_ms as f64 / 1000.0,
        silence_peak_fraction: defaults.silence_peak_fraction,
        absolute_silence_rms: defaults.absolute_silence_floor,
        silence_hold_blocks: hold_blocks,
        min_gap_secs: min_gap_ms as f64 / 1000.0,
        scan_both: false,
        gap_offset_tolerance_secs: 0.5,
    }
}

pub fn assert_gap_expectations(
    case_id: &str,
    expected: &[ExpectedGap],
    detected: &[Gap],
    case_tolerance: Option<f64>,
    defaults: &GapCorpusDefaults,
) {
    let tolerance = case_tolerance.unwrap_or(defaults.tolerance_secs);

    // Build a matched set: for each expected gap, find the closest unmatched detected gap.
    let mut matched = vec![false; detected.len()];

    for exp in expected {
        let tol = exp.tolerance_secs.unwrap_or(tolerance);
        let found = detected.iter().enumerate().find(|(i, det)| {
            !matched[*i]
                && (det.video_a_start_secs - exp.start_secs).abs() <= tol
                && (det.video_a_end_secs - exp.end_secs).abs() <= tol
        });
        match found {
            Some((i, _)) => matched[i] = true,
            None => {
                let nearest = detected.iter().min_by(|a, b| {
                    let da = (a.video_a_start_secs - exp.start_secs).abs();
                    let db = (b.video_a_start_secs - exp.start_secs).abs();
                    da.partial_cmp(&db).unwrap()
                });
                panic!(
                    "case {case_id}: missed expected gap [{:.2}s – {:.2}s] (tolerance ±{tol:.2}s).\n\
                     Nearest detected: {}\n\
                     All detected: {:#?}",
                    exp.start_secs,
                    exp.end_secs,
                    nearest.map_or("none".to_string(), |g| format!(
                        "[{:.2}s – {:.2}s]",
                        g.video_a_start_secs, g.video_a_end_secs
                    )),
                    detected
                        .iter()
                        .map(|g| format!("[{:.2}s – {:.2}s]", g.video_a_start_secs, g.video_a_end_secs))
                        .collect::<Vec<_>>(),
                );
            }
        }
    }

    // Any unmatched detected gap is a false positive.
    for (i, det) in detected.iter().enumerate() {
        if !matched[i] {
            panic!(
                "case {case_id}: unexpected (false-positive) gap [{:.2}s – {:.2}s].\n\
                 Expected gaps: {:#?}",
                det.video_a_start_secs,
                det.video_a_end_secs,
                expected
                    .iter()
                    .map(|g| format!("[{:.2}s – {:.2}s]", g.start_secs, g.end_secs))
                    .collect::<Vec<_>>(),
            );
        }
    }
}

// ── #[cfg(test)] ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "run manually to regenerate: cargo test -p clip-sync-repair gap_corpus_regenerate -- --ignored --nocapture"]
    fn gap_corpus_regenerate_committed_wav_fixtures() {
        write_committed_wav_fixtures();
        eprintln!("wrote fixtures under {}", corpus_root().join("wav").display());
    }

    #[test]
    fn gap_corpus_manifest_loads() {
        let manifest = load_manifest();
        assert!(manifest.version >= 1, "manifest version should be >= 1");
        assert!(!manifest.case.is_empty(), "manifest should have at least one case");
        let committed: Vec<_> = manifest.case.iter()
            .filter(|c| c.tier == GapCorpusTier::Committed)
            .collect();
        assert!(!committed.is_empty(), "manifest should have at least one committed case");
    }

    struct NeverCalledAligner;

    impl Aligner for NeverCalledAligner {
        fn align(
            &self,
            _: clip_sync::AlignVideosRequest,
            _: &dyn clip_sync::ProgressReporter,
        ) -> Result<clip_sync::AlignmentResult, clip_sync::AppError> {
            unreachable!("corpus tests use scan_after_alignment directly")
        }
    }

    fn run_manifest_cases(tier: GapCorpusTier) {
        let manifest = load_manifest();
        let media_reader = SymphoniaMediaReader;
        let progress = FakeProgressReporter;
        let aligner = NeverCalledAligner;
        let scan = ScanGaps::new(&media_reader, &progress, &aligner);

        for case in manifest.case.iter().filter(|c| c.tier == tier && !c.ignore) {
            let started = std::time::Instant::now();
            let (_guard, video_a) = resolve_case_path(case, &manifest.defaults);

            if tier == GapCorpusTier::Committed {
                assert!(
                    video_a.is_file(),
                    "case {}: missing committed fixture {}",
                    case.id,
                    video_a.display()
                );
            }

            let request = build_scan_request(video_a, case, &manifest.defaults);
            let alignment = no_op_alignment();
            let report = scan
                .scan_after_alignment(request, alignment)
                .unwrap_or_else(|e| panic!("case {} failed: {e}", case.id));

            assert_gap_expectations(
                &case.id,
                &case.expected_gaps,
                &report.gaps,
                case.tolerance_secs,
                &manifest.defaults,
            );

            eprintln!("case {}: {} gap(s) detected in {:.2?}", case.id, report.gaps.len(), started.elapsed());
        }
    }

    #[test]
    fn gap_corpus_committed() {
        run_manifest_cases(GapCorpusTier::Committed);
    }

    #[test]
    #[ignore = "generates WAV fixtures at test time; cargo test -p clip-sync-repair gap_corpus_generated -- --ignored"]
    fn gap_corpus_generated() {
        run_manifest_cases(GapCorpusTier::Generated);
    }

    #[test]
    #[ignore = "requires CLIP_SYNC_GAP_CORPUS env var pointing to real media files"]
    fn gap_corpus_external() {
        if std::env::var("CLIP_SYNC_GAP_CORPUS").is_err() {
            eprintln!("skipping gap_corpus_external: CLIP_SYNC_GAP_CORPUS not set");
            return;
        }
        run_manifest_cases(GapCorpusTier::External);
    }
}
