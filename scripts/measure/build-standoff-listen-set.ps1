#!/usr/bin/env pwsh
#Requires -Version 7.0
# Blind listen set for the `border_standoff_secs` α test (docs/dev/archive/TEMP-border-standoff-plan.md § 7).
#
# Renders each selected gap at two placements (standoff 0.35 vs 0) from REAL repair output, checks
# that the audio carries the placement the sweep measured, and writes a shuffled, neutrally named clip
# set plus a key the listener must not open until every label is in. The design, gap list, stop rule
# and seed live in § 7; this script is its executable half and must agree with it. Change the gap
# list here only together with § 7, and only before listening starts.
#
# WHY NOT `--gap-listen`. Its clips disagreed with production placement by 10 ms on 47/g14, which is
# too much error for a 40-140 ms placement test. Each run here is a normal repair with `--mux` to
# `.mkv` carrying FLAC (lossless) audio, set through a generated config. Classic WAV cannot be used:
# pairs 3 and 53 exceed the 4 GiB limit at 6 ch / 48 kHz / 16 bit.
#
# CHECKS (the set is not built unless all pass):
#   1. report match: each run's gap has the routing, adjust frames, align_adjustment_secs and
#      registration_residual_ms of its reference report (to 0.1 ms);
#   2. same cut: a gap's two arm clips are sample-identical outside [A start - 50 ms, A end + 50 ms];
#   3. placement readback: each arm's fill interior is located in B (one ffmpeg decode of B per gap,
#      so seek error cancels); the two B positions differ by the reports' align_adjustment difference
#      to within 1 ms. Arm-vs-arm correlation was tried first and failed on 53/g8 for a check-design
#      reason (see TEMP-border-standoff-plan.md § 7), so the fills are read against their source.
#
# OUTPUT (-OutDir, default gap-files/2026-09-19-standoff-listen/):
#   clips/clip-NN.wav, clips/labels.csv   <- the listener's folder
#   key.csv                               <- DO NOT OPEN before labelling; never printed by this script
#   runs/<run>.json, runs/<run>.log, runs/<run>-<gap>.raw   (reports, logs, cut PCM)
#
# RESUME: a run whose report and cut PCM already exist is not repeated. The muxed .mkv is deleted
# after cutting unless -KeepMux (one is several GB). -Force re-runs everything.
#
# Usage:
#   ./scripts/measure/build-standoff-listen-set.ps1 -Manifest gap-files/extended-pairs.csv
#   ./scripts/measure/build-standoff-listen-set.ps1 -Manifest gap-files/extended-pairs.csv -PlanOnly
#
# MEDIA HYGIENE: the manifest, reports and logs carry absolute media paths. Output stays under the
# gitignored gap-files/. This script prints pair labels and gap indices only.
[CmdletBinding()]
param(
    # Corpus manifest (label, A, B[, extra]); only the labels named in $Runs are read, extras ignored.
    [Parameter(Mandatory = $true)]
    [string]$Manifest,

    [string]$OutDir = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'gap-files/2026-09-19-standoff-listen'),

    # Fixed in § 7. Changing it builds a different set.
    [int]$Seed = 20260919,

    # Seconds of context either side of the gap.
    [double]$ContextSecs = 4.0,

    [switch]$PlanOnly,
    [switch]$KeepMux,
    [switch]$Force,
    [switch]$SkipBuild,
    [string]$CargoProfile = 'release',
    [string]$Features = 'he-aac,ac3,ffmpeg-mux'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
function RepoPath([string]$p) { Join-Path $RepoRoot $p }

# ---- design (must match TEMP-border-standoff-plan.md § 7) --------------------------------------
$pin = '--residual-border-standoff-secs 0.35'
$Runs = @(
    @{ Id = '3-s0.35';   Pair = '3';  Arm = '0.35'; Args = "--border-standoff-secs 0.35 $pin"; Gaps = @(10);      Ref = 'gap-files/2026-09-19-standoff-fixed-0.35/3.json' }
    @{ Id = '3-s0';      Pair = '3';  Arm = '0';    Args = "--border-standoff-secs 0 $pin";    Gaps = @(10);      Ref = 'gap-files/2026-09-19-standoff-fixed-0/3.json' }
    @{ Id = '50-s0.35';  Pair = '50'; Arm = '0.35'; Args = "--border-standoff-secs 0.35 $pin"; Gaps = @(3);       Ref = 'gap-files/2026-09-19-standoff-fixed-0.35/50.json' }
    @{ Id = '50-s0';     Pair = '50'; Arm = '0';    Args = "--border-standoff-secs 0 $pin";    Gaps = @(3);       Ref = 'gap-files/2026-09-19-standoff-fixed-0/50.json' }
    @{ Id = '53-s0.35';  Pair = '53'; Arm = '0.35'; Args = "--border-standoff-secs 0.35 $pin"; Gaps = @(2, 6, 8); Ref = 'gap-files/2026-09-19-standoff-fixed-0.35/53.json' }
    @{ Id = '53-s0';     Pair = '53'; Arm = '0';    Args = "--border-standoff-secs 0 $pin";    Gaps = @(2, 6, 8); Ref = 'gap-files/2026-09-19-standoff-fixed-0/53.json' }
    # Known-audible anchor, default settings; reference is the 2026-09-18 corpus run.
    @{ Id = '26-anchor'; Pair = '26'; Arm = 'anchor'; Args = '';                                  Gaps = @(6);       Ref = 'gap-files/2026-09-18-corpus-patch/26.json' }
)
$RepeatsPerArm = @('0.35', '0')   # one repeat drawn from each arm
$EdgeGuardSecs = 0.050            # check 2 exclusion band and check 3 interior trim
$PlacementTolMs = 1.0
$ReportTolMs = 0.1
$SearchPadSecs = 2.0              # check 3: B window reaches this far past both arms' nominal positions
$FoundMinR = 0.80                 # check 3: a fill below this correlation was not found in B
# -------------------------------------------------------------------------------------------------

$gapCount = ($Runs | Where-Object { $_.Arm -ne 'anchor' } | ForEach-Object { $_.Gaps.Count } | Measure-Object -Sum).Sum / 2
Write-Host ("design: {0} gaps x 2 arms + 1 anchor + {1} repeats = {2} clips; seed {3}" -f `
    $gapCount, $RepeatsPerArm.Count, (2 * $gapCount + 1 + $RepeatsPerArm.Count), $Seed)
foreach ($r in $Runs) {
    $only = ($r.Gaps | ForEach-Object { $_ + 1 }) -join ','
    Write-Host ("  run {0,-10} pair {1,-3} --only-gaps {2,-6} {3}" -f $r.Id, $r.Pair, $only, $r.Args)
}
if ($PlanOnly) { return }

# ---- numeric helpers (compiled once; pwsh loops are too slow for sample work) ------------------
Add-Type -TypeDefinition @'
using System;
public static class ListenDsp {
    public static short[] ReadS16(string path) {
        var b = System.IO.File.ReadAllBytes(path);
        var s = new short[b.Length / 2];
        Buffer.BlockCopy(b, 0, s, 0, s.Length * 2);
        return s;
    }
    public static double[] Mono(short[] s, int ch) {
        int n = s.Length / ch; var m = new double[n];
        for (int i = 0; i < n; i++) { double a = 0; for (int c = 0; c < ch; c++) a += s[i * ch + c]; m[i] = a / ch / 32768.0; }
        return m;
    }
    // First frame outside [lo, hi) where the two interleaved buffers differ; -1 if none; -2 on length mismatch.
    public static long FirstDiffOutside(short[] a, short[] b, int ch, long lo, long hi) {
        if (a.Length != b.Length) return -2;
        long n = a.Length / ch;
        for (long f = 0; f < n; f++) {
            if (f >= lo && f < hi) continue;
            for (int c = 0; c < ch; c++) if (a[f * ch + c] != b[f * ch + c]) return f;
        }
        return -1;
    }
    public static double[] ReadF32(string path) {
        var b = System.IO.File.ReadAllBytes(path);
        var f = new float[b.Length / 4];
        Buffer.BlockCopy(b, 0, f, 0, f.Length * 4);
        var d = new double[f.Length];
        for (int i = 0; i < f.Length; i++) d[i] = f[i];
        return d;
    }
    static double[] Decimate(double[] x, int k) {
        var y = new double[x.Length / k];
        for (int i = 0; i < y.Length; i++) { double a = 0; for (int j = 0; j < k; j++) a += x[i * k + j]; y[i] = a / k; }
        return y;
    }
    static double CorrAt(double[] hay, double[] nd, double ndNorm, int pos) {
        double sxy = 0, syy = 0;
        for (int i = 0; i < nd.Length; i++) { double h = hay[pos + i]; sxy += h * nd[i]; syy += h * h; }
        return syy > 0 ? sxy / Math.Sqrt(syy) / ndNorm : double.NegativeInfinity;
    }
    // Position (frames) in hay where needle[s0..s1) best matches by normalized correlation; {pos, r}.
    // Coarse pass on 8x block-averaged signals, then a full-rate refine around the coarse peak.
    public static double[] FindInHaystack(double[] hay, double[] mono, int s0, int s1) {
        var nd = new double[s1 - s0]; double mean = 0;
        for (int i = 0; i < nd.Length; i++) { nd[i] = mono[s0 + i]; mean += nd[i]; }
        mean /= nd.Length; double nn = 0;
        for (int i = 0; i < nd.Length; i++) { nd[i] -= mean; nn += nd[i] * nd[i]; }
        nn = Math.Sqrt(nn);
        const int K = 8;
        var hd = Decimate(hay, K); var ndd = Decimate(nd, K); double ndn = 0;
        foreach (var v in ndd) ndn += v * v;
        ndn = Math.Sqrt(ndn);
        int bestC = 0; double bestCr = double.NegativeInfinity;
        for (int p = 0; p + ndd.Length <= hd.Length; p++) { double r = CorrAt(hd, ndd, ndn, p); if (r > bestCr) { bestCr = r; bestC = p; } }
        int best = 0; double bestR = double.NegativeInfinity;
        for (int p = Math.Max(0, bestC * K - 4 * K); p <= Math.Min(hay.Length - nd.Length, bestC * K + 4 * K); p++) {
            double r = CorrAt(hay, nd, nn, p); if (r > bestR) { bestR = r; best = p; }
        }
        return new double[] { best, bestR };
    }
    // Masking-study step: max |dx| within +-1 ms of the join, in dB re the 100 ms RMS on the program side.
    public static double StepDb(double[] m, int seam, int sr, bool programAfter) {
        int w = sr / 1000, r = sr / 10; double step = 0;
        for (int i = Math.Max(1, seam - w); i <= Math.Min(m.Length - 1, seam + w); i++) step = Math.Max(step, Math.Abs(m[i] - m[i - 1]));
        int a = programAfter ? seam : seam - r, e = programAfter ? seam + r : seam; double ss = 0; int n = 0;
        for (int i = Math.Max(0, a); i < Math.Min(m.Length, e); i++) { ss += m[i] * m[i]; n++; }
        double rms = n > 0 ? Math.Sqrt(ss / n) : 0;
        return (rms > 0 && step > 0) ? 20 * Math.Log10(step / rms) : double.NaN;
    }
}
'@

# ---- manifest ----------------------------------------------------------------------------------
if (-not (Test-Path -LiteralPath $Manifest)) { throw "manifest not found: $Manifest" }
$pairs = @{}
foreach ($line in Get-Content -LiteralPath $Manifest) {
    if ($line -match '^\s*(#|$)') { continue }
    # Quote-aware split, as in measure-patch-outcomes.ps1: titles may contain the delimiter.
    $f = [System.Collections.Generic.List[string]]::new(); $cur = [System.Text.StringBuilder]::new(); $q = $false
    foreach ($c in $line.ToCharArray()) {
        if ($c -eq '"') { $q = -not $q; continue }
        if ($c -eq ',' -and -not $q) { $f.Add($cur.ToString()); [void]$cur.Clear(); continue }
        [void]$cur.Append($c)
    }
    $f.Add($cur.ToString())
    if ($f.Count -ge 3) { $pairs[$f[0].Trim()] = @{ A = $f[1].Trim(); B = $f[2].Trim() } }
}
foreach ($r in $Runs) { if (-not $pairs.ContainsKey($r.Pair)) { throw "pair $($r.Pair) not in manifest" } }

# ---- build -------------------------------------------------------------------------------------
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

$runDir = Join-Path $OutDir 'runs'
$clipDir = Join-Path $OutDir 'clips'
New-Item -ItemType Directory -Force -Path $runDir, $clipDir | Out-Null
$cfg = Join-Path $runDir 'flac-mux.toml'
Set-Content -LiteralPath $cfg -Encoding utf8 -Value @'
# Generated by build-standoff-listen-set.ps1: lossless mux so clips carry the patched PCM exactly.
[repair.output]
video_codec = "copy"
audio_codec = "flac"
mux_audio_bitrate = "default"
'@

function Read-Json([string]$p) { Get-Content -LiteralPath $p -Raw | ConvertFrom-Json -Depth 64 }

# ---- runs, cuts, check 1 -----------------------------------------------------------------------
$items = [System.Collections.Generic.List[object]]::new()
$failures = [System.Collections.Generic.List[string]]::new()

foreach ($r in $Runs) {
    $report = Join-Path $runDir "$($r.Id).json"
    $log = Join-Path $runDir "$($r.Id).log"
    $mkv = Join-Path $runDir "$($r.Id).mkv"
    $raws = $r.Gaps | ForEach-Object { Join-Path $runDir ("{0}-g{1}.raw" -f $r.Id, $_) }
    $done = (Test-Path -LiteralPath $report) -and ((Get-Item -LiteralPath $report).Length -gt 0) -and
        -not ($raws | Where-Object { -not (Test-Path -LiteralPath $_) })

    if ($Force -or -not $done) {
        $p = $pairs[$r.Pair]
        $only = ($r.Gaps | ForEach-Object { $_ + 1 }) -join ','
        $argv = @($p.A, $p.B, '-c', $cfg, '--mux', $mkv, '--format', 'json', '--only-gaps', $only)
        if ($r.Args) { $argv += $r.Args.Split(' ', [StringSplitOptions]::RemoveEmptyEntries) }
        Write-Host ("[{0}] repair pair {1} ({2}) ..." -f (Get-Date -Format 'HH:mm'), $r.Pair, $r.Id)
        & $exe @argv > $report 2> $log
        if ($LASTEXITCODE -ne 0) { throw "repair $($r.Id) exited $LASTEXITCODE (see $log)" }
    }

    $doc = Read-Json $report
    $ref = Read-Json (RepoPath $r.Ref)
    $sr = [int]$doc.scan.track_compatibility.a_sample_rate
    $ch = [int]$doc.scan.track_compatibility.a_channels

    foreach ($gi in $r.Gaps) {
        $g = $doc.patch.gaps[$gi]; $rg = $ref.patch.gaps[$gi]
        $p = $g.status.patched; $rp = $rg.status.patched
        $tag = "{0}/g{1} {2}" -f $r.Pair, $gi, $r.Arm
        if ($null -eq $p -or $null -eq $rp) { $failures.Add("check 1 $($tag): not patched in run or reference"); continue }
        $bad = @()
        if ([math]::Abs($g.a_start_secs - $rg.a_start_secs) -gt 1e-6) { $bad += 'a_start' }
        foreach ($k in 'dual_fit_used', 'gap_start_adjust_frames', 'gap_end_adjust_frames') { if ($p.$k -ne $rp.$k) { $bad += $k } }
        if ([math]::Abs(($p.align_adjustment_secs - $rp.align_adjustment_secs) * 1000) -gt $ReportTolMs) { $bad += 'align_adjustment' }
        if ([math]::Abs($p.registration_residual_ms - $rp.registration_residual_ms) -gt $ReportTolMs) { $bad += 'residual' }
        if ($r.Arm -ne 'anchor' -and ($p.gap_start_adjust_frames -ne 0 -or $p.gap_end_adjust_frames -ne 0)) { $bad += 'extension (not placement-only)' }
        if ($bad) { $failures.Add("check 1 $($tag): mismatch on $($bad -join ', ')") }
        else { Write-Host ("  check 1 ok  {0,-16} adj {1,8:0.0} ms  residual {2,7:+0.0;-0.0}" -f $tag, ($p.align_adjustment_secs * 1000), $p.registration_residual_ms) }

        $f0 = [long][math]::Round(($g.a_start_secs - $ContextSecs) * $sr)
        $f1 = [long][math]::Round(($g.a_end_secs + $ContextSecs) * $sr)
        $raw = Join-Path $runDir ("{0}-g{1}.raw" -f $r.Id, $gi)
        if ($Force -or -not (Test-Path -LiteralPath $raw)) {
            if (-not (Test-Path -LiteralPath $mkv)) { throw "cut needed but $($r.Id).mkv is gone; re-run with -Force" }
            # atrim counts samples from the stream start, independent of container timestamps.
            ffmpeg -v error -y -i $mkv -map 0:a:0 -af "atrim=start_sample=$($f0):end_sample=$($f1)" -f s16le -c:a pcm_s16le $raw
            if ($LASTEXITCODE) { throw "cut failed: $($r.Id) g$gi" }
        }
        $items.Add([pscustomobject]@{
            pair = $r.Pair; gap = $gi; arm = $r.Arm; raw = $raw; sr = $sr; ch = $ch; f0 = $f0
            a_start = [double]$g.a_start_secs; a_end = [double]$g.a_end_secs
            adj_ms = [double]$p.align_adjustment_secs * 1000; residual_ms = [double]$p.registration_residual_ms
            offset_secs = [double]$doc.scan.alignment.recommended_offset_secs
        })
    }
    if ((Test-Path -LiteralPath $mkv) -and -not $KeepMux) { Remove-Item -LiteralPath $mkv -Force }
}

# ---- checks 2 and 3, seam steps ----------------------------------------------------------------
$pcm = @{}
foreach ($it in $items) {
    $s = [ListenDsp]::ReadS16($it.raw)
    $pcm[$it.raw] = $s
    $m = [ListenDsp]::Mono($s, $it.ch)
    $pre = [int]([math]::Round($it.a_start * $it.sr) - $it.f0)
    $post = [int]([math]::Round($it.a_end * $it.sr) - $it.f0)
    $it | Add-Member step_pre_db ([ListenDsp]::StepDb($m, $pre, $it.sr, $false))
    $it | Add-Member step_post_db ([ListenDsp]::StepDb($m, $post, $it.sr, $true))
    $it | Add-Member mono $m
    $it | Add-Member move_ms $null
}
foreach ($grp in ($items | Where-Object arm -ne 'anchor' | Group-Object pair, gap)) {
    $late = $grp.Group | Where-Object arm -eq '0.35'; $zero = $grp.Group | Where-Object arm -eq '0'
    $tag = "{0}/g{1}" -f $late.pair, $late.gap
    $guard = [long][math]::Round($EdgeGuardSecs * $late.sr)
    $lo = [long][math]::Round($late.a_start * $late.sr) - $late.f0 - $guard
    $hi = [long][math]::Round($late.a_end * $late.sr) - $late.f0 + $guard
    $d = [ListenDsp]::FirstDiffOutside($pcm[$late.raw], $pcm[$zero.raw], $late.ch, $lo, $hi)
    if ($d -eq -2) { $failures.Add("check 2 $($tag): clip lengths differ") }
    elseif ($d -ge 0) { $failures.Add("check 2 $($tag): arms differ outside the fill at frame $d") }
    else { Write-Host "  check 2 ok  $tag  arms identical outside the fill" }

    # One decode of B covering both arms' nominal positions; each fill interior is then located in it.
    # Nominal position = A time + recommended offset + align_adjustment; the pad absorbs any constant
    # between that and where the fill really sits, and a shared window makes seek error cancel.
    $late.move_ms = $zero.adj_ms - $late.adj_ms; $zero.move_ms = $late.move_ms
    $s0 = [int]($lo + 2 * $guard); $s1 = [int]($hi - 2 * $guard)
    $interiorA = $late.a_start + $EdgeGuardSecs
    $adjLo = [math]::Min($late.adj_ms, $zero.adj_ms) / 1000; $adjHi = [math]::Max($late.adj_ms, $zero.adj_ms) / 1000
    $t0 = $interiorA + $late.offset_secs + $adjLo - $SearchPadSecs
    $dur = ($adjHi - $adjLo) + 2 * $SearchPadSecs + ($s1 - $s0) / $late.sr
    $hayPath = Join-Path $runDir ("{0}-g{1}-b.f32" -f $late.pair, $late.gap)
    ffmpeg -v error -y -ss ('{0:0.000000}' -f $t0) -i $pairs[$late.pair].B -map 0:a:0 -t ('{0:0.000}' -f $dur) `
        -ac 1 -ar $late.sr -f f32le $hayPath
    if ($LASTEXITCODE) { throw "B decode failed: $tag" }
    $hay = [ListenDsp]::ReadF32($hayPath)
    $fl = [ListenDsp]::FindInHaystack($hay, $late.mono, $s0, $s1)
    $fz = [ListenDsp]::FindInHaystack($hay, $zero.mono, $s0, $s1)
    $expected = $late.adj_ms - $zero.adj_ms
    $readMs = ($fl[0] - $fz[0]) * 1000.0 / $late.sr
    $line = "{0}  readback {1:0.00} ms vs reports {2:0.00} ms (r {3:0.000} / {4:0.000})" -f $tag, $readMs, $expected, $fl[1], $fz[1]
    if ($fl[1] -lt $FoundMinR -or $fz[1] -lt $FoundMinR) { $failures.Add("check 3 $($line): a fill was not found in B") }
    elseif ([math]::Abs($readMs - $expected) -gt $PlacementTolMs) { $failures.Add("check 3 $line") }
    else { Write-Host "  check 3 ok  $line" }
}

if ($failures.Count) {
    $failures | ForEach-Object { Write-Warning $_ }
    throw "$($failures.Count) check(s) failed; no listen set written (§ 7: the set is not built unless every check passes)"
}

# ---- blind set ---------------------------------------------------------------------------------
$rng = [System.Random]::new($Seed)
$playlist = [System.Collections.Generic.List[object]]::new()
foreach ($it in $items) { $playlist.Add([pscustomobject]@{ item = $it; repeat = $false }) }
foreach ($arm in $RepeatsPerArm) {
    $pool = @($items | Where-Object arm -eq $arm)
    $playlist.Add([pscustomobject]@{ item = $pool[$rng.Next($pool.Count)]; repeat = $true })
}
# Seeded Fisher-Yates, redrawn until no two clips of the same gap sit next to each other.
$gapKey = { param($e) '{0}/{1}' -f $e.item.pair, $e.item.gap }
for ($attempt = 1; ; $attempt++) {
    $arr = $playlist.ToArray()
    for ($i = $arr.Count - 1; $i -gt 0; $i--) { $j = $rng.Next($i + 1); $t = $arr[$i]; $arr[$i] = $arr[$j]; $arr[$j] = $t }
    $adjacent = $false
    for ($i = 1; $i -lt $arr.Count; $i++) { if ((& $gapKey $arr[$i]) -eq (& $gapKey $arr[$i - 1])) { $adjacent = $true; break } }
    if (-not $adjacent) { break }
    if ($attempt -ge 1000) { throw 'could not place clips without same-gap neighbours' }
}

Get-ChildItem -LiteralPath $clipDir -Filter 'clip-*.wav' -File | Remove-Item -Force
$key = [System.Collections.Generic.List[object]]::new()
for ($i = 0; $i -lt $arr.Count; $i++) {
    $e = $arr[$i]; $it = $e.item
    $id = 'clip-{0:00}' -f ($i + 1)
    ffmpeg -v error -y -f s16le -ar $it.sr -ac $it.ch -i $it.raw -c:a pcm_s16le (Join-Path $clipDir "$id.wav")
    if ($LASTEXITCODE) { throw "wav write failed: $id" }
    $key.Add([pscustomobject]@{
        clip = $id; pair = $it.pair; gap_index = $it.gap; arm = $it.arm; repeat = $e.repeat
        a_start_secs = $it.a_start; seam_in_clip_secs = $ContextSecs
        align_adjustment_ms = [math]::Round($it.adj_ms, 2); residual_ms = [math]::Round($it.residual_ms, 2)
        move_ms = if ($null -ne $it.move_ms) { [math]::Round($it.move_ms, 2) } else { '' }
        step_pre_db = [math]::Round($it.step_pre_db, 1); step_post_db = [math]::Round($it.step_post_db, 1)
    })
}
$key | Export-Csv -LiteralPath (Join-Path $OutDir 'key.csv') -NoTypeInformation -Encoding utf8
$key | ForEach-Object { [pscustomobject]@{ clip = $_.clip; label = ''; note = '' } } |
    Export-Csv -LiteralPath (Join-Path $clipDir 'labels.csv') -NoTypeInformation -Encoding utf8

Write-Host ""
Write-Host ("wrote {0} clips to {1}" -f $key.Count, $clipDir)
Write-Host "labels: audible / clean / unsure, one pass, no going back. Leave key.csv closed until done."
