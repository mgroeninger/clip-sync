use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use tempfile::TempDir;

use crate::application::align_videos::{AlignVideos, AlignVideosRequest};
use crate::application::config::{AlignConfig, AlignmentMode, ClipConfig};
use crate::application::error::AppError;
use crate::application::ports::{Aligner, Fingerprinter, MediaReader};
use crate::application::testing::corpus_sources::{
    self, find_source, load_sources, source_cache_path,
};
use crate::domain::AlignmentModeUsed;
use crate::domain::AlignmentResult;
use crate::test_support::audio_fixtures::{
    write_looped_chirp_wav_pair, write_near_silence_wav_pair, write_offset_chirp_wav_pair,
    write_offset_chirp_wav_pair_with_delay, write_piecewise_offset_chirp_pair,
    write_query_reference_b_longer_chirp_pair, write_query_reference_chirp_pair,
    write_repeated_segment_wav_pair, write_tone_wav, write_tone_wav_at_frequency,
    write_two_clip_inconsistent_pair, ChirpDelayOn,
};
use crate::test_support::ffmpeg_util::{self, EncodeFormat};

/// Short clips to keep Tier-B fixtures under the ~5 MB repo budget (see tests/corpus/README.md).
const DEFAULT_SAMPLE_RATE: u32 = 11_025;
const DEFAULT_TOTAL_SECS: u32 = 30;
const NEGATIVE_CASE_SECS: u32 = 20;

#[derive(Debug, Deserialize)]
pub struct CorpusManifest {
    pub version: u32,
    #[serde(default)]
    pub defaults: CorpusDefaults,
    #[serde(default)]
    pub case: Vec<CorpusCase>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorpusDefaults {
    #[serde(default = "default_clip_length_secs")]
    pub clip_length_secs: u64,
    #[serde(default = "default_num_clips")]
    pub num_clips: u32,
    #[serde(default = "default_tolerance_secs")]
    pub tolerance_secs: f64,
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,
    #[serde(default = "default_target_sample_rate")]
    pub target_sample_rate: u32,
}

impl Default for CorpusDefaults {
    fn default() -> Self {
        Self {
            clip_length_secs: default_clip_length_secs(),
            num_clips: default_num_clips(),
            tolerance_secs: default_tolerance_secs(),
            min_confidence: default_min_confidence(),
            target_sample_rate: default_target_sample_rate(),
        }
    }
}

fn default_clip_length_secs() -> u64 {
    60
}

fn default_num_clips() -> u32 {
    1
}

fn default_tolerance_secs() -> f64 {
    0.15
}

fn default_min_confidence() -> f32 {
    0.5
}

fn default_target_sample_rate() -> u32 {
    DEFAULT_SAMPLE_RATE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CorpusTier {
    Committed,
    Generated,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChirpDelaySide {
    #[default]
    B,
    A,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorpusCase {
    pub id: String,
    pub tier: CorpusTier,
    pub video_a: Option<String>,
    pub video_b: Option<String>,
    #[serde(default)]
    pub generator: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub total_secs: Option<u32>,
    #[serde(default)]
    pub offset_secs: Option<u32>,
    #[serde(default)]
    pub delay_on: Option<ChirpDelaySide>,
    #[serde(default)]
    pub offset_secs_end: Option<u32>,
    #[serde(default)]
    pub tail_silence_secs: Option<u32>,
    #[serde(default)]
    pub prefer_program_track: Option<bool>,
    #[serde(default)]
    pub clip_length_secs: Option<u64>,
    #[serde(default)]
    pub num_clips: Option<u32>,
    #[serde(default)]
    pub require_consistent_offsets: Option<bool>,
    #[serde(default)]
    pub try_all_tracks: Option<bool>,
    #[serde(default)]
    pub refine_offset_with_pcm: Option<bool>,
    #[serde(default)]
    pub compare_refine_pcm: bool,
    #[serde(default)]
    pub refine_offset_high_rate: Option<bool>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub tolerance_secs: Option<f64>,
    pub expected_offset_secs: Option<f64>,
    pub expect_aligned: Option<bool>,
    pub expect_recommended: Option<bool>,
    #[serde(default)]
    pub expect_offsets_consistent: Option<bool>,
    #[serde(default)]
    pub requires_ffmpeg: bool,
    #[serde(default)]
    pub requires_he_aac: bool,
    #[serde(default)]
    pub max_wall_secs: Option<f64>,
    #[serde(default)]
    pub check_clip_repetition: bool,
    #[serde(default)]
    pub expect_clip_repetition: Option<bool>,
    #[serde(default)]
    pub verify_offset: bool,
    #[serde(default)]
    pub expect_offset_verified: Option<bool>,
    /// When set, dedicated probe tests run hold-out verification with this wrong Δ (seconds).
    #[serde(default)]
    pub probe_wrong_verification_offset_secs: Option<f64>,
    /// Generator metadata for dedicated probe tests only — excluded from `corpus_generated_cases`.
    #[serde(default)]
    pub probe_only: bool,
    #[serde(default)]
    pub ignore: bool,
    /// Wikimedia (or other) master listed in tests/corpus/sources.toml.
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub requires_source: bool,
    /// Force alignment algorithm: `auto`, `symmetric`, or `queryreference`.
    #[serde(default)]
    pub alignment_mode: Option<String>,
    /// A timeline position where the short query clip starts (query-reference cases).
    #[serde(default)]
    pub expect_clip_on_a_start_secs: Option<f64>,
    /// Longer-file anchor for query-reference cases (when it differs from `clip_on_a_start`).
    #[serde(default)]
    pub expect_anchor_on_reference_secs: Option<f64>,
    /// Duration of the short query clip on B (query-reference generator).
    #[serde(default)]
    pub query_duration_secs: Option<u32>,
    /// Where the query clip is embedded on the long reference (query-reference generator).
    #[serde(default)]
    pub query_anchor_secs: Option<u32>,
}

pub struct GeneratedCasePaths {
    pub _temp: TempDir,
    pub video_a: PathBuf,
    pub video_b: PathBuf,
}

pub fn corpus_root() -> PathBuf {
    corpus_sources::corpus_root()
}

pub fn load_manifest() -> CorpusManifest {
    let path = corpus_root().join("manifest.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    toml::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

pub fn resolve_committed_pair(case: &CorpusCase) -> (PathBuf, PathBuf) {
    let root = corpus_root();
    let video_a = root.join(case.video_a.as_ref().expect("committed case needs video_a"));
    let video_b = root.join(case.video_b.as_ref().expect("committed case needs video_b"));
    (video_a, video_b)
}

fn parse_encode_format(format: &str) -> Option<EncodeFormat> {
    match format {
        "mp3" => Some(EncodeFormat::Mp3),
        "mp3_no_duration" => Some(EncodeFormat::Mp3NoDurationTag),
        "mp4" => Some(EncodeFormat::Mp4Aac),
        "mp4_stereo" => Some(EncodeFormat::Mp4AacStereo),
        "mkv" => Some(EncodeFormat::MkvFlac),
        "mkv_aac" => Some(EncodeFormat::MkvAac),
        #[cfg(feature = "he-aac")]
        "mp4_he_aac" => Some(EncodeFormat::Mp4HeAac),
        "wav" | "cross_mp3_mp4" => None,
        _ => None,
    }
}

fn extension_for_format(format: &str) -> &'static str {
    match format {
        "mp3" | "mp3_no_duration" => "mp3",
        "mp4" | "mp4_stereo" | "mp4_he_aac" => "mp4",
        "mkv" | "mkv_aac" | "mkv_padded_duration" => "mkv",
        _ => "wav",
    }
}

fn require_ffmpeg(case: &CorpusCase) {
    if case.requires_ffmpeg && !ffmpeg_util::ffmpeg_available() {
        panic!(
            "case {} requires ffmpeg but ffmpeg was not found on PATH",
            case.id
        );
    }
}

fn write_chirp_pair_wavs(
    dir: &Path,
    case: &CorpusCase,
    sample_rate: u32,
    total_secs: u32,
) -> (PathBuf, PathBuf) {
    match case.generator.as_deref() {
        Some("offset_chirp_pair") => {
            let offset_secs = case.offset_secs.unwrap_or(0);
            let delay_on = match case.delay_on.unwrap_or_default() {
                ChirpDelaySide::B => ChirpDelayOn::B,
                ChirpDelaySide::A => ChirpDelayOn::A,
            };
            write_offset_chirp_wav_pair_with_delay(
                dir,
                sample_rate,
                total_secs,
                offset_secs,
                delay_on,
            )
        }
        Some("two_clip_inconsistent") => {
            let offset_secs = case.offset_secs.unwrap_or(12);
            let tail_silence_secs = case.tail_silence_secs.unwrap_or(18);
            let split_secs = case.clip_length_secs.unwrap_or(60) as u32;
            write_two_clip_inconsistent_pair(
                dir,
                sample_rate,
                total_secs,
                split_secs,
                offset_secs,
                tail_silence_secs,
            )
        }
        Some("piecewise_offset_chirp") => {
            let split_secs = case.clip_length_secs.unwrap_or(60) as u32;
            let offset_start = case.offset_secs.unwrap_or(10);
            let offset_end = case.offset_secs_end.unwrap_or(20);
            write_piecewise_offset_chirp_pair(
                dir,
                sample_rate,
                total_secs,
                split_secs,
                offset_start,
                offset_end,
            )
        }
        Some("near_silence_pair") => write_near_silence_wav_pair(dir, sample_rate, total_secs),
        Some("repeated_segment_pair") => {
            let offset_secs = case.offset_secs.unwrap_or(0);
            write_repeated_segment_wav_pair(dir, sample_rate, total_secs, offset_secs)
        }
        Some("looped_chirp_pair") => {
            let offset_secs = case.offset_secs.unwrap_or(0);
            write_looped_chirp_wav_pair(dir, sample_rate, total_secs, offset_secs)
        }
        Some("query_reference_chirp_pair") => {
            let reference_secs = case
                .total_secs
                .expect("query_reference_chirp_pair needs total_secs");
            let query_anchor_secs = case
                .query_anchor_secs
                .expect("query_reference_chirp_pair needs query_anchor_secs");
            let query_duration_secs = case
                .query_duration_secs
                .expect("query_reference_chirp_pair needs query_duration_secs");
            write_query_reference_chirp_pair(
                dir,
                sample_rate,
                reference_secs,
                query_anchor_secs,
                query_duration_secs,
            )
        }
        Some("query_reference_b_longer_chirp_pair") => {
            let reference_secs = case
                .total_secs
                .expect("query_reference_b_longer_chirp_pair needs total_secs");
            let query_anchor_secs = case
                .query_anchor_secs
                .expect("query_reference_b_longer_chirp_pair needs query_anchor_secs");
            let query_duration_secs = case
                .query_duration_secs
                .expect("query_reference_b_longer_chirp_pair needs query_duration_secs");
            write_query_reference_b_longer_chirp_pair(
                dir,
                sample_rate,
                reference_secs,
                query_anchor_secs,
                query_duration_secs,
            )
        }
        generator => panic!("case {}: unsupported generator {generator:?}", case.id),
    }
}

fn write_source_offset_pair_wavs(
    dir: &Path,
    case: &CorpusCase,
    defaults: &CorpusDefaults,
) -> (PathBuf, PathBuf) {
    let source_id = case
        .source_id
        .as_deref()
        .expect("source_offset_pair case needs source_id");
    let sources = load_sources();
    let source = find_source(&sources, source_id);
    let source_path = source_cache_path(source);
    assert!(
        source_path.is_file(),
        "case {}: missing source {} at {} (run scripts/fetch_corpus_sources.ps1)",
        case.id,
        source_id,
        source_path.display()
    );

    let sample_rate = case.sample_rate.unwrap_or(defaults.target_sample_rate);
    let total_secs = case.total_secs;
    let offset_secs = case.offset_secs.unwrap_or(0);
    let delay_on = case.delay_on.unwrap_or_default();

    let master = dir.join("master.wav");
    assert!(
        corpus_sources::prepare_source_master_wav(&source_path, &master, sample_rate, total_secs),
        "case {}: ffmpeg decode/resample failed for {}",
        case.id,
        source_path.display()
    );

    let wav_a = dir.join("a.wav");
    let wav_b = dir.join("b.wav");
    let delay_ms = offset_secs * 1000;

    match delay_on {
        ChirpDelaySide::B => {
            std::fs::copy(&master, &wav_a).expect("copy master to a");
            assert!(
                ffmpeg_util::delay_wav(&master, &wav_b, delay_ms),
                "case {}: ffmpeg adelay on B failed",
                case.id
            );
        }
        ChirpDelaySide::A => {
            std::fs::copy(&master, &wav_b).expect("copy master to b");
            assert!(
                ffmpeg_util::delay_wav(&master, &wav_a, delay_ms),
                "case {}: ffmpeg adelay on A failed",
                case.id
            );
        }
    }

    (wav_a, wav_b)
}

fn encode_or_rename_pair(
    dir: &Path,
    case: &CorpusCase,
    wav_a: PathBuf,
    wav_b: PathBuf,
) -> (PathBuf, PathBuf) {
    let format = case.format.as_deref().expect("generated case needs format");
    let (path_a, path_b) = if format == "cross_mp3_mp4" {
        (
            dir.join(format!("{}_a.mp3", case.id)),
            dir.join(format!("{}_b.mp4", case.id)),
        )
    } else {
        let ext = extension_for_format(format);
        (
            dir.join(format!("{}_a.{ext}", case.id)),
            dir.join(format!("{}_b.{ext}", case.id)),
        )
    };

    if format == "wav" {
        std::fs::rename(&wav_a, &path_a).expect("rename generated a");
        std::fs::rename(&wav_b, &path_b).expect("rename generated b");
        (path_a, path_b)
    } else if format == "cross_mp3_mp4" {
        assert!(
            ffmpeg_util::encode_audio(&wav_a, &path_a, EncodeFormat::Mp3),
            "case {}: ffmpeg encode a (mp3) failed",
            case.id
        );
        assert!(
            ffmpeg_util::encode_audio(&wav_b, &path_b, EncodeFormat::Mp4Aac),
            "case {}: ffmpeg encode b (mp4) failed",
            case.id
        );
        (path_a, path_b)
    } else if format == "mp4_dual" {
        let total_secs = case.total_secs.unwrap_or(DEFAULT_TOTAL_SECS);
        // Distinct decoy tones so try_all_tracks cannot prefer a decoy↔decoy false match.
        let decoy_a = dir.join("decoy_a.wav");
        let decoy_b = dir.join("decoy_b.wav");
        write_tone_wav_at_frequency(&decoy_a, 11_025, total_secs, 220.0);
        write_tone_wav_at_frequency(&decoy_b, 11_025, total_secs, 330.0);
        let prefer_program = case.prefer_program_track.unwrap_or(true);
        assert!(
            ffmpeg_util::encode_dual_track_mp4(&wav_a, &decoy_a, &path_a, prefer_program),
            "case {}: dual-track encode a failed",
            case.id
        );
        assert!(
            ffmpeg_util::encode_dual_track_mp4(&wav_b, &decoy_b, &path_b, prefer_program),
            "case {}: dual-track encode b failed",
            case.id
        );
        (path_a, path_b)
    } else if format == "mkv_padded_duration" {
        // MKV FLAC where the Matroska container duration is doubled via binary patch.
        // Used for Phase 0 regression anchoring of hold-out placement when container
        // duration exceeds the actual decodable extent (Phase 4 MediaExtent fixes this).
        assert!(
            ffmpeg_util::encode_mkv_with_padded_container_duration(&wav_a, &path_a),
            "case {}: mkv_padded_duration encode a failed (ffmpeg unavailable or patch failed)",
            case.id
        );
        assert!(
            ffmpeg_util::encode_mkv_with_padded_container_duration(&wav_b, &path_b),
            "case {}: mkv_padded_duration encode b failed (ffmpeg unavailable or patch failed)",
            case.id
        );
        (path_a, path_b)
    } else {
        let encode = parse_encode_format(format)
            .unwrap_or_else(|| panic!("case {}: unknown format {format}", case.id));
        assert!(
            ffmpeg_util::encode_audio(&wav_a, &path_a, encode),
            "case {}: ffmpeg encode a failed",
            case.id
        );
        assert!(
            ffmpeg_util::encode_audio(&wav_b, &path_b, encode),
            "case {}: ffmpeg encode b failed",
            case.id
        );
        (path_a, path_b)
    }
}

pub fn generate_case_pair(case: &CorpusCase, defaults: &CorpusDefaults) -> GeneratedCasePaths {
    require_ffmpeg(case);

    let total_secs = case.total_secs.unwrap_or(DEFAULT_TOTAL_SECS);
    let sample_rate = case.sample_rate.unwrap_or(defaults.target_sample_rate);

    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path();

    let (wav_a, wav_b) = if case.generator.as_deref() == Some("source_offset_pair") {
        write_source_offset_pair_wavs(dir, case, defaults)
    } else {
        write_chirp_pair_wavs(dir, case, sample_rate, total_secs)
    };
    let (path_a, path_b) = encode_or_rename_pair(dir, case, wav_a, wav_b);

    GeneratedCasePaths {
        _temp: temp,
        video_a: path_a,
        video_b: path_b,
    }
}

pub fn resolve_case_paths(
    case: &CorpusCase,
    defaults: &CorpusDefaults,
) -> (Option<GeneratedCasePaths>, PathBuf, PathBuf) {
    match case.tier {
        CorpusTier::Committed => {
            let (a, b) = resolve_committed_pair(case);
            (None, a, b)
        }
        CorpusTier::Generated => {
            let generated = generate_case_pair(case, defaults);
            let a = generated.video_a.clone();
            let b = generated.video_b.clone();
            (Some(generated), a, b)
        }
        CorpusTier::External => {
            let base = std::env::var("CLIP_SYNC_CORPUS")
                .unwrap_or_else(|_| panic!("case {}: CLIP_SYNC_CORPUS not set", case.id));
            let generated = generate_into(Path::new(&base), case, defaults);
            let a = generated.video_a.clone();
            let b = generated.video_b.clone();
            (Some(generated), a, b)
        }
    }
}

fn generate_into(base: &Path, case: &CorpusCase, defaults: &CorpusDefaults) -> GeneratedCasePaths {
    let dir = base.join(&case.id);
    std::fs::create_dir_all(&dir).expect("create external case dir");
    let temp = tempfile::tempdir_in(dir).expect("tempdir");
    let mut paths = generate_case_pair(case, defaults);
    let case_dir = temp.path();
    let a = case_dir.join(paths.video_a.file_name().expect("generated a filename"));
    let b = case_dir.join(paths.video_b.file_name().expect("generated b filename"));
    std::fs::rename(&paths.video_a, &a).expect("move external a");
    std::fs::rename(&paths.video_b, &b).expect("move external b");
    paths._temp = temp;
    paths.video_a = a;
    paths.video_b = b;
    paths
}

pub fn build_config(case: &CorpusCase, defaults: &CorpusDefaults) -> AlignConfig {
    let clip_length_secs = case.clip_length_secs.unwrap_or(defaults.clip_length_secs);
    let num_clips = case.num_clips.unwrap_or(defaults.num_clips);

    let mut config = AlignConfig {
        clip: ClipConfig {
            clip_length: Duration::from_secs(clip_length_secs),
            num_clips,
            target_sample_rate: Some(defaults.target_sample_rate),
            // Synthetic fixtures encode offset as leading silence; trimming would erase it.
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
            ..ClipConfig::default()
        },
        ..Default::default()
    };

    if let Some(require_consistent_offsets) = case.require_consistent_offsets {
        config.alignment.require_consistent_offsets = require_consistent_offsets;
    }
    if let Some(try_all_tracks) = case.try_all_tracks {
        config.alignment.try_all_tracks = try_all_tracks;
    }
    if let Some(refine_offset_with_pcm) = case.refine_offset_with_pcm {
        config.alignment.refine_offset_with_pcm = refine_offset_with_pcm;
    }
    if let Some(refine_offset_high_rate) = case.refine_offset_high_rate {
        config.alignment.refine_offset_high_rate = refine_offset_high_rate;
    }
    if case.check_clip_repetition {
        config.validation.check_clip_repetition = true;
    }
    if case.verify_offset {
        config.validation.verify_offset = true;
    }
    if let Some(mode) = &case.alignment_mode {
        config.alignment.mode = match mode.as_str() {
            "symmetric" => AlignmentMode::Symmetric,
            "queryreference" | "query_reference" => AlignmentMode::QueryReference,
            _ => AlignmentMode::Auto,
        };
    }

    config
}

pub fn run_corpus_case_with_config<MR, FP, AL>(
    use_case: &AlignVideos<'_, MR, FP, AL>,
    video_a: PathBuf,
    video_b: PathBuf,
    config: AlignConfig,
) -> Result<AlignmentResult, AppError>
where
    MR: MediaReader,
    FP: Fingerprinter,
    AL: Aligner,
{
    let response = use_case.execute(AlignVideosRequest {
        video_a,
        video_b,
        config,
    })?;
    Ok(response.result)
}

pub fn run_corpus_case<MR, FP, AL>(
    use_case: &AlignVideos<'_, MR, FP, AL>,
    case: &CorpusCase,
    defaults: &CorpusDefaults,
    video_a: PathBuf,
    video_b: PathBuf,
) -> Result<AlignmentResult, AppError>
where
    MR: MediaReader,
    FP: Fingerprinter,
    AL: Aligner,
{
    run_corpus_case_with_config(use_case, video_a, video_b, build_config(case, defaults))
}

/// Logs recommended offset vs oracle for third-party source cases (`--nocapture`).
pub fn log_source_offset_precision(
    case: &CorpusCase,
    defaults: &CorpusDefaults,
    result: &AlignmentResult,
) {
    if !case.requires_source {
        return;
    }
    let Some(expected) = case.expected_offset_secs else {
        return;
    };
    let tolerance_ms = case.tolerance_secs.unwrap_or(defaults.tolerance_secs) * 1000.0;
    match result.recommended_offset_secs {
        Some(actual) => {
            let error_ms = (actual - expected).abs() * 1000.0;
            eprintln!(
                "case {}: offset error {error_ms:.1} ms (actual {actual:.6}s, expected {expected:.6}s, tolerance ±{tolerance_ms:.0} ms)",
                case.id
            );
        }
        None => eprintln!("case {}: offset error n/a (no recommendation)", case.id),
    }
    if case.verify_offset {
        if let Some(verify) = &result.offset_verification {
            eprintln!(
                "case {}: verify verified={} confidence={:.3}",
                case.id, verify.verified, verify.confidence
            );
        }
    }
}

pub fn assert_corpus_expectations(
    case: &CorpusCase,
    defaults: &CorpusDefaults,
    result: &AlignmentResult,
) {
    if let Some(expect_aligned) = case.expect_aligned {
        assert_eq!(
            result.start_aligned, expect_aligned,
            "case {}: start_aligned",
            case.id
        );
    }

    if let Some(expect_recommended) = case.expect_recommended {
        if expect_recommended {
            assert!(
                result.recommended_offset_secs.is_some(),
                "case {}: expected recommended offset",
                case.id
            );
        } else {
            assert_eq!(
                result.recommended_offset_secs, None,
                "case {}: expected no recommended offset",
                case.id
            );
        }
    }

    if let Some(expected_offset) = case.expected_offset_secs {
        let actual = result
            .recommended_offset_secs
            .unwrap_or_else(|| panic!("case {}: missing recommended offset", case.id));
        let tolerance = case.tolerance_secs.unwrap_or(defaults.tolerance_secs);
        assert!(
            (actual - expected_offset).abs() <= tolerance,
            "case {}: offset {actual}, expected {expected_offset} ± {tolerance}",
            case.id,
        );
    }

    if let Some(expect_offsets_consistent) = case.expect_offsets_consistent {
        assert_eq!(
            result.offsets_consistent, expect_offsets_consistent,
            "case {}: offsets_consistent",
            case.id
        );
    }

    if case.expect_aligned == Some(true) {
        let confidence = result
            .start_clip()
            .or_else(|| result.clips.first())
            .map(|clip| clip.confidence)
            .unwrap_or(0.0);
        assert!(
            confidence >= defaults.min_confidence,
            "case {}: confidence {confidence} below {}",
            case.id,
            defaults.min_confidence
        );
    }

    if let Some(expect_verified) = case.expect_offset_verified {
        let verify = result
            .offset_verification
            .as_ref()
            .unwrap_or_else(|| panic!("case {}: expected offset_verification", case.id));
        assert_eq!(
            verify.verified, expect_verified,
            "case {}: offset_verification={verify:?} recommended_offset={:?}",
            case.id, result.recommended_offset_secs
        );
        if expect_verified {
            assert!(
                !verify.skipped,
                "case {}: verified hold-out must not be skipped: {:?}",
                case.id, verify.skip_reason
            );
            assert!(
                verify.confidence >= defaults.min_confidence,
                "case {}: verification confidence {} below {}",
                case.id,
                verify.confidence,
                defaults.min_confidence
            );
        }
    }

    if case.alignment_mode.as_deref() == Some("queryreference")
        || case.alignment_mode.as_deref() == Some("query_reference")
    {
        assert_eq!(
            result.alignment_mode_used,
            Some(AlignmentModeUsed::QueryReference),
            "case {}: alignment_mode_used",
            case.id
        );
        assert!(
            result.query_localization.is_some(),
            "case {}: expected query_localization",
            case.id
        );
    }

    if let Some(expect_clip) = case.expect_clip_on_a_start_secs {
        let loc = result
            .query_localization
            .as_ref()
            .unwrap_or_else(|| panic!("case {}: expected query_localization", case.id));
        let tolerance = case.tolerance_secs.unwrap_or(defaults.tolerance_secs);
        let expect_anchor = case.expect_anchor_on_reference_secs.unwrap_or(expect_clip);
        assert!(
            (loc.anchor_ref_secs - expect_anchor).abs() <= tolerance,
            "case {}: anchor_ref_secs {} expected {expect_anchor} ± {tolerance}",
            case.id,
            loc.anchor_ref_secs
        );
        assert!(
            (loc.clip_on_a_start_secs - expect_clip).abs() <= tolerance,
            "case {}: clip_on_a_start_secs {} expected {expect_clip} ± {tolerance}",
            case.id,
            loc.clip_on_a_start_secs
        );
        if let Some(expected_offset) = case.expected_offset_secs {
            let actual = result
                .recommended_offset_secs
                .unwrap_or_else(|| panic!("case {}: missing recommended offset", case.id));
            assert!(
                (actual - expected_offset).abs() <= tolerance,
                "case {}: recommended offset {actual}, expected {expected_offset} ± {tolerance}",
                case.id
            );
        }
    }

    if let Some(expect) = case.expect_clip_repetition {
        let rep = result
            .start_clip()
            .or_else(|| result.clips.first())
            .and_then(|clip| clip.repetition.as_ref());
        if expect {
            let report = rep.unwrap_or_else(|| {
                panic!("case {}: expected repetition report on start clip", case.id)
            });
            let finding = report.a.as_ref().or(report.b.as_ref()).unwrap_or_else(|| {
                panic!(
                    "case {}: expected at least one repetition finding on start clip",
                    case.id
                )
            });
            assert!(
                (28.0_f64..=32.0).contains(&finding.lag_secs),
                "case {}: lag_secs={} expected in [28, 32]",
                case.id,
                finding.lag_secs,
            );
            assert!(
                finding.confidence >= 0.5,
                "case {}: repetition confidence={} below 0.5",
                case.id,
                finding.confidence,
            );
        } else {
            let has_finding = rep.is_some_and(|r| r.a.is_some() || r.b.is_some());
            assert!(
                !has_finding,
                "case {}: expected no repetition finding",
                case.id
            );
        }
        if expect {
            assert!(
                result.offset_ambiguous_mod_secs.is_some(),
                "case {}: expected offset_ambiguous_mod_secs when repetition finding present",
                case.id
            );
        }
    }
}

/// Writes Tier-B WAV fixtures under `tests/corpus/wav/`.
pub fn write_committed_wav_fixtures() {
    let wav_dir = corpus_root().join("wav");
    std::fs::create_dir_all(&wav_dir).expect("create wav dir");

    let (baseline_a, baseline_b) =
        write_offset_chirp_wav_pair(&wav_dir, DEFAULT_SAMPLE_RATE, DEFAULT_TOTAL_SECS, 0);
    std::fs::rename(&baseline_a, wav_dir.join("baseline_0s_a.wav")).expect("rename baseline a");
    std::fs::rename(&baseline_b, wav_dir.join("baseline_0s_b.wav")).expect("rename baseline b");

    let (leader_a, leader_b) =
        write_offset_chirp_wav_pair(&wav_dir, DEFAULT_SAMPLE_RATE, DEFAULT_TOTAL_SECS, 3);
    std::fs::rename(&leader_a, wav_dir.join("leader_3s_a.wav")).expect("rename leader a");
    std::fs::rename(&leader_b, wav_dir.join("leader_3s_b.wav")).expect("rename leader b");

    let (chirp_temp_a, chirp_temp_b) =
        write_offset_chirp_wav_pair(&wav_dir, DEFAULT_SAMPLE_RATE, NEGATIVE_CASE_SECS, 0);
    std::fs::rename(&chirp_temp_a, wav_dir.join("chirp_a.wav")).expect("rename chirp a");
    let _ = std::fs::remove_file(chirp_temp_b);

    write_tone_wav(
        &wav_dir.join("tone_b.wav"),
        DEFAULT_SAMPLE_RATE,
        NEGATIVE_CASE_SECS,
    );
}

/// Runs hold-out verification with a deliberately wrong recommended offset (Option A probe).
#[cfg(test)]
pub fn run_wrong_offset_verification_probe(
    path_a: &Path,
    path_b: &Path,
    wrong_offset_secs: f64,
    clip_length_secs: u64,
    target_sample_rate: u32,
) -> crate::domain::OffsetVerification {
    use std::time::Duration;

    use crate::application::config::{
        AlignConfig, AlignmentConfig, ChromaprintPreset, ClipConfig, ValidationConfig,
    };
    use crate::application::offset_verification::{
        apply_offset_verification, OffsetVerificationDeps, OffsetVerificationInput,
    };
    use crate::application::ports::{MediaReader, MediaSession};
    use crate::application::testing::fakes::FakeProgressReporter;
    use crate::domain::{AlignmentResult, ClipLabel, ClipWindow, MediaExtent, MediaSource};
    use crate::infrastructure::chromaprint::{
        ChromaprintAligner, ChromaprintClipRepetitionDetector, ChromaprintFingerprinter,
    };
    use crate::infrastructure::symphonia::SymphoniaMediaReader;

    let media_reader = SymphoniaMediaReader;
    let preset = ChromaprintPreset::default();
    let fingerprinter = ChromaprintFingerprinter::new(preset);
    let aligner = ChromaprintAligner::new(preset);
    let progress = FakeProgressReporter;

    let mut session_a = media_reader
        .open(&MediaSource::new(path_a))
        .expect("open a");
    let mut session_b = media_reader
        .open(&MediaSource::new(path_b))
        .expect("open b");
    let tracks_a = session_a.list_tracks().expect("tracks a");
    let tracks_b = session_b.list_tracks().expect("tracks b");
    let track_a = &tracks_a[0];
    let track_b = &tracks_b[0];
    let duration = track_a.duration.expect("duration");

    let mut result = AlignmentResult {
        clips: vec![],
        start_aligned: true,
        end_aligned: None,
        recommended_offset_secs: Some(wrong_offset_secs),
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap: None,
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: Some(10.0),
        alignment_mode_used: None,
        query_localization: None,
        end_clip_anchor: None,
    };

    let clip_config = ClipConfig {
        clip_length: Duration::from_secs(clip_length_secs),
        num_clips: 1,
        target_sample_rate: Some(target_sample_rate),
        normalize_loudness: false,
        trim_silence: false,
        window_slide_secs: 0,
        ..ClipConfig::default()
    };
    let config = AlignConfig {
        clip: clip_config,
        validation: ValidationConfig {
            verify_offset: true,
            min_verification_confidence: 0.5,
            check_clip_repetition: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let discovery_windows = vec![ClipWindow::new(
        Duration::ZERO,
        Duration::from_secs(30),
        ClipLabel::Start,
    )];
    let alignment = AlignmentConfig::default();
    let extent = MediaExtent::from_declared(duration);

    apply_offset_verification(
        &mut OffsetVerificationInput {
            session_a: &mut session_a,
            session_b: &mut session_b,
            track_a,
            track_b,
            discovery_windows: &discovery_windows,
            extent_a: extent,
            extent_b: extent,
            min_holdout_decode_fraction: alignment.min_end_clip_decode_fraction,
            max_holdout_decode_skips: alignment.max_end_clip_decode_skips,
            resampler: &crate::infrastructure::resample::RubatoResampler,
            correlator: &crate::infrastructure::correlation::FftCorrelator,
        },
        &config,
        &mut result,
        &OffsetVerificationDeps {
            fingerprinter: &fingerprinter,
            aligner: &aligner,
            repetition_detector: &ChromaprintClipRepetitionDetector,
        },
        &progress,
    );

    result
        .offset_verification
        .unwrap_or_else(|| panic!("offset_verification must be set for wrong-offset probe"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::config::ChromaprintPreset;
    use crate::application::testing::fakes::FakeProgressReporter;
    use crate::infrastructure::chromaprint::{
        ChromaprintAligner, ChromaprintClipRepetitionDetector, ChromaprintFingerprinter,
    };
    use crate::infrastructure::symphonia::SymphoniaMediaReader;

    #[test]
    #[ignore = "run manually: cargo test regenerate_committed_wav_fixtures -- --ignored --nocapture"]
    fn regenerate_committed_wav_fixtures() {
        write_committed_wav_fixtures();
        eprintln!("wrote fixtures under {}", corpus_root().display());
    }

    #[test]
    fn corpus_manifest_loads() {
        let manifest = load_manifest();
        assert!(manifest.version >= 1);
        assert!(!manifest.case.is_empty());
    }

    #[test]
    #[ignore = "optional CC sources; run scripts/fetch_corpus_sources.ps1 first"]
    fn corpus_source_cases() {
        let manifest = load_manifest();
        let source_cases: Vec<_> = manifest
            .case
            .iter()
            .filter(|case| case.requires_source && !case.ignore && !case.probe_only)
            .collect();
        if source_cases.is_empty() {
            return;
        }
        if !ffmpeg_util::ffmpeg_available() {
            eprintln!("skipping corpus_source_cases: ffmpeg unavailable");
            return;
        }
        let ready = source_cases.iter().all(|case| {
            case.source_id
                .as_deref()
                .map(corpus_sources::source_ready)
                .unwrap_or(false)
        });
        if !ready {
            eprintln!(
                "skipping corpus_source_cases: run scripts/fetch_corpus_sources.ps1 (see tests/corpus/README.md)"
            );
            return;
        }
        run_manifest_cases(CorpusTier::Generated, true);
    }

    fn run_manifest_cases(tier: CorpusTier, source_only: bool) {
        let manifest = load_manifest();
        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &crate::infrastructure::resample::RubatoResampler,
            &crate::infrastructure::correlation::FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

        for case in manifest.case.iter().filter(|case| {
            case.tier == tier
                && !case.ignore
                && !case.probe_only
                && case.requires_source == source_only
        }) {
            if case.requires_ffmpeg && !ffmpeg_util::ffmpeg_available() {
                eprintln!("skipping case {}: ffmpeg unavailable", case.id);
                continue;
            }
            if case.requires_he_aac && !cfg!(feature = "he-aac") {
                eprintln!("skipping case {}: he-aac feature not enabled", case.id);
                continue;
            }

            let started = std::time::Instant::now();
            let (_guard, video_a, video_b) = resolve_case_paths(case, &manifest.defaults);

            if tier == CorpusTier::Committed {
                assert!(
                    video_a.is_file(),
                    "case {}: missing {}",
                    case.id,
                    video_a.display()
                );
                assert!(
                    video_b.is_file(),
                    "case {}: missing {}",
                    case.id,
                    video_b.display()
                );
            }

            if case.compare_refine_pcm {
                for refine in [true, false] {
                    let mut config = build_config(case, &manifest.defaults);
                    config.alignment.refine_offset_with_pcm = refine;
                    let result = run_corpus_case_with_config(
                        &use_case,
                        video_a.clone(),
                        video_b.clone(),
                        config,
                    )
                    .unwrap_or_else(|error| {
                        panic!("case {} (refine={refine}) failed: {error}", case.id)
                    });
                    assert_corpus_expectations(case, &manifest.defaults, &result);
                    log_source_offset_precision(case, &manifest.defaults, &result);
                }
            } else {
                let result = run_corpus_case(&use_case, case, &manifest.defaults, video_a, video_b)
                    .unwrap_or_else(|error| panic!("case {} failed: {error}", case.id));

                assert_corpus_expectations(case, &manifest.defaults, &result);
                log_source_offset_precision(case, &manifest.defaults, &result);
            }

            let elapsed = started.elapsed();
            eprintln!("case {}: {:.2?}", case.id, elapsed);

            if let Some(max_wall_secs) = case.max_wall_secs {
                assert!(
                    elapsed.as_secs_f64() <= max_wall_secs,
                    "case {}: wall time {:.2?} exceeds {:.1}s budget",
                    case.id,
                    elapsed,
                    max_wall_secs
                );
            }
        }
    }

    #[test]
    fn corpus_committed_cases() {
        run_manifest_cases(CorpusTier::Committed, false);
    }

    #[test]
    fn corpus_verify_offset_pass() {
        let manifest = load_manifest();
        let case = manifest
            .case
            .iter()
            .find(|entry| entry.id == "verify_offset_pass")
            .expect("verify_offset_pass case in manifest");
        assert!(
            case.verify_offset && case.expect_offset_verified == Some(true),
            "manifest must request verification"
        );

        let paths = generate_case_pair(case, &manifest.defaults);
        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &crate::infrastructure::resample::RubatoResampler,
            &crate::infrastructure::correlation::FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
        let result = run_corpus_case(
            &use_case,
            case,
            &manifest.defaults,
            paths.video_a,
            paths.video_b,
        )
        .unwrap_or_else(|error| panic!("verify_offset_pass failed: {error}"));
        assert_corpus_expectations(case, &manifest.defaults, &result);
    }

    #[test]
    fn corpus_verify_option_a_false_pass_probe() {
        let manifest = load_manifest();
        let case = manifest
            .case
            .iter()
            .find(|entry| entry.id == "verify_option_a_false_pass_probe")
            .expect("verify_option_a_false_pass_probe case in manifest");
        let wrong_offset = case
            .probe_wrong_verification_offset_secs
            .expect("manifest must set probe_wrong_verification_offset_secs");
        let clip_length_secs = case
            .clip_length_secs
            .unwrap_or(manifest.defaults.clip_length_secs);
        let target_sample_rate = case
            .sample_rate
            .unwrap_or(manifest.defaults.target_sample_rate);

        let paths = generate_case_pair(case, &manifest.defaults);

        const LOOP_PERIOD_SECS: f64 = 10.0;
        let true_offset_secs = f64::from(case.offset_secs.unwrap_or(3));

        // Wrong Δ not equivalent to true offset mod loop period (e.g. +8, +18 ≡ +8 mod 10).
        for probe_offset in [wrong_offset, wrong_offset + LOOP_PERIOD_SECS] {
            let verify = run_wrong_offset_verification_probe(
                &paths.video_a,
                &paths.video_b,
                probe_offset,
                clip_length_secs,
                target_sample_rate,
            );
            assert!(
                !verify.skipped,
                "probe Δ={probe_offset}: verification should run, got skip_reason={:?}",
                verify.skip_reason
            );
            assert!(
                !verify.verified,
                "non-period wrong Δ={probe_offset}: verified=true confidence={} \
                 (see docs/dev/corpus-validation.md § Option A false-pass evidence)",
                verify.confidence
            );
        }

        // Period-equivalent alias: true offset + N×loop period (discovery often picks +13 s here).
        let period_alias_offset = true_offset_secs + LOOP_PERIOD_SECS;
        let alias_verify = run_wrong_offset_verification_probe(
            &paths.video_a,
            &paths.video_b,
            period_alias_offset,
            clip_length_secs,
            target_sample_rate,
        );
        assert!(
            !alias_verify.skipped,
            "period-alias probe Δ={period_alias_offset}: verification should run, got skip_reason={:?}",
            alias_verify.skip_reason
        );
        assert!(
            !alias_verify.verified,
            "period-equivalent wrong Δ={period_alias_offset} must not report verified=true \
             (confidence={}, inconclusive={}); see docs/dev/corpus-validation.md",
            alias_verify.confidence, alias_verify.verify_inconclusive
        );
        assert!(
            alias_verify.verify_inconclusive,
            "period alias should set verify_inconclusive"
        );
        assert!(
            alias_verify
                .independent_offset_secs
                .is_some_and(|o| (o - true_offset_secs).abs() < 1.5),
            "parallel recheck should recover true offset ~{true_offset_secs}, got {:?}",
            alias_verify.independent_offset_secs
        );
    }

    #[test]
    fn corpus_looped_discovery_alias_sets_ambiguity_flag() {
        use crate::application::config::ChromaprintPreset;
        use crate::application::testing::fakes::FakeProgressReporter;
        use crate::infrastructure::chromaprint::{
            ChromaprintAligner, ChromaprintClipRepetitionDetector, ChromaprintFingerprinter,
        };
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        let manifest = load_manifest();
        let case = manifest
            .case
            .iter()
            .find(|entry| entry.id == "verify_option_a_false_pass_probe")
            .expect("verify_option_a_false_pass_probe case in manifest");
        let paths = generate_case_pair(case, &manifest.defaults);

        let mut config = build_config(case, &manifest.defaults);
        config.validation.check_clip_repetition = true;
        config.validation.verify_offset = true;

        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &crate::infrastructure::resample::RubatoResampler,
            &crate::infrastructure::correlation::FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
        let result = run_corpus_case_with_config(&use_case, paths.video_a, paths.video_b, config)
            .expect("align looped pair");

        assert!(
            result
                .offset_ambiguous_mod_secs
                .is_some_and(|t| (t - 10.0).abs() < 3.0),
            "strong repetition should set fundamental period ~10s, got {:?}",
            result.offset_ambiguous_mod_secs
        );
        let recommended = result.recommended_offset_secs.expect("recommended offset");
        assert!(
            (recommended - 13.0).abs() < 2.0 || (recommended - 3.0).abs() < 1.5,
            "discovery expected ~+13s alias or +3s true, got {recommended}"
        );
        let verify = result.offset_verification.expect("verify should run");
        assert!(!verify.verified, "verify must not pass on period alias");
        if (recommended - 13.0).abs() < 2.0 {
            assert!(
                verify.verify_inconclusive,
                "period alias discovery should mark verify inconclusive"
            );
        }
    }

    #[test]
    #[ignore = "slow: generated corpus + ffmpeg; cargo test corpus_mkv_tail_decodable_extent_gap -- --ignored"]
    fn corpus_mkv_tail_decodable_extent_gap() {
        let manifest = load_manifest();
        let case = manifest
            .case
            .iter()
            .find(|entry| entry.id == "mkv_tail_decodable_extent_gap")
            .expect("mkv_tail_decodable_extent_gap case in manifest");
        assert!(
            case.verify_offset && case.expect_offset_verified == Some(true),
            "manifest must request verified hold-out on decodable extent"
        );

        let paths = generate_case_pair(case, &manifest.defaults);
        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &crate::infrastructure::resample::RubatoResampler,
            &crate::infrastructure::correlation::FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
        let result = run_corpus_case(
            &use_case,
            case,
            &manifest.defaults,
            paths.video_a,
            paths.video_b,
        )
        .unwrap_or_else(|error| panic!("mkv_tail_decodable_extent_gap failed: {error}"));
        assert_corpus_expectations(case, &manifest.defaults, &result);
    }

    #[test]
    fn corpus_repeated_segment_sets_ambiguity_flag() {
        let manifest = load_manifest();
        let case = manifest
            .case
            .iter()
            .find(|entry| entry.id == "repeated_segment_in_clip")
            .expect("repeated_segment_in_clip case in manifest");
        assert!(
            case.check_clip_repetition && case.expect_clip_repetition == Some(true),
            "manifest must request repetition on repeated_segment_in_clip"
        );

        let paths = generate_case_pair(case, &manifest.defaults);
        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &crate::infrastructure::resample::RubatoResampler,
            &crate::infrastructure::correlation::FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
        let result = run_corpus_case(
            &use_case,
            case,
            &manifest.defaults,
            paths.video_a,
            paths.video_b,
        )
        .unwrap_or_else(|error| panic!("repeated_segment_in_clip failed: {error}"));

        assert!(
            result.offset_ambiguous_mod_secs.is_some(),
            "repeated segment pair should set offset_ambiguous_mod_secs, got {:?}",
            result.offset_ambiguous_mod_secs
        );
        assert!(
            result.start_aligned,
            "repeated_segment_in_clip should align"
        );
        let actual = result
            .recommended_offset_secs
            .expect("repeated_segment_in_clip should recommend offset");
        let tolerance = case
            .tolerance_secs
            .unwrap_or(manifest.defaults.tolerance_secs);
        assert!(
            (actual
                - case
                    .expected_offset_secs
                    .expect("expected offset in manifest"))
            .abs()
                <= tolerance,
            "offset {actual}, expected {} ± {tolerance}",
            case.expected_offset_secs.unwrap()
        );
        let report = result
            .start_clip()
            .and_then(|clip| clip.repetition.as_ref())
            .expect("repeated_segment_in_clip: repetition report on start clip");
        let finding = report
            .a
            .as_ref()
            .or(report.b.as_ref())
            .expect("repeated_segment_in_clip: repetition finding");
        assert!(
            (28.0..=32.0).contains(&finding.lag_secs),
            "lag_secs={} expected in [28, 32]",
            finding.lag_secs
        );
    }

    #[test]
    #[ignore = "slow: 60 min query-reference generated case; cargo test corpus_query_reference_45min_anchor -- --ignored"]
    fn corpus_query_reference_45min_anchor() {
        let manifest = load_manifest();
        let case = manifest
            .case
            .iter()
            .find(|c| c.id == "wav_query_reference_45min_anchor")
            .expect("manifest case wav_query_reference_45min_anchor");
        assert_eq!(case.tier, CorpusTier::Generated);

        let paths = generate_case_pair(case, &manifest.defaults);
        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &crate::infrastructure::resample::RubatoResampler,
            &crate::infrastructure::correlation::FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
        let result = run_corpus_case(
            &use_case,
            case,
            &manifest.defaults,
            paths.video_a,
            paths.video_b,
        )
        .expect("query-reference corpus case should succeed");
        assert_corpus_expectations(case, &manifest.defaults, &result);
    }

    #[test]
    fn corpus_query_reference_b_longer_fast() {
        let manifest = load_manifest();
        let case = manifest
            .case
            .iter()
            .find(|c| c.id == "wav_query_reference_b_longer_fast")
            .expect("manifest case wav_query_reference_b_longer_fast");
        assert_eq!(case.tier, CorpusTier::Generated);

        let paths = generate_case_pair(case, &manifest.defaults);
        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &crate::infrastructure::resample::RubatoResampler,
            &crate::infrastructure::correlation::FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
        let result = run_corpus_case(
            &use_case,
            case,
            &manifest.defaults,
            paths.video_a,
            paths.video_b,
        )
        .expect("fast B-longer query-reference corpus case should succeed");
        assert_corpus_expectations(case, &manifest.defaults, &result);
    }

    #[test]
    #[ignore = "slow: 60 min B-longer query-reference generated case; cargo test corpus_query_reference_b_longer_anchor -- --ignored"]
    fn corpus_query_reference_b_longer_anchor() {
        let manifest = load_manifest();
        let case = manifest
            .case
            .iter()
            .find(|c| c.id == "wav_query_reference_b_longer_anchor")
            .expect("manifest case wav_query_reference_b_longer_anchor");
        assert_eq!(case.tier, CorpusTier::Generated);

        let paths = generate_case_pair(case, &manifest.defaults);
        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &crate::infrastructure::resample::RubatoResampler,
            &crate::infrastructure::correlation::FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
        let result = run_corpus_case(
            &use_case,
            case,
            &manifest.defaults,
            paths.video_a,
            paths.video_b,
        )
        .expect("B-longer query-reference corpus case should succeed");
        assert_corpus_expectations(case, &manifest.defaults, &result);
    }

    #[test]
    fn corpus_two_clip_inconsistent_blocks_recommendation() {
        let manifest = load_manifest();
        let case = manifest
            .case
            .iter()
            .find(|c| c.id == "two_clip_inconsistent")
            .expect("case");
        let (_guard, video_a, video_b) = resolve_case_paths(case, &manifest.defaults);
        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = crate::application::AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &crate::infrastructure::resample::RubatoResampler,
            &crate::infrastructure::correlation::FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
        let result =
            run_corpus_case(&use_case, case, &manifest.defaults, video_a, video_b).expect("align");
        assert_corpus_expectations(case, &manifest.defaults, &result);
    }

    #[test]
    #[ignore = "slow: generated corpus + ffmpeg; cargo test corpus_generated -- --ignored"]
    fn corpus_generated_cases() {
        run_manifest_cases(CorpusTier::Generated, false);
    }

    #[test]
    #[ignore = "long smoke; set CLIP_SYNC_CORPUS to a persistent directory"]
    fn corpus_external_cases() {
        if std::env::var("CLIP_SYNC_CORPUS").is_err() {
            eprintln!("skipping corpus_external_cases: CLIP_SYNC_CORPUS not set");
            return;
        }
        run_manifest_cases(CorpusTier::External, false);
    }
}
