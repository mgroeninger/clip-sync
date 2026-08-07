#!/usr/bin/env pwsh
# Phase 1 census for docs/dev/archive/TEMP-residual-measured-unify-plan.md — dump-only, no gate change.
#
# Walks existing repair / fingerprint JSON and counts residual sides where
# `probe_non_finite` appears, especially the asymmetric cell (exactly one side) and whether
# a live headroom veto was at stake. Prefers dumps that already carry `uninformative_pre` /
# `_post` (post-2026-08-05); older dumps are skipped and tallied as excluded.
#
# Usage:
#   ./scripts/census-residual-measured.ps1 -DumpDir gap-files/2026-08-05-fill-level
#   ./scripts/census-residual-measured.ps1 -DumpDir gap-files/2026-08-05-fill-level -MarginDb 6
#   ./scripts/census-residual-measured.ps1 -DumpDir gap-files/fingerprint-corpus -CsvOut gap-files/census-residual-measured.csv
#
# Accepted layouts (same tree may mix them):
#   * Repair / patch-outcomes: `<DumpDir>/<pair>.json` with `patch.gaps[*].residual`
#     (from `measure-patch-outcomes.ps1` / `--patch-only --format json`)
#   * Fingerprint corpus: `<DumpDir>/<pair>/**/*.json` with `gaps[*].residual`
#     (from `measure-gap-fingerprints.ps1`; skips `corpus.json` / `manifest.json`)
#
# Buckets (plan §4):
#   A       — exactly one side `probe_non_finite`, other usable (absent)
#   A∩veto  — A and finite worst headroom > margin
#   B       — both sides `probe_non_finite`
#   C       — one `probe_non_finite` and the other `floor_above_ok_db`
#
# Headroom matches `SeamResidualVerdict::worst_headroom_db`: max of finite
# `(chosen_*_db - floor_*_db)` per side. Margin defaults to 6 dB; fingerprint
# `gate_recipe.residual_headroom_margin_db` / repair-adjacent recipe wins when present.
#
# MEDIA HYGIENE: emit pair label + gap index only — never media paths or titles.
# Docs: docs/dev/archive/TEMP-residual-measured-unify-plan.md §4.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$DumpDir,

    # Headroom margin (dB) when the dump does not echo a recipe value.
    [double]$MarginDb = 6.0,

    # Optional CSV of every ProbeNonFinite-bearing gap (pair, gap_index, bucket, …).
    [string]$CsvOut = '',

    # Also list every Bucket A / A∩veto row on the console (still pair/gap only).
    [switch]$ListA
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $DumpDir)) {
    throw "dump dir not found: $DumpDir"
}

function Get-Reason([object]$Value) {
    if ($null -eq $Value) { return $null }
    if ($Value -is [string]) {
        $s = $Value.Trim()
        return $(if ($s -eq '') { $null } else { $s })
    }
    # Enum-as-object / unexpected shape — stringify wire form.
    $s = [string]$Value
    if ($s -eq '' -or $s -eq 'System.Management.Automation.PSCustomObject') { return $null }
    return $s
}

function Get-WorstHeadroomDb([object]$Residual) {
    $heads = [System.Collections.Generic.List[double]]::new()
    foreach ($pair in @(
            @{ C = $Residual.chosen_pre_db; F = $Residual.floor_pre_db },
            @{ C = $Residual.chosen_post_db; F = $Residual.floor_post_db }
        )) {
        if ($null -eq $pair.C -or $null -eq $pair.F) { continue }
        $c = [double]$pair.C
        $f = [double]$pair.F
        if ([double]::IsNaN($c) -or [double]::IsInfinity($c)) { continue }
        if ([double]::IsNaN($f) -or [double]::IsInfinity($f)) { continue }
        $heads.Add($c - $f)
    }
    if ($heads.Count -eq 0) { return [double]::NaN }
    return ($heads | Measure-Object -Maximum).Maximum
}

function Resolve-MarginDb([object]$Root, [double]$Fallback) {
    foreach ($cand in @(
            $Root.gate_recipe.residual_headroom_margin_db,
            $Root.recipe.residual_headroom_margin_db,
            $Root.scan.residual_headroom_margin_db,
            $Root.patch.residual_headroom_margin_db
        )) {
        if ($null -ne $cand -and [double]::IsFinite([double]$cand)) {
            return [double]$cand
        }
    }
    return $Fallback
}

function Get-AChannels([object]$Root, [object]$Gap) {
    $tc = $Root.scan.track_compatibility
    if ($null -ne $tc -and $null -ne $tc.a_channels) { return [int]$tc.a_channels }
    if ($null -ne $Gap.channels) { return [int]$Gap.channels }
    if ($null -ne $Root.source -and $null -ne $Root.source.channels) {
        return [int]$Root.source.channels
    }
    return $null
}

function Get-DualFitUsed([object]$Gap) {
    if ($null -ne $Gap.tags -and $null -ne $Gap.tags.dual_fit_used) {
        return [bool]$Gap.tags.dual_fit_used
    }
    $patched = $Gap.status.patched
    if ($null -ne $patched -and $null -ne $patched.dual_fit_used) {
        return [bool]$patched.dual_fit_used
    }
    # Fingerprint dumps do not reliably flag dual-fit the way repair tags do.
    return $null
}

function Test-ResidualSchemaEligible([object]$Residual) {
    # Pre-2026-08-05 dumps omit uninformative_*; they also omit floor_source_*. Current emitters
    # always write floor_source when a residual exists, even when both sides are usable (so the
    # uninformative_* keys are skipped). Use floor_source as the schema gate.
    $names = @($Residual.PSObject.Properties.Name)
    return ($names -contains 'floor_source_pre') -or ($names -contains 'floor_source_post') -or
    ($names -contains 'uninformative_pre') -or ($names -contains 'uninformative_post')
}

function Resolve-ResidualBucket([object]$Residual) {
    $pre = Get-Reason $Residual.uninformative_pre
    $post = Get-Reason $Residual.uninformative_post
    if (-not (Test-ResidualSchemaEligible -Residual $Residual)) {
        return @{ Eligible = $false; Bucket = $null; Pre = $pre; Post = $post }
    }

    $prePnf = $pre -eq 'probe_non_finite'
    $postPnf = $post -eq 'probe_non_finite'
    $preFloor = $pre -eq 'floor_above_ok_db'
    $postFloor = $post -eq 'floor_above_ok_db'
    $preOk = $null -eq $pre
    $postOk = $null -eq $post

    $bucket = $null
    if (($prePnf -and $postOk) -or ($postPnf -and $preOk)) {
        $bucket = 'A'
    }
    elseif ($prePnf -and $postPnf) {
        $bucket = 'B'
    }
    elseif (($prePnf -and $postFloor) -or ($postPnf -and $preFloor)) {
        $bucket = 'C'
    }

    return @{
        Eligible = $true
        Bucket   = $bucket
        Pre      = $pre
        Post     = $post
    }
}

function Get-ResidualNodes([object]$Root) {
    $out = [System.Collections.Generic.List[object]]::new()
    if ($null -ne $Root.patch -and $null -ne $Root.patch.gaps) {
        $gi = 0
        foreach ($g in @($Root.patch.gaps)) {
            if ($null -ne $g.residual) {
                $out.Add([pscustomobject]@{ GapIndex = $gi; Gap = $g; Residual = $g.residual })
            }
            $gi++
        }
        return $out
    }
    if ($null -ne $Root.gaps) {
        foreach ($g in @($Root.gaps)) {
            if ($null -eq $g.residual) { continue }
            $idx = if ($null -ne $g.index) { [int]$g.index } else { $out.Count }
            $out.Add([pscustomobject]@{ GapIndex = $idx; Gap = $g; Residual = $g.residual })
        }
    }
    return $out
}

function Find-DumpFiles([string]$Root) {
    $files = [System.Collections.Generic.List[object]]::new()
    # Pair-level repair JSON directly under DumpDir.
    Get-ChildItem -LiteralPath $Root -File -Filter '*.json' -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -notmatch '^(corpus|manifest|fill-level-rollup|census)' } |
        ForEach-Object {
            $files.Add([pscustomobject]@{ Pair = $_.BaseName; Path = $_.FullName; Kind = 'repair' })
        }
    # Fingerprint layout: DumpDir/<pair>/**/*.json
    Get-ChildItem -LiteralPath $Root -Directory -ErrorAction SilentlyContinue | ForEach-Object {
        $pair = $_.Name
        Get-ChildItem -LiteralPath $_.FullName -Recurse -File -Filter '*.json' -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -notmatch '^(corpus|manifest)\.json$' } |
            ForEach-Object {
                $files.Add([pscustomobject]@{ Pair = $pair; Path = $_.FullName; Kind = 'fingerprint' })
            }
    }
    return $files
}

$dumpFull = (Resolve-Path -LiteralPath $DumpDir).Path
$files = @(Find-DumpFiles -Root $dumpFull)
if ($files.Count -eq 0) {
    throw "no JSON dumps under $dumpFull"
}

$rows = [System.Collections.Generic.List[object]]::new()
$filesSeen = 0
$filesExcluded = 0
$gapsEligible = 0
$gapsExcluded = 0
$countA = 0
$countAVeto = 0
$countB = 0
$countC = 0

# Split tallies: key = "ch=<n>|dual=<bool|?>"
$split = @{}

foreach ($f in $files) {
    $filesSeen++
    try {
        $root = Get-Content -LiteralPath $f.Path -Raw | ConvertFrom-Json
    }
    catch {
        Write-Warning "skip unparseable $($f.Path): $_"
        $filesExcluded++
        continue
    }

    $margin = Resolve-MarginDb -Root $root -Fallback $MarginDb
    $nodes = @(Get-ResidualNodes -Root $root)
    if ($nodes.Count -eq 0) {
        $filesExcluded++
        continue
    }

    $fileHadEligible = $false
    foreach ($n in $nodes) {
        $cls = Resolve-ResidualBucket -Residual $n.Residual
        if (-not $cls.Eligible) {
            $gapsExcluded++
            continue
        }
        $fileHadEligible = $true
        $gapsEligible++

        $worst = Get-WorstHeadroomDb -Residual $n.Residual
        $veto = [double]::IsFinite($worst) -and ($worst -gt $margin)
        $bucket = $cls.Bucket
        if ($bucket -eq 'A' -and $veto) {
            $bucket = 'A_veto'
            $countA++
            $countAVeto++
        }
        elseif ($bucket -eq 'A') {
            $countA++
        }
        elseif ($bucket -eq 'B') { $countB++ }
        elseif ($bucket -eq 'C') { $countC++ }

        $ch = Get-AChannels -Root $root -Gap $n.Gap
        $dual = Get-DualFitUsed -Gap $n.Gap
        $splitKey = 'ch={0}|dual={1}' -f $(if ($null -eq $ch) { '?' } else { $ch }),
        $(if ($null -eq $dual) { '?' } else { $dual })
        if (-not $split.ContainsKey($splitKey)) {
            $split[$splitKey] = [ordered]@{ A = 0; A_veto = 0; B = 0; C = 0; Other = 0 }
        }
        if ($null -eq $cls.Bucket) {
            $split[$splitKey].Other++
        }
        elseif ($bucket -eq 'A_veto') {
            $split[$splitKey].A++
            $split[$splitKey].A_veto++
        }
        else {
            $split[$splitKey][$cls.Bucket]++
        }

        if ($null -ne $cls.Bucket) {
            $informative = $n.Residual.informative
            $rows.Add([pscustomobject]@{
                    pair            = $f.Pair
                    gap_index       = $n.GapIndex
                    bucket          = $(if ($bucket -eq 'A_veto') { 'A_veto' } else { $cls.Bucket })
                    uninformative_pre  = $(if ($null -eq $cls.Pre) { '' } else { $cls.Pre })
                    uninformative_post = $(if ($null -eq $cls.Post) { '' } else { $cls.Post })
                    informative     = $informative
                    worst_headroom_db = $(if ([double]::IsFinite($worst)) { [math]::Round($worst, 3) } else { '' })
                    margin_db       = $margin
                    a_channels      = $(if ($null -eq $ch) { '' } else { $ch })
                    dual_fit_used   = $(if ($null -eq $dual) { '' } else { $dual })
                    residual_band   = $(if ($n.Gap.tags.residual_band) { $n.Gap.tags.residual_band } else { '' })
                    kind            = $f.Kind
                })
        }
    }
    if (-not $fileHadEligible) { $filesExcluded++ }
}

Write-Host ''
Write-Host 'Residual measuredness census (Phase 1)' -ForegroundColor Green
Write-Host "DumpDir:  $dumpFull"
Write-Host "Margin:   $MarginDb dB (fallback; per-dump recipe overrides when present)"
Write-Host ("Files:    {0} seen, {1} with no eligible residual" -f $filesSeen, $filesExcluded)
Write-Host ("Gaps:     {0} eligible, {1} excluded (pre-uninformative schema or no residual)" -f `
        $gapsEligible, $gapsExcluded)
Write-Host ''
Write-Host 'Buckets (plan section 4)' -ForegroundColor Cyan
'{0,-10} {1,8} {2,10}' -f 'bucket', 'count', '% eligible'
Write-Host ('-' * 32)
function Show-Pct([int]$n, [int]$den) {
    if ($den -le 0) { return '-' }
    return ('{0:N2}%' -f (100.0 * $n / $den))
}
'{0,-10} {1,8} {2,10}' -f 'A', $countA, (Show-Pct $countA $gapsEligible)
'{0,-10} {1,8} {2,10}' -f 'A_veto', $countAVeto, (Show-Pct $countAVeto $gapsEligible)
'{0,-10} {1,8} {2,10}' -f 'B', $countB, (Show-Pct $countB $gapsEligible)
'{0,-10} {1,8} {2,10}' -f 'C', $countC, (Show-Pct $countC $gapsEligible)
Write-Host ('-' * 32)
Write-Host ("A without veto stake: {0}" -f ($countA - $countAVeto)) -ForegroundColor DarkGray

Write-Host ''
Write-Host 'Split (a_channels x dual_fit_used; ? = tag absent)' -ForegroundColor Cyan
'{0,-22} {1,6} {2,8} {3,6} {4,6} {5,8}' -f 'key', 'A', 'A_veto', 'B', 'C', 'other'
Write-Host ('-' * 60)
foreach ($k in ($split.Keys | Sort-Object)) {
    $s = $split[$k]
    '{0,-22} {1,6} {2,8} {3,6} {4,6} {5,8}' -f $k, $s.A, $s.A_veto, $s.B, $s.C, $s.Other
}

$aRows = @($rows | Where-Object { $_.bucket -eq 'A' -or $_.bucket -eq 'A_veto' } |
        Sort-Object { [string]$_.pair }, { [int]$_.gap_index })
if ($ListA -or $aRows.Count -le 40) {
Write-Host ''
Write-Host 'Bucket A / A_veto rows (pair, gap_index only)' -ForegroundColor Cyan
    '{0,-12} {1,8} {2,8} {3,8} {4,10} {5,6} {6,6} {7,6}' -f `
        'pair', 'gap', 'bucket', 'inf', 'headroom', 'ch', 'dual', 'margin'
    Write-Host ('-' * 72)
    foreach ($r in $aRows) {
        $hr = if ($r.worst_headroom_db -eq '') { 'NaN' } else { $r.worst_headroom_db }
        '{0,-12} {1,8} {2,8} {3,8} {4,10} {5,6} {6,6} {7,6}' -f `
            $r.pair, $r.gap_index, $r.bucket, $r.informative, $hr, $r.a_channels, $r.dual_fit_used, $r.margin_db
    }
}
elseif ($aRows.Count -gt 0) {
    Write-Host ''
    Write-Host ("Bucket A / A_veto: {0} row(s) — pass -ListA to print, or -CsvOut to write." -f $aRows.Count) `
        -ForegroundColor DarkGray
}

if ($CsvOut -ne '') {
    $csvDir = Split-Path -Parent $CsvOut
    if ($csvDir -and -not (Test-Path -LiteralPath $csvDir)) {
        New-Item -ItemType Directory -Force -Path $csvDir | Out-Null
    }
    $rows | Sort-Object { [string]$_.pair }, { [int]$_.gap_index }, bucket |
        Export-Csv -LiteralPath $CsvOut -NoTypeInformation -Encoding utf8
    Write-Host ''
    Write-Host "CSV: $CsvOut ($($rows.Count) ProbeNonFinite-bearing gap(s))" -ForegroundColor DarkGray
}

Write-Host ''
Write-Host 'Paste counts into docs/dev/archive/TEMP-residual-measured-unify-plan.md §5.1 before Phase 3.' `
    -ForegroundColor DarkGray
Write-Host 'No gate change from this script.' -ForegroundColor DarkGray

if ($countA -eq 0) {
    Write-Host ''
    Write-Host 'A is empty on this dump set — keep the Phase 0 pin; do not unify on zero evidence.' `
        -ForegroundColor Yellow
}
