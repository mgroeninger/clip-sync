#!/usr/bin/env pwsh
# Dump-only census of the per-gap collar level offset (`scan.gaps[*].collar_level`).
#
# The calibration pass of docs/dev/TEMP-local-paired-gain-plan.md §4a. Per-**gap** sibling of
# census-program-levels.ps1, which is per-**pair**: every question §4a asks is about one hole, not
# one file. Consumer only — opens no media, runs no repair, needs no oracle. Point it at repair JSON
# that `measure-patch-outcomes.ps1` (or any `--patch-only --format json` run) already wrote.
#
# THE QUESTION. Three constants in the apply half are arguments, not measurements —
# `MIN_LOCAL_PAIRS`, `local_pair_collar_secs`, `MAX_LOCAL_RESIDUAL_DB`. This dump is what turns them
# into numbers, and §4a gates the apply half on it: do not enable a local gain, and do not quote the
# §10 cohort table, until this has been run and the three constants are stated against it. If the SE
# column says one collar is noisier than the residual it would correct, the answer is a wider collar
# or a higher floor — not shipping and listening.
#
# Usage:
#   ./scripts/measure/census-collar-levels.ps1 -DumpDir gap-files/<date>-patch-outcomes
#   ./scripts/measure/census-collar-levels.ps1 -DumpDir gap-files/<date>-patch-outcomes -CsvOut gap-files/collar-levels.csv
#
# Accepted layout: `<DumpDir>/<pair>.json`, repair reports carrying `scan.gaps`. Fingerprint-corpus
# dumps are NOT accepted — collar levels live in the repair scan report.
#
# STATES, KEPT APART, same vocabulary as census-program-levels.ps1. Collapsing them produces a
# confident wrong answer. Two levels, because two different things can be missing:
#
#   Per report (does a collar have any chance of existing?):
#     stale     — `scan.program_levels` absent entirely. Binary predates even the whole-program
#                 capture. NOT a corpus finding; rebuild and re-run.
#     unaligned — `program_levels` present but `b` absent. B was never level-scanned, because that
#                 only runs when an alignment offset exists — and the collar is mapped through the
#                 same offset, so no gap in this report can carry one.
#     measured  — the report could carry collars; its gaps are examined below.
#
#   Per gap, inside a `measured` report:
#     abstained — `collar_level` absent. Channels incomparable, or NOTHING in the collar paired.
#                 A real result about the hole. Note this is not "too few pairs": the estimator
#                 carries no floor, precisely so this census can see the distribution below
#                 `MIN_LOCAL_PAIRS` before that constant is chosen. Thin collars appear as
#                 `measured` rows with small `n_pairs`.
#     measured  — `collar_level` present.
#
# A `measured` report whose gaps ALL abstain is reported separately: that shape is also what a
# pre-feature binary looks like, and the script cannot tell the two apart from the JSON alone.
#
# RESIDUAL. `collar_level.offset_db − program_levels.offset.offset_db` — the quantity the apply half
# would turn into a gain. Null when the pair's whole-program offset abstained: §4 makes the prior 0
# there, so there is no residual to take, and averaging a null into the distribution would understate
# it.
#
# SIDE DISAGREEMENT. `|pre_offset_db − post_offset_db|` is §3a's straddle detector in the level
# domain. Its expected corpus count is roughly ONE (settled.md §D; 31/g7). A census that flags many
# collars is not finding many straddles — it is saying the collar misregisters for some other
# reason, which is the more valuable reading.
#
# MEDIA HYGIENE: pair labels come from filenames and nothing else is read from the path. Use numeric
# pair indices as labels — never titles or media paths, in the manifest or in anything quoted from
# this output. See docs/dev/repair-perf.md §"Media handling".

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$DumpDir,

    # Per-gap CSV: one row per gap in a `measured` report, abstentions included.
    [string]$CsvOut = '',

    # Reporting cuts. NOT gates, and not validated thresholds — the point of the pass is that the
    # dump comes before the constants. Rows are counted either side of each so the distribution is
    # visible.
    [double]$SeDb = 1.0,
    [double]$ResidualDb = 1.0,
    [double]$SideDeltaDb = 1.0,

    # Candidate pair floors to score. The point of Q2 is to CHOOSE one, so the census reports what
    # each would cost rather than assuming the provisional 8.
    [int[]]$PairFloors = @(4, 8, 16)
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $DumpDir)) {
    throw "dump dir not found: $DumpDir"
}

# `$null` for an absent JSON key, so an absent field and a present zero never collapse.
function Get-Node([object]$Parent, [string]$Name) {
    if ($null -eq $Parent) { return $null }
    if ($Parent.PSObject.Properties.Name -notcontains $Name) { return $null }
    return $Parent.$Name
}

function Format-Db([object]$Value, [int]$Places = 2) {
    if ($null -eq $Value) { return '-' }
    # Round BEFORE choosing the sign section. .NET picks the section from the unrounded value, so a
    # value like -0.004 takes the negative section, renders through the positive one, and comes out
    # "-+0.00". Rounding first also collapses negative zero.
    $rounded = [Math]::Round([double]$Value, $Places)
    if ($rounded -eq 0) { $rounded = [double]0 }
    return $rounded.ToString("+0.$('0' * $Places);-0.$('0' * $Places)")
}

function Format-Ratio([object]$Value) {
    if ($null -eq $Value) { return '-' }
    return ([double]$Value).ToString('0.000')
}

# Nearest-rank, matching the estimator's own percentile convention (and its median: lower-middle,
# never averaged, so every quoted value is one actually observed).
function Get-Percentile([double[]]$Sorted, [double]$Fraction) {
    if ($Sorted.Count -eq 0) { return $null }
    $rank = [int][Math]::Ceiling($Fraction * $Sorted.Count) - 1
    if ($rank -lt 0) { $rank = 0 }
    if ($rank -ge $Sorted.Count) { $rank = $Sorted.Count - 1 }
    return $Sorted[$rank]
}

$files = @(Get-ChildItem -LiteralPath $DumpDir -Filter '*.json' -File | Sort-Object Name)
if ($files.Count -eq 0) {
    throw "no *.json under $DumpDir — expected repair reports from measure-patch-outcomes.ps1"
}

$rows = [System.Collections.Generic.List[object]]::new()
$reportStates = [System.Collections.Generic.List[object]]::new()

foreach ($file in $files) {
    $label = [System.IO.Path]::GetFileNameWithoutExtension($file.Name)

    # A zero-length file is the pair currently being written by the sweep. Caught before the parse
    # because `ConvertFrom-Json` returns $null on empty input *without throwing*, which would fall
    # through to the `stale` branch below — the loudest false alarm this script can raise.
    if ($file.Length -eq 0) {
        Write-Warning "$label`: empty file — in-flight or failed run, not a stale binary. Skipped."
        continue
    }
    try {
        $report = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    } catch {
        Write-Warning "$label`: unreadable JSON — skipped ($($_.Exception.Message))"
        continue
    }
    if ($null -eq $report) {
        Write-Warning "$label`: parsed to nothing — truncated or still being written. Skipped."
        continue
    }

    $scan = Get-Node $report 'scan'
    $levels = Get-Node $scan 'program_levels'
    $programOffset = Get-Node $levels 'offset'
    $gaps = @(Get-Node $scan 'gaps')

    $state = 'measured'
    if ($null -eq $levels) {
        $state = 'stale'
    } elseif ($null -eq (Get-Node $levels 'b')) {
        $state = 'unaligned'
    }

    $reportStates.Add([pscustomobject][ordered]@{
            pair              = $label
            state             = $state
            n_gaps            = $gaps.Count
            n_collars         = 0
            program_offset_db = if ($null -eq $programOffset) { $null } else { [double](Get-Node $programOffset 'offset_db') }
        })
    if ($state -ne 'measured') { continue }

    $pairOffsetDb = if ($null -eq $programOffset) { $null } else { [double](Get-Node $programOffset 'offset_db') }

    for ($i = 0; $i -lt $gaps.Count; $i++) {
        $gap = $gaps[$i]
        $collar = Get-Node $gap 'collar_level'
        $aStart = Get-Node $gap 'video_a_start_secs'
        $aEnd = Get-Node $gap 'video_a_end_secs'

        $row = [ordered]@{
            pair              = $label
            gap_index         = $i
            a_start_secs      = $aStart
            gap_secs          = if ($null -eq $aStart -or $null -eq $aEnd) { $null } else { [double]$aEnd - [double]$aStart }
            state             = 'abstained'
            n_pairs           = $null
            n_active          = $null
            n_steady          = $null
            steady_per_active = $null
            pairs_per_steady  = $null
            n_pre             = $null
            n_post            = $null
            offset_db         = $null
            pre_offset_db     = $null
            post_offset_db    = $null
            side_delta_db     = $null
            spread_db         = $null
            se_db             = $null
            paired_span_secs  = $null
            pair_offset_db    = $pairOffsetDb
            residual_db       = $null
        }

        if ($null -ne $collar) {
            $row.state = 'measured'
            $row.n_pairs = [int](Get-Node $collar 'n_pairs')
            $row.n_active = [int](Get-Node $collar 'n_active')
            $row.n_steady = [int](Get-Node $collar 'n_steady')
            # Which filter spent the collar. Left $null rather than 0 on a zero denominator: an
            # undefined ratio is not a yield of nothing.
            if ($row.n_active -gt 0) { $row.steady_per_active = $row.n_steady / $row.n_active }
            if ($row.n_steady -gt 0) { $row.pairs_per_steady = $row.n_pairs / $row.n_steady }
            $row.n_pre = [int](Get-Node $collar 'n_pre')
            $row.n_post = [int](Get-Node $collar 'n_post')
            $row.offset_db = [double](Get-Node $collar 'offset_db')
            $row.spread_db = [double](Get-Node $collar 'spread_db')

            $pre = Get-Node $collar 'pre_offset_db'
            $post = Get-Node $collar 'post_offset_db'
            $row.pre_offset_db = $pre
            $row.post_offset_db = $post
            # Only defined on a two-sided collar. A leading- or trailing-edge gap is one-sided by
            # construction — that is not agreement and must not be counted as a zero.
            if ($null -ne $pre -and $null -ne $post) {
                $row.side_delta_db = [Math]::Abs([double]$pre - [double]$post)
            }

            # SE of the median from the MAD: 1.4826 MAD -> sigma, 1.2533 sigma/sqrt(n) -> SE(median).
            # Both factors are normal-theory; the collar population is small and not guaranteed
            # normal, so read this as an order of magnitude, not a confidence interval.
            if ($row.n_pairs -gt 0) {
                $row.se_db = 1.2533 * 1.4826 * $row.spread_db / [Math]::Sqrt($row.n_pairs)
            }

            $first = Get-Node $collar 'first_pair_secs'
            $last = Get-Node $collar 'last_pair_secs'
            if ($null -ne $first -and $null -ne $last) {
                # first..last spans the hole on a two-sided collar, so this is the paired
                # SPAN, not a reach. Read it against 2*W + gap_secs.
                $row.paired_span_secs = [double]$last - [double]$first
            }

            # Null when the whole-program offset abstained: §4 sets the prior to 0 there, so there is
            # no residual — writing 0 would report a perfect local match that was never measured.
            if ($null -ne $pairOffsetDb) {
                $row.residual_db = $row.offset_db - $pairOffsetDb
            }
        }

        $rows.Add([pscustomobject]$row)
    }
}

foreach ($report in $reportStates) {
    $report.n_collars = @($rows | Where-Object { $_.pair -eq $report.pair -and $_.state -eq 'measured' }).Count
}

$measured = @($rows | Where-Object { $_.state -eq 'measured' })
$examined = @($reportStates | Where-Object { $_.state -eq 'measured' })

Write-Host ''
Write-Host "Collar-level census over $DumpDir"
Write-Host ("  reports: {0}" -f $reportStates.Count)
foreach ($group in ($reportStates | Group-Object state | Sort-Object Name)) {
    Write-Host ("  {0,-10} {1}" -f $group.Name, $group.Count)
}

$stale = @($reportStates | Where-Object { $_.state -eq 'stale' })
if ($stale.Count -gt 0) {
    Write-Host ''
    Write-Warning ("{0} report(s) carry no scan.program_levels at all — those are STALE BINARY runs, not corpus findings. Rebuild and re-run before reading anything below as a corpus result: {1}" -f $stale.Count, (($stale.pair) -join ', '))
}

$silent = @($examined | Where-Object { $_.n_gaps -gt 0 -and $_.n_collars -eq 0 })
if ($silent.Count -gt 0) {
    Write-Host ''
    Write-Warning ("{0} report(s) have gaps but not one collar. That is ALSO what a pre-collar binary looks like — confirm the binary before reading it as abstention: {1}" -f $silent.Count, (($silent.pair) -join ', '))
}

if ($rows.Count -eq 0) {
    Write-Host ''
    Write-Host 'No gaps in any examined report — nothing to calibrate against.'
    Write-Host ''
    return
}

Write-Host ''
Write-Host '4a Q1. How often does the collar exist at all?'
Write-Host ("  gaps in examined reports  {0}" -f $rows.Count)
Write-Host ("  collar measured           {0} ({1:P1})" -f $measured.Count, ($measured.Count / $rows.Count))
Write-Host ("  collar abstained          {0}" -f ($rows.Count - $measured.Count))
Write-Host '  (an estimator that abstains on most holes does not exist: program gain silently carries and the Change bought nothing)'

if ($measured.Count -eq 0) {
    Write-Host ''
    Write-Host 'No measured collar anywhere — the remaining questions have no population.'
    Write-Host ''
    return
}

$tableFormat = '{0,-10} {1,4} {2,10} {3,8} {4,5} {5,5} {6,5} {7,5} {8,8} {9,8} {10,8} {11,7} {12,8} {13,8}'
Write-Host ''
Write-Host ($tableFormat -f 'pair', 'gap', 'a_start', 'gap_s', 'act', 'std', 'pre', 'post', 'offset', 'spread', 'se', '|p-p|', 'prog', 'resid')
Write-Host '  act -> std -> (pre + post) is where the collar was spent: a thin pair count is read off those, not guessed at'
foreach ($row in $measured) {
    Write-Host ($tableFormat -f `
            $row.pair,
        $row.gap_index,
        (Format-Ratio $row.a_start_secs),
        (Format-Ratio $row.gap_secs),
        $row.n_active,
        $row.n_steady,
        $row.n_pre,
        $row.n_post,
        (Format-Db $row.offset_db),
        (Format-Ratio $row.spread_db),
        (Format-Ratio $row.se_db),
        $(if ($null -eq $row.side_delta_db) { '-' } else { Format-Ratio $row.side_delta_db }),
        (Format-Db $row.pair_offset_db),
        (Format-Db $row.residual_db))
}

$nPairs = [double[]]@($measured | ForEach-Object { [double]$_.n_pairs } | Sort-Object)
Write-Host ''
Write-Host '4a Q2. What does n_pairs actually look like?'
Write-Host ("  min / p10 / median / p90 / max   {0:N0} / {1:N0} / {2:N0} / {3:N0} / {4:N0}" -f `
    (Get-Percentile $nPairs 0.0), (Get-Percentile $nPairs 0.10), (Get-Percentile $nPairs 0.50), (Get-Percentile $nPairs 0.90), (Get-Percentile $nPairs 1.0))
$oneSided = @($measured | Where-Object { $_.n_pre -eq 0 -or $_.n_post -eq 0 })
Write-Host ("  one-sided collars               {0} of {1}" -f $oneSided.Count, $measured.Count)
Write-Host '  (ceiling is 2 x W / block_secs before the active/steady/B filters — 40 at W=2.0 s, 100 ms blocks)'
Write-Host ''
Write-Host '  What a pair floor would cost (the estimator applies none - that is why this is choosable):'
foreach ($floor in ($PairFloors | Sort-Object)) {
    $under = @($measured | Where-Object { $_.n_pairs -lt $floor })
    Write-Host ("    n_pairs < {0,-3}  {1} of {2} measured collars would fall back to program gain ({3:P1} of all gaps)" -f `
            $floor, $under.Count, $measured.Count, (($under.Count + $rows.Count - $measured.Count) / $rows.Count))
}
# Which filter spends the collar decides the remedy: steadiness -> widen W; B coverage -> the
# collar is fine and a wider one will not help.
$steadyRatios = [double[]]@($measured | Where-Object { $null -ne $_.steady_per_active } | ForEach-Object { [double]$_.steady_per_active } | Sort-Object)
$coverRatios = [double[]]@($measured | Where-Object { $null -ne $_.pairs_per_steady } | ForEach-Object { [double]$_.pairs_per_steady } | Sort-Object)
if ($steadyRatios.Count -gt 0) {
    Write-Host ''
    Write-Host ("  median n_steady/n_active        {0}   (steadiness cost - low means widen the collar)" -f (Format-Ratio (Get-Percentile $steadyRatios 0.50)))
}
if ($coverRatios.Count -gt 0) {
    Write-Host ("  median n_pairs/n_steady         {0}   (B coverage - low means a wider collar will not help)" -f (Format-Ratio (Get-Percentile $coverRatios 0.50)))
}

$ses = [double[]]@($measured | Where-Object { $null -ne $_.se_db } | ForEach-Object { [double]$_.se_db } | Sort-Object)
if ($ses.Count -gt 0) {
    Write-Host ''
    Write-Host '4a Q3. How noisy is one collar?'
    Write-Host ("  SE dB  median / p90 / max       {0} / {1} / {2}" -f `
        (Format-Ratio (Get-Percentile $ses 0.50)), (Format-Ratio (Get-Percentile $ses 0.90)), (Format-Ratio (Get-Percentile $ses 1.0)))
    Write-Host ("  SE <= {0,-5} dB                   {1} of {2}" -f $SeDb, @($measured | Where-Object { $null -ne $_.se_db -and $_.se_db -le $SeDb }).Count, $ses.Count)
    Write-Host '  (compare against the residuals below: an estimator noisier than what it corrects wants a wider W or a higher floor, not a listening cohort)'
}

$withResidual = @($measured | Where-Object { $null -ne $_.residual_db })
Write-Host ''
Write-Host '4a Q4. Where do real residuals stop?'
if ($withResidual.Count -eq 0) {
    Write-Host '  no collar sits in a pair whose whole-program offset was measured — residual is undefined, not zero'
} else {
    $absResiduals = [double[]]@($withResidual | ForEach-Object { [Math]::Abs([double]$_.residual_db) } | Sort-Object)
    Write-Host ("  collars with a prior            {0} of {1}" -f $withResidual.Count, $measured.Count)
    Write-Host ("  |residual| median / p90 / max   {0} / {1} / {2}" -f `
        (Format-Ratio (Get-Percentile $absResiduals 0.50)), (Format-Ratio (Get-Percentile $absResiduals 0.90)), (Format-Ratio (Get-Percentile $absResiduals 1.0)))
    Write-Host ("  |residual| <= {0,-5} dB           {1} of {2}" -f $ResidualDb, @($withResidual | Where-Object { [Math]::Abs([double]$_.residual_db) -le $ResidualDb }).Count, $withResidual.Count)
    Write-Host '  (MAX_LOCAL_RESIDUAL_DB belongs where the readings stop, not at a rail)'
}

$twoSided = @($measured | Where-Object { $null -ne $_.side_delta_db })
Write-Host ''
Write-Host '4a Q5. How often do the sides disagree?'
if ($twoSided.Count -eq 0) {
    Write-Host '  no two-sided collar — the straddle detector has no population here'
} else {
    $flagged = @($twoSided | Where-Object { $_.side_delta_db -gt $SideDeltaDb })
    $sideDeltas = [double[]]@($twoSided | ForEach-Object { [double]$_.side_delta_db } | Sort-Object)
    Write-Host ("  two-sided collars               {0}" -f $twoSided.Count)
    Write-Host ("  |pre-post| median / p90 / max   {0} / {1} / {2}" -f `
        (Format-Ratio (Get-Percentile $sideDeltas 0.50)), (Format-Ratio (Get-Percentile $sideDeltas 0.90)), (Format-Ratio (Get-Percentile $sideDeltas 1.0)))
    Write-Host ("  |pre-post| > {0,-5} dB            {1}" -f $SideDeltaDb, $flagged.Count)
    if ($flagged.Count -gt 0) {
        Write-Host ("    at: {0}" -f (($flagged | ForEach-Object { "$($_.pair)/g$($_.gap_index)" }) -join ', '))
    }
    Write-Host '  (settled.md section D expects roughly ONE across the corpus. Many flags means the collar is misregistering, not that the corpus is full of straddles.)'
}

if ($CsvOut) {
    $rows | Export-Csv -LiteralPath $CsvOut -NoTypeInformation
    Write-Host ''
    Write-Host "per-gap CSV -> $CsvOut"
}
Write-Host ''
