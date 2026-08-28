#!/usr/bin/env pwsh
#Requires -Version 7.0
# Route 2 consensus + peak_z re-gate (docs/dev/archive/TEMP-fill-placement-registration-plan.md §0 S2).
#
# Asks one question: does the per-shoulder waveform lag (`lag_decision`, a measurement) agree
# with the slide the repair actually applied (`align_adjustment_secs`, a decision)? Agreement
# says the big slides are registration, not fill-placement error; disagreement names suspects.
#
# JOIN RULE: scan reports x fingerprint dumps on `geometry.a_start_secs`, rounded to 1 ms.
# NEVER on gap index — indices are not stable between a scan and a fingerprint run.
#
# Reduction discipline (plan §0 S2(2)): pre and post `frac_lag_ms` are absolute lags on ONE
# scale, but post is GROSS (`L_pre + L_post_fine`) because its search is centred on
# `S + D_A + round(L_pre)`, so its magnitude guard is ±1200 ms and pre's is ±600
# (docs/dev/gap-fingerprint.md §"lag_decision"). Averaging two shoulders is invalid when the
# step between them is large, so big-step gaps are REPORTED, not reduced — and they are also
# excluded from the pair pool, since a gap whose shoulders disagree by more than -StepMaxMs
# carries no estimate of the pair's offset. Big-step is read from `splice.step_ms`, computed
# from BOTH raw shoulders, so a gap that loses one shoulder to the confidence gate is still
# caught (3/g9 under peak_z: post drops, pre +10 ms alone would otherwise speak for pair 3).
#
# A pair needs at least -MinUnified unified (fit_terms-carrying, non-dual-fit) gaps before its
# consensus is quoted. dual-fit sets `fit_terms: null` and reports adjustment 0.0 by
# construction, so dual gaps never enter a consensus and are pooled separately.
#
# Usage:
#   ./scripts/measure/route2-consensus.ps1 -ScanDir gap-files/<date>-repeat-terms `
#       -DumpDir gap-files/<date>-oriented -ExcludePairs 6,16
#   ... -Gate peak_z -PairTable
#   ... -ReductionRobustness -ShoulderMap -OutOfWindow
#   ... -CsvOut gap-files/route2-consensus.csv
#
# Layouts:
#   -ScanDir  `<ScanDir>/<pair>.json`      with `patch.gaps[*].status.patched`
#             (measure-patch-outcomes.ps1 / `--patch-only --format json`)
#   -DumpDir  `<DumpDir>/<pair>/*_full*.json` with `gaps[*].lag_decision`
#             (measure-gap-fingerprints.ps1). A pair dir is used only when `corpus.json` and
#             `manifest.json` are both present — `write_corpus_dir` writes manifest last, so a
#             dir still receiving `*_full*.json` is skipped rather than joined half-written.
#             A patched scan gap with no dump match aborts (no partial join).
#             Commit 1e4fef21 dropped the verdict token (`_full_patch.json` / `_full_skip.json`
#             -> `_full.json`); both are accepted so pre-1e4fef21 archives can calibrate this code.
#   Oriented corpus rates (header targets below) require `-ExcludePairs 6,16` (6≡22, 16≡28,
#   same media). The script warns if analysed pairs share `manifest.json` a_id/b_id.
#
# Calibration targets (do not trust a change to this script until it reproduces them):
#   archive mixed-ab (2026-08-12-fit-terms-baseline x 2026-08-08-v0.5.2-full-corpus):
#     peak_r  in-window 26/26 agree, load-bearing cell 6/6 = {3, 6, 13, 18, 22, 32}
#     peak_z  shoulders 432 -> 270 kept, no-usable-shoulder gaps 4 -> 58, bias -24.8 ms
#   oriented corpus, -ExcludePairs 6,16:
#     peak_r  34/34, cell 9/9 = {2, 3, 13, 14, 18, 22, 32, 47, 51}, bias -21.9 ms
#     peak_z  30/31, cell 8/8 = {2, 3, 13, 14, 18, 32, 47, 51},     bias -26.1 ms
#
# MEDIA HYGIENE: emit pair label + gap index only — never media paths or titles.
# Docs: docs/dev/archive/TEMP-fill-placement-registration-plan.md §0.0, §0 S2.

[CmdletBinding()]
param(
    # Scan / patch reports: <ScanDir>/<pair>.json.
    [Parameter(Mandatory = $true)]
    [string]$ScanDir,

    # Fingerprint dumps: <DumpDir>/<pair>/*_full*.json.
    [Parameter(Mandatory = $true)]
    [string]$DumpDir,

    # Shoulder confidence gate. 'both' runs peak_r then peak_z.
    [ValidateSet('peak_r', 'peak_z', 'both')]
    [string]$Gate = 'both',

    # How a gap's two shoulders become one lag. 'mean' refuses to average across a big step.
    [ValidateSet('mean', 'pre', 'minabs', 'bestr')]
    [string]$Reduction = 'mean',

    # Restrict to these pair labels (default: every pair present in both dirs).
    [int[]]$Pairs = @(),

    # Drop these pair labels (duplicate media, swapped orientation, …).
    [int[]]$ExcludePairs = @(),

    # Minimum unified gaps before a pair's consensus is quoted.
    [int]$MinUnified = 3,

    # Above this |post - pre| the two shoulders are not reduced together and the gap is
    # excluded from its pair's pool.
    [double]$StepMaxMs = 100.0,

    # Pair-level |pooled - consensus| that counts as agreement.
    [double]$AgreeMs = 50.0,

    # |consensus| at or above this makes a pair load-bearing; |adj| above this makes a gap a
    # big-slide gap (the decisive cell).
    [double]$LoadBearingMs = 150.0,

    # |consensus| within this is inside the ±600 ms sweep, so the measurement can see it.
    [double]$InWindowMs = 600.0,

    # Per-gap |adj - lag| bands: <= confirmed, <= near, above = contradicted / suspect.
    [double]$ConfirmMs = 50.0,
    [double]$NearMs = 100.0,

    # Per-pair consensus/pooled/delta table.
    [switch]$PairTable,

    # Pairs whose consensus is outside ±InWindowMs (the pair-31 class).
    [switch]$OutOfWindow,

    # Per-pair map of gaps with no usable shoulder, in gap order (X = none).
    [switch]$ShoulderMap,

    # Re-run every reduction and compare — the cell must not depend on the reduction choice.
    [switch]$ReductionRobustness,

    # Cap on the suspect list printed to the console (CSV always carries every gap).
    [int]$SuspectLimit = 12,

    # Per-gap CSV: pair, gap_index, adj_ms, lag_ms, residual_ms, shoulders, …
    [string]$CsvOut = ''
)

$ErrorActionPreference = 'Stop'

# Magnitude guards. Pre is fine (±600 ms around mapped nominal); post is gross, so its lag is
# pre + post_fine and it needs the wider guard (gap-fingerprint.md).
$PreGuardMs = 600.0
$PostGuardMs = 1200.0

foreach ($d in @($ScanDir, $DumpDir)) {
    if (-not (Test-Path -LiteralPath $d)) { throw "directory not found: $d" }
}

function Get-Median([object]$Values) {
    $xs = @($Values | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ } | Sort-Object)
    if ($xs.Count -eq 0) { return $null }
    # [int] rounds half to even — floor, or an odd count lands one slot past the median.
    $mid = [int][math]::Floor($xs.Count / 2)
    if ($xs.Count % 2 -eq 1) { return $xs[$mid] }
    return ($xs[$mid - 1] + $xs[$mid]) / 2.0
}

function Get-StartKey([object]$Secs) {
    # 1 ms resolution, culture-invariant, so scan and dump agree on the join key.
    return ([math]::Round([double]$Secs, 3)).ToString('F3', [cultureinfo]::InvariantCulture)
}

function Test-HasProp([object]$Obj, [string]$Name) {
    if ($null -eq $Obj) { return $false }
    return $null -ne $Obj.PSObject.Properties[$Name]
}

function Get-ScanIndex([string]$Dir) {
    # label -> { start_key -> @{ Idx; AdjMs; Dual; Unified } } over patched gaps only.
    $out = @{}
    Get-ChildItem -LiteralPath $Dir -File -Filter '*.json' -ErrorAction SilentlyContinue |
        Where-Object { $_.BaseName -match '^\d+$' } |
        ForEach-Object {
            $label = [int]$_.BaseName
            $root = Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json
            $byStart = @{}
            $i = 0
            foreach ($g in @($root.patch.gaps)) {
                if ($null -eq $g) { continue }
                $s = $g.status.patched
                if ($null -eq $s) { $i++; continue }
                $adj = 0.0
                if ($null -ne $s.align_adjustment_secs) { $adj = [double]$s.align_adjustment_secs }
                $byStart[(Get-StartKey $g.a_start_secs)] = @{
                    Idx     = $i
                    AdjMs   = $adj * 1000.0
                    Dual    = [bool]$s.dual_fit_used
                    Unified = ((Test-HasProp $s 'fit_terms') -and $null -ne $s.fit_terms)
                }
                $i++
            }
            if ($byStart.Count -gt 0) { $out[$label] = $byStart }
        }
    return $out
}

function Get-DumpIndex([string]$Dir) {
    # Complete pair dirs only (`manifest.json` is write_corpus_dir's last file).
    # .Pairs       label -> { start_key -> @{ Pre; Post; StepMs; SpliceEdge } }
    # .Incomplete  numeric labels whose dir exists but is still writing
    # .Identity    label -> @{ AId; BId } from manifest.json
    $out = @{}
    $incomplete = [System.Collections.Generic.List[int]]::new()
    $identity = @{}
    Get-ChildItem -LiteralPath $Dir -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^\d+$' } |
        ForEach-Object {
            $label = [int]$_.Name
            $pairDir = $_.FullName
            $corpusPath = Join-Path $pairDir 'corpus.json'
            $manifestPath = Join-Path $pairDir 'manifest.json'
            if (-not (Test-Path -LiteralPath $corpusPath) -or -not (Test-Path -LiteralPath $manifestPath)) {
                $incomplete.Add($label)
                return
            }
            $man = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
            if ((Test-HasProp $man 'a_id') -and (Test-HasProp $man 'b_id') -and
                $null -ne $man.a_id -and $null -ne $man.b_id) {
                $identity[$label] = @{ AId = [string]$man.a_id; BId = [string]$man.b_id }
            }
            $byStart = @{}
            Get-ChildItem -LiteralPath $pairDir -Recurse -File -Filter '*.json' -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -like '*_full*' } |
                Where-Object { $_.Name -notmatch '^(corpus|manifest|listen)\.json$' } |
                ForEach-Object {
                    $root = Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json
                    foreach ($g in @($root.gaps)) {
                        if ($null -eq $g) { continue }
                        $geo = $g.geometry
                        $ld = $g.lag_decision
                        if ($null -eq $geo -or $null -eq $ld) { continue }
                        if (-not (Test-HasProp $geo 'a_start_secs')) { continue }
                        $pre = @($ld.pre_anchor)  | Select-Object -First 1
                        $post = @($ld.post_anchor) | Select-Object -First 1
                        $byStart[(Get-StartKey $geo.a_start_secs)] = @{
                            Pre        = $pre
                            Post       = $post
                            StepMs     = $(if ($null -ne $g.splice -and $null -ne $g.splice.step_ms) { [double]$g.splice.step_ms } else { $null })
                            SpliceEdge = [bool]($g.splice.edge_pinned)
                        }
                    }
                }
            if ($byStart.Count -gt 0) { $out[$label] = $byStart }
        }
    return [pscustomobject]@{
        Pairs      = $out
        Incomplete = @($incomplete | Sort-Object)
        Identity   = $identity
    }
}

function Get-ConfidentShoulder([object]$Shoulder, [string]$Side, [string]$Gate) {
    # @{ Lag; R; Z } if this shoulder is usable under $Gate, else $null.
    if ($null -eq $Shoulder) { return $null }
    $lag = $Shoulder.frac_lag_ms
    $r = $Shoulder.peak_r
    $z = $Shoulder.peak_z
    if ($null -eq $lag -or $null -eq $r -or $null -eq $z) { return $null }
    if ([bool]$Shoulder.edge_pinned) { return $null }
    $guard = $(if ($Side -eq 'pre') { $PreGuardMs } else { $PostGuardMs })
    if ([math]::Abs([double]$lag) -gt $guard) { return $null }
    if ($Gate -eq 'peak_r') { if ([double]$r -lt 0.5) { return $null } }
    elseif ($Gate -eq 'peak_z') { if ([double]$z -lt 12.0) { return $null } }
    else { throw "unknown gate: $Gate" }
    return @{ Lag = [double]$lag; R = [double]$r; Z = [double]$z }
}

function Get-GapShoulders([hashtable]$Entry, [string]$Gate) {
    $out = @{}
    foreach ($side in @('pre', 'post')) {
        $raw = $(if ($side -eq 'pre') { $Entry.Pre } else { $Entry.Post })
        $c = Get-ConfidentShoulder -Shoulder $raw -Side $side -Gate $Gate
        if ($null -ne $c) { $out[$side] = $c }
    }
    return $out
}

function Test-BigStep([hashtable]$Entry, [hashtable]$Sh) {
    # `splice.step_ms` is post - pre over BOTH raw shoulders, so it survives one shoulder being
    # dropped by the gate. An edge-pinned splice makes the step GIGO — reportable, not poolable.
    $step = $Entry.StepMs
    if ($null -eq $step) {
        if ($Sh.Count -eq 2) { $step = $Sh['post'].Lag - $Sh['pre'].Lag }
        else { return $false }
    }
    if ($Entry.SpliceEdge) { return $true }
    return [math]::Abs([double]$step) -gt $StepMaxMs
}

function Get-ReducedLag([hashtable]$Sh, [string]$Reduction) {
    if ($Sh.Count -eq 0) { return $null }
    $pre = $Sh['pre']
    $post = $Sh['post']
    switch ($Reduction) {
        'pre' {
            if ($null -ne $pre) { return $pre.Lag }
            return $null
        }
        'minabs' {
            $best = $null
            foreach ($v in $Sh.Values) {
                if ($null -eq $best -or [math]::Abs($v.Lag) -lt [math]::Abs($best)) { $best = $v.Lag }
            }
            return $best
        }
        'bestr' {
            $best = $null
            $bestR = [double]::NegativeInfinity
            foreach ($v in $Sh.Values) {
                if ($v.R -gt $bestR) { $bestR = $v.R; $best = $v.Lag }
            }
            return $best
        }
        default {
            # mean: refuse to average across a large step.
            if ($null -ne $pre -and $null -ne $post) {
                if ([math]::Abs($post.Lag - $pre.Lag) -gt $StepMaxMs) { return $null }
                return ($pre.Lag + $post.Lag) / 2.0
            }
            if ($null -ne $pre) { return $pre.Lag }
            return $post.Lag
        }
    }
}

function Invoke-Route2 {
    param(
        [hashtable]$Scan,
        [hashtable]$Dumps,
        [string]$Gate,
        [string]$Reduction,
        [int[]]$Restrict
    )

    $pairs = @($Restrict | Sort-Object)
    $joined = 0
    $missed = 0
    $missedKeys = [System.Collections.Generic.List[string]]::new()
    # Plain hashtable, not [ordered]: OrderedDictionary treats an [int] key as a POSITIONAL
    # index, and pair labels are ints. Callers sort .Keys.
    $perPair = @{}
    $gapRows = [System.Collections.Generic.List[object]]::new()
    $shouldersSeen = 0
    $shouldersKept = 0
    $noShoulder = 0

    foreach ($pr in $pairs) {
        $pool = [System.Collections.Generic.List[double]]::new()
        $cons = [System.Collections.Generic.List[double]]::new()
        $dualLags = [System.Collections.Generic.List[double]]::new()

        foreach ($start in ($Scan[$pr].Keys | Sort-Object { [double]$_ })) {
            $sg = $Scan[$pr][$start]
            $d = $Dumps[$pr][$start]
            if ($null -eq $d) {
                $missed++
                $missedKeys.Add(('{0}/{1}' -f $pr, $start))
                continue
            }
            $joined++
            foreach ($raw in @($d.Pre, $d.Post)) { if ($null -ne $raw) { $shouldersSeen++ } }

            $sh = Get-GapShoulders -Entry $d -Gate $Gate
            $shouldersKept += $sh.Count
            if ($sh.Count -eq 0) { $noShoulder++ }
            if ($sg.Unified) { $cons.Add([double]$sg.AdjMs) }

            $bigStep = Test-BigStep -Entry $d -Sh $sh
            if (-not $bigStep) {
                foreach ($v in $sh.Values) {
                    if ($sg.Dual) { $dualLags.Add($v.Lag) } else { $pool.Add($v.Lag) }
                }
            }

            $own = Get-ReducedLag -Sh $sh -Reduction $Reduction
            $gapRows.Add([pscustomobject]@{
                    Pair       = $pr
                    Idx        = $sg.Idx
                    StartKey   = $start
                    AdjMs      = [double]$sg.AdjMs
                    LagMs      = $own
                    Dual       = [bool]$sg.Dual
                    Unified    = [bool]$sg.Unified
                    BigStep    = [bool]$bigStep
                    Shoulders  = $sh.Count
                    PreLagMs   = $(if ($sh['pre']) { $sh['pre'].Lag } else { $null })
                    PreR       = $(if ($sh['pre']) { $sh['pre'].R } else { $null })
                    PreZ       = $(if ($sh['pre']) { $sh['pre'].Z } else { $null })
                    PostLagMs  = $(if ($sh['post']) { $sh['post'].Lag } else { $null })
                    PostR      = $(if ($sh['post']) { $sh['post'].R } else { $null })
                    PostZ      = $(if ($sh['post']) { $sh['post'].Z } else { $null })
                    StepMs     = $d.StepMs
                    SpliceEdge = [bool]$d.SpliceEdge
                })
        }

        $perPair[$pr] = @{
            Consensus  = $(if ($cons.Count -ge $MinUnified) { Get-Median $cons } else { $null })
            Pooled     = Get-Median $pool
            DualPooled = Get-Median $dualLags
            NUnified   = $cons.Count
        }
    }

    if ($missed -gt 0) {
        $shown = @($missedKeys | Select-Object -First 8)
        $more = $(if ($missedKeys.Count -gt 8) { '…' } else { '' })
        throw ("{0} patched scan gap(s) have no dump match on a_start_secs (1 ms join). Partial join refused. e.g. {1}{2}" -f `
                $missed, ($shown -join ', '), $more)
    }

    # --- A. pair-level agreement ---
    $rows = [System.Collections.Generic.List[object]]::new()
    foreach ($pr in ($perPair.Keys | Sort-Object)) {
        $v = $perPair[$pr]
        if ($null -eq $v.Consensus -or $null -eq $v.Pooled) { continue }
        $rows.Add([pscustomobject]@{
                Pair        = $pr
                Consensus   = [double]$v.Consensus
                Pooled      = [double]$v.Pooled
                Delta       = [double]$v.Pooled - [double]$v.Consensus
                LoadBearing = [math]::Abs([double]$v.Consensus) -ge $LoadBearingMs
                InWindow    = [math]::Abs([double]$v.Consensus) -le $InWindowMs
            })
    }
    $inWin = @($rows | Where-Object { $_.InWindow })
    $agree = @($inWin | Where-Object { [math]::Abs($_.Delta) -le $AgreeMs })
    $lb = @($inWin | Where-Object { $_.LoadBearing })
    $lbAgree = @($lb | Where-Object { [math]::Abs($_.Delta) -le $AgreeMs })

    # --- B/C/D. per-gap residual ---
    $withLag = @($gapRows | Where-Object { $null -ne $_.LagMs -and -not $_.Dual })
    $big = @($withLag | Where-Object { [math]::Abs($_.AdjMs) -gt $LoadBearingMs })
    $resOf = { param($g) [math]::Abs($g.AdjMs - $g.LagMs) }

    $confirmed = @($big | Where-Object { (& $resOf $_) -le $ConfirmMs }).Count
    $near = @($big | Where-Object { (& $resOf $_) -gt $ConfirmMs -and (& $resOf $_) -le $NearMs }).Count
    $contra = @($big | Where-Object { (& $resOf $_) -gt $NearMs })
    $suspects = @($withLag | Where-Object { (& $resOf $_) -gt $NearMs } |
            Sort-Object -Property @{ Expression = { [math]::Abs($_.AdjMs - $_.LagMs) }; Descending = $true })

    return @{
        Gate          = $Gate
        Reduction     = $Reduction
        Pairs         = $pairs
        Joined        = $joined
        Missed        = $missed
        PerPair       = $perPair
        Rows          = $rows
        InWindow      = $inWin
        Agree         = $agree
        LoadBearing   = $lb
        LbAgree       = $lbAgree
        GapRows       = $gapRows
        WithLag       = $withLag
        Big           = $big
        Confirmed     = $confirmed
        Near          = $near
        Contradicted  = $contra
        Suspects      = $suspects
        Bias          = @($inWin | ForEach-Object { $_.Delta })
        ShouldersSeen = $shouldersSeen
        ShouldersKept = $shouldersKept
        NoShoulder    = $noShoulder
    }
}

function Show-Route2([hashtable]$R) {
    $medAdj = Get-Median @($R.WithLag | ForEach-Object { [math]::Abs($_.AdjMs) })
    $medRes = Get-Median @($R.WithLag | ForEach-Object { [math]::Abs($_.AdjMs - $_.LagMs) })
    $bigAdj = Get-Median @($R.Big | ForEach-Object { [math]::Abs($_.AdjMs) })
    $bigRes = Get-Median @($R.Big | ForEach-Object { [math]::Abs($_.AdjMs - $_.LagMs) })

    Write-Host ''
    Write-Host ("===== gate={0}  reduction={1} =====" -f $R.Gate, $R.Reduction) -ForegroundColor Green
    Write-Host ("pairs {0}   joined gaps {1}   unjoined {2}" -f $R.Pairs.Count, $R.Joined, $R.Missed)
    Write-Host ("shoulders {0}/{1} kept   gaps with no usable shoulder {2}/{3}" -f `
            $R.ShouldersKept, $R.ShouldersSeen, $R.NoShoulder, $R.Joined)
    Write-Host ("A. in-window pairs {0}/{1} agree within {2:N0} ms | load-bearing (|consensus|>={3:N0}) {4}/{5}  {6}" -f `
            $R.Agree.Count, $R.InWindow.Count, $AgreeMs, $LoadBearingMs, $R.LbAgree.Count, $R.LoadBearing.Count,
        ('{' + (@($R.LoadBearing | ForEach-Object { $_.Pair } | Sort-Object) -join ', ') + '}'))
    if ($null -ne $medAdj) {
        Write-Host ("B. all n={0}  median |adj| {1:N4} s -> median |adj-lag| {2:N4} s" -f `
                $R.WithLag.Count, ($medAdj / 1000), ($medRes / 1000))
    }
    if ($null -ne $bigAdj) {
        $ratio = $(if ($bigRes -gt 0) { $bigAdj / $bigRes } else { [double]::NaN })
        Write-Host ("   big-slide n={0}  median |adj| {1:N4} s -> {2:N4} s   ({3:N1}x)" -f `
                $R.Big.Count, ($bigAdj / 1000), ($bigRes / 1000), $ratio)
    }
    Write-Host ("C. decisive cell {0} confirmed / {1} near / {2} contradicted  (n={3})" -f `
            $R.Confirmed, $R.Near, $R.Contradicted.Count, $R.Big.Count)
    $bias = Get-Median $R.Bias
    if ($null -ne $bias) {
        $neg = @($R.Bias | Where-Object { $_ -lt 0 }).Count
        Write-Host ("E. signed bias median {0:+0.0;-0.0} ms   negative on {1}/{2} pairs" -f `
                $bias, $neg, $R.Bias.Count)
    }
    if ($R.Suspects.Count -gt 0) {
        $shown = @($R.Suspects | Select-Object -First $SuspectLimit | ForEach-Object {
                '{0}/g{1} adj {2:+0.0;-0.0} lag {3:+0.0;-0.0} d {4:+0.0;-0.0}' -f `
                    $_.Pair, $_.Idx, $_.AdjMs, $_.LagMs, ($_.AdjMs - $_.LagMs)
            })
        Write-Host ("D. suspects ({0}): {1}" -f $R.Suspects.Count, ($shown -join ', ')) -ForegroundColor Yellow
    }
    else {
        Write-Host 'D. suspects: none'
    }
}

function Show-PairTable([hashtable]$R, [hashtable]$Scan) {
    Write-Host ''
    Write-Host ("Per-pair table [{0}]" -f $R.Gate) -ForegroundColor Cyan
    '{0,5} {1,13} {2,11} {3,9} {4,10} {5,10} {6,7}' -f `
        'pair', 'consensus_ms', 'pooled_ms', 'delta', 'n_unified', 'n_patched', 'n_dual' | Write-Host
    Write-Host ('-' * 72)
    foreach ($pr in ($R.PerPair.Keys | Sort-Object)) {
        $v = $R.PerPair[$pr]
        $nPatched = $Scan[$pr].Count
        $nDual = @($Scan[$pr].Values | Where-Object { $_.Dual }).Count
        $cs = $(if ($null -ne $v.Consensus) { '{0:N1}' -f $v.Consensus } else { '-' })
        $ms = $(if ($null -ne $v.Pooled) { '{0:N1}' -f $v.Pooled } else { '-' })
        $ds = $(if ($null -ne $v.Consensus -and $null -ne $v.Pooled) { '{0:N1}' -f ($v.Pooled - $v.Consensus) } else { '-' })
        $star = $(if ($null -ne $v.Consensus -and [math]::Abs($v.Consensus) -ge $LoadBearingMs) { ' <<' } else { '' })
        Write-Host (('{0,5} {1,13} {2,11} {3,9} {4,10} {5,10} {6,7}' -f `
                    $pr, $cs, $ms, $ds, $v.NUnified, $nPatched, $nDual) + $star)
    }
}

# ---------------------------------------------------------------------------

$scanFull = (Resolve-Path -LiteralPath $ScanDir).Path
$dumpFull = (Resolve-Path -LiteralPath $DumpDir).Path
$scan = Get-ScanIndex -Dir $scanFull
$dumpIx = Get-DumpIndex -Dir $dumpFull
$dumps = $dumpIx.Pairs
if ($scan.Count -eq 0) { throw "no <pair>.json scan reports with patched gaps under $scanFull" }
if ($dumps.Count -eq 0) { throw "no complete <pair>/ dumps with lag_decision under $dumpFull (need corpus.json + manifest.json)" }
if ($dumpIx.Incomplete.Count -gt 0) {
    Write-Warning ("skipping pair dir(s) without corpus.json+manifest.json (dump still writing): {0}" -f ($dumpIx.Incomplete -join ', '))
}
if ($Pairs.Count -gt 0) {
    $notReady = @($Pairs | Where-Object { $dumpIx.Incomplete -contains $_ })
    if ($notReady.Count -gt 0) {
        throw ("requested -Pairs not complete (no corpus.json+manifest.json): {0}" -f ($notReady -join ', '))
    }
}

$restrict = @($scan.Keys | Where-Object { $dumps.ContainsKey($_) })
if ($Pairs.Count -gt 0) { $restrict = @($restrict | Where-Object { $Pairs -contains $_ }) }
if ($ExcludePairs.Count -gt 0) { $restrict = @($restrict | Where-Object { $ExcludePairs -notcontains $_ }) }
$restrict = @($restrict | Sort-Object)
if ($restrict.Count -eq 0) { throw 'no pairs left after -Pairs / -ExcludePairs' }

$byMedia = @{}
foreach ($pr in $restrict) {
    $id = $dumpIx.Identity[$pr]
    if ($null -eq $id) { continue }
    $key = '{0}|{1}' -f $id.AId, $id.BId
    if (-not $byMedia.ContainsKey($key)) { $byMedia[$key] = [System.Collections.Generic.List[int]]::new() }
    $byMedia[$key].Add($pr)
}
foreach ($labels in $byMedia.Values) {
    if ($labels.Count -lt 2) { continue }
    $shown = @($labels | Sort-Object)
    Write-Warning ("pairs {0} share a_id/b_id (same media, same orientation). Quoting both inflates corpus rates — keep one label with -ExcludePairs (oriented: 6=22, 16=28)." -f ($shown -join ', '))
}

Write-Host ''
Write-Host 'Route 2 consensus (registration plan §0 S2)' -ForegroundColor Green
Write-Host "ScanDir:  $scanFull"
Write-Host "DumpDir:  $dumpFull"
Write-Host ("Scan:     {0} pairs, {1} patched gaps" -f $scan.Count, (@($scan.Values | ForEach-Object { $_.Count }) | Measure-Object -Sum).Sum)
Write-Host ("Dumps:    {0} complete pairs, {1} gaps with lag_decision{2}" -f $dumps.Count, (@($dumps.Values | ForEach-Object { $_.Count }) | Measure-Object -Sum).Sum,
    $(if ($dumpIx.Incomplete.Count -gt 0) { " (skipped incomplete: $($dumpIx.Incomplete -join ', '))" } else { '' }))
Write-Host ("Analysed: {0} pairs{1}" -f $restrict.Count,
    $(if ($ExcludePairs.Count -gt 0) { " (excluded: $($ExcludePairs -join ', '))" } else { '' }))
Write-Host ("Rules:    MinUnified={0}  StepMax={1:N0} ms  Agree={2:N0} ms  LoadBearing={3:N0} ms  InWindow=±{4:N0} ms" -f `
        $MinUnified, $StepMaxMs, $AgreeMs, $LoadBearingMs, $InWindowMs)

# [string[]] is load-bearing: a one-element @() unrolls to a scalar string, and $gates[0] would
# then be a char.
[string[]]$gates = $(if ($Gate -eq 'both') { 'peak_r', 'peak_z' } else { $Gate })
$results = [ordered]@{}
foreach ($g in $gates) {
    $r = Invoke-Route2 -Scan $scan -Dumps $dumps -Gate $g -Reduction $Reduction -Restrict $restrict
    $results[$g] = $r
    Show-Route2 -R $r
    if ($PairTable) { Show-PairTable -R $r -Scan $scan }
}

if ($OutOfWindow) {
    Write-Host ''
    Write-Host ("Pairs whose consensus is outside ±{0:N0} ms (the pair-31 class)" -f $InWindowMs) -ForegroundColor Cyan
    $r = $results[$gates[0]]
    $any = $false
    foreach ($pr in ($r.PerPair.Keys | Sort-Object)) {
        $v = $r.PerPair[$pr]
        if ($null -eq $v.Consensus -or [math]::Abs($v.Consensus) -le $InWindowMs) { continue }
        $any = $true
        $ps = $(if ($null -ne $v.Pooled) { '{0:+0.0000;-0.0000} s' -f ($v.Pooled / 1000) } else { '-' })
        Write-Host ("  pair {0,3}: consensus {1:+0.0000;-0.0000} s   pooled {2}   n_unified {3}" -f `
                $pr, ($v.Consensus / 1000), $ps, $v.NUnified)
    }
    if (-not $any) { Write-Host '  none' }
}

if ($ShoulderMap) {
    foreach ($g in $gates) {
        Write-Host ''
        Write-Host ("{0}: gaps with no usable shoulder, in gap order per pair (X = none)" -f $g) -ForegroundColor Cyan
        $any = $false
        foreach ($pr in $restrict) {
            $marks = @()
            foreach ($start in ($scan[$pr].Keys | Sort-Object { [double]$_ })) {
                $d = $dumps[$pr][$start]
                if ($null -eq $d) { continue }
                $sh = Get-GapShoulders -Entry $d -Gate $g
                $marks += $(if ($sh.Count -eq 0) { 'X' } else { '.' })
            }
            if ($marks -contains 'X') {
                $any = $true
                Write-Host ("  pair {0,3}: {1}" -f $pr, ($marks -join ' '))
            }
        }
        if (-not $any) { Write-Host '  none' }
    }
}

if ($ReductionRobustness) {
    Write-Host ''
    Write-Host 'Reduction robustness — the cell must not depend on the reduction choice' -ForegroundColor Cyan
    '{0,-8} {1,-8} {2,12} {3,22} {4,12} {5,26}' -f `
        'gate', 'reduce', 'in-window', 'load-bearing', 'bias_ms', 'big |adj| -> |adj-lag|' | Write-Host
    Write-Host ('-' * 92)
    foreach ($g in $gates) {
        foreach ($red in @('mean', 'pre', 'minabs', 'bestr')) {
            $r = Invoke-Route2 -Scan $scan -Dumps $dumps -Gate $g -Reduction $red -Restrict $restrict
            $bigAdj = Get-Median @($r.Big | ForEach-Object { [math]::Abs($_.AdjMs) })
            $bigRes = Get-Median @($r.Big | ForEach-Object { [math]::Abs($_.AdjMs - $_.LagMs) })
            $cell = '{' + (@($r.LoadBearing | ForEach-Object { $_.Pair } | Sort-Object) -join ',') + '}'
            $band = $(if ($null -ne $bigAdj) { '{0:N4} -> {1:N4} s' -f ($bigAdj / 1000), ($bigRes / 1000) } else { '-' })
            '{0,-8} {1,-8} {2,12} {3,22} {4,12} {5,26}' -f `
                $g, $red,
            ('{0}/{1}' -f $r.Agree.Count, $r.InWindow.Count),
            ('{0}/{1} {2}' -f $r.LbAgree.Count, $r.LoadBearing.Count, $cell),
            ('{0:+0.0;-0.0}' -f (Get-Median $r.Bias)),
            $band | Write-Host
        }
    }
}

if ($CsvOut -ne '') {
    $csvDir = Split-Path -Parent $CsvOut
    if ($csvDir -and -not (Test-Path -LiteralPath $csvDir)) {
        New-Item -ItemType Directory -Force -Path $csvDir | Out-Null
    }
    $out = [System.Collections.Generic.List[object]]::new()
    foreach ($g in $gates) {
        foreach ($row in $results[$g].GapRows) {
            $out.Add([pscustomobject]@{
                    gate         = $g
                    reduction    = $Reduction
                    pair         = $row.Pair
                    gap_index    = $row.Idx
                    a_start_secs = $row.StartKey
                    adj_ms       = [math]::Round($row.AdjMs, 3)
                    lag_ms       = $(if ($null -ne $row.LagMs) { [math]::Round($row.LagMs, 3) } else { '' })
                    residual_ms  = $(if ($null -ne $row.LagMs) { [math]::Round($row.AdjMs - $row.LagMs, 3) } else { '' })
                    dual_fit     = $row.Dual
                    unified      = $row.Unified
                    big_step     = $row.BigStep
                    shoulders    = $row.Shoulders
                    pre_lag_ms   = $(if ($null -ne $row.PreLagMs) { [math]::Round($row.PreLagMs, 3) } else { '' })
                    pre_r        = $(if ($null -ne $row.PreR) { [math]::Round($row.PreR, 4) } else { '' })
                    pre_z        = $(if ($null -ne $row.PreZ) { [math]::Round($row.PreZ, 2) } else { '' })
                    post_lag_ms  = $(if ($null -ne $row.PostLagMs) { [math]::Round($row.PostLagMs, 3) } else { '' })
                    post_r       = $(if ($null -ne $row.PostR) { [math]::Round($row.PostR, 4) } else { '' })
                    post_z       = $(if ($null -ne $row.PostZ) { [math]::Round($row.PostZ, 2) } else { '' })
                    step_ms      = $(if ($null -ne $row.StepMs) { [math]::Round($row.StepMs, 3) } else { '' })
                    splice_edge  = $row.SpliceEdge
                })
        }
    }
    $out | Sort-Object gate, pair, gap_index |
        Export-Csv -LiteralPath $CsvOut -NoTypeInformation -Encoding utf8
    Write-Host ''
    Write-Host "CSV: $CsvOut ($($out.Count) row(s))" -ForegroundColor DarkGray
}

Write-Host ''
Write-Host 'Dump-only analysis — no gate change, no repair re-run. Plan: docs/dev/archive/TEMP-fill-placement-registration-plan.md §0 S2' `
    -ForegroundColor DarkGray
