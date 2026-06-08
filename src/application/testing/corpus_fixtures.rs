use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use tempfile::TempDir;

use crate::application::align_videos::{AlignVideos, AlignVideosRequest};
use crate::application::config::{AppConfig, ClipConfig};
use crate::application::error::AppError;
use crate::application::ports::{Aligner, Fingerprinter, MediaReader, ProgressReporter};
use crate::application::testing::audio_fixtures::{
    write_near_silence_wav_pair, write_offset_chirp_wav_pair,
    write_offset_chirp_wav_pair_with_delay, write_piecewise_offset_chirp_pair, write_tone_wav,
    write_tone_wav_at_frequency, write_two_clip_inconsistent_pair, ChirpDelayOn,
};
use crate::application::testing::ffmpeg_util::{self, EncodeFormat};
use crate::domain::AlignmentResult;

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
    #[allow(dead_code)]
    pub expect_exit_code: Option<u32>,
    #[serde(default)]
    pub requires_ffmpeg: bool,
    #[serde(default)]
    pub requires_he_aac: bool,
    #[serde(default)]
    pub max_wall_secs: Option<f64>,
    #[serde(default)]
    pub ignore: bool,
}

pub struct GeneratedCasePaths {
    pub _temp: TempDir,
    pub video_a: PathBuf,
    pub video_b: PathBuf,
}

pub fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("corpus")
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
        "mkv" => "mkv",
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
            write_two_clip_inconsistent_pair(
                dir,
                sample_rate,
                total_secs,
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
        generator => panic!("case {}: unsupported generator {generator:?}", case.id),
    }
}

fn encode_or_rename_pair(
    dir: &Path,
    case: &CorpusCase,
    wav_a: PathBuf,
    wav_b: PathBuf,
) -> (PathBuf, PathBuf) {
    let format = case
        .format
        .as_deref()
        .expect("generated case needs format");
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

    let (wav_a, wav_b) = write_chirp_pair_wavs(dir, case, sample_rate, total_secs);
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
    let a = case_dir.join(
        paths
            .video_a
            .file_name()
            .expect("generated a filename"),
    );
    let b = case_dir.join(
        paths
            .video_b
            .file_name()
            .expect("generated b filename"),
    );
    std::fs::rename(&paths.video_a, &a).expect("move external a");
    std::fs::rename(&paths.video_b, &b).expect("move external b");
    paths._temp = temp;
    paths.video_a = a;
    paths.video_b = b;
    paths
}

pub fn build_config(case: &CorpusCase, defaults: &CorpusDefaults) -> AppConfig {
    let clip_length_secs = case
        .clip_length_secs
        .unwrap_or(defaults.clip_length_secs);
    let num_clips = case.num_clips.unwrap_or(defaults.num_clips);

    let mut config = AppConfig {
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

    config
}

pub fn run_corpus_case_with_config<MR, FP, AL, PR>(
    use_case: &AlignVideos<'_, MR, FP, AL, PR>,
    video_a: PathBuf,
    video_b: PathBuf,
    config: AppConfig,
) -> Result<AlignmentResult, AppError>
where
    MR: MediaReader,
    FP: Fingerprinter,
    AL: Aligner,
    PR: ProgressReporter,
{
    let response = use_case.execute(AlignVideosRequest {
        video_a,
        video_b,
        config,
    })?;
    Ok(response.result)
}

pub fn run_corpus_case<MR, FP, AL, PR>(
    use_case: &AlignVideos<'_, MR, FP, AL, PR>,
    case: &CorpusCase,
    defaults: &CorpusDefaults,
    video_a: PathBuf,
    video_b: PathBuf,
) -> Result<AlignmentResult, AppError>
where
    MR: MediaReader,
    FP: Fingerprinter,
    AL: Aligner,
    PR: ProgressReporter,
{
    run_corpus_case_with_config(
        use_case,
        video_a,
        video_b,
        build_config(case, defaults),
    )
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
        let tolerance = case
            .tolerance_secs
            .unwrap_or(defaults.tolerance_secs);
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
            .clips
            .first()
            .map(|clip| clip.confidence)
            .unwrap_or(0.0);
        assert!(
            confidence >= defaults.min_confidence,
            "case {}: confidence {confidence} below {}",
            case.id,
            defaults.min_confidence
        );
    }
}

/// Writes Tier-B WAV fixtures under `tests/corpus/wav/`.
pub fn write_committed_wav_fixtures() {
    let wav_dir = corpus_root().join("wav");
    std::fs::create_dir_all(&wav_dir).expect("create wav dir");

    let (baseline_a, baseline_b) = write_offset_chirp_wav_pair(
        &wav_dir,
        DEFAULT_SAMPLE_RATE,
        DEFAULT_TOTAL_SECS,
        0,
    );
    std::fs::rename(&baseline_a, wav_dir.join("baseline_0s_a.wav")).expect("rename baseline a");
    std::fs::rename(&baseline_b, wav_dir.join("baseline_0s_b.wav")).expect("rename baseline b");

    let (leader_a, leader_b) = write_offset_chirp_wav_pair(
        &wav_dir,
        DEFAULT_SAMPLE_RATE,
        DEFAULT_TOTAL_SECS,
        3,
    );
    std::fs::rename(&leader_a, wav_dir.join("leader_3s_a.wav")).expect("rename leader a");
    std::fs::rename(&leader_b, wav_dir.join("leader_3s_b.wav")).expect("rename leader b");

    let (chirp_temp_a, chirp_temp_b) = write_offset_chirp_wav_pair(
        &wav_dir,
        DEFAULT_SAMPLE_RATE,
        NEGATIVE_CASE_SECS,
        0,
    );
    std::fs::rename(&chirp_temp_a, wav_dir.join("chirp_a.wav")).expect("rename chirp a");
    let _ = std::fs::remove_file(chirp_temp_b);

    write_tone_wav(
        &wav_dir.join("tone_b.wav"),
        DEFAULT_SAMPLE_RATE,
        NEGATIVE_CASE_SECS,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::config::ChromaprintPreset;
    use crate::application::testing::fakes::FakeProgressReporter;
    use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
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

    fn run_manifest_cases(tier: CorpusTier) {
        let manifest = load_manifest();
        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&media_reader, &fingerprinter, &aligner, &progress);

        for case in manifest
            .case
            .iter()
            .filter(|case| case.tier == tier && !case.ignore)
        {
            if case.requires_ffmpeg && !ffmpeg_util::ffmpeg_available() {
                eprintln!("skipping case {}: ffmpeg unavailable", case.id);
                continue;
            }
            if case.requires_he_aac && !cfg!(feature = "he-aac") {
                eprintln!("skipping case {}: he-aac feature not enabled", case.id);
                continue;
            }

            let started = std::time::Instant::now();
            let (_guard, video_a, video_b) =
                resolve_case_paths(case, &manifest.defaults);

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
                }
            } else {
                let result = run_corpus_case(
                    &use_case,
                    case,
                    &manifest.defaults,
                    video_a,
                    video_b,
                )
                .unwrap_or_else(|error| panic!("case {} failed: {error}", case.id));

                assert_corpus_expectations(case, &manifest.defaults, &result);
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
        run_manifest_cases(CorpusTier::Committed);
    }

    #[test]
    #[ignore = "slow: generated corpus + ffmpeg; cargo test corpus_generated -- --ignored"]
    fn corpus_generated_cases() {
        run_manifest_cases(CorpusTier::Generated);
    }

    #[test]
    #[ignore = "long smoke; set CLIP_SYNC_CORPUS to a persistent directory"]
    fn corpus_external_cases() {
        if std::env::var("CLIP_SYNC_CORPUS").is_err() {
            eprintln!("skipping corpus_external_cases: CLIP_SYNC_CORPUS not set");
            return;
        }
        run_manifest_cases(CorpusTier::External);
    }
}
