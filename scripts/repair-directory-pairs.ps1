#!/usr/bin/env pwsh
# Discover media pairs in a directory by the `<name>.<ext>` + `<name>.2.<ext>` convention,
# list them, and/or run clip-sync-repair on each pair while saving per-pair logs.
# Can also run from a previously written manifest (same format as measure-repair-perf.ps1).
#
# Pairing rule (same directory, same extension):
#   A (gap file)  = movie.mkv
#   B (donor)     = movie.2.mkv
# Files whose stem already ends in `.2` are never treated as A (avoids looking for `movie.2.2.mkv`).
#
# Usage:
#   ./scripts/repair-directory-pairs.ps1 -MediaDir D:\media\pair-batch
#   ./scripts/repair-directory-pairs.ps1 -MediaDir D:\media -ListOnly
#   ./scripts/repair-directory-pairs.ps1 -MediaDir D:\media -WriteManifest pairs.csv
#   ./scripts/repair-directory-pairs.ps1 -Manifest pairs.csv
#   ./scripts/repair-directory-pairs.ps1 -Manifest pairs.csv -RepairArgs "--no-fft-repeat-band" -KeepWav
#   ./scripts/repair-directory-pairs.ps1 -MediaDir D:\media -Mux D:\out\muxed -MuxExt .mkv
#   ./scripts/repair-directory-pairs.ps1 -MediaDir D:\media -Mux D:\out\muxed -Force
#   ./scripts/repair-directory-pairs.ps1 -MediaDir D:\media -Preview
#
# Manifest format (CSV or TSV; '#' comments and blank lines ignored): one pair per line, no header
#     label , path/to/A.mkv , path/to/B.m4v [, extra per-pair repair args]
# Delimiter is auto-picked from the extension (.tsv => tab, else comma); override with -Delimiter.
#
# Default write path is `--wav` (real repair). Pass -Mux DIR for `--mux` instead (or as well,
# with -KeepWav). Mux/WAV outputs are named from A: `<stem>.repaired.<ext>`
# (e.g. movie.m4v → movie.repaired.mkv with -MuxExt .mkv). Existing mux/WAV targets are
# refused by default; pass -Force to overwrite. Logs stay under -OutDir as `<label>.log`.
# Use -Preview for characterize-only (`--repair-preview`, no splice/write). Shared flags go
# in -RepairArgs; per-pair `extra` from the manifest is appended after those.
#
# MEDIA HYGIENE: pair lists and logs contain absolute media paths / titles. Neither may be
# committed. -OutDir defaults outside the repo ($env:TEMP); if you put output under the repo,
# use the gitignored `gap-files/`. Prefer numeric labels (default) when recording results in
# docs — never the paths. See docs/dev/repair-perf.md §"Media handling".

[CmdletBinding(DefaultParameterSetName = 'RunDir')]
param(
    [Parameter(ParameterSetName = 'RunDir', Mandatory = $true)]
    [Parameter(ParameterSetName = 'ListOnly', Mandatory = $true)]
    [Parameter(ParameterSetName = 'WriteManifest', Mandatory = $true)]
    [string]$MediaDir,

    # Run from an existing pair manifest instead of scanning a directory.
    [Parameter(ParameterSetName = 'RunManifest', Mandatory = $true)]
    [string]$Manifest,

    # List matching pairs and exit (no build, no repair).
    [Parameter(ParameterSetName = 'ListOnly')]
    [switch]$ListOnly,

    # Write a measure-repair-perf / measure-gap-fingerprints manifest and exit (no repair).
    [Parameter(ParameterSetName = 'WriteManifest', Mandatory = $true)]
    [string]$WriteManifest,

    # Shared repair args appended after `A B --wav OUT` (or `--repair-preview`) for every pair.
    # Per-pair `extra` from a manifest is appended after these.
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'RunManifest')]
    [string]$RepairArgs = '',

    # Characterize only — no WAV/mux write. Mutually exclusive with -Mux / -KeepWav.
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'RunManifest')]
    [switch]$Preview,

    # Mux patched audio into video A (`--mux`). Output directory — each pair writes
    # `<A-stem>.repaired.<ext>` there. If you pass a path with an extension (e.g. D:\out\x.mkv)
    # the parent directory is used and that extension becomes the default -MuxExt.
    # Implies the `ffmpeg-mux` cargo feature (added automatically if missing from -Features).
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'RunManifest')]
    [string]$Mux = '',

    # Extension for mux outputs (e.g. '.mkv'). Empty = follow A's extension.
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'RunManifest')]
    [string]$MuxExt = '',

    # Allow overwriting an existing mux/WAV output. Default is to refuse (no-clobber).
    # Logs are always overwritten.
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'RunManifest')]
    [switch]$Force,

    # Where per-pair logs (and WAVs, if written) go. WAV files are `<A-stem>.repaired.wav`.
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'RunManifest')]
    [string]$OutDir = (Join-Path $env:TEMP 'clip-sync-repair-pairs'),

    # Also write `--wav` (and keep it). Without -Mux the default is a throwaway WAV under -OutDir;
    # with -Mux, WAV is omitted unless this switch is set. Prefer -Mux for long surround (classic
    # WAV 4 GiB limit).
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'RunManifest')]
    [switch]$KeepWav,

    # Manifest delimiter: 'auto' (by extension), 'comma', or 'tab'.
    [Parameter(ParameterSetName = 'RunManifest')]
    [ValidateSet('auto', 'comma', 'tab')]
    [string]$Delimiter = 'auto',

    # Label logs/WAV by A stem instead of 1..N (directory discovery only). Stems are often
    # titles — prefer the default numeric labels when anything might be committed or shared.
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'ListOnly')]
    [Parameter(ParameterSetName = 'WriteManifest')]
    [switch]$LabelByStem,

    # Also search subdirectories. Pairing is still per-directory (A and B must share a folder).
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'ListOnly')]
    [Parameter(ParameterSetName = 'WriteManifest')]
    [switch]$Recurse,

    # Optional extension filter, e.g. @('.mkv', '.m4v', '.mp4'). Empty = any extension.
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'ListOnly')]
    [Parameter(ParameterSetName = 'WriteManifest')]
    [string[]]$Extensions = @(),

    # Skip `cargo build` and use an existing binary under target/<profile>/.
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'RunManifest')]
    [switch]$SkipBuild,

    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'RunManifest')]
    [string]$CargoProfile = 'release',

    # Real MKV/MP4 movie audio is usually AC-3/E-AC-3 (`ac3`) or HE-AAC (`he-aac`).
    # -Mux also needs `ffmpeg-mux` (injected below if absent).
    [Parameter(ParameterSetName = 'RunDir')]
    [Parameter(ParameterSetName = 'RunManifest')]
    [string]$Features = 'he-aac,ac3'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

$useMux = $Mux -ne ''
if ($Preview -and $useMux) { throw "-Preview cannot be combined with -Mux" }
if ($Preview -and $KeepWav) { throw "-Preview cannot be combined with -KeepWav" }
if ($MuxExt -ne '' -and -not $useMux) { throw "-MuxExt requires -Mux" }
if ($useMux -and $Features -notmatch '(^|,)ffmpeg-mux($|,)') {
    $Features = "$Features,ffmpeg-mux"
}

function ConvertTo-FileExtension {
    param([string]$Ext)
    if ($Ext -eq '') { return '' }
    if ($Ext.StartsWith('.')) { return $Ext.ToLowerInvariant() }
    return ('.' + $Ext).ToLowerInvariant()
}

# movie.mkv + ext .mkv → movie.repaired.mkv
function Get-RepairedFileName {
    param(
        [string]$APath,
        [string]$Ext
    )
    $stem = [IO.Path]::GetFileNameWithoutExtension($APath)
    $ext = ConvertTo-FileExtension $Ext
    if ($ext -eq '') { $ext = '.mkv' }
    return '{0}.repaired{1}' -f $stem, $ext
}

# -Mux is an output directory. A path with an extension (D:\out\x.mkv) means: use the parent
# as the directory and that extension as the default MuxExt when -MuxExt was not given.
function Resolve-MuxDirectory {
    param(
        [string]$MuxPath,
        [string]$ExtOverride
    )
    $override = ConvertTo-FileExtension $ExtOverride
    $pathExt = [IO.Path]::GetExtension($MuxPath)

    if (Test-Path -LiteralPath $MuxPath -PathType Container) {
        return [pscustomobject]@{
            Dir = (Resolve-Path -LiteralPath $MuxPath).Path
            Ext = $override
        }
    }
    if (Test-Path -LiteralPath $MuxPath -PathType Leaf) {
        throw "-Mux must be a directory (or a not-yet-created directory path); existing file: $MuxPath"
    }

    # Not on disk yet: no extension → create as directory; with extension → parent + default ext.
    if ($pathExt -eq '') {
        New-Item -ItemType Directory -Force -Path $MuxPath | Out-Null
        return [pscustomobject]@{
            Dir = (Resolve-Path -LiteralPath $MuxPath).Path
            Ext = $override
        }
    }

    $parent = Split-Path -Parent $MuxPath
    if (-not $parent) { $parent = (Get-Location).Path }
    if (-not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    $ext = if ($override -ne '') { $override } else { ConvertTo-FileExtension $pathExt }
    return [pscustomobject]@{
        Dir = (Resolve-Path -LiteralPath $parent).Path
        Ext = $ext
    }
}

function Assert-OutputWritable {
    param(
        [string]$Path,
        [string]$PairLabel,
        [string]$Kind
    )
    if ((Test-Path -LiteralPath $Path) -and -not $Force) {
        throw ("pair '{0}': {1} output already exists (pass -Force to overwrite): {2}" -f `
                $PairLabel, $Kind, $Path)
    }
}

function Test-ExtensionAllowed {
    param([string]$Ext)
    if ($Extensions.Count -eq 0) { return $true }
    $norm = $Ext.ToLowerInvariant()
    if (-not $norm.StartsWith('.')) { $norm = ".$norm" }
    foreach ($want in $Extensions) {
        $w = $want.ToLowerInvariant()
        if (-not $w.StartsWith('.')) { $w = ".$w" }
        if ($norm -eq $w) { return $true }
    }
    return $false
}

# Find A/B pairs: A = name.ext, B = name.2.ext (same directory).
function Find-RepairPairs {
    param([string]$Root, [switch]$RecurseDirs)

    $dirs = if ($RecurseDirs) {
        @(Get-ChildItem -LiteralPath $Root -Directory -Recurse) + (Get-Item -LiteralPath $Root)
    } else {
        @(Get-Item -LiteralPath $Root)
    }

    $found = [System.Collections.Generic.List[object]]::new()
    foreach ($dir in $dirs) {
        $files = @(Get-ChildItem -LiteralPath $dir.FullName -File | Where-Object {
                (Test-ExtensionAllowed $_.Extension) -and -not $_.Name.StartsWith('.')
            })
        $byName = @{}
        foreach ($f in $files) { $byName[$f.Name] = $f }

        foreach ($a in ($files | Sort-Object Name)) {
            # Skip donor-side names so we do not chase name.2.2.ext.
            if ($a.BaseName -match '\.2$') { continue }
            $bName = '{0}.2{1}' -f $a.BaseName, $a.Extension
            if (-not $byName.ContainsKey($bName)) { continue }
            $b = $byName[$bName]
            $found.Add([pscustomobject]@{
                    Stem  = $a.BaseName
                    A     = $a.FullName
                    B     = $b.FullName
                    Dir   = $dir.FullName
                    Ext   = $a.Extension
                    Extra = ''
                })
        }
    }
    return $found
}

function Read-ManifestPairs {
    param(
        [string]$Path,
        [string]$DelimMode
    )
    if (-not (Test-Path -LiteralPath $Path)) { throw "manifest not found: $Path" }
    $resolved = (Resolve-Path -LiteralPath $Path).Path
    $delimChar = switch ($DelimMode) {
        'tab' { "`t" }
        'comma' { ',' }
        default { if ([IO.Path]::GetExtension($resolved) -eq '.tsv') { "`t" } else { ',' } }
    }
    $rows = Get-Content -LiteralPath $resolved |
        Where-Object { $_.Trim() -ne '' -and -not $_.Trim().StartsWith('#') } |
        ConvertFrom-Csv -Delimiter $delimChar -Header 'label', 'a', 'b', 'extra'

    $found = [System.Collections.Generic.List[object]]::new()
    foreach ($row in $rows) {
        $label = ("$($row.label)").Trim()
        $aPath = ("$($row.a)").Trim()
        $bPath = ("$($row.b)").Trim()
        $extra = ("$($row.extra)").Trim()
        if ($label -eq '' -or $aPath -eq '' -or $bPath -eq '') {
            Write-Warning "manifest row skipped (need label,A,B): '$($row.label),$($row.a),$($row.b)'"
            continue
        }
        if ($label -match '[\\/:*?"<>|]') {
            throw "pair label '$label' contains path-invalid characters"
        }
        foreach ($p in @($aPath, $bPath)) {
            if (-not (Test-Path -LiteralPath $p)) { throw "pair '$label': media not found: $p" }
        }
        $found.Add([pscustomobject]@{
                Label = $label
                Stem  = [IO.Path]::GetFileNameWithoutExtension($aPath)
                A     = $aPath
                B     = $bPath
                Dir   = [IO.Path]::GetDirectoryName($aPath)
                Ext   = [IO.Path]::GetExtension($aPath)
                Extra = $extra
            })
    }
    return $found
}

# ---- resolve pair list ---------------------------------------------------------------------------

$indexed = [System.Collections.Generic.List[object]]::new()
$sourceDesc = ''

if ($PSCmdlet.ParameterSetName -eq 'RunManifest') {
    $sourceDesc = "manifest $Manifest"
    foreach ($p in (Read-ManifestPairs -Path $Manifest -DelimMode $Delimiter)) {
        $indexed.Add($p)
    }
} else {
    if (-not (Test-Path -LiteralPath $MediaDir -PathType Container)) {
        throw "media directory not found: $MediaDir"
    }
    $MediaDir = (Resolve-Path -LiteralPath $MediaDir).Path
    $sourceDesc = $MediaDir

    $pairs = @(Find-RepairPairs -Root $MediaDir -RecurseDirs:$Recurse)
    if ($pairs.Count -eq 0) {
        throw "no <name>.<ext> + <name>.2.<ext> pairs found under $MediaDir"
    }

    $i = 0
    foreach ($p in $pairs) {
        $i++
        $label = if ($LabelByStem) { $p.Stem } else { "$i" }
        if ($label -match '[\\/:*?"<>|]') {
            throw "pair label '$label' contains path-invalid characters (stem='$($p.Stem)')"
        }
        $indexed.Add([pscustomobject]@{
                Label = $label
                Stem  = $p.Stem
                A     = $p.A
                B     = $p.B
                Dir   = $p.Dir
                Ext   = $p.Ext
                Extra = ''
            })
    }
}

if ($indexed.Count -eq 0) {
    throw "no pairs to process from $sourceDesc"
}

Write-Host ("Found {0} pair(s) from {1}" -f $indexed.Count, $sourceDesc) -ForegroundColor Cyan
Write-Host ''
'{0,-8} {1}' -f 'label', 'A  /  B'
Write-Host ('-' * 72)
foreach ($p in $indexed) {
    '{0,-8} {1}' -f $p.Label, $p.A
    '{0,-8} {1}' -f '', $p.B
    if ($p.Extra -ne '') { '{0,-8} extra: {1}' -f '', $p.Extra }
    Write-Host ''
}

if ($PSCmdlet.ParameterSetName -eq 'ListOnly') {
    return
}

if ($PSCmdlet.ParameterSetName -eq 'WriteManifest') {
    $manifestPath = $WriteManifest
    if (-not [IO.Path]::IsPathRooted($manifestPath)) {
        $manifestPath = Join-Path (Get-Location) $manifestPath
    }
    $parent = Split-Path -Parent $manifestPath
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    $delim = if ([IO.Path]::GetExtension($manifestPath) -eq '.tsv') { "`t" } else { ',' }
    $lines = foreach ($p in $indexed) {
        # Quote paths so spaces/commas survive ConvertFrom-Csv in the measure scripts.
        $qa = '"' + ($p.A -replace '"', '""') + '"'
        $qb = '"' + ($p.B -replace '"', '""') + '"'
        '{0}{1}{2}{1}{3}' -f $p.Label, $delim, $qa, $qb
    }
    Set-Content -LiteralPath $manifestPath -Value $lines -Encoding utf8
    Write-Host "Manifest written: $manifestPath" -ForegroundColor Green
    Write-Host "Re-run with -Manifest, or feed to measure-repair-perf.ps1 / measure-gap-fingerprints.ps1" -ForegroundColor DarkGray
    return
}

# ---- Run repairs ---------------------------------------------------------------------------------

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path

# Drop a local copy of the pair map next to the logs (gitignored / TEMP — not the repo).
$localManifest = Join-Path $OutDir 'pairs.csv'
$manifestLines = foreach ($p in $indexed) {
    $qa = '"' + ($p.A -replace '"', '""') + '"'
    $qb = '"' + ($p.B -replace '"', '""') + '"'
    if ($p.Extra -ne '') {
        $qe = '"' + ($p.Extra -replace '"', '""') + '"'
        '{0},{1},{2},{3}' -f $p.Label, $qa, $qb, $qe
    } else {
        '{0},{1},{2}' -f $p.Label, $qa, $qb
    }
}
Set-Content -LiteralPath $localManifest -Value $manifestLines -Encoding utf8

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
if (-not (Test-Path $exe)) {
    throw "repair binary not found at target/$CargoProfile/ (build first or drop -SkipBuild)"
}

$results = [System.Collections.Generic.List[object]]::new()

# Without -Mux the real write path is a (usually throwaway) WAV. With -Mux, WAV is only
# requested when -KeepWav is also set.
$wantWav = -not $Preview -and ((-not $useMux) -or $KeepWav)

$muxDir = $null
$muxExtResolved = ''
if ($useMux) {
    $muxResolved = Resolve-MuxDirectory -MuxPath $Mux -ExtOverride $MuxExt
    $muxDir = $muxResolved.Dir
    $muxExtResolved = $muxResolved.Ext
}

# Fail early on duplicate output paths (e.g. same A stem under -Recurse).
$plannedOutputs = @{}
foreach ($p in $indexed) {
    if ($wantWav) {
        $wavPath = Join-Path $OutDir (Get-RepairedFileName -APath $p.A -Ext '.wav')
        if ($plannedOutputs.ContainsKey($wavPath)) {
            throw ("WAV output collision: '{0}' and '{1}' both map to {2}" -f `
                    $plannedOutputs[$wavPath], $p.Label, $wavPath)
        }
        $plannedOutputs[$wavPath] = $p.Label
    }
    if ($useMux) {
        $aExt = [IO.Path]::GetExtension($p.A)
        if ($aExt -eq '') { $aExt = '.mkv' }
        $ext = if ($muxExtResolved -ne '') { $muxExtResolved } else { $aExt }
        $muxPath = Join-Path $muxDir (Get-RepairedFileName -APath $p.A -Ext $ext)
        if ($plannedOutputs.ContainsKey($muxPath)) {
            throw ("mux output collision: '{0}' and '{1}' both map to {2}" -f `
                    $plannedOutputs[$muxPath], $p.Label, $muxPath)
        }
        $plannedOutputs[$muxPath] = $p.Label
    }
}

foreach ($p in $indexed) {
    $log = Join-Path $OutDir "$($p.Label).log"
    $wav = $null
    if ($wantWav) {
        $wav = Join-Path $OutDir (Get-RepairedFileName -APath $p.A -Ext '.wav')
        # Only guard kept WAVs; throwaway temps are deleted after the run.
        if ($KeepWav) { Assert-OutputWritable -Path $wav -PairLabel $p.Label -Kind 'WAV' }
    }
    $muxOut = $null
    if ($useMux) {
        $aExt = [IO.Path]::GetExtension($p.A)
        if ($aExt -eq '') { $aExt = '.mkv' }
        $ext = if ($muxExtResolved -ne '') { $muxExtResolved } else { $aExt }
        $muxOut = Join-Path $muxDir (Get-RepairedFileName -APath $p.A -Ext $ext)
        Assert-OutputWritable -Path $muxOut -PairLabel $p.Label -Kind 'mux'
    }

    $argList = [System.Collections.Generic.List[string]]::new()
    $argList.Add($p.A)
    $argList.Add($p.B)
    if ($Preview) {
        $argList.Add('--repair-preview')
    } else {
        if ($wantWav) {
            $argList.Add('--wav')
            $argList.Add($wav)
        }
        if ($useMux) {
            $argList.Add('--mux')
            $argList.Add($muxOut)
        }
    }
    if ($RepairArgs -ne '') {
        foreach ($tok in ($RepairArgs -split '\s+' | Where-Object { $_ -ne '' })) {
            $argList.Add($tok)
        }
    }
    if ($p.Extra -ne '') {
        foreach ($tok in ($p.Extra -split '\s+' | Where-Object { $_ -ne '' })) {
            $argList.Add($tok)
        }
    }

    $mode = if ($Preview) { 'preview' } elseif ($useMux) { 'mux' } else { 'repair' }
    $destNote = if ($useMux) { "; mux -> $muxOut" } elseif ($wantWav -and $KeepWav) { "; wav -> $wav" } else { '' }
    Write-Host "[$($p.Label)] $mode (log -> $log$destNote)..." -ForegroundColor Cyan
    $prevLog = $env:RUST_LOG
    if (-not $prevLog) { $env:RUST_LOG = 'warn,clip_sync_repair=info' }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    # Stderr carries tracing; capture both streams to the pair log.
    & $exe @($argList.ToArray()) 2>&1 | Tee-Object -FilePath $log | Out-Null
    $rc = $LASTEXITCODE
    $sw.Stop()
    if ($null -eq $prevLog) { Remove-Item Env:\RUST_LOG -ErrorAction SilentlyContinue }
    else { $env:RUST_LOG = $prevLog }

    if ($wantWav -and -not $KeepWav -and $null -ne $wav -and (Test-Path -LiteralPath $wav)) {
        Remove-Item -LiteralPath $wav -Force
    }

    Write-Host ("[{0}] exit {1} in {2:N1}s" -f $p.Label, $rc, $sw.Elapsed.TotalSeconds)
    if ($rc -ne 0) { Write-Warning "[$($p.Label)] exited $rc (log kept at $log)" }

    $results.Add([pscustomobject]@{
            Label    = $p.Label
            ExitCode = $rc
            Seconds  = [math]::Round($sw.Elapsed.TotalSeconds, 1)
            Log      = $log
            Wav      = if ($wantWav -and $KeepWav) { $wav } else { $null }
            Mux      = $muxOut
        })
}

Write-Host ''
Write-Host 'Summary' -ForegroundColor Green
if ($useMux) {
    '{0,-8} {1,6} {2,8}  {3}' -f 'label', 'exit', 'secs', 'mux'
    Write-Host ('-' * 72)
    foreach ($r in $results) {
        '{0,-8} {1,6} {2,8:N1}  {3}' -f $r.Label, $r.ExitCode, $r.Seconds, $r.Mux
    }
} else {
    '{0,-8} {1,6} {2,8}' -f 'label', 'exit', 'secs'
    Write-Host ('-' * 28)
    foreach ($r in $results) {
        '{0,-8} {1,6} {2,8:N1}' -f $r.Label, $r.ExitCode, $r.Seconds
    }
}
Write-Host ''
Write-Host "Manifest: $localManifest" -ForegroundColor DarkGray
Write-Host "Logs:     $OutDir" -ForegroundColor DarkGray


$failed = @($results | Where-Object { $_.ExitCode -ne 0 })
if ($failed.Count -gt 0) {
    throw "$($failed.Count) pair(s) failed: $($failed.Label -join ', ')"
}
