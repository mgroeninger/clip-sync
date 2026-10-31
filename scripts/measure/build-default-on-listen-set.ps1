#!/usr/bin/env pwsh
#Requires -Version 7.0
# Blind A/B listen set for the edge-snap default-on check (docs/dev/TEMP-soft-edge-census-plan.md
# § 5, "Step 3 — listen protocol"). Sibling of build-edge-snap-listen-set.ps1, which stays the
# executable record of Phase 3.
#
# Renders each selected gap twice from REAL repair output: *current* (defaults) and *snapped*
# (`--edge-snap-max-ms 40`). It checks that each arm reproduces its corpus report, so the listener
# hears the re-placement the corpus made, then writes a shuffled set of `tNN_a.wav` / `tNN_b.wav`
# trials, plus a key the listener must not open until every verdict is in. The strata, walk orders,
# quotas, controls, seed and decision rule live in § 5. This script is their executable half and
# must agree with them. Change a list here only together with § 5, and only before listening starts.
#
# The walk orders were drawn once from the strata (seed 20261031, sort by (pair, gap), then shuffle).
# They cannot be re-derived here, because the draw used Python's RNG. Instead, every listed gap's
# stratum is re-derived from the two corpus reports and the census before any run, and must match.
#
# SELECTION WALK: round 1 runs the first `quota` gaps of every stratum, both named gaps and both
# controls. Each later round runs only replacements, the next walk rank of a stratum short of its
# quota, never past rank 8. One repair per (round, pair, arm), with `--only-gaps` naming that round's
# gaps. A failed candidate is recorded with its reason. A failed named gap is recorded and not
# replaced. A failed control stops the script.
#
# CHECKS (per gap; see § 5):
#   1. reference match (all gaps): the current arm reproduces the 2026-10-26 report: patched, same
#      a_start, anchor_seam_used, dual_fit_used, patch_tier and adjust frames, and
#      align_adjustment_secs and registration_residual_ms to 0.1 ms.
#   Re-placed strata (named, anchor, dual, tier, moved):
#   2. candidate match: the snapped arm reproduces the 2026-10-30 guarded report: the same
#      anchor_seam_used, dual_fit_used, patch_tier and adjust frames, and align_adjustment_secs to 0.1 ms.
#   3. confined: the arms are byte-identical outside [earliest write start - 12 ms, latest write end
#      + 12 ms], over both arms. A write runs from a_start + start adjust to a_end + end adjust, and the
#      snapped arm's also reaches out by each census onset <= 40 ms.
#   Trail stratum and controls (Phase 3 checks; check 2 is the trail-onset variant):
#   2. snap reached the write: the arms first differ within [post seam - cf - 2 ms, post seam + 1 ms]
#      and are identical from trail onset + cf + 2 ms past the post seam on. Controls are byte-identical.
#   3. placement only: the snapped arm keeps dual_fit_used and moves align_adjustment by <= 1 ms.
#
# CLIP: 4 s before the earlier write start to 4 s after the later write end, the same window for
# both arms.
#
# OUTPUT:
#   -BlindDir (gap-files/2026-10-31-blind-default-on/): tNN_a.wav, tNN_b.wav, README.txt, labels.csv
#   -KeyDir   (gap-files/2026-10-31-blind-default-on-key/): key.json. DO NOT OPEN before scoring. This
#             script never prints it.
#   -WorkDir: reports, logs, cut PCM, flac-mux.toml. The labelled renders live here, so point it at
#             the session scratchpad, never at gap-files/.
#
# RESUME: a run whose report and cut PCM exist is not repeated. The muxed .mkv is deleted after
# cutting unless -KeepMux. -Force re-runs everything.
#
# Usage:
#   ./scripts/measure/build-default-on-listen-set.ps1 -PlanOnly
#   ./scripts/measure/build-default-on-listen-set.ps1 -WorkDir <scratchpad>/default-on-work
#
# MEDIA HYGIENE: the manifest, reports and logs carry absolute media paths. This script prints pair
# labels, gap indices and numeric fields only. Of the census, it reads only pair, gap_index, edge and
# onset_near_ms.
[CmdletBinding()]
param(
    [string]$Manifest = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'gap-files/corpus-pairs.csv'),
    [string]$RefDir = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'gap-files/2026-10-26-edge-onset'),
    [string]$CandDir = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'gap-files/2026-10-30-edge-snap-guard'),
    [string]$BlindDir = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'gap-files/2026-10-31-blind-default-on'),
    [string]$KeyDir = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'gap-files/2026-10-31-blind-default-on-key'),
    [string]$WorkDir = (Join-Path ([System.IO.Path]::GetTempPath()) 'clip-sync-default-on-listen'),

    # Fixed in § 5. Changing it builds a different set.
    [int]$Seed = 20261031,

    [switch]$PlanOnly,
    [switch]$KeepMux,
    [switch]$Force,
    [switch]$SkipBuild,
    [string]$CargoProfile = 'release',
    [string]$Features = 'he-aac,ac3,ffmpeg-mux'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

# ---- design (must match TEMP-soft-edge-census-plan.md § 5, Step 3) -------------------------------
$Strata = [ordered]@{
    named  = @{ Quota = 2; Walk = @('69/9', '32/10') }
    anchor = @{ Quota = 3; Walk = @('48/3', '48/5', '83/7', '50/8', '49/34', '85/85', '7/4', '86/12') }
    dual   = @{ Quota = 3; Walk = @('68/3', '17/34', '43/7', '10/8', '26/6', '33/14', '80/11', '79/9') }
    tier   = @{ Quota = 3; Walk = @('45/1', '69/5', '39/7', '84/9', '32/13', '68/2', '5/3', '63/5') }
    moved  = @{ Quota = 3; Walk = @('71/12', '76/3', '13/31', '13/17', '64/46', '23/4', '3/17', '9/5') }
    trail  = @{ Quota = 4; Walk = @('12/1', '51/1', '37/5', '11/4', '85/88', '80/8', '72/6', '65/9') }
}
$Replaced = @('named', 'anchor', 'dual', 'tier', 'moved')
$Controls = @('69/6', '32/9')
$Repeats = 2                    # drawn from the accepted re-placed gaps
$SnappedOnA = 10                # of the 20 non-control trials
$SnapArgs = @('--edge-snap-max-ms', '40')
$CapMs = 40.0
$ContextSecs = 4.0
$CrossfadeMs = 10.0             # default crossfade_ms
$WindowSlackMs = 2.0
$WindowLateMs = 1.0
$ConfineMs = 12.0
$ReportTolMs = 0.1
$PlacementTolMs = 1.0
# -------------------------------------------------------------------------------------------------

function Split-Key([string]$k, [string]$stratum) {
    $p = $k.Split('/'); [pscustomobject]@{ Pair = $p[0]; Gap = [int]$p[1]; Key = $k; Stratum = $stratum }
}
$all = @(foreach ($s in $Strata.Keys) { $Strata[$s].Walk }) + $Controls
$dupes = $all | Group-Object | Where-Object Count -gt 1
if ($dupes) { throw "gap listed twice: $(($dupes | ForEach-Object Name) -join ', ')" }

# ---- census onsets -------------------------------------------------------------------------------
$censusPath = Join-Path $RefDir 'edge-onset/census.csv'
if (-not (Test-Path -LiteralPath $censusPath)) { throw "census not found: $censusPath" }
$leadMs = @{}; $trailMs = @{}   # census onset_near_ms; absent when the edge has no onset
foreach ($r in (Import-Csv -LiteralPath $censusPath | Select-Object pair, gap_index, edge, onset_near_ms)) {
    if ($r.onset_near_ms -eq '') { continue }
    $k = "$($r.pair)/$([int]$r.gap_index)"
    if ($r.edge -eq 'lead') { $leadMs[$k] = [double]$r.onset_near_ms } elseif ($r.edge -eq 'trail') { $trailMs[$k] = [double]$r.onset_near_ms }
}
# The snap moves an edge out by its onset when the onset is within the cap (the zero-run guard refuses
# only 10/g11 and 14/g10, neither listed). Used for the write extents and check 2's windows.
function Get-SnapMs([hashtable]$m, [string]$k) { if ($m.ContainsKey($k) -and $m[$k] -le $CapMs) { $m[$k] } else { 0.0 } }

# ---- corpus reports: re-derive every listed gap's stratum ---------------------------------------
function Read-Json([string]$p) { Get-Content -LiteralPath $p -Raw | ConvertFrom-Json -Depth 64 }
$reports = @{}
function Get-Report([string]$dir, [string]$pair) {
    $k = "$dir|$pair"
    if (-not $reports.ContainsKey($k)) { $reports[$k] = Read-Json (Join-Path $dir "$pair.json") }
    $reports[$k]
}
function Get-Rate($doc) { [int]$doc.scan.track_compatibility.a_sample_rate }

# Primary stratum as strata.py assigns it; 'unpatched' when either corpus arm is not patched.
function Get-Stratum([string]$pair, [int]$gap) {
    $rd = Get-Report $RefDir $pair; $cd = Get-Report $CandDir $pair
    $rg = $rd.patch.gaps[$gap]; $cg = $cd.patch.gaps[$gap]
    $rp = $rg.status.patched; $cp = $cg.status.patched
    if ($null -eq $rp -or $null -eq $cp) { return 'unpatched' }
    if ([bool]$rp.anchor_seam_used -ne [bool]$cp.anchor_seam_used) { return 'anchor' }
    if ([bool]$rp.dual_fit_used -ne [bool]$cp.dual_fit_used) { return 'dual' }
    if ($rg.tags.patch_tier -ne $cg.tags.patch_tier) { return 'tier' }
    $fpm = (Get-Rate $rd) / 1000.0
    $move = [math]::Abs($cp.align_adjustment_secs - $rp.align_adjustment_secs) * 1000
    foreach ($k in 'gap_start_adjust_frames', 'gap_end_adjust_frames', 'anchor_bracket_move_frames') {
        $move = [math]::Max($move, [math]::Abs([long]$cp.$k - [long]$rp.$k) / $fpm)
    }
    if ($move -gt $CapMs) { return 'moved' }
    ''
}

$walk = [ordered]@{}
foreach ($s in $Strata.Keys) {
    $walk[$s] = @($Strata[$s].Walk | ForEach-Object { Split-Key $_ $s })
    foreach ($c in $walk[$s]) {
        $got = Get-Stratum $c.Pair $c.Gap
        $ok = switch ($s) {
            'named' { $got -in 'anchor', 'dual', 'tier', 'moved' }
            'trail' { $got -eq '' -and (Get-SnapMs $trailMs $c.Key) -gt 0 -and (Get-SnapMs $leadMs $c.Key) -eq 0 }
            default { $got -eq $s }
        }
        if (-not $ok) { throw "$($c.Key) is listed under '$s' but the reports and census give '$got'" }
    }
}
$ctrl = @($Controls | ForEach-Object { Split-Key $_ 'control' })
foreach ($c in $ctrl) {
    $rg = (Get-Report $RefDir $c.Pair).patch.gaps[$c.Gap]; $cg = (Get-Report $CandDir $c.Pair).patch.gaps[$c.Gap]
    if ($null -eq $rg.status.patched -or $leadMs.ContainsKey($c.Key) -or $trailMs.ContainsKey($c.Key) -or
        (ConvertTo-Json $rg -Depth 64 -Compress) -ne (ConvertTo-Json $cg -Depth 64 -Compress)) {
        throw "control $($c.Key) is not a patched gap, unchanged between the corpus runs, with no onset on either edge"
    }
}

$targetTotal = ($Strata.Values | ForEach-Object Quota | Measure-Object -Sum).Sum
Write-Host ("design: {0} A/B gaps + {1} repeats + {2} controls = {3} trials; seed {4}" -f `
    $targetTotal, $Repeats, $Controls.Count, ($targetTotal + $Repeats + $Controls.Count), $Seed)
foreach ($s in $walk.Keys) {
    $q = $Strata[$s].Quota
    Write-Host ("  {0,-7} quota {1}: {2}" -f $s, $q, (($walk[$s] | ForEach-Object -Begin { $i = 0 } -Process {
        $i++; $lead = Get-SnapMs $leadMs $_.Key; $trail = Get-SnapMs $trailMs $_.Key
        "{0}{1} [{2}/{3}]" -f $_.Key, $(if ($i -le $q) { '*' } else { '' }), $(if ($lead) { '{0:0.0}' -f $lead } else { '-' }), $(if ($trail) { '{0:0.0}' -f $trail } else { '-' })
    }) -join ', '))
}
Write-Host ("  control: {0}   (* = round 1; [lead/trail snap ms])" -f ($Controls -join ', '))
function Show-Round($gaps) {
    foreach ($grp in ($gaps | Group-Object Pair | Sort-Object { [int]$_.Name })) {
        $only = ($grp.Group | ForEach-Object { $_.Gap + 1 } | Sort-Object) -join ','
        Write-Host ("  pair {0,-3} --only-gaps {1,-12} x 2 arms (current, snapped {2})" -f $grp.Name, $only, ($SnapArgs -join ' '))
    }
}
$next = @{}
$round1 = @(foreach ($s in $walk.Keys) { $next[$s] = $Strata[$s].Quota; $walk[$s] | Select-Object -First $Strata[$s].Quota }) + $ctrl
Write-Host "round 1 runs ($(@($round1 | Group-Object Pair).Count) pairs):"
Show-Round $round1
if ($PlanOnly) { return }

# ---- numeric helpers -------------------------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
public static class DefaultOnDsp {
    public static short[] ReadS16(string path) {
        var b = System.IO.File.ReadAllBytes(path);
        var s = new short[b.Length / 2];
        Buffer.BlockCopy(b, 0, s, 0, s.Length * 2);
        return s;
    }
    // {first, last} frame where any channel differs; {-1, -1} if identical; {-2, -2} on length mismatch.
    public static long[] DiffSpan(short[] a, short[] b, int ch) {
        if (a.Length != b.Length) return new long[] { -2, -2 };
        long n = a.Length / ch, first = -1, last = -1;
        for (long f = 0; f < n; f++) {
            for (int c = 0; c < ch; c++) {
                if (a[f * ch + c] != b[f * ch + c]) { if (first < 0) first = f; last = f; break; }
            }
        }
        return new long[] { first, last };
    }
}
'@

# ---- manifest ------------------------------------------------------------------------------------
if (-not (Test-Path -LiteralPath $Manifest)) { throw "manifest not found: $Manifest" }
$pairs = @{}
foreach ($line in Get-Content -LiteralPath $Manifest) {
    if ($line -match '^\s*(#|$)') { continue }
    # Quote-aware split, as in measure-patch-outcomes.ps1: titles may contain the delimiter.
    $f = [System.Collections.Generic.List[string]]::new(); $cur = [System.Text.StringBuilder]::new(); $q = $false
    foreach ($ch in $line.ToCharArray()) {
        if ($ch -eq '"') { $q = -not $q; continue }
        if ($ch -eq ',' -and -not $q) { $f.Add($cur.ToString()); [void]$cur.Clear(); continue }
        [void]$cur.Append($ch)
    }
    $f.Add($cur.ToString())
    if ($f.Count -ge 3) { $pairs[$f[0].Trim()] = @{ A = $f[1].Trim(); B = $f[2].Trim() } }
}
foreach ($c in @($walk.Values | ForEach-Object { $_ }) + $ctrl) { if (-not $pairs.ContainsKey($c.Pair)) { throw "pair $($c.Pair) not in manifest" } }

# ---- build ---------------------------------------------------------------------------------------
$exe = Join-Path $RepoRoot "target/$CargoProfile/clip-sync-repair.exe"
if (-not $SkipBuild) {
    Push-Location $RepoRoot
    try {
        $profileArg = if ($CargoProfile -eq 'release') { '--release' } else { "--profile=$CargoProfile" }
        cargo build $profileArg -p clip-sync-repair --features $Features
        if ($LASTEXITCODE) { throw "cargo build failed ($LASTEXITCODE)" }
    }
    finally { Pop-Location }
}
if (-not (Test-Path -LiteralPath $exe)) { throw "binary not found: $exe" }
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) { throw 'ffmpeg not on PATH' }

$runDir = Join-Path $WorkDir 'runs'
New-Item -ItemType Directory -Force -Path $runDir, $BlindDir, $KeyDir | Out-Null
$cfg = Join-Path $runDir 'flac-mux.toml'
Set-Content -LiteralPath $cfg -Encoding utf8 -Value @'
# Generated by build-default-on-listen-set.ps1: lossless mux so clips carry the patched PCM exactly.
[repair.output]
video_codec = "copy"
audio_codec = "flac"
mux_audio_bitrate = "default"
'@

# One repair per (round, pair, arm). Re-run only when the report or any cut is missing and the mux is gone.
function Invoke-Arm([int]$round, [string]$pair, [string]$arm, [int[]]$gaps) {
    $id = "r{0}-{1}-{2}" -f $round, $pair, $arm
    $report = Join-Path $runDir "$id.json"; $log = Join-Path $runDir "$id.log"; $mkv = Join-Path $runDir "$id.mkv"
    $raws = $gaps | ForEach-Object { Join-Path $runDir ("{0}-g{1}.raw" -f $id, $_) }
    $haveReport = (Test-Path -LiteralPath $report) -and ((Get-Item -LiteralPath $report).Length -gt 0)
    $haveCuts = -not ($raws | Where-Object { -not (Test-Path -LiteralPath $_) })
    if ($Force -or -not $haveReport -or (-not $haveCuts -and -not (Test-Path -LiteralPath $mkv))) {
        $p = $pairs[$pair]
        $only = ($gaps | ForEach-Object { $_ + 1 } | Sort-Object) -join ','
        $argv = @($p.A, $p.B, '-c', $cfg, '--mux', $mkv, '--format', 'json', '--only-gaps', $only)
        if ($arm -eq 'snapped') { $argv += $SnapArgs }
        Write-Host ("[{0}] repair {1} (--only-gaps {2}) ..." -f (Get-Date -Format 'HH:mm'), $id, $only)
        & $exe @argv > $report 2> $log
        if ($LASTEXITCODE -ne 0) { throw "repair $id exited $LASTEXITCODE (see its log in the work dir)" }
    }
    $doc = Read-Json $report
    @{ id = $id; doc = $doc; mkv = $mkv; sr = (Get-Rate $doc); ch = [int]$doc.scan.track_compatibility.a_channels }
}

function Get-Cut($arm, [int]$gap, [long]$f0, [long]$f1) {
    $raw = Join-Path $runDir ("{0}-g{1}.raw" -f $arm.id, $gap)
    if ($Force -or -not (Test-Path -LiteralPath $raw)) {
        if (-not (Test-Path -LiteralPath $arm.mkv)) { throw "cut needed but $($arm.id).mkv is gone; re-run with -Force" }
        # atrim counts samples from the stream start, independent of container timestamps.
        ffmpeg -v error -y -i $arm.mkv -map 0:a:0 -af "atrim=start_sample=$($f0):end_sample=$($f1)" -f s16le -c:a pcm_s16le $raw
        if ($LASTEXITCODE) { throw "cut failed: $($arm.id) g$gap" }
    }
    $raw
}

# Placement fields that differ between a report gap and a corpus gap; residual only for check 1.
function Compare-Placement($g, $ref, [bool]$withResidual) {
    $p = $g.status.patched; $rp = $ref.status.patched
    $bad = @()
    if ([math]::Abs($g.a_start_secs - $ref.a_start_secs) -gt 1e-6) { $bad += 'a_start' }
    foreach ($k in 'anchor_seam_used', 'dual_fit_used') { if ([bool]$p.$k -ne [bool]$rp.$k) { $bad += $k } }
    foreach ($k in 'gap_start_adjust_frames', 'gap_end_adjust_frames', 'anchor_bracket_move_frames') { if ([long]$p.$k -ne [long]$rp.$k) { $bad += $k } }
    if ($g.tags.patch_tier -ne $ref.tags.patch_tier) { $bad += 'patch_tier' }
    if ([math]::Abs(($p.align_adjustment_secs - $rp.align_adjustment_secs) * 1000) -gt $ReportTolMs) { $bad += 'align_adjustment' }
    if ($withResidual -and [math]::Abs($p.registration_residual_ms - $rp.registration_residual_ms) -gt $ReportTolMs) { $bad += 'residual' }
    $bad
}

# Report-level checks; returns $null on pass, else the reason.
function Test-Report($c, $curG, $snpG) {
    $rg = (Get-Report $RefDir $c.Pair).patch.gaps[$c.Gap]
    if ($null -eq $curG.status.patched) { return 'check 1: current arm is not patched' }
    $bad = Compare-Placement $curG $rg $true
    if ($bad) { return "check 1: current arm differs from the 2026-10-26 report on $($bad -join ', ')" }
    if ($null -eq $snpG.status.patched) { return 'snapped arm is not patched' }
    if ($c.Stratum -in $Replaced) {
        $bad = Compare-Placement $snpG (Get-Report $CandDir $c.Pair).patch.gaps[$c.Gap] $false
        if ($bad) { return "check 2: snapped arm differs from the 2026-10-30 report on $($bad -join ', ')" }
        return $null
    }
    $p = $curG.status.patched; $s = $snpG.status.patched
    if ([bool]$s.dual_fit_used -ne [bool]$p.dual_fit_used) { return 'check 3: snapped arm changes dual_fit_used' }
    $move = ($s.align_adjustment_secs - $p.align_adjustment_secs) * 1000
    if ([math]::Abs($move) -gt $PlacementTolMs) { return ('check 3: snapped arm moves align_adjustment by {0:0.00} ms' -f $move) }
    $null
}

# Write extent [start, end) in seconds on A's timeline.
function Get-Extent($g, [int]$sr, [bool]$snapped, [string]$key) {
    $p = $g.status.patched
    $s = $g.a_start_secs + [long]$p.gap_start_adjust_frames / $sr
    $e = $g.a_end_secs + [long]$p.gap_end_adjust_frames / $sr
    if ($snapped) { $s -= (Get-SnapMs $leadMs $key) / 1000; $e += (Get-SnapMs $trailMs $key) / 1000 }
    , @($s, $e)
}

# Audio checks on the two cuts; returns $null on pass, else the reason.
function Test-Audio($c, $cur, $snp, $curRaw, $snpRaw, [long]$f0, $xc, $xs, $curG, $snpG) {
    $sr = $cur.sr
    $span = [DefaultOnDsp]::DiffSpan([DefaultOnDsp]::ReadS16($curRaw), [DefaultOnDsp]::ReadS16($snpRaw), $cur.ch)
    if ($span[0] -eq -2) { return 'clip lengths differ' }
    $toMs = { param([long]$frame, [double]$secs) ($frame + $f0) * 1000.0 / $sr - $secs * 1000 }
    if ($c.Stratum -eq 'control') {
        if ($span[0] -ge 0) { return ('check 2: control arms differ ({0:0.0} to {1:0.0} ms from the write start)' -f (& $toMs $span[0] $xc[0]), (& $toMs $span[1] $xc[0])) }
        return $null
    }
    if ($span[0] -lt 0) { return 'arms are identical' }
    if ($c.Stratum -in $Replaced) {
        $lo = [math]::Min($xc[0], $xs[0]); $hi = [math]::Max($xc[1], $xs[1])
        $firstMs = & $toMs $span[0] $lo; $lastMs = & $toMs $span[1] $hi
        if ($firstMs -lt -$ConfineMs -or $lastMs -gt $ConfineMs) {
            return ('check 3: arms differ from {0:+0.0;-0.0} ms of the earliest write start to {1:+0.0;-0.0} ms of the latest write end (allowed ±{2})' -f $firstMs, $lastMs, $ConfineMs)
        }
        Write-Host ("  ok  {0,-6} {1,-6} confined: {2:+0.0;-0.0} / {3:+0.0;-0.0} ms" -f $c.Key, $c.Stratum, $firstMs, $lastMs)
        return $null
    }
    # Trail: the current arm's post seam is its write end; the snap moves it out by the trail onset.
    $trail = Get-SnapMs $trailMs $c.Key
    $firstMs = & $toMs $span[0] $xc[1]; $lastMs = & $toMs $span[1] $xc[1]
    if ($firstMs -lt -($CrossfadeMs + $WindowSlackMs) -or $firstMs -gt $WindowLateMs) {
        return ('check 2: first difference at post seam {0:+0.00;-0.00} ms, outside [-{1}, +{2}]' -f $firstMs, ($CrossfadeMs + $WindowSlackMs), $WindowLateMs)
    }
    if ($lastMs -ge $trail + $CrossfadeMs + $WindowSlackMs) {
        return ('check 2: arms still differ {0:+0.00} ms after the post seam (allowed until {1:+0.00})' -f $lastMs, ($trail + $CrossfadeMs + $WindowSlackMs))
    }
    Write-Host ("  ok  {0,-6} trail   first difference at post seam {1:+0.00;-0.00} ms, last {2:+0.00} ms (trail onset {3:0.00})" -f $c.Key, $firstMs, $lastMs, $trail)
    $null
}

# ---- selection walk ------------------------------------------------------------------------------
$accepted = [System.Collections.Generic.List[object]]::new()
$skipped = [System.Collections.Generic.List[object]]::new()
$pending = $round1
$round = 1
while ($pending.Count) {
    if ($round -gt 1) { Write-Host "round $round runs:"; Show-Round $pending }
    foreach ($grp in ($pending | Group-Object Pair | Sort-Object { [int]$_.Name })) {
        $gaps = [int[]]($grp.Group | ForEach-Object Gap)
        $cur = Invoke-Arm $round $grp.Name 'current' $gaps
        $snp = Invoke-Arm $round $grp.Name 'snapped' $gaps
        foreach ($c in $grp.Group) {
            $curG = $cur.doc.patch.gaps[$c.Gap]; $snpG = $snp.doc.patch.gaps[$c.Gap]
            $why = Test-Report $c $curG $snpG
            $entry = $null
            if (-not $why) {
                $xc = Get-Extent $curG $cur.sr $false $c.Key; $xs = Get-Extent $snpG $snp.sr $true $c.Key
                $f0 = [long][math]::Round(([math]::Min($xc[0], $xs[0]) - $ContextSecs) * $cur.sr)
                $f1 = [long][math]::Round(([math]::Max($xc[1], $xs[1]) + $ContextSecs) * $cur.sr)
                $curRaw = Get-Cut $cur $c.Gap $f0 $f1; $snpRaw = Get-Cut $snp $c.Gap $f0 $f1
                $why = Test-Audio $c $cur $snp $curRaw $snpRaw $f0 $xc $xs $curG $snpG
                $entry = [pscustomobject]@{ c = $c; curG = $curG; snpG = $snpG; sr = $cur.sr; ch = $cur.ch
                    curRaw = $curRaw; snpRaw = $snpRaw; f0 = $f0; xc = $xc; xs = $xs }
            }
            if ($c.Stratum -eq 'control') {
                if ($why) { throw "control $($c.Key) failed: $why. The snap is not a no-op on a hard edge; no set written." }
                Write-Host "  ok  $($c.Key) control: arms byte-identical"
            }
            elseif ($why) {
                Write-Warning "$($c.Key) ($($c.Stratum)) skipped: $why"
                $skipped.Add([pscustomobject]@{ gap = $c.Key; stratum = $c.Stratum; round = $round; reason = $why })
                continue
            }
            $accepted.Add($entry)
        }
        foreach ($arm in $cur, $snp) { if ((Test-Path -LiteralPath $arm.mkv) -and -not $KeepMux) { Remove-Item -LiteralPath $arm.mkv -Force } }
    }
    $pending = @()
    foreach ($s in $walk.Keys) {
        if ($s -eq 'named') { continue }
        $need = $Strata[$s].Quota - @($accepted | Where-Object { $_.c.Stratum -eq $s }).Count
        while ($need -gt 0 -and $next[$s] -lt $walk[$s].Count) { $pending += $walk[$s][$next[$s]]; $next[$s]++; $need-- }
    }
    $round++
}
foreach ($s in $walk.Keys) {
    $n = @($accepted | Where-Object { $_.c.Stratum -eq $s }).Count
    if ($n -lt $Strata[$s].Quota) { Write-Warning "stratum $s filled $n of $($Strata[$s].Quota) by rank 8; § 5: listen to those and say so" }
}
$ab = @($accepted | Where-Object { $_.c.Stratum -ne 'control' })
$replacedOk = @($ab | Where-Object { $_.c.Stratum -in $Replaced })
if ($replacedOk.Count -lt $Repeats) { throw 'too few re-placed gaps to draw the repeats' }

# ---- blind set -----------------------------------------------------------------------------------
$rng = [System.Random]::new($Seed)
function Shuffle([object[]]$a) { for ($i = $a.Count - 1; $i -gt 0; $i--) { $j = $rng.Next($i + 1); $t = $a[$i]; $a[$i] = $a[$j]; $a[$j] = $t }; , $a }

# Shuffle returns its array as one pipeline object, so index it rather than piping to Select-Object.
$repeatIdx = @((Shuffle (0..($replacedOk.Count - 1)))[0..($Repeats - 1)])
$trials = [System.Collections.Generic.List[object]]::new()
foreach ($e in $ab) { $trials.Add([pscustomobject]@{ e = $e; repeat = $false }) }
foreach ($i in $repeatIdx) { $trials.Add([pscustomobject]@{ e = $replacedOk[$i]; repeat = $true }) }
$nonControl = $trials.Count
$onA = if ($nonControl -eq $targetTotal + $Repeats) { $SnappedOnA } else { [math]::Floor($nonControl / 2) }
$sides = Shuffle ([bool[]]((1..$nonControl) | ForEach-Object { $_ -le $onA }))
for ($i = 0; $i -lt $nonControl; $i++) { $trials[$i] | Add-Member snappedOnA $sides[$i] }
foreach ($e in ($accepted | Where-Object { $_.c.Stratum -eq 'control' })) { $trials.Add([pscustomobject]@{ e = $e; repeat = $false; snappedOnA = ($rng.Next(2) -eq 0) }) }
if ($trials.Count -ne $ab.Count + $Repeats + $Controls.Count -or @($trials | Where-Object { $_.e -is [array] }).Count) {
    throw 'trial list is malformed'
}

for ($attempt = 1; ; $attempt++) {
    $order = Shuffle $trials.ToArray()
    $adjacent = $false
    for ($i = 1; $i -lt $order.Count; $i++) { if ($order[$i].e.c.Key -eq $order[$i - 1].e.c.Key) { $adjacent = $true; break } }
    if (-not $adjacent) { break }
    if ($attempt -ge 1000) { throw 'could not order trials without same-gap neighbours' }
}

Get-ChildItem -LiteralPath $BlindDir -Filter 't*_?.wav' -File | Remove-Item -Force
$key = [System.Collections.Generic.List[object]]::new()
for ($i = 0; $i -lt $order.Count; $i++) {
    $t = $order[$i]; $e = $t.e
    $id = 't{0:00}' -f ($i + 1)
    $a = if ($t.snappedOnA) { $e.snpRaw } else { $e.curRaw }
    $b = if ($t.snappedOnA) { $e.curRaw } else { $e.snpRaw }
    foreach ($side in @(@{ s = 'a'; x = $a }, @{ s = 'b'; x = $b })) {
        ffmpeg -v error -y -f s16le -ar "$($e.sr)" -ac "$($e.ch)" -i "$($side.x)" -c:a pcm_s16le (Join-Path $BlindDir "$($id)_$($side.s).wav")
        if ($LASTEXITCODE) { throw "wav write failed: $id $($side.s)" }
    }
    $p = $e.curG.status.patched; $s = $e.snpG.status.patched
    $clip0 = $e.f0 / $e.sr
    $arm = { param($g, $pp, $x) [ordered]@{
            write_in_clip_secs = @([math]::Round($x[0] - $clip0, 3), [math]::Round($x[1] - $clip0, 3))
            anchor_seam_used = [bool]$pp.anchor_seam_used; dual_fit_used = [bool]$pp.dual_fit_used; patch_tier = $g.tags.patch_tier
            align_adjustment_ms = [math]::Round($pp.align_adjustment_secs * 1000, 2); registration_residual_ms = $pp.registration_residual_ms
        } }
    $key.Add([ordered]@{
        trial = $id; pair = $e.c.Pair; gap_index = $e.c.Gap; stratum = $e.c.Stratum; repeat = $t.repeat
        a = if ($t.snappedOnA) { 'snapped' } else { 'current' }; b = if ($t.snappedOnA) { 'current' } else { 'snapped' }
        lead_snap_ms = Get-SnapMs $leadMs $e.c.Key; trail_snap_ms = Get-SnapMs $trailMs $e.c.Key
        current = & $arm $e.curG $p $e.xc; snapped = & $arm $e.snpG $s $e.xs
    })
}
[ordered]@{
    seed = $Seed; snap_args = ($SnapArgs -join ' '); context_secs = $ContextSecs
    trials = $key; skipped_candidates = $skipped
} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $KeyDir 'key.json') -Encoding utf8

$key | ForEach-Object { [pscustomobject]@{ trial = $_.trial; verdict = ''; location = ''; note = '' } } |
    Export-Csv -LiteralPath (Join-Path $BlindDir 'labels.csv') -NoTypeInformation -Encoding utf8
Set-Content -LiteralPath (Join-Path $BlindDir 'README.txt') -Encoding utf8 -Value @"
Blind A/B ear check: edge snap default-on (TEMP-soft-edge-census-plan.md § 5, Step 3).

$($order.Count) trials, tNN_a.wav and tNN_b.wav. Each clip runs from $ContextSecs s before a repair to
$ContextSecs s after it, so the repaired stretch starts about $ContextSecs s in. Some repairs are several
seconds long. The two files of a trial differ, if at all, only in and around the repaired stretch.

For each trial, play a and b as often as you like, then write one verdict in labels.csv:
  a      a is better
  b      b is better
  same   no difference you can hear
Optional: location (before the repair / at a seam / inside the fill / after) and a free note.

One sitting, trials in order, no going back. Do not open the key folder until every verdict is in.
"@

Write-Host ''
Write-Host ("wrote {0} trials to the blind folder; {1} A/B gaps + {2} repeats + {3} controls" -f `
    $order.Count, $ab.Count, $repeatIdx.Count, $Controls.Count)
foreach ($s in $walk.Keys) { Write-Host ("  {0,-7} {1}" -f $s, (($ab | Where-Object { $_.c.Stratum -eq $s } | ForEach-Object { $_.c.Key }) -join ', ')) }
Write-Host ("balance: snapped on a in {0} of {1} non-control trials" -f @($trials | Select-Object -First $nonControl | Where-Object snappedOnA).Count, $nonControl)
if ($skipped.Count) { Write-Host ("skipped candidates: {0}" -f (($skipped | ForEach-Object { "$($_.gap) ($($_.stratum))" }) -join ', ')) }
Write-Host 'Leave the key folder closed until scoring is done.'
