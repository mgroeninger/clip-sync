#!/usr/bin/env pwsh
#Requires -Version 7.0
# Matched per-gap comparison of TWO collar-level census arms (§4b of
# docs/dev/archive/TEMP-local-paired-gain-plan.md). Sibling of census-collar-levels.ps1, which describes ONE
# arm; this one answers "did the change help?", which no single-dump census can.
#
# WHY THIS IS A SCRIPT AND NOT AN AD-HOC JOIN. Every arm comparison in §4b has to obey the same two
# rules, and both have already produced a wrong reading once:
#
#   1. Median OF THE PER-GAP RATIOS, never the ratio of the aggregate medians (settled.md §E). The
#      arms measure different gap populations at the margin, so aggregates move for reasons that have
#      nothing to do with the change. The `W` arms were misread this way: the aggregate SE median
#      moved 0.329 -> 0.310 and looked flat, while the matched median ratio was 0.74x.
#
#   2. EXCLUDE ROWS THE CHANGE DID NOT TOUCH. When an arm only moves a subset of gaps, the untouched
#      rows are bit-identical between the dumps and their ratio is exactly 1.000. Pool them in and
#      the median is 1.000 no matter how large the effect on the rows that did move. Arm 1 -> arm 3
#      was misread this way: 307 of 830 trusted collars fitted a lag of zero, the unfiltered matched
#      ratio read 1.000 for spread, residual AND SE, and the moved-only cohort read 0.293 / 0.405.
#
# Rule 2 is the general form of "compare only the gaps whose map moved". This script does not need to
# know WHAT changed between the arms: it identifies untouched rows by their readings being identical
# and reports the changed cohort as the headline, so it works for a map arm, a width arm, or anything
# else. The unchanged count is printed rather than hidden — a comparison where almost nothing changed
# is a finding about the change, and it is the one shape that looks identical to a bug.
#
# Usage (each side is a dump dir OR an already-written per-gap CSV from census-collar-levels.ps1):
#   ./scripts/measure/compare-collar-arms.ps1 -BaseDir gap-files/<date>-collar-census `
#                                             -NewDir  gap-files/<date>-collar-census-lag-work
#   ./scripts/measure/compare-collar-arms.ps1 -BaseDir a/collar-levels.csv -NewDir b/collar-levels.csv `
#                                             -CsvOut gap-files/arm1-vs-arm3.csv
#
# Given a dump dir, the per-gap rows are produced by INVOKING census-collar-levels.ps1, not by a
# second parser. A comparator that read the JSON its own way could disagree with the census about the
# same dump, which is the one failure mode that would discredit both.
#
# BOTH SIDES MUST DIFFER IN ONE THING. Arms 1->2 isolated `W`; arms 1->3 isolated the map; arm 4
# confounds both and is a shipping check, not an experiment. This script cannot tell which you gave
# it — it prints the stamped collar width of each side (scan.local_pair_collar_secs, stamped since
# 2026-08-27) and whether each carries `map_lag_ms`, so a confounded pair is at least visible.
#
# MEDIA HYGIENE: pair labels come from filenames and nothing else is read from the path. Numeric pair
# indices only — never titles or media paths. See docs/dev/repair-perf.md §"Media handling".

[CmdletBinding()]
param(
    # Baseline arm: a dump dir of <pair>.json, or a per-gap CSV written by census-collar-levels.ps1.
    [Parameter(Mandatory = $true)]
    [string]$BaseDir,

    # The arm under test. Ratios are new/base, so below 1.0 is an improvement for spread, SE and
    # |residual|, and above 1.0 is an improvement for n_pairs.
    [Parameter(Mandatory = $true)]
    [string]$NewDir,

    [string]$BaseLabel = '',
    [string]$NewLabel = '',

    # Joined per-gap CSV, one row per gap measured in both arms.
    [string]$CsvOut = '',

    # Secondary cut, applied to BOTH arms. The recorded §4b baselines were taken at n>=8 and comparing
    # an unfiltered cohort against them reads two different populations.
    [int]$MinPairs = 8,

    # Reporting cut for the cohort-clearing counts. Not a gate.
    [double]$SeDb = 1.0
)

$ErrorActionPreference = 'Stop'

function Format-Ratio([object]$Value) {
    if ($null -eq $Value) { return '  -  ' }
    return ([double]$Value).ToString('0.000')
}

# Nearest-rank, matching census-collar-levels.ps1 (lower-middle median, never averaged, so every
# quoted value is one actually observed). The two scripts must not disagree about what a median is.
function Get-Percentile([double[]]$Sorted, [double]$Fraction) {
    if ($Sorted.Count -eq 0) { return $null }
    $rank = [int][Math]::Ceiling($Fraction * $Sorted.Count) - 1
    if ($rank -lt 0) { $rank = 0 }
    if ($rank -ge $Sorted.Count) { $rank = $Sorted.Count - 1 }
    return $Sorted[$rank]
}

function Get-Median([double[]]$Values) {
    if ($Values.Count -eq 0) { return $null }
    return Get-Percentile ([double[]]@($Values | Sort-Object)) 0.5
}

# Import-Csv renders an absent value as '', which [double] silently turns into 0. A null residual
# means the row ABSTAINED (no collar at all) and a null map_lag_ms means no fit cleared the trust
# floor; both would read as a real zero and change every count below.
#
# Since 2026-08-31 a residual is no longer null merely because the whole-program offset was refused
# or absent: §4's prior is 0 there, so the census records the residual as `d_local` itself and names
# the case in `prior_source`. Both arms read the same pairs, so the prior cannot differ across a
# joined row - the change moves rows INTO every cohort below rather than between them.
function AsD([object]$Value) {
    if ($null -eq $Value -or "$Value" -eq '') { return $null }
    return [double]$Value
}

# The stamped width of an arm, for the header. Read from the dump itself rather than the directory
# name — §4b's "confirm the arm is the arm you meant", which is exactly the check a comparison needs
# and the one arms 1-3 could not make.
function Get-ArmProvenance([string]$Path) {
    $out = [ordered]@{ width = $null; has_map_lag = $null; source = $Path }
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) { return $out }
    foreach ($file in @(Get-ChildItem -LiteralPath $Path -Filter '*.json' -File | Sort-Object Name)) {
        if ($file.Length -eq 0) { continue }
        try { $report = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json } catch { continue }
        $scan = $report.scan
        if ($null -eq $scan) { continue }
        if ($scan.PSObject.Properties.Name -contains 'local_pair_collar_secs' -and $null -ne $scan.local_pair_collar_secs) {
            $out.width = [double]$scan.local_pair_collar_secs
        }
        foreach ($gap in @($scan.gaps)) {
            $collar = $gap.collar_level
            if ($null -ne $collar) {
                $out.has_map_lag = ($collar.PSObject.Properties.Name -contains 'map_lag_ms')
                break
            }
        }
        break
    }
    return $out
}

# A side is either a CSV or a dump dir. A dump dir is censused rather than re-parsed, so both scripts
# always agree about the same dump. Write-Host (stream 6) is suppressed because the single-arm
# questions are not this script's output; warnings are NOT — a stale binary or a mixed-width DumpDir
# must still be shouted about, and is more dangerous here than in the census.
function Get-ArmRows([string]$Path, [string]$Name) {
    if (Test-Path -LiteralPath $Path -PathType Leaf) {
        if ([System.IO.Path]::GetExtension($Path) -ne '.csv') {
            throw "$Name`: expected a dump dir or a .csv from census-collar-levels.ps1, got $Path"
        }
        return @(Import-Csv -LiteralPath $Path)
    }
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "$Name`: not found: $Path"
    }
    $census = Join-Path $PSScriptRoot 'census-collar-levels.ps1'
    if (-not (Test-Path -LiteralPath $census)) { throw "census-collar-levels.ps1 not found next to this script" }
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("collar-arm-{0}.csv" -f [guid]::NewGuid())
    try {
        & $census -DumpDir $Path -CsvOut $tmp 6> $null
        return @(Import-Csv -LiteralPath $tmp)
    } finally {
        if (Test-Path -LiteralPath $tmp) { Remove-Item -LiteralPath $tmp -Force }
    }
}

if (-not $BaseLabel) { $BaseLabel = Split-Path -Leaf ($BaseDir.TrimEnd('\', '/')) }
if (-not $NewLabel) { $NewLabel = Split-Path -Leaf ($NewDir.TrimEnd('\', '/')) }

$baseRows = Get-ArmRows $BaseDir 'BaseDir'
$newRows = Get-ArmRows $NewDir 'NewDir'
$baseProv = Get-ArmProvenance $BaseDir
$newProv = Get-ArmProvenance $NewDir

$baseM = @($baseRows | Where-Object { $_.state -eq 'measured' })
$newM = @($newRows | Where-Object { $_.state -eq 'measured' })

Write-Host ''
Write-Host 'Matched collar-arm comparison'
foreach ($side in @(@('base', $BaseLabel, $baseProv, $baseRows, $baseM), @('new ', $NewLabel, $newProv, $newRows, $newM))) {
    $prov = $side[2]
    $width = if ($null -eq $prov.width) { 'n/a (predates the stamp)' } else { ('{0:0.0} s' -f $prov.width) }
    $map = switch ($prov.has_map_lag) {
        $true { 'map_lag_ms yes' }
        $false { 'map_lag_ms NO (nominal map)' }
        default { 'map unknown (CSV input)' }
    }
    Write-Host ("  {0} {1,-38} gaps {2,5}  measured {3,5}  W {4,-24} {5}" -f `
            $side[0], $side[1], @($side[3]).Count, @($side[4]).Count, $width, $map)
}
if ($null -ne $baseProv.width -and $null -ne $newProv.width -and $baseProv.width -ne $newProv.width `
        -and $baseProv.has_map_lag -ne $newProv.has_map_lag) {
    Write-Warning 'CONFOUNDED ARMS: these two differ in BOTH collar width and map. Nothing below attributes an effect to either one. That is a shipping check, not an experiment (§4b).'
}

# ---------------------------------------------------------------------------------------------
# Coverage. Gained/lost collars are a result in their own right and must not be silently dropped
# by the join: an arm that measures more gaps is measuring THINNER ones, and those drag every
# aggregate. This is the same trap §4b's Result 1 note describes for the `W` arms.
# ---------------------------------------------------------------------------------------------
$baseIdx = @{}
foreach ($r in $baseM) { $baseIdx["$($r.pair)|$($r.gap_index)"] = $r }
$newIdx = @{}
foreach ($r in $newM) { $newIdx["$($r.pair)|$($r.gap_index)"] = $r }

$joined = [System.Collections.Generic.List[object]]::new()
foreach ($r in $newM) {
    $key = "$($r.pair)|$($r.gap_index)"
    if ($baseIdx.ContainsKey($key)) { $joined.Add([pscustomobject]@{ key = $key; b = $baseIdx[$key]; n = $r }) }
}
$gained = @($newM | Where-Object { -not $baseIdx.ContainsKey("$($_.pair)|$($_.gap_index)") })
$lost = @($baseM | Where-Object { -not $newIdx.ContainsKey("$($_.pair)|$($_.gap_index)") })

Write-Host ''
Write-Host 'Coverage'
Write-Host ("  measured in both                {0}" -f $joined.Count)
Write-Host ("  gained a collar in the new arm  {0}{1}" -f $gained.Count, `
    $(if ($gained.Count) { '  (median n_pairs {0} - thin collars drag every aggregate)' -f (Get-Median ([double[]]@($gained | ForEach-Object { [double]$_.n_pairs }))) } else { '' }))
Write-Host ("  lost a collar                   {0}" -f $lost.Count)

if ($joined.Count -eq 0) {
    Write-Host ''
    Write-Host 'No gap is measured in both arms - nothing to match on.'
    Write-Host ''
    return
}

# ---------------------------------------------------------------------------------------------
# Rule 2. A row the change did not touch reads identically in both arms and contributes an exact
# 1.000 to every ratio. Identity is judged on the READINGS, not on any knowledge of what the arms
# differ in, so this works for a map arm, a width arm, or anything else.
# ---------------------------------------------------------------------------------------------
function Test-Unchanged($b, $n) {
    foreach ($field in @('n_pairs', 'offset_db', 'spread_db')) {
        $bv = AsD $b.$field
        $nv = AsD $n.$field
        if ($null -eq $bv -and $null -eq $nv) { continue }
        if ($null -eq $bv -or $null -eq $nv) { return $false }
        if ([Math]::Abs($bv - $nv) -gt 1e-9) { return $false }
    }
    return $true
}
foreach ($j in $joined) { $j | Add-Member -NotePropertyName changed -NotePropertyValue (-not (Test-Unchanged $j.b $j.n)) }
$changed = @($joined | Where-Object { $_.changed })

Write-Host ''
Write-Host 'What the change actually touched'
Write-Host ("  identical in both arms          {0} of {1}  (contribute an exact 1.000 to every ratio - EXCLUDED below)" -f `
    ($joined.Count - $changed.Count), $joined.Count)
Write-Host ("  changed                         {0}" -f $changed.Count)
if ($changed.Count -eq 0) {
    Write-Host ''
    Write-Host '  Nothing changed. Either the arms are the same run, or the knob did not reach the estimator -'
    Write-Host '  confirm the binary and the config before reading this as "the change had no effect".'
    Write-Host ''
    return
}
if ($changed.Count * 4 -lt $joined.Count) {
    Write-Host ("  NOTE: only {0:0.0}% of matched gaps moved. The headline below describes those; the corpus-wide" -f (100.0 * $changed.Count / $joined.Count))
    Write-Host '  effect is that, diluted by the rest. Both numbers are real - quote which one you mean.'
}

# ---------------------------------------------------------------------------------------------
# Rule 1. Median of the per-gap ratios. Aggregate medians are printed BESIDE them, deliberately:
# seeing the two disagree is what teaches the §E lesson, and hiding the aggregate just means
# someone recomputes it by hand later.
# ---------------------------------------------------------------------------------------------
$metrics = @(
    @{ name = 'n_pairs '; field = 'n_pairs'; abs = $false; better = 'higher' },
    @{ name = 'spread  '; field = 'spread_db'; abs = $false; better = 'lower' },
    @{ name = 'se      '; field = 'se_db'; abs = $false; better = 'lower' },
    @{ name = '|resid| '; field = 'residual_db'; abs = $true; better = 'lower' }
)

function Show-Cohort([string]$Title, $Set) {
    Write-Host ("  -- {0}: {1} gaps --" -f $Title, @($Set).Count)
    if (@($Set).Count -eq 0) { return }
    foreach ($m in $metrics) {
        $ratios = [System.Collections.Generic.List[double]]::new()
        $bvals = [System.Collections.Generic.List[double]]::new()
        $nvals = [System.Collections.Generic.List[double]]::new()
        $better = 0
        foreach ($j in $Set) {
            $bv = AsD $j.b.($m.field)
            $nv = AsD $j.n.($m.field)
            if ($null -eq $bv -or $null -eq $nv) { continue }
            if ($m.abs) { $bv = [Math]::Abs($bv); $nv = [Math]::Abs($nv) }
            $bvals.Add($bv); $nvals.Add($nv)
            # A zero denominator has no ratio. Dropping the row understates nothing: it is counted in
            # the aggregate medians beside it, and a zero base reading is not an improvable one.
            if ($bv -gt 0) { $ratios.Add($nv / $bv) }
            if ($m.better -eq 'lower') { if ($nv -lt $bv) { $better++ } } else { if ($nv -gt $bv) { $better++ } }
        }
        if ($bvals.Count -eq 0) { Write-Host ("     {0} no rows carry it in both arms" -f $m.name); continue }
        Write-Host ("     {0} ratio med {1}  ({2} on {3,4} of {4,4})   aggregate med {5} -> {6}" -f `
                $m.name, (Format-Ratio (Get-Median ([double[]]$ratios))), $m.better, $better, $bvals.Count, `
            (Format-Ratio (Get-Median ([double[]]$bvals))), (Format-Ratio (Get-Median ([double[]]$nvals))))
    }
}

Write-Host ''
Write-Host ("Matched per-gap ratios, changed rows only (new/base; ratio med is the statistic, per settled.md {0}E)" -f [char]0xA7)
Show-Cohort 'all changed' $changed
$deep = @($changed | Where-Object { [double]$_.b.n_pairs -ge $MinPairs -and [double]$_.n.n_pairs -ge $MinPairs })
Show-Cohort ("n>={0} in both" -f $MinPairs) $deep

# ---------------------------------------------------------------------------------------------
# Which map each side read through. Only meaningful when both arms carry the field; when they read
# through the SAME map (a width arm) the partition is empty and says so, rather than inventing a
# cohort.
#
# A NULL map_lag_ms COUNTS AS ZERO HERE, and only here. The census keeps "no fit cleared the trust
# floor" (null) apart from "the dump predates the field" (absent) because they are different facts
# about provenance. For a MAP comparison they are the same fact: `map_lag_ms` is measured relative to
# the nominal map, so both mean the collar was read at nominal, i.e. displaced from it by zero.
# Treating null as its own value instead makes every arm-1-vs-arm-3 row "a different map" - including
# the ones the corrected arm fitted at 0, which are read through the identical map by construction -
# and readmits exactly the dilution rule 2 exists to keep out (it read spread 0.821 / |resid| 1.000
# against the true 0.293 / 0.403).
# ---------------------------------------------------------------------------------------------
function Get-EffectiveLag($Row) {
    $v = AsD $Row.map_lag_ms
    if ($null -eq $v) { return [double]0 }
    return $v
}
$anyLag = @($joined | Where-Object { $null -ne (AsD $_.b.map_lag_ms) -or $null -ne (AsD $_.n.map_lag_ms) })
if ($anyLag.Count -gt 0) {
    $lagMoved = @($joined | Where-Object {
            [Math]::Abs((Get-EffectiveLag $_.b) - (Get-EffectiveLag $_.n)) -gt 1e-9
        })
    Write-Host ''
    Write-Host 'Map partition'
    Write-Host ("  gaps read through a DIFFERENT map between the arms  {0} of {1}" -f $lagMoved.Count, $joined.Count)
    if ($lagMoved.Count -eq 0) {
        Write-Host '  (both arms read every gap at the same lag - this is a width arm, or the map did not move)'
    } else {
        # Rule 2 again, inside the partition. A row whose map moved but whose readings did not is
        # counted here rather than dropped silently: it is evidence about the correction, not noise.
        $movedSame = @($lagMoved | Where-Object { -not $_.changed }).Count
        if ($movedSame -gt 0) {
            Write-Host ("  of those, {0} read identically anyway (map moved, nothing else did) - excluded below" -f $movedSame)
        }
        $sel = @($lagMoved | Where-Object { $_.changed -and [double]$_.b.n_pairs -ge $MinPairs -and [double]$_.n.n_pairs -ge $MinPairs })
        Show-Cohort ("map moved, n>={0} in both" -f $MinPairs) $sel
    }
}

# ---------------------------------------------------------------------------------------------
# DECISION IMPACT. Every ratio above compares the instrument; this compares the PRODUCT. |residual|
# is |g_local_db| (§4), so the change in residual between two arms is, in dB, the change in the gain
# that lands on a fill. A ratio cannot answer this: ×0.5 on a 0.1 dB residual is inaudible and ×0.5
# on a 6 dB one is not, and an arm that halves a residual has still MOVED the gain by half of it.
#
# This is the number that says whether an arm is a measurement cleanup or a correctness fix, and it
# is reported as a difference in dB against thresholds, never as a ratio.
# ---------------------------------------------------------------------------------------------
$gainRows = @($joined | Where-Object { $null -ne (AsD $_.b.residual_db) -and $null -ne (AsD $_.n.residual_db) })
if ($gainRows.Count -gt 0) {
    Write-Host ''
    Write-Host 'Decision impact: how far the applied gain moved (|resid_new - resid_base|, dB)'
    $sets = @(@('all matched   ', $gainRows),
        @(("n>={0} in both " -f $MinPairs), @($gainRows | Where-Object { [double]$_.b.n_pairs -ge $MinPairs -and [double]$_.n.n_pairs -ge $MinPairs })))
    # The changed-rows cut matters here for the same reason as everywhere else: untouched rows
    # contribute an exact 0.000 dB and drag the median to zero however large the real effect is.
    $sets += , @(("changed, n>={0}" -f $MinPairs), @($gainRows | Where-Object { $_.changed -and [double]$_.b.n_pairs -ge $MinPairs -and [double]$_.n.n_pairs -ge $MinPairs }))
    foreach ($set in $sets) {
        $d = [double[]]@($set[1] | ForEach-Object { [Math]::Abs((AsD $_.n.residual_db) - (AsD $_.b.residual_db)) } | Sort-Object)
        if ($d.Count -eq 0) { Write-Host ("  {0} none" -f $set[0]); continue }
        Write-Host ("  {0} n={1,4}  p50 {2}  p90 {3}  max {4}" -f `
                $set[0], $d.Count, (Format-Ratio (Get-Percentile $d 0.50)), (Format-Ratio (Get-Percentile $d 0.90)), (Format-Ratio (Get-Percentile $d 1.0)))
        foreach ($t in @(0.5, 1.0, 3.0)) {
            $c = @($d | Where-Object { $_ -gt $t }).Count
            Write-Host ("       moved the gain by > {0,3:0.0} dB  {1,4} of {2,4}  ({3,4:0.0}%)" -f $t, $c, $d.Count, (100.0 * $c / $d.Count))
        }
    }
}

# ---------------------------------------------------------------------------------------------
# Reporting cuts, not chosen constants (§4b's standing caution). Printed for both arms because the
# cohort that clears a floor AND an SE bound is what a candidate `MIN_LOCAL_PAIRS` actually costs.
# ---------------------------------------------------------------------------------------------
Write-Host ''
Write-Host ("Cohort clearing n_pairs >= {0} and se <= {1} dB  (reporting cuts, NOT constants)" -f $MinPairs, $SeDb)
foreach ($side in @(@($BaseLabel, $baseRows, $baseM), @($NewLabel, $newRows, $newM))) {
    $clear = @($side[2] | Where-Object {
            $se = AsD $_.se_db
            [double]$_.n_pairs -ge $MinPairs -and $null -ne $se -and $se -le $SeDb
        })
    $total = @($side[1]).Count
    Write-Host ("  {0,-40} {1,5} of {2,5} gaps ({3:0.0}%)" -f $side[0], $clear.Count, $total, `
        $(if ($total) { 100.0 * $clear.Count / $total } else { 0 }))
}

if ($CsvOut) {
    $out = foreach ($j in $joined) {
        [pscustomobject][ordered]@{
            pair             = $j.n.pair
            gap_index        = $j.n.gap_index
            gap_secs         = $j.n.gap_secs
            changed          = $j.changed
            base_n_pairs     = $j.b.n_pairs
            new_n_pairs      = $j.n.n_pairs
            base_spread_db   = $j.b.spread_db
            new_spread_db    = $j.n.spread_db
            base_se_db       = $j.b.se_db
            new_se_db        = $j.n.se_db
            base_offset_db   = $j.b.offset_db
            new_offset_db    = $j.n.offset_db
            base_residual_db = $j.b.residual_db
            new_residual_db  = $j.n.residual_db
            base_map_lag_ms  = $j.b.map_lag_ms
            new_map_lag_ms   = $j.n.map_lag_ms
        }
    }
    $out | Export-Csv -LiteralPath $CsvOut -NoTypeInformation
    Write-Host ''
    Write-Host "joined per-gap CSV -> $CsvOut"
}
Write-Host ''
