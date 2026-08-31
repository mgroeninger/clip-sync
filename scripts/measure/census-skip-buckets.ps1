#!/usr/bin/env pwsh
#Requires -Version 7.0
# Dump-only census of the rigid-shoulder-offset skip class
# (docs/dev/TEMP-rigid-shoulder-offset-skips-2026-08-31.md § *Class predicate*). No gate change.
#
# Emits ONE ROW PER SKIPPED GAP — every bucket, not just the class — carrying each predicate leg's
# RAW INPUT beside the bucket it lands in. That shape is the point: the BACKLOG row's original
# "95 / 644" is unrecoverable because the pass that produced it emitted counts, not rows, so the
# 95-vs-90 delta could only be narrowed (to the donor leg, 11 gaps) and never re-derived. With rows,
# a future disagreement is a diff on the `bucket` column instead of another forensic dig.
#
# Consumer only — opens no media, runs no repair, needs no oracle. Point it at fingerprint-corpus
# JSON that `measure-gap-fingerprints.ps1` already wrote.
#
# WHAT THIS CANNOT ANSWER. Question 2 (would the keep path patch the remainder after a map
# shift) re-runs evaluate_seam_gate on a shifted B clock. No dump carries that outcome; it is
# the Phase 1 overlay media run. This census sizes and names the class — nothing more.
#
# Usage:
#   ./scripts/measure/census-skip-buckets.ps1 -DumpDir gap-files/<date>-complete-corpus
#   ./scripts/measure/census-skip-buckets.ps1 -DumpDir gap-files/<date>-complete-corpus -CsvOut gap-files/skip-census.csv
#   ./scripts/measure/census-skip-buckets.ps1 -DumpDir gap-files/<date>-complete-corpus -ListClass
#
# Accepted layout: `<DumpDir>/<pair>/` fingerprint corpora. Per pair the canonical
# `<pair>/corpus.json` is read when present — it carries every gap, both detail tiers — and that
# pair's per-gap `*_full.json` / `*_summary.json` siblings are then skipped as duplicates. A pair
# directory without a `corpus.json` (a `--fingerprint-gap`-filtered targeted dump) falls back to its
# per-gap files. `manifest.json` / `listen.json` are always skipped; the latter is `--gap-listen`'s
# sibling manifest, which lands in the same directory when the WAV root defaults to the JSON root.
# Repair / patch-outcomes dumps are NOT accepted: `lag_decision` and `donor_interior_nominal` live
# in the fingerprint corpus.
#
# DENOMINATORS. `summary`-tier gaps carry no `outcome` at all — they were never characterized
# pairwise — so they can be neither skip nor patch. On the 2026-08-29 corpus that is 1073 gaps
# total, 1049 of them `full`-tier, and the 644 skips are all drawn from those 1049. The console
# prints both counts; do not read the skip rate against the 1073.
#
# BUCKETS, KEPT APART, in predicate order (a gap lands in the first that claims it):
#   no_shoulders     — `lag_decision.pre_anchor[0]` or `post_anchor[0]` absent. The 1 s probe never
#                      produced a usable lock on one side; nothing downstream is measurable.
#   donor_absent     — B is silent or discontinuous at the NOMINAL span
#                      (`donor_interior_nominal.continuous` false, or `silence_fraction` > 0).
#                      Correctly refused: there is no donor to register.
#   class            — both shoulders live (`min(peak_r) >= RFloor`) and rigid
#                      (`|post.frac_lag_ms - pre.frac_lag_ms| <= StepMaxMs`) over a present donor.
#                      This is the class the BACKLOG row is about.
#   weak_or_stepped  — live donor, but the shoulders are weak or carry a step. Not this class.
#   unmeasured       — the dump does not carry `donor_interior_nominal` at all (pre-field corpus).
#                      Reported separately rather than folded into `donor_absent`: absence of the
#                      measurement is not evidence the donor is absent.
#
# THRESHOLDS. `-RFloor` / `-StepMaxMs` are the predicate's own knobs and default to the documented
# 0.94 / 25 ms. The GATE thresholds (`min_fill_correlation`, `fill_absolute_floor`) are NOT
# hardcoded — each row echoes its own dump's `source.gate_recipe`, so a corpus written under a
# different recipe reads correctly instead of silently against today's 0.35 / 0.12.
#
# MEDIA HYGIENE: emit pair label + gap index only — never media paths or titles.
# Docs: docs/dev/TEMP-rigid-shoulder-offset-skips-2026-08-31.md.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$DumpDir,

    # Shoulder liveness floor: min(pre.peak_r, post.peak_r) must clear it.
    [double]$RFloor = 0.94,

    # Rigidity tolerance (ms) on |post.frac_lag_ms - pre.frac_lag_ms|.
    [double]$StepMaxMs = 25.0,

    # Optional CSV of every skipped gap (pair, gap_index, bucket, and each leg's raw input).
    [string]$CsvOut = '',

    # Also list the class members on the console (still pair/gap only).
    [switch]$ListClass
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $DumpDir)) {
    throw "dump dir not found: $DumpDir"
}

# `ConvertFrom-Json` yields PSCustomObjects; a missing key is not $null, it is a missing property.
# Distinguishing the two is the whole reason `unmeasured` exists as a bucket.
function Test-Prop([object]$Obj, [string]$Name) {
    if ($null -eq $Obj) { return $false }
    return [bool]($Obj.PSObject.Properties.Name -contains $Name)
}

function Get-Prop([object]$Obj, [string]$Name) {
    if (-not (Test-Prop $Obj $Name)) { return $null }
    return $Obj.$Name
}

# Export-Csv stringifies doubles in the current culture, which writes a decimal COMMA on a
# comma-locale host and silently corrupts the CSV. Format every number invariantly instead.
function Format-Num([object]$Value) {
    if ($null -eq $Value) { return '' }
    if ($Value -is [bool]) { return $Value.ToString() }
    $d = $Value -as [double]
    if ($null -eq $d) { return [string]$Value }
    if ([double]::IsNaN($d)) { return 'NaN' }
    return $d.ToString('R', [System.Globalization.CultureInfo]::InvariantCulture)
}

function Get-MaxSeam([object[]]$Brackets, [string]$Field) {
    $vals = @($Brackets | ForEach-Object { Get-Prop $_ $Field } | Where-Object { $null -ne $_ })
    if ($vals.Count -eq 0) { return $null }
    return ($vals | Measure-Object -Maximum).Maximum
}

$skipNames = @('manifest.json', 'listen.json')
$candidates = @(Get-ChildItem -LiteralPath $DumpDir -Recurse -File -Filter '*.json' |
    Where-Object { $skipNames -notcontains $_.Name })

# Prefer each pair's canonical `corpus.json`; its per-gap siblings repeat the same gaps, so reading
# both would double every count. Pairs without one keep their per-gap files.
$pairOf = {
    param($f)
    ([System.IO.Path]::GetRelativePath($DumpDir, $f.FullName) -split '[\\/]')[0]
}
$canonical = @{}
foreach ($f in $candidates) {
    if ($f.Name -eq 'corpus.json') { $canonical[(& $pairOf $f)] = $true }
}
$files = @($candidates | Where-Object {
    $f = $_
    if ($f.Name -eq 'corpus.json') { return $true }
    return -not $canonical.ContainsKey((& $pairOf $f))
})

if ($files.Count -eq 0) {
    throw "no fingerprint JSON under: $DumpDir"
}

$rows = [System.Collections.Generic.List[object]]::new()
$pairs = [System.Collections.Generic.HashSet[string]]::new()
$totalGaps = 0
$outcomeGaps = 0
$unparsed = 0

foreach ($file in $files) {
    try {
        $doc = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    } catch {
        $unparsed++
        continue
    }
    if (-not (Test-Prop $doc 'gaps')) { continue }

    # Pair label = the directory directly under -DumpDir, matching the rest of docs/ (index only).
    $rel = [System.IO.Path]::GetRelativePath($DumpDir, $file.FullName)
    $pair = ($rel -split '[\\/]')[0]
    [void]$pairs.Add($pair)

    $gate = Get-Prop (Get-Prop $doc 'source') 'gate_recipe'

    foreach ($g in @($doc.gaps)) {
        $totalGaps++
        $outcome = Get-Prop $g 'outcome'
        # `summary`-tier gaps have no `outcome` — never characterized pairwise, so never a skip.
        if ($null -ne $outcome) { $outcomeGaps++ }
        if ((Get-Prop $outcome 'tier') -ne 'skip') { continue }

        $ld = Get-Prop $g 'lag_decision'
        $pre = @(Get-Prop $ld 'pre_anchor')[0]
        $post = @(Get-Prop $ld 'post_anchor')[0]
        $donorMeasured = Test-Prop $g 'donor_interior_nominal'
        $dn = Get-Prop $g 'donor_interior_nominal'
        $df = Get-Prop $g 'splice_dualfit'
        $brackets = @(Get-Prop $g 'brackets')

        $midMs = $null
        $stepMs = $null
        if ($null -ne $pre -and $null -ne $post) {
            $midMs = ([double]$pre.frac_lag_ms + [double]$post.frac_lag_ms) / 2.0
            $stepMs = [double]$post.frac_lag_ms - [double]$pre.frac_lag_ms
        }

        # Predicate order. First bucket that claims the gap wins; see the header.
        if ($null -eq $pre -or $null -eq $post) {
            $bucket = 'no_shoulders'
        } elseif (-not $donorMeasured) {
            $bucket = 'unmeasured'
        } elseif (-not ((Get-Prop $dn 'continuous') -eq $true -and [double](Get-Prop $dn 'silence_fraction') -eq 0.0)) {
            $bucket = 'donor_absent'
        } elseif ([Math]::Min([double]$pre.peak_r, [double]$post.peak_r) -ge $RFloor -and
                  [Math]::Abs($stepMs) -le $StepMaxMs) {
            $bucket = 'class'
        } else {
            $bucket = 'weak_or_stepped'
        }

        $rows.Add([pscustomobject][ordered]@{
            pair                     = $pair
            gap_index                = Get-Prop $g 'index'
            bucket                   = $bucket
            tier                     = Get-Prop $outcome 'tier'
            skip_reason              = Get-Prop $outcome 'skip_reason'
            dual_fit_rescue          = Format-Num (Get-Prop $outcome 'dual_fit_rescue')
            pre_peak_r               = Format-Num (Get-Prop $pre 'peak_r')
            post_peak_r              = Format-Num (Get-Prop $post 'peak_r')
            pre_frac_lag_ms          = Format-Num (Get-Prop $pre 'frac_lag_ms')
            post_frac_lag_ms         = Format-Num (Get-Prop $post 'frac_lag_ms')
            mid_lag_ms               = Format-Num $midMs
            step_ms                  = Format-Num $stepMs
            pre_lag0_r               = Format-Num (Get-Prop $pre 'lag0_r')
            post_lag0_r              = Format-Num (Get-Prop $post 'lag0_r')
            pre_peak_z               = Format-Num (Get-Prop $pre 'peak_z')
            post_peak_z              = Format-Num (Get-Prop $post 'peak_z')
            pre_edge_pinned          = Format-Num (Get-Prop $pre 'edge_pinned')
            post_edge_pinned         = Format-Num (Get-Prop $post 'edge_pinned')
            pre_envelope_rescue      = Get-Prop $pre 'envelope_rescue'
            post_envelope_rescue     = Get-Prop $post 'envelope_rescue'
            donor_measured           = Format-Num $donorMeasured
            donor_continuous         = Format-Num (Get-Prop $dn 'continuous')
            donor_silence_fraction   = Format-Num (Get-Prop $dn 'silence_fraction')
            donor_longest_silence_ms = Format-Num (Get-Prop $dn 'longest_silence_ms')
            donor_rms_db             = Format-Num (Get-Prop $dn 'rms_db')
            bracket_count            = $brackets.Count
            # Independent maxima over the bracket list — the sweep's ceiling per side, NOT one
            # bracket's pair. Do not read the two together as a single placement.
            bracket_max_seam_pre     = Format-Num (Get-MaxSeam $brackets 'seam_pre')
            bracket_max_seam_post    = Format-Num (Get-MaxSeam $brackets 'seam_post')
            dualfit_pre_seam_r       = Format-Num (Get-Prop $df 'pre_seam_r')
            dualfit_post_seam_r      = Format-Num (Get-Prop $df 'post_seam_r')
            dualfit_gate_pass        = Format-Num (Get-Prop $df 'gate_pass')
            min_fill_correlation     = Format-Num (Get-Prop $gate 'min_fill_correlation')
            fill_absolute_floor      = Format-Num (Get-Prop $gate 'fill_absolute_floor')
        })
    }
}

if ($unparsed -gt 0) {
    Write-Host "unparsed JSON file(s): $unparsed" -ForegroundColor Yellow
}

$sorted = @($rows | Sort-Object @{ Expression = { ($_.pair -as [int]) ?? [int]::MaxValue } },
                                 @{ Expression = { $_.pair } },
                                 @{ Expression = { [int]$_.gap_index } })

$class = @($sorted | Where-Object { $_.bucket -eq 'class' })

Write-Host ""
Write-Host ("Corpus: {0}" -f $DumpDir)
Write-Host ("Pairs {0} | gaps {1} ({2} characterized, {3} summary-tier with no outcome) | skips {4}" -f
    $pairs.Count, $totalGaps, $outcomeGaps, ($totalGaps - $outcomeGaps), $sorted.Count)
Write-Host ("Predicate: peak_r >= {0}, |step| <= {1} ms, donor continuous and silence_fraction == 0" -f $RFloor, $StepMaxMs)
Write-Host ""

foreach ($b in @('class', 'no_shoulders', 'donor_absent', 'weak_or_stepped', 'unmeasured')) {
    $n = @($sorted | Where-Object { $_.bucket -eq $b }).Count
    if ($n -gt 0 -or $b -ne 'unmeasured') {
        Write-Host ("  {0,-16} {1,5}" -f $b, $n)
    }
}

if ($class.Count -gt 0) {
    # |mid_lag| strata — Result 1's bimodality, and the frame any "remainder estimated by
    # stratum" reading of Question 2 has to quote.
    $strata = [ordered]@{ '|mid| < 5 ms' = 0; '5-50 ms' = 0; '50-200 ms' = 0; '> 200 ms' = 0 }
    foreach ($r in $class) {
        $m = [Math]::Abs([double]$r.mid_lag_ms)
        if ($m -lt 5) { $strata['|mid| < 5 ms']++ }
        elseif ($m -le 50) { $strata['5-50 ms']++ }
        elseif ($m -le 200) { $strata['50-200 ms']++ }
        else { $strata['> 200 ms']++ }
    }
    Write-Host ""
    Write-Host "Class by |mid_lag|:"
    foreach ($k in $strata.Keys) { Write-Host ("  {0,-14} {1,5}" -f $k, $strata[$k]) }

    $unrescued = @($class | Where-Object { $_.dual_fit_rescue -eq 'False' })
    Write-Host ""
    Write-Host ("dual_fit_rescue: true {0} | false {1} (the production remainder)" -f
        ($class.Count - $unrescued.Count), $unrescued.Count)
    foreach ($r in $unrescued) {
        Write-Host ("  {0}/g{1}  mid {2} ms" -f $r.pair, $r.gap_index, [Math]::Round([double]$r.mid_lag_ms, 1))
    }
}

if ($ListClass -and $class.Count -gt 0) {
    Write-Host ""
    $class | Format-Table pair, gap_index, mid_lag_ms, step_ms, pre_peak_r, post_peak_r,
                          skip_reason, dual_fit_rescue -AutoSize
}

if ($CsvOut -ne '') {
    $csvDir = Split-Path -Parent $CsvOut
    if ($csvDir -ne '' -and -not (Test-Path -LiteralPath $csvDir)) {
        New-Item -ItemType Directory -Path $csvDir | Out-Null
    }
    $sorted | Export-Csv -LiteralPath $CsvOut -NoTypeInformation -Encoding utf8
    Write-Host ""
    Write-Host "CSV: $CsvOut ($($sorted.Count) skipped gap(s))" -ForegroundColor DarkGray
}
