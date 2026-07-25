#!/usr/bin/env pwsh
# Repair-path performance harness: runs a manifest of media pairs under span timing and reports where the
# time actually goes, as a span tree with exclusive costs.
#
# Reference numbers this produces live in docs/dev/TEMP-anchor-search-perf-baseline.md (derived figures only —
# see MEDIA HYGIENE below). It began life as the M0 harness for the `bracket_fill` elimination plan; that
# question is settled (0.053% over 17 pairs, plan §3.1) so the reporting is now general. Use -Focus to put a
# specific span back under the microscope the way M0 did.
#
# HOW IT MEASURES: `CLIP_SYNC_SPAN_TIMING=1` switches the fmt subscriber to `FmtSpan::CLOSE`, so every span
# emits `time.busy`/`time.idle` when it closes. This sums busy time per span, keyed by its full parent chain.
#
# READING THE OUTPUT — the one thing to get right: spans NEST, and a parent's `time.busy` INCLUDES its
# children's. So the TotalSecs column does not sum to 100%, and a fat parent tells you nothing about where its
# time went. The `ExclSecs` column is the honest one: a span's own busy minus its direct children's. Ranked
# exclusive cost is the "where does the time go" answer, and unlike TotalSecs it IS a partition of the root.
# A large exclusive cost on a span that HAS children means time inside it is covered by no child span — an
# instrumentation gap, not a leaf hotspot. The report calls that out rather than letting it read as a cost.
#
# NON-NEGOTIABLES (the measurement is invalid otherwise):
#   * `--release`. Debug timings are meaningless, and debug-only `debug_assert` shadows re-run real work
#     (e.g. the fill-assembly shadow calls `execute_bracket_fill` an extra time).
#   * A real write path (`--wav`). `--repair-preview` stops after characterize, so nothing on the executor
#     side runs at all. `--gap-fingerprints` is the dump/oracle path, which has a different shape from
#     production (it enumerates brackets exhaustively instead of short-circuiting at the first winner).
#   * `CLIP_SYNC_SPAN_TIMING` set, or no timings emit.
#
# Two modes:
#   Run pairs from a manifest:
#     ./scripts/measure-repair-perf.ps1 -Manifest pairs.csv
#   Roll up logs you already captured (skip the runs):
#     ./scripts/measure-repair-perf.ps1 -Logs perf_logs/
#   Put specific spans back under the microscope:
#     ./scripts/measure-repair-perf.ps1 -Logs perf_logs/ -Focus exec_fill_assembly,gate_anchor_search
#
# Manifest format (CSV or TSV; '#' comments and blank lines ignored): one pair per line, no header row
#     label , path/to/A.mkv , path/to/B.m4v [, extra per-pair repair args]
# The log for each row is named after its label (`<label>.log`), as is its WAV. Delimiter is auto-picked from
# the extension (.tsv => tab, else comma); override with -Delimiter. Fields may be quoted
# ("C:\my movies\a.mkv") so paths with spaces/commas are fine.
#
# `fill_mode` is request-level, not per-gap, so a Fit/Gate comparison comes from running the manifest ONCE PER
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
    [string]$OutDir = (Join-Path $env:TEMP 'clip-sync-repair-perf'),

    # Keep the patched WAVs instead of deleting them after each pair. They are only a side effect here —
    # the measurement is the log — but a full-length WAV per pair is multi-GB, so they go by default.
    # Long surround pairs can exceed the classic-WAV 4 GiB limit and fail AT THE WRITE; that is downstream
    # of every span measured here, so such a log is still valid.
    [Parameter(ParameterSetName = 'Run')]
    [switch]$KeepWav,

    # Manifest delimiter: 'auto' (by extension), 'comma', or 'tab'.
    [Parameter(ParameterSetName = 'Run')]
    [ValidateSet('auto', 'comma', 'tab')]
    [string]$Delimiter = 'auto',

    # Roll up existing logs instead of running. A directory (all *.log) or a glob.
    [Parameter(ParameterSetName = 'Logs', Mandatory = $true)]
    [string]$Logs,

    # Span names to call out individually against the root total, after the tree. This is the generalized
    # form of the old M0 verdict: "what share of the run is span X, and how many times did it run".
    [string[]]$Focus = @(),

    # Hide spans whose inclusive share of the root is below this, to keep deep trees readable. 0 shows all.
    [double]$MinPct = 0.0,

    # cargo build/run profile. Changing this off `release` invalidates the measurement (see header).
    [string]$CargoProfile = 'release',

    # cargo features. Real MKV/MP4 movie audio is usually AC-3/E-AC-3 (`ac3`) or HE-AAC (`he-aac`); without
    # them tracks demux but come back non-decodable. `ffmpeg-mux` is NOT needed (this runs `--wav`).
    # Span emission rides on clip-sync's `default-tracing`, which is on by default.
    [string]$Features = 'he-aac,ac3'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

# `pwsh -File script.ps1 -Focus a,b` binds ONE string "a,b" (with -File every arg is a string, so array
# coercion never happens), unlike `-Command`. Split here so both invocation styles behave the same.
$Focus = @($Focus | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' })

function New-Tally {
    param([string]$Label)
    [pscustomobject]@{
        Label = $Label
        # full chain key ("patch_audio>characterize>char_gate_search") ->
        #   @{ Name; Parent; Depth; Secs; Calls }
        Paths    = @{}
        Lines    = 0
        Unparsed = 0
    }
}

function Add-SpanPath {
    param($T, [string[]]$Chain, [double]$Secs)
    $key = $Chain -join '>'
    if (-not $T.Paths.ContainsKey($key)) {
        $parent = if ($Chain.Count -gt 1) { ($Chain[0..($Chain.Count - 2)] -join '>') } else { '' }
        $T.Paths[$key] = [pscustomobject]@{
            Name   = $Chain[-1]
            Parent = $parent
            Depth  = $Chain.Count - 1
            Secs   = [double]0
            Calls  = 0
        }
    }
    $T.Paths[$key].Secs += $Secs
    $T.Paths[$key].Calls += 1
}

# Split a span chain on ':' at brace/quote depth 0. Span FIELDS can contain colons (`outcome="a:b"`, Windows
# paths), so a naive -split ':' mangles the chain.
function Split-SpanChain {
    param([string]$Chain)
    $out = [System.Collections.Generic.List[string]]::new()
    $depth = 0
    $inQuote = $false
    $sb = [System.Text.StringBuilder]::new()
    foreach ($ch in $Chain.ToCharArray()) {
        if ($ch -eq '"') { $inQuote = -not $inQuote; [void]$sb.Append($ch); continue }
        if (-not $inQuote) {
            if ($ch -eq '{') { $depth++ }
            elseif ($ch -eq '}') { if ($depth -gt 0) { $depth-- } }
            elseif ($ch -eq ':' -and $depth -eq 0) {
                [void]$out.Add($sb.ToString()); [void]$sb.Clear(); continue
            }
        }
        [void]$sb.Append($ch)
    }
    if ($sb.Length -gt 0) { [void]$out.Add($sb.ToString()) }
    return $out
}

# Parse one log's span-close lines into a tally.
#
# A close line looks like:
#   2026-07-24T21:00:00Z  INFO patch_gap{gap_index=1}:char_fill_assembly: clip_sync_repair::x: close time.busy=1.2ms time.idle=4us
# Recovering the FULL chain (not just the innermost span) means trimming three things: the ' close
# time.busy=' tail, the `crate::module::path` target, and the leading timestamp + level. The timestamp
# contains colons, so it has to go before the chain is split.
function Read-LogTally {
    param([string]$Path, [string]$Label)
    $t = New-Tally -Label $Label
    $rxBusy = [regex]'close\s+time\.busy=(?<v>[\d.]+)(?<u>ns|[\u00B5\u03BCu]s|ms|s)\b'
    $rxTarget = [regex]'(?<t>[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z0-9_]+)+)$'
    $rxLevel = [regex]'\s(?:TRACE|DEBUG|INFO|WARN|ERROR)\s+'
    $rxIdent = [regex]'^[A-Za-z_][A-Za-z0-9_]*$'
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
            $prefix = $line.Substring(0, $m.Index).TrimEnd().TrimEnd(':').TrimEnd()
            $tm = $rxTarget.Match($prefix)
            if ($tm.Success) {
                $prefix = $prefix.Substring(0, $tm.Index).TrimEnd().TrimEnd(':').TrimEnd()
            }
            # Drop timestamp + level, or the timestamp's colons become chain separators.
            $lm = $rxLevel.Match($prefix)
            if ($lm.Success) { $prefix = $prefix.Substring($lm.Index + $lm.Length) }

            # Chain elements are `name` or `name{field=v ...}`; keep the names, drop anything unrecognizable.
            $chain = [System.Collections.Generic.List[string]]::new()
            foreach ($part in (Split-SpanChain $prefix)) {
                $name = $part.Trim()
                $brace = $name.IndexOf('{')
                if ($brace -ge 0) { $name = $name.Substring(0, $brace) }
                $name = $name.Trim()
                if ($name -ne '' -and $rxIdent.IsMatch($name)) { [void]$chain.Add($name) }
            }
            if ($chain.Count -eq 0) { $t.Unparsed += 1; continue }

            Add-SpanPath -T $t -Chain $chain.ToArray() -Secs $secs
            $t.Lines += 1
        }
    }
    finally {
        $reader.Dispose()
        $fs.Dispose()
    }
    return $t
}

function Get-ChildKeys {
    param($T, [string]$Key)
    return @($T.Paths.Keys | Where-Object { $T.Paths[$_].Parent -eq $Key })
}

# Exclusive time = own busy minus DIRECT children's busy. Children are matched on the parent-chain key, so
# the same span name appearing under two different parents (char_* under `characterize` vs under
# `patch_anchored_retry`) stays separate here, and is only merged in the by-name roll-up.
function Get-ExclusiveSecs {
    param($T, [string]$Key)
    $kids = 0.0
    foreach ($k in (Get-ChildKeys -T $T -Key $Key)) { $kids += $T.Paths[$k].Secs }
    return $T.Paths[$Key].Secs - $kids
}

function Get-RootSecs {
    param($T)
    $sum = 0.0
    foreach ($k in $T.Paths.Keys) { if ($T.Paths[$k].Depth -eq 0) { $sum += $T.Paths[$k].Secs } }
    return $sum
}

function Add-TreeRows {
    param($T, [string]$Key, $Rows, [double]$RootSecs, [double]$MinPct)
    $p = $T.Paths[$Key]
    $pct = if ($RootSecs -gt 0) { 100 * $p.Secs / $RootSecs } else { 0 }
    if ($MinPct -gt 0 -and $pct -lt $MinPct -and $p.Depth -gt 0) { return }
    $excl = Get-ExclusiveSecs -T $T -Key $Key
    $Rows.Add([pscustomobject]@{
        Span      = ('  ' * $p.Depth) + $p.Name
        TotalSecs = [math]::Round($p.Secs, 3)
        Calls     = $p.Calls
        PerCallMs = [math]::Round(($p.Secs / $p.Calls) * 1000, 2)
        PctOfRoot = if ($RootSecs -gt 0) { [math]::Round($pct, 2) } else { $null }
        ExclSecs  = [math]::Round($excl, 3)
        ExclPct   = if ($RootSecs -gt 0) { [math]::Round(100 * $excl / $RootSecs, 2) } else { $null }
    })
    # Siblings by descending inclusive cost, so the expensive branch reads first.
    foreach ($k in ((Get-ChildKeys -T $T -Key $Key) | Sort-Object { - $T.Paths[$_].Secs })) {
        Add-TreeRows -T $T -Key $k -Rows $Rows -RootSecs $RootSecs -MinPct $MinPct
    }
}

function Write-SpanTree {
    param($T)
    Write-Host ""
    Write-Host ("=== {0} ===" -f $T.Label) -ForegroundColor Cyan
    if ($T.Lines -eq 0) {
        Write-Warning "no span-close lines in this log — was CLIP_SYNC_SPAN_TIMING set, and is this stderr?"
        return
    }

    $roots = @($T.Paths.Keys | Where-Object { $T.Paths[$_].Depth -eq 0 })
    $rootSecs = Get-RootSecs -T $T
    if ($rootSecs -le 0) {
        Write-Warning "no top-level span — shares omitted (did the run reach the instrumented phase?)"
    }
    if ($roots.Count -gt 1) {
        Write-Host ("  ({0} top-level spans; shares are against their combined {1:N1}s)" -f $roots.Count, $rootSecs) -ForegroundColor DarkGray
    }

    $rows = [System.Collections.Generic.List[object]]::new()
    foreach ($r in ($roots | Sort-Object { - $T.Paths[$_].Secs })) {
        Add-TreeRows -T $T -Key $r -Rows $rows -RootSecs $rootSecs -MinPct $MinPct
    }
    $rows | Format-Table -AutoSize

    Write-Host "  TotalSecs NESTS (a parent includes its children, so the column does not sum to 100%)." -ForegroundColor DarkGray
    Write-Host "  ExclSecs is own time minus direct children's — that column IS a partition of the root." -ForegroundColor DarkGray
    if ($MinPct -gt 0) { Write-Host ("  (spans under {0}% of root hidden)" -f $MinPct) -ForegroundColor DarkGray }
}

function Write-HotSpots {
    param($T, [int]$Top = 8)
    if ($T.Lines -eq 0) { return }
    $rootSecs = Get-RootSecs -T $T
    if ($rootSecs -le 0) { return }

    # Merge by span NAME (a name can appear at several places in the tree) and rank by EXCLUSIVE cost. This
    # is the "where does the time actually go" table, and unlike TotalSecs it partitions the root.
    $byName = @{}
    foreach ($k in $T.Paths.Keys) {
        $n = $T.Paths[$k].Name
        if (-not $byName.ContainsKey($n)) { $byName[$n] = [pscustomobject]@{ Excl = [double]0; Calls = 0 } }
        $byName[$n].Excl += (Get-ExclusiveSecs -T $T -Key $k)
        $byName[$n].Calls += $T.Paths[$k].Calls
    }

    Write-Host ""
    Write-Host "--- where the time goes (exclusive, merged by span name) ---" -ForegroundColor Cyan
    $ranked = $byName.GetEnumerator() | Sort-Object { - $_.Value.Excl } | Select-Object -First $Top
    $shown = 0.0
    foreach ($e in $ranked) {
        $shown += $e.Value.Excl
        Write-Host ("  {0,-26} {1,10:N2}s  {2,6:N2}%  ({3} calls)" -f `
            $e.Key, $e.Value.Excl, (100 * $e.Value.Excl / $rootSecs), $e.Value.Calls)
    }
    $rest = $rootSecs - $shown
    if ([math]::Abs($rest) -gt 0.001) {
        Write-Host ("  {0,-26} {1,10:N2}s  {2,6:N2}%" -f '(everything else)', $rest, (100 * $rest / $rootSecs)) -ForegroundColor DarkGray
    }
    Write-Host ("  total instrumented: {0:N1}s" -f $rootSecs) -ForegroundColor DarkGray

    # A big exclusive cost on a span that HAS children means uninstrumented time inside it — worth naming,
    # because it reads like a hotspot but is actually a blind spot.
    foreach ($k in $T.Paths.Keys) {
        if ((Get-ChildKeys -T $T -Key $k).Count -eq 0) { continue }
        $e = Get-ExclusiveSecs -T $T -Key $k
        if ((100 * $e / $rootSecs) -ge 10) {
            Write-Host ("  note: {0} holds {1:N1}s ({2:N1}%) covered by no child span — instrumentation gap, not a leaf cost." -f `
                $T.Paths[$k].Name, $e, (100 * $e / $rootSecs)) -ForegroundColor DarkGray
        }
        if ($e -lt 0 -and (100 * [math]::Abs($e) / $rootSecs) -ge 1) {
            Write-Warning ("  {0} has NEGATIVE exclusive time ({1:N2}s): its children outlast it." -f $T.Paths[$k].Name, $e)
            Write-Warning "  Either a child span escapes the parent's scope, or mismatched logs were merged."
        }
    }
}

function Write-Focus {
    param($T, [string[]]$Names)
    if ($Names.Count -eq 0 -or $T.Lines -eq 0) { return }
    $rootSecs = Get-RootSecs -T $T

    Write-Host ""
    Write-Host "--- focus ---" -ForegroundColor Cyan
    foreach ($n in $Names) {
        $secs = 0.0; $calls = 0; $places = 0
        foreach ($k in $T.Paths.Keys) {
            if ($T.Paths[$k].Name -eq $n) { $secs += $T.Paths[$k].Secs; $calls += $T.Paths[$k].Calls; $places++ }
        }
        if ($calls -eq 0) {
            Write-Warning ("  {0}: never ran. Wrong span name, or this run did not take that path" -f $n)
            Write-Warning "  (e.g. --repair-preview skips everything on the executor side)."
            continue
        }
        $pct = if ($rootSecs -gt 0) { 100 * $secs / $rootSecs } else { 0 }
        $where = if ($places -gt 1) { " across $places places in the tree" } else { '' }
        Write-Host ("  {0,-26} {1,10:N3}s  {2,7:N4}% of root  ({3} calls{4})" -f $n, $secs, $pct, $calls, $where)
    }
}

# ---- collect logs (running the pairs first, if in Run mode) --------------------------------------

$logFiles = [System.Collections.Generic.List[object]]::new()

if ($PSCmdlet.ParameterSetName -eq 'Run') {
    if (-not (Test-Path $Manifest)) { throw "manifest not found: $Manifest" }
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

    if ($CargoProfile -ne 'release') {
        Write-Warning "profile '$CargoProfile' is not release: timings are meaningless, and debug-only assert"
        Write-Warning "shadows re-run real work, so per-span costs will be inflated."
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
        # path; `--repair-preview` would skip the executor entirely.
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

$tallies = @(foreach ($lf in $logFiles) { Read-LogTally -Path $lf.Path -Label $lf.Label })

foreach ($t in $tallies) {
    Write-SpanTree -T $t
    Write-HotSpots -T $t
    Write-Focus -T $t -Names $Focus
}

if ($tallies.Count -gt 1) {
    $grand = New-Tally -Label 'ALL PAIRS'
    foreach ($t in $tallies) {
        $grand.Lines += $t.Lines
        $grand.Unparsed += $t.Unparsed
        foreach ($key in $t.Paths.Keys) {
            if (-not $grand.Paths.ContainsKey($key)) {
                $src = $t.Paths[$key]
                $grand.Paths[$key] = [pscustomobject]@{
                    Name = $src.Name; Parent = $src.Parent; Depth = $src.Depth
                    Secs = [double]0; Calls = 0
                }
            }
            $grand.Paths[$key].Secs += $t.Paths[$key].Secs
            $grand.Paths[$key].Calls += $t.Paths[$key].Calls
        }
    }
    Write-SpanTree -T $grand
    Write-HotSpots -T $grand
    Write-Focus -T $grand -Names $Focus
}

$unparsed = ($tallies | Measure-Object -Property Unparsed -Sum).Sum
if ($unparsed -gt 0) {
    Write-Warning "$unparsed close line(s) had no recoverable span chain and were dropped."
}

Write-Host ""
Write-Host "Logs: $(($logFiles | ForEach-Object { $_.Path }) -join ', ')" -ForegroundColor DarkGray
Write-Host "Baseline figures: docs/dev/TEMP-anchor-search-perf-baseline.md (derived numbers only — never paths)" -ForegroundColor DarkGray
