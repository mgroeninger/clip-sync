#!/usr/bin/env pwsh
#Requires -Version 7.0
# Distribution of `registration_residual_ms` over patched gaps (BACKLOG.md § Fill placement, α).
#
# Walks `measure-patch-outcomes.ps1` output (`<DumpDir>/<pair>.json`, `patch.gaps[*]`) and summarizes
# the record-only placement residual on patched gaps: `applied_ms − shoulder_lag_ms`, positive = the
# written fill sits LATE of its shoulder. Bracket patches (`dual_fit_used: false`) are the α population;
# dual-fit rescues are tallied separately because their applied placement is hardcoded 0.0.
#
# VINTAGE. Dumps before 2026-09-18 did not shift the shoulder anchors by the template displacement
# nor add Fit's trim-head move, so their bracket values mostly read the ~350 ms border standoff, not
# registration (docs/json-output.md `registration_residual_ms`). `-BaselineDir` exists to put such a
# run beside a fixed one; do not read the old one as a placement measurement.
#
# PARTIAL RUNS are fine: zero-byte / unparseable reports (a pair still running or killed) are skipped
# and counted, so the census can be read while `measure-patch-outcomes.ps1` is mid-manifest.
#
# Usage:
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<date>-corpus-patch
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<new> -BaselineDir gap-files/<old>
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<new> -BinMs 10 -RangeMs 100
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<new> -CsvOut gap-files/residual.csv
#
# MEDIA HYGIENE: emits pair label + gap index only — never media paths or titles.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$DumpDir,

    # Optional second run summarized beside -DumpDir, with a per-gap paired delta on gaps both runs
    # patched as brackets (matched on pair label + A start to the millisecond).
    [string]$BaselineDir = '',

    # Histogram bin width and half-range (ms). Values beyond ±RangeMs land in the overflow rows.
    [double]$BinMs = 25.0,
    [double]$RangeMs = 400.0,

    # Optional CSV: one row per patched gap (pair, gap_index, a_start_secs, dual_fit, residual_ms, …).
    [string]$CsvOut = ''
)

$ErrorActionPreference = 'Stop'

function Read-Run([string]$Dir) {
    if (-not (Test-Path -LiteralPath $Dir)) { throw "dump dir not found: $Dir" }
    $rows = [System.Collections.Generic.List[object]]::new()
    $pairs = 0; $skipped = 0
    $files = Get-ChildItem -LiteralPath $Dir -Filter '*.json' -File |
        Sort-Object { [int]($_.BaseName -replace '\D', '0') }, BaseName
    foreach ($f in $files) {
        if ($f.Length -eq 0) { $skipped++; continue }
        try { $doc = Get-Content -LiteralPath $f.FullName -Raw | ConvertFrom-Json -Depth 64 }
        catch { $skipped++; continue }
        if ($null -eq $doc.patch -or $null -eq $doc.patch.gaps) { $skipped++; continue }
        $pairs++
        $i = -1
        foreach ($g in $doc.patch.gaps) {
            $i++
            $p = $g.status.patched
            if ($null -eq $p) { continue }
            $has = $p.PSObject.Properties.Name -contains 'registration_residual_ms'
            $rows.Add([pscustomobject]@{
                pair          = $f.BaseName
                gap_index     = $i
                a_start_secs  = [double]$g.a_start_secs
                dual_fit      = [bool]$p.dual_fit_used
                confidence    = [string]$p.confidence
                align_adj_ms  = [double]$p.align_adjustment_secs * 1000.0
                residual_ms   = if ($has -and $null -ne $p.registration_residual_ms) { [double]$p.registration_residual_ms } else { $null }
            })
        }
    }
    [pscustomobject]@{ Dir = $Dir; Pairs = $pairs; Skipped = $skipped; Rows = $rows }
}

function Get-Quantile([double[]]$Sorted, [double]$Q) {
    if ($Sorted.Count -eq 0) { return [double]::NaN }
    $pos = ($Sorted.Count - 1) * $Q
    $lo = [math]::Floor($pos); $hi = [math]::Ceiling($pos)
    $Sorted[$lo] + ($Sorted[$hi] - $Sorted[$lo]) * ($pos - $lo)
}

function Get-Stats([double[]]$Values) {
    $s = [double[]]($Values | Sort-Object)
    $abs = [double[]]($Values | ForEach-Object { [math]::Abs($_) } | Sort-Object)
    [pscustomobject]@{
        n          = $s.Count
        median     = Get-Quantile $s 0.5
        q1         = Get-Quantile $s 0.25
        q3         = Get-Quantile $s 0.75
        mean       = if ($s.Count) { ($s | Measure-Object -Average).Average } else { [double]::NaN }
        abs_median = Get-Quantile $abs 0.5
        late       = @($s | Where-Object { $_ -gt 0 }).Count
        early      = @($s | Where-Object { $_ -lt 0 }).Count
        within_5   = @($abs | Where-Object { $_ -le 5 }).Count
        within_25  = @($abs | Where-Object { $_ -le 25 }).Count
    }
}

function Fmt([double]$v) { if ([double]::IsNaN($v)) { 'n/a' } else { '{0:+0.0;-0.0;0.0}' -f $v } }

function Write-Run([object]$Run, [string]$Title) {
    $patched = $Run.Rows
    $bracket = @($patched | Where-Object { -not $_.dual_fit })
    $dual = @($patched | Where-Object { $_.dual_fit })
    $bVals = [double[]]@($bracket | Where-Object { $null -ne $_.residual_ms } | ForEach-Object residual_ms)
    $dVals = [double[]]@($dual | Where-Object { $null -ne $_.residual_ms } | ForEach-Object residual_ms)

    Write-Host ""
    Write-Host "== $Title  ($($Run.Dir))"
    Write-Host ("pairs read {0}, unreadable/in-progress skipped {1}; patched gaps {2} (bracket {3}, dual-fit {4})" -f `
        $Run.Pairs, $Run.Skipped, $patched.Count, $bracket.Count, $dual.Count)

    foreach ($set in @(@('bracket (α population)', $bVals, $bracket.Count), @('dual-fit (applied = 0.0)', $dVals, $dual.Count))) {
        $name, $vals, $total = $set
        $st = Get-Stats $vals
        Write-Host ("  {0}: residual present {1}/{2}" -f $name, $st.n, $total)
        if ($st.n -eq 0) { continue }
        Write-Host ("    median {0} ms  (IQR {1} … {2}), mean {3}, median |r| {4:0.0} ms" -f `
            (Fmt $st.median), (Fmt $st.q1), (Fmt $st.q3), (Fmt $st.mean), $st.abs_median)
        Write-Host ("    late (>0) {0}, early (<0) {1};  |r| <= 5 ms: {2}, <= 25 ms: {3}" -f `
            $st.late, $st.early, $st.within_5, $st.within_25)
    }

    if ($bVals.Count -gt 0) {
        Write-Host "  bracket histogram (ms, bin $BinMs):"
        $under = @($bVals | Where-Object { $_ -lt -$RangeMs }).Count
        $over = @($bVals | Where-Object { $_ -ge $RangeMs }).Count
        $peak = 1
        $bins = [ordered]@{}
        for ($lo = -$RangeMs; $lo -lt $RangeMs; $lo += $BinMs) {
            $hi = $lo + $BinMs
            $c = @($bVals | Where-Object { $_ -ge $lo -and $_ -lt $hi }).Count
            $bins["$lo"] = $c
            if ($c -gt $peak) { $peak = $c }
        }
        if ($under) { Write-Host ("    {0,9}  {1,4}" -f "< $(-$RangeMs)", $under) }
        foreach ($k in $bins.Keys) {
            $c = $bins[$k]
            if ($c -eq 0) { continue }
            $bar = '#' * [math]::Max(1, [math]::Round(40.0 * $c / $peak))
            Write-Host ("    {0,9}  {1,4}  {2}" -f ('{0:+0;-0;0}' -f [double]$k), $c, $bar)
        }
        if ($over) { Write-Host ("    {0,9}  {1,4}" -f ">= $RangeMs", $over) }
    }
}

$run = Read-Run $DumpDir
Write-Run $run 'run'

if ($BaselineDir) {
    $base = Read-Run $BaselineDir
    Write-Run $base 'baseline'

    $key = { param($r) '{0}|{1:0.000}' -f $r.pair, $r.a_start_secs }
    $baseMap = @{}
    foreach ($r in $base.Rows) { if (-not $r.dual_fit -and $null -ne $r.residual_ms) { $baseMap[(& $key $r)] = $r } }
    $deltas = [System.Collections.Generic.List[double]]::new()
    foreach ($r in $run.Rows) {
        if ($r.dual_fit -or $null -eq $r.residual_ms) { continue }
        $b = $baseMap[(& $key $r)]
        if ($b) { $deltas.Add($r.residual_ms - $b.residual_ms) }
    }
    Write-Host ""
    Write-Host "== paired (bracket in both, matched on pair + A start): n = $($deltas.Count)"
    if ($deltas.Count) {
        $st = Get-Stats $deltas.ToArray()
        Write-Host ("    run − baseline: median {0} ms (IQR {1} … {2})" -f (Fmt $st.median), (Fmt $st.q1), (Fmt $st.q3))
    }
}

if ($CsvOut) {
    $run.Rows | Select-Object pair, gap_index, a_start_secs, dual_fit, confidence, align_adj_ms, residual_ms |
        Export-Csv -LiteralPath $CsvOut -NoTypeInformation -Encoding utf8
    Write-Host ""
    Write-Host "wrote $CsvOut"
}
