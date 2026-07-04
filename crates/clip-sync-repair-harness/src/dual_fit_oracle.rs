//! Real-media **stepped** gap builder — proves the A3 dual-fit rescue (G6) on open-licensed
//! Wikimedia/Musopen masters instead of a frozen `corpus.json`'s static predicates (B2, see
//! `docs/TEMP-pipeline-perf-redesign-plan.md` §4.7). Produces the same [`BuiltFloorOracle`] shape
//! as [`crate::floor_oracle::build_floor_oracle_pair`], so it runs through the identical
//! `residual_gate::run_built_floor_oracle_cfg` → production `PatchAudio` path — no second
//! execution path to drift from what production actually does.
//!
//! **Construction.** B is the master, untouched. A is the master with a genuine jump-cut: the
//! nominal gap span is silenced, and everything after it is sourced `step_ms` further along the
//! master's own timeline than a naive rigid (single-lag) continuation would predict. So the
//! pre-shoulder aligns with B at lag 0, but the post-shoulder needs a `step_ms` correction — no
//! single rigid placement satisfies both seams, forcing bracket exhaustion and the G6 dual-fit
//! rescue. Mirrors `domain::dual_fit::tests::recovers_a_stepped_silence_splice`, but end-to-end
//! through real codecs and the real `PatchAudio` entry point.

use std::path::Path;

use clip_sync::testing::corpus_sources::{
    find_source, load_sources, prepare_source_master_wav, source_cache_path, source_ready,
};
use clip_sync::testing::ffmpeg_util::ffmpeg_available;
use clip_sync_repair_fixtures::energy_signature_fixtures::{gap_anchor_secs, ProductionScenarioSpec};
use hound::{WavReader, WavWriter};
use serde::Deserialize;

use crate::floor_oracle::{
    decode_to_mono_wav_at, encode_with_format, floor_oracle_corpus_root, output_path,
    read_mono_wav, secs_to_frames, BuiltFloorOracle, FloorOracleDefaults, FloorOracleFormat,
    OracleVariant, SourceGapOracleMeta,
};

#[derive(Debug, Deserialize)]
pub struct DualFitOracleManifest {
    pub version: u32,
    #[serde(default)]
    pub defaults: FloorOracleDefaults,
    #[serde(default)]
    pub case: Vec<DualFitOracleCase>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DualFitOracleCase {
    pub id: String,
    pub source_id: String,
    /// Post-cut jump, in ms: how much further along the master's own timeline the post-gap
    /// content really sits vs. a naive rigid (single-lag) continuation. Must stay comfortably
    /// under the dual-fit seam-local search radius (600 ms, `SEAM_LOCAL_SEARCH_MS`) to be
    /// discoverable, and large enough that a rigid placement genuinely decorrelates one seam.
    pub step_ms: f64,
    #[serde(default)]
    pub format: Option<FloorOracleFormat>,
    #[serde(default)]
    pub bitrate: Option<String>,
    #[serde(default)]
    pub total_secs: Option<u32>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub gap_duration_secs: Option<f64>,
    /// Absolute gap-anchor position (seconds from start); defaults to the same mid-window
    /// placement `floor_oracle` uses when unset.
    #[serde(default)]
    pub gap_anchor_secs: Option<f64>,
    #[serde(default)]
    pub ignore: bool,
}

pub fn load_manifest(repair_tests_dir: &Path) -> DualFitOracleManifest {
    let path = floor_oracle_corpus_root(repair_tests_dir).join("dual_fit_manifest.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

pub fn case_sources_ready(case: &DualFitOracleCase) -> bool {
    source_ready(&case.source_id)
}

pub fn require_case_sources(case: &DualFitOracleCase) {
    assert!(
        case_sources_ready(case),
        "validation tier: source cache missing for case `{}` (source `{}`) — run scripts/fetch_corpus_sources.ps1",
        case.id,
        case.source_id
    );
}

fn gap_frames_for_case(case: &DualFitOracleCase, defaults: &FloorOracleDefaults) -> (usize, usize) {
    let total_secs = f64::from(case.total_secs.unwrap_or(defaults.total_secs));
    let context_secs = defaults.gap_signature_context_secs;
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

/// Build a jump-cut stepped-gap oracle. Returns the same [`BuiltFloorOracle`] shape as
/// [`crate::floor_oracle::build_floor_oracle_pair`] so it runs through the identical harness.
pub fn build_stepped_floor_oracle_pair(
    dir: &Path,
    case: &DualFitOracleCase,
    defaults: &FloorOracleDefaults,
) -> BuiltFloorOracle {
    assert!(ffmpeg_available(), "dual-fit oracle build requires ffmpeg on PATH");
    std::fs::create_dir_all(dir).expect("create case output dir");

    let sample_rate = case.sample_rate.unwrap_or(defaults.sample_rate);
    let total_secs = case.total_secs.unwrap_or(defaults.total_secs);
    let format = case.format.unwrap_or(FloorOracleFormat::Wav);
    let bitrate = case.bitrate.as_deref().unwrap_or(defaults.bitrate.as_str());

    let sources = load_sources();
    let source = find_source(&sources, &case.source_id);
    let source_path = source_cache_path(source);
    assert!(
        source_path.is_file(),
        "missing source {} at {} (run scripts/fetch_corpus_sources.ps1)",
        case.source_id,
        source_path.display()
    );
    let master = dir.join(format!("{}_master.wav", case.source_id));
    assert!(
        prepare_source_master_wav(&source_path, &master, sample_rate, Some(total_secs)),
        "ffmpeg decode failed for {}",
        source_path.display()
    );

    let (gap_start, gap_end) = gap_frames_for_case(case, defaults);
    let step_frames = secs_to_frames(case.step_ms / 1000.0, sample_rate) as i64;

    let mut reader = WavReader::open(&master).expect("open master wav");
    assert_eq!(reader.spec().channels, 1, "dual-fit oracle expects mono WAV");
    let spec = reader.spec();
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|s| s.expect("read sample"))
        .collect();

    let post_start = (gap_end as i64 + step_frames).max(gap_end as i64) as usize;
    assert!(
        post_start < samples.len(),
        "case {}: step {}ms + gap runs past master end ({} frames)",
        case.id,
        case.step_ms,
        samples.len()
    );

    let mut a_samples: Vec<i16> =
        Vec::with_capacity(gap_start + (gap_end - gap_start) + (samples.len() - post_start));
    a_samples.extend_from_slice(&samples[..gap_start]);
    a_samples.extend(std::iter::repeat(0i16).take(gap_end - gap_start));
    a_samples.extend_from_slice(&samples[post_start..]);

    let a_pregap = dir.join("a_pregap.wav");
    {
        let mut writer = WavWriter::create(&a_pregap, spec).expect("write a_pregap wav");
        for s in &a_samples {
            writer.write_sample(*s).expect("write sample");
        }
        writer.finalize().expect("finalize a_pregap wav");
    }

    let b_full = dir.join("b_full.wav");
    std::fs::copy(&master, &b_full).expect("copy master to b_full");

    let path_a = output_path(dir, "a", format);
    assert!(
        encode_with_format(&a_pregap, &path_a, format, bitrate),
        "encode A failed for case {}",
        case.id
    );
    let path_b = output_path(dir, "b", format);
    assert!(
        encode_with_format(&b_full, &path_b, format, bitrate),
        "encode B failed for case {}",
        case.id
    );

    let built = BuiltFloorOracle {
        path_a,
        path_b,
        meta: SourceGapOracleMeta {
            case_id: case.id.clone(),
            source_id: case.source_id.clone(),
            donor_source_id: None,
            variant: OracleVariant::SameMaster,
            sample_rate,
            format_a: format,
            format_b: format,
            gap_start_frame: gap_start,
            gap_end_frame: gap_end,
            expect_informative_floor: true,
            gap_interior_peak_max: defaults.gap_interior_peak_max,
        },
    };

    validate_stepped_oracle(&built, defaults).unwrap_or_else(|e| {
        panic!("dual-fit oracle post-encode validation failed for {}: {e}", case.id)
    });

    built
}

fn validate_stepped_oracle(built: &BuiltFloorOracle, defaults: &FloorOracleDefaults) -> Result<(), String> {
    if !ffmpeg_available() {
        return Err("ffmpeg unavailable".into());
    }
    let dir = built
        .path_a
        .parent()
        .ok_or_else(|| "path_a has no parent".to_string())?;
    let decoded_a = dir.join("validate_a.wav");
    let rate = built.meta.sample_rate;
    if !decode_to_mono_wav_at(&built.path_a, &decoded_a, rate, None) {
        return Err("decode A failed".into());
    }
    let (_, samples_a) = read_mono_wav(&decoded_a);

    let gap_start = built.meta.gap_start_frame;
    let gap_end = built.meta.gap_end_frame;
    let edge = secs_to_frames(defaults.gap_interior_edge_secs, rate);
    let interior_start = gap_start.saturating_add(edge);
    let interior_end = gap_end.saturating_sub(edge).min(samples_a.len());
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
