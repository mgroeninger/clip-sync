#!/usr/bin/env pwsh
# Measurement harness for M0 (docs/dev/TEMP-patch-audio-bracket-fill-elimination-plan.md §3).
#
# THE QUESTION: the characterize->execute split means the bracket fill is assembled TWICE per patched gap —
# once in characterize (scored for the report-vs-splice reconciliation + the normalize gain, then discarded)
# and once in execute (the bytes actually spliced). `exec_fill_assembly` is the cost that split ADDED. This
# harness measures it against the run it belongs to, and decides whether a dedup/hoist phase (H?) exists.
#
# Prior bound (2026-07-20 run, docs/dev/archive/TEMP-production-repair-perf-plan.md §0): 1872 s total,
# `char_gate_search` 93.3%, decode 6.5%, execute+splice 0.008 s. Everything else in characterize — the fill
# assembly included — fit in the ~4.8 s remainder (<=0.26%). Expect a small number; the point is to replace
# "expect" with a measurement.
#
# HOW IT MEASURES: `CLIP_SYNC_SPAN_TIMING=1` switches the fmt subscriber to `FmtSpan::CLOSE`, so every span
# emits `time.busy`/`time.idle` on close (Level A). This sums busy time per span name over a real repair run.
#
# NON-NEGOTIABLES (the measurement is invalid otherwise):
#   * `--release`. Debug timings are meaningless AND the `debug_assert_eq!` fill shadow is live in debug —
#     it calls `execute_bracket_fill` an extra time, so `exec_fill_assembly` would be double-counted.
#   * A real write path (`--wav`). `--repair-preview` stops after characterize, so `exec_fill_assembly` never
#     runs — it cannot answer this question. `--gap-fingerprints` is the dump path, not production.
#   * `CLIP_SYNC_SPAN_TIMING` set, or no timings emit at all.
#
# Two modes:
#   Run pairs from a manifest:
#     ./scripts/measure-fill-assembly.ps1 -Manifest pairs.csv
#   Roll up logs you already captured (skip the runs):
#     ./scripts/measure-fill-assembly.ps1 -Logs perf_logs/
#
# Manifest format (CSV or TSV; '#' comments and blank lines ignored): one pair per line, no header row
#     label , path/to/A.mkv , path/to/B.m4v [, extra per-pair repair args]
# The log for each row is named after its label (`<label>.log`), as is its WAV. Delimiter is auto-picked from
# the extension (.tsv => tab, else comma); override with -Delimiter. Fields may be quoted
# ("C:\my movies\a.mkv") so paths with spaces/commas are fine.
#
# `fill_mode` is request-level, not per-gap, so the Fit/Gate split comes from running the manifest ONCE PER
# MODE (vary it via -RepairArgs) rather than from a span field. Use -OutDir to keep the two apart.
#
# MEDIA HYGIENE: the manifest and every log this writes contain absolute paths to licensed media. NEITHER MAY
# BE COMMITTED. -OutDir defaults outside the repo ($env:TEMP) for that reason; if you point it inside the repo,
# use the gitignored `gap-files/`. When recording results in docs, carry over only the derived numbers, keyed
# by the manifest's label — never the paths. See docs/dev/TEMP-anchor-search-perf-baseline.md §"Media handling".

[CmdletBinding(DefaultParameterSetName = 'Run')]
param(
    [Parameter(ParameterSetName = 'Run', Mandatory = $true)]
    [string]$Manifest,

    # Shared repair args appended after `A B --wav OUT` for every pair (match your production recipe).
    # Per-pair `extra` from the manifest is appended after these. This is where the fill-mode flag goes.
    [Parameter(ParameterSetName = 'Run')]
    [string]$RepairArgs = '',

    # Where per-pair logs (and the throwaway WAVs) go.
    [Parameter(ParameterSetName = 'Run')]
    [string]$OutDir = (Join-Path $env:TEMP 'clip-sync-fill-assembly'),

    # Keep the patched WAVs instead of deleting them after each pair. They are only a side effect here —
    # the measurement is the log — but a full-length WAV per pair is multi-GB, so they go by default.
    [Parameter(ParameterSetName = 'Run')]
    [switch]$KeepWav,

    # Manifest delimiter: 'auto' (by extension), 'comma', or 'tab'.
    [Parameter(ParameterSetName = 'Run')]
    [ValidateSet('auto', 'comma', 'tab')]
    [string]$Delimiter = 'auto',

    # Roll up existing logs instead of running. A directory (all *.log) or a glob.
    [Parameter(ParameterSetName = 'Logs', Mandatory = $true)]
    [string]$Logs,

    # cargo build/run profile. Changing this off `release` invalidates the measurement (see header).
    [string]$CargoProfile = 'release',

    # cargo features. Real MKV/MP4 movie audio is usually AC-3/E-AC-3 (`ac3`) or HE-AAC (`he-aac`); without
    # them tracks demux but come back non-decodable. `ffmpeg-mux` is NOT needed (this runs `--wav`).
    # Span emission rides on clip-sync's `default-tracing`, which is on by default.
    [string]$Features = 'he-aac,ac3'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

# Spans this harness reports on, in display order. `patch_audio` wraps the whole repair and is the
# denominator; the rest are read against it.
$SpansOfInterest = @(
    'patch_audio',
    'char_gate_search',
    'char_fill_assembly',
    'exec_fill_assembly',
    'characterize',
    'patch_gap',
    'patch_splice'
)

# M0's decision threshold: exec-side assembly under this share of `patch_audio` retires H? outright.
$ImmaterialPct = 1.0

function New-Tally {
    param([string]$Label)
    [pscustomobject]@{
        Label = $Label
        # span name -> [pscustomobject]@{ Secs; Calls }
        Spans = @{}
        Lines = 0
    }
}

function Add-Span {
    param($T, [string]$Name, [double]$Secs)
    if (-not $T.Spans.ContainsKey($Name)) {
        $T.Spans[$Name] = [pscustomobject]@{ Secs = [double]0; Calls = 0 }
    }
    $T.Spans[$Name].Secs += $Secs
    $T.Spans[$Name].Calls += 1
}

# Parse one log's span-close lines into a tally.
#
# A close line looks like:
#   2026-07-24T21:00:00Z  INFO patch_gap{gap_index=1 ...}:char_fill_assembly: clip_sync_repair::...: close time.busy=1.23ms time.idle=4.5us
# The span name is the INNERMOST entry of the `a{...}:b{...}:c` chain that precedes the target path. Span
# fields can contain spaces, so the chain is recovered by slicing at ' close time.busy=' rather than by
# tokenizing the line.
function Read-LogTally {
    param([string]$Path, [string]$Label)
    $t = New-Tally -Label $Label
    $rxBusy = [regex]'close\s+time\.busy=(?<v>[\d.]+)(?<u>ns|[\u00B5\u03BCu]s|ms|s)\b'
    $rxTarget = [regex]'(?<t>[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z0-9_]+)+)$'
    $rxSpan = [regex]'(?<span>[A-Za-z0-9_]+)(\{[^}]*\})?$'
    $unit = @{ 'ns' = 1e-9; 'ms' = 1e-3; 's' = 1.0 }

    # Share ReadWrite/Delete: rolling up while a run is still appending to its log is a normal case (checking
    # an in-flight sweep), and the default ReadLines share mode throws on the open handle.
    $fs = [System.IO.File]::Open(
        $Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete)
    $reader = [System.IO.StreamReader]::new($fs, [System.Text.Encoding]::UTF8)
    try {
    while ($null -ne ($line = $reader.ReadLine())) {
        $m = $rxBusy.Match($line)
        if (-not $m.Success) { continue }

        $u = $m.Groups['u'].Value
        $mult = if ($unit.ContainsKey($u)) { $unit[$u] } else { 1e-6 }  # anything else is a micro-sign variant
        $secs = [double]$m.Groups['v'].Value * $mult

        # Everything left of ' close time.busy=' is timestamp + level + span chain + target.
        $prefix = $line.Substring(0, $m.Index).TrimEnd()
        $prefix = $prefix.TrimEnd(':').TrimEnd()
        $tm = $rxTarget.Match($prefix)
        if ($tm.Success) {
            $prefix = $prefix.Substring(0, $tm.Index).TrimEnd().TrimEnd(':').TrimEnd()
        }
        $sm = $rxSpan.Match($prefix)
        if (-not $sm.Success) { continue }

        Add-Span -T $t -Name $sm.Groups['span'].Value -Secs $secs
        $t.Lines += 1
    }
    }
    finally {
        $reader.Dispose()
        $fs.Dispose()
    }
    return $t
}

function Get-Span {
    param($T, [string]$Name)
    if ($T.Spans.ContainsKey($Name)) { return $T.Spans[$Name] }
    return [pscustomobject]@{ Secs = [double]0; Calls = 0 }
}

function Write-TallyReport {
    param($T)
    $total = (Get-Span $T 'patch_audio').Secs
    Write-Host ""
    Write-Host ("=== {0} ===" -f $T.Label) -ForegroundColor Cyan
    if ($T.Lines -eq 0) {
        Write-Warning "no span-close lines in this log — was CLIP_SYNC_SPAN_TIMING set, and is this stderr?"
        return
    }
    if ($total -le 0) {
        Write-Warning "no `patch_audio` span — shares omitted (did the run reach the patch phase?)"
    }

    $rows = foreach ($name in $SpansOfInterest) {
        $s = Get-Span $T $name
        if ($s.Calls -eq 0) { continue }
        [pscustomobject]@{
            Span         = $name
            TotalSecs    = [math]::Round($s.Secs, 4)
            Calls        = $s.Calls
            PerCallMs    = [math]::Round(($s.Secs / $s.Calls) * 1000, 3)
            'PctOfRepair' = if ($total -gt 0) { [math]::Round(100 * $s.Secs / $total, 4) } else { $null }
        }
    }
    $rows | Format-Table -AutoSize
    Write-Host "  (spans nest: a parent's busy time INCLUDES its children's, so shares do not sum to 100%)" -ForegroundColor DarkGray
}

function Write-Verdict {
    param($T)
    $total = (Get-Span $T 'patch_audio').Secs
    $exec = Get-Span $T 'exec_fill_assembly'
    $char = Get-Span $T 'char_fill_assembly'
    $gate = Get-Span $T 'char_gate_search'
    if ($total -le 0) { Write-Warning "no patch_audio span — cannot render an M0 verdict"; return }

    $execPct = 100 * $exec.Secs / $total
    $gatePct = 100 * $gate.Secs / $total

    Write-Host ""
    Write-Host "--- M0 verdict ---" -ForegroundColor Cyan
    Write-Host ("  added cost (exec_fill_assembly): {0:N3}s over {1} gaps = {2:N4}% of repair" -f $exec.Secs, $exec.Calls, $execPct)

    if ($exec.Calls -eq 0) {
        Write-Warning "  exec_fill_assembly never ran. Either no gap took the Bracket path, or this was a"
        Write-Warning "  --repair-preview / non-write run, which cannot measure the executor. Re-run with --wav."
    }
    elseif ($execPct -lt $ImmaterialPct) {
        Write-Host ("  -> IMMATERIAL (<{0}%). Retire H? in the plan; record this number in the M0 ledger row." -f $ImmaterialPct) -ForegroundColor Green
    }
    else {
        Write-Host ("  -> MATERIAL (>={0}%). H? is live; the Fit/Gate split decides whether a hoist EXISTS." -f $ImmaterialPct) -ForegroundColor Yellow
        Write-Host "     Gate mode: the executor's border rebuild is hoistable (derived A geometry, not a decision)."
        Write-Host "     Fit mode:  the cost is the length-fit search, and the only thing worth caching IS the"
        Write-Host "     assembled fill — i.e. the carry F1/F2 deleted. That outcome is accept-or-revert, not hoist."
    }

    # The two spans do NOT cover the same work: `char_fill_assembly` wraps `assemble_bracket_fill` alone,
    # while `exec_fill_assembly` also wraps the border-template rebuild and the B re-slice. exec > char is
    # expected BY CONSTRUCTION — the difference IS the rebuild, which is what a Gate-mode hoist would target.
    # Only exec running CHEAPER than char is suspicious (it should strictly dominate on the same inputs), and
    # only worth reporting once enough gaps have run for the ratio to mean anything.
    if ($exec.Calls -ge 5 -and $char.Calls -ge 5) {
        $ratio = $exec.Secs / $char.Secs
        Write-Host ("  exec/char = {0:N2}x; the excess over 1.0 is the executor's border rebuild ({1:N4}s total)." -f `
            $ratio, ($exec.Secs - $char.Secs))
        if ($ratio -lt 0.9) {
            Write-Warning ("  exec is CHEAPER than char ({0:N2}x) — it does strictly more work, so this wants" -f $ratio)
            Write-Warning "  explaining before the number is trusted."
        }
        if ($exec.Calls -ne $char.Calls) {
            Write-Warning ("  call counts differ (char {0} vs exec {1}). In a release run they should match —" -f $char.Calls, $exec.Calls)
            Write-Warning "  a debug build's assert shadow inflates exec, and anchored retry re-characterizes gaps."
        }
    }

    # Sanity: the baseline shape. If the gate no longer dominates, comparisons to the 2026-07-20 run are void.
    if ($gate.Calls -gt 0 -and $gatePct -lt 50) {
        Write-Warning ("  char_gate_search is only {0:N1}% of this run (baseline: 93.3%). The run shape moved;" -f $gatePct)
        Write-Warning "  do not compare this against the 2026-07-20 numbers without explaining why."
    }
}

# ---- collect logs (running the pairs first, if in Run mode) --------------------------------------

$logFiles = [System.Collections.Generic.List[object]]::new()

if ($PSCmdlet.ParameterSetName -eq 'Run') {
    if (-not (Test-Path $Manifest)) { throw "manifest not found: $Manifest" }
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

    if ($CargoProfile -ne 'release') {
        Write-Warning "profile '$CargoProfile' is not release: timings are meaningless and the debug fill-shadow"
        Write-Warning "assert double-counts exec_fill_assembly. The result will NOT answer M0."
    }

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
        # The label names the log and the WAV, so it has to be a usable filename.
        if ($label -match '[\\/:*?"<>|]') { throw "pair label '$label' contains path-invalid characters" }
        foreach ($p in @($aPath, $bPath)) {
            if (-not (Test-Path $p)) { throw "pair '$label': media not found: $p" }
        }

        $log = Join-Path $OutDir "$label.log"
        $wav = Join-Path $OutDir "$label.wav"

        # A B --wav OUT <shared repair args> <per-pair extra>. `--wav` (not --mux) is the cheapest real write
        # path; `--repair-preview` would skip the executor entirely and measure nothing.
        $argList = @($aPath, $bPath, '--wav', $wav)
        if ($RepairArgs -ne '') { $argList += ($RepairArgs -split '\s+' | Where-Object { $_ -ne '' }) }
        if ($extra -ne '') { $argList += ($extra -split '\s+' | Where-Object { $_ -ne '' }) }

        Write-Host "[$label] repairing (log -> $log)..." -ForegroundColor Cyan
        $env:CLIP_SYNC_SPAN_TIMING = '1'
        $env:RUST_LOG = 'clip_sync_repair=info'
        $sw = [Diagnostics.Stopwatch]::StartNew()
        # Stderr carries the tracing events; capture both streams to the pair log.
        & $exe @argList 2>&1 | Tee-Object -FilePath $log | Out-Null
        $rc = $LASTEXITCODE
        $sw.Stop()
        Remove-Item Env:\CLIP_SYNC_SPAN_TIMING -ErrorAction SilentlyContinue
        Remove-Item Env:\RUST_LOG -ErrorAction SilentlyContinue
        Write-Host ("[$label] exit {0} in {1:N1}s" -f $rc, $sw.Elapsed.TotalSeconds)
        if ($rc -ne 0) { Write-Warning "[$label] exited $rc (log kept at $log)" }

        if (-not $KeepWav -and (Test-Path $wav)) { Remove-Item $wav -Force }

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

foreach ($t in $tallies) {
    Write-TallyReport -T $t
    Write-Verdict -T $t
}

if ($tallies.Count -gt 1) {
    $grand = New-Tally -Label 'ALL PAIRS'
    foreach ($t in $tallies) {
        $grand.Lines += $t.Lines
        foreach ($name in $t.Spans.Keys) {
            if (-not $grand.Spans.ContainsKey($name)) {
                $grand.Spans[$name] = [pscustomobject]@{ Secs = [double]0; Calls = 0 }
            }
            $grand.Spans[$name].Secs += $t.Spans[$name].Secs
            $grand.Spans[$name].Calls += $t.Spans[$name].Calls
        }
    }
    Write-TallyReport -T $grand
    Write-Verdict -T $grand
}

Write-Host ""
Write-Host "Logs: $(($logFiles | ForEach-Object { $_.Path }) -join ', ')" -ForegroundColor DarkGray
Write-Host "Record the number in the M0 ledger row of docs/dev/TEMP-patch-audio-bracket-fill-elimination-plan.md" -ForegroundColor DarkGray
