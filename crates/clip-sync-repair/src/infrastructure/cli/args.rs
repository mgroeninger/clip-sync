use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use clip_sync::LogLevel;

use crate::domain::{FillMode, FillOffsetMode, GapSignatureMode, RepairProfile};
use crate::infrastructure::config::OutputFormat;

#[derive(Parser, Debug)]
#[command(
    name = "clip-sync-repair",
    about = "Scan two videos for audio gaps in A that B could fill",
    version
)]
pub struct Args {
    /// Video A — the file to scan for audio gaps.
    pub video_a: PathBuf,

    /// Video B — the reference file used to check if gaps are fillable.
    pub video_b: PathBuf,

    /// TOML config file (align + repair + logging sections).
    #[arg(short, long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Diagnostic: after scan, write a licensing-safe gap-fingerprint corpus into DIR
    /// (`corpus.json` with all gaps + one self-contained JSON per gap + `manifest.json`).
    #[arg(long, value_name = "DIR")]
    pub gap_fingerprints: Option<PathBuf>,

    /// Gap index (repeatable). When given, characterize ONLY these gaps; omit to characterize ALL
    /// gaps. Each characterized gap gets full detail (per-bracket scores + lag). Use the normal repair
    /// gap table to pick which gaps are worth characterizing. Only meaningful with --gap-fingerprints.
    #[arg(long, value_name = "IDX")]
    pub fingerprint_gap: Vec<usize>,

    /// Output format.
    #[arg(long, default_value = "human")]
    pub format: OutputFormat,

    /// Clip window length (e.g. 15m, 90s) [default: 15m].
    #[arg(long, value_parser = parse_duration)]
    pub clip_length: Option<std::time::Duration>,

    /// Override: minimum gap duration to report (ms) [default: 1000].
    #[arg(long, value_name = "MS")]
    pub min_gap_ms: Option<u64>,

    /// Override: silence threshold as a fraction of peak amplitude [default: 0.01].
    #[arg(long, value_name = "FRACTION")]
    pub silence_fraction: Option<f32>,

    /// Override: decode chunk size for sequential scan (seconds) [default: 10].
    #[arg(long, value_name = "SECS", alias = "scan-window-secs")]
    pub decode_chunk_secs: Option<u64>,

    /// Override: analysis block size for silence detection (ms) [default: 250].
    #[arg(long, value_name = "MS")]
    pub scan_block_ms: Option<u64>,

    /// Override: consecutive non-silent time to absorb before closing a silence run (ms)
    /// [default: 500].
    #[arg(long, value_name = "MS")]
    pub silence_hold_ms: Option<u64>,

    /// Override: absolute RMS floor for silence detection (0–32767 scale; 0 disables)
    /// [default: 33].
    #[arg(long, value_name = "N")]
    pub absolute_silence_rms: Option<f32>,

    /// Enable bidirectional silence scan (scan B's timeline too) [default: enabled].
    #[arg(long, overrides_with = "no_scan_both")]
    pub scan_both: bool,

    /// Disable bidirectional silence scan.
    #[arg(long, overrides_with = "scan_both")]
    pub no_scan_both: bool,

    /// Write patched audio to a WAV file (implies write mode).
    #[arg(long, value_name = "PATH")]
    pub wav: Option<PathBuf>,

    /// Mux patched audio into video A (implies write mode; requires `ffmpeg-mux` build feature).
    #[arg(long, value_name = "PATH")]
    pub mux: Option<PathBuf>,

    /// Disable loudness normalization of fill segments [default: normalization enabled].
    #[arg(long)]
    pub no_normalize: bool,

    /// Enable the A3 dual-fit repair fallback: for a gap the seam gate skips (bracket-exhausted), attempt an
    /// independent per-shoulder fit + interior-trim fill, validated by the unchanged gate [default: off].
    #[arg(long)]
    pub dual_fit: bool,

    /// Stricter seam checks: always run the waveform Pearson gate (never skip it when
    /// structure scores are high), disable partial-structure threshold soften, and require
    /// both pre/post waveform seams to pass (no short-gap mean or one-strong-seam shortcuts).
    /// Does not disable structure matching on B or gap boundary extension retries.
    /// [fill-mode: gate only]
    #[arg(long)]
    pub no_structure_trust: bool,

    /// Override: minimum Pearson correlation at gap seams when the waveform gate runs
    /// [default: 0.35].
    #[arg(long, value_name = "N")]
    pub min_fill_correlation: Option<f32>,

    /// Override: maximum B fill slide during structure match (seconds) [default: 0.5].
    /// Legacy structure polish window. Under `fill_mode = fit`, B slide radius is
    /// `--fill-border-search-secs` (not this flag).
    #[arg(long, value_name = "SECS")]
    pub max_fill_align_adjust_secs: Option<f64>,

    /// Override: B slide radius for unified gap-fill search (seconds) [default: 10].
    /// Primary performance lever; also sizes part of the per-gap B extract window.
    #[arg(long, value_name = "SECS")]
    pub fill_border_search_secs: Option<f64>,

    /// Override: extra B audio extracted on each side of the mapped gap (seconds) [default: 1].
    #[arg(long, value_name = "SECS")]
    pub fill_align_margin_secs: Option<f64>,

    /// Override: A audio on each side of the gap used for structure signatures (seconds)
    /// [default: 3].
    #[arg(long, value_name = "SECS")]
    pub gap_signature_context_secs: Option<f64>,

    /// Override: how far B fill end may differ from A gap length when locating post-border
    /// (seconds) [default: 5].
    #[arg(long, value_name = "SECS")]
    pub fill_length_slack_secs: Option<f64>,

    /// Unified fit scorer: structure term weight (`fill_mode = fit` only) [default: 0.35].
    #[arg(long, value_name = "N")]
    pub fill_fit_structure_weight: Option<f64>,

    /// Unified fit scorer: waveform term weight (`fill_mode = fit` only) [default: 0.65].
    #[arg(long, value_name = "N")]
    pub fill_fit_waveform_weight: Option<f64>,

    /// Unified fit scorer: repeat-at-border penalty when waveform seams are weak (`fill_mode = fit` only)
    /// [default: 0.4].
    #[arg(long, value_name = "N")]
    pub fill_repeat_penalty_weight: Option<f64>,

    /// Override: A-side audio excluded adjacent to the dropout for border templates (seconds)
    /// [default: 0.35].
    #[arg(long, value_name = "SECS")]
    pub border_standoff_secs: Option<f64>,

    /// Per-gap B mapping [default: recommended]: `recommended`, `interpolated` (clip drift),
    /// `anchored` (anchor interpolation; not wired in patch yet), `anchored-retry` (two-pass retry).
    #[arg(long, value_parser = clap::value_parser!(FillOffsetMode), value_name = "MODE")]
    pub fill_offset: Option<FillOffsetMode>,

    /// Structure signature for gap fill search (`fill_mode = fit` only) [default: auto]:
    /// `bool` (active/silent bins), `energy` (envelope Pearson), `auto` (energy when envelope has contour).
    #[arg(long, value_parser = clap::value_parser!(GapSignatureMode), value_name = "MODE")]
    pub gap_signature_mode: Option<GapSignatureMode>,

    /// Minimum `min(pre, post)` for a pass-1 patch to become an offset anchor
    /// (`fill_offset = anchored-retry`) [default: 0.35].
    #[arg(long, value_name = "N")]
    pub fill_anchor_min_correlation: Option<f32>,

    /// Include structure-trusted gate patches (no waveform) in the anchor table
    /// (`fill_offset = anchored-retry`). Default excludes them.
    #[arg(long)]
    pub fill_anchor_include_structure_trusted: bool,

    /// Reject anchors whose fine slide exceeds this fraction of `fill_border_search_secs`
    /// (`fill_offset = anchored-retry`) [default: 0.9].
    #[arg(long, value_name = "N")]
    pub fill_anchor_max_adjustment_frac: Option<f64>,

    /// Fit mode: penalize unified-search candidates far from patch-anchor prediction
    /// (`fill_offset = anchored-retry`; 0 = off) [default: 0].
    #[arg(long, value_name = "N")]
    pub fill_anchor_search_prior_weight: Option<f64>,

    /// `anchored_retry` pass 2: re-run fit-mode marginal pass-1 patches with anchored offset;
    /// keep pass 2 only when seam confidence upgrades to `high`.
    #[arg(long)]
    pub fill_anchor_retry_marginal: bool,

    /// Gap-fill placement: `gate` (legacy threshold checks) or `fit` (waveform slide search)
    /// [default: fit].
    #[arg(long, value_parser = clap::value_parser!(FillMode), value_name = "MODE")]
    pub fill_mode: Option<FillMode>,

    /// Residual/floor headroom gate (fit mode only): `off`, `veto`, or `veto_rescue` [default: veto].
    #[arg(long, value_parser = clap::value_parser!(crate::domain::ResidualGateMode), value_name = "MODE")]
    pub residual_gate: Option<crate::domain::ResidualGateMode>,

    /// Nominal floor ceiling (dB) for informative residual gating [default: -15].
    #[arg(long, value_name = "DB")]
    pub residual_floor_ok_db: Option<f64>,

    /// Max residual headroom (dB) before veto/rescue [default: 6].
    #[arg(long, value_name = "DB")]
    pub residual_headroom_margin_db: Option<f64>,

    /// Unified seam/floor lag search radius (seconds) [default: 0.010].
    #[arg(long, value_name = "SECS")]
    pub residual_lag_secs: Option<f64>,

    /// Draft repair profile: smaller haystack, no extension, baseline-only fit path.
    #[arg(long, conflicts_with = "full")]
    pub quick: bool,

    /// Quality repair profile: full boundary grid and extension retries.
    #[arg(long, conflicts_with = "quick")]
    pub full: bool,

    /// Explicit repair profile (`default`, `quick`, `full`).
    #[arg(long, value_parser = clap::value_parser!(RepairProfile), value_name = "NAME")]
    pub profile: Option<RepairProfile>,

    /// Disable post-seam gap-end extension.
    /// Fit + full grid: disables joint grid end axis. Fit + baseline_only: no effect until --full.
    /// Gate: disables post-seam retry loop.
    #[arg(long)]
    pub no_gap_end_extend: bool,

    /// Disable pre-seam gap-start extension.
    /// Fit + full grid: disables joint grid start axis. Fit + baseline_only: no effect until --full.
    /// Gate: disables pre-seam retry loop.
    #[arg(long)]
    pub no_gap_start_extend: bool,

    /// Disable short-gap fallback that patches when either seam passes (after mean rule fails).
    /// [fill-mode: gate only]
    #[arg(long)]
    pub no_short_gap_one_strong_seam: bool,

    /// Maximum A-boundary shift for extension (ms) [default: 500].
    /// Fit + full grid: grid span and B haystack slack. Gate: retry span and haystack slack.
    /// Inert under fit + baseline_only (default profile); `-v` logs when inactive.
    #[arg(long, value_name = "MS")]
    pub gap_end_extend_max_ms: Option<u64>,

    /// Step size for A-boundary extension (ms) [default: 20].
    /// Fit + full grid: grid step. Gate: retry step. Inert under fit + baseline_only.
    #[arg(long, value_name = "MS")]
    pub gap_end_extend_step_ms: Option<u64>,

    /// Editorial seam anchor search (`off`, `auto`, `force`); fit mode only [default: off].
    #[arg(long, value_parser = clap::value_parser!(crate::domain::AnchorSeamMode), value_name = "MODE")]
    pub anchor_seam_mode: Option<crate::domain::AnchorSeamMode>,

    /// Maximum A bracket span when searching editorial seam anchors (seconds) [default: 5.0].
    #[arg(long, value_name = "SECS")]
    pub max_anchor_bracket_secs: Option<f64>,

    /// Max anchor candidates per side of the scan hole [default: 5].
    #[arg(long, value_name = "N")]
    pub max_anchors_per_side: Option<usize>,

    /// Minimum energy-bin prominence for peak anchor candidates [default: 0].
    #[arg(long, value_name = "N")]
    pub anchor_seam_min_prominence: Option<f32>,

    /// Crossfade duration at gap boundaries (ms) [default: 10].
    #[arg(long, value_name = "MS")]
    pub crossfade_ms: Option<u64>,

    /// Override: number of alignment clips per video [default: 2].
    #[arg(long, value_name = "N")]
    pub num_clips: Option<u32>,

    /// Show diagnostics and verbose progress.
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress progress output.
    #[arg(short, long)]
    pub quiet: bool,

    /// Log level for tracing [default: info].
    #[arg(long, value_enum)]
    pub log_level: Option<LogLevelArg>,

    /// Write logs to a file (also logs to stderr).
    #[arg(long)]
    pub log_file: Option<PathBuf>,

    /// Try every decodable audio track on both files and keep the best alignment [default: disabled].
    #[arg(long, overrides_with = "no_try_all_tracks")]
    pub try_all_tracks: bool,

    /// Disable try-all-tracks (overrides config).
    #[arg(long, overrides_with = "try_all_tracks")]
    pub no_try_all_tracks: bool,

    /// After discovery alignment, FFT-refine offset on a short native-rate hold-out segment [default: enabled].
    #[arg(long, overrides_with = "no_refine_offset_high_rate")]
    pub refine_offset_high_rate: bool,

    /// Disable high-rate offset refinement (overrides config).
    #[arg(long, overrides_with = "refine_offset_high_rate")]
    pub no_refine_offset_high_rate: bool,

    /// When start/end Chromaprint offsets disagree, PCM-search the end clip around the
    /// high-confidence start offset [default: enabled].
    #[arg(long, overrides_with = "no_constrain_end_clip_to_start_offset")]
    pub constrain_end_clip_to_start_offset: bool,

    /// Keep independent end-clip Chromaprint when it disagrees with start (overrides config).
    #[arg(long, overrides_with = "constrain_end_clip_to_start_offset")]
    pub no_constrain_end_clip_to_start_offset: bool,

    /// After dual-anchor high-rate refinement, recompute recommended offset from updated
    /// clip offsets [default: enabled].
    #[arg(long, overrides_with = "no_high_rate_recommended_refusion")]
    pub high_rate_recommended_refusion: bool,

    /// Apply only the start-anchor high-rate tweak to recommended offset (legacy behavior).
    #[arg(long, overrides_with = "high_rate_recommended_refusion")]
    pub no_high_rate_recommended_refusion: bool,

    /// Force query-reference alignment (short clip localized against long recording).
    #[arg(long)]
    pub query_reference: bool,

    /// Force symmetric multi-clip alignment (legacy behaviour).
    #[arg(long)]
    pub symmetric_align: bool,

    /// Coarse search stride on the reference timeline when using query-reference mode (seconds) [default: 60].
    #[arg(long, value_name = "SECS")]
    pub query_stride: Option<f64>,

    /// When using query-reference mode, allow filling gaps outside the located clip coverage
    /// [default: gaps outside mapped region are not fillable].
    #[arg(long)]
    pub no_limit_fill_region: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LogLevelArg {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevelArg> for LogLevel {
    fn from(value: LogLevelArg) -> Self {
        match value {
            LogLevelArg::Error => LogLevel::Error,
            LogLevelArg::Warn => LogLevel::Warn,
            LogLevelArg::Info => LogLevel::Info,
            LogLevelArg::Debug => LogLevel::Debug,
            LogLevelArg::Trace => LogLevel::Trace,
        }
    }
}

fn parse_duration(raw: &str) -> Result<std::time::Duration, String> {
    let raw = raw.trim().to_ascii_lowercase();
    if let Some(stripped) = raw.strip_suffix('m') {
        let minutes: u64 = stripped
            .parse()
            .map_err(|_| format!("invalid duration: {raw}"))?;
        return Ok(std::time::Duration::from_secs(minutes * 60));
    }
    if let Some(stripped) = raw.strip_suffix('s') {
        let seconds: u64 = stripped
            .parse()
            .map_err(|_| format!("invalid duration: {raw}"))?;
        return Ok(std::time::Duration::from_secs(seconds));
    }
    let seconds: u64 = raw
        .parse()
        .map_err(|_| format!("invalid duration: {raw}"))?;
    Ok(std::time::Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use crate::infrastructure::config::RepairAppConfig;

    #[test]
    fn parse_duration_minutes() {
        assert_eq!(parse_duration("15m").unwrap().as_secs(), 15 * 60);
    }

    #[test]
    fn parse_duration_seconds() {
        assert_eq!(parse_duration("90s").unwrap().as_secs(), 90);
    }

    #[test]
    fn optional_overrides_remain_none_when_omitted() {
        let args = Args::parse_from(["clip-sync-repair", "a.wav", "b.wav"]);
        assert!(args.min_gap_ms.is_none());
        assert!(args.clip_length.is_none());
        assert!(args.query_stride.is_none());
        assert!(args.log_level.is_none());
    }

    #[test]
    fn help_documents_repair_defaults() {
        let defaults = RepairAppConfig::default();
        let help = Args::command().render_help().to_string();

        for needle in [
            "[default: 15m]",
            &format!("[default: {}]", defaults.repair.min_gap_ms),
            &format!("[default: {}]", defaults.repair.silence_peak_fraction),
            &format!("[default: {}]", defaults.repair.decode_chunk_secs),
            &format!("[default: {}]", defaults.repair.scan_block_ms),
            &format!("[default: {}]", defaults.repair.silence_hold_ms),
            &format!("[default: {}]", defaults.repair.absolute_silence_rms as u32),
            &format!("[default: {}]", defaults.repair.min_fill_correlation),
            &format!("[default: {}]", defaults.repair.max_fill_align_adjustment_secs),
            &format!("[default: {}]", defaults.repair.fill_border_search_secs),
            &format!("[default: {}]", defaults.repair.fill_align_margin_secs),
            &format!("[default: {}]", defaults.repair.gap_signature_context_secs),
            &format!("[default: {}]", defaults.repair.fill_length_slack_secs),
            &format!("[default: {}]", defaults.repair.fill_fit_structure_weight),
            &format!("[default: {}]", defaults.repair.fill_fit_waveform_weight),
            &format!("[default: {}]", defaults.repair.fill_repeat_penalty_weight),
            &format!("[default: {}]", defaults.repair.border_standoff_secs),
            &format!("[default: {}]", defaults.repair.crossfade_ms),
            &format!("[default: {}]", defaults.repair.fill_anchor_min_correlation),
            &format!("[default: {}]", defaults.repair.fill_anchor_max_adjustment_frac),
            &format!("[default: {}]", defaults.repair.fill_anchor_search_prior_weight),
            "[default: auto]",
            "anchored-retry",
            "gap-signature-mode",
            "fill-anchor-min-correlation",
            "fill-anchor-include-structure-trusted",
            "fill-anchor-max-adjustment-frac",
            "fill-anchor-search-prior-weight",
            "fill-border-search-secs",
            "fill-align-margin-secs",
            "gap-signature-context-secs",
            "fill-length-slack-secs",
            "[default: fit]",
            "[fill-mode: gate only]",
            "[default: 2]",
            "[default: 60]",
            "[default: info]",
            "[default: enabled]",
            "[default: disabled]",
        ] {
            assert!(
                help.contains(needle),
                "help missing {needle:?}:\n{help}"
            );
        }
    }
}
