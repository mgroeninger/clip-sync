# Test tier selector for clip-sync workspace.
# Run from repo root: .\scripts\test-tier.ps1 -Tier pr
# See docs/dev/test-tiers.md and docs/dev/development.md.
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet(
        'unit', 'integration', 'oracle', 'validation', 'diagnostic',
        'pr', 'pr-repair', 'pr-repair-extended', 'pr-align',
        'validation-align', 'diagnostic-align'
    )]
    [string] $Tier,

    [ValidateSet('clip-sync-repair', 'clip-sync', 'clip-sync-cli', 'workspace')]
    [string] $Package = 'workspace',

    [switch] $Nocapture
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path $PSScriptRoot -Parent
Push-Location $RepoRoot
try {
    function Test-FfmpegOnPath {
        return $null -ne (Get-Command ffmpeg -ErrorAction SilentlyContinue)
    }

    function Invoke-CargoTest {
        param(
            [Parameter(Mandatory = $true)]
            [string[]] $CargoArgs
        )

        $testArgs = @('test') + $CargoArgs
        if ($Nocapture) {
            if ($testArgs -contains '--') {
                $testArgs += '--nocapture'
            } else {
                $testArgs += '--', '--nocapture'
            }
        }

        Write-Host ">> cargo $($testArgs -join ' ')" -ForegroundColor Cyan
        & cargo @testArgs
        if ($LASTEXITCODE -ne 0) {
            exit $LASTEXITCODE
        }
    }

    function Invoke-RepairLibUnits {
        Invoke-CargoTest @('-p', 'clip-sync-repair', '--lib')
        Invoke-CargoTest @('-p', 'clip-sync-repair-fixtures', '--lib')
    }

    function Invoke-RepairPrRepair {
        Invoke-RepairLibUnits
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--test', 'config_roundtrip',
            '--test', 'scan_gaps_integration',
            '--test', 'cli_wav_integration',
            '--test', 'query_reference_integration',
            '--test', 'integration_residual_gate_smoke',
            '--test', 'integration_floor_oracle_smoke',
            '--test', 'integration_gap_corpus',
            '--test', 'integration_energy_smoke',
            '--test', 'oracle_energy',
            '--test', 'seam_residual_corpus',
            '--test', 'wav_bit_depth_integration',
            '--test', 'gap_cell_fixtures',
            '--test', 'golden_baseline_invariance',
            '--test', 'gap_repair_spec_diff',
            '--test', 'curated_fixture_backfill',
            '--test', 'decode_path_projection',
            '--test', 'equivalence_divergence',
            '--test', 'w5_timing_offset'
        )

        if (Test-FfmpegOnPath) {
            Invoke-CargoTest @(
                '-p', 'clip-sync-repair',
                '--features', 'ffmpeg-mux',
                '--test', 'cli_mux_integration'
            )
        } else {
            Write-Host '>> skip cli_mux_integration (ffmpeg not on PATH)' -ForegroundColor DarkYellow
        }

        # Needs its own invocation: `--gap-fingerprints` / `--gap-listen` only exist under
        # `calibration`, and these files are `#![cfg(feature = "calibration")]`, so folding them into
        # the batch above would build empty binaries and pass vacuously. ~130 s in debug (47 + 48 +
        # 35) — kept in debug so the tier stays on one profile rather than paying for a whole
        # optimized build. `gap_listen_integration`'s slow gate-refusal row is `#[ignore]`d (it pays
        # the fingerprint anchor oracle, not the refusal) and runs in the diagnostic tier instead.
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--features', 'calibration',
            '--test', 'cli_gap_fingerprint_provenance',
            '--test', 'gap_listen_integration',
            '--test', 'cli_gap_listen'
        )

        Invoke-CargoTest @('-p', 'clip-sync-repair-harness', '--lib')
    }

    function Invoke-RepairPrRepairExtended {
        Invoke-RepairPrRepair
        Invoke-CargoTest @('-p', 'clip-sync-repair', '--test', 'patch_audio_integration')
    }

    function Invoke-RepairIntegrationOnly {
        $cargoArgs = @(
            '-p', 'clip-sync-repair',
            '--test', 'config_roundtrip',
            '--test', 'scan_gaps_integration',
            '--test', 'patch_audio_integration',
            '--test', 'query_reference_integration',
            '--test', 'cli_wav_integration',
            '--test', 'integration_energy_smoke',
            '--test', 'integration_energy_patch',
            '--test', 'integration_floor_oracle_smoke',
            '--test', 'integration_gap_corpus',
            '--test', 'integration_residual_gate_smoke',
            '--test', 'anchor_seam_oracle',
            '--test', 'oracle_energy',
            '--test', 'seam_residual_corpus',
            '--test', 'wav_bit_depth_integration'
        )
        if (Test-FfmpegOnPath) {
            $cargoArgs += '--features', 'ffmpeg-mux', '--test', 'cli_mux_integration'
        } else {
            Write-Host '>> omit cli_mux_integration (ffmpeg not on PATH)' -ForegroundColor DarkYellow
        }
        Invoke-CargoTest $cargoArgs
    }

    function Invoke-RepairOracle {
        Invoke-CargoTest @('-p', 'clip-sync-repair', '--test', 'oracle_energy')
        Invoke-CargoTest @('-p', 'clip-sync-repair', '--test', 'oracle_energy', '--', '--ignored')
    }

    function Invoke-RepairValidation {
        if (-not (Test-FfmpegOnPath)) {
            Write-Warning 'validation tier: ffmpeg recommended on PATH for floor_oracle / codec rows'
        }
        # golden_baseline_invariance + gap_repair_spec_diff moved to pr-repair (media-free curated
        # fixtures) in Phase 3 of the gap-fixture-corpus plan.
        # calibrate_anchor_prominence is listed so the binary builds under this feature, but its
        # only row is #[ignore] (settled CSV probe — re-run with --ignored --nocapture on demand).
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--features', 'validation-tests',
            '--test', 'validate_floor_oracle',
            '--test', 'validate_residual_gate',
            '--test', 'validate_patch_audio',
            '--test', 'validate_dual_fit_oracle',
            '--test', 'calibrate_anchor_prominence'
        )
        # Patch-timing rows: release-calibrated budgets; debug is ~10–20× slower.
        foreach ($gapFilter in @('gap_corpus_generated', 'gap_corpus_external', 'gap_corpus_patch_timing')) {
            Invoke-CargoTest @(
                '-p', 'clip-sync-repair',
                '--release',
                '--test', 'integration_gap_corpus',
                $gapFilter,
                '--', '--ignored'
            )
        }
        if (Test-FfmpegOnPath) {
            Invoke-CargoTest @(
                '-p', 'clip-sync-repair',
                '--features', 'ffmpeg-mux',
                '--test', 'cli_mux_integration',
                '--', '--ignored'
            )
        } else {
            Write-Host '>> skip cli_mux_integration ignored e2e (ffmpeg not on PATH)' -ForegroundColor DarkYellow
        }
    }

    function Invoke-RepairDiagnostic {
        # Feature-gated diagnostic binaries (CSV / sweeps; no #[ignore]).
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--features', 'diagnostic-tests',
            '--test', 'diag_energy_matrix',
            '--test', 'diag_seam_residual',
            '--test', 'diag_patch_audio',
            '--test', 'diag_anchor_seam',
            '--test', 'diag_w5_anchor_rescue',
            '--test', 'diag_w5_timing_offset',
            '--test', 'diag_anchor_quiet_gap',
            '--test', 'seam_residual_oracle'
        )
        # W5 timing-offset gate probe (slow: full unified gate per cell; release-only by preference).
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--features', 'diagnostic-tests',
            '--test', 'diag_w5_timing_offset',
            'diag_w5_timing_offset_gate_probe',
            '--', '--ignored'
        )
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--features', 'diagnostic-tests',
            '--test', 'seam_residual_oracle',
            'broadband_oracle_veto_rescue_patches_marginal',
            '--', '--ignored'
        )
        # A6 anchor-rescue pipeline (slow: full PatchAudio anchor rescue; release-only, times out in
        # debug). Asserting #[ignore] rows on the noise-collar W5 fixture — plan §8 Q1.
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--release',
            '--test', 'anchor_seam_oracle',
            'w5_anchor_rescue_pipeline',
            '--', '--ignored'
        )
        # `--gap-listen` against a gate-refused gap (slow: ~350 s, of which ~99.8% is the Full-tier
        # fingerprint per-bracket anchor oracle, not the refusal the test asserts — see
        # docs/dev/TEMP-gap-listen-wav-plan.md §12.2). Release-only for the same reason as above.
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--release',
            '--features', 'calibration',
            '--test', 'gap_listen_integration',
            'a_gate_refused_gap',
            '--', '--ignored'
        )
        # Lib stragglers (golden generator; ffmpeg mux unit when feature enabled).
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            'write_full_surface_repair_golden',
            '--', '--ignored'
        )
        if (Test-FfmpegOnPath) {
            Invoke-CargoTest @(
                '-p', 'clip-sync-repair',
                '--features', 'ffmpeg-mux',
                'mux_reports_progress_for_short_fixture',
                '--', '--ignored'
            )
            # Soft-skips without SPLICE_EXP_* env vars; ffmpeg required when those are set.
            Invoke-CargoTest @(
                '-p', 'clip-sync-repair',
                '--features', 'diagnostic-tests',
                '--test', 'diag_splice_timescale'
            )
        } else {
            Write-Host '>> skip mux_reports_progress_for_short_fixture (ffmpeg not on PATH)' -ForegroundColor DarkYellow
            Write-Host '>> skip diag_splice_timescale (ffmpeg not on PATH)' -ForegroundColor DarkYellow
        }
    }

    function Invoke-PrAlign {
        Invoke-CargoTest @('-p', 'clip-sync', 'corpus_committed')
    }

    function Invoke-ClipSyncUnit {
        Invoke-CargoTest @('-p', 'clip-sync', '--lib')
    }

    function Invoke-ClipSyncValidation {
        if (-not (Test-FfmpegOnPath)) {
            Write-Warning 'validation tier: ffmpeg recommended for generated corpus cases'
        }
        foreach ($corpusFilter in @(
            'corpus_generated', 'corpus_external', 'corpus_source', 'corpus_mkv_tail',
            'corpus_query_reference_45min', 'corpus_query_reference_b_longer_anchor'
        )) {
            Invoke-CargoTest @(
                '-p', 'clip-sync', '--features', 'he-aac,test-utils',
                $corpusFilter, '--', '--ignored'
            )
        }
    }

    function Invoke-ClipSyncDiagnostic {
        Invoke-CargoTest @(
            '-p', 'clip-sync',
            'regenerate_committed_wav_fixtures', 'pcm_discover', 'refine_recovers', 'locate_query', '--', '--ignored'
        )
    }

    function Invoke-Phase2bStub {
        param([string] $TierName)
        Write-Error "$TierName is not implemented until Phase 2b (clip-sync tests/ binaries). See docs/dev/test-tier-remainder.md."
        exit 1
    }

    switch ($Tier) {
        'validation-align' { Invoke-Phase2bStub 'validation-align' }
        'diagnostic-align' { Invoke-Phase2bStub 'diagnostic-align' }

        'pr-align' {
            if ($Package -ne 'workspace' -and $Package -ne 'clip-sync') {
                Write-Error "pr-align applies to clip-sync only (got Package=$Package)"
                exit 1
            }
            Invoke-PrAlign
        }

        'pr-repair' {
            if ($Package -ne 'workspace' -and $Package -ne 'clip-sync-repair') {
                Write-Error "pr-repair applies to clip-sync-repair only (got Package=$Package)"
                exit 1
            }
            Invoke-RepairPrRepair
        }

        'pr-repair-extended' {
            if ($Package -ne 'workspace' -and $Package -ne 'clip-sync-repair') {
                Write-Error "pr-repair-extended applies to clip-sync-repair only (got Package=$Package)"
                exit 1
            }
            Invoke-RepairPrRepairExtended
        }

        'pr' {
            if ($Package -ne 'workspace') {
                Write-Error 'pr tier requires Package=workspace'
                exit 1
            }
            Invoke-PrAlign
            Invoke-RepairPrRepair
            Invoke-CargoTest @('-p', 'clip-sync-cli')
        }

        'unit' {
            switch ($Package) {
                'workspace' {
                    Invoke-RepairLibUnits
                    Invoke-ClipSyncUnit
                }
                'clip-sync-repair' { Invoke-RepairLibUnits }
                'clip-sync' { Invoke-ClipSyncUnit }
                'clip-sync-cli' {
                    Write-Error 'clip-sync-cli has no lib unit tests; use -Tier pr or integration'
                    exit 1
                }
            }
        }

        'integration' {
            switch ($Package) {
                'workspace' { Invoke-RepairIntegrationOnly }
                'clip-sync-repair' { Invoke-RepairIntegrationOnly }
                'clip-sync' {
                    Write-Error 'clip-sync integration binaries land in Phase 2b; use -Tier pr-align for corpus_committed'
                    exit 1
                }
                'clip-sync-cli' { Invoke-CargoTest @('-p', 'clip-sync-cli') }
            }
        }

        'oracle' {
            switch ($Package) {
                'workspace' { Invoke-RepairOracle }
                'clip-sync-repair' { Invoke-RepairOracle }
                'clip-sync' {
                    Write-Error 'clip-sync oracle binaries land in Phase 2b'
                    exit 1
                }
                'clip-sync-cli' {
                    Write-Error 'clip-sync-cli has no oracle-label tests'
                    exit 1
                }
            }
        }

        'validation' {
            switch ($Package) {
                'workspace' {
                    Invoke-RepairValidation
                    Invoke-ClipSyncValidation
                }
                'clip-sync-repair' { Invoke-RepairValidation }
                'clip-sync' { Invoke-ClipSyncValidation }
                'clip-sync-cli' {
                    Write-Error 'clip-sync-cli has no validation tier'
                    exit 1
                }
            }
        }

        'diagnostic' {
            switch ($Package) {
                'workspace' {
                    Invoke-RepairDiagnostic
                    Invoke-ClipSyncDiagnostic
                }
                'clip-sync-repair' { Invoke-RepairDiagnostic }
                'clip-sync' { Invoke-ClipSyncDiagnostic }
                'clip-sync-cli' {
                    Write-Error 'clip-sync-cli has no diagnostic tier'
                    exit 1
                }
            }
        }
    }
} finally {
    Pop-Location
}
