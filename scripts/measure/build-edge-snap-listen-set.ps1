#!/usr/bin/env pwsh
#Requires -Version 7.0
# Blind A/B listen set for the soft-edge ear check (docs/dev/TEMP-soft-edge-census-plan.md § 5,
# "Phase 3 — ear check protocol").
#
# Renders each selected gap twice from REAL repair output: *current* (defaults) and *snapped*
# (`--edge-snap-max-ms 40`). It checks that the snap reached the write and nothing else moved, and
# writes a shuffled set of `tNN_a.wav` / `tNN_b.wav` trials, plus a key the listener must not open
# until every verdict is in. The selection, checks, seed and decision rule live in § 5. This script
# is their executable half and must agree with them. Change the candidate list here only together
# with § 5, and only before listening starts.
#
# WHY NOT `--gap-listen`: its placement disagreed with production by 10 ms on 47/g14
# (border-standoff § 7), which is the scale of the band under test. Each run is a normal repair with
# `--mux` to `.mkv` carrying FLAC audio, set through a generated config, as in
# build-standoff-listen-set.ps1.
#
# SELECTION WALK: candidates are taken in the fixed § 5 order until 12 pass the checks. Round 1 runs
# the first 12 candidates and the 2 controls. Each later round runs only the replacements, one
# repair per (pair, arm) with `--only-gaps` naming that round's gaps. A failed candidate is recorded
# with its reason. A failed control stops the script, because the snap is then not a no-op on a hard
# edge. Before any run, the candidate order is re-derived from the census and must match.
#
# CHECKS (per gap; see § 5):
#   1. reference match: the current arm reproduces the 2026-10-26 reference report (patched, same
#      dual_fit_used and adjust frames, align_adjustment_secs and registration_residual_ms to 0.1 ms);
#   2. snap reached the write: the arms first differ within [onset - cf - 2 ms, onset + 1 ms], and are
#      identical from the post seam on: from a_end + end adjust when the census has no trail onset,
#      else from the trail onset + cf + 2 ms (a trail snap moves the post seam). Controls are
#      byte-identical;
#   3. placement only: the snapped arm keeps dual_fit_used and moves align_adjustment by <= 1 ms.
#
# OUTPUT:
#   -BlindDir (gap-files/2026-10-28-blind-edge-snap/): tNN_a.wav, tNN_b.wav, README.txt, labels.csv
#   -KeyDir   (gap-files/2026-10-28-blind-edge-snap-key/): key.json. DO NOT OPEN before scoring. This
#             script never prints it.
#   -WorkDir: reports, logs, cut PCM, flac-mux.toml. The labelled renders live here, so point it at
#             the session scratchpad, never at gap-files/.
#
# RESUME: a run whose report and cut PCM exist is not repeated. The muxed .mkv is deleted after
# cutting unless -KeepMux. -Force re-runs everything.
#
# Usage:
#   ./scripts/measure/build-edge-snap-listen-set.ps1 -PlanOnly
#   ./scripts/measure/build-edge-snap-listen-set.ps1 -WorkDir <scratchpad>/edge-snap-work
#
# MEDIA HYGIENE: the manifest, reports and logs carry absolute media paths. This script prints pair
# labels, gap indices and numeric fields only. Of the census, it reads only pair, gap_index, edge,
# disposition, fill_path, onset_near_ms and exposure_est_ms.
[CmdletBinding()]
param(
    [string]$Manifest = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'gap-files/corpus-pairs.csv'),
    [string]$RefDir = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'gap-files/2026-10-26-edge-onset'),
    [string]$BlindDir = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'gap-files/2026-10-28-blind-edge-snap'),
    [string]$KeyDir = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'gap-files/2026-10-28-blind-edge-snap-key'),
    [string]$WorkDir = (Join-Path ([System.IO.Path]::GetTempPath()) 'clip-sync-edge-snap-listen'),

    # Fixed in § 5. Changing it builds a different set.
    [int]$Seed = 20261028,

    [switch]$PlanOnly,
    [switch]$KeepMux,
    [switch]$Force,
    [switch]$SkipBuild,
    [string]$CargoProfile = 'release',
    [string]$Features = 'he-aac,ac3,ffmpeg-mux'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

# ---- design (must match TEMP-soft-edge-census-plan.md § 5, Phase 3) ------------------------------
$Candidates = @(
    '64/14', '36/5', '38/2', '76/9', '66/7', '64/2', '39/5', '58/7', '3/6', '30/43', '3/10', '74/26',
    '64/15', '3/4', '79/1', '34/6', '50/7', '11/6', '83/5', '9/4', '3/12', '83/3', '64/47', '68/3'
)
$Controls = @('64/3', '36/4')
$Target = 12
$Repeats = 2
$SnappedOnA = 7                 # of the 14 non-control trials
$SnapArgs = @('--edge-snap-max-ms', '40')
$ContextSecs = 4.0
$CrossfadeMs = 10.0             # default crossfade_ms; check 2's window is [onset - cf - 2, onset + 1]
$WindowSlackMs = 2.0
$WindowLateMs = 1.0
$ReportTolMs = 0.1
$PlacementTolMs = 1.0
# -------------------------------------------------------------------------------------------------

function Split-Key([string]$k) { $p = $k.Split('/'); [pscustomobject]@{ Pair = $p[0]; Gap = [int]$p[1]; Key = $k } }
$cand = $Candidates | ForEach-Object { Split-Key $_ }
$ctrl = $Controls | ForEach-Object { Split-Key $_ }

# ---- census: re-derive the ranking, read onsets --------------------------------------------------
$censusPath = Join-Path $RefDir 'edge-onset/census.csv'
if (-not (Test-Path -LiteralPath $censusPath)) { throw "census not found: $censusPath" }
$census = Import-Csv -LiteralPath $censusPath |
    Select-Object pair, gap_index, edge, disposition, fill_path, onset_near_ms, exposure_est_ms
$ranked = @($census |
    Where-Object { $_.disposition -eq 'patched' -and $_.fill_path -ne 'anchor_seam' -and $_.exposure_est_ms -ne '' } |
    Sort-Object @{ e = { [double]$_.exposure_est_ms }; Descending = $true }, @{ e = { [int]$_.pair } }, @{ e = { [int]$_.gap_index } } |
    Select-Object -First $Candidates.Count)
for ($i = 0; $i -lt $Candidates.Count; $i++) {
    $r = $ranked[$i]
    if ("$($r.pair)/$($r.gap_index)" -ne $Candidates[$i]) { throw "census rank $($i + 1) is $($r.pair)/g$($r.gap_index), § 5 lists $($Candidates[$i])" }
    if ($r.edge -ne 'lead') { throw "rank $($i + 1) ($($Candidates[$i])) is a $($r.edge) edge; this script handles lead edges only" }
}
$onsetMs = @{}
$trailOnsetMs = @{}             # 0 when the census has no trail onset
foreach ($c in $cand) {
    $row = $census | Where-Object { $_.pair -eq $c.Pair -and [int]$_.gap_index -eq $c.Gap -and $_.edge -eq 'lead' }
    $onsetMs[$c.Key] = [double]$row.onset_near_ms
    $trail = $census | Where-Object { $_.pair -eq $c.Pair -and [int]$_.gap_index -eq $c.Gap -and $_.edge -eq 'trail' }
    $trailOnsetMs[$c.Key] = if ($trail.onset_near_ms -ne '') { [double]$trail.onset_near_ms } else { 0.0 }
}
$fillPath = @{}
foreach ($c in $ctrl) {
    $rows = @($census | Where-Object { $_.pair -eq $c.Pair -and [int]$_.gap_index -eq $c.Gap })
    if ($rows.Count -ne 2 -or ($rows | Where-Object { $_.disposition -ne 'patched' -or $_.fill_path -eq 'anchor_seam' -or $_.onset_near_ms -ne '' })) {
        throw "control $($c.Key) is not a patched, non-anchor gap with no onset on either edge"
    }
    $fillPath[$c.Key] = $rows[0].fill_path
}
foreach ($r in $ranked) { $fillPath["$($r.pair)/$($r.gap_index)"] = $r.fill_path }

Write-Host ("design: first {0} of {1} ranked candidates + {2} controls + {3} repeats = {4} trials; seed {5}" -f `
    $Target, $Candidates.Count, $Controls.Count, $Repeats, ($Target + $Controls.Count + $Repeats), $Seed)
for ($i = 0; $i -lt $Candidates.Count; $i++) {
    $c = $cand[$i]; $r = $ranked[$i]
    Write-Host ("  rank {0,2}  {1,-6} {2,-10} exposure_est {3,6:0.00} ms  onset {4,6:0.00} ms{5}" -f `
        ($i + 1), $c.Key, $r.fill_path, [double]$r.exposure_est_ms, $onsetMs[$c.Key], $(if ($i -lt $Target) { '  (round 1)' } else { '' }))
}
foreach ($c in $ctrl) { Write-Host ("  control  {0,-6} {1}" -f $c.Key, $fillPath[$c.Key]) }
function Show-Round($gaps) {
    foreach ($grp in ($gaps | Group-Object Pair | Sort-Object { [int]$_.Name })) {
        $only = ($grp.Group | ForEach-Object { $_.Gap + 1 } | Sort-Object) -join ','
        Write-Host ("  pair {0,-3} --only-gaps {1,-12} x 2 arms (current, snapped {2})" -f $grp.Name, $only, ($SnapArgs -join ' '))
    }
}
Write-Host 'round 1 runs:'
$round1 = @($cand | Select-Object -First $Target) + @($ctrl)
Show-Round $round1
if ($PlanOnly) { return }

# ---- numeric helpers -------------------------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
public static class EdgeSnapDsp {
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
foreach ($c in @($cand) + @($ctrl)) { if (-not $pairs.ContainsKey($c.Pair)) { throw "pair $($c.Pair) not in manifest" } }

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
# Generated by build-edge-snap-listen-set.ps1: lossless mux so clips carry the patched PCM exactly.
[repair.output]
video_codec = "copy"
audio_codec = "flac"
mux_audio_bitrate = "default"
'@

function Read-Json([string]$p) { Get-Content -LiteralPath $p -Raw | ConvertFrom-Json -Depth 64 }
$refs = @{}
function Get-Ref([string]$pair) {
    if (-not $refs.ContainsKey($pair)) { $refs[$pair] = Read-Json (Join-Path $RefDir "$pair.json") }
    $refs[$pair]
}

# One repair per (round, pair, arm); returns per-gap @{ g = patch gap; raw; sr; ch; f0 }.
function Invoke-Arm([int]$round, [string]$pair, [string]$arm, [int[]]$gaps) {
    $id = "r{0}-{1}-{2}" -f $round, $pair, $arm
    $report = Join-Path $runDir "$id.json"; $log = Join-Path $runDir "$id.log"; $mkv = Join-Path $runDir "$id.mkv"
    $raws = $gaps | ForEach-Object { Join-Path $runDir ("{0}-g{1}.raw" -f $id, $_) }
    $done = (Test-Path -LiteralPath $report) -and ((Get-Item -LiteralPath $report).Length -gt 0) -and
        -not ($raws | Where-Object { -not (Test-Path -LiteralPath $_) })
    if ($Force -or -not $done) {
        $p = $pairs[$pair]
        $only = ($gaps | ForEach-Object { $_ + 1 } | Sort-Object) -join ','
        $argv = @($p.A, $p.B, '-c', $cfg, '--mux', $mkv, '--format', 'json', '--only-gaps', $only)
        if ($arm -eq 'snapped') { $argv += $SnapArgs }
        Write-Host ("[{0}] repair {1} (--only-gaps {2}) ..." -f (Get-Date -Format 'HH:mm'), $id, $only)
        & $exe @argv > $report 2> $log
        if ($LASTEXITCODE -ne 0) { throw "repair $id exited $LASTEXITCODE (see its log in the work dir)" }
    }
    $doc = Read-Json $report
    $sr = [int]$doc.scan.track_compatibility.a_sample_rate
    $ch = [int]$doc.scan.track_compatibility.a_channels
    $out = @{}
    foreach ($gi in $gaps) {
        $g = $doc.patch.gaps[$gi]
        $f0 = [long][math]::Round(($g.a_start_secs - $ContextSecs) * $sr)
        $f1 = [long][math]::Round(($g.a_end_secs + $ContextSecs) * $sr)
        $raw = Join-Path $runDir ("{0}-g{1}.raw" -f $id, $gi)
        if ($Force -or -not (Test-Path -LiteralPath $raw)) {
            if (-not (Test-Path -LiteralPath $mkv)) { throw "cut needed but $id.mkv is gone; re-run with -Force" }
            # atrim counts samples from the stream start, independent of container timestamps.
            ffmpeg -v error -y -i $mkv -map 0:a:0 -af "atrim=start_sample=$($f0):end_sample=$($f1)" -f s16le -c:a pcm_s16le $raw
            if ($LASTEXITCODE) { throw "cut failed: $id g$gi" }
        }
        $out[$gi] = @{ g = $g; raw = $raw; sr = $sr; ch = $ch; f0 = $f0 }
    }
    if ((Test-Path -LiteralPath $mkv) -and -not $KeepMux) { Remove-Item -LiteralPath $mkv -Force }
    $out
}

# Checks 1-3 for one gap; returns $null on pass, else the reason.
function Test-Gap($c, $cur, $snp, [bool]$isControl) {
    $p = $cur.g.status.patched; $s = $snp.g.status.patched
    $rg = (Get-Ref $c.Pair).patch.gaps[$c.Gap]; $rp = $rg.status.patched
    if ($null -eq $p -or $null -eq $rp) { return 'check 1: not patched in the current arm or the reference' }
    $bad = @()
    if ([math]::Abs($cur.g.a_start_secs - $rg.a_start_secs) -gt 1e-6) { $bad += 'a_start' }
    foreach ($k in 'dual_fit_used', 'gap_start_adjust_frames', 'gap_end_adjust_frames') { if ($p.$k -ne $rp.$k) { $bad += $k } }
    if ([math]::Abs(($p.align_adjustment_secs - $rp.align_adjustment_secs) * 1000) -gt $ReportTolMs) { $bad += 'align_adjustment' }
    if ([math]::Abs($p.registration_residual_ms - $rp.registration_residual_ms) -gt $ReportTolMs) { $bad += 'residual' }
    if ($bad) { return "check 1: current arm differs from the reference on $($bad -join ', ')" }

    if ($null -eq $s) { return 'check 3: snapped arm is not patched' }
    if ($s.dual_fit_used -ne $p.dual_fit_used) { return 'check 3: snapped arm changes dual_fit_used' }
    $move = ($s.align_adjustment_secs - $p.align_adjustment_secs) * 1000
    if ([math]::Abs($move) -gt $PlacementTolMs) { return ('check 3: snapped arm moves align_adjustment by {0:0.00} ms' -f $move) }

    $span = [EdgeSnapDsp]::DiffSpan([EdgeSnapDsp]::ReadS16($cur.raw), [EdgeSnapDsp]::ReadS16($snp.raw), $cur.ch)
    if ($span[0] -eq -2) { return 'check 2: clip lengths differ' }
    if ($isControl) {
        if ($span[0] -ge 0) { return ('check 2: control arms differ (frames {0}-{1} of the clip)' -f $span[0], $span[1]) }
        return $null
    }
    if ($span[0] -lt 0) { return 'check 2: arms are identical (the snap did not reach the write)' }
    $sr = $cur.sr
    $onset = [long][math]::Truncate($cur.g.a_start_secs * $sr) - [long][math]::Round($onsetMs[$c.Key] * $sr / 1000) - $cur.f0
    $firstMs = ($span[0] - $onset) * 1000.0 / $sr
    if ($firstMs -lt (-($CrossfadeMs + $WindowSlackMs)) -or $firstMs -gt $WindowLateMs) {
        return ('check 2: first difference at onset {0:+0.00;-0.00} ms, outside [-{1}, +{2}]' -f $firstMs, ($CrossfadeMs + $WindowSlackMs), $WindowLateMs)
    }
    $endAdj = [math]::Max([long]$p.gap_end_adjust_frames, [long]$s.gap_end_adjust_frames)
    # The post seam must not move unless the trail itself snapped: a moved post seam would put a
    # second, unrelated difference into the trial (38/g2's post crossfade differed by -3.7 dB).
    $tailMs = if ($trailOnsetMs[$c.Key] -gt 0) { $trailOnsetMs[$c.Key] + $CrossfadeMs + $WindowSlackMs } else { 0.0 }
    $clear = [long][math]::Truncate($cur.g.a_end_secs * $sr) + $endAdj + [long][math]::Round($tailMs * $sr / 1000) - $cur.f0
    if ($span[1] -ge $clear) { return ('check 2: arms still differ {0:+0.00;-0.00} ms after a_end (allowed until {1:+0.00;-0.00})' -f (($span[1] + $cur.f0) * 1000.0 / $sr - $cur.g.a_end_secs * 1000), ($tailMs + $endAdj * 1000.0 / $sr)) }
    Write-Host ("  ok  {0,-6} first difference at onset {1:+0.00;-0.00} ms, align move {2:+0.00;-0.00} ms" -f $c.Key, $firstMs, $move)
    $null
}

# ---- selection walk ------------------------------------------------------------------------------
$accepted = [System.Collections.Generic.List[object]]::new()
$skipped = [System.Collections.Generic.List[object]]::new()
$next = 0
$pending = @($cand | Select-Object -First $Target); $next = $pending.Count
$round = 1
while ($true) {
    $todo = @($pending) + $(if ($round -eq 1) { @($ctrl) } else { @() })
    if ($round -gt 1) { Write-Host "round $round runs:"; Show-Round $todo }
    foreach ($grp in ($todo | Group-Object Pair | Sort-Object { [int]$_.Name })) {
        $gaps = [int[]]($grp.Group | ForEach-Object Gap)
        $cur = Invoke-Arm $round $grp.Name 'current' $gaps
        $snp = Invoke-Arm $round $grp.Name 'snapped' $gaps
        foreach ($c in $grp.Group) {
            $isControl = $Controls -contains $c.Key
            $why = Test-Gap $c $cur[$c.Gap] $snp[$c.Gap] $isControl
            if ($isControl) {
                if ($why) { throw "control $($c.Key) failed: $why. The snap is not a no-op on a hard edge; no set written." }
                Write-Host "  ok  $($c.Key) control: arms byte-identical"
                $accepted.Add([pscustomobject]@{ c = $c; control = $true; cur = $cur[$c.Gap]; snp = $snp[$c.Gap] })
            }
            elseif ($why) {
                Write-Warning "$($c.Key) skipped: $why"
                $skipped.Add([pscustomobject]@{ gap = $c.Key; round = $round; reason = $why })
            }
            else { $accepted.Add([pscustomobject]@{ c = $c; control = $false; cur = $cur[$c.Gap]; snp = $snp[$c.Gap] }) }
        }
    }
    $need = $Target - @($accepted | Where-Object { -not $_.control }).Count
    if ($need -le 0 -or $next -ge $cand.Count) { break }
    $pending = @($cand | Select-Object -Skip $next -First $need); $next += $pending.Count
    $round++
}
$exposed = @($accepted | Where-Object { -not $_.control })
if ($exposed.Count -lt $Target) { Write-Warning "only $($exposed.Count) of $Target exposed gaps passed; § 5: listen to those and say so" }
if ($exposed.Count -lt $Repeats) { throw 'too few exposed gaps to draw the repeats' }

# ---- blind set -----------------------------------------------------------------------------------
$rng = [System.Random]::new($Seed)
function Shuffle([object[]]$a) { for ($i = $a.Count - 1; $i -gt 0; $i--) { $j = $rng.Next($i + 1); $t = $a[$i]; $a[$i] = $a[$j]; $a[$j] = $t }; , $a }

# Shuffle returns its array as one pipeline object, so index it rather than piping to Select-Object.
$repeatIdx = @((Shuffle (0..($exposed.Count - 1)))[0..($Repeats - 1)])
$trials = [System.Collections.Generic.List[object]]::new()
foreach ($e in $exposed) { $trials.Add([pscustomobject]@{ e = $e; repeat = $false }) }
foreach ($i in $repeatIdx) { $trials.Add([pscustomobject]@{ e = $exposed[$i]; repeat = $true }) }
$nonControl = $trials.Count
$onA = $SnappedOnA
if ($nonControl -ne $Target + $Repeats) { $onA = [math]::Floor($nonControl / 2) }
$sides = Shuffle ([bool[]]((1..$nonControl) | ForEach-Object { $_ -le $onA }))
for ($i = 0; $i -lt $nonControl; $i++) { $trials[$i] | Add-Member snappedOnA $sides[$i] }
foreach ($e in ($accepted | Where-Object control)) { $trials.Add([pscustomobject]@{ e = $e; repeat = $false; snappedOnA = ($rng.Next(2) -eq 0) }) }
if ($trials.Count -ne $exposed.Count + $Repeats + @($accepted | Where-Object control).Count -or @($trials | Where-Object { $_.e -is [array] }).Count) {
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
    $a = if ($t.snappedOnA) { $e.snp } else { $e.cur }
    $b = if ($t.snappedOnA) { $e.cur } else { $e.snp }
    foreach ($side in @(@{ s = 'a'; x = $a }, @{ s = 'b'; x = $b })) {
        $x = $side.x
        ffmpeg -v error -y -f s16le -ar "$($x.sr)" -ac "$($x.ch)" -i "$($x.raw)" -c:a pcm_s16le (Join-Path $BlindDir "$($id)_$($side.s).wav")
        if ($LASTEXITCODE) { throw "wav write failed: $id $($side.s)" }
    }
    $p = $e.cur.g.status.patched; $s = $e.snp.g.status.patched
    $key.Add([ordered]@{
        trial = $id; pair = $e.c.Pair; gap_index = $e.c.Gap; control = $e.control; repeat = $t.repeat
        a = if ($t.snappedOnA) { 'snapped' } else { 'current' }; b = if ($t.snappedOnA) { 'current' } else { 'snapped' }
        fill_path = $fillPath[$e.c.Key]; onset_near_ms = $onsetMs[$e.c.Key]
        gap_start_in_clip_secs = $ContextSecs
        align_adjustment_ms = @{ current = [math]::Round($p.align_adjustment_secs * 1000, 2); snapped = [math]::Round($s.align_adjustment_secs * 1000, 2) }
        registration_residual_ms = @{ current = $p.registration_residual_ms; snapped = $s.registration_residual_ms }
    })
}
[ordered]@{
    seed = $Seed; snap_args = ($SnapArgs -join ' '); context_secs = $ContextSecs
    trials = $key; skipped_candidates = $skipped
} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $KeyDir 'key.json') -Encoding utf8

$key | ForEach-Object { [pscustomobject]@{ trial = $_.trial; verdict = ''; location = ''; note = '' } } |
    Export-Csv -LiteralPath (Join-Path $BlindDir 'labels.csv') -NoTypeInformation -Encoding utf8
Set-Content -LiteralPath (Join-Path $BlindDir 'README.txt') -Encoding utf8 -Value @"
Blind A/B ear check: soft gap edges (TEMP-soft-edge-census-plan.md § 5, Phase 3).

$($order.Count) trials, tNN_a.wav and tNN_b.wav. Each clip runs from $ContextSecs s before a repaired gap
to $ContextSecs s after it, so the gap starts $ContextSecs s in. The two files of a trial differ, if at all, only
at the edges of the gap.

For each trial, play a and b as often as you like, then write one verdict in labels.csv:
  a      a is better
  b      b is better
  same   no difference you can hear
Optional: location (before the gap / at the seam / inside the fill) and a free note.

One sitting, trials in order, no going back. Do not open the key folder until every verdict is in.
"@

Write-Host ''
Write-Host ("wrote {0} trials to the blind folder; {1} exposed + {2} repeats + {3} controls" -f `
    $order.Count, $exposed.Count, $repeatIdx.Count, @($accepted | Where-Object control).Count)
Write-Host ("balance: snapped on a in {0} of {1} non-control trials" -f @($trials | Select-Object -First $nonControl | Where-Object snappedOnA).Count, $nonControl)
if ($skipped.Count) { Write-Host ("skipped candidates: {0}" -f (($skipped | ForEach-Object gap) -join ', ')) }
Write-Host 'Leave the key folder closed until scoring is done.'
