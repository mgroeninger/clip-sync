//! Real-media floor-oracle pair builder for FLOOR_OK calibration (integration tests).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clip_sync::testing::corpus_sources::{
    find_source, load_sources, prepare_source_master_wav, source_cache_path, source_ready,
};
use clip_sync::testing::ffmpeg_util::{self, delay_wav, ffmpeg_available};
use clip_sync_repair::test_support::energy_signature_fixtures::{
    gap_anchor_secs, ProductionScenarioSpec,
};
use hound::{WavReader, WavWriter};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct FloorOracleManifest {
    pub version: u32,
    #[serde(default)]
    pub defaults: FloorOracleDefaults,
    #[serde(default)]
    pub case: Vec<FloorOracleCase>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FloorOracleDefaults {
    #[serde(default = "default_total_secs")]
    pub total_secs: u32,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default = "default_gap_duration_secs")]
    pub gap_duration_secs: f64,
    #[serde(default = "default_gap_signature_context_secs")]
    pub gap_signature_context_secs: f64,
    /// Per-encode bitrate (any lossy codec, not just AAC), e.g. "128k", "64k".
    #[serde(default = "default_bitrate", alias = "aac_bitrate")]
    pub bitrate: String,
    #[serde(default = "default_duration_match_tolerance_secs")]
    pub duration_match_tolerance_secs: f64,
    #[serde(default = "default_gap_interior_edge_secs")]
    pub gap_interior_edge_secs: f64,
    #[serde(default = "default_gap_interior_peak_max")]
    pub gap_interior_peak_max: i16,
}

impl Default for FloorOracleDefaults {
    fn default() -> Self {
        Self {
            total_secs: default_total_secs(),
            sample_rate: default_sample_rate(),
            gap_duration_secs: default_gap_duration_secs(),
            gap_signature_context_secs: default_gap_signature_context_secs(),
            bitrate: default_bitrate(),
            duration_match_tolerance_secs: default_duration_match_tolerance_secs(),
            gap_interior_edge_secs: default_gap_interior_edge_secs(),
            gap_interior_peak_max: default_gap_interior_peak_max(),
        }
    }
}

fn default_total_secs() -> u32 {
    60
}
fn default_sample_rate() -> u32 {
    48_000
}
fn default_gap_duration_secs() -> f64 {
    1.0
}
fn default_gap_signature_context_secs() -> f64 {
    3.0
}
fn default_bitrate() -> String {
    "128k".into()
}
fn default_duration_match_tolerance_secs() -> f64 {
    0.05
}
fn default_gap_interior_edge_secs() -> f64 {
    0.05
}
fn default_gap_interior_peak_max() -> i16 {
    500
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OracleVariant {
    SameMaster,
    TwoMic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloorOracleFormat {
    Wav,
    Mp3,
    Mp4Aac,
    /// Ogg/Vorbis — decodable by the production pipeline (Symphonia `format-ogg` + `codec-vorbis`),
    /// so the `.ogg` is fed straight through. (Opus is *not* decodable: no Symphonia Opus codec.)
    Vorbis,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FloorOracleCase {
    pub id: String,
    pub source_id: String,
    #[serde(default)]
    pub donor_source_id: Option<String>,
    #[serde(default)]
    pub oracle_variant: Option<OracleVariant>,
    #[serde(default)]
    pub format_a: Option<FloorOracleFormat>,
    #[serde(default)]
    pub format_b: Option<FloorOracleFormat>,
    #[serde(default, alias = "aac_bitrate_a")]
    pub bitrate_a: Option<String>,
    #[serde(default, alias = "aac_bitrate_b")]
    pub bitrate_b: Option<String>,
    #[serde(default)]
    pub total_secs: Option<u32>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub gap_duration_secs: Option<f64>,
    #[serde(default)]
    pub gap_signature_context_secs: Option<f64>,
    /// Absolute gap-anchor position (seconds from start). When set, overrides the default
    /// `gap_anchor_secs(spec)` mid-window placement — used to anchor the gap on a chosen region
    /// (e.g. a loud broadband transient) for the dead-zone probe (Run B).
    #[serde(default)]
    pub gap_anchor_secs: Option<f64>,
    /// Per-case override of `gap_interior_peak_max`. Loud-border inject-then-encode cases bleed
    /// MDCT energy into the silenced interior, so the default (500) would false-fail validation;
    /// the borders (not the interior) drive the seam, so a relaxed cap is acceptable there.
    #[serde(default)]
    pub gap_interior_peak_max: Option<i16>,
    /// **Punch-after-encode geometry** (same-master only). A is encoded from the *full* master,
    /// decoded, then the gap is zeroed in PCM — so A's borders are *native* lossy-decoded audio
    /// (no inject-then-encode MDCT ringing) and the gap interior is genuinely clean. This removes
    /// the floor-corruption confound that makes loud-border inject-then-encode cases read an
    /// uninformative (NaN) floor (G5). A is fed to the pipeline as WAV; B is the independent encode.
    #[serde(default)]
    pub punch_after_encode: bool,
    #[serde(default)]
    pub b_encode_delay_ms: Option<u32>,
    #[serde(default)]
    pub expect_informative_floor: Option<bool>,
    #[serde(default)]
    pub ignore: bool,
}

impl FloorOracleCase {
    pub fn variant(&self) -> OracleVariant {
        self.oracle_variant.unwrap_or(OracleVariant::SameMaster)
    }

    pub fn expect_informative(&self) -> bool {
        self.expect_informative_floor.unwrap_or(matches!(
            self.variant(),
            OracleVariant::SameMaster
        ))
    }
}

#[derive(Debug, Clone)]
pub struct SourceGapOracleMeta {
    pub case_id: String,
    pub source_id: String,
    pub donor_source_id: Option<String>,
    pub variant: OracleVariant,
    pub sample_rate: u32,
    pub format_a: FloorOracleFormat,
    pub format_b: FloorOracleFormat,
    pub gap_start_frame: usize,
    pub gap_end_frame: usize,
    pub expect_informative_floor: bool,
    /// Resolved post-encode interior silence cap (per-case override or default).
    pub gap_interior_peak_max: i16,
}

pub struct BuiltFloorOracle {
    pub path_a: PathBuf,
    pub path_b: PathBuf,
    pub meta: SourceGapOracleMeta,
}

pub fn corpus_root() -> PathBuf {
    if let Ok(root) = std::env::var("CLIP_SYNC_WORKSPACE_ROOT") {
        return PathBuf::from(root)
            .join("crates/clip-sync-repair/tests/floor_oracle");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/floor_oracle")
}

pub fn load_manifest() -> FloorOracleManifest {
    let path = corpus_root().join("manifest.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    toml::from_str(&text)
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

pub fn case_sources_ready(case: &FloorOracleCase) -> bool {
    if !source_ready(&case.source_id) {
        return false;
    }
    if let Some(donor) = &case.donor_source_id {
        return source_ready(donor);
    }
    true
}

fn secs_to_frames(secs: f64, sample_rate: u32) -> usize {
    (secs * sample_rate as f64).round().max(0.0) as usize
}

pub fn gap_frames_for_case(case: &FloorOracleCase, defaults: &FloorOracleDefaults) -> (usize, usize) {
    let total_secs = f64::from(case.total_secs.unwrap_or(defaults.total_secs));
    let context_secs = case
        .gap_signature_context_secs
        .unwrap_or(defaults.gap_signature_context_secs);
    let gap_duration = case.gap_duration_secs.unwrap_or(defaults.gap_duration_secs);
    let sample_rate = case.sample_rate.unwrap_or(defaults.sample_rate);

    let anchor_secs = case.gap_anchor_secs.unwrap_or_else(|| {
        let spec = ProductionScenarioSpec::production_standard(total_secs, context_secs);
        gap_anchor_secs(&spec)
    });
    let gap_start = secs_to_frames(anchor_secs, sample_rate);
    let gap_end = gap_start + secs_to_frames(gap_duration, sample_rate);
    (gap_start, gap_end)
}

fn zero_mono_frame_range(path: &Path, start_frame: usize, end_frame: usize) {
    let mut reader = WavReader::open(path).expect("open wav for gap inject");
    assert_eq!(reader.spec().channels, 1, "floor oracle expects mono WAV");
    let spec = reader.spec();
    let mut samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|s| s.expect("read sample"))
        .collect();
    for sample in samples
        .iter_mut()
        .skip(start_frame)
        .take(end_frame.saturating_sub(start_frame))
    {
        *sample = 0;
    }
    let mut writer = WavWriter::create(path, spec).expect("rewrite wav");
    for s in samples {
        writer.write_sample(s).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

pub fn read_mono_wav(path: &Path) -> (u32, Vec<i16>) {
    let mut reader = WavReader::open(path).expect("open mono wav");
    assert_eq!(reader.spec().channels, 1);
    let rate = reader.spec().sample_rate;
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|s| s.expect("read sample"))
        .collect();
    (rate, samples)
}

pub fn decode_to_mono_wav_at(
    input: &Path,
    output: &Path,
    sample_rate: u32,
    max_duration_secs: Option<u32>,
) -> bool {
    ffmpeg_util::decode_to_mono_wav(input, output, sample_rate, max_duration_secs)
}

fn output_path(dir: &Path, stem: &str, format: FloorOracleFormat) -> PathBuf {
    let ext = match format {
        FloorOracleFormat::Wav => "wav",
        FloorOracleFormat::Mp3 => "mp3",
        FloorOracleFormat::Mp4Aac => "mp4",
        FloorOracleFormat::Vorbis => "ogg",
    };
    dir.join(format!("{stem}.{ext}"))
}

fn encode_with_format(input_wav: &Path, output: &Path, format: FloorOracleFormat, bitrate: &str) -> bool {
    if !ffmpeg_available() {
        return false;
    }
    match format {
        FloorOracleFormat::Wav => std::fs::copy(input_wav, output).is_ok(),
        FloorOracleFormat::Mp3 => encode_mp3(input_wav, output, bitrate),
        FloorOracleFormat::Mp4Aac => encode_aac_mp4(input_wav, output, bitrate),
        FloorOracleFormat::Vorbis => encode_vorbis(input_wav, output, bitrate),
    }
}

/// CBR-ish Ogg/Vorbis at `bitrate` (`-c:a libvorbis -b:a`). Fed straight to the pipeline — Symphonia
/// decodes Ogg/Vorbis natively, so the floor reflects the real production decode path.
fn encode_vorbis(input_wav: &Path, output: &Path, bitrate: &str) -> bool {
    Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input_wav)
        .args(["-vn", "-c:a", "libvorbis", "-b:a", bitrate])
        .arg("-f")
        .arg("ogg")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// CBR MP3 at `bitrate` (e.g. "128k", "64k") — configurable, unlike the fixed `-q:a 4` helper, so
/// low-bitrate cases and genuine dual-encode (different A/B bitrates) are expressible.
fn encode_mp3(input_wav: &Path, output: &Path, bitrate: &str) -> bool {
    Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input_wav)
        .args(["-vn", "-c:a", "libmp3lame", "-b:a", bitrate])
        .arg("-f")
        .arg("mp3")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn encode_aac_mp4(input_wav: &Path, output: &Path, bitrate: &str) -> bool {
    Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input_wav)
        .args(["-vn", "-c:a", "aac", "-b:a", bitrate, "-movflags", "+faststart"])
        .arg("-f")
        .arg("mp4")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn resolve_source_master(
    dir: &Path,
    source_id: &str,
    sample_rate: u32,
    total_secs: u32,
) -> PathBuf {
    let sources = load_sources();
    let source = find_source(&sources, source_id);
    let source_path = source_cache_path(source);
    assert!(
        source_path.is_file(),
        "missing source {source_id} at {} (run scripts/fetch_corpus_sources.ps1)",
        source_path.display()
    );
    let master = dir.join(format!("{source_id}_master.wav"));
    assert!(
        prepare_source_master_wav(&source_path, &master, sample_rate, Some(total_secs)),
        "ffmpeg decode failed for {}",
        source_path.display()
    );
    master
}

pub fn build_floor_oracle_pair(
    dir: &Path,
    case: &FloorOracleCase,
    defaults: &FloorOracleDefaults,
) -> BuiltFloorOracle {
    assert!(ffmpeg_available(), "floor oracle build requires ffmpeg on PATH");
    std::fs::create_dir_all(dir).expect("create case output dir");

    let sample_rate = case.sample_rate.unwrap_or(defaults.sample_rate);
    let total_secs = case.total_secs.unwrap_or(defaults.total_secs);
    let format_a = case.format_a.unwrap_or(FloorOracleFormat::Wav);
    let format_b = case.format_b.unwrap_or(FloorOracleFormat::Wav);
    let bitrate_a = case
        .bitrate_a
        .as_deref()
        .unwrap_or(defaults.bitrate.as_str());
    let bitrate_b = case
        .bitrate_b
        .as_deref()
        .unwrap_or(defaults.bitrate.as_str());
    let variant = case.variant();

    let master_a = resolve_source_master(dir, &case.source_id, sample_rate, total_secs);
    let (gap_start, gap_end) = gap_frames_for_case(case, defaults);

    // A-side source PCM with the gap. Inject-then-encode: zero the master, then encode (the encoder
    // sees the silence → MDCT ringing at the loud borders). Punch-after-encode: encode the *full*
    // master, decode, then zero — A's borders are native, the gap interior is clean.
    let a_pregap = dir.join("a_pregap.wav");
    if case.punch_after_encode {
        assert!(
            matches!(variant, OracleVariant::SameMaster),
            "punch_after_encode is same-master only (case {})",
            case.id
        );
        let a_native_enc = output_path(dir, "a_native_enc", format_a);
        assert!(
            encode_with_format(&master_a, &a_native_enc, format_a, bitrate_a),
            "punch_after_encode: encode A (full master) failed for case {}",
            case.id
        );
        assert!(
            decode_to_mono_wav_at(&a_native_enc, &a_pregap, sample_rate, None),
            "punch_after_encode: decode A failed for case {}",
            case.id
        );
        zero_mono_frame_range(&a_pregap, gap_start, gap_end);
    } else {
        std::fs::copy(&master_a, &a_pregap).expect("copy master to a_pregap");
        zero_mono_frame_range(&a_pregap, gap_start, gap_end);
    }

    let b_full = dir.join("b_full.wav");
    match (&case.donor_source_id, variant) {
        (Some(donor), OracleVariant::TwoMic) => {
            let master_b = resolve_source_master(dir, donor, sample_rate, total_secs);
            std::fs::copy(&master_b, &b_full).expect("copy donor master to b_full");
        }
        _ => {
            std::fs::copy(&master_a, &b_full).expect("copy master to b_full");
        }
    }

    if let Some(delay_ms) = case.b_encode_delay_ms.filter(|&ms| ms > 0) {
        let delayed = dir.join("b_delayed.wav");
        assert!(
            delay_wav(&b_full, &delayed, delay_ms),
            "ffmpeg adelay failed for case {}",
            case.id
        );
        std::fs::copy(&delayed, &b_full).expect("copy delayed b");
    }

    // Punch-after-encode: A already carries its codec round-trip + clean punched gap, so feed the
    // WAV directly — re-encoding here would re-introduce the inject-then-encode ringing we removed.
    let path_a = if case.punch_after_encode {
        a_pregap.clone()
    } else {
        let p = output_path(dir, "a", format_a);
        assert!(
            encode_with_format(&a_pregap, &p, format_a, bitrate_a),
            "encode A failed for case {}",
            case.id
        );
        p
    };
    let path_b = output_path(dir, "b", format_b);
    assert!(
        encode_with_format(&b_full, &path_b, format_b, bitrate_b),
        "encode B failed for case {}",
        case.id
    );

    let built = BuiltFloorOracle {
        path_a,
        path_b,
        meta: SourceGapOracleMeta {
            case_id: case.id.clone(),
            source_id: case.source_id.clone(),
            donor_source_id: case.donor_source_id.clone(),
            variant,
            sample_rate,
            format_a,
            format_b,
            gap_start_frame: gap_start,
            gap_end_frame: gap_end,
            expect_informative_floor: case.expect_informative(),
            gap_interior_peak_max: case
                .gap_interior_peak_max
                .unwrap_or(defaults.gap_interior_peak_max),
        },
    };

    validate_built_oracle(&built, defaults).unwrap_or_else(|e| {
        panic!("floor oracle post-encode validation failed for {}: {e}", case.id)
    });

    built
}

pub fn validate_built_oracle(
    built: &BuiltFloorOracle,
    defaults: &FloorOracleDefaults,
) -> Result<(), String> {
    if !ffmpeg_available() {
        return Err("ffmpeg unavailable".into());
    }

    let dir = built
        .path_a
        .parent()
        .ok_or_else(|| "path_a has no parent".to_string())?;
    let decoded_a = dir.join("validate_a.wav");
    let decoded_b = dir.join("validate_b.wav");
    let rate = built.meta.sample_rate;
    if !decode_to_mono_wav_at(&built.path_a, &decoded_a, rate, None) {
        return Err("decode A failed".into());
    }
    if !decode_to_mono_wav_at(&built.path_b, &decoded_b, rate, None) {
        return Err("decode B failed".into());
    }

    let (_, samples_a) = read_mono_wav(&decoded_a);
    let (_, samples_b) = read_mono_wav(&decoded_b);
    let frames_a = samples_a.len();
    let frames_b = samples_b.len();
    let tol_frames = secs_to_frames(defaults.duration_match_tolerance_secs, rate);
    if frames_a.abs_diff(frames_b) > tol_frames {
        return Err(format!(
            "A/B duration mismatch: {frames_a} vs {frames_b} frames (tol {tol_frames})"
        ));
    }

    let gap_start = built.meta.gap_start_frame;
    let gap_end = built.meta.gap_end_frame;
    let edge = secs_to_frames(defaults.gap_interior_edge_secs, rate);
    let interior_start = gap_start.saturating_add(edge);
    let interior_end = gap_end.saturating_sub(edge);
    if interior_start >= interior_end {
        return Err(format!(
            "gap too short for interior check: {gap_start}..{gap_end}"
        ));
    }
    let peak = samples_a[interior_start..interior_end]
        .iter()
        .map(|s| s.unsigned_abs())
        .max()
        .unwrap_or(0);
    if peak > built.meta.gap_interior_peak_max as u16 {
        return Err(format!(
            "gap interior peak {peak} exceeds max {} after encode",
            built.meta.gap_interior_peak_max
        ));
    }

    Ok(())
}

pub fn format_label(format: FloorOracleFormat) -> &'static str {
    match format {
        FloorOracleFormat::Wav => "wav",
        FloorOracleFormat::Mp3 => "mp3",
        FloorOracleFormat::Mp4Aac => "mp4_aac",
        FloorOracleFormat::Vorbis => "vorbis",
    }
}
