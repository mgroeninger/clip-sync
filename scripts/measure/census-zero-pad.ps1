#!/usr/bin/env pwsh
#Requires -Version 7.0
# Consumer-only census of `GapTags::zero_pad_frames` over a patch-outcome corpus: which patched
# gaps came up SHORT of the gap length because donor B ran out, and how that rate splits by
# `signature_mode` / `fit_path`.
#
# WHAT THE FIELD MEANS NOW. Since 2026-09-23 (`b635e15`) a shortfall is *not* written as silence:
# `execute_bracket_output` shortens the patch destination to what B actually supplied, so A keeps
# its own material across the tail. `zero_pad_frames > 0` therefore reads "donor B was exhausted
# and this many frames went unrepaired", NOT "this many frames of digital black were spliced in".
# Before that commit the same count would have been a hole flush against the post seam. A census run
# against pre-b635e15 reports is still meaningful, but it is measuring the padded behaviour — the
# two arms are not poolable, and this script cannot tell them apart (no report carries the binary's
# commit). Keep the arms in separate -OutDir trees.
#
# ABSENCE IS A REAL ZERO. `zero_pad_frames` carries `skip_serializing_if = "is_zero_frames"`
# (domain/gap_tags.rs), so it is omitted from JSON whenever it is 0. A missing key is therefore
# "no shortfall", never "not instrumented" — this script treats absent as 0 deliberately.
#
# THE DENOMINATOR IS THE BRACKET PATH ONLY. `execute.rs` has two patch arms. The bracket arm passes
# `bracket_fill.zero_pad_frames` through; the SilenceSplice/dual-fit arm hardcodes 0 — correctly,
# because the dual-fit bridge is *synthesized* to the gap length, never cut and padded to it. A 0 on
# a dual-fit gap is true, not missing instrumentation. So the only gaps that CAN short-fill are
# `patched` with `dual_fit_used = false`, and that is the denominator reported as "bracket-eligible".
# Rates quoted against all gaps, or against all patched gaps, understate it.
#
# WHAT THIS CANNOT ANSWER. Whether a short fill is AUDIBLE. The count is a frame shortfall; audibility
# depends on shoulder continuity, which no field here carries (see the refuted `fill_level` cutoff).
# It also cannot produce a before/after differential on its own — no run predating the field carries
# it, so "0 now" has no same-gap "n then" to sit against.
#
# Consumer only — opens no media, runs no repair, needs no oracle. Point it at a directory
# measure-patch-outcomes.ps1 already wrote.
#
# Usage:
#   ./scripts/measure/census-zero-pad.ps1 -OutDir gap-files/<date>-zeropad-census
#   ./scripts/measure/census-zero-pad.ps1 -OutDir gap-files/<date>-zeropad-census -CsvOut gap-files/zero-pad.csv
#   ./scripts/measure/census-zero-pad.ps1 -OutDir gap-files/<date>-zeropad-census -AllGaps   # every gap, not just short fills
#
# Accepted layout: `<OutDir>/<label>.json`, one per pair, as measure-patch-outcomes.ps1 writes them.
# `fill-level-rollup.csv` and any zero-byte report (a pair still in flight) are skipped and counted.
# Safe to run against a live sweep.
#
# MEDIA HYGIENE: emit pair label + gap index only — never media paths or titles. The reports DO carry
# `scan.video_a` / `scan.video_b`; this script must never read them into output.
# Docs: docs/dev/archive/TEMP-dual-fit-registration-routing-plan.md ("Reading the zero-pad census").

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$OutDir,

    # Per-gap CSV of every gap that could short-fill (bracket-eligible), short or not.
    [string]$CsvOut = '',

    # Emit the per-gap table for ALL bracket-eligible gaps, not only the short fills.
    [switch]$AllGaps
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $OutDir)) {
    throw "patch-outcome dir not found: $OutDir"
}

function Test-Prop([object]$Obj, [string]$Name) {
    if ($null -eq $Obj) { return $false }
    return [bool]($Obj.PSObject.Properties.Name -contains $Name)
}

function Get-Prop([object]$Obj, [string]$Name) {
    if (-not (Test-Prop $Obj $Name)) { return $null }
    return $Obj.$Name
}

# The status is an externally-tagged enum: a one-key object (`patched` / `skipped` / `not_planned`).
function Get-StatusName([object]$Status) {
    if ($null -eq $Status) { return 'none' }
    if ($Status -is [string]) { return $Status }
    $names = @($Status.PSObject.Properties.Name)
    if ($names.Count -eq 0) { return 'none' }
    return $names[0]
}

function Format-Num([object]$Value) {
    if ($null -eq $Value) { return '' }
    if ($Value -is [bool]) { return $Value.ToString() }
    $d = $Value -as [double]
    if ($null -eq $d) { return [string]$Value }
    if ([double]::IsNaN($d)) { return 'NaN' }
    return $d.ToString('R', [System.Globalization.CultureInfo]::InvariantCulture)
}

$files = @(Get-ChildItem -LiteralPath $OutDir -File -Filter '*.json' | Sort-Object {
    ($_.BaseName -as [int]) ?? [int]::MaxValue
}, Name)

if ($files.Count -eq 0) {
    throw "no per-pair JSON under: $OutDir"
}

$rows = [System.Collections.Generic.List[object]]::new()
$pairs = [System.Collections.Generic.HashSet[string]]::new()
$statusCounts = @{}
$totalGaps = 0
$inFlight = [System.Collections.Generic.List[string]]::new()
$unparsed = [System.Collections.Generic.List[string]]::new()

foreach ($file in $files) {
    $label = $file.BaseName
    if ($file.Length -eq 0) {
        # measure-patch-outcomes.ps1 creates the redirect target at start and writes the document in
        # one shot at the end, so a zero-byte report is a pair still running, not a corrupt one.
        $inFlight.Add($label)
        continue
    }
    try {
        $doc = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    } catch {
        $unparsed.Add($label)
        continue
    }

    $patch = Get-Prop $doc 'patch'
    if ($null -eq $patch -or -not (Test-Prop $patch 'gaps')) { continue }
    [void]$pairs.Add($label)

    # ms conversion uses the pair's own rate, never a hardcoded 48k.
    $rate = Get-Prop (Get-Prop (Get-Prop $doc 'scan') 'track_compatibility') 'a_sample_rate'
    if ($null -eq $rate -or [double]$rate -le 0) { $rate = 48000 }

    $index = 0
    foreach ($g in @($patch.gaps)) {
        $index++
        $totalGaps++

        $status = Get-StatusName (Get-Prop $g 'status')
        if (-not $statusCounts.ContainsKey($status)) { $statusCounts[$status] = 0 }
        $statusCounts[$status]++

        $tags = Get-Prop $g 'tags'
        $dualFit = (Get-Prop $tags 'dual_fit_used') -eq $true

        # Only the bracket arm can short-fill; dual-fit's 0 is structural, so it is not a denominator.
        $eligible = ($status -eq 'patched') -and (-not $dualFit)
        if (-not $eligible) { continue }

        $zp = Get-Prop $tags 'zero_pad_frames'
        if ($null -eq $zp) { $zp = 0 }   # skip_serializing_if — absent IS zero
        $zp = [int]$zp

        $rows.Add([pscustomobject][ordered]@{
            pair             = $label
            gap_index        = $index
            a_start_secs     = Format-Num (Get-Prop $g 'a_start_secs')
            a_end_secs       = Format-Num (Get-Prop $g 'a_end_secs')
            zero_pad_frames  = $zp
            zero_pad_ms      = [math]::Round(($zp / [double]$rate) * 1000.0, 2)
            short_fill       = ($zp -gt 0)
            signature_mode   = Format-Num (Get-Prop $tags 'signature_mode')
            fit_path         = Format-Num (Get-Prop $tags 'fit_path')
            patch_tier       = Format-Num (Get-Prop $tags 'patch_tier')
            plan_kind        = Format-Num (Get-Prop $tags 'plan_kind')
            seam_shape       = Format-Num (Get-Prop $tags 'seam_shape')
            anchor_seam_used = Format-Num (Get-Prop $tags 'anchor_seam_used')
            residual_band    = Format-Num (Get-Prop $tags 'residual_band')
            fill_level_db    = Format-Num (Get-Prop (Get-Prop $g 'fill_level') 'peak_delta_db')
            sample_rate      = $rate
        })
    }
}

$shorts = @($rows | Where-Object { $_.short_fill })

Write-Host ""
Write-Host ("Corpus: {0}" -f $OutDir)
Write-Host ("Pairs parsed {0} | gaps {1}" -f $pairs.Count, $totalGaps)
if ($inFlight.Count -gt 0) {
    Write-Host ("in flight (zero-byte, pair still running): {0}" -f ($inFlight -join ', ')) -ForegroundColor DarkGray
}
if ($unparsed.Count -gt 0) {
    Write-Host ("UNPARSEABLE: {0}" -f ($unparsed -join ', ')) -ForegroundColor Yellow
}

$statusLine = (($statusCounts.GetEnumerator() | Sort-Object Name |
    ForEach-Object { '{0} {1}' -f $_.Key, $_.Value }) -join ' | ')
Write-Host ("Status: {0}" -f $statusLine)

$pct = if ($totalGaps -gt 0) { 100.0 * $rows.Count / $totalGaps } else { 0 }
Write-Host ("Bracket-eligible (patched, not dual-fit): {0} ({1:N0}% of gaps) <-- the denominator" -f $rows.Count, $pct)

$shortPct = if ($rows.Count -gt 0) { 100.0 * $shorts.Count / $rows.Count } else { 0 }
Write-Host ("Short fills (zero_pad_frames > 0)       : {0} ({1:N2}% of eligible)" -f $shorts.Count, $shortPct)
Write-Host ""

if ($rows.Count -eq 0) {
    Write-Host "no bracket-eligible gaps — nothing to census." -ForegroundColor Yellow
    return
}

# The Energy-heavy prediction needs BOTH arms present; a one-armed corpus tests nothing, so say so.
Write-Host "--- short-fill rate by signature_mode ---"
$byMode = @($rows | Group-Object signature_mode | Sort-Object Name)
foreach ($grp in $byMode) {
    $n = $grp.Count
    $s = @($grp.Group | Where-Object { $_.short_fill }).Count
    Write-Host ("  {0,-10} {1,5} eligible | {2,4} short ({3,6:N2}%)" -f $grp.Name, $n, $s, (100.0 * $s / $n))
}
if ($byMode.Count -lt 2) {
    Write-Host ("  only one signature_mode present — the mode comparison is UNDETERMINED, not null.") -ForegroundColor Yellow
}
Write-Host ""

Write-Host "--- short-fill rate by fit_path ---"
foreach ($grp in @($rows | Group-Object fit_path | Sort-Object Name)) {
    $n = $grp.Count
    $s = @($grp.Group | Where-Object { $_.short_fill }).Count
    Write-Host ("  {0,-16} {1,5} eligible | {2,4} short ({3,6:N2}%)" -f $grp.Name, $n, $s, (100.0 * $s / $n))
}
Write-Host ""

$table = if ($AllGaps) { $rows } else { $shorts }
if ($table.Count -gt 0) {
    $caption = if ($AllGaps) { 'all bracket-eligible gaps' } else { 'short fills' }
    Write-Host ("--- {0} ---" -f $caption)
    $table | Sort-Object @{ Expression = { ($_.pair -as [int]) ?? [int]::MaxValue } }, gap_index |
        Format-Table pair, gap_index, zero_pad_frames, zero_pad_ms, signature_mode, fit_path,
                     patch_tier, anchor_seam_used, fill_level_db -AutoSize
} elseif (-not $AllGaps) {
    Write-Host "no short fills — every bracket-eligible gap was filled to length from contiguous B."
    Write-Host "  (that is the post-b635e15 expectation: extend_short_fill_to_gap_frames takes every" -ForegroundColor DarkGray
    Write-Host "   frame B can supply, so a shortfall now requires genuine donor exhaustion.)" -ForegroundColor DarkGray
}

if ($CsvOut -ne '') {
    $csvDir = Split-Path -Parent $CsvOut
    if ($csvDir -ne '' -and -not (Test-Path -LiteralPath $csvDir)) {
        New-Item -ItemType Directory -Path $csvDir | Out-Null
    }
    $rows | Sort-Object @{ Expression = { ($_.pair -as [int]) ?? [int]::MaxValue } }, gap_index |
        Export-Csv -LiteralPath $CsvOut -NoTypeInformation -Encoding utf8
    Write-Host ""
    Write-Host "CSV: $CsvOut ($($rows.Count) bracket-eligible gap(s))" -ForegroundColor DarkGray
}
