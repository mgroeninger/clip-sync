#!/usr/bin/env pwsh
#Requires -Version 7.0
# Dump-only census of the per-gap collar level offset (`scan.gaps[*].collar_level`).
#
# Each collar is read through that gap's OWN registered map since 2026-08-27 (`map_lag_ms`), not the
# whole-program offset. Q6 reports which map was used and compares the two cohorts; on a dump that
# predates the field, Q6 says so rather than guessing.
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
# RESIDUAL. `collar_level.offset_db − prior` — the quantity the apply half turns into a gain
# (`g_local_db = −residual`). ALWAYS defined on a measured collar.
#
# THE PRIOR IS THE APPLIED PROGRAM OFFSET, NOT THE MEASURED ONE (§4). Program gain is refused on
# `spread_db` past `max_spread_db`, `|offset_db|` past `max_program_gain_db`, incomparable channels,
# a non-finite reading, or `--no-program-gain` — and §4 then sets the prior to 0, which makes the
# residual `d_local` itself and the gain a hole-local master match on its own. Two things follow,
# and this script got both wrong until 2026-08-31:
#
#   - Subtracting a REFUSED offset reports a residual the apply half would never compute. The run
#     already recorded which way it went, so read the decision off `patch.program_gain` rather than
#     re-deriving it from `program_levels`.
#   - A pair whose offset abstained does not have a MISSING residual; it has one taken against 0.
#     Dropping those rows censors the very distribution that sets MAX_LOCAL_RESIDUAL_DB, in the
#     direction that makes the bound look safer than it is.
#
# `prior_source` on every row says which case it is, and Q4 breaks the population down by it. A dump
# with no `patch` block (report-only run, or a binary predating the field) carries no recorded
# decision and falls back to re-deriving one — labelled `derived:*`, warned about once, and governed
# by -MaxProgramSpreadDb / -MaxProgramGainDb, which are a COPY of the domain constants and must be
# passed explicitly for a run made under non-default flags.
#
# SIDE DISAGREEMENT. `|pre_offset_db − post_offset_db|` was specified as §3a's straddle detector in
# the level domain, expecting roughly ONE flag across the corpus (settled.md §D; 31/g7). The
# 2026-08-27 census refuted that: disagreement tracks `spread_db` and nothing else — 0 of 76
# well-populated collars under 0.5 dB spread disagreed by >1 dB, against 12 of 14 above 2.0 dB, with
# no dependence on gap length — and the one real straddle reads mid-pack among the noise. Q5 now
# reports it banded by spread; a FLAT profile across those bands would reopen the straddle reading.
#
# MAP. Since 2026-08-27 each collar is mapped at its own registered shoulder lag rather than the
# whole-program offset, recorded per gap as `collar_level.map_lag_ms`. Q6 reports which map each row
# was read through and compares the corrected cohort against the already-registered one.
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
    [int[]]$PairFloors = @(4, 8, 16),

    # Program-gain refusal thresholds, used ONLY to re-derive the §4 prior on a dump that carries no
    # `patch.program_gain` (see RESIDUAL above). Defaults mirror DEFAULT_PROGRAM_GAIN_MAX_SPREAD_DB
    # and DEFAULT_MAX_PROGRAM_GAIN_DB in crates/clip-sync-repair/src/domain/program_gain.rs. They are
    # a copy, not a read: a run made under non-default flags must pass its own values.
    [double]$MaxProgramSpreadDb = 2.5,
    [double]$MaxProgramGainDb = 6.0
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
            # The width these collars were read over, stamped by the scan since 2026-08-27. Null on
            # an older dump, where the only record is whichever config was passed at the time.
            collar_secs       = Get-Node $scan 'local_pair_collar_secs'
        })
    if ($state -ne 'measured') { continue }

    $pairOffsetDb = if ($null -eq $programOffset) { $null } else { [double](Get-Node $programOffset 'offset_db') }

    # THE PRIOR (see RESIDUAL in the header). Read the DECISION the run recorded, not the
    # measurement: `patch.program_gain` carries `gain`, the `offset_db` it applied or refused, and
    # `refused`. A refusal means the prior is 0 — not that the residual is unknown.
    $patchSummary = Get-Node $report 'patch'
    $programGain = Get-Node $patchSummary 'program_gain'
    $priorDb = 0.0
    $priorSource = $null
    if ($null -ne $programGain) {
        $refused = Get-Node $programGain 'refused'
        $appliedDb = Get-Node $programGain 'offset_db'
        if ($null -ne $refused) {
            $priorSource = "refused:$refused"
        } elseif ($null -ne $appliedDb) {
            $priorDb = [double]$appliedDb
            $priorSource = 'applied'
        } else {
            # Neither refused nor carrying the offset it applied: not a shape the binary emits.
            # Named rather than silently treated as a refusal, because it means the dump is wrong.
            $priorSource = 'malformed'
        }
    } elseif ($null -eq $pairOffsetDb) {
        # No `patch` block AND no measured offset. Both roads lead to a prior of 0, and no threshold
        # is involved, so this needs no `derived:` caveat.
        $priorSource = 'no_offset'
    } else {
        # No recorded decision, but an offset exists — re-derive with the SAME order and rules as
        # domain::program_fill_gain. Every branch here is a copy that can drift from the real one;
        # that is what the `derived:` prefix and the one-time warning below are for.
        $spreadDb = Get-Node $programOffset 'spread_db'
        $comparable = Get-Node $programOffset 'channels_comparable'
        if ($comparable -eq $false) {
            $priorSource = 'derived:channels'
        } elseif ($null -eq $spreadDb -or -not [double]::IsFinite([double]$spreadDb) -or -not [double]::IsFinite($pairOffsetDb)) {
            $priorSource = 'derived:not_finite'
        } elseif ([double]$spreadDb -gt $MaxProgramSpreadDb) {
            $priorSource = 'derived:spread'
        } elseif ([Math]::Abs($pairOffsetDb) -gt $MaxProgramGainDb) {
            $priorSource = 'derived:too_large'
        } else {
            $priorDb = $pairOffsetDb
            $priorSource = 'derived:applied'
        }
    }

    # One probe per report: does this binary emit `map_lag_ms`? An old dump has no such property,
    # which is NOT the same as a collar that declined the correction — see `registered` below.
    $hasMapLag = $false
    foreach ($g in $gaps) {
        $c = Get-Node $g 'collar_level'
        if ($null -ne $c -and $c.PSObject.Properties.Name -contains 'map_lag_ms') { $hasMapLag = $true; break }
    }

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
            map_lag_ms        = $null
            registered        = $null
            pair_offset_db    = $pairOffsetDb
            # What §4 actually subtracts, and why it is that number. `prior_db` differs from
            # `pair_offset_db` on exactly the rows where program gain was not applied.
            prior_db          = $priorDb
            prior_source      = $priorSource
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

            # A null `map_lag_ms` means the collar was read at the nominal map because no fit cleared
            # the trust floor; an ABSENT property means the dump predates the field entirely. Those
            # are different, so `registered` is left $null unless this report carries the field at
            # all — $false then means "a correction was available here and was declined".
            if ($hasMapLag) {
                $lag = Get-Node $collar 'map_lag_ms'
                $row.registered = ($null -ne $lag)
                if ($null -ne $lag) { $row.map_lag_ms = [double]$lag }
            }

            # ALWAYS defined on a measured collar. Where program gain was refused the prior is 0
            # (§4), so the residual is `d_local` itself and the gain is a hole-local master match —
            # a real reading that belongs in the distribution, not an absence. `prior_source` keeps
            # the two cases apart without censoring either.
            $row.residual_db = $row.offset_db - $priorDb
        }

        $rows.Add([pscustomobject]$row)
    }
}

foreach ($report in $reportStates) {
    $report.n_collars = @($rows | Where-Object { $_.pair -eq $report.pair -and $_.state -eq 'measured' }).Count
}

$measured = @($rows | Where-Object { $_.state -eq 'measured' })
$examined = @($reportStates | Where-Object { $_.state -eq 'measured' })

# The prior is only authoritative when the run recorded its own program-gain decision. Say so once,
# loudly, rather than per row: a census quoted from re-derived priors is quoting this script's copy
# of the thresholds, not the ones the binary ran.
$derived = @($measured | Where-Object { $_.prior_source -like 'derived:*' })
if ($derived.Count -gt 0) {
    $derivedPairs = @($derived | Select-Object -ExpandProperty pair -Unique)
    Write-Warning ("{0} of {1} collars carry a RE-DERIVED §4 prior — those reports have no 'patch.program_gain' (report-only run, or a binary predating the field), so the refusal thresholds applied were this script's -MaxProgramSpreadDb {2} / -MaxProgramGainDb {3}, not the run's. Pass the run's own values if it used non-default flags. Pairs: {4}" -f `
            $derived.Count, $measured.Count, $MaxProgramSpreadDb, $MaxProgramGainDb, ($derivedPairs -join ', '))
}
$malformed = @($measured | Where-Object { $_.prior_source -eq 'malformed' })
if ($malformed.Count -gt 0) {
    Write-Warning ("{0} collars sit in a report whose 'patch.program_gain' neither refused nor recorded an offset. That is not a shape the binary emits; treat those rows as suspect. Pairs: {1}" -f `
            $malformed.Count, (@($malformed | Select-Object -ExpandProperty pair -Unique) -join ', '))
}

Write-Host ''
Write-Host "Collar-level census over $DumpDir"
Write-Host ("  reports: {0}" -f $reportStates.Count)
foreach ($group in ($reportStates | Group-Object state | Sort-Object Name)) {
    Write-Host ("  {0,-10} {1}" -f $group.Name, $group.Count)
}

$stale = @($reportStates | Where-Object { $_.state -eq 'stale' })
# One census must describe one arm. Every question below pools rows across reports, so a DumpDir
# holding two collar widths silently averages them into a width that was never run — and before the
# scan stamped it, the only thing standing between us and that was the directory name.
$widths = @($reportStates | Where-Object { $null -ne $_.collar_secs } | ForEach-Object { [double]$_.collar_secs } | Sort-Object -Unique)
if ($widths.Count -eq 1) {
    Write-Host ("collar half-width               {0,6:0.0} s  (stamped by the scan)" -f $widths[0])
} elseif ($widths.Count -gt 1) {
    Write-Warning ("MIXED ARMS: this DumpDir holds {0} different collar widths ({1} s). Every pooled statistic below is meaningless. Split the reports by width and run this once per arm." -f `
        $widths.Count, (($widths | ForEach-Object { '{0:0.0}' -f $_ }) -join ', '))
} elseif ($reportStates.Count -gt 0) {
    Write-Host 'collar half-width                  n/a  (dump predates the stamp — the width is whatever config was passed)'
}

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

$tableFormat = '{0,-10} {1,4} {2,10} {3,8} {4,5} {5,5} {6,5} {7,5} {8,8} {9,8} {10,8} {11,7} {12,8} {13,8} {14,8} {15,7}'
Write-Host ''
# `lag` is the map this row was read through, in ms: '-' means the nominal map (no trusted fit, or a
# dump predating the field), '0' means a fit was believed and it said zero. Not the same fact.
# `prog` is the MEASURED whole-program offset; `prior` is what §4 subtracts, which is 0 wherever
# program gain was refused. They differ on exactly the rows the residual used to be wrong on.
Write-Host ($tableFormat -f 'pair', 'gap', 'a_start', 'gap_s', 'act', 'std', 'pre', 'post', 'offset', 'spread', 'se', '|p-p|', 'prog', 'prior', 'resid', 'lag_ms')
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
        (Format-Db $row.prior_db),
        (Format-Db $row.residual_db),
        $(if ($null -eq $row.map_lag_ms) { '-' } else { '{0:0}' -f [double]$row.map_lag_ms }))
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
    Write-Host '  no residual on any measured collar — impossible unless a row failed to compute one'
} else {
    $absResiduals = [double[]]@($withResidual | ForEach-Object { [Math]::Abs([double]$_.residual_db) } | Sort-Object)
    Write-Host ("  collars with a residual         {0} of {1}" -f $withResidual.Count, $measured.Count)
    # Which number each residual was taken against. §4 subtracts the APPLIED program offset, so
    # every non-`applied` row below has a prior of 0 and reads `-d_local` — a hole-local master
    # match on its own. That is a result, not a gap in the data, but it changes what the
    # distribution means: a corpus dominated by refusals is not measuring a residual at all.
    Write-Host '  prior each residual was taken against:'
    foreach ($sourceGroup in ($withResidual | Group-Object prior_source | Sort-Object Count -Descending)) {
        Write-Host ("    {0,-26} {1,4}" -f $sourceGroup.Name, $sourceGroup.Count)
    }
    Write-Host ("  |residual| median / p90 / max   {0} / {1} / {2}" -f `
        (Format-Ratio (Get-Percentile $absResiduals 0.50)), (Format-Ratio (Get-Percentile $absResiduals 0.90)), (Format-Ratio (Get-Percentile $absResiduals 1.0)))
    Write-Host ("  |residual| <= {0,-5} dB           {1} of {2}" -f $ResidualDb, @($withResidual | Where-Object { [Math]::Abs([double]$_.residual_db) -le $ResidualDb }).Count, $withResidual.Count)
    Write-Host '  (MAX_LOCAL_RESIDUAL_DB belongs where the readings stop, not at a rail)'

    # |residual| IS |g_local_db| (§4: g_local_db = -(d_local - offset_prior)). Every row above is a
    # gain that would land on a fill, not a diagnostic, so the same distribution is worth reading in
    # the unit that ships - and read against each candidate floor, because the floor turns out to
    # control the applied gain far more than the residual bound does.
    Write-Host ''
    Write-Host '  The same distribution as the GAIN it becomes (|residual| = |g_local_db|):'
    Write-Host ('    floor      n     p50     p90     p99     max   >0.5dB      %   >1dB   >3dB   >6dB')
    foreach ($floor in (@(0) + $PairFloors | Sort-Object -Unique)) {
        $sub = [double[]]@($withResidual | Where-Object { [double]$_.n_pairs -ge $floor } |
            ForEach-Object { [Math]::Abs([double]$_.residual_db) } | Sort-Object)
        if ($sub.Count -eq 0) { continue }
        $over = { param($t) @($sub | Where-Object { $_ -gt $t }).Count }
        Write-Host ("    n>={0,-4} {1,5}  {2}  {3}  {4}  {5}  {6,4} {7,5:0.0}%  {8,4}   {9,4}   {10,4}" -f `
                $floor, $sub.Count, (Format-Ratio (Get-Percentile $sub 0.50)), (Format-Ratio (Get-Percentile $sub 0.90)), `
            (Format-Ratio (Get-Percentile $sub 0.99)), (Format-Ratio (Get-Percentile $sub 1.0)), `
            (& $over 0.5), (100.0 * (& $over 0.5) / $sub.Count), (& $over 1.0), (& $over 3.0), (& $over 6.0))
    }
    Write-Host '    (a candidate MAX_LOCAL_RESIDUAL_DB above the p99 column is inert - the max_fill_gain_db'
    Write-Host '     failure mode: a bound so far out it forfeits the detection §4 wants it for)'
}

$twoSided = @($measured | Where-Object { $null -ne $_.side_delta_db })
Write-Host ''
Write-Host '4a Q5. Do the two sides agree, and is a disagreement anything but noise?'
if ($twoSided.Count -eq 0) {
    Write-Host '  no two-sided collar - nothing to compare'
} else {
    $sideDeltas = [double[]]@($twoSided | ForEach-Object { [double]$_.side_delta_db } | Sort-Object)
    Write-Host ("  two-sided collars               {0}" -f $twoSided.Count)
    Write-Host ("  |pre-post| median / p90 / max   {0} / {1} / {2}" -f `
        (Format-Ratio (Get-Percentile $sideDeltas 0.50)), (Format-Ratio (Get-Percentile $sideDeltas 0.90)), (Format-Ratio (Get-Percentile $sideDeltas 1.0)))

    # Read against spread, not as a raw count. The 2026-08-27 corpus settled what this statistic is:
    # side disagreement is a function of `spread_db` and nothing else - zero of 76 well-populated
    # collars under 0.5 dB spread disagreed by more than 1 dB, against 12 of 14 above 2.0 dB, with no
    # dependence on gap length. It was specified as a map-straddle detector (settled.md section D)
    # and it is not one: the corpus's one real straddle reads mid-pack among the noise. A raw flag
    # count here therefore measures how unsteady the material is, which `spread_db` already says.
    Write-Host '  |pre-post| by collar spread (the only thing it tracks):'
    foreach ($band in @(@(0.0, 0.5), @(0.5, 1.0), @(1.0, 2.0), @(2.0, [double]::PositiveInfinity))) {
        $inBand = @($twoSided | Where-Object { [double]$_.spread_db -ge $band[0] -and [double]$_.spread_db -lt $band[1] })
        if ($inBand.Count -eq 0) { continue }
        $bandDeltas = [double[]]@($inBand | ForEach-Object { [double]$_.side_delta_db } | Sort-Object)
        $hi = if ([double]::IsInfinity($band[1])) { '  +' } else { '{0,3:0.0}' -f $band[1] }
        Write-Host ("    spread {0,3:0.0}-{1} dB : {2,4} collars, median {3}, over {4} dB: {5}" -f `
            $band[0], $hi, $inBand.Count, (Format-Ratio (Get-Percentile $bandDeltas 0.50)), `
            $SideDeltaDb, @($inBand | Where-Object { $_.side_delta_db -gt $SideDeltaDb }).Count)
    }
    Write-Host '  (a flat profile across the bands would REOPEN the straddle reading; a rising one is the settled result)'
}

$withLag = @($measured | Where-Object { $null -ne $_.registered })
Write-Host ''
Write-Host '4a Q6. Which map was each collar read through?'
if ($withLag.Count -eq 0) {
    Write-Host '  no collar carries map_lag_ms - this dump predates the per-gap map correction'
} else {
    $reg = @($withLag | Where-Object { $_.registered })
    Write-Host ("  collars with a trusted lag      {0} of {1}" -f $reg.Count, $withLag.Count)
    Write-Host ("  read at the nominal map         {0}  (no fit above the trust floor)" -f ($withLag.Count - $reg.Count))
    if ($reg.Count -gt 0) {
        $lags = [double[]]@($reg | ForEach-Object { [Math]::Abs([double]$_.map_lag_ms) } | Sort-Object)
        Write-Host ("  |map_lag_ms| median / p90 / max {0,6:0} / {1,6:0} / {2,6:0}" -f `
            (Get-Percentile $lags 0.50), (Get-Percentile $lags 0.90), (Get-Percentile $lags 1.0))
        Write-Host ("  moved at all                    {0}  ({1} stayed at zero)" -f `
            @($lags | Where-Object { $_ -gt 0 }).Count, @($lags | Where-Object { $_ -eq 0 }).Count)
        # The comparison this section exists for: a corrected map should pull spread and residual
        # toward what the already-registered cohort reads. If it does not, the residuals are real.
        #
        # Reported BOTH unfiltered and at n_pairs >= 8, because the recorded nominal-map baseline the
        # corrected arm is judged against - lag 0 reading spread 0.25 / |resid| 0.08 (plan section 4b,
        # Result 3) - was taken at n >= 8. Comparing an unfiltered cohort against it would be reading
        # two different populations, and the thin collars are exactly where the noise lives.
        $cohorts = @(@('lag = 0 (map already right)', { [Math]::Abs([double]$_.map_lag_ms) -eq 0 }),
                     @('lag > 0 (map corrected)    ', { [Math]::Abs([double]$_.map_lag_ms) -gt 0 }))
        foreach ($cut in @(@('all', 0), @('n>=8', 8))) {
            Write-Host ("    -- {0} collars --" -f $cut[0])
            foreach ($cohort in $cohorts) {
                $sub = @($reg | Where-Object $cohort[1] | Where-Object { [double]$_.n_pairs -ge $cut[1] })
                if ($sub.Count -eq 0) {
                    Write-Host ("      {0}: none" -f $cohort[0])
                    continue
                }
                $sp = [double[]]@($sub | ForEach-Object { [double]$_.spread_db } | Sort-Object)
                $rs = [double[]]@($sub | Where-Object { $null -ne $_.residual_db } | ForEach-Object { [Math]::Abs([double]$_.residual_db) } | Sort-Object)
                Write-Host ("      {0}: {1,4} collars | spread med {2} | |resid| med {3}" -f `
                    $cohort[0], $sub.Count, (Format-Ratio (Get-Percentile $sp 0.50)), `
                    $(if ($rs.Count) { Format-Ratio (Get-Percentile $rs 0.50) } else { '   -  ' }))
            }
        }
        Write-Host '    (baseline to beat: lag = 0 at n>=8 read spread 0.250 / |resid| 0.080 on the nominal-map arm)'

        # Banded by lag magnitude, n>=8. Emitted here because deriving it by hand got the bin edge
        # wrong once: a `> 200` filter under a `>= 200` label parks the collars sitting at exactly
        # 200 ms in the band below, which on the 2026-08-27 corrected arm moved the worst remaining
        # residual (0.256 over 47 collars) into the lower band and manufactured an inversion. Bins
        # are HALF-OPEN and the boundary value belongs to the band it opens.
        #
        # Read the shape, not just the levels: what remains after the correction is the residual
        # misregistration budget, and its profile is what discriminates the candidate causes. Flat
        # would be the 100 ms quantization floor; monotone in lag would be uncorrected
        # misregistration; `|pre-post|` rising with lag would be one fitted lag failing to describe
        # both shoulders (seam-splice-dualfit). The corrected arm showed none of the three.
        $bands = @(
            @{ label = '0 ms      '; lo = 0.0; hi = 1e-9 },
            @{ label = '100 ms    '; lo = 1e-9; hi = 200.0 },
            @{ label = '200 ms    '; lo = 200.0; hi = 300.0 },
            @{ label = '>= 300 ms '; lo = 300.0; hi = [double]::PositiveInfinity })
        Write-Host '    -- by lag magnitude, n>=8 (half-open bins; the boundary belongs to the band it opens) --'
        foreach ($band in $bands) {
            $sub = @($reg | Where-Object {
                    $a = [Math]::Abs([double]$_.map_lag_ms)
                    [double]$_.n_pairs -ge 8 -and $a -ge $band.lo -and $a -lt $band.hi
                })
            if ($sub.Count -eq 0) { Write-Host ("      {0}: none" -f $band.label); continue }
            $sp = [double[]]@($sub | ForEach-Object { [double]$_.spread_db } | Sort-Object)
            $rs = [double[]]@($sub | Where-Object { $null -ne $_.residual_db } | ForEach-Object { [Math]::Abs([double]$_.residual_db) } | Sort-Object)
            $sd = [double[]]@($sub | Where-Object { $null -ne $_.side_delta_db } | ForEach-Object { [double]$_.side_delta_db } | Sort-Object)
            Write-Host ("      {0}: {1,4} collars | spread med {2} | |resid| med {3} | |pre-post| med {4}" -f `
                    $band.label, $sub.Count, (Format-Ratio (Get-Percentile $sp 0.50)), `
                $(if ($rs.Count) { Format-Ratio (Get-Percentile $rs 0.50) } else { '  -  ' }), `
                $(if ($sd.Count) { Format-Ratio (Get-Percentile $sd 0.50) } else { '  -  ' }))
        }
    }
}

if ($CsvOut) {
    $rows | Export-Csv -LiteralPath $CsvOut -NoTypeInformation
    Write-Host ''
    Write-Host "per-gap CSV -> $CsvOut"
}
Write-Host ''
