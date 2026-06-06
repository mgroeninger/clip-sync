use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::application::align_videos::{AlignVideos, AlignVideosRequest};
use crate::application::config::{AppConfig, ClipConfig};
use crate::application::error::AppError;
use crate::application::ports::{Aligner, Fingerprinter, MediaReader, ProgressReporter};
use crate::application::testing::audio_fixtures::{write_offset_chirp_wav_pair, write_tone_wav};
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
    1.0
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

#[derive(Debug, Clone, Deserialize)]
pub struct CorpusCase {
    pub id: String,
    pub tier: CorpusTier,
    pub video_a: Option<String>,
    pub video_b: Option<String>,
    pub expected_offset_secs: Option<f64>,
    pub expect_aligned: Option<bool>,
    pub expect_recommended: Option<bool>,
    #[serde(default)]
    #[allow(dead_code)]
    pub expect_exit_code: Option<u32>,
    #[serde(default)]
    pub ignore: bool,
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

pub fn build_config(defaults: &CorpusDefaults) -> AppConfig {
    AppConfig {
        clip: ClipConfig {
            clip_length: Duration::from_secs(defaults.clip_length_secs),
            num_clips: defaults.num_clips,
            target_sample_rate: Some(defaults.target_sample_rate),
            // Synthetic fixtures encode offset as leading silence; trimming would erase it.
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
            ..ClipConfig::default()
        },
        ..Default::default()
    }
}

pub fn run_corpus_case<MR, FP, AL, PR>(
    use_case: &AlignVideos<'_, MR, FP, AL, PR>,
    _case: &CorpusCase,
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
    let response = use_case.execute(AlignVideosRequest {
        video_a,
        video_b,
        config: build_config(defaults),
    })?;
    Ok(response.result)
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
        assert!(
            (actual - expected_offset).abs() <= defaults.tolerance_secs,
            "case {}: offset {actual}, expected {expected_offset} ± {}",
            case.id,
            defaults.tolerance_secs
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

    #[test]
    fn corpus_committed_cases() {
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
            .filter(|case| case.tier == CorpusTier::Committed && !case.ignore)
        {
            let (video_a, video_b) = resolve_committed_pair(case);
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
    }
}
