# Test tier selector for clip-sync workspace.
# Run from repo root: .\scripts\test-tier.ps1 -Tier pr
# See docs/TEMP-test-tier-plan.md and docs/development.md.
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
            '--test', 'wav_bit_depth_integration'
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
    }

    function Invoke-RepairPrRepairExtended {
        Invoke-RepairPrRepair
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--test', 'patch_audio_integration', '--',
            '--skip', 'i1_', '--skip', 'i2_', '--skip', 'i3_'
        )
    }

    function Invoke-RepairIntegrationOnly {
        $args = @(
            '-p', 'clip-sync-repair',
            '--test', 'config_roundtrip',
            '--test', 'scan_gaps_integration',
            '--test', 'patch_audio_integration',
            '--test', 'query_reference_integration',
            '--test', 'cli_wav_integration',
            '--test', 'integration_energy_smoke',
            '--test', 'integration_floor_oracle_smoke',
            '--test', 'integration_gap_corpus',
            '--test', 'integration_residual_gate_smoke',
            '--test', 'oracle_energy',
            '--test', 'seam_residual_corpus',
            '--test', 'wav_bit_depth_integration'
        )
        if (Test-FfmpegOnPath) {
            $args += '--features', 'ffmpeg-mux', '--test', 'cli_mux_integration'
        } else {
            Write-Host '>> omit cli_mux_integration (ffmpeg not on PATH)' -ForegroundColor DarkYellow
        }
        Invoke-CargoTest $args
    }

    function Invoke-RepairOracle {
        Invoke-CargoTest @('-p', 'clip-sync-repair', '--test', 'oracle_energy')
        Invoke-CargoTest @('-p', 'clip-sync-repair', '--test', 'seam_residual_corpus', 'seam_residual_disagreement_oracles')
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair', '--test', 'oracle_energy',
            'p1_f1_production_energy_unified_finds_true_offset',
            'p2_f2_production_energy_unified_finds_pause_one',
            'f1_production_oracle_patch_control', 'f2_production_oracle_patch_smoke',
            'f1_production_haystack_scan_vs_oracle', 'p4_f4_decoy_unified_search_diverges',
            'f4_decoy_placement_informative_with_high_headroom', '--', '--ignored'
        )
    }

    function Invoke-RepairValidation {
        if (-not (Test-FfmpegOnPath)) {
            Write-Warning 'validation tier: ffmpeg recommended on PATH for floor_oracle / codec rows'
        }
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--features', 'validation-tests',
            '--test', 'validate_floor_oracle',
            '--test', 'validate_residual_gate'
        )
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--test', 'integration_gap_corpus',
            'gap_corpus_generated', 'gap_corpus_external', 'gap_corpus_patch_timing',
            '--', '--ignored'
        )
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--test', 'patch_audio_integration',
            'patch_audio_fit_production_defaults', '--', '--ignored'
        )
    }

    function Invoke-RepairDiagnostic {
        # Feature-gated diagnostic binaries (CSV / sweeps; no #[ignore]).
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--features', 'diagnostic-tests',
            '--test', 'diag_energy_matrix',
            '--test', 'diag_seam_residual',
            '--test', 'seam_residual_oracle'
        )
        # Straggler diagnostic #[ignore] rows still in --lib (golden generator, ffmpeg unit).
        Invoke-CargoTest @('-p', 'clip-sync-repair', '--lib', '--', '--ignored')
        Invoke-CargoTest @(
            '-p', 'clip-sync-repair',
            '--test', 'patch_audio_integration',
            '--', '--ignored',
            '--skip', 'patch_audio_fit_production_defaults'
        )
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
        Invoke-CargoTest @(
            '-p', 'clip-sync', '--features', 'he-aac,test-utils',
            'corpus_generated', 'corpus_external', 'corpus_source', 'corpus_mkv_tail',
            'corpus_query_reference_45min', 'corpus_query_reference_b_longer_anchor', '--', '--ignored'
        )
    }

    function Invoke-ClipSyncDiagnostic {
        Invoke-CargoTest @(
            '-p', 'clip-sync',
            'regenerate_committed_wav_fixtures', 'pcm_discover', 'refine_recovers', 'locate_query', '--', '--ignored'
        )
    }

    function Invoke-Phase2bStub {
        param([string] $TierName)
        Write-Error "$TierName is not implemented until Phase 2b (clip-sync tests/ binaries). See docs/TEMP-test-tier-plan.md."
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
