# Download optional third-party corpus sources into tests/corpus/_sources/.
# Verifies SHA-256 against tests/corpus/sources.toml.

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

$sourcesToml = Join-Path "tests" "corpus" "sources.toml"
$sourcesDir = Join-Path "tests" "corpus" "_sources"

if (-not (Test-Path $sourcesToml)) {
    throw "missing $sourcesToml"
}

New-Item -ItemType Directory -Force -Path $sourcesDir | Out-Null

function Get-SourceEntries {
    $text = Get-Content $sourcesToml -Raw
    $entries = @()
    $current = @{}
    foreach ($line in ($text -split "`n")) {
        $trimmed = $line.Trim()
        if ($trimmed -eq "[[source]]") {
            if ($current.Count -gt 0) { $entries += [pscustomobject]$current }
            $current = @{}
            continue
        }
        if ($trimmed -match '^(\w+)\s*=\s*"(.*)"\s*$') {
            $current[$Matches[1]] = $Matches[2]
        }
    }
    if ($current.Count -gt 0) { $entries += [pscustomobject]$current }
    return $entries
}

$failed = $false
foreach ($source in Get-SourceEntries) {
    $dest = Join-Path $sourcesDir $source.filename
    if (Test-Path $dest) {
        $hash = (Get-FileHash $dest -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($hash -eq $source.sha256.ToLowerInvariant()) {
            Write-Host "OK (cached): $($source.id)"
            continue
        }
        Write-Host "Re-downloading $($source.id): hash mismatch"
    } else {
        Write-Host "Downloading $($source.id) ..."
    }

    Invoke-WebRequest -Uri $source.url -OutFile $dest
    $hash = (Get-FileHash $dest -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $source.sha256.ToLowerInvariant()) {
        Write-Error "SHA-256 mismatch for $($source.id): got $hash, expected $($source.sha256)"
        $failed = $true
    } else {
        Write-Host "Verified: $($source.id) ($($source.filename))"
    }
}

if ($failed) {
    throw "One or more sources failed verification"
}

Write-Host "Done."
Write-Host "  cargo test -p clip-sync corpus_source_cases -- --ignored"
Write-Host "  cargo test -p clip-sync-repair source_gap_oracle_floor_csv -- --ignored --nocapture"
