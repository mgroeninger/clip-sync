#!/usr/bin/env pwsh
#Requires -Version 7.0
# Dump-only census of the whole-program A/B level offset (`scan.program_levels`).
#
# Tier 3 of the validation ladder in docs/dev/archive/TEMP-ab-program-levels-plan.md §7a, and the read the
# BACKLOG entry asks for. Consumer only — opens no media, runs no repair. Point it at repair JSON
# that `measure-patch-outcomes.ps1` (or any `--patch-only --format json` run) already wrote.
#
# THE QUESTION. Not "what is the offset" — *does a single scalar exist at all*. That is the
# precondition §8 puts on any later use of this number as a gain. A corpus where `spread_db` is
# routinely wide, or where `chunks` drift within a pair, settles the question NEGATIVELY, and that
# is a real result rather than a failed run. Report it as one.
#
# Usage:
#   ./scripts/measure/census-program-levels.ps1 -DumpDir gap-files/<date>-patch-outcomes
#   ./scripts/measure/census-program-levels.ps1 -DumpDir gap-files/<date>-patch-outcomes -CsvOut gap-files/program-levels.csv
#   ./scripts/measure/census-program-levels.ps1 -DumpDir gap-files/<date>-patch-outcomes -GainCsvOut gap-files/program-levels-gain.csv
#
# Accepted layout: `<DumpDir>/<pair>.json`, repair reports carrying `scan.program_levels`.
# Fingerprint-corpus dumps are NOT accepted: program levels live in the repair scan report, and no
# fingerprint dump carries them.
#
# FOUR STATES, KEPT APART. Collapsing them produces a confident wrong answer:
#   stale     — `scan.program_levels` absent entirely. Binary predates the feature; the JSON is
#               otherwise healthy and the field is silently missing. NOT a corpus finding.
#   unaligned — `program_levels` present but `b` absent. B was never scanned, because the B level
#               scan only runs when an alignment offset exists.
#   abstained — `b` present but `offset` absent. B was scanned and fewer than `MIN_PAIRS` (50)
#               survived pairing. A real result about the pair.
#   measured  — `offset` present.
#
# `chunks` absent is NOT a state: it is omitted when fewer than two chunks clear `MIN_CHUNK_PAIRS`,
# i.e. the program is shorter than ~2x600 s. Chunk drift is undefined there, not zero.
#
# GAIN JOIN. The same reports carry `patch.gaps[*].status.patched.normalize_gain`, so -GainCsvOut
# emits one row per patched gap with the pair's offset beside the gap's own boost. Expected
# direction, if the offset means anything: a pair whose B is quieter than A (`offset_db` < 0) should
# need more boosting. The census reports the data and does not assert the relationship.
#
# MEDIA HYGIENE: pair labels come from filenames and nothing else is read from the path. Use numeric
# pair indices as labels — never titles or media paths, in the manifest or in anything quoted from
# this output. See docs/dev/repair-perf.md §"Media handling".

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$DumpDir,

    # Per-pair CSV: label, state, offset_db, spread_db, chunk drift, counts, yields.
    [string]$CsvOut = '',

    # Per-patched-gap CSV joining `normalize_gain` to the pair's offset.
    [string]$GainCsvOut = '',

    # Reporting cut for "a single scalar plausibly exists" — NOT a gate, and not a validated
    # threshold. Pairs are counted either side of it so the distribution is visible.
    [double]$SpreadDb = 1.0,

    # Reporting cut for chunk drift (max - min across `chunks`). Same caveat.
    [double]$DriftDb = 1.0
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

$files = @(Get-ChildItem -LiteralPath $DumpDir -Filter '*.json' -File | Sort-Object Name)
if ($files.Count -eq 0) {
    throw "no *.json under $DumpDir — expected repair reports from measure-patch-outcomes.ps1"
}

$rows = [System.Collections.Generic.List[object]]::new()
$gainRows = [System.Collections.Generic.List[object]]::new()

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
    $row = [ordered]@{
        pair                = $label
        state               = 'stale'
        offset_db           = $null
        spread_db           = $null
        p10_db              = $null
        p90_db              = $null
        chunk_drift_db      = $null
        robust_drift_db     = $null
        min_chunk_pairs     = $null
        thin_chunks         = $null
        n_chunks            = $null
        n_pairs             = $null
        n_steady            = $null
        n_active            = $null
        pairs_per_steady    = $null
        steady_per_active   = $null
        channels_comparable = $null
    }

    $offset = $null
    if ($null -ne $levels) {
        $offset = Get-Node $levels 'offset'
        if ($null -eq (Get-Node $levels 'b')) {
            $row.state = 'unaligned'
        } elseif ($null -eq $offset) {
            $row.state = 'abstained'
        } else {
            $row.state = 'measured'
        }
    }

    if ($null -ne $offset) {
        $row.offset_db = [double](Get-Node $offset 'offset_db')
        $row.spread_db = [double](Get-Node $offset 'spread_db')
        $row.p10_db = Get-Node $offset 'p10_db'
        $row.p90_db = Get-Node $offset 'p90_db'
        $row.n_pairs = [int](Get-Node $offset 'n_pairs')
        $row.n_steady = [int](Get-Node $offset 'n_steady')
        $row.n_active = [int](Get-Node $offset 'n_active')
        $row.channels_comparable = [bool](Get-Node $offset 'channels_comparable')

        if ($row.n_steady -gt 0) {
            $row.pairs_per_steady = $row.n_pairs / $row.n_steady
        }
        if ($row.n_active -gt 0) {
            $row.steady_per_active = $row.n_steady / $row.n_active
        }

        # Absent `chunks` means "one chunk cleared MIN_CHUNK_PAIRS", so drift is undefined rather
        # than zero — a short program must not be counted as evidence of a constant offset.
        $chunksNode = Get-Node $offset 'chunks'
        $chunks = if ($null -eq $chunksNode) { @() } else { @($chunksNode) }
        # Left $null, not 0: the field is omitted below two chunks, so its count is unknown here.
        if ($chunks.Count -ge 2) {
            $row.n_chunks = $chunks.Count
            $chunkDbs = @($chunks | ForEach-Object { [double]$_.offset_db })
            $row.chunk_drift_db = ($chunkDbs | Measure-Object -Maximum).Maximum - ($chunkDbs | Measure-Object -Minimum).Minimum

            # Drift is max-minus-min, so it is set by whichever chunk is *least* reliable — and
            # `MIN_CHUNK_PAIRS` (20) is nowhere near strict enough to protect a range statistic. A
            # single thin end-of-program chunk reads as a drifting offset. Report the weakest
            # population alongside the drift, and a second drift computed over the chunks that carry
            # at least half the pair's median chunk population.
            $counts = @($chunks | ForEach-Object { [int]$_.n_pairs } | Sort-Object)
            $row.min_chunk_pairs = $counts[0]
            $medianCount = $counts[[int][Math]::Floor(($counts.Count - 1) / 2)]
            $bar = $medianCount / 2.0
            $solid = @($chunks | Where-Object { [int]$_.n_pairs -ge $bar })
            $row.thin_chunks = $chunks.Count - $solid.Count
            if ($solid.Count -ge 2) {
                $solidDbs = @($solid | ForEach-Object { [double]$_.offset_db })
                $row.robust_drift_db = ($solidDbs | Measure-Object -Maximum).Maximum - ($solidDbs | Measure-Object -Minimum).Minimum
            }
        }
    }

    $rows.Add([pscustomobject]$row)

    # Gain join: one row per *patched* gap, carrying the pair's offset for comparison.
    foreach ($gap in @(Get-Node (Get-Node $report 'patch') 'gaps')) {
        $patched = Get-Node (Get-Node $gap 'status') 'patched'
        if ($null -eq $patched) { continue }
        $gain = Get-Node $patched 'normalize_gain'
        if ($null -eq $gain) { continue }
        $gainDb = if ([double]$gain -gt 0) { 20.0 * [Math]::Log10([double]$gain) } else { $null }
        $gainRows.Add([pscustomobject][ordered]@{
                pair            = $label
                a_start_secs    = Get-Node $gap 'a_start_secs'
                normalize_gain  = [double]$gain
                gain_db         = $gainDb
                pair_offset_db  = $row.offset_db
                pair_spread_db  = $row.spread_db
            })
    }
}

$byState = $rows | Group-Object state | Sort-Object Name
$measured = @($rows | Where-Object { $_.state -eq 'measured' })

Write-Host ''
Write-Host "Program-level census over $DumpDir"
Write-Host ("  reports: {0}" -f $rows.Count)
foreach ($group in $byState) {
    Write-Host ("  {0,-10} {1}" -f $group.Name, $group.Count)
}

$stale = @($rows | Where-Object { $_.state -eq 'stale' })
if ($stale.Count -gt 0) {
    Write-Host ''
    Write-Warning ("{0} report(s) carry no scan.program_levels at all — those are STALE BINARY runs, not corpus findings. Rebuild and re-run before reading anything below as a corpus result: {1}" -f $stale.Count, (($stale.pair) -join ', '))
}

if ($measured.Count -gt 0) {
    Write-Host ''
    $tableFormat = '{0,-12} {1,8} {2,8} {3,8} {4,8} {5,6} {6,5} {7,8} {8,7} {9,7} {10}'
    Write-Host ($tableFormat -f `
            'pair', 'offset', 'spread', 'drift', 'drift*', 'chunks', 'thin', 'n_pairs', 'pr/std', 'std/act', 'ch')
    Write-Host '  drift = max-min across chunks; drift* = same over chunks holding >= half the median chunk population; thin = chunks below that bar'
    foreach ($row in $measured) {
        Write-Host ($tableFormat -f `
                $row.pair,
            (Format-Db $row.offset_db),
            (Format-Ratio $row.spread_db),
            $(if ($null -eq $row.chunk_drift_db) { '-' } else { Format-Ratio $row.chunk_drift_db }),
            $(if ($null -eq $row.robust_drift_db) { '-' } else { Format-Ratio $row.robust_drift_db }),
            $(if ($null -eq $row.n_chunks) { '-' } else { $row.n_chunks }),
            $(if ($null -eq $row.thin_chunks) { '-' } else { $row.thin_chunks }),
            $row.n_pairs,
            (Format-Ratio $row.pairs_per_steady),
            (Format-Ratio $row.steady_per_active),
            $(if ($row.channels_comparable) { 'ok' } else { 'MISMATCH' }))
    }

    $spreads = @($measured | ForEach-Object { $_.spread_db } | Sort-Object)
    $medianSpread = $spreads[[int][Math]::Floor(($spreads.Count - 1) / 2)]
    $tightCount = @($measured | Where-Object { $_.spread_db -le $SpreadDb }).Count
    $withDrift = @($measured | Where-Object { $null -ne $_.chunk_drift_db })
    $steadyRatios = @($measured | Where-Object { $null -ne $_.steady_per_active } | ForEach-Object { $_.steady_per_active } | Sort-Object)

    Write-Host ''
    Write-Host 'Does a single scalar exist?'
    Write-Host ("  median spread_db          {0}" -f (Format-Ratio $medianSpread))
    Write-Host ("  spread <= {0,-5} dB        {1} of {2}" -f $SpreadDb, $tightCount, $measured.Count)
    if ($withDrift.Count -gt 0) {
        $steadyDrift = @($withDrift | Where-Object { $_.chunk_drift_db -le $DriftDb }).Count
        Write-Host ("  chunk drift <= {0,-5} dB   {1} of {2} multi-chunk pair(s)" -f $DriftDb, $steadyDrift, $withDrift.Count)
        $withRobust = @($withDrift | Where-Object { $null -ne $_.robust_drift_db })
        if ($withRobust.Count -gt 0) {
            $robustOk = @($withRobust | Where-Object { $_.robust_drift_db -le $DriftDb }).Count
            Write-Host ("    ... dropping thin chunks   {0} of {1}" -f $robustOk, $withRobust.Count)
        }
        # A pair that only clears the bar once the thin chunks go is an artifact of the range
        # statistic, not a steady offset — name it rather than let it hide inside the improved count.
        $rescued = @($withDrift | Where-Object {
                $_.chunk_drift_db -gt $DriftDb -and $null -ne $_.robust_drift_db -and $_.robust_drift_db -le $DriftDb
            })
        if ($rescued.Count -gt 0) {
            Write-Host ("    drift was a thin chunk in: {0}" -f (($rescued.pair) -join ', '))
        }
    } else {
        Write-Host '  chunk drift               no pair spans two chunks — drift is untested, not zero'
    }
    $mismatch = @($measured | Where-Object { -not $_.channels_comparable }).Count
    if ($mismatch -gt 0) {
        Write-Host ("  channel mismatch          {0} pair(s) — offset is not a pure gain there" -f $mismatch)
    }
    if ($steadyRatios.Count -gt 0) {
        Write-Host ("  median n_steady/n_active  {0}   (Tier 2 saw 0.350 music / 0.268 speech)" -f (Format-Ratio $steadyRatios[[int][Math]::Floor(($steadyRatios.Count - 1) / 2)]))
    }
}

if ($gainRows.Count -gt 0) {
    $boosts = @($gainRows | Where-Object { $null -ne $_.gain_db -and $_.gain_db -gt 0.01 })
    Write-Host ''
    Write-Host 'Normalize-gain join'
    Write-Host ("  patched gaps with a gain  {0}" -f $gainRows.Count)
    Write-Host ("  boosted (> 0.01 dB)       {0}" -f $boosts.Count)
    if ($boosts.Count -gt 0) {
        $maxBoost = ($boosts | Measure-Object -Property gain_db -Maximum).Maximum
        Write-Host ("  largest boost             {0} dB" -f (Format-Ratio $maxBoost))
        $withOffset = @($boosts | Where-Object { $null -ne $_.pair_offset_db })
        if ($withOffset.Count -gt 0) {
            $quieterB = @($withOffset | Where-Object { $_.pair_offset_db -lt 0 }).Count
            Write-Host ("  of those, pairs with B quieter than A (offset < 0): {0} of {1}" -f $quieterB, $withOffset.Count)
            Write-Host '  (direction only — the census does not assert the relationship)'
        }
    }
}

if ($CsvOut) {
    $rows | Export-Csv -LiteralPath $CsvOut -NoTypeInformation
    Write-Host ''
    Write-Host "per-pair CSV -> $CsvOut"
}
if ($GainCsvOut) {
    $gainRows | Export-Csv -LiteralPath $GainCsvOut -NoTypeInformation
    Write-Host "gain-join CSV -> $GainCsvOut"
}
Write-Host ''
