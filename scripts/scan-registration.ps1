#!/usr/bin/env pwsh
# Scan a manifest of media pairs and capture the **scan-only** JSON report for each — one
# `<label>.json` per pair, carrying `scan.gap_equivalence[*].donor_registration` (lag, peak_r,
# interior levels, `interior_delta_db`, and both dB envelopes) for EVERY gap.
#
# Sibling of `measure-gap-fingerprints.ps1` (same manifest format) but deliberately NOT a dump run:
# no `--gap-fingerprints`, no `--wav`, no `--mux`, no `--repair-preview`. Those four route the run
# into a post-scan arm; omitting all of them lands on `PendingAfterScan::None`
# (`composition.rs` `is_scan_only_run`), which stops at the scan report. That is the whole point —
# donor registration is computed during the scan from block levels alone, so the per-bracket anchor
# oracle (the dominant cost of a fingerprint dump) is pure waste for registration questions.
#
# Usage:
#   ./scripts/scan-registration.ps1 -Manifest pairs.csv
#   ./scripts/scan-registration.ps1 -Manifest pairs.csv -OutDir gap-files/2026-08-04-registration-rate
#   ./scripts/scan-registration.ps1 -Manifest pairs.csv -ScanArgs "--min-gap-ms 500"
#   ./scripts/scan-registration.ps1 -Manifest pairs.csv -SkipBuild -Force
#
# Manifest format (CSV or TSV; '#' comments and blank lines ignored): one pair per line, no header
#     label , path/to/A.mkv , path/to/B.m4v [, extra per-pair args]
# Identical to `measure-gap-fingerprints.ps1` / `measure-repair-perf.ps1` — reuse the same file.
# Delimiter is auto-picked from the extension (.tsv => tab, else comma); override with -Delimiter.
# Fields may be quoted ("C:\my movies\a.mkv"); the 4th field runs to end of line.
#
# STDOUT vs STDERR: the report goes to stdout, progress goes to stderr
# (`StderrProgressReporter`), so the two are captured to separate files and the JSON stays
# parseable. Do not "improve" this into `2>&1 | Tee-Object` the way the repair runner does — that
# interleaves progress lines into the document and nothing downstream will parse it.
#
# PRECHECK: the first pair that reports any gaps is checked for populated
# `donor_registration.envelopes` before the rest of the manifest runs. A misconfigured run (no AAC
# decoder, B not scanned) produces well-formed JSON with the registration silently missing, and
# without this you would discover that after every pair had decoded. Bypass with -SkipPrecheck.
#
# MEDIA HYGIENE: every report contains `video_a` / `video_b` as absolute paths, and so does the
# manifest. -OutDir defaults under `gap-files/`, which is gitignored (`.gitignore`: `/gap-files/`);
# point it elsewhere in the repo and that protection is gone — the script warns but does not refuse.
# -LogDir defaults to $env:TEMP, outside the repo entirely. Neither media filenames nor titles
# belong in the repo: quote results by pair label/index only.
# See docs/dev/repair-perf.md §"Media handling".

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Manifest,

    # Where per-pair scan reports are written, as `<label>.json`.
    [string]$OutDir = (Join-Path (Split-Path -Parent $PSScriptRoot) 'gap-files/registration-scan'),

    # Where per-pair stderr (progress + warnings) goes, as `<label>.log`.
    [string]$LogDir = (Join-Path $env:TEMP 'clip-sync-scan-registration'),

    # Shared scan/recipe args appended after `A B --format json` for every pair.
    # Per-pair `extra` from the manifest is appended after these.
    [string]$ScanArgs = '',

    # Manifest delimiter: 'auto' (by extension), 'comma', or 'tab'.
    [ValidateSet('auto', 'comma', 'tab')]
    [string]$Delimiter = 'auto',

    # Overwrite existing `<label>.json`. Without this, a pair whose report already exists is
    # skipped — so an interrupted run resumes instead of re-decoding what it already has.
    [switch]$Force,

    # Skip the first-pair registration precheck (see PRECHECK above).
    [switch]$SkipPrecheck,

    # Skip `cargo build` and use an existing binary under target/<profile>/.
    [switch]$SkipBuild,

    [string]$CargoProfile = 'release',

    # Codec features must match the media. `he-aac` registers the only AAC decoder — including for
    # AAC-LC — so without it an AAC pair scans to nothing and every gap loses its registration.
    # `calibration` is NOT needed: this path never passes --gap-fingerprints.
    [string]$Features = 'he-aac,ac3'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

if (-not (Test-Path $Manifest)) { throw "manifest not found: $Manifest" }

# Flags that would take the run out of the scan-only arm. Caught here rather than at the binary,
# which would happily accept them and quietly do far more work (or write media).
$forbidden = @('--gap-fingerprints', '--gap-listen', '--wav', '--mux', '--repair-preview', '--format')
foreach ($f in $forbidden) {
    if ($ScanArgs -match [regex]::Escape($f)) {
        throw "-ScanArgs must not contain '$f' — this script runs scan-only with --format json. " +
        "For dumps use measure-gap-fingerprints.ps1; for repairs use repair-directory-pairs.ps1."
    }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

# Reports carry absolute media paths. Inside the repo, only `gap-files/` is gitignored.
$outFull = (Resolve-Path $OutDir).Path.TrimEnd('\', '/')
$repoFull = (Resolve-Path $RepoRoot).Path.TrimEnd('\', '/')
$ignoredRoot = (Join-Path $repoFull 'gap-files').TrimEnd('\', '/')
$cmp = [StringComparison]::OrdinalIgnoreCase
if ($outFull.StartsWith($repoFull, $cmp) -and
    -not ($outFull -eq $ignoredRoot -or $outFull.StartsWith($ignoredRoot + [IO.Path]::DirectorySeparatorChar, $cmp))) {
    Write-Warning "-OutDir is inside the repo but outside gap-files/ — reports carry absolute media paths and are NOT gitignored there: $outFull"
}

$exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair" }

if (-not $SkipBuild) {
    Write-Host "Building clip-sync-repair ($CargoProfile, --features $Features)..." -ForegroundColor Cyan
    $profileFlag = if ($CargoProfile -eq 'release') { '--release' } else { "--profile=$CargoProfile" }
    Push-Location $RepoRoot
    try {
        & cargo build $profileFlag --features $Features -p clip-sync-repair --bin clip-sync-repair
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    }
    finally { Pop-Location }
    $exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair.exe"
    if (-not (Test-Path $exe)) { $exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair" }
}
if (-not (Test-Path $exe)) { throw "repair binary not found at target/$CargoProfile/ (build first or drop -SkipBuild)" }

$delimChar = switch ($Delimiter) {
    'tab' { "`t" }
    'comma' { ',' }
    default { if ([IO.Path]::GetExtension($Manifest) -eq '.tsv') { "`t" } else { ',' } }
}

$dataLines = @(Get-Content $Manifest |
    Where-Object { $_.Trim() -ne '' -and -not $_.Trim().StartsWith('#') })

# Same hand-split as measure-gap-fingerprints.ps1, and for the same reason: `ConvertFrom-Csv` drops
# every field past the 4th without a word, and the `extra` column holds CLI arguments that are
# themselves comma-separated. Quotes are honored where present; unquoted delimiters in the tail are
# rejoined rather than refused.
function Split-ManifestRow {
    param([string]$Line, [string]$Delim)
    $fields = [System.Collections.Generic.List[string]]::new()
    $cur = [System.Text.StringBuilder]::new()
    $inQuotes = $false
    foreach ($ch in $Line.ToCharArray()) {
        if ($ch -eq '"') { $inQuotes = -not $inQuotes; continue }
        if ($ch -eq $Delim -and -not $inQuotes) { $fields.Add($cur.ToString()); $cur.Clear() | Out-Null; continue }
        $cur.Append($ch) | Out-Null
    }
    $fields.Add($cur.ToString())
    [pscustomobject]@{
        label = if ($fields.Count -gt 0) { $fields[0] } else { '' }
        a     = if ($fields.Count -gt 1) { $fields[1] } else { '' }
        b     = if ($fields.Count -gt 2) { $fields[2] } else { '' }
        extra = if ($fields.Count -gt 3) { ($fields[3..($fields.Count - 1)]) -join $Delim } else { '' }
    }
}

$rows = @($dataLines | ForEach-Object { Split-ManifestRow -Line $_ -Delim $delimChar })

# Registration tallies for one report. Deliberately descriptive only — the authoritative
# would-`Apply` answer comes from `equivalence-calibration --replay`, which re-runs the production
# classifier on the recorded envelopes. Reimplementing the classification here would be a second
# opinion nobody asked for; counting fields is not.
function Get-RegistrationSummary {
    param([string]$JsonPath)
    $doc = Get-Content $JsonPath -Raw | ConvertFrom-Json
    $verdicts = @($doc.scan.gap_equivalence)
    $regs = @($verdicts | Where-Object { $null -ne $_.donor_registration } | ForEach-Object { $_.donor_registration })
    [pscustomobject]@{
        Gaps      = @($doc.scan.gaps).Count
        Verdicts  = $verdicts.Count
        Regs      = $regs.Count
        Envelopes = @($regs | Where-Object { $null -ne $_.envelopes }).Count
        NonzeroLag = @($regs | Where-Object { $_.lag_blocks -ne 0 }).Count
        # Mirrors DonorRegistrationParams::default().min_envelope_r — what Apply would refuse to
        # place. A preview of the abstain rate, not a decision.
        LowR      = @($regs | Where-Object { $_.peak_r -lt 0.70 }).Count
        Deltas    = @($regs | Where-Object { $null -ne $_.interior_delta_db }).Count
    }
}

$results = [System.Collections.Generic.List[object]]::new()
$prechecked = $SkipPrecheck.IsPresent

foreach ($row in $rows) {
    $label = ("$($row.label)").Trim()
    $aPath = ("$($row.a)").Trim()
    $bPath = ("$($row.b)").Trim()
    $extra = ("$($row.extra)").Trim()
    if ($label -eq '' -or $aPath -eq '' -or $bPath -eq '') {
        Write-Warning "manifest row skipped (need label,A,B): '$($row.label),$($row.a),$($row.b)'"
        continue
    }
    if ($label -match '[\\/:*?"<>|]') { throw "pair label '$label' contains path-invalid characters" }
    foreach ($p in @($aPath, $bPath)) {
        if (-not (Test-Path $p)) { throw "pair '$label': media not found: $p" }
    }

    $json = Join-Path $OutDir "$label.json"
    $log = Join-Path $LogDir "$label.log"

    # Resume: an existing report is honored only if it PARSES. A failed run leaves an empty
    # stdout file behind, and "the file exists" would otherwise retire that pair permanently —
    # a resumed manifest would report it as done and the corpus would be quietly short a pair.
    $summary = $null
    if ((Test-Path $json) -and -not $Force) {
        try { $summary = Get-RegistrationSummary -JsonPath $json }
        catch { Write-Warning "[$label] existing report did not parse, re-scanning: $json" }
    }
    if ($null -ne $summary) {
        Write-Host "[$label] report exists, skipping (use -Force to redo): $json" -ForegroundColor DarkGray
        $results.Add([pscustomobject]@{
                Label      = $label
                ExitCode   = 0
                Seconds    = $null
                Gaps       = ${summary}?.Gaps
                Regs       = ${summary}?.Regs
                Envelopes  = ${summary}?.Envelopes
                NonzeroLag = ${summary}?.NonzeroLag
                LowR       = ${summary}?.LowR
                Report     = $json
                Log        = $log
                Skipped    = $true
            })
        continue
    }

    # A B --format json [shared] [per-pair extra]. Nothing that leaves the scan-only arm.
    $argList = [System.Collections.Generic.List[string]]::new()
    $argList.AddRange([string[]]@($aPath, $bPath, '--format', 'json'))
    if ($ScanArgs -ne '') {
        foreach ($tok in ($ScanArgs -split '\s+' | Where-Object { $_ -ne '' })) { $argList.Add($tok) }
    }
    if ($extra -ne '') {
        foreach ($tok in ($extra -split '\s+' | Where-Object { $_ -ne '' })) { $argList.Add($tok) }
    }

    Write-Host "[$label] scanning → $json ..." -ForegroundColor Cyan
    $prevLog = $env:RUST_LOG
    if (-not $prevLog) { $env:RUST_LOG = 'warn,clip_sync_repair=info' }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    # Streams kept apart on purpose — see STDOUT vs STDERR above. `$ErrorActionPreference` is
    # relaxed for the call so native stderr does not abort the run before the exit code is read.
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $exe @($argList.ToArray()) 1> $json 2> $log
        $rc = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $prevEap
        if ($null -eq $prevLog) { Remove-Item Env:\RUST_LOG -ErrorAction SilentlyContinue }
        else { $env:RUST_LOG = $prevLog }
    }
    $sw.Stop()

    $summary = $null
    if ($rc -eq 0 -and (Test-Path $json)) {
        try { $summary = Get-RegistrationSummary -JsonPath $json }
        catch { Write-Warning "[$label] report written but did not parse: $_" }
    }

    Write-Host ("[$label] exit {0} in {1:N1}s  gaps={2} reg={3} env={4} lag!=0:{5} r<0.70:{6}" -f
        $rc, $sw.Elapsed.TotalSeconds,
        (${summary}?.Gaps ?? '?'), (${summary}?.Regs ?? '?'), (${summary}?.Envelopes ?? '?'),
        (${summary}?.NonzeroLag ?? '?'), (${summary}?.LowR ?? '?'))
    if ($rc -ne 0) {
        # Drop the stub stdout file so a resumed run re-scans this pair instead of reading its
        # existence as success. The stderr log is what diagnoses the failure, and it is kept.
        Remove-Item $json -Force -ErrorAction SilentlyContinue
        Write-Warning "[$label] exited $rc (stderr kept at $log)"
    }

    # Fail on the FIRST pair that could have shown registration and did not, rather than after the
    # whole manifest has decoded. A pair with no gaps proves nothing either way, so the check moves
    # to the next pair that has some.
    if (-not $prechecked -and $rc -eq 0 -and $null -ne $summary -and $summary.Gaps -gt 0) {
        if ($summary.Regs -eq 0 -or $summary.Envelopes -eq 0) {
            throw @"
[$label] scanned $($summary.Gaps) gap(s) but recorded $($summary.Regs) donor registration(s) and $($summary.Envelopes) envelope(s).
Stopping before the rest of the manifest decodes. Usual causes, in order:
  * built without a decoder for this media — 'he-aac' registers the only AAC decoder, including
    for AAC-LC. Current -Features: $Features
  * B was not scanned (--no-scan-both, or scan_both=false in the config): registration correlates
    A's envelope against B's, so with no B levels there is nothing to align.
  * every gap is not_evaluated (no donor mapped) — check $log and the report's gap_equivalence.
Re-run with -SkipPrecheck to proceed anyway.
"@
        }
        Write-Host "[$label] precheck ok — registration + envelopes present on $($summary.Envelopes)/$($summary.Gaps) gap(s)" -ForegroundColor Green
        $prechecked = $true
    }

    $results.Add([pscustomobject]@{
            Label      = $label
            ExitCode   = $rc
            Seconds    = [math]::Round($sw.Elapsed.TotalSeconds, 1)
            Gaps       = ${summary}?.Gaps
            Regs       = ${summary}?.Regs
            Envelopes  = ${summary}?.Envelopes
            NonzeroLag = ${summary}?.NonzeroLag
            LowR       = ${summary}?.LowR
            Report     = $json
            Log        = $log
            Skipped    = $false
        })
}

Write-Host ''
Write-Host 'Summary' -ForegroundColor Green
'{0,-20} {1,6} {2,8} {3,6} {4,6} {5,6} {6,8} {7,8}' -f 'pair', 'exit', 'secs', 'gaps', 'reg', 'env', 'lag!=0', 'r<0.70'
Write-Host ('-' * 78)
$tGaps = 0; $tReg = 0; $tEnv = 0; $tLag = 0; $tLow = 0
foreach ($r in $results) {
    '{0,-20} {1,6} {2,8} {3,6} {4,6} {5,6} {6,8} {7,8}' -f $r.Label, $r.ExitCode,
    ($null -eq $r.Seconds ? '-' : ('{0:N1}' -f $r.Seconds)),
    ($r.Gaps ?? '?'), ($r.Regs ?? '?'), ($r.Envelopes ?? '?'), ($r.NonzeroLag ?? '?'), ($r.LowR ?? '?')
    $tGaps += ($r.Gaps ?? 0); $tReg += ($r.Regs ?? 0); $tEnv += ($r.Envelopes ?? 0)
    $tLag += ($r.NonzeroLag ?? 0); $tLow += ($r.LowR ?? 0)
}
Write-Host ('-' * 78)
'{0,-20} {1,6} {2,8} {3,6} {4,6} {5,6} {6,8} {7,8}' -f 'TOTAL', '', '', $tGaps, $tReg, $tEnv, $tLag, $tLow

Write-Host ''
Write-Host "Reports: $OutDir" -ForegroundColor DarkGray
Write-Host "Logs:    $LogDir" -ForegroundColor DarkGray
Write-Host ''
# The tallies above are descriptive. `lag!=0` and `r<0.70` are previews of the two rates the
# registration question is about; the would-`Apply` class-flip count needs the production
# classifier, which lives in the replay.
Write-Host "Nonzero registration lag on $tLag of $tReg registered gap(s); peak_r < 0.70 on $tLow." -ForegroundColor DarkGray
Write-Host "These are counts, not the decision — run the replay for what Apply would do." -ForegroundColor DarkGray
Write-Host "Docs:    docs/dev/TEMP-equivalence-band-gate-off-findings.md §6.8.6" -ForegroundColor DarkGray

$failed = @($results | Where-Object { $_.ExitCode -ne 0 })
if ($failed.Count -gt 0) {
    throw "$($failed.Count) pair(s) failed: $($failed.Label -join ', ')"
}
