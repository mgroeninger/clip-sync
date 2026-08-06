#!/usr/bin/env pwsh
# Write-mode repair over a pair manifest, capturing `fill_level` (fill-level check) as parseable JSON.
#
# Sibling of `scan-registration.ps1` (same manifest format, same stdout/stderr split) but runs the
# **full patch path**: `--patch-only` so splice + `measure_fill_level` execute with no audio file
# written, `--format json` so each pair lands as `<label>.json` with `patch.gaps[*].fill_level`.
# Preview / scan-only cannot produce this field — `--repair-preview` returns before execute.
# See docs/pipeline.md § Fill-level check / docs/json-output.md § FillLevelCheck.
#
# Usage:
#   ./scripts/measure-fill-level.ps1 -Manifest pairs.csv
#   ./scripts/measure-fill-level.ps1 -Manifest pairs.csv -OutDir gap-files/2026-08-04-fill-level
#   ./scripts/measure-fill-level.ps1 -Manifest pairs.csv -RepairArgs "--min-gap-ms 500" -SkipBuild -Force
#   ./scripts/measure-fill-level.ps1 -Manifest pairs.csv -RollupOnly   # re-derive CSV from existing JSON
#
# Manifest format (CSV or TSV; '#' comments and blank lines ignored): one pair per line, no header
#     label , path/to/A.mkv , path/to/B.m4v [, extra per-pair repair args]
# Identical to `scan-registration.ps1` / `measure-gap-fingerprints.ps1` / `repair-directory-pairs.ps1`.
#
# STDOUT vs STDERR: repair JSON → stdout → `<label>.json`; progress → stderr → `<label>.log`.
# Do not Tee both streams into one file — that interleaves progress into the document.
#
# NO THROWAWAY WAV. This used to force write mode with a `--wav` nobody wanted, which cost the
# 2026-08-05 run 8 of 39 pairs: their patched PCM exceeded the 4 GiB classic WAV limit, the write
# error took the whole run down (exit 4), and the fill-level numbers — already computed minutes
# earlier, before the splice — died with the report. `--patch-only` runs the same patch and skips
# the sink. Do not reintroduce `--wav` here; `-WavDir` / `-KeepWav` are gone with it.
#
# PRECHECK: the first pair with patched_count > 0 must carry at least one `fill_level`. A binary
# built before the fill-level check, or `--no-measure-fill-level`, produces healthy JSON with the field silently
# absent — stop before the rest of the manifest pays full repair cost. Bypass with -SkipPrecheck.
#
# MEDIA HYGIENE: reports and the manifest carry absolute media paths. -OutDir defaults under
# `gap-files/` (gitignored). Prefer numeric labels in docs — never paths or titles.
# See docs/dev/repair-perf.md §"Media handling".

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Manifest,

    # Per-pair repair JSON: `<label>.json`. Also receives `fill-level-rollup.csv` at the end.
    [string]$OutDir = (Join-Path (Split-Path -Parent $PSScriptRoot) 'gap-files/fill-level'),

    # Per-pair stderr (progress + warnings): `<label>.log`.
    [string]$LogDir = (Join-Path $env:TEMP 'clip-sync-fill-level'),

    # Shared repair args after `A B --patch-only --format json`. Per-pair `extra` follows these.
    [string]$RepairArgs = '',

    [ValidateSet('auto', 'comma', 'tab')]
    [string]$Delimiter = 'auto',

    # Overwrite existing `<label>.json`. Without this, parseable reports are skipped (resume).
    [switch]$Force,

    # Skip the first-pair fill_level precheck.
    [switch]$SkipPrecheck,

    # Only rebuild `fill-level-rollup.csv` from existing `<label>.json` files (no cargo, no repair).
    [switch]$RollupOnly,

    [switch]$SkipBuild,

    [string]$CargoProfile = 'release',

    # Codec features must match the media. `he-aac` registers the only AAC decoder (incl. AAC-LC).
    [string]$Features = 'he-aac,ac3'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

if (-not (Test-Path $Manifest)) { throw "manifest not found: $Manifest" }

$forbidden = @(
    '--gap-fingerprints', '--gap-listen', '--wav', '--mux', '--repair-preview',
    '--patch-only', '--format', '--no-measure-fill-level'
)
foreach ($f in $forbidden) {
    if ($RepairArgs -match [regex]::Escape($f)) {
        throw "-RepairArgs must not contain '$f'. This script supplies --patch-only and " +
        "--format json, and requires measure_fill_level on. For scan-only registration use " +
        "scan-registration.ps1; for dumps use measure-gap-fingerprints.ps1; for kept repairs use " +
        "repair-directory-pairs.ps1."
    }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

$outFull = (Resolve-Path $OutDir).Path.TrimEnd('\', '/')
$repoFull = (Resolve-Path $RepoRoot).Path.TrimEnd('\', '/')
$ignoredRoot = (Join-Path $repoFull 'gap-files').TrimEnd('\', '/')
$cmp = [StringComparison]::OrdinalIgnoreCase
if ($outFull.StartsWith($repoFull, $cmp) -and
    -not ($outFull -eq $ignoredRoot -or $outFull.StartsWith($ignoredRoot + [IO.Path]::DirectorySeparatorChar, $cmp))) {
    Write-Warning "-OutDir is inside the repo but outside gap-files/ — reports carry absolute media paths and are NOT gitignored there: $outFull"
}

function Split-ManifestRow {
    param([string]$Line, [string]$Delim)
    $fields = [System.Collections.Generic.List[string]]::new()
    $cur = [System.Text.StringBuilder]::new()
    $inQuotes = $false
    foreach ($ch in $Line.ToCharArray()) {
        if ($ch -eq '"') { $inQuotes = -not $inQuotes; continue }
        if ($ch -eq $Delim -and -not $inQuotes) { $fields.Add($cur.ToString()); $cur.Clear() | Out-Null; continue }
        $cur.Append($ch) | Out-Null
    }
    $fields.Add($cur.ToString())
    [pscustomobject]@{
        label = if ($fields.Count -gt 0) { $fields[0] } else { '' }
        a     = if ($fields.Count -gt 1) { $fields[1] } else { '' }
        b     = if ($fields.Count -gt 2) { $fields[2] } else { '' }
        extra = if ($fields.Count -gt 3) { ($fields[3..($fields.Count - 1)]) -join $Delim } else { '' }
    }
}

function Test-StatusIsPatched {
    param($Status)
    if ($null -eq $Status) { return $false }
    # Externally tagged: { "patched": { ... } } or string forms in older dumps.
    if ($Status -is [string]) { return $Status -match '(?i)patched' }
    return $null -ne $Status.PSObject.Properties['patched']
}

function Get-DualFitUsed {
    param($Outcome)
    $tags = $Outcome.tags
    if ($null -eq $tags) { return $null }
    if ($null -ne $tags.PSObject.Properties['dual_fit_used']) { return [bool]$tags.dual_fit_used }
    $status = $Outcome.status
    if ($null -ne $status -and $null -ne $status.patched -and
        $null -ne $status.patched.PSObject.Properties['dual_fit_used']) {
        return [bool]$status.patched.dual_fit_used
    }
    return $null
}

# One row per spliced gap that carries fill_level. Empty list if the report has no patch block.
function Get-FillLevelRows {
    param(
        [string]$JsonPath,
        [string]$Label
    )
    $doc = Get-Content -LiteralPath $JsonPath -Raw | ConvertFrom-Json
    if ($null -eq $doc.patch -or $null -eq $doc.patch.gaps) {
        return ,@()
    }
    $rows = [System.Collections.Generic.List[object]]::new()
    $i = 0
    foreach ($g in @($doc.patch.gaps)) {
        $fl = $g.fill_level
        if ($null -ne $fl) {
            $rows.Add([pscustomobject]@{
                    Label          = $Label
                    Gap1Based      = $i + 1
                    Gap0Based      = $i
                    AStartSecs     = $g.a_start_secs
                    AEndSecs       = $g.a_end_secs
                    PeakDeltaDb    = [double]$fl.peak_delta_db
                    PeakBinDb      = [double]$fl.peak_bin_db
                    ReferenceDb    = [double]$fl.reference_db
                    PreShoulderDb  = $(if ($null -ne $fl.pre_shoulder_db) { [double]$fl.pre_shoulder_db } else { $null })
                    PostShoulderDb = $(if ($null -ne $fl.post_shoulder_db) { [double]$fl.post_shoulder_db } else { $null })
                    PeakBinIndex   = [int]$fl.peak_bin_index
                    Bins           = [int]$fl.bins
                    BinMs          = [double]$fl.bin_ms
                    # True when every measurable shoulder was digital silence — usually a
                    # neighbouring dropout inside the shoulder window. The delta is then the fill's
                    # absolute level plus 120 dB and says nothing about substitution magnitude, so
                    # these rows are excluded from the listen candidates below (kept in the CSV).
                    RefAtFloor     = [bool]$fl.reference_at_floor
                    DualFitUsed    = Get-DualFitUsed -Outcome $g
                    Report         = $JsonPath
                })
        }
        $i++
    }
    return , @($rows)
}

function Get-FillLevelSummary {
    param([string]$JsonPath)
    $doc = Get-Content -LiteralPath $JsonPath -Raw | ConvertFrom-Json
    $gaps = if ($null -ne $doc.scan.gaps) { @($doc.scan.gaps).Count } else { 0 }
    $patched = 0
    $withLevel = 0
    $maxDelta = $null
    if ($null -ne $doc.patch) {
        $patched = [int]$doc.patch.patched_count
        foreach ($g in @($doc.patch.gaps)) {
            if ($null -ne $g.fill_level) {
                $withLevel++
                $d = [double]$g.fill_level.peak_delta_db
                if ($null -eq $maxDelta -or $d -gt $maxDelta) { $maxDelta = $d }
            }
        }
    }
    [pscustomobject]@{
        Gaps      = $gaps
        Patched   = $patched
        WithLevel = $withLevel
        MaxDelta  = $maxDelta
        HasPatch  = ($null -ne $doc.patch)
    }
}

function Write-FillLevelRollup {
    param(
        [string]$Directory,
        [object[]]$PairLabels
    )
    $all = [System.Collections.Generic.List[object]]::new()
    foreach ($label in $PairLabels) {
        $json = Join-Path $Directory "$label.json"
        if (-not (Test-Path -LiteralPath $json)) { continue }
        try {
            foreach ($row in (Get-FillLevelRows -JsonPath $json -Label $label)) {
                $all.Add($row)
            }
        }
        catch {
            Write-Warning "[$label] rollup skipped (unreadable): $_"
        }
    }
    $sorted = @($all | Sort-Object -Property PeakDeltaDb -Descending)
    $csv = Join-Path $Directory 'fill-level-rollup.csv'
    $sorted | Select-Object Label, Gap1Based, Gap0Based, AStartSecs, AEndSecs, PeakDeltaDb,
    PeakBinDb, ReferenceDb, RefAtFloor, PreShoulderDb, PostShoulderDb, PeakBinIndex, Bins, BinMs,
    DualFitUsed, Report |
        Export-Csv -LiteralPath $csv -NoTypeInformation -Encoding utf8
    # Ranking keeps every row in the CSV but drops floored references from the listen candidates:
    # sorted by delta they would otherwise occupy the whole top of the table with an artifact.
    $ranked = @($sorted | Where-Object { -not $_.RefAtFloor })
    return [pscustomobject]@{
        Path    = $csv
        Count   = $sorted.Count
        Rows    = $sorted
        Ranked  = $ranked
        Floored = ($sorted.Count - $ranked.Count)
    }
}

# Top deltas, floored references held out (they sort to the top and mean nothing there).
function Write-FillLevelCandidates {
    param([object]$Rollup)
    if ($Rollup.Count -le 0) { return }
    Write-Host ''
    Write-Host 'Top peak_delta_db (listen candidates)' -ForegroundColor Cyan
    '{0,-12} {1,5} {2,10} {3,10} {4,10}' -f 'pair', 'gap#', 'delta_db', 'peak_db', 'ref_db'
    Write-Host ('-' * 52)
    foreach ($r in @($Rollup.Ranked | Select-Object -First 15)) {
        '{0,-12} {1,5} {2,10:N2} {3,10:N2} {4,10:N2}' -f $r.Label, $r.Gap1Based,
        $r.PeakDeltaDb, $r.PeakBinDb, $r.ReferenceDb
    }
    if ($Rollup.Floored -gt 0) {
        Write-Host ''
        Write-Host ("$($Rollup.Floored) row(s) excluded: reference_at_floor — every shoulder was " +
            'digital silence (neighbouring dropout), so the delta is an artifact. In the CSV as ' +
            'RefAtFloor=True.') -ForegroundColor DarkYellow
    }
}

$delimChar = switch ($Delimiter) {
    'tab' { "`t" }
    'comma' { ',' }
    default { if ([IO.Path]::GetExtension($Manifest) -eq '.tsv') { "`t" } else { ',' } }
}

$dataLines = @(Get-Content $Manifest |
    Where-Object { $_.Trim() -ne '' -and -not $_.Trim().StartsWith('#') })
$rows = @($dataLines | ForEach-Object { Split-ManifestRow -Line $_ -Delim $delimChar })

$labels = [System.Collections.Generic.List[string]]::new()
foreach ($row in $rows) {
    $label = ("$($row.label)").Trim()
    if ($label -ne '') { $labels.Add($label) }
}

if ($RollupOnly) {
    $rollup = Write-FillLevelRollup -Directory $OutDir -PairLabels @($labels)
    Write-Host "Rollup: $($rollup.Count) fill_level row(s) → $($rollup.Path)" -ForegroundColor Green
    Write-FillLevelCandidates -Rollup $rollup
    Write-Host ''
    Write-Host "Docs: docs/pipeline.md § Fill-level check; docs/json-output.md § FillLevelCheck" -ForegroundColor DarkGray
    return
}

$exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair" }

if (-not $SkipBuild) {
    Write-Host "Building clip-sync-repair ($CargoProfile, --features $Features)..." -ForegroundColor Cyan
    $profileFlag = if ($CargoProfile -eq 'release') { '--release' } else { "--profile=$CargoProfile" }
    Push-Location $RepoRoot
    try {
        & cargo build $profileFlag --features $Features -p clip-sync-repair --bin clip-sync-repair
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    }
    finally { Pop-Location }
    $exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair.exe"
    if (-not (Test-Path $exe)) { $exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair" }
}
if (-not (Test-Path $exe)) { throw "repair binary not found at target/$CargoProfile/ (build first or drop -SkipBuild)" }

$results = [System.Collections.Generic.List[object]]::new()
$prechecked = $SkipPrecheck.IsPresent

foreach ($row in $rows) {
    $label = ("$($row.label)").Trim()
    $aPath = ("$($row.a)").Trim()
    $bPath = ("$($row.b)").Trim()
    $extra = ("$($row.extra)").Trim()
    if ($label -eq '' -or $aPath -eq '' -or $bPath -eq '') {
        Write-Warning "manifest row skipped (need label,A,B): '$($row.label),$($row.a),$($row.b)'"
        continue
    }
    if ($label -match '[\\/:*?"<>|]') { throw "pair label '$label' contains path-invalid characters" }
    foreach ($p in @($aPath, $bPath)) {
        if (-not (Test-Path -LiteralPath $p)) { throw "pair '$label': media not found: $p" }
    }

    $json = Join-Path $OutDir "$label.json"
    $log = Join-Path $LogDir "$label.log"

    $summary = $null
    if ((Test-Path -LiteralPath $json) -and -not $Force) {
        try { $summary = Get-FillLevelSummary -JsonPath $json }
        catch { Write-Warning "[$label] existing report did not parse, re-running: $json" }
    }
    if ($null -ne $summary) {
        Write-Host "[$label] report exists, skipping (use -Force to redo): $json" -ForegroundColor DarkGray
        $results.Add([pscustomobject]@{
                Label     = $label
                ExitCode  = 0
                Seconds   = $null
                Gaps      = $summary.Gaps
                Patched   = $summary.Patched
                WithLevel = $summary.WithLevel
                MaxDelta  = $summary.MaxDelta
                Report    = $json
                Log       = $log
                Skipped   = $true
            })
        continue
    }

    # A B --patch-only --format json [shared] [per-pair extra]. The patch must actually run —
    # fill_level is measured during the splice — but nothing is written.
    $argList = [System.Collections.Generic.List[string]]::new()
    $argList.AddRange([string[]]@($aPath, $bPath, '--patch-only', '--format', 'json'))
    if ($RepairArgs -ne '') {
        foreach ($tok in ($RepairArgs -split '\s+' | Where-Object { $_ -ne '' })) { $argList.Add($tok) }
    }
    if ($extra -ne '') {
        foreach ($tok in ($extra -split '\s+' | Where-Object { $_ -ne '' })) { $argList.Add($tok) }
    }

    Write-Host "[$label] repairing → $json ..." -ForegroundColor Cyan
    $prevLog = $env:RUST_LOG
    if (-not $prevLog) { $env:RUST_LOG = 'warn,clip_sync_repair=info' }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $exe @($argList.ToArray()) 1> $json 2> $log
        $rc = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $prevEap
        if ($null -eq $prevLog) { Remove-Item Env:\RUST_LOG -ErrorAction SilentlyContinue }
        else { $env:RUST_LOG = $prevLog }
    }
    $sw.Stop()

    $summary = $null
    if ($rc -eq 0 -and (Test-Path -LiteralPath $json)) {
        try { $summary = Get-FillLevelSummary -JsonPath $json }
        catch { Write-Warning "[$label] report written but did not parse: $_" }
    }

    $maxNote = if ($null -eq $summary -or $null -eq $summary.MaxDelta) { '-' } else { '{0:N1}' -f $summary.MaxDelta }
    Write-Host ("[$label] exit {0} in {1:N1}s  gaps={2} patched={3} fill_level={4} max_delta={5}" -f
        $rc, $sw.Elapsed.TotalSeconds,
        ($summary?.Gaps ?? '?'), ($summary?.Patched ?? '?'), ($summary?.WithLevel ?? '?'), $maxNote)
    if ($rc -ne 0) {
        Remove-Item -LiteralPath $json -Force -ErrorAction SilentlyContinue
        Write-Warning "[$label] exited $rc (stderr kept at $log)"
    }

    # Fail on the first pair that patched something but recorded no fill_level — otherwise the
    # whole corpus pays full repair cost for empty calibration data.
    if (-not $prechecked -and $rc -eq 0 -and $null -ne $summary -and $summary.HasPatch -and $summary.Patched -gt 0) {
        if ($summary.WithLevel -eq 0) {
            throw @"
[$label] patched $($summary.Patched) gap(s) but recorded 0 fill_level measurement(s).
Stopping before the rest of the manifest repairs. Usual causes:
  * binary predates the fill-level check (`measure_fill_level`) — rebuild with current sources
  * measure_fill_level disabled in config / TOML (this script forbids --no-measure-fill-level in -RepairArgs)
  * every splice failed after the gate approved (not_applied) — check $log
Re-run with -SkipPrecheck to proceed anyway.
"@
        }
        Write-Host "[$label] precheck ok — fill_level on $($summary.WithLevel)/$($summary.Patched) patched gap(s)" -ForegroundColor Green
        $prechecked = $true
    }

    $results.Add([pscustomobject]@{
            Label     = $label
            ExitCode  = $rc
            Seconds   = [math]::Round($sw.Elapsed.TotalSeconds, 1)
            Gaps      = ${summary}?.Gaps
            Patched   = ${summary}?.Patched
            WithLevel = ${summary}?.WithLevel
            MaxDelta  = ${summary}?.MaxDelta
            Report    = $json
            Log       = $log
            Skipped   = $false
        })
}

Write-Host ''
Write-Host 'Summary' -ForegroundColor Green
'{0,-20} {1,6} {2,8} {3,6} {4,8} {5,8} {6,10}' -f 'pair', 'exit', 'secs', 'gaps', 'patched', 'levels', 'max_delta'
Write-Host ('-' * 72)
$tGaps = 0; $tPatched = 0; $tLevel = 0
foreach ($r in $results) {
    $maxNote = if ($null -eq $r.MaxDelta) { '-' } else { '{0:N1}' -f $r.MaxDelta }
    '{0,-20} {1,6} {2,8} {3,6} {4,8} {5,8} {6,10}' -f $r.Label, $r.ExitCode,
    ($null -eq $r.Seconds ? '-' : ('{0:N1}' -f $r.Seconds)),
    ($r.Gaps ?? '?'), ($r.Patched ?? '?'), ($r.WithLevel ?? '?'), $maxNote
    $tGaps += ($r.Gaps ?? 0); $tPatched += ($r.Patched ?? 0); $tLevel += ($r.WithLevel ?? 0)
}
Write-Host ('-' * 72)
'{0,-20} {1,6} {2,8} {3,6} {4,8} {5,8} {6,10}' -f 'TOTAL', '', '', $tGaps, $tPatched, $tLevel, ''

$doneLabels = @($results | Where-Object { $_.ExitCode -eq 0 } | ForEach-Object { $_.Label })
$rollup = Write-FillLevelRollup -Directory $OutDir -PairLabels $doneLabels

Write-Host ''
Write-Host "Reports: $OutDir" -ForegroundColor DarkGray
Write-Host "Logs:    $LogDir" -ForegroundColor DarkGray
Write-Host "Rollup:  $($rollup.Path) ($($rollup.Count) row(s), sorted by peak_delta_db desc)" -ForegroundColor DarkGray

Write-FillLevelCandidates -Rollup $rollup

Write-Host ''
Write-Host "Ear-check high deltas with --gap-listen / fingerprint-gap; threshold is still open." -ForegroundColor DarkGray
Write-Host "Docs: docs/pipeline.md § Fill-level check; docs/json-output.md § FillLevelCheck" -ForegroundColor DarkGray

$failed = @($results | Where-Object { $_.ExitCode -ne 0 })
if ($failed.Count -gt 0) {
    throw "$($failed.Count) pair(s) failed: $($failed.Label -join ', ')"
}
