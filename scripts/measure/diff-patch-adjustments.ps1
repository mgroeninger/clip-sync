#!/usr/bin/env pwsh
#Requires -Version 7.0
# Cross-arm placement differential for the `border_standoff_secs` sweep
# (docs/dev/archive/TEMP-fill-placement-registration-plan.md § "Consequences" (2)).
#
# Asks one question: does the applied placement (`align_adjustment_secs`) move with the
# standoff? With `adj = T + alpha`, `T` is the true registration and `alpha` is the ~25 ms by
# which applied placement sits late (positive on 156/183 gaps). If `alpha` IS the standoff
# holding production's search template off the gap edge, then the same gap patched at two
# standoffs must differ by an amount that scales with the standoff difference. Invariance kills
# the hypothesis; a non-linear response says the standoff perturbs a fit that has its own bias.
#
# JOIN RULE: arm x arm on `patch.gaps[*].a_start_secs`, rounded to 1 ms, within a pair label.
# NEVER on gap index — the arms do not agree on which gaps get patched (see FIT-PATH FLIPS),
# so index i is not the same gap in two arms.
#
# WHAT IS EXCLUDED, AND WHY (all three are counted and reported, never silently dropped):
#   dual-fit   `align_adjustment_secs` is 0.0 by construction on a dual-fit gap regardless of
#              the slide actually applied (the real slide lands in
#              `residual.placement_slide_frames`). A gap dual-fit in ANY arm is excluded from
#              every arm — differencing a real adjustment against a structural 0.0 manufactures
#              a delta the size of the adjustment.
#   unpatched  No arm patched it (not_planned everywhere). Never in scope — counted apart from the
#              flips, because folding it in would inflate the one number that says the standoff
#              changed a decision.
#   fit-path   The standoff changes which path a gap takes, not just where it lands: the s0 arm
#              flipped `dual_fit_used` true->false on several gaps and moved pair 11 from 13 to
#              14 patched. A gap whose status differs across arms is a different DECISION, not a
#              placement delta. Excluded from the pool, reported separately — a median fitted
#              over silently-included flips is meaningless.
#   unjoined   A patched gap with no 1 ms match in another arm (arms disagree on the gap set).
#
# ARM GEOMETRY: each arm's standoff is read from `patch.fill_geometry.border_standoff_secs`
# (stamped since 2026-08-17) so an arm is identified by its own JSON rather than by the
# directory it landed in — a mis-applied `--border-standoff-secs` would otherwise produce a
# plausible-looking arm indistinguishable from a correct one. Arms written before that stamp
# carry no geometry; supply `-Standoffs` in arm order to analyse them, and the script warns
# that the value is asserted, not recorded. If arms differ in a geometry field OTHER than the
# standoff, the attribution is not clean and the script says so.
#
# Usage:
#   ./scripts/measure/diff-patch-adjustments.ps1 `
#       -Arms gap-files/2026-08-14-standoff/s0, gap-files/2026-08-17-standoff/s35
#   ... -Arms .../s0, .../s35, .../s70 -PairTable
#   ... -Standoffs 0,0.35          # arms predating the fill_geometry stamp
#   ... -CsvOut gap-files/standoff-delta.csv
#
# Layout: `<arm>/<pair>.json` — a `--patch-only --format json` report per pair, as written by
# measure-patch-outcomes.ps1. The FIRST arm is the baseline every other arm is differenced
# against.
#
# MEDIA HYGIENE: emit pair label + gap index only — never media paths or titles.
# Docs: docs/dev/archive/TEMP-fill-placement-registration-plan.md.

[CmdletBinding()]
param(
    # Two or more arm directories, each `<arm>/<pair>.json`. First = baseline.
    [Parameter(Mandatory = $true)]
    [string[]]$Arms,

    # Standoff (seconds) per arm, in arm order. Only for arms written before the 2026-08-17
    # `fill_geometry` stamp — a stamped arm reads its own value and disagreement is an error.
    [double[]]$Standoffs = @(),

    # Restrict to these pair labels (default: every pair present in ALL arms).
    [int[]]$Pairs = @(),

    # Drop these pair labels (duplicate media: oriented corpus 6=22, 16=28).
    [int[]]$ExcludePairs = @(),

    # Per-pair delta table.
    [switch]$PairTable,

    # Print every excluded gap, not just the counts.
    [switch]$ShowExclusions,

    # Cap on the fit-path flip list printed to the console (CSV always carries every gap).
    [int]$FlipLimit = 15,

    # Per-gap CSV: pair, gap_index, a_start_secs, adj per arm, delta per arm, exclusion reason.
    [string]$CsvOut = ''
)

$ErrorActionPreference = 'Stop'

if ($Arms.Count -lt 2) { throw 'need at least two -Arms (first is the baseline)' }
if ($Standoffs.Count -gt 0 -and $Standoffs.Count -ne $Arms.Count) {
    throw ("-Standoffs has {0} value(s) for {1} arm(s); supply one per arm, in arm order" -f $Standoffs.Count, $Arms.Count)
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
    # 1 ms resolution, culture-invariant, so two arms agree on the join key.
    return ([math]::Round([double]$Secs, 3)).ToString('F3', [cultureinfo]::InvariantCulture)
}

function Test-HasProp([object]$Obj, [string]$Name) {
    if ($null -eq $Obj) { return $false }
    return $null -ne $Obj.PSObject.Properties[$Name]
}

function Get-StatusName([object]$Status) {
    # GapPatchStatus is an externally tagged enum: { "patched": {...} } / { "skipped": {...} }.
    if ($null -eq $Status) { return 'absent' }
    $p = @($Status.PSObject.Properties)
    if ($p.Count -eq 0) { return 'absent' }
    return $p[0].Name
}

function Get-ArmIndex([string]$Dir) {
    # .Pairs     label -> { start_key -> @{ Idx; Status; AdjMs; Dual; Confidence } } over ALL gaps
    # .Geometry  the arm's single `patch.fill_geometry`, or $null when unstamped
    # .Files     count of <pair>.json read
    $out = @{}
    $geo = $null
    $geoPair = $null
    $files = 0
    Get-ChildItem -LiteralPath $Dir -File -Filter '*.json' -ErrorAction SilentlyContinue |
        Where-Object { $_.BaseName -match '^\d+$' } |
        ForEach-Object {
            $label = [int]$_.BaseName
            $root = Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json
            if ($null -eq $root.patch) { return }
            $files++

            if (Test-HasProp $root.patch 'fill_geometry') {
                $g = $root.patch.fill_geometry
                if ($null -eq $geo) { $geo = $g; $geoPair = $label }
                else {
                    foreach ($f in @($geo.PSObject.Properties.Name)) {
                        if ([double]$g.$f -ne [double]$geo.$f) {
                            throw ("arm '{0}' is not one arm: pair {1} was patched at {2}={3} but pair {4} at {5}. Each arm must be a single geometry." -f `
                                    $Dir, $label, $f, $g.$f, $geoPair, $geo.$f)
                        }
                    }
                }
            }

            $byStart = @{}
            $i = 0
            foreach ($gap in @($root.patch.gaps)) {
                if ($null -eq $gap) { continue }
                $name = Get-StatusName $gap.status
                $s = $gap.status.patched
                $adj = $null
                $dual = $false
                $conf = ''
                if ($null -ne $s) {
                    $adj = $(if ($null -ne $s.align_adjustment_secs) { [double]$s.align_adjustment_secs * 1000.0 } else { 0.0 })
                    $dual = [bool]$s.dual_fit_used
                    $conf = [string]$s.confidence
                }
                $byStart[(Get-StartKey $gap.a_start_secs)] = @{
                    Idx        = $i
                    Status     = $name
                    AdjMs      = $adj
                    Dual       = $dual
                    Confidence = $conf
                }
                $i++
            }
            if ($byStart.Count -gt 0) { $out[$label] = $byStart }
        }
    return [pscustomobject]@{ Pairs = $out; Geometry = $geo; Files = $files }
}

# --- load arms -------------------------------------------------------------

$armInfo = [System.Collections.Generic.List[object]]::new()
for ($a = 0; $a -lt $Arms.Count; $a++) {
    $dir = $Arms[$a]
    if (-not (Test-Path -LiteralPath $dir)) { throw "arm directory not found: $dir" }
    $full = (Resolve-Path -LiteralPath $dir).Path
    $ix = Get-ArmIndex -Dir $full
    if ($ix.Pairs.Count -eq 0) { throw "no <pair>.json patch reports under $full" }

    $stand = $null
    $source = ''
    if ($null -ne $ix.Geometry -and (Test-HasProp $ix.Geometry 'border_standoff_secs')) {
        $stand = [double]$ix.Geometry.border_standoff_secs
        $source = 'recorded'
        if ($Standoffs.Count -gt 0 -and [math]::Abs($Standoffs[$a] - $stand) -gt 1e-9) {
            throw ("-Standoffs says arm '{0}' ran at {1} s but its own fill_geometry records {2} s. The report wins; drop the override." -f `
                    (Split-Path -Leaf $full), $Standoffs[$a], $stand)
        }
    }
    elseif ($Standoffs.Count -gt 0) {
        $stand = $Standoffs[$a]
        $source = 'asserted'
    }

    $armInfo.Add([pscustomobject]@{
            Index    = $a
            Label    = (Split-Path -Leaf $full)
            Dir      = $full
            Pairs    = $ix.Pairs
            Geometry = $ix.Geometry
            Standoff = $stand
            Source   = $source
        })
}

$baseline = $armInfo[0]

# Geometry attribution: the standoff is allowed to differ (that IS the sweep); anything else
# differing means a delta cannot be attributed to the standoff.
$stamped = @($armInfo | Where-Object { $null -ne $_.Geometry })
if ($stamped.Count -lt $armInfo.Count) {
    $bare = @($armInfo | Where-Object { $null -eq $_.Geometry } | ForEach-Object { $_.Label })
    Write-Warning ("arm(s) {0} carry no patch.fill_geometry (written before 2026-08-17). Their geometry is {1}." -f `
        ($bare -join ', '), $(if ($Standoffs.Count -gt 0) { 'ASSERTED via -Standoffs, not recorded' } else { 'unknown — pass -Standoffs to analyse them' }))
}
if ($stamped.Count -ge 2) {
    $ref = $stamped[0]
    foreach ($f in @($ref.Geometry.PSObject.Properties.Name)) {
        if ($f -eq 'border_standoff_secs') { continue }
        foreach ($other in ($stamped | Select-Object -Skip 1)) {
            if (-not (Test-HasProp $other.Geometry $f)) { continue }
            if ([double]$other.Geometry.$f -ne [double]$ref.Geometry.$f) {
                Write-Warning ("arms differ in {0} ({1}={2} vs {3}={4}) as well as the standoff — a delta is NOT attributable to the standoff alone." -f `
                        $f, $ref.Label, $ref.Geometry.$f, $other.Label, $other.Geometry.$f)
            }
        }
    }
}
$knownStand = @($armInfo | Where-Object { $null -ne $_.Standoff })
if ($knownStand.Count -eq $armInfo.Count) {
    $dupes = @($knownStand | Group-Object -Property Standoff | Where-Object { $_.Count -gt 1 })
    foreach ($d in $dupes) {
        Write-Warning ("arms {0} all ran at standoff {1} s — the flag did not vary between them." -f `
            (@($d.Group | ForEach-Object { $_.Label }) -join ', '), $d.Name)
    }
}

# --- pair selection --------------------------------------------------------

$restrict = @($baseline.Pairs.Keys)
foreach ($arm in $armInfo) { $restrict = @($restrict | Where-Object { $arm.Pairs.ContainsKey($_) }) }
if ($Pairs.Count -gt 0) { $restrict = @($restrict | Where-Object { $Pairs -contains $_ }) }
if ($ExcludePairs.Count -gt 0) { $restrict = @($restrict | Where-Object { $ExcludePairs -notcontains $_ }) }
$restrict = @($restrict | Sort-Object)
if ($restrict.Count -eq 0) { throw 'no pair label is present in every arm after -Pairs / -ExcludePairs' }

if ($Pairs.Count -gt 0) {
    $missing = @($Pairs | Where-Object { $restrict -notcontains $_ })
    if ($missing.Count -gt 0) {
        throw ("requested -Pairs missing from at least one arm: {0}" -f ($missing -join ', '))
    }
}

# --- join ------------------------------------------------------------------

$rows = [System.Collections.Generic.List[object]]::new()
foreach ($pr in $restrict) {
    # Union of start keys across arms: a gap patched in one arm only is a flip, not an absence,
    # and has to be visible rather than dropped by iterating the baseline alone.
    $keys = [System.Collections.Generic.HashSet[string]]::new()
    foreach ($arm in $armInfo) {
        foreach ($k in $arm.Pairs[$pr].Keys) { [void]$keys.Add($k) }
    }

    foreach ($start in (@($keys) | Sort-Object { [double]$_ })) {
        $cells = @{}
        $idx = $null
        foreach ($arm in $armInfo) {
            $e = $arm.Pairs[$pr][$start]
            $cells[$arm.Index] = $e
            if ($null -ne $e -and $null -eq $idx) { $idx = $e.Idx }
        }

        $present = @($armInfo | Where-Object { $null -ne $cells[$_.Index] })
        $patched = @($armInfo | Where-Object { $null -ne $cells[$_.Index] -and $cells[$_.Index].Status -eq 'patched' })
        $dual = @($armInfo | Where-Object { $null -ne $cells[$_.Index] -and $cells[$_.Index].Dual })

        # `unpatched` is NOT a flip: a gap no arm patched (not_planned everywhere) was never in scope,
        # and counting it as a fit-path change would inflate the one number that says the standoff
        # moved a decision. A flip is a gap the arms DISAGREE about.
        $reason = ''
        if ($present.Count -lt $armInfo.Count) { $reason = 'unjoined' }
        elseif ($patched.Count -eq 0) { $reason = 'unpatched' }
        elseif ($patched.Count -lt $armInfo.Count) { $reason = 'fit_path' }
        elseif ($dual.Count -eq $armInfo.Count) { $reason = 'dual_fit' }
        elseif ($dual.Count -gt 0) { $reason = 'fit_path' }

        $statuses = @($armInfo | ForEach-Object {
                $e = $cells[$_.Index]
                $(if ($null -eq $e) { 'absent' } elseif ($e.Dual) { $e.Status + '+dual' } else { $e.Status })
            })

        $adj = @($armInfo | ForEach-Object { $(if ($null -ne $cells[$_.Index]) { $cells[$_.Index].AdjMs } else { $null }) })
        $deltas = @(for ($a = 1; $a -lt $armInfo.Count; $a++) {
                $(if ($reason -eq '' -and $null -ne $adj[$a] -and $null -ne $adj[0]) { $adj[$a] - $adj[0] } else { $null })
            })

        $rows.Add([pscustomobject]@{
                Pair     = $pr
                Idx      = $idx
                StartKey = $start
                Adj      = $adj
                Deltas   = $deltas
                Statuses = $statuses
                Reason   = $reason
                Pooled   = ($reason -eq '')
            })
    }
}

$pooled = @($rows | Where-Object { $_.Pooled })
$exDual = @($rows | Where-Object { $_.Reason -eq 'dual_fit' })
$exFlip = @($rows | Where-Object { $_.Reason -eq 'fit_path' })
$exJoin = @($rows | Where-Object { $_.Reason -eq 'unjoined' })
$exNone = @($rows | Where-Object { $_.Reason -eq 'unpatched' })

# --- report ----------------------------------------------------------------

Write-Host ''
Write-Host 'Cross-arm placement differential (border_standoff_secs sweep)' -ForegroundColor Green
'{0,-16} {1,11} {2,10} {3,7} {4,9}' -f 'arm', 'standoff_s', 'geometry', 'pairs', 'gaps' | Write-Host
Write-Host ('-' * 58)
foreach ($arm in $armInfo) {
    $st = $(if ($null -ne $arm.Standoff) { '{0:N3}' -f $arm.Standoff } else { '?' })
    $src = $(if ($arm.Source -ne '') { $arm.Source } else { 'unknown' })
    $gaps = (@($arm.Pairs.Values | ForEach-Object { $_.Count }) | Measure-Object -Sum).Sum
    $tag = $(if ($arm.Index -eq 0) { ' (baseline)' } else { '' })
    Write-Host (('{0,-16} {1,11} {2,10} {3,7} {4,9}' -f $arm.Label, $st, $src, $arm.Pairs.Count, $gaps) + $tag)
}
Write-Host ("Analysed: {0} pair(s) present in every arm{1}" -f $restrict.Count,
    $(if ($ExcludePairs.Count -gt 0) { " (excluded: $($ExcludePairs -join ', '))" } else { '' }))

Write-Host ''
Write-Host ("Pool: {0} gap(s) patched non-dual in EVERY arm" -f $pooled.Count) -ForegroundColor Cyan
Write-Host ("Excluded: {0} fit-path flip (arms disagree), {1} dual-fit in every arm (adj is structurally 0.0), {2} patched by no arm, {3} unjoined" -f `
        $exFlip.Count, $exDual.Count, $exNone.Count, $exJoin.Count)

if ($pooled.Count -eq 0) {
    throw 'no gap is patched non-dual in every arm — nothing to difference'
}

Write-Host ''
Write-Host 'Per-arm delta vs baseline (signed, arm - baseline)' -ForegroundColor Cyan
'{0,-16} {1,6} {2,12} {3,12} {4,14} {5,16}' -f 'arm', 'n', 'median_ms', 'mean_ms', 'sign', 'slope_ms_per_s' | Write-Host
Write-Host ('-' * 82)
$slopes = [System.Collections.Generic.List[object]]::new()
for ($a = 1; $a -lt $armInfo.Count; $a++) {
    $arm = $armInfo[$a]
    $ds = @($pooled | ForEach-Object { $_.Deltas[$a - 1] } | Where-Object { $null -ne $_ })
    $med = Get-Median $ds
    $mean = (@($ds) | Measure-Object -Average).Average
    $neg = @($ds | Where-Object { $_ -lt 0 }).Count
    $pos = @($ds | Where-Object { $_ -gt 0 }).Count
    $slope = $null
    if ($null -ne $arm.Standoff -and $null -ne $baseline.Standoff) {
        # NOT `$dS`: PowerShell variable names are case-insensitive, so `$dS` IS `$ds` and the
        # standoff difference would overwrite the delta sample this row is reporting.
        $standoffDelta = $arm.Standoff - $baseline.Standoff
        if ([math]::Abs($standoffDelta) -gt 1e-9 -and $null -ne $med) { $slope = $med / $standoffDelta }
    }
    $slopes.Add([pscustomobject]@{ Arm = $arm; Median = $med; Slope = $slope; N = $ds.Count })
    '{0,-16} {1,6} {2,12} {3,12} {4,14} {5,16}' -f `
        $arm.Label, $ds.Count,
    $(if ($null -ne $med) { '{0:+0.0;-0.0}' -f $med } else { '-' }),
    $(if ($null -ne $mean) { '{0:+0.0;-0.0}' -f $mean } else { '-' }),
    ('{0}-/{1}+' -f $neg, $pos),
    $(if ($null -ne $slope) { '{0:+0.0;-0.0}' -f $slope } else { '-' }) | Write-Host
}

# Linearity: with three or more standoffs, a constant slope says the standoff IS the bias; a
# collapsing slope says it only perturbs a fit that carries its own.
$withSlope = @($slopes | Where-Object { $null -ne $_.Slope })
if ($withSlope.Count -ge 2) {
    $vals = @($withSlope | ForEach-Object { $_.Slope })
    $spread = ([math]::Abs((($vals | Measure-Object -Maximum).Maximum) - (($vals | Measure-Object -Minimum).Minimum)))
    $scale = ($vals | ForEach-Object { [math]::Abs($_) } | Measure-Object -Maximum).Maximum
    $rel = $(if ($scale -gt 0) { $spread / $scale } else { 0.0 })
    Write-Host ''
    if ($rel -le 0.25) {
        Write-Host ("Linearity: slopes agree within {0:P0} — response is proportional to the standoff (alpha IS the standoff)." -f $rel) -ForegroundColor Green
    }
    else {
        Write-Host ("Linearity: slopes spread {0:P0} — response is NOT proportional; the standoff perturbs a fit that carries its own bias." -f $rel) -ForegroundColor Yellow
    }
}
elseif ($armInfo.Count -eq 2) {
    Write-Host ''
    Write-Host 'Linearity: needs a third standoff — two arms fix a slope but cannot test proportionality.' -ForegroundColor DarkGray
}

if ($exFlip.Count -gt 0) {
    Write-Host ''
    Write-Host ("Fit-path flips ({0}) — a different DECISION, not a placement delta" -f $exFlip.Count) -ForegroundColor Yellow
    $shown = @($exFlip | Select-Object -First $FlipLimit | ForEach-Object {
            '{0}/g{1} [{2}]' -f $_.Pair, $_.Idx, ($_.Statuses -join ' -> ')
        })
    Write-Host ('  ' + ($shown -join ', '))
    if ($exFlip.Count -gt $FlipLimit) { Write-Host ("  … and {0} more (see -CsvOut)" -f ($exFlip.Count - $FlipLimit)) }
}

if ($ShowExclusions -and ($exDual.Count + $exJoin.Count + $exNone.Count) -gt 0) {
    Write-Host ''
    Write-Host 'Other exclusions' -ForegroundColor DarkGray
    foreach ($r in @($exDual + $exJoin + $exNone)) {
        Write-Host ('  {0}/g{1} {2} [{3}]' -f $r.Pair, $r.Idx, $r.Reason, ($r.Statuses -join ' | '))
    }
}

if ($PairTable) {
    Write-Host ''
    Write-Host 'Per-pair median delta vs baseline' -ForegroundColor Cyan
    $hdr = '{0,5} {1,7}' -f 'pair', 'n'
    foreach ($arm in ($armInfo | Select-Object -Skip 1)) { $hdr += ('{0,14}' -f $arm.Label) }
    Write-Host $hdr
    Write-Host ('-' * (12 + 14 * ($armInfo.Count - 1)))
    foreach ($pr in $restrict) {
        $mine = @($pooled | Where-Object { $_.Pair -eq $pr })
        $line = '{0,5} {1,7}' -f $pr, $mine.Count
        for ($a = 1; $a -lt $armInfo.Count; $a++) {
            $m = Get-Median @($mine | ForEach-Object { $_.Deltas[$a - 1] })
            $line += ('{0,14}' -f $(if ($null -ne $m) { '{0:+0.0;-0.0}' -f $m } else { '-' }))
        }
        Write-Host $line
    }
}

if ($CsvOut -ne '') {
    $csvDir = Split-Path -Parent $CsvOut
    if ($csvDir -and -not (Test-Path -LiteralPath $csvDir)) {
        New-Item -ItemType Directory -Force -Path $csvDir | Out-Null
    }
    $out = [System.Collections.Generic.List[object]]::new()
    foreach ($r in $rows) {
        $rec = [ordered]@{
            pair         = $r.Pair
            gap_index    = $r.Idx
            a_start_secs = $r.StartKey
            pooled       = $r.Pooled
            excluded     = $r.Reason
        }
        for ($a = 0; $a -lt $armInfo.Count; $a++) {
            $lbl = $armInfo[$a].Label
            $rec["adj_ms_$lbl"] = $(if ($null -ne $r.Adj[$a]) { [math]::Round([double]$r.Adj[$a], 3) } else { '' })
            $rec["status_$lbl"] = $r.Statuses[$a]
            if ($a -gt 0) {
                $rec["delta_ms_$lbl"] = $(if ($null -ne $r.Deltas[$a - 1]) { [math]::Round([double]$r.Deltas[$a - 1], 3) } else { '' })
            }
        }
        $out.Add([pscustomobject]$rec)
    }
    $out | Sort-Object pair, gap_index |
        Export-Csv -LiteralPath $CsvOut -NoTypeInformation -Encoding utf8
    Write-Host ''
    Write-Host "CSV: $CsvOut ($($out.Count) row(s))" -ForegroundColor DarkGray
}

Write-Host ''
Write-Host 'Report-only analysis — no repair re-run. Plan: docs/dev/archive/TEMP-fill-placement-registration-plan.md' `
    -ForegroundColor DarkGray
