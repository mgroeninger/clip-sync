# Verify clip-sync-repair integration test binaries match Cargo.toml [[test]] entries, and that
# every declared binary is actually run by some tier in test-tier.ps1.
#
# Two ways a test can exist and never run:
#   1. With autotests = false, a tests/*.rs without [[test]] is silently ignored by cargo test.
#   2. Every tier in test-tier.ps1 names its targets explicitly with `--test`, so a [[test]] entry
#      no tier lists is built by nobody. This is the same failure one level up, and it is the one
#      that actually bit: gap_listen_integration and cli_gap_listen were both declared and correct
#      and ran in zero tiers.
#
# Run from repo root: .\scripts\check-repair-test-manifest.ps1

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path $PSScriptRoot -Parent
$RepairRoot = Join-Path $RepoRoot 'crates\clip-sync-repair'
$CargoToml = Join-Path $RepairRoot 'Cargo.toml'
$TestsDir = Join-Path $RepairRoot 'tests'

if (-not (Test-Path $CargoToml)) {
    Write-Error "Missing $CargoToml"
}

$cargoText = Get-Content -Raw -Path $CargoToml
$declaredPaths = [System.Collections.Generic.HashSet[string]]::new(
    [StringComparer]::OrdinalIgnoreCase
)
$declaredNames = [System.Collections.Generic.HashSet[string]]::new(
    [StringComparer]::OrdinalIgnoreCase
)

foreach ($block in [regex]::Matches($cargoText, '(?ms)\[\[test\]\].*?(?=\n\[\[|\z)')) {
    $pathMatch = [regex]::Match($block.Value, 'path\s*=\s*"([^"]+)"')
    $nameMatch = [regex]::Match($block.Value, 'name\s*=\s*"([^"]+)"')
    if ($pathMatch.Success) {
        $rel = $pathMatch.Groups[1].Value -replace '/', '\'
        $full = Join-Path $RepairRoot $rel
        [void]$declaredPaths.Add($full)
    }
    if ($nameMatch.Success) {
        [void]$declaredNames.Add($nameMatch.Groups[1].Value)
    }
}

$diskFiles = Get-ChildItem -Path $TestsDir -Filter '*.rs' -File |
    ForEach-Object { $_.FullName }

$missingFromCargo = @()
foreach ($file in $diskFiles) {
    if (-not $declaredPaths.Contains($file)) {
        $missingFromCargo += $file.Substring($TestsDir.Length + 1)
    }
}

$missingOnDisk = @()
foreach ($path in $declaredPaths) {
    if (-not (Test-Path -LiteralPath $path)) {
        $missingOnDisk += $path.Substring($RepairRoot.Length + 1)
    }
}

# Targets no tier runs. This is a **ratchet**, not a blessing: kept empty once pre-existing holes
# are wired. Never add a name to silence this check — wire the target into a tier instead.
$untieredBacklog = [System.Collections.Generic.HashSet[string]]::new(
    [string[]] @(),
    [StringComparer]::OrdinalIgnoreCase
)

$TierScript = Join-Path $PSScriptRoot 'test-tier.ps1'
if (-not (Test-Path $TierScript)) {
    Write-Error "Missing $TierScript"
}
$tierText = Get-Content -Raw -Path $TierScript
$tieredNames = [System.Collections.Generic.HashSet[string]]::new(
    [StringComparer]::OrdinalIgnoreCase
)
foreach ($m in [regex]::Matches($tierText, "'--test'\s*,\s*'([^']+)'")) {
    [void]$tieredNames.Add($m.Groups[1].Value)
}

$untiered = @()
$staleBacklog = @()
foreach ($name in $declaredNames) {
    if ($tieredNames.Contains($name)) {
        if ($untieredBacklog.Contains($name)) {
            $staleBacklog += $name
        }
    } elseif (-not $untieredBacklog.Contains($name)) {
        $untiered += $name
    }
}

$failed = $false
if ($untiered.Count -gt 0) {
    $failed = $true
    Write-Host '[[test]] binaries no test-tier.ps1 tier runs (built by nobody):' -ForegroundColor Red
    $untiered | Sort-Object | ForEach-Object { Write-Host "  $_" }
    Write-Host '  -> add each to a tier in scripts/test-tier.ps1 (match the //! Tier: line).'
}

if ($staleBacklog.Count -gt 0) {
    $failed = $true
    Write-Host 'now wired into a tier but still listed in $untieredBacklog:' -ForegroundColor Red
    $staleBacklog | Sort-Object | ForEach-Object { Write-Host "  $_" }
    Write-Host '  -> delete these from the backlog list in this script.'
}

if ($missingFromCargo.Count -gt 0) {
    $failed = $true
    Write-Host 'tests/*.rs files without [[test]] in Cargo.toml (silently ignored by cargo):' -ForegroundColor Red
    $missingFromCargo | Sort-Object | ForEach-Object { Write-Host "  $_" }
}

if ($missingOnDisk.Count -gt 0) {
    $failed = $true
    Write-Host '[[test]] path entries missing on disk:' -ForegroundColor Red
    $missingOnDisk | Sort-Object | ForEach-Object { Write-Host "  $_" }
}

if ($failed) {
    Write-Host ''
    Write-Host "Declared [[test]] binaries: $($declaredNames.Count)" -ForegroundColor DarkGray
    exit 1
}

$tieredCount = $declaredNames.Count - $untieredBacklog.Count
Write-Host "clip-sync-repair test manifest OK ($($declaredNames.Count) [[test]] entries, $($diskFiles.Count) tests/*.rs, $tieredCount tiered)" -ForegroundColor Green
if ($untieredBacklog.Count -gt 0) {
    Write-Host "  $($untieredBacklog.Count) known-untiered target(s) on the backlog list — see this script's header" -ForegroundColor DarkYellow
}
