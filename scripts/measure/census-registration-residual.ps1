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
# SEAM CENSUS (Phase 1 of the dual-fit registration routing plan). When the reports carry the
# per-seam fields (`registration_residual_pre_ms` / `_post_ms`, added 2026-09-22), the script also
# sizes the routing cohort: `max_seam_err = max(|pre|, |post|)` per gap, split by kind
# (step-kind when |pre - post| > -StepKindMs, common-mode otherwise), with routed counts at each
# -RouteBoundsMs value. Dual-fit gaps are EXCLUDED from that census: `applied_ms` is hardcoded 0.0
# on that arm, so its residual is minus the displacement from nominal, not a seam error. The sign of
# merit is inverted between the arms; see the plan's Phase 0 section.
#
# DUAL-FIT DELTA (Phase 1c item 3). With -FingerprintDir, the patched gaps are joined to a gap
# fingerprint dump on pair label + A start to the millisecond, and the per-shoulder delta is printed:
# `delta_s = dual_fit_placement.<s>_lag_ms - registration_applied_ms`, i.e. how far dual-fit would
# move each shoulder from where the keep path actually put the fill. Both terms are PLANNED-frame, so
# the crossfade cancels and -FrameOffsetMs is NOT applied to them. Reported beside
# `delta_s + registration_residual_<s>_ms` (also planned-frame), which is ~0 exactly when dual-fit
# lands where the keep path's own residual says it should have. Requires a dump written on or after
# 2026-09-23; older ones carry no `dual_fit_placement` and the section says so and reports nothing.
#
# This SIZES a cohort, it does not decide one. No routing predicate lives here: the script reports
# how many gaps sit above each pre-registered bound and names them, and the bound itself is settled
# by the ear pass.
#
# FRAME. Report values describe the PLANNED placement; the written fill sits one seam crossfade
# width ahead of it (apply_seam_crossfade starts the gap interior at b_fill[cf]), and the report
# carries no crossfade term. The seam census is printed in the WRITTEN frame (report +
# -FrameOffsetMs, default 10.0 = the crossfade_ms default at 48 kHz), which is what a listener
# hears and the frame the plan quotes bounds in. Pass -FrameOffsetMs to match a run made at a
# different --crossfade-ms.
#
# Usage:
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<date>-corpus-patch
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<new> -BaselineDir gap-files/<old>
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<new> -BinMs 10 -RangeMs 100
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<new> -CsvOut gap-files/residual.csv
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<new> -ExcludePairs 6,16
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<new> -RouteBoundsMs 50,75,100
#   ./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/<patch> -FingerprintDir gap-files/<fp>
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
    [string]$CsvOut = '',

    # Pair labels dropped entirely (duplicate media: 6 = 22 and 16 = 28 on the current corpus).
    [int[]]$ExcludePairs = @(),

    # Candidate routing bounds for the seam census, in the written frame. Pre-registered: 50, 75, 100.
    [double[]]$RouteBoundsMs = @(50.0, 75.0, 100.0),

    # written frame = planned (report) frame + this. The seam crossfade width: crossfade_ms at the
    # run's sample rate, 10 ms = 480 frames at 48 kHz on the shipped default.
    [double]$FrameOffsetMs = 10.0,

    # |pre - post| above this is step-kind; at or below it is common-mode.
    [double]$StepKindMs = 20.0,

    # Gaps above this bound are listed individually by pair/gap index.
    [double]$ListAboveMs = 75.0,

    # Optional gap-fingerprint dump joined to -DumpDir for the per-shoulder dual-fit delta.
    # Layout is census-skip-buckets.ps1's: <dir>/<pair>/corpus.json, per-gap *_full.json otherwise.
    [string]$FingerprintDir = ''
)

$ErrorActionPreference = 'Stop'

function Read-Run([string]$Dir) {
    if (-not (Test-Path -LiteralPath $Dir)) { throw "dump dir not found: $Dir" }
    $rows = [System.Collections.Generic.List[object]]::new()
    $pairs = 0; $skipped = 0; $excluded = 0
    $files = Get-ChildItem -LiteralPath $Dir -Filter '*.json' -File |
        Sort-Object { [int]($_.BaseName -replace '\D', '0') }, BaseName
    foreach ($f in $files) {
        if ($ExcludePairs.Count -and ($f.BaseName -match '^\d+$') -and ([int]$f.BaseName -in $ExcludePairs)) { $excluded++; continue }
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
                pre_ms        = if ($null -ne $p.registration_residual_pre_ms) { [double]$p.registration_residual_pre_ms + $FrameOffsetMs } else { $null }
                post_ms       = if ($null -ne $p.registration_residual_post_ms) { [double]$p.registration_residual_post_ms + $FrameOffsetMs } else { $null }
                # The same two, unshifted. The dual-fit delta is planned-frame on both sides, so it
                # must not inherit the crossfade the seam census adds for the ear.
                pre_raw_ms    = if ($null -ne $p.registration_residual_pre_ms) { [double]$p.registration_residual_pre_ms } else { $null }
                post_raw_ms   = if ($null -ne $p.registration_residual_post_ms) { [double]$p.registration_residual_post_ms } else { $null }
                applied_ms    = if ($null -ne $p.registration_applied_ms) { [double]$p.registration_applied_ms } else { $null }
            })
        }
    }
    [pscustomobject]@{ Dir = $Dir; Pairs = $pairs; Skipped = $skipped; Excluded = $excluded; Rows = $rows }
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
    Write-Host ("pairs read {0}, excluded {1}, unreadable/in-progress skipped {2}; patched gaps {3} (bracket {4}, dual-fit {5})" -f `
        $Run.Pairs, $Run.Excluded, $Run.Skipped, $patched.Count, $bracket.Count, $dual.Count)

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

# Phase 1's seam census: max_seam_err = max(|pre|, |post|) on BRACKET gaps only, in the written frame.
# Sizes the routing cohort at each candidate bound. Decides nothing.
function Write-SeamCensus([object]$Run) {
    $bracket = @($Run.Rows | Where-Object { -not $_.dual_fit })
    $seam = @($bracket | Where-Object { $null -ne $_.pre_ms -and $null -ne $_.post_ms } | ForEach-Object {
        $step = $_.pre_ms - $_.post_ms
        [pscustomobject]@{
            pair    = $_.pair
            gap     = $_.gap_index
            pre_ms  = $_.pre_ms
            post_ms = $_.post_ms
            max_err = [math]::Max([math]::Abs($_.pre_ms), [math]::Abs($_.post_ms))
            step_ms = $step
            kind    = if ([math]::Abs($step) -gt $StepKindMs) { 'step' } else { 'common' }
        }
    })

    Write-Host ""
    Write-Host "== seam census (written frame = report + $FrameOffsetMs ms crossfade; bracket gaps only)"
    if ($seam.Count -eq 0) {
        Write-Host "  no per-seam fields in this run - it predates registration_residual_pre_ms/_post_ms."
        return
    }
    $missing = $bracket.Count - $seam.Count
    Write-Host ("  bracket gaps {0} over {1} pair(s); missing a per-seam value {2}; dual-fit excluded {3}" -f `
        $seam.Count, @($seam | Group-Object pair).Count, $missing, @($Run.Rows | Where-Object { $_.dual_fit }).Count)

    foreach ($set in @(@('all', $seam), @('step-kind', @($seam | Where-Object kind -eq 'step')), @('common-mode', @($seam | Where-Object kind -eq 'common')))) {
        $name, $sel = $set
        if ($sel.Count -eq 0) { Write-Host ("  {0,-12} n = 0" -f $name); continue }
        $v = [double[]]@($sel | ForEach-Object max_err | Sort-Object)
        Write-Host ("  {0,-12} n {1,4}   median {2,6:0.0}   p75 {3,6:0.0}   p90 {4,6:0.0}   p95 {5,6:0.0}   max {6,7:0.0}" -f `
            $name, $v.Count, (Get-Quantile $v 0.5), (Get-Quantile $v 0.75), (Get-Quantile $v 0.90), (Get-Quantile $v 0.95), $v[-1])
    }

    $late = @($seam | Where-Object { $_.pre_ms -gt 0 -and $_.post_ms -gt 0 }).Count
    $early = @($seam | Where-Object { $_.pre_ms -lt 0 -and $_.post_ms -lt 0 }).Count
    Write-Host ("  direction: late of both {0}, early of both {1}, opposite signs {2}; step-kind gaps {3}" -f `
        $late, $early, ($seam.Count - $late - $early), @($seam | Where-Object kind -eq 'step').Count)

    Write-Host ""
    Write-Host "  routed count by candidate bound (sizing only - the bound is settled by ear):"
    Write-Host ("    {0,-10} {1,6} {2,7} {3,7} {4,8}" -f 'bound', 'gaps', 'pairs', 'step', 'common')
    foreach ($b in ($RouteBoundsMs | Sort-Object)) {
        $sel = @($seam | Where-Object { $_.max_err -gt $b })
        Write-Host ("    > {0,-8:0} {1,6} {2,7} {3,7} {4,8}" -f $b, $sel.Count, @($sel | Group-Object pair).Count, `
            @($sel | Where-Object kind -eq 'step').Count, @($sel | Where-Object kind -eq 'common').Count)
    }

    $listed = @($seam | Where-Object { $_.max_err -gt $ListAboveMs } | Sort-Object max_err -Descending)
    if ($listed.Count) {
        Write-Host ""
        Write-Host "  gaps above $ListAboveMs ms (pair/gap index only):"
        foreach ($r in $listed) {
            Write-Host ("    {0,-10} {1,-7} pre {2,9:0.0}  post {3,9:0.0}  max {4,8:0.0}  step {5,7:0.0}" -f `
                "$($r.pair)/g$($r.gap)", $r.kind, $r.pre_ms, $r.post_ms, $r.max_err, $r.step_ms)
        }
    }
}

# Phase 1c item 3: dual-fit's placement, joined to the fill the keep path actually wrote.
#
# By JOIN rather than by emitting a keep-path placement into the fingerprint dump: the patch report
# already carries `registration_applied_ms`, and a second copy of it written from a different code
# path is a drift bug waiting to happen. The join key is pair label + A start to the millisecond,
# never the gap index - the two dumps enumerate different gap populations.
function Read-Placements([string]$Dir) {
    if (-not (Test-Path -LiteralPath $Dir)) { throw "fingerprint dir not found: $Dir" }
    $skipNames = @('manifest.json', 'listen.json')
    $candidates = @(Get-ChildItem -LiteralPath $Dir -Recurse -File -Filter '*.json' |
        Where-Object { $skipNames -notcontains $_.Name })
    $pairOf = { param($f) ([System.IO.Path]::GetRelativePath($Dir, $f.FullName) -split '[\\/]')[0] }
    $canonical = @{}
    foreach ($f in $candidates) { if ($f.Name -eq 'corpus.json') { $canonical[(& $pairOf $f)] = $true } }
    $files = @($candidates | Where-Object {
        if ($_.Name -eq 'corpus.json') { return $true }
        return -not $canonical.ContainsKey((& $pairOf $_))
    })

    if ($files.Count -eq 0) { throw "no fingerprint JSON under: $Dir" }

    $map = @{}
    $gaps = 0; $withPlacement = 0
    foreach ($f in $files) {
        try { $doc = Get-Content -LiteralPath $f.FullName -Raw | ConvertFrom-Json -Depth 64 } catch { continue }
        if ($null -eq $doc.PSObject.Properties['gaps']) { continue }
        $pair = & $pairOf $f
        foreach ($g in @($doc.gaps)) {
            $geom = $g.PSObject.Properties['geometry']
            if ($null -eq $geom -or $null -eq $geom.Value) { continue }
            $aStart = $geom.Value.PSObject.Properties['a_start_secs']
            if ($null -eq $aStart) { continue }
            $gaps++
            $place = $g.PSObject.Properties['dual_fit_placement']
            if ($null -eq $place -or $null -eq $place.Value) { continue }
            $withPlacement++
            $map['{0}|{1:0.000}' -f $pair, [double]$aStart.Value] = $place.Value
        }
    }
    [pscustomobject]@{ Dir = $Dir; Map = $map; Gaps = $gaps; WithPlacement = $withPlacement }
}

function Write-DualFitDelta([object]$Run, [object]$Fp) {
    Write-Host ""
    Write-Host "== dual-fit delta (planned frame on BOTH sides - no crossfade term)  ($($Fp.Dir))"
    if ($Fp.WithPlacement -eq 0) {
        Write-Host "  no dual_fit_placement in this dump - it predates 2026-09-23. Nothing to join."
        Write-Host ("  ({0} fingerprint gaps read)" -f $Fp.Gaps)
        return
    }

    $joined = [System.Collections.Generic.List[object]]::new()
    $unjoined = 0
    foreach ($r in $Run.Rows) {
        if ($null -eq $r.applied_ms) { continue }
        $place = $Fp.Map['{0}|{1:0.000}' -f $r.pair, $r.a_start_secs]
        if ($null -eq $place) { $unjoined++; continue }
        $dPre = [double]$place.pre_lag_ms - $r.applied_ms
        $dPost = [double]$place.post_lag_ms - $r.applied_ms
        $joined.Add([pscustomobject]@{
            pair      = $r.pair
            gap       = $r.gap_index
            dual_fit  = $r.dual_fit
            delta_pre = $dPre
            delta_post = $dPost
            # ~0 when dual-fit lands where the keep path's own residual says it should have.
            left_pre  = if ($null -ne $r.pre_raw_ms) { $dPre + $r.pre_raw_ms } else { $null }
            left_post = if ($null -ne $r.post_raw_ms) { $dPost + $r.post_raw_ms } else { $null }
            common_ms = ($dPre + $dPost) / 2.0
            step_ms   = $dPre - $dPost
        })
    }

    Write-Host ("  fingerprint gaps {0} ({1} carry a placement); patched gaps joined {2}, unjoined {3}" -f `
        $Fp.Gaps, $Fp.WithPlacement, $joined.Count, $unjoined)
    if ($joined.Count -eq 0) {
        Write-Host "  nothing joined - check that both dumps cover the same pairs."
        return
    }

    # Bracket gaps are the population the routing question is about: on the dual-fit arm
    # `applied_ms` is hardcoded 0.0, so the delta there is dual-fit against nominal, not against a
    # placement the keep path chose. Tallied, never pooled.
    $bracket = @($joined | Where-Object { -not $_.dual_fit })
    $dual = @($joined | Where-Object { $_.dual_fit })
    Write-Host ("  bracket {0}, dual-fit rescues {1} (reported apart - applied_ms is 0.0 on that arm)" -f `
        $bracket.Count, $dual.Count)

    foreach ($axis in @('delta_pre', 'delta_post', 'common_ms', 'step_ms', 'left_pre', 'left_post')) {
        $v = [double[]]@($bracket | ForEach-Object { $_.$axis } | Where-Object { $null -ne $_ })
        if ($v.Count -eq 0) { Write-Host ("  {0,-10} n = 0" -f $axis); continue }
        $st = Get-Stats $v
        Write-Host ("  {0,-10} n {1,4}   median {2,8}   IQR {3,8} … {4,-8}  median |d| {5,7:0.0}   |d| <= 25 ms {6,4}" -f `
            $axis, $st.n, (Fmt $st.median), (Fmt $st.q1), (Fmt $st.q3), $st.abs_median, $st.within_25)
    }

    $moved = @($bracket | Where-Object { [math]::Max([math]::Abs($_.delta_pre), [math]::Abs($_.delta_post)) -gt $ListAboveMs } |
        Sort-Object { [math]::Max([math]::Abs($_.delta_pre), [math]::Abs($_.delta_post)) } -Descending)
    if ($moved.Count) {
        Write-Host ""
        Write-Host "  bracket gaps dual-fit would move more than $ListAboveMs ms (pair/gap index only):"
        foreach ($r in $moved) {
            Write-Host ("    {0,-10} pre {1,9:0.0}  post {2,9:0.0}  common {3,9:0.0}  step {4,8:0.0}" -f `
                "$($r.pair)/g$($r.gap)", $r.delta_pre, $r.delta_post, $r.common_ms, $r.step_ms)
        }
    }
}

$run = Read-Run $DumpDir
Write-Run $run 'run'
Write-SeamCensus $run

if ($FingerprintDir) { Write-DualFitDelta $run (Read-Placements $FingerprintDir) }

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
    $run.Rows |
        Select-Object pair, gap_index, a_start_secs, dual_fit, confidence, align_adj_ms, residual_ms,
            pre_ms, post_ms, applied_ms,
            @{ n = 'max_seam_err_ms'; e = { if ($null -ne $_.pre_ms -and $null -ne $_.post_ms) { [math]::Max([math]::Abs($_.pre_ms), [math]::Abs($_.post_ms)) } else { $null } } },
            @{ n = 'step_ms'; e = { if ($null -ne $_.pre_ms -and $null -ne $_.post_ms) { $_.pre_ms - $_.post_ms } else { $null } } },
            @{ n = 'kind'; e = { if ($null -eq $_.pre_ms -or $null -eq $_.post_ms) { $null } elseif ([math]::Abs($_.pre_ms - $_.post_ms) -gt $StepKindMs) { 'step' } else { 'common' } } } |
        Export-Csv -LiteralPath $CsvOut -NoTypeInformation -Encoding utf8
    Write-Host ""
    Write-Host "wrote $CsvOut"
}
