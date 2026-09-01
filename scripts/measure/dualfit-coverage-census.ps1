#!/usr/bin/env pwsh
#Requires -Version 7.0
# Dump-only census: would dual-fit-alone accept EVERY gap, not just the ones production's
# skip_or_dual_fit actually reaches? Answers "could dual-fit replace the keep-path gate search
# as the primary/sole mechanism", without any production code change or fresh media run.
#
# WHY THIS IS POSSIBLE FROM EXISTING DUMPS: `compute_region_measurements` (overlay.rs) computes
# `splice_dualfit` UNCONDITIONALLY for every gap, patched or skipped — it is never gated on the
# bracket-gate outcome. The only thing suppressed on a patched gap is the derived VERDICT
# (`dual_fit_rescue_flag` returns `None` when `patched`, schema.rs, "a patched gap never reaches
# skip_or_dual_fit"). That suppression is about production's control flow, not data availability:
# the raw legs the verdict is built from are already in the JSON for both patch and skip gaps.
# So this script recomputes the verdict itself, for every gap, mirroring the exact predicate in
# `dual_fit_rescue_flag` (crates/clip-sync-repair/src/application/gap_fingerprint/schema.rs):
#   1. bracket failure class is dual-fit-eligible (any bracket's failure_stage != structure_align)
#   2. splice_dualfit.gate_pass
#   3. splice_dualfit.bridge_frames > 0            (shoulders don't cross)
#   4. step_real: post_seam_r >= post_seam_global_r + DUALFIT_STEP_REAL_MARGIN (0.15)
#   5. donor_interior_aligned.continuous            (aligned donor bridges the hole)
#   6. donor_interior_nominal.silence_fraction < PROGRAM_QUIET_SILENCE_FRAC (0.5)
# `-InclRigidFill` additionally reports the relaxed predicate from
# docs/dev/TEMP-seam-lock-rigid-fill-plan.md, which drops leg 4 (step_real) and instead requires
# only legs 1-3 + 5-6 — i.e. "would a trim-0 rigid fill be assembled", not "would today's dual-fit".
#
# THE AXIS THIS SCRIPT USES FOR "keep-path's own disposition": `outcome.tier` ("patch" / "skip").
# That is contractually the bracket-gate `any_ok` result and nothing else (schema.rs) — it does
# NOT already fold in production's dual-fit rescue (dual-fit only ever runs after a gate skip), so
# it is exactly keep-path's verdict in isolation, independent of dual-fit. That is the correct
# left axis for "could dual-fit replace keep path": tier=="patch" means keep-path solved it without
# any help from dual-fit.
#
# WHAT THIS CANNOT ANSWER. Whether dual-fit's PLACEMENT/FILL would sound the same as keep-path's on
# the "both would patch" quadrant — this predicate is accept/decline only. It also cannot answer
# whether swapping dual-fit to primary is SAFE (a "keep-path skip, dual-fit accept" gap might be one
# keep-path was right to skip); it only sizes the coverage question the accept/decline axis can see.
#
# Consumer only — opens no media, runs no repair, needs no oracle. Point it at fingerprint-corpus
# JSON that measure-gap-fingerprints.ps1 already wrote.
#
# Usage:
#   ./scripts/measure/dualfit-coverage-census.ps1 -DumpDir gap-files/<date>-complete-corpus
#   ./scripts/measure/dualfit-coverage-census.ps1 -DumpDir gap-files/<date>-complete-corpus -CsvOut gap-files/dualfit-coverage.csv
#   ./scripts/measure/dualfit-coverage-census.ps1 -DumpDir gap-files/<date>-complete-corpus -InclRigidFill
#
# Accepted layout: same as census-skip-buckets.ps1 — `<DumpDir>/<pair>/corpus.json` preferred,
# per-gap `*_full.json` fallback when no corpus.json exists for that pair. `manifest.json` /
# `listen.json` are always skipped.
#
# MEDIA HYGIENE: emit pair label + gap index only — never media paths or titles.
# Docs: docs/dev/TEMP-seam-lock-rigid-fill-plan.md, docs/dev/TEMP-rigid-shoulder-offset-skips-2026-08-31.md.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$DumpDir,

    # DUALFIT_STEP_REAL_MARGIN (domain/dual_fit.rs). Only override to test sensitivity.
    [double]$StepRealMargin = 0.15,

    # PROGRAM_QUIET_SILENCE_FRAC (domain/donor.rs).
    [double]$ProgramQuietFrac = 0.5,

    # Also report the rigid-fill plan's relaxed predicate (drops the step_real leg).
    [switch]$InclRigidFill,

    # CSV of every characterized (full-tier) gap, both patch and skip.
    [string]$CsvOut = ''
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $DumpDir)) {
    throw "dump dir not found: $DumpDir"
}

function Test-Prop([object]$Obj, [string]$Name) {
    if ($null -eq $Obj) { return $false }
    return [bool]($Obj.PSObject.Properties.Name -contains $Name)
}

function Get-Prop([object]$Obj, [string]$Name) {
    if (-not (Test-Prop $Obj $Name)) { return $null }
    return $Obj.$Name
}

function Format-Num([object]$Value) {
    if ($null -eq $Value) { return '' }
    if ($Value -is [bool]) { return $Value.ToString() }
    $d = $Value -as [double]
    if ($null -eq $d) { return [string]$Value }
    if ([double]::IsNaN($d)) { return 'NaN' }
    return $d.ToString('R', [System.Globalization.CultureInfo]::InvariantCulture)
}

# Mirrors `partial_cmp(...).is_some_and(|ord| ord != Less)` — NaN on either side is "not real".
function Test-StepReal([Nullable[double]]$PostR, [Nullable[double]]$PostGlobalR, [double]$Margin) {
    if ($null -eq $PostR -or $null -eq $PostGlobalR) { return $false }
    if ([double]::IsNaN($PostR) -or [double]::IsNaN($PostGlobalR)) { return $false }
    return $PostR -ge ($PostGlobalR + $Margin)
}

function Test-BracketsDualFitEligible([object[]]$Brackets) {
    foreach ($b in $Brackets) {
        $stage = Get-Prop $b 'failure_stage'
        if ($null -ne $stage -and $stage -ne 'structure_align') { return $true }
    }
    return $false
}

$skipNames = @('manifest.json', 'listen.json')
$candidates = @(Get-ChildItem -LiteralPath $DumpDir -Recurse -File -Filter '*.json' |
    Where-Object { $skipNames -notcontains $_.Name })

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
$unparsed = 0

foreach ($file in $files) {
    try {
        $doc = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    } catch {
        $unparsed++
        continue
    }
    if (-not (Test-Prop $doc 'gaps')) { continue }

    $rel = [System.IO.Path]::GetRelativePath($DumpDir, $file.FullName)
    $pair = ($rel -split '[\\/]')[0]
    [void]$pairs.Add($pair)

    foreach ($g in @($doc.gaps)) {
        $outcome = Get-Prop $g 'outcome'
        $tier = Get-Prop $outcome 'tier'
        # Only full-tier gaps were characterized pairwise; summary-tier carries no outcome at all.
        if ($null -eq $tier) { continue }
        $totalGaps++

        $brackets = @(Get-Prop $g 'brackets')
        $df = Get-Prop $g 'splice_dualfit'
        $aligned = Get-Prop $g 'donor_interior_aligned'
        $nominal = Get-Prop $g 'donor_interior_nominal'

        $measurable = ($null -ne $df -and $null -ne $aligned -and $null -ne $nominal)

        $eligible = Test-BracketsDualFitEligible $brackets
        $gatePass = ($null -ne $df) -and ((Get-Prop $df 'gate_pass') -eq $true)
        $bridgePos = ($null -ne $df) -and ([double](Get-Prop $df 'bridge_frames' ) -gt 0)
        $postR = if ($null -ne $df) { Get-Prop $df 'post_seam_r' } else { $null }
        $postGlobalR = if ($null -ne $df) { Get-Prop $df 'post_seam_global_r' } else { $null }
        $stepReal = Test-StepReal $postR $postGlobalR $StepRealMargin
        $continuous = ($null -ne $aligned) -and ((Get-Prop $aligned 'continuous') -eq $true)
        $programQuiet = ($null -ne $nominal) -and
            ([double](Get-Prop $nominal 'silence_fraction') -ge $ProgramQuietFrac)

        $verdict = $null
        $verdictRigid = $null
        if ($measurable) {
            $verdict = $eligible -and $gatePass -and $bridgePos -and $stepReal -and
                $continuous -and (-not $programQuiet)
            if ($InclRigidFill) {
                # Rigid-fill plan: drop the step_real leg (trim-0 fill covers that case instead
                # of declining). Everything else unchanged.
                $verdictRigid = $eligible -and $gatePass -and $bridgePos -and
                    $continuous -and (-not $programQuiet)
            }
        }

        $rows.Add([pscustomobject][ordered]@{
            pair                  = $pair
            gap_index             = Get-Prop $g 'index'
            keep_path_tier        = $tier
            skip_reason           = Get-Prop $outcome 'skip_reason'
            dual_fit_rescue_field = Format-Num (Get-Prop $outcome 'dual_fit_rescue')
            measurable            = Format-Num $measurable
            eligible              = Format-Num $eligible
            gate_pass             = Format-Num $gatePass
            bridge_frames_pos     = Format-Num $bridgePos
            step_real             = Format-Num $stepReal
            donor_continuous      = Format-Num $continuous
            program_quiet         = Format-Num $programQuiet
            dualfit_alone_verdict = Format-Num $verdict
            dualfit_rigid_verdict = Format-Num $verdictRigid
            dualfit_pre_seam_r    = Format-Num (Get-Prop $df 'pre_seam_r')
            dualfit_post_seam_r   = Format-Num $postR
            dualfit_post_global_r = Format-Num $postGlobalR
            dualfit_trim_frames   = Format-Num (Get-Prop $df 'trim_frames')
            bracket_count         = $brackets.Count
        })
    }
}

if ($unparsed -gt 0) {
    Write-Host "unparsed JSON file(s): $unparsed" -ForegroundColor Yellow
}

$sorted = @($rows | Sort-Object @{ Expression = { ($_.pair -as [int]) ?? [int]::MaxValue } },
                                 @{ Expression = { $_.pair } },
                                 @{ Expression = { [int]$_.gap_index } })

Write-Host ""
Write-Host ("Corpus: {0}" -f $DumpDir)
Write-Host ("Pairs {0} | characterized gaps {1}" -f $pairs.Count, $sorted.Count)
Write-Host ("Predicate: eligible bracket AND gate_pass AND bridge_frames>0 AND step_real (margin {0}) AND donor continuous AND NOT program-quiet (frac >= {1})" -f $StepRealMargin, $ProgramQuietFrac)
Write-Host ""

$measurableRows = @($sorted | Where-Object { $_.measurable -eq 'True' })
$unmeasurable = $sorted.Count - $measurableRows.Count
if ($unmeasurable -gt 0) {
    Write-Host "unmeasurable (missing splice_dualfit / donor fields): $unmeasurable" -ForegroundColor Yellow
}

function Show-CrossTab([object[]]$Rows, [string]$VerdictCol, [string]$Label) {
    $patchAccept = @($Rows | Where-Object { $_.keep_path_tier -eq 'patch' -and $_.$VerdictCol -eq 'True' })
    $patchDecline = @($Rows | Where-Object { $_.keep_path_tier -eq 'patch' -and $_.$VerdictCol -eq 'False' })
    $skipAccept = @($Rows | Where-Object { $_.keep_path_tier -eq 'skip' -and $_.$VerdictCol -eq 'True' })
    $skipDecline = @($Rows | Where-Object { $_.keep_path_tier -eq 'skip' -and $_.$VerdictCol -eq 'False' })

    Write-Host ("--- {0} ---" -f $Label)
    Write-Host ("  keep-path patch | dual-fit accept  : {0,5}   (agree)" -f $patchAccept.Count)
    Write-Host ("  keep-path patch | dual-fit decline : {0,5}   <-- decisive: keep-path finds these, dual-fit-alone can't" -f $patchDecline.Count)
    Write-Host ("  keep-path skip  | dual-fit accept  : {0,5}   (today's dual_fit_rescue candidate set)" -f $skipAccept.Count)
    Write-Host ("  keep-path skip  | dual-fit decline : {0,5}   (neither mechanism fills)" -f $skipDecline.Count)
    Write-Host ""
    return $patchDecline
}

$decisive = Show-CrossTab $measurableRows 'dualfit_alone_verdict' "today's dual-fit predicate"
if ($InclRigidFill) {
    Show-CrossTab $measurableRows 'dualfit_rigid_verdict' 'rigid-fill relaxed predicate (drops step_real)' | Out-Null
}

if ($decisive.Count -gt 0) {
    Write-Host "keep-path patch / dual-fit decline (the decisive quadrant):"
    $decisive | Sort-Object @{ Expression = { ($_.pair -as [int]) ?? [int]::MaxValue } }, gap_index |
        Format-Table pair, gap_index, gate_pass, bridge_frames_pos, step_real, donor_continuous, program_quiet -AutoSize
}

if ($CsvOut -ne '') {
    $csvDir = Split-Path -Parent $CsvOut
    if ($csvDir -ne '' -and -not (Test-Path -LiteralPath $csvDir)) {
        New-Item -ItemType Directory -Path $csvDir | Out-Null
    }
    $sorted | Export-Csv -LiteralPath $CsvOut -NoTypeInformation -Encoding utf8
    Write-Host ""
    Write-Host "CSV: $CsvOut ($($sorted.Count) characterized gap(s))" -ForegroundColor DarkGray
}
