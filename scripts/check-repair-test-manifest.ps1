# Verify clip-sync-repair integration test binaries match Cargo.toml [[test]] entries.
# With autotests = false, a new tests/*.rs without [[test]] is silently ignored by cargo test.
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

$failed = $false
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

Write-Host "clip-sync-repair test manifest OK ($($declaredNames.Count) [[test]] entries, $($diskFiles.Count) tests/*.rs)" -ForegroundColor Green
