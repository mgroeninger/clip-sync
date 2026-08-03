pub mod args;
pub mod exit_code;
pub mod output;

use clip_sync::{AlignmentMode, ProgressMode};

use crate::domain::RepairProfile;
use crate::infrastructure::config::RepairAppConfig;

use self::args::Args;

pub fn apply_cli_overrides(config: &mut RepairAppConfig, args: &Args) {
    if let Some(profile) = resolve_cli_profile(args) {
        config.repair.profile = profile;
        // Preserve fields the user set explicitly in TOML (same mask the loader used).
        let mask = config.repair.profile_field_mask;
        config.repair.apply_profile_bundle(mask);
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
        // CLI documents 0–32767 i16 units; config stores normalized amplitude.
        config.repair.absolute_silence_rms =
            crate::infrastructure::config::normalize_absolute_silence_rms_i16(rms);
    }
    if args.scan_both {
        config.repair.scan_both = true;
    } else if args.no_scan_both {
        config.repair.scan_both = false;
    }
    if args.skip_equivalent_gaps {
        config.repair.skip_equivalent_gaps = true;
    } else if args.no_skip_equivalent_gaps {
        config.repair.skip_equivalent_gaps = false;
    }
    if let Some(tokens) = &args.only_gaps {
        config.repair.only_gaps = Some(tokens.clone());
        config.repair.skip_gaps = None;
    }
    if let Some(tokens) = &args.skip_gaps {
        config.repair.skip_gaps = Some(tokens.clone());
        config.repair.only_gaps = None;
    }
    if args.dual_fit {
        config.repair.dual_fit = true;
    } else if args.no_dual_fit {
        config.repair.dual_fit = false;
    }
    #[cfg(feature = "calibration")]
    if args.fingerprint_diagnostics {
        config.repair.fingerprint_diagnostics = true;
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
    // Run-mode flag next to dry_run/outputs: preview keeps dry_run (no file write) and is
    // mutually exclusive with output paths via `RepairConfig::validate`.
    if args.repair_preview {
        config.repair.repair_preview = true;
        config.repair.dry_run = true;
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
        config.repair.profile_field_mask.fill_border_search_secs = true;
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
    if let Some(secs) = args.fill_extract_tail_slack_secs {
        config.repair.fill_extract_tail_slack_secs = secs;
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
    if args.no_fft_seam_search {
        config.repair.fft_seam_search = false;
    }
    if args.no_fft_repeat_band {
        config.repair.fft_repeat_band = false;
    }
    if args.no_gap_end_extend {
        config.repair.gap_end_extend_on_post_seam_fail = false;
        config
            .repair
            .profile_field_mask
            .gap_end_extend_on_post_seam_fail = true;
    }
    if args.no_gap_start_extend {
        config.repair.gap_start_extend_on_pre_seam_fail = false;
        config
            .repair
            .profile_field_mask
            .gap_start_extend_on_pre_seam_fail = true;
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

/// `--fingerprint-gap`, `--fingerprint-diagnostics` and `--gap-listen` only apply when dumping a
/// corpus; `--gap-listen` additionally rejects the run modes that cannot produce a patched WAV.
/// The flags exist only under the `calibration` feature, so this is a no-op otherwise.
pub(crate) fn validate_fingerprint_flags(args: &Args) -> Result<(), String> {
    #[cfg(feature = "calibration")]
    {
        if args.gap_listen.is_some() {
            validate_gap_listen_run_mode(args)?;
        }
        if args.gap_fingerprints.is_some() {
            return Ok(());
        }
        if !args.fingerprint_gap.is_empty() {
            return Err("--fingerprint-gap requires --gap-fingerprints DIR".into());
        }
        if args.fingerprint_diagnostics {
            return Err("--fingerprint-diagnostics requires --gap-fingerprints DIR".into());
        }
        if args.gap_listen.is_some() {
            return Err("--gap-listen requires --gap-fingerprints DIR".into());
        }
    }
    #[cfg(not(feature = "calibration"))]
    let _ = args;
    Ok(())
}

/// Run modes `--gap-listen` refuses, each because it cannot deliver the patched WAV that is the
/// point of the flag. Every rejection is explicit: silently degrading a multi-hour diagnostic run
/// to two-thirds of its output is the misleading-diagnostics failure this feature exists to avoid,
/// and is why this deliberately diverges from the warn-and-ignore `--mux` / `--gap-fingerprints`
/// precedent.
///
/// **`dry_run` is not in this list.** It defaults to `true` and is cleared only by an output flag,
/// so a scan-only run *is* the canonical listen invocation — rejecting it would reject everything.
#[cfg(feature = "calibration")]
fn validate_gap_listen_run_mode(args: &Args) -> Result<(), String> {
    if args.repair_preview {
        return Err("--gap-listen cannot be used with --repair-preview: preview stops before \
                    execute, so no patched audio exists to export"
            .into());
    }
    if args.wav.is_some() {
        return Err("--gap-listen cannot be used with --wav: run the listen pass for gap clips, \
                    then a separate --wav pass for the full patched timeline"
            .into());
    }
    if args.mux.is_some() {
        return Err("--gap-listen cannot be used with --mux".into());
    }
    Ok(())
}

/// `--gap-listen` takes its gap set from `--fingerprint-gap`, so a second selector must be refused.
///
/// Listen is a *mode of the fingerprint dump* — it cannot run without `--gap-fingerprints`, and the
/// WAV stems are the corpus entry stems — so the dump's selector is the only one that makes sense.
/// `--only-gaps` / `--skip-gaps` would leave the corpus and the fill plan covering different gap
/// sets, which is the two-list drift this design exists to prevent.
///
/// Separate from [`validate_gap_listen_run_mode`] because the reason is different in kind: those
/// flags *cannot* produce a patched WAV, whereas these merely compete for the same job.
///
/// Takes `config` rather than just `args` so a `only_gaps`/`skip_gaps` key in the TOML is caught
/// too — [`apply_cli_overrides`] has already folded the CLI flags in by the time this runs. Still
/// **pre-scan**: on real media the scan is the expensive part, and a conflict this cheap to detect
/// must never cost one.
#[cfg(feature = "calibration")]
pub(crate) fn validate_gap_listen_selector(
    args: &Args,
    config: &RepairAppConfig,
) -> Result<(), String> {
    if args.gap_listen.is_none() {
        return Ok(());
    }
    let second = if config.repair.only_gaps.is_some() {
        "only_gaps / --only-gaps"
    } else if config.repair.skip_gaps.is_some() {
        "skip_gaps / --skip-gaps"
    } else {
        return Ok(());
    };
    Err(format!(
        "--gap-listen selects gaps with --fingerprint-gap, not {second} (one selector drives both \
         the corpus and the patch plan)"
    ))
}

#[cfg(test)]
mod cli_override_tests {
    use super::*;
    use crate::domain::FillOffsetMode;
    use crate::infrastructure::config::RepairAppConfig;
    use clip_sync::AlignmentMode;

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

        let args = Args::parse_from(["clip-sync-repair", "a.wav", "b.wav", "--no-structure-trust"]);
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
        assert!(
            (config.repair.absolute_silence_rms - (25.0 / 32767.0)).abs() < 1e-9,
            "CLI 0–32767 units must normalize; got {}",
            config.repair.absolute_silence_rms
        );
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
        assert_eq!(
            config.repair.fill_offset_mode,
            FillOffsetMode::AnchoredRetry
        );
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
    fn quick_cli_preserves_toml_explicit_border_search() {
        use clap::Parser;
        // TOML set fill_border_search_secs explicitly; --quick must not stomp it.
        let mut config = RepairAppConfig::default();
        config.repair.fill_border_search_secs = 12.0;
        config.repair.profile_field_mask.fill_border_search_secs = true;
        let args = crate::infrastructure::cli::args::Args::try_parse_from([
            "clip-sync-repair",
            "a.mkv",
            "b.mkv",
            "--quick",
        ])
        .expect("parse args");
        apply_cli_overrides(&mut config, &args);
        assert_eq!(config.repair.profile, RepairProfile::Quick);
        assert!((config.repair.fill_border_search_secs - 12.0).abs() < f64::EPSILON);
        // Bundle fields not masked still take the quick profile.
        assert!(!config.repair.gap_end_extend_on_post_seam_fail);
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
            err.to_string().contains("quick") && err.to_string().contains("full"),
            "unexpected clap error: {err}"
        );
    }

    #[test]
    fn repair_preview_with_wav_is_rejected_by_config_validate() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--repair-preview",
            "--wav",
            "out.wav",
        ]);
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        let err = config.repair.validate().expect_err("preview + wav");
        assert!(
            format!("{err:?}").contains("repair_preview"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn repair_preview_cli_sets_config_flag_and_keeps_dry_run() {
        use clap::Parser;

        let args = Args::parse_from(["clip-sync-repair", "a.wav", "b.wav", "--repair-preview"]);
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert!(config.repair.repair_preview);
        assert!(config.repair.dry_run);
        config.repair.validate().expect("preview alone is valid");
    }

    #[cfg(feature = "calibration")]
    #[test]
    fn fingerprint_gap_without_dump_dir_is_rejected() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--fingerprint-gap",
            "3",
        ]);
        let err = validate_fingerprint_flags(&args).unwrap_err();
        assert!(err.contains("--gap-fingerprints"));
    }

    #[cfg(feature = "calibration")]
    #[test]
    fn fingerprint_diagnostics_without_dump_dir_is_rejected() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--fingerprint-diagnostics",
        ]);
        let err = validate_fingerprint_flags(&args).unwrap_err();
        assert!(err.contains("--gap-fingerprints"));
    }

    #[cfg(feature = "calibration")]
    #[test]
    fn fingerprint_flags_ok_with_dump_dir() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--gap-fingerprints",
            "gap-files/out",
            "--fingerprint-gap",
            "3",
            "--fingerprint-diagnostics",
        ]);
        validate_fingerprint_flags(&args).expect("valid fingerprint flag combo");
    }

    #[cfg(feature = "calibration")]
    #[test]
    fn gap_listen_without_dump_dir_is_rejected() {
        use clap::Parser;

        let args = Args::parse_from(["clip-sync-repair", "a.wav", "b.wav", "--gap-listen"]);
        let err = validate_fingerprint_flags(&args).unwrap_err();
        assert!(err.contains("--gap-fingerprints"), "got {err}");
    }

    /// Each run mode that cannot produce the patched WAV must be refused by name, and the message
    /// must say which flag collided — a bare "invalid combination" would send the user back to the
    /// docs after they have already paid for a scan.
    #[cfg(feature = "calibration")]
    #[test]
    fn gap_listen_rejects_run_modes_that_cannot_produce_a_patched_wav() {
        use clap::Parser;

        // `--mux` is declared unconditionally (the `ffmpeg-mux` feature gates muxing, not the flag),
        // so all three are reachable in a default calibration build.
        for flag in [
            vec!["--repair-preview"],
            vec!["--wav", "out.wav"],
            vec!["--mux", "out.mkv"],
        ] {
            let mut argv = vec![
                "clip-sync-repair",
                "a.wav",
                "b.wav",
                "--gap-fingerprints",
                "gap-files/out",
                "--gap-listen",
            ];
            argv.extend_from_slice(&flag);
            let args = Args::parse_from(argv);
            let Err(err) = validate_fingerprint_flags(&args) else {
                panic!("{flag:?} must be rejected alongside --gap-listen");
            };
            assert!(
                err.contains("--gap-listen") && err.contains(flag[0]),
                "message must name both flags, got {err}"
            );
        }
    }

    /// The second selector must be refused before the scan. Regression guard for the version that
    /// only caught this in `composition::gap_listen`, i.e. after a full scan had been paid for.
    /// Both polarities, and both the flag and the TOML key, since `apply_cli_overrides` has already
    /// merged them by the time the check runs.
    #[cfg(feature = "calibration")]
    #[test]
    fn gap_listen_rejects_a_second_gap_selector_before_the_scan() {
        use clap::Parser;

        for extra in [["--only-gaps", "3"], ["--skip-gaps", "3"]] {
            let args = Args::parse_from([
                "clip-sync-repair",
                "a.wav",
                "b.wav",
                "--gap-fingerprints",
                "gap-files/out",
                "--gap-listen",
                extra[0],
                extra[1],
            ]);
            let mut config = RepairAppConfig::default();
            apply_cli_overrides(&mut config, &args);
            let err = validate_gap_listen_selector(&args, &config).unwrap_err();
            assert!(
                err.contains("--fingerprint-gap") && err.contains(extra[0]),
                "message must name the rejected selector and the one to use instead, got {err}"
            );
        }
    }

    /// `--fingerprint-gap` is the selector `--gap-listen` *wants*, so it must survive the check that
    /// used to reject it — and it must reach the fill plan as an `Only` selection, not just the
    /// corpus filter it is on a plain dump run.
    #[cfg(feature = "calibration")]
    #[test]
    fn gap_listen_accepts_fingerprint_gap_and_it_bounds_the_plan() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--gap-fingerprints",
            "gap-files/out",
            "--gap-listen",
            "--fingerprint-gap",
            "3,7",
        ]);
        assert_eq!(args.fingerprint_gap, vec![3, 7], "comma form must split");
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert!(validate_fingerprint_flags(&args).is_ok());
        assert!(validate_gap_listen_selector(&args, &config).is_ok());
        // The conversion `composition::gap_listen` performs: 1-based numbers → `--only-gaps` tokens.
        assert_eq!(
            crate::domain::gap_fill::GapSelectionMode::Only(
                args.fingerprint_gap.iter().map(usize::to_string).collect()
            ),
            crate::domain::gap_fill::GapSelectionMode::Only(vec!["3".into(), "7".into()])
        );
    }

    /// The canonical listen invocation — no output flags, so `dry_run` stays `true`. This is the
    /// row the `--dry-run` rejection would have broken had it survived review.
    #[cfg(feature = "calibration")]
    #[test]
    fn gap_listen_with_dump_dir_and_no_output_flags_is_accepted() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--gap-fingerprints",
            "gap-files/out",
            "--gap-listen",
            "--only-gaps",
            "3",
        ]);
        validate_fingerprint_flags(&args).expect("canonical listen invocation must be accepted");
    }

    #[test]
    fn no_dual_fit_cli_overrides_config() {
        use clap::Parser;

        let args = Args::parse_from(["clip-sync-repair", "a.wav", "b.wav", "--no-dual-fit"]);
        let mut config = RepairAppConfig::default();
        assert!(config.repair.dual_fit);
        apply_cli_overrides(&mut config, &args);
        assert!(!config.repair.dual_fit);
    }

    #[test]
    fn only_gaps_cli_clears_skip_gaps_from_toml() {
        use clap::Parser;

        let args = Args::parse_from(["clip-sync-repair", "a.wav", "b.wav", "--only-gaps", "1,3"]);
        let mut config = RepairAppConfig::default();
        config.repair.skip_gaps = Some(vec!["2".into()]);
        apply_cli_overrides(&mut config, &args);
        assert_eq!(config.repair.only_gaps, Some(vec!["1".into(), "3".into()]));
        assert!(config.repair.skip_gaps.is_none());
        assert_eq!(
            config.repair.gap_selection_mode(),
            crate::domain::GapSelectionMode::Only(vec!["1".into(), "3".into()])
        );
    }

    #[test]
    fn skip_gaps_cli_clears_only_gaps_from_toml() {
        use clap::Parser;

        let args = Args::parse_from(["clip-sync-repair", "a.wav", "b.wav", "--skip-gaps", "2"]);
        let mut config = RepairAppConfig::default();
        config.repair.only_gaps = Some(vec!["1".into()]);
        apply_cli_overrides(&mut config, &args);
        assert_eq!(config.repair.skip_gaps, Some(vec!["2".into()]));
        assert!(config.repair.only_gaps.is_none());
    }
}
