#!/usr/bin/env pwsh
# Fingerprint a manifest of media pairs: one `--gap-fingerprints` dump per row.
#
# Sibling of `measure-repair-perf.ps1` (same manifest format), but runs the diagnostic dump path
# instead of a repair write. Use this for Phase B corpus placement roll-ups, fixture extraction, or
# any bulk `--gap-fingerprints` pass. Not a perf harness — no span timing.
#
# Usage:
#   ./scripts/measure-gap-fingerprints.ps1 -Manifest pairs.csv
#   ./scripts/measure-gap-fingerprints.ps1 -Manifest pairs.csv -CorpusRoot gap-files/anchor-bracket-corpus
#   ./scripts/measure-gap-fingerprints.ps1 -Manifest pairs.csv -ScanArgs "--min-gap-ms 500" -FingerprintDiagnostics
#   ./scripts/measure-gap-fingerprints.ps1 -Manifest pairs.csv -Check
#   # listenable WAVs too (see LISTEN below) — narrow each row with --fingerprint-gap first:
#   ./scripts/measure-gap-fingerprints.ps1 -Manifest narrowed.csv -ScanArgs "--gap-listen"
#
# Manifest format (CSV or TSV; '#' comments and blank lines ignored): one pair per line, no header
#     label , path/to/A.mkv , path/to/B.m4v [, extra per-pair args]
# Delimiter is auto-picked from the extension (.tsv => tab, else comma); override with -Delimiter.
# Fields may be quoted ("C:\my movies\a.mkv") so paths with spaces/commas are fine.
# The 4th field runs to end of line: `extra` is free-form CLI args, so unquoted delimiters inside it
# are rejoined, not treated as further columns. `label,A,B,--fingerprint-gap 3,7,12` dumps all three.
#
# SELECTING GAPS: use `--fingerprint-gap` (repeatable, or comma-separated), not `--only-gaps`. It is
# the corpus filter, so a narrowed row costs only the named gaps; `--only-gaps` bounds the scan/fill
# plan instead, and is *rejected* outright alongside `--gap-listen` (one selector must drive both the
# corpus and the patch plan). Numbers are 1-BASED, as shown in the repair gap table's `#` column, but
# the emitted filenames are 0-based: `--fingerprint-gap 3` writes `..._g002_....json`. Locate files
# by the A-timeline timestamp in the name rather than by counting.
#
# Each pair writes to <CorpusRoot>/<label>/ (corpus.json + per-gap JSON + manifest.json) and a log
# at <OutDir>/<label>.log.
#
# LISTEN: passing `--gap-listen` through -ScanArgs (or a manifest row's `extra`) additionally writes
# three WAVs per characterized gap — the gap + context from A, the mapped donor span from B, and,
# when the production gate patches, the same A window after the splice. With no value it writes them
# beside each pair's corpus, which is already per-pair, so no extra parameter is needed here. It is
# not free: it runs a production characterize on every selected gap, the dominant cost of a repair
# run. Narrow with `--fingerprint-gap`. Size is modest by comparison — clips are the gap plus
# `gap_signature_context_secs` (default 3 s) on both sides, at the source's own rate/channels/depth,
# so a 1 s gap is ~21 s of audio across the three clips (~4 MB stereo 16-bit, ~18 MB 5.1 24-bit).
# The `clips` summary column counts what landed; `patched` counts the third clip, so `clips` without
# `patched` is a gap the production gate declined. Sharing the corpus dir is safe for -Check: both
# the analyzer and the checker filter on a `.json` extension, so stray WAVs are ignored.
# See docs/dev/gap-fingerprint.md § `--gap-listen` (design record: archive/TEMP-gap-listen-wav-plan.md).
#
# MEDIA HYGIENE: nothing this script produces may be committed, and the two outputs are protected by
# different mechanisms — know which is which before you move either.
#   * -CorpusRoot defaults under `gap-files/`, which is gitignored (`.gitignore`: `/gap-files/`).
#     Point it somewhere else in the repo and the ignore no longer covers it. Listen WAVs land here
#     too: decoded licensed audio, the heaviest artifact here and deletable wholesale.
#   * -OutDir defaults to $env:TEMP — outside the repo entirely, not gitignored. Every log contains
#     absolute paths to licensed media, as does the manifest itself.
# Neither media filenames nor titles belong in the repo; refer to pairs by label or index.
# See docs/dev/repair-perf.md §"Media handling" and docs/dev/gap-fingerprint.md.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Manifest,

    # Shared scan/recipe args appended after `A B --gap-fingerprints DIR` for every pair.
    # Per-pair `extra` from the manifest is appended after these.
    [string]$ScanArgs = '',

    # Where per-pair fingerprint corpora are written. Each pair gets a subdir named for its label.
    [string]$CorpusRoot = (Join-Path (Split-Path -Parent $PSScriptRoot) 'gap-files/fingerprint-corpus'),

    # Where per-pair logs go.
    [string]$OutDir = (Join-Path $env:TEMP 'clip-sync-gap-fingerprints'),

    # Manifest delimiter: 'auto' (by extension), 'comma', or 'tab'.
    [ValidateSet('auto', 'comma', 'tab')]
    [string]$Delimiter = 'auto',

    # Include Tier-3 X-set (`seam_probe`, `wide_envelope`, diagnostic `lag`, `b_levels`).
    [switch]$FingerprintDiagnostics,

    # After all pairs succeed, run `gap-fingerprint-stats --check` on -CorpusRoot (dump integrity).
    # Does not run the prevalence analyzer. Failures exit non-zero.
    [switch]$Check,

    # Skip `cargo build` and use an existing binary under target/<profile>/.
    [switch]$SkipBuild,

    [string]$CargoProfile = 'release',

    # MUST include `calibration` (enables --gap-fingerprints). Codec features match movie audio.
    # `ffmpeg-mux` is not required (this path never muxes).
    [string]$Features = 'he-aac,ac3,calibration'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

if ($Features -notmatch '(^|,)calibration($|,)') {
    throw "-Features must include 'calibration' (enables --gap-fingerprints). Got: $Features"
}
if (-not (Test-Path $Manifest)) { throw "manifest not found: $Manifest" }

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path $CorpusRoot | Out-Null

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

# Hand-split rather than `ConvertFrom-Csv -Header label,a,b,extra`, which DISCARDS every field past
# the 4th without a word. The `extra` column holds CLI arguments, and the ones this script exists to
# pass are themselves comma-separated (`--fingerprint-gap 3,7,12`) — under ConvertFrom-Csv an
# unquoted row truncates that to `3`, exits 0, and fingerprints the wrong gaps.
#
# RFC-4180 quoting is the textbook fix and it does work here, but requiring it is the wrong trade:
# there is no fifth column, so everything after the third delimiter is `extra` by definition and
# there is nothing for quotes to disambiguate. Demanding them only converts a silent wrong answer
# into a loud refusal for a row whose meaning was never in doubt. So: quotes are honored (and
# stripped) where present, and unquoted delimiters in the tail are simply rejoined.
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
    # Fields 4..n are one free-form argument string; rejoin with the delimiter that split them so an
    # unquoted `--only-gaps 3,7,12` round-trips intact.
    [pscustomobject]@{
        label = if ($fields.Count -gt 0) { $fields[0] } else { '' }
        a     = if ($fields.Count -gt 1) { $fields[1] } else { '' }
        b     = if ($fields.Count -gt 2) { $fields[2] } else { '' }
        extra = if ($fields.Count -gt 3) { ($fields[3..($fields.Count - 1)]) -join $Delim } else { '' }
    }
}

$rows = @($dataLines | ForEach-Object { Split-ManifestRow -Line $_ -Delim $delimChar })

$results = [System.Collections.Generic.List[object]]::new()

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

    $corpus = Join-Path $CorpusRoot $label
    $log = Join-Path $OutDir "$label.log"
    New-Item -ItemType Directory -Force -Path $corpus | Out-Null

    # A B --gap-fingerprints DIR [shared] [diagnostics] [per-pair extra]. No --mux/--wav.
    $argList = [System.Collections.Generic.List[string]]::new()
    $argList.AddRange([string[]]@($aPath, $bPath, '--gap-fingerprints', $corpus))
    if ($ScanArgs -ne '') {
        foreach ($tok in ($ScanArgs -split '\s+' | Where-Object { $_ -ne '' })) { $argList.Add($tok) }
    }
    if ($FingerprintDiagnostics) { $argList.Add('--fingerprint-diagnostics') }
    if ($extra -ne '') {
        foreach ($tok in ($extra -split '\s+' | Where-Object { $_ -ne '' })) { $argList.Add($tok) }
    }

    Write-Host "[$label] fingerprinting → $corpus ..." -ForegroundColor Cyan
    $prevLog = $env:RUST_LOG
    if (-not $prevLog) { $env:RUST_LOG = 'warn,clip_sync_repair=info' }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & $exe @($argList.ToArray()) 2>&1 | Tee-Object -FilePath $log | Out-Null
    $rc = $LASTEXITCODE
    $sw.Stop()
    if ($null -eq $prevLog) { Remove-Item Env:\RUST_LOG -ErrorAction SilentlyContinue }
    else { $env:RUST_LOG = $prevLog }

    $corpusJson = Join-Path $corpus 'corpus.json'
    $gapCount = $null
    if (Test-Path $corpusJson) {
        try {
            $gapCount = (Get-Content $corpusJson -Raw | ConvertFrom-Json).gaps.Count
        }
        catch { $gapCount = $null }
    }

    # Listen clips, when the run wrote any. Counted from disk rather than scraped from stdout: the
    # suffixes are the verdict. Every characterized gap gets `_a_surround` + `_b_surround`, but
    # `_a_patched` only exists when the production gate actually patched — so clips-without-patched
    # is a gap the gate declined, which is the thing worth seeing in a bulk run.
    #
    # Only finds clips written *beside* the corpus, which is where bare `--gap-listen` puts them. A
    # row that passes an explicit `--gap-listen DIR` in its `extra` reports blank, not zero.
    $clipCount = $null
    $patchedCount = $null
    $clips = @(Get-ChildItem -Path $corpus -Filter '*.wav' -File -ErrorAction SilentlyContinue)
    if ($clips.Count -gt 0) {
        $clipCount = $clips.Count
        $patchedCount = @($clips | Where-Object { $_.Name.EndsWith('_a_patched.wav') }).Count
    }

    Write-Host ("[$label] exit {0} in {1:N1}s  gaps={2}{3}" -f $rc, $sw.Elapsed.TotalSeconds, $(
            if ($null -eq $gapCount) { '?' } else { $gapCount }
        ), $(
            if ($null -eq $clipCount) { '' } else { "  clips=$clipCount (patched=$patchedCount)" }
        ))
    if ($rc -ne 0) { Write-Warning "[$label] exited $rc (log kept at $log)" }

    $results.Add([pscustomobject]@{
            Label      = $label
            ExitCode   = $rc
            Seconds    = [math]::Round($sw.Elapsed.TotalSeconds, 1)
            Gaps       = $gapCount
            Clips      = $clipCount
            Patched    = $patchedCount
            CorpusDir  = $corpus
            Log        = $log
        })
}

Write-Host ''
Write-Host 'Summary' -ForegroundColor Green
# `clips` / `patched` stay '-' unless the run wrote WAVs, so a plain dump does not look like a
# listen run that exported nothing.
'{0,-20} {1,6} {2,8} {3,5} {4,6} {5,8}' -f 'pair', 'exit', 'secs', 'gaps', 'clips', 'patched'
Write-Host ('-' * 60)
foreach ($r in $results) {
    '{0,-20} {1,6} {2,8:N1} {3,5} {4,6} {5,8}' -f $r.Label, $r.ExitCode, $r.Seconds, $(
        if ($null -eq $r.Gaps) { '?' } else { $r.Gaps }
    ), $(
        if ($null -eq $r.Clips) { '-' } else { $r.Clips }
    ), $(
        if ($null -eq $r.Patched) { '-' } else { $r.Patched }
    )
}
Write-Host ''
Write-Host "Corpus root: $CorpusRoot" -ForegroundColor DarkGray
Write-Host "Logs:        $OutDir" -ForegroundColor DarkGray
Write-Host "Docs:        docs/dev/gap-fingerprint.md" -ForegroundColor DarkGray

$failed = @($results | Where-Object { $_.ExitCode -ne 0 })
if ($failed.Count -gt 0) {
    throw "$($failed.Count) pair(s) failed: $($failed.Label -join ', ')"
}

if ($Check) {
    Write-Host ''
    Write-Host "Running gap-fingerprint-stats --check on $CorpusRoot ..." -ForegroundColor Cyan
    Push-Location $RepoRoot
    try {
        # Stats bin lives on the harness crate; calibration feature matches the dump path.
        & cargo run -p clip-sync-repair-harness --features calibration --bin gap-fingerprint-stats -- --check $CorpusRoot
        if ($LASTEXITCODE -ne 0) {
            throw "gap-fingerprint-stats --check failed (exit $LASTEXITCODE)"
        }
    }
    finally { Pop-Location }
}
