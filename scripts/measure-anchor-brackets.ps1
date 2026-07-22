#!/usr/bin/env pwsh
# Measurement harness for perf-plan §2.5 lever 1c #2 ("cut k" — the anchor-bracket matchability pre-gate).
#
# Sizes lever #2's ceiling on real media: of the time `gate_anchor_search` spends running a full
# `bracket_unified_search` on each anchor bracket, how much is RECOVERABLE — i.e. spent on brackets that
# fail the matchability arm (a nominal-window matchability pre-gate could skip them) vs NOT recoverable
# (structure-doomed, which still needs the search). Multi-pair because k and the matchability/structure
# split are content-dependent; one pair can't size the lever.
#
# Data source: the `CLIP_SYNC_BRACKET_STATS` instrumentation in patch_region.rs emits one
# `bracket_stats`-target event per scored anchor bracket:
#     ... INFO bracket_stats: anchor bracket a_start_secs=<f> category="<cat>" search_us=<u> ...
# categories: pass_arms | reject_structure_only | reject_matchability_only | reject_both | no_placement
# Recoverable = reject_matchability_only + reject_both (the matchability arm the pre-gate targets).
#
# Two modes:
#   Run pairs from a manifest, then roll up:
#     ./scripts/measure-anchor-brackets.ps1 -Manifest pairs.tsv -BinArgs "--wav {out} --fill-mode fit"
#   Roll up logs you already captured (skip the runs):
#     ./scripts/measure-anchor-brackets.ps1 -Logs perf_logs/
#
# Manifest format (TSV, '#' comments and blank lines ignored): one pair per line
#     label <TAB> path/to/A.mkv <TAB> path/to/B.mkv [<TAB> extra per-pair repair args]
#
# -BinArgs is the shared repair recipe (match the recipe your perf_N runs used). The token `{out}` in
# -BinArgs is replaced per pair with a throwaway output path under -OutDir (deleted after parsing). If
# -BinArgs contains no `{out}`, no output substitution happens (you own the write flag).

[CmdletBinding(DefaultParameterSetName = 'Run')]
param(
    [Parameter(ParameterSetName = 'Run', Mandatory = $true)]
    [string]$Manifest,

    # Shared repair args appended after `A B`. Use `{out}` for the per-pair throwaway output path.
    [Parameter(ParameterSetName = 'Run')]
    [string]$BinArgs = '--wav {out} --fill-mode fit',

    # Roll up existing logs instead of running. A directory (all *.log) or a glob.
    [Parameter(ParameterSetName = 'Logs', Mandatory = $true)]
    [string]$Logs,

    # Where per-pair logs + throwaway outputs go (Run mode).
    [Parameter(ParameterSetName = 'Run')]
    [string]$OutDir = (Join-Path $env:TEMP 'clip-sync-anchor-bracket-stats'),

    # Keep the throwaway repair outputs (Run mode) instead of deleting them.
    [switch]$KeepOutputs,

    # cargo build/run profile.
    [string]$Profile = 'release'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

function New-PairTally {
    param([string]$Label)
    [pscustomobject]@{
        Label      = $Label
        Brackets   = 0
        TotalUs    = [long]0
        # per-category microseconds
        Cat        = @{
            pass_arms               = [long]0
            reject_structure_only   = [long]0
            reject_matchability_only = [long]0
            reject_both             = [long]0
            no_placement            = [long]0
        }
        CatN       = @{
            pass_arms               = 0
            reject_structure_only   = 0
            reject_matchability_only = 0
            reject_both             = 0
            no_placement            = 0
        }
    }
}

# Parse one log file's bracket_stats lines into a tally.
function Read-LogTally {
    param([string]$Path, [string]$Label)
    $t = New-PairTally -Label $Label
    $rx = [regex]'bracket_stats:.*?category="(?<cat>[a-z_]+)".*?search_us=(?<us>\d+)'
    foreach ($line in [System.IO.File]::ReadLines($Path)) {
        if ($line -notlike '*bracket_stats*') { continue }
        $m = $rx.Match($line)
        if (-not $m.Success) { continue }
        $cat = $m.Groups['cat'].Value
        $us = [long]$m.Groups['us'].Value
        if (-not $t.Cat.ContainsKey($cat)) { $t.Cat[$cat] = [long]0; $t.CatN[$cat] = 0 }
        $t.Cat[$cat] += $us
        $t.CatN[$cat] += 1
        $t.TotalUs += $us
        $t.Brackets += 1
    }
    return $t
}

function Format-Sec { param([long]$Us) '{0,8:N1}s' -f ($Us / 1e6) }

function Write-TallyRow {
    param($T, [long]$GrandTotalUs)
    $recovUs = $T.Cat.reject_matchability_only + $T.Cat.reject_both
    $recovPct = if ($T.TotalUs -gt 0) { 100.0 * $recovUs / $T.TotalUs } else { 0.0 }
    '{0,-20} {1,5} {2} {3} {4,7:N1}%  (m-only {5}, both {6}, struct {7}, pass {8}, noplace {9})' -f `
        $T.Label,
        $T.Brackets,
        (Format-Sec $T.TotalUs),
        (Format-Sec $recovUs),
        $recovPct,
        $T.CatN.reject_matchability_only,
        $T.CatN.reject_both,
        $T.CatN.reject_structure_only,
        $T.CatN.pass_arms,
        $T.CatN.no_placement
}

# ---- collect logs (running the pairs first, if in Run mode) --------------------------------------

$logFiles = [System.Collections.Generic.List[object]]::new()

if ($PSCmdlet.ParameterSetName -eq 'Run') {
    if (-not (Test-Path $Manifest)) { throw "manifest not found: $Manifest" }
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

    Write-Host "Building clip-sync-repair ($Profile)..." -ForegroundColor Cyan
    $profileFlag = if ($Profile -eq 'release') { '--release' } else { "--profile=$Profile" }
    & cargo build $profileFlag -p clip-sync-repair --bin clip-sync-repair
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $exe = Join-Path $RepoRoot "target/$Profile/clip-sync-repair.exe"
    if (-not (Test-Path $exe)) { $exe = Join-Path $RepoRoot "target/$Profile/clip-sync-repair" }
    if (-not (Test-Path $exe)) { throw "repair binary not found at target/$Profile/" }

    $lineNo = 0
    foreach ($raw in Get-Content $Manifest) {
        $lineNo++
        $line = $raw.Trim()
        if ($line -eq '' -or $line.StartsWith('#')) { continue }
        $cols = $line -split "`t"
        if ($cols.Count -lt 3) { Write-Warning "manifest line ${lineNo}: need label<TAB>A<TAB>B, got '$line'"; continue }
        $label = $cols[0].Trim()
        $aPath = $cols[1].Trim()
        $bPath = $cols[2].Trim()
        $extra = if ($cols.Count -gt 3) { $cols[3].Trim() } else { '' }
        foreach ($p in @($aPath, $bPath)) {
            if (-not (Test-Path $p)) { throw "pair '$label': media not found: $p" }
        }

        $out = Join-Path $OutDir "$label.out.wav"
        $log = Join-Path $OutDir "$label.log"
        $resolvedArgs = $BinArgs.Replace('{out}', $out)

        # A B <shared args> <per-pair extra>, split on whitespace (paths are quoted below via the array form).
        $argList = @($aPath, $bPath) + ($resolvedArgs -split '\s+' | Where-Object { $_ -ne '' })
        if ($extra -ne '') { $argList += ($extra -split '\s+' | Where-Object { $_ -ne '' }) }

        Write-Host "[$label] running..." -ForegroundColor Cyan
        $env:CLIP_SYNC_BRACKET_STATS = '1'
        $env:RUST_LOG = 'warn,bracket_stats=info'
        # Stderr carries the tracing events; capture both streams to the pair log.
        & $exe @argList 2>&1 | Tee-Object -FilePath $log | Out-Null
        $rc = $LASTEXITCODE
        Remove-Item Env:\CLIP_SYNC_BRACKET_STATS -ErrorAction SilentlyContinue
        if ($rc -ne 0) { Write-Warning "[$label] exited $rc (log kept at $log)" }
        if (-not $KeepOutputs) { Remove-Item -Force -ErrorAction SilentlyContinue $out }

        $logFiles.Add([pscustomobject]@{ Path = $log; Label = $label })
    }
}
else {
    $paths = if (Test-Path $Logs -PathType Container) { Get-ChildItem -Path $Logs -Filter *.log -File } else { Get-ChildItem -Path $Logs -File }
    foreach ($f in $paths) { $logFiles.Add([pscustomobject]@{ Path = $f.FullName; Label = $f.BaseName }) }
    if ($logFiles.Count -eq 0) { throw "no log files matched: $Logs" }
}

# ---- aggregate + report --------------------------------------------------------------------------

$tallies = foreach ($lf in $logFiles) { Read-LogTally -Path $lf.Path -Label $lf.Label }
$grand = New-PairTally -Label 'TOTAL'
foreach ($t in $tallies) {
    $grand.Brackets += $t.Brackets
    $grand.TotalUs += $t.TotalUs
    foreach ($k in @($t.Cat.Keys)) {
        if (-not $grand.Cat.ContainsKey($k)) { $grand.Cat[$k] = [long]0; $grand.CatN[$k] = 0 }
        $grand.Cat[$k] += $t.Cat[$k]
        $grand.CatN[$k] += $t.CatN[$k]
    }
}

Write-Host ''
Write-Host 'Anchor-bracket search time — lever #2 (cut k) ceiling' -ForegroundColor Green
Write-Host 'Recoverable = search time on matchability-doomed brackets (a nominal-window pre-gate could skip them).'
Write-Host ''
'{0,-20} {1,5} {2,9} {3,9} {4,8}' -f 'pair', 'brkts', 'total', 'recov', 'recov%'
Write-Host ('-' * 96)
foreach ($t in $tallies) { Write-TallyRow -T $t -GrandTotalUs $grand.TotalUs }
Write-Host ('-' * 96)
Write-TallyRow -T $grand -GrandTotalUs $grand.TotalUs
Write-Host ''

$recovUs = $grand.Cat.reject_matchability_only + $grand.Cat.reject_both
$recovPct = if ($grand.TotalUs -gt 0) { 100.0 * $recovUs / $grand.TotalUs } else { 0.0 }
Write-Host ("Ceiling: lever #2 can remove at most {0:N1}% of anchor-bracket search time across {1} pair(s)." -f $recovPct, $tallies.Count)
if ($recovPct -lt 10.0) {
    Write-Host "  -> Low ceiling: most brackets are structure-doomed, not matchability-doomed. Favor 'stop here'." -ForegroundColor Yellow
} else {
    Write-Host "  -> Material ceiling: proceed to design/implement the matchability pre-gate (perf-plan §2.5 #2)." -ForegroundColor Green
}
