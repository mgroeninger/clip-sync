#!/usr/bin/env pwsh
# Measurement harness for perf-plan §2.5 lever 1c #2 ("cut k" — the anchor-bracket matchability pre-gate).
#
# Sizes lever #2's ceiling on real media: of the time `gate_anchor_search` spends running a full
# `bracket_unified_search` on each anchor bracket, how much is RECOVERABLE — i.e. spent on brackets that
# fail the matchability arm (a nominal-window matchability pre-gate could skip them) vs NOT recoverable
# (structure-doomed, which still needs the search). Multi-pair because k and the matchability/structure
# split are content-dependent; one pair can't size the lever.
#
# HOW IT RUNS THE MEASUREMENT: a `--gap-fingerprints DIR` run scores every anchor bracket through the SAME
# `evaluate_seam_gate_fit_candidate` production uses (via `oracle_score_fit_candidate(..., anchor=true)`),
# but WITHOUT the mux/write step — so it produces the per-bracket perf data AND a licensing-safe extended
# corpus (`corpus.json` + per-gap JSON + `manifest.json`) in a single pass over each pair. With
# `CLIP_SYNC_BRACKET_STATS=1` the gate emits one `bracket_stats` event per scored bracket, categorized by
# BOTH arms (unlike the corpus's own first-failing `failure_stage`, which can't separate matchability-doomed
# from structure-doomed):
#     ... INFO bracket_stats: anchor bracket a_start_secs=<f> category="<cat>" search_us=<u> pregate_doomed=<bool> ...
# categories: pass_arms | reject_structure_only | reject_matchability_only | reject_both | no_placement
# Recoverable (CEILING) = reject_matchability_only + reject_both — the matchability arm, measured at the
# SEARCHED placement. This is the most any matchability pre-gate could skip.
#
# REALIZABLE (perf lever #2 §7, the actual decision gate): `pregate_doomed=true` marks brackets the BYTE-SAFE
# pre-gate predicate (`anchor_bracket_matchability_doomed`: windowed-max seam Pearson < min_pearson −
# xcorr_ambiguous_band = −0.03 with defaults, over the reachable placement window) would ACTUALLY skip. It is
# a subset of the ceiling because the unified search optimizes a joint structure+wave score (not seam Pearson)
# and xcorr rescue lowers the safe floor to −0.03. The roll-up reports realizable count/time and the
# realizable/ceiling efficiency; a `pregate_doomed=true` on a non-`reject_matchability` bracket is a superset
# violation (false skip = correctness bug) and is flagged LOUD. See docs/archive/TEMP-anchor-pregate-plan.md §7.
#
# Two modes:
#   Run pairs from a manifest (builds the corpus under -CorpusRoot AND rolls up perf):
#     ./scripts/measure-anchor-brackets.ps1 -Manifest pairs.csv -CorpusRoot ./corpus-out
#   Roll up logs you already captured (skip the runs):
#     ./scripts/measure-anchor-brackets.ps1 -Logs perf_logs/
#
# Manifest format (CSV or TSV; '#' comments and blank lines ignored): one pair per line, no header row
#     label , path/to/A.mkv , path/to/B.mkv [, extra per-pair scan args]
# Delimiter is auto-picked from the extension (.tsv => tab, else comma); override with -Delimiter.
# Fields may be quoted ("C:\my movies\a.mkv") so paths with spaces/commas are fine.
#
# -ScanArgs is a shared scan recipe appended to every pair (e.g. "--min-gap-ms 500"); per-pair `extra`
# from the manifest is appended after it. No output/mux flags are involved — the fingerprint run never
# writes patched audio, only the corpus dir.

[CmdletBinding(DefaultParameterSetName = 'Run')]
param(
    [Parameter(ParameterSetName = 'Run', Mandatory = $true)]
    [string]$Manifest,

    # Shared scan-recipe args appended after `A B --gap-fingerprints DIR` for every pair (match your
    # production recipe). Per-pair `extra` from the manifest is appended after these.
    [Parameter(ParameterSetName = 'Run')]
    [string]$ScanArgs = '',

    # Where the per-pair fingerprint corpora are written (persisted — this is the extended corpus). Each
    # pair gets a subdir named for its label.
    [Parameter(ParameterSetName = 'Run')]
    [string]$CorpusRoot = (Join-Path (Get-Location) 'anchor-bracket-corpus'),

    # Roll up existing logs instead of running. A directory (all *.log) or a glob.
    [Parameter(ParameterSetName = 'Logs', Mandatory = $true)]
    [string]$Logs,

    # Where per-pair logs go (Run mode).
    [Parameter(ParameterSetName = 'Run')]
    [string]$OutDir = (Join-Path $env:TEMP 'clip-sync-anchor-bracket-stats'),

    # Manifest delimiter: 'auto' (by extension), 'comma', or 'tab'.
    [Parameter(ParameterSetName = 'Run')]
    [ValidateSet('auto', 'comma', 'tab')]
    [string]$Delimiter = 'auto',

    # cargo build/run profile.
    [string]$CargoProfile = 'release',

    # cargo features to build with. MUST include `calibration` (enables --gap-fingerprints). Codec
    # decoders are opt-in: real MKV/MP4 movie audio is usually AC-3/E-AC-3 (`ac3`) or HE-AAC (`he-aac`);
    # without them tracks demux but come back non-decodable ("no decodable audio tracks"). `ffmpeg-mux` is
    # NOT needed here (the fingerprint run never muxes). Matches docs/development.md minus ffmpeg-mux.
    [string]$Features = 'he-aac,ffmpeg-mux,ac3,calibration'
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
        # Perf lever #2 §7 — the REALIZABLE arm: brackets the byte-safe pre-gate predicate
        # (`anchor_bracket_matchability_doomed`, windowed-max seam Pearson < floor over the reachable window)
        # would actually skip. Distinct from the ceiling (`reject_matchability_*`, measured at the SEARCHED
        # placement). By the superset proof PregateN ⊆ recoverable; PregateLeakN counts any violation (a
        # pre-gate skip on a bracket the searched gate would have PASSED = a correctness bug → must stay 0).
        PregateN     = 0
        PregateUs    = [long]0
        PregateLeakN = 0
    }
}

# Parse one log file's bracket_stats lines into a tally.
function Read-LogTally {
    param([string]$Path, [string]$Label)
    $t = New-PairTally -Label $Label
    # `pregate_doomed` may appear before or after `search_us` depending on field order; capture it separately
    # and default to false for older logs that predate the §7 instrumentation.
    $rx = [regex]'bracket_stats:.*?category="(?<cat>[a-z_]+)".*?search_us=(?<us>\d+)'
    $rxDoom = [regex]'pregate_doomed=(?<d>true|false)'
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
        $dm = $rxDoom.Match($line)
        if ($dm.Success -and $dm.Groups['d'].Value -eq 'true') {
            $t.PregateN += 1
            $t.PregateUs += $us
            if ($cat -ne 'reject_matchability_only' -and $cat -ne 'reject_both') { $t.PregateLeakN += 1 }
        }
    }
    return $t
}

function Format-Sec { param([long]$Us) '{0,8:N1}s' -f ($Us / 1e6) }

function Write-TallyRow {
    param($T, [long]$GrandTotalUs)
    $recovUs = $T.Cat.reject_matchability_only + $T.Cat.reject_both
    $recovPct = if ($T.TotalUs -gt 0) { 100.0 * $recovUs / $T.TotalUs } else { 0.0 }
    $recovN = $T.CatN.reject_matchability_only + $T.CatN.reject_both
    # Realizable (§7): byte-safe pre-gate would skip these. `real%` is by-time vs total; `eff%` is the
    # realizable/ceiling efficiency (how much of the ceiling the byte-safe predicate actually captures).
    $realPct = if ($T.TotalUs -gt 0) { 100.0 * $T.PregateUs / $T.TotalUs } else { 0.0 }
    $effPct = if ($recovN -gt 0) { 100.0 * $T.PregateN / $recovN } else { 0.0 }
    $leak = if ($T.PregateLeakN -gt 0) { " LEAK={0}!" -f $T.PregateLeakN } else { '' }
    '{0,-20} {1,5} {2} {3} {4,6:N1}% | real {5} {6,6:N1}% eff {7,5:N1}%  (ceilN {8}, m-only {9}, both {10}, struct {11}){12}' -f `
        $T.Label,
        $T.Brackets,
        (Format-Sec $T.TotalUs),
        (Format-Sec $recovUs),
        $recovPct,
        (Format-Sec $T.PregateUs),
        $realPct,
        $effPct,
        $recovN,
        $T.CatN.reject_matchability_only,
        $T.CatN.reject_both,
        $T.CatN.reject_structure_only,
        $leak
}

# ---- collect logs (running the pairs first, if in Run mode) --------------------------------------

$logFiles = [System.Collections.Generic.List[object]]::new()

if ($PSCmdlet.ParameterSetName -eq 'Run') {
    if (-not (Test-Path $Manifest)) { throw "manifest not found: $Manifest" }
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
    New-Item -ItemType Directory -Force -Path $CorpusRoot | Out-Null

    Write-Host "Building clip-sync-repair ($CargoProfile, --features $Features)..." -ForegroundColor Cyan
    $profileFlag = if ($CargoProfile -eq 'release') { '--release' } else { "--profile=$CargoProfile" }
    & cargo build $profileFlag --features $Features -p clip-sync-repair --bin clip-sync-repair
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair.exe"
    if (-not (Test-Path $exe)) { $exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair" }
    if (-not (Test-Path $exe)) { throw "repair binary not found at target/$CargoProfile/" }

    $delimChar = switch ($Delimiter) {
        'tab' { "`t" }
        'comma' { ',' }
        default { if ([IO.Path]::GetExtension($Manifest) -eq '.tsv') { "`t" } else { ',' } }
    }
    # Strip comments/blank lines, then let ConvertFrom-Csv handle quoting (paths with spaces/commas).
    $rows = Get-Content $Manifest |
        Where-Object { $_.Trim() -ne '' -and -not $_.Trim().StartsWith('#') } |
        ConvertFrom-Csv -Delimiter $delimChar -Header 'label', 'a', 'b', 'extra'
    foreach ($row in $rows) {
        $label = ("$($row.label)").Trim()
        $aPath = ("$($row.a)").Trim()
        $bPath = ("$($row.b)").Trim()
        $extra = ("$($row.extra)").Trim()
        if ($label -eq '' -or $aPath -eq '' -or $bPath -eq '') {
            Write-Warning "manifest row skipped (need label,A,B): '$($row.label),$($row.a),$($row.b)'"; continue
        }
        foreach ($p in @($aPath, $bPath)) {
            if (-not (Test-Path $p)) { throw "pair '$label': media not found: $p" }
        }

        $corpus = Join-Path $CorpusRoot $label
        $log = Join-Path $OutDir "$label.log"

        # A B --gap-fingerprints <corpus> <shared scan args> <per-pair extra>. No mux/write flag: the
        # fingerprint run only writes the corpus dir. Paths are passed via the array form (space-safe).
        $argList = @($aPath, $bPath, '--gap-fingerprints', $corpus)
        if ($ScanArgs -ne '') { $argList += ($ScanArgs -split '\s+' | Where-Object { $_ -ne '' }) }
        if ($extra -ne '') { $argList += ($extra -split '\s+' | Where-Object { $_ -ne '' }) }

        Write-Host "[$label] fingerprinting (corpus -> $corpus)..." -ForegroundColor Cyan
        $env:CLIP_SYNC_BRACKET_STATS = '1'
        $env:RUST_LOG = 'warn,bracket_stats=info'
        # Stderr carries the tracing events; capture both streams to the pair log.
        & $exe @argList 2>&1 | Tee-Object -FilePath $log | Out-Null
        $rc = $LASTEXITCODE
        Remove-Item Env:\CLIP_SYNC_BRACKET_STATS -ErrorAction SilentlyContinue
        if ($rc -ne 0) { Write-Warning "[$label] exited $rc (log kept at $log)" }

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
    $grand.PregateN += $t.PregateN
    $grand.PregateUs += $t.PregateUs
    $grand.PregateLeakN += $t.PregateLeakN
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

# Deterministic bracket-count share of the recoverable arm — the licensing-safe ceiling proxy (each
# bracket runs one ~constant-cost unified search, so count-fraction ~ time-fraction, and it reproduces
# across runs unlike wall-clock search_us).
$recovN = $grand.CatN.reject_matchability_only + $grand.CatN.reject_both
$recovNPct = if ($grand.Brackets -gt 0) { 100.0 * $recovN / $grand.Brackets } else { 0.0 }
$recovUs = $grand.Cat.reject_matchability_only + $grand.Cat.reject_both
$recovPct = if ($grand.TotalUs -gt 0) { 100.0 * $recovUs / $grand.TotalUs } else { 0.0 }
Write-Host ("Ceiling (by time):     lever #2 can remove at most {0:N1}% of anchor-bracket search time." -f $recovPct)
Write-Host ("Ceiling (by bracket #): {0} of {1} brackets are matchability-doomed (searched placement) = {2:N1}% (deterministic proxy)." -f $recovN, $grand.Brackets, $recovNPct)
Write-Host ("Across {0} pair(s)." -f $tallies.Count)

# ---- REALIZABLE arm (perf lever #2 §7 — the actual decision gate) --------------------------------
# The ceiling above is measured at the SEARCHED placement; the byte-safe pre-gate skips only when
# windowed-max seam Pearson < floor (= min_pearson - xcorr_ambiguous_band = -0.03 with defaults) over the
# whole reachable window. That is a subset — this is what the lever would ACTUALLY save.
$realN = $grand.PregateN
$realUs = $grand.PregateUs
$realNPct = if ($grand.Brackets -gt 0) { 100.0 * $realN / $grand.Brackets } else { 0.0 }
$realPct = if ($grand.TotalUs -gt 0) { 100.0 * $realUs / $grand.TotalUs } else { 0.0 }
$effPct = if ($recovN -gt 0) { 100.0 * $realN / $recovN } else { 0.0 }
Write-Host ''
Write-Host 'REALIZABLE (byte-safe pre-gate `anchor_bracket_matchability_doomed`, the §7 decision gate):' -ForegroundColor Green
Write-Host ("Realizable (by time):     {0:N1}% of anchor-bracket search time." -f $realPct)
Write-Host ("Realizable (by bracket #): {0} of {1} brackets = {2:N1}% (deterministic); efficiency = {3:N1}% of the ceiling." -f $realN, $grand.Brackets, $realNPct, $effPct)
if ($grand.PregateLeakN -gt 0) {
    # A superset violation invalidates the byte-identity argument — it must block the go/no-go verdict, not
    # coexist with it. Fix the predicate/window before the realizable rate means anything.
    Write-Host ("  !! SUPERSET VIOLATION: {0} bracket(s) pre-gate-doomed but NOT searched-rejected — a false skip = CORRECTNESS BUG." -f $grand.PregateLeakN) -ForegroundColor Red
    Write-Host "  -> STOP: the byte-identity superset proof is contradicted by data. Do NOT wire. Audit anchor_bracket_matchability_doomed / the reachable window (plan §4 R1) before any go/no-go read." -ForegroundColor Red
}
elseif ($realNPct -lt 10.0) {
    Write-Host "  -> Low realizable rate: the -0.03 floor prediction holds; the byte-safe pre-gate does not pay off. Record the negative result and drop the lever (plan §7)." -ForegroundColor Yellow
} else {
    Write-Host "  -> Material realizable rate: un-pause and wire the pre-gate (plan §3)." -ForegroundColor Green
}
