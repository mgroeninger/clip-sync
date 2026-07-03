pub mod args;
pub mod exit_code;
pub mod output;

use clip_sync::{AlignmentMode, ProgressMode};

use crate::domain::{RepairProfile, RepairProfileFieldMask};
use crate::infrastructure::config::RepairAppConfig;

use self::args::Args;

pub fn apply_cli_overrides(config: &mut RepairAppConfig, args: &Args) {
    if let Some(profile) = resolve_cli_profile(args) {
        config.repair.profile = profile;
        config.repair.apply_profile_bundle(RepairProfileFieldMask::default());
    }
    if let Some(duration) = args.clip_length {
        config.align.clip.clip_length = duration;
    }
    if let Some(ms) = args.min_gap_ms {
        config.repair.min_gap_ms = ms;
    }
    if let Some(f) = args.silence_fraction {
        config.repair.silence_peak_fraction = f;
    }
    if let Some(s) = args.decode_chunk_secs {
        config.repair.decode_chunk_secs = s;
    }
    if let Some(ms) = args.scan_block_ms {
        config.repair.scan_block_ms = ms;
    }
    if let Some(ms) = args.silence_hold_ms {
        config.repair.silence_hold_ms = ms;
    }
    if let Some(rms) = args.absolute_silence_rms {
        config.repair.absolute_silence_rms = rms;
    }
    if args.scan_both {
        config.repair.scan_both = true;
    } else if args.no_scan_both {
        config.repair.scan_both = false;
    }
    if args.dual_fit {
        config.repair.dual_fit = true;
    }
    if let Some(wav_path) = &args.wav {
        config.repair.output.wav_path = Some(wav_path.clone());
        config.repair.dry_run = false;
    }
    #[cfg(feature = "ffmpeg-mux")]
    if let Some(mux_path) = &args.mux {
        config.repair.output.video_path = Some(mux_path.clone());
        config.repair.dry_run = false;
    }
    if args.no_normalize {
        config.repair.normalize_fill = false;
    }
    if args.no_structure_trust {
        config.repair.disable_structure_trust = true;
    }
    if let Some(corr) = args.min_fill_correlation {
        config.repair.min_fill_correlation = corr;
    }
    if let Some(secs) = args.max_fill_align_adjust_secs {
        config.repair.max_fill_align_adjustment_secs = secs;
    }
    if let Some(secs) = args.fill_border_search_secs {
        config.repair.fill_border_search_secs = secs;
    }
    if let Some(secs) = args.fill_align_margin_secs {
        config.repair.fill_align_margin_secs = secs;
    }
    if let Some(secs) = args.gap_signature_context_secs {
        config.repair.gap_signature_context_secs = secs;
    }
    if let Some(secs) = args.fill_length_slack_secs {
        config.repair.fill_length_slack_secs = secs;
    }
    if let Some(secs) = args.border_standoff_secs {
        config.repair.border_standoff_secs = secs;
    }
    if let Some(mode) = args.fill_offset {
        config.repair.fill_offset_mode = mode;
    }
    if let Some(mode) = args.gap_signature_mode {
        config.repair.gap_signature_mode = mode;
    }
    if let Some(corr) = args.fill_anchor_min_correlation {
        config.repair.fill_anchor_min_correlation = corr;
    }
    if args.fill_anchor_include_structure_trusted {
        config.repair.fill_anchor_exclude_structure_trusted = false;
    }
    if let Some(frac) = args.fill_anchor_max_adjustment_frac {
        config.repair.fill_anchor_max_adjustment_frac = frac;
    }
    if let Some(w) = args.fill_anchor_search_prior_weight {
        config.repair.fill_anchor_search_prior_weight = w;
    }
    if args.fill_anchor_retry_marginal {
        config.repair.fill_anchor_retry_marginal = true;
    }
    if let Some(mode) = args.fill_mode {
        config.repair.fill_mode = mode;
    }
    if let Some(mode) = args.residual_gate {
        config.repair.residual_gate = mode;
    }
    if let Some(db) = args.residual_floor_ok_db {
        config.repair.residual_floor_ok_db = db;
    }
    if let Some(db) = args.residual_headroom_margin_db {
        config.repair.residual_headroom_margin_db = db;
    }
    if let Some(secs) = args.residual_lag_secs {
        config.repair.residual_lag_secs = secs;
    }
    if let Some(w) = args.fill_fit_structure_weight {
        config.repair.fill_fit_structure_weight = w;
    }
    if let Some(w) = args.fill_fit_waveform_weight {
        config.repair.fill_fit_waveform_weight = w;
    }
    if let Some(w) = args.fill_repeat_penalty_weight {
        config.repair.fill_repeat_penalty_weight = w;
    }
    if args.no_gap_end_extend {
        config.repair.gap_end_extend_on_post_seam_fail = false;
    }
    if args.no_gap_start_extend {
        config.repair.gap_start_extend_on_pre_seam_fail = false;
    }
    if args.no_short_gap_one_strong_seam {
        config.repair.short_gap_one_strong_seam_fallback = false;
    }
    if let Some(ms) = args.gap_end_extend_max_ms {
        config.repair.gap_end_extend_max_ms = ms;
    }
    if let Some(ms) = args.gap_end_extend_step_ms {
        config.repair.gap_end_extend_step_ms = ms;
    }
    if let Some(mode) = args.anchor_seam_mode {
        config.repair.anchor_seam_mode = mode;
    }
    if let Some(secs) = args.max_anchor_bracket_secs {
        config.repair.max_anchor_bracket_secs = secs;
    }
    if let Some(n) = args.max_anchors_per_side {
        config.repair.max_anchors_per_side = n;
    }
    if let Some(p) = args.anchor_seam_min_prominence {
        config.repair.anchor_seam_min_prominence = p;
    }
    if let Some(ms) = args.crossfade_ms {
        config.repair.crossfade_ms = ms;
    }
    if let Some(num_clips) = args.num_clips {
        config.align.clip.num_clips = num_clips;
    }
    if args.verbose {
        config.logging.progress = ProgressMode::Verbose;
    }
    if args.quiet {
        config.logging.progress = ProgressMode::Quiet;
    }
    if let Some(level) = args.log_level {
        config.logging.level = level.into();
    }
    if let Some(path) = &args.log_file {
        config.logging.log_file = Some(path.clone());
    }
    if args.try_all_tracks {
        config.align.alignment.try_all_tracks = true;
    } else if args.no_try_all_tracks {
        config.align.alignment.try_all_tracks = false;
    }
    if args.refine_offset_high_rate {
        config.align.alignment.refine_offset_high_rate = true;
    } else if args.no_refine_offset_high_rate {
        config.align.alignment.refine_offset_high_rate = false;
    }
    if args.constrain_end_clip_to_start_offset {
        config.align.alignment.constrain_end_clip_to_start_offset = true;
    } else if args.no_constrain_end_clip_to_start_offset {
        config.align.alignment.constrain_end_clip_to_start_offset = false;
    }
    if args.high_rate_recommended_refusion {
        config.align.alignment.high_rate_recommended_refusion = true;
    } else if args.no_high_rate_recommended_refusion {
        config.align.alignment.high_rate_recommended_refusion = false;
    }
    if args.query_reference {
        config.align.alignment.mode = AlignmentMode::QueryReference;
    } else if args.symmetric_align {
        config.align.alignment.mode = AlignmentMode::Symmetric;
    }
    if let Some(stride) = args.query_stride {
        config.align.alignment.query_search_stride_secs = stride;
    }
    if args.no_limit_fill_region {
        config.repair.limit_fill_to_mapped_region = false;
    }
}

fn resolve_cli_profile(args: &Args) -> Option<RepairProfile> {
    if args.quick {
        Some(RepairProfile::Quick)
    } else if args.full {
        Some(RepairProfile::Full)
    } else {
        args.profile
    }
}

/// Reject incompatible repair profile CLI flags.
pub(crate) fn validate_repair_profile_flags(args: &Args) -> Result<(), String> {
    if args.quick && args.full {
        return Err("cannot use --quick and --full together".into());
    }
    Ok(())
}

#[cfg(test)]
mod cli_override_tests {
    use super::*;
    use clip_sync::AlignmentMode;
    use crate::domain::FillOffsetMode;
    use crate::infrastructure::config::RepairAppConfig;

    #[test]
    fn query_reference_cli_overrides_config() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--query-reference",
            "--query-stride",
            "45",
            "--no-limit-fill-region",
        ]);
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert_eq!(config.align.alignment.mode, AlignmentMode::QueryReference);
        assert!((config.align.alignment.query_search_stride_secs - 45.0).abs() < f64::EPSILON);
        assert!(!config.repair.limit_fill_to_mapped_region);
    }

    #[test]
    fn no_structure_trust_cli_overrides_config() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--no-structure-trust",
        ]);
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert!(config.repair.disable_structure_trust);
    }

    #[test]
    fn patch_and_scan_cli_overrides_config() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--silence-hold-ms",
            "400",
            "--absolute-silence-rms",
            "25",
            "--min-fill-correlation",
            "0.45",
            "--max-fill-align-adjust-secs",
            "0.25",
            "--border-standoff-secs",
            "0.5",
            "--fill-border-search-secs",
            "5",
            "--fill-align-margin-secs",
            "0.5",
            "--gap-signature-context-secs",
            "2",
            "--fill-length-slack-secs",
            "3",
            "--fill-offset",
            "interpolated",
            "--fill-fit-structure-weight",
            "0.4",
            "--fill-fit-waveform-weight",
            "0.6",
            "--fill-repeat-penalty-weight",
            "0.25",
            "--no-gap-end-extend",
            "--no-gap-start-extend",
            "--no-short-gap-one-strong-seam",
            "--gap-end-extend-max-ms",
            "300",
            "--gap-end-extend-step-ms",
            "10",
        ]);
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert_eq!(config.repair.silence_hold_ms, 400);
        assert!((config.repair.absolute_silence_rms - 25.0).abs() < f32::EPSILON);
        assert!((config.repair.min_fill_correlation - 0.45).abs() < f32::EPSILON);
        assert!((config.repair.max_fill_align_adjustment_secs - 0.25).abs() < f64::EPSILON);
        assert!((config.repair.fill_border_search_secs - 5.0).abs() < f64::EPSILON);
        assert!((config.repair.fill_align_margin_secs - 0.5).abs() < f64::EPSILON);
        assert!((config.repair.gap_signature_context_secs - 2.0).abs() < f64::EPSILON);
        assert!((config.repair.fill_length_slack_secs - 3.0).abs() < f64::EPSILON);
        assert!((config.repair.border_standoff_secs - 0.5).abs() < f64::EPSILON);
        assert_eq!(config.repair.fill_offset_mode, FillOffsetMode::Interpolated);
        assert!((config.repair.fill_fit_structure_weight - 0.4).abs() < f64::EPSILON);
        assert!((config.repair.fill_fit_waveform_weight - 0.6).abs() < f64::EPSILON);
        assert!((config.repair.fill_repeat_penalty_weight - 0.25).abs() < f64::EPSILON);
        assert!(!config.repair.gap_end_extend_on_post_seam_fail);
        assert!(!config.repair.gap_start_extend_on_pre_seam_fail);
        assert!(!config.repair.short_gap_one_strong_seam_fallback);
        assert_eq!(config.repair.gap_end_extend_max_ms, 300);
        assert_eq!(config.repair.gap_end_extend_step_ms, 10);
    }

    #[test]
    fn anchor_and_signature_cli_overrides_config() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--fill-offset",
            "anchored-retry",
            "--gap-signature-mode",
            "energy",
            "--fill-anchor-min-correlation",
            "0.4",
            "--fill-anchor-include-structure-trusted",
            "--fill-anchor-max-adjustment-frac",
            "0.8",
            "--fill-anchor-search-prior-weight",
            "0.15",
        ]);
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert_eq!(config.repair.fill_offset_mode, FillOffsetMode::AnchoredRetry);
        assert_eq!(
            config.repair.gap_signature_mode,
            crate::domain::GapSignatureMode::Energy
        );
        assert!((config.repair.fill_anchor_min_correlation - 0.4).abs() < f32::EPSILON);
        assert!(!config.repair.fill_anchor_exclude_structure_trusted);
        assert!((config.repair.fill_anchor_max_adjustment_frac - 0.8).abs() < f64::EPSILON);
        assert!((config.repair.fill_anchor_search_prior_weight - 0.15).abs() < f64::EPSILON);
    }

    #[test]
    fn quick_cli_applies_profile_bundle() {
        use clap::Parser;
        let args = crate::infrastructure::cli::args::Args::try_parse_from([
            "clip-sync-repair",
            "a.mkv",
            "b.mkv",
            "--quick",
        ])
        .expect("parse args");
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert_eq!(config.repair.profile, RepairProfile::Quick);
        assert!((config.repair.fill_border_search_secs - 5.0).abs() < f64::EPSILON);
        assert!(!config.repair.gap_end_extend_on_post_seam_fail);
        assert!(!config.repair.gap_start_extend_on_pre_seam_fail);
        assert_eq!(
            config.repair.fit_boundary_search,
            crate::domain::FitBoundarySearch::BaselineOnly
        );
    }

    #[test]
    fn full_cli_applies_profile_bundle() {
        use clap::Parser;
        let args = crate::infrastructure::cli::args::Args::try_parse_from([
            "clip-sync-repair",
            "a.mkv",
            "b.mkv",
            "--full",
        ])
        .expect("parse args");
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert_eq!(config.repair.profile, RepairProfile::Full);
        assert_eq!(
            config.repair.fit_boundary_search,
            crate::domain::FitBoundarySearch::FullGrid
        );
    }

    #[test]
    fn quick_cli_override_border_search_secs() {
        use clap::Parser;
        let args = crate::infrastructure::cli::args::Args::try_parse_from([
            "clip-sync-repair",
            "a.mkv",
            "b.mkv",
            "--quick",
            "--fill-border-search-secs",
            "8",
        ])
        .expect("parse args");
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert!((config.repair.fill_border_search_secs - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn quick_and_full_together_is_rejected() {
        use clap::Parser;
        let err = crate::infrastructure::cli::args::Args::try_parse_from([
            "clip-sync-repair",
            "a.mkv",
            "b.mkv",
            "--quick",
            "--full",
        ])
        .unwrap_err();
        assert!(
            err.to_string().contains("quick")
                && err.to_string().contains("full"),
            "unexpected clap error: {err}"
        );
    }
}
