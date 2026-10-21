#!/usr/bin/env pwsh
#Requires -Version 7.0
# Cross-arm OUTCOME differential for two `measure-patch-outcomes.ps1` runs.
#
# Asks one question: does the same gap reach the same VERDICT in both arms, and where it does, which
# fields moved? Not "did one metric move" — did production decide differently, and is every
# difference one the Change under test is supposed to make. Written for the early-shoulder Tier-2
# control (docs/dev/archive/TEMP-early-shoulder-locks-collapse-skip-plan.md § 8.1); the expectation
# is now a parameter (-ExpectReasons / -ExpectPaths), so any flag-off vs flag-on or old-vs-new pair
# of arms can be judged against what it is meant to change.
#
# Unlike diff-patch-adjustments.ps1 this compares the WHOLE gap object field by field rather than
# one metric, so a field this script has never heard of still shows up. New fields are a first-
# class result here: a Change that adds a key is not "unchanged", and a diff that silently ignored
# unknown keys would report a clean run for a corpus it never really compared.
#
# JOIN RULE: baseline x candidate on `patch.gaps[*].a_start_secs`, rounded to 1 ms, WITHIN a pair
# label. NEVER on gap index — the arms do not agree on which gaps get patched or even planned, so
# index i is not the same gap in two arms. A gap with no 1 ms partner is reported UNJOINED per
# side and never silently dropped; unjoined gaps are the loudest possible signal (the arms
# disagree about the gap set itself) and they count into the verdict rather than being excluded
# from it. Gaps left over on BOTH sides get a second pass, matched closest-pair-first by time
# within -JoinToleranceMs (default 1.0, the key's own resolution): a gap whose time moved by a hair
# can straddle a rounding boundary and split into two spurious unjoined findings. Every rescued
# join is listed under NEAR-MISS JOINS with its delta, so no join is silently fuzzy.
#
# CLASSIFICATION, most severe first. Each joined gap lands in exactly one class:
#   status_flip    the externally tagged `status` variant differs (patched <-> skipped <->
#                  not_planned). A different DECISION.
#   reason_change  same variant, different reason name (e.g. correlation_below_threshold ->
#                  shoulders_abut). Same verdict, different cause — this is where an abut lands.
#   field_change   same variant and reason, but some field moved. Subtyped by variant so a
#                  patched-with-different-numbers gap reads apart from a skip whose payload moved.
#   unchanged      every compared field equal (see -Tolerance).
# Absent vs present is a difference, not a tie: several payload booleans carry
# `skip_serializing_if = "is_false"`, so `clamped` flipping false->true appears as a field ADDED.
# Both arms come from one binary, so an added or removed key is a real behaviour change.
#
# RUN-LEVEL FIELDS are compared too — everything on `patch` except `gaps`. They are not derivable
# from the gap list, and two of them decide whether the comparison means anything:
#   patch_anchors_used  step 7's anchored-retry population, the one thing arm B cannot neutralize.
#   fill_geometry       the recipe the arm ran. If the arms differ here, a gap-level difference is
#                       not attributable to the flag under test, and the script says so.
# They are split by whether they FOLLOW from the per-gap verdicts. The four counts and the
# run-level `shoulders_abut` republish move mechanically whenever a gap flips, so they are reported
# as DERIVED and excluded from the verdict — counting them would guarantee noise on a Change that
# worked. Everything else is INDEPENDENT and is a finding.
#
# CHANGED PATHS: a run-wide table over the field_change gaps — each field path with array indices
# folded to `[]`, how many gaps it moved on, and for numbers the median / max |delta| and how many
# went up vs down. A flag that shifts one field on every patch reads as one row here instead of a
# thousand per-gap dumps, and the rare path that should not have moved stands out. Status flips and
# reason changes are left out of the table: their payload is replaced wholesale and would swamp it.
#
# VERDICT: changed gaps split into EXPECTED and UNEXPLAINED. A gap is EXPECTED when
#   - it flipped status or changed reason, and the candidate's reason (or its status, when it has no
#     reason — e.g. `patched`) matches a -ExpectReasons pattern; or
#   - it is a field_change and EVERY changed path matches a -ExpectPaths pattern.
# Unjoined gaps are never expected: the arms disagreeing about the gap set is always a finding. With
# no expectation declared nothing is expected, so the bar is "bit-identical" — the right one for a
# refactor or a default-off flag checked against its own baseline. The bar is UNEXPLAINED = 0 AND
# run-level INDEPENDENT = 0; the script exits 1 when either is non-zero so a run can be gated on it.
# -NoFailOnUnexplained keeps the exit code at 0 for exploratory diffs.
#
# PATTERNS (-IgnorePaths, -ExpectPaths, -ExpectReasons): `*` matches any run of characters, `?`
# one; everything else is literal, `[` and `]` included, so 'tags[*].name' matches 'tags[3].name'
# and 'residual.*' matches every residual leaf. Matched case-insensitively against the whole path.
#
# Usage:
#   ./scripts/measure/diff-patch-outcomes.ps1 `
#       -Baseline gap-files/<date>-flag-off -Candidate gap-files/<date>-flag-on
#   ... -NoFailOnUnexplained          # exploratory: read the CHANGED PATHS table first
#   ... -ExpectPaths 'status.patched.pre_correlation','status.patched.post_correlation'
#   ... -ExpectReasons 'shoulders_abut'   # the old Tier-2 bar
#   ... -Detail                       # every changed field path per gap, not just the count
#   ... -ExcludePairs 75              # shared media (75 reuses 31's B file; see the manifest)
#   ... -CsvOut gap-files/diff.csv
#   ... -IgnorePaths 'tags.*','residual.placement_slide_frames'
#
# Layout: `<arm>/<pair>.json` — a `--patch-only --format json` report per pair, as written by
# measure-patch-outcomes.ps1.
#
# BASELINE ORDER MATTERS for reading the output, not for the maths: name the OLD/control arm
# -Baseline and the NEW/default arm -Candidate, so "reason_change -> shoulders_abut" and the deltas
# in CHANGED PATHS read in the direction the Change actually moved.
#
# MEDIA HYGIENE: emit pair label, gap index and gap time only — never media paths or titles.
# Docs: first written for docs/dev/archive/TEMP-early-shoulder-locks-collapse-skip-plan.md.

[CmdletBinding()]
param(
    # Control / old arm. Directory of `<pair>.json`.
    [Parameter(Mandatory = $true)]
    [string]$Baseline,

    # Default / new arm. Directory of `<pair>.json`.
    [Parameter(Mandatory = $true)]
    [string]$Candidate,

    # Restrict to these pair labels (default: every pair present in BOTH arms).
    [int[]]$Pairs = @(),

    # Drop these pair labels (shared media; the manifest's header comments list the known cases).
    [int[]]$ExcludePairs = @(),

    # Field paths to treat as equal (see PATTERNS, e.g. 'tags.*'). Ignored paths are counted and
    # reported, never silently dropped — an ignore that fires is a caveat on the verdict.
    [string[]]$IgnorePaths = @(),

    # Absolute tolerance for numeric fields. Default 0 = exact. Both arms come from one binary and
    # the pipeline is deterministic, so any non-zero value here asserts a fuzz that should not
    # exist; the script says so when you pass one.
    [double]$Tolerance = 0.0,

    # Slack, in milliseconds, for the SECOND join pass over gaps left unmatched on both sides.
    # Default 1.0 = the join key's own resolution, so it only rescues pairs that a rounding
    # boundary split. Every rescue is reported. 0 disables the pass (exact keys only).
    [double]$JoinToleranceMs = 1.0,

    # Candidate-side reasons (or statuses, for a reasonless verdict like `patched`) a status flip or
    # reason change is EXPECTED to land on. Default none.
    [string[]]$ExpectReasons = @(),

    # Field paths a same-verdict gap is EXPECTED to move on (see PATTERNS). A field_change gap is
    # expected only when every one of its changed paths matches. Default none.
    [string[]]$ExpectPaths = @(),

    # Print every changed field path per gap, not just the count.
    [switch]$Detail,

    # Cap on gaps listed per class (CSV always carries every gap).
    [int]$Show = 40,

    # Per-gap CSV: pair, indices, time, class, statuses, reasons, changed paths.
    [string]$CsvOut = '',

    # Exit 0 even when unexplained changes exist.
    [switch]$NoFailOnUnexplained
)

$ErrorActionPreference = 'Stop'

# --- helpers ---------------------------------------------------------------

function Get-StartKey([object]$Secs) {
    # 1 ms resolution, culture-invariant, so two arms agree on the join key.
    return ([math]::Round([double]$Secs, 3)).ToString('F3', [cultureinfo]::InvariantCulture)
}

# Glob over field paths: `*` and `?` are the only metacharacters. PowerShell's -like would read the
# `[3]` in an array path as a character class, so 'gaps[3]' would never match itself.
$script:PatternCache = @{}
function Test-PathMatch([string]$Path, [string[]]$Patterns) {
    foreach ($pat in $Patterns) {
        $rx = $script:PatternCache[$pat]
        if ($null -eq $rx) {
            $body = [regex]::Escape($pat).Replace('\*', '.*').Replace('\?', '.')
            $rx = [regex]::new("^$body$", 'IgnoreCase, CultureInvariant')
            $script:PatternCache[$pat] = $rx
        }
        if ($rx.IsMatch($Path)) { return $true }
    }
    return $false
}

# Array indices folded away, so one field across every element (and every gap) is one row.
function Get-PathShape([string]$Path) { return [regex]::Replace($Path, '\[\d+\]', '[]') }

function Test-HasProp([object]$Obj, [string]$Name) {
    if ($null -eq $Obj) { return $false }
    return $null -ne $Obj.PSObject.Properties[$Name]
}

function Get-VariantName([object]$Node) {
    # Externally tagged enums: a unit variant serializes as "name", a payload variant as
    # { "name": {...} }. Both must answer the same question.
    if ($null -eq $Node) { return 'absent' }
    if ($Node -is [string]) { return [string]$Node }
    $p = @($Node.PSObject.Properties)
    if ($p.Count -eq 0) { return 'absent' }
    # Exactly one key is what "externally tagged" means. More than one is a shape this script does
    # not understand, and picking $p[0] by document order would answer confidently and wrongly.
    if ($p.Count -gt 1) { return ('<multi:' + (@($p.Name) -join '+') + '>') }
    return $p[0].Name
}

function Get-StatusName([object]$Status) { return Get-VariantName $Status }

function Get-ReasonName([object]$Status) {
    # The reason sits under the variant payload: status.skipped.reason (itself an enum, string or
    # single-key object) and status.not_planned.reason (a plain string). `patched` has none.
    $name = Get-StatusName $Status
    if ($name -eq 'absent' -or $name -eq 'patched') { return '' }
    $payload = $Status.PSObject.Properties[$name].Value
    if (-not (Test-HasProp $payload 'reason')) { return '' }
    return Get-VariantName $payload.reason
}

function Test-IsNumber([object]$V) {
    return ($V -is [double] -or $V -is [int] -or $V -is [long] -or $V -is [decimal] -or $V -is [single])
}

function Get-TypeTag([object]$V) {
    # Comparison is type-aware: JSON `true` and the string "true" both render as `true`, so a
    # value-only compare would call a type change equal.
    if ($null -eq $V) { return 'null' }
    if ($V -is [bool]) { return 'bool' }
    if (Test-IsNumber $V) { return 'num' }
    if ($V -is [System.Collections.IList]) { return 'list' }
    if ($V -is [pscustomobject]) { return 'object' }
    return 'string'
}

function Format-Scalar([object]$V) {
    if ($null -eq $V) { return '<null>' }
    if ($V -is [bool]) { if ($V) { return 'true' } else { return 'false' } }
    # Bare ToString on .NET 5+ is the SHORTEST ROUND-TRIPPABLE form: exact, and still readable
    # (0.35 stays "0.35", where G17 gives "0.34999999999999998"). Precision here is not cosmetic —
    # G9 renders 0.9558685349419397 and 0.9558685349419398 identically, which would print a changed
    # row whose two sides look the same and read as a broken tool.
    if (Test-IsNumber $V) { return ([double]$V).ToString([cultureinfo]::InvariantCulture) }
    # Objects and lists are expanded leaf by leaf before they ever reach here; the fallback exists
    # so an unexpected shape reads as a shape rather than as PowerShell's `@{...}` stringification.
    if ($V -is [pscustomobject]) { return '<object>' }
    if ($V -is [System.Collections.IList]) { return '<list>' }
    return [string]$V
}

# Sentinel for "key absent", distinct from a real JSON null so the two cannot collapse together.
# Identified by REFERENCE, not by a marker property: a report field that happened to be named the
# same as the marker would otherwise be mistaken for the sentinel and silently dropped.
$script:Missing = [pscustomobject]@{ __absent = $true }

function Test-IsMissing([object]$V) {
    return [object]::ReferenceEquals($V, $script:Missing)
}

# Recursive field-by-field comparison; one record per differing leaf, with a dotted path.
function Compare-Node {
    param([object]$A, [object]$B, [string]$Path, [System.Collections.Generic.List[object]]$Out)

    $aMissing = Test-IsMissing $A
    $bMissing = Test-IsMissing $B
    if ($aMissing -and $bMissing) { return }

    if ($aMissing -or $bMissing) {
        $present = $A
        if ($aMissing) { $present = $B }
        # Recurse into a whole subtree that appeared or vanished, so its leaves are named. A status
        # flip replaces one payload with another, and that payload IS the finding — collapsing it to
        # one `@{reason=}` row would hide the very fields the flip is being read for.
        # Descend one level per call (never re-enter on the same node, which would not terminate):
        # each child is compared against the sentinel until it bottoms out in scalars.
        # An empty object or list expands to nothing, which would make the difference invisible;
        # $before catches that and emits the node itself instead.
        $before = $Out.Count
        if ($present -is [System.Collections.IList]) {
            for ($i = 0; $i -lt $present.Count; $i++) {
                $av = $script:Missing
                $bv = $script:Missing
                if ($aMissing) { $bv = $present[$i] } else { $av = $present[$i] }
                Compare-Node -A $av -B $bv -Path "$Path[$i]" -Out $Out
            }
            if ($Out.Count -eq $before) {
                $Out.Add([pscustomobject]@{ Path = $Path; Base = '[]'; Cand = '[]'; Kind = $(if ($aMissing) { 'added' } else { 'removed' }) })
            }
            return
        }
        if ($present -is [pscustomobject]) {
            foreach ($p in $present.PSObject.Properties) {
                $av = $script:Missing
                $bv = $script:Missing
                if ($aMissing) { $bv = $p.Value } else { $av = $p.Value }
                $child = $p.Name
                if ($Path) { $child = "$Path.$($p.Name)" }
                Compare-Node -A $av -B $bv -Path $child -Out $Out
            }
            if ($Out.Count -eq $before) {
                $Out.Add([pscustomobject]@{ Path = $Path; Base = '{}'; Cand = '{}'; Kind = $(if ($aMissing) { 'added' } else { 'removed' }) })
            }
            return
        }
        $av = ''
        $bv = ''
        if (-not $aMissing) { $av = Format-Scalar $A }
        if (-not $bMissing) { $bv = Format-Scalar $B }
        $kind = 'removed'
        if ($aMissing) { $kind = 'added' }
        $Out.Add([pscustomobject]@{ Path = $Path; Base = $av; Cand = $bv; Kind = $kind })
        return
    }

    if ($null -eq $A -and $null -eq $B) { return }
    if ($null -eq $A -or $null -eq $B) {
        $Out.Add([pscustomobject]@{ Path = $Path; Base = (Format-Scalar $A); Cand = (Format-Scalar $B); Kind = 'changed' })
        return
    }

    # Arrays compare positionally: these are ordered result lists, not sets.
    if ($A -is [System.Collections.IList] -and $B -is [System.Collections.IList]) {
        $n = [math]::Max($A.Count, $B.Count)
        for ($i = 0; $i -lt $n; $i++) {
            $av = $script:Missing
            $bv = $script:Missing
            if ($i -lt $A.Count) { $av = $A[$i] }
            if ($i -lt $B.Count) { $bv = $B[$i] }
            Compare-Node -A $av -B $bv -Path "$Path[$i]" -Out $Out
        }
        return
    }

    if ($A -is [pscustomobject] -and $B -is [pscustomobject]) {
        $names = [System.Collections.Generic.List[string]]::new()
        foreach ($p in $A.PSObject.Properties) { $names.Add($p.Name) }
        foreach ($p in $B.PSObject.Properties) { if (-not $names.Contains($p.Name)) { $names.Add($p.Name) } }
        foreach ($nm in $names) {
            $av = $script:Missing
            $bv = $script:Missing
            if (Test-HasProp $A $nm) { $av = $A.PSObject.Properties[$nm].Value }
            if (Test-HasProp $B $nm) { $bv = $B.PSObject.Properties[$nm].Value }
            $child = $nm
            if ($Path) { $child = "$Path.$nm" }
            Compare-Node -A $av -B $bv -Path $child -Out $Out
        }
        return
    }

    # A type change is itself a difference — object where a scalar was, string where a bool was.
    # Reached only when the two sides are not the same composite kind, so the pairs handled above
    # (list/list, object/object) never fall in here.
    $ta = Get-TypeTag $A
    $tb = Get-TypeTag $B
    if ($ta -ne $tb) {
        $Out.Add([pscustomobject]@{
                Path = $Path
                Base = ('{0} ({1})' -f (Format-Scalar $A), $ta)
                Cand = ('{0} ({1})' -f (Format-Scalar $B), $tb)
                Kind = 'changed'
            })
        return
    }

    if ((Test-IsNumber $A) -and (Test-IsNumber $B)) {
        if ([math]::Abs([double]$A - [double]$B) -le $Tolerance) { return }
        $Out.Add([pscustomobject]@{ Path = $Path; Base = (Format-Scalar $A); Cand = (Format-Scalar $B); Kind = 'changed' })
        return
    }

    if ((Format-Scalar $A) -cne (Format-Scalar $B)) {
        $Out.Add([pscustomobject]@{ Path = $Path; Base = (Format-Scalar $A); Cand = (Format-Scalar $B); Kind = 'changed' })
    }
}

function Get-ArmIndex {
    # .Pairs  label -> { start_key -> @{ Idx; Gap } } over ALL gaps (patched or not)
    # .Runs   label -> the `patch` object MINUS `gaps` (run-level summary fields)
    # .Files  count of <pair>.json read
    # .Dupes  gaps colliding on the 1 ms join key
    param([string]$Dir)
    $out = @{}
    $runs = @{}
    $files = 0
    $dupes = [System.Collections.Generic.List[string]]::new()
    Get-ChildItem -LiteralPath $Dir -File -Filter '*.json' -ErrorAction SilentlyContinue |
        Where-Object { $_.BaseName -match '^\d+$' } |
        ForEach-Object {
            $label = [int]$_.BaseName
            $root = Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json
            if ($null -eq $root) {
                # measure-patch-outcomes.ps1 creates the report when a pair starts, so an empty file
                # is usually the pair still running.
                Write-Warning "pair $label in $(Split-Path -Leaf $Dir) is empty (still running?) - skipped"
                return
            }
            if ($null -eq $root.patch) {
                Write-Warning "pair $label in $(Split-Path -Leaf $Dir) has no patch section - skipped"
                return
            }
            $files++

            # Run-level summary: everything on `patch` except the per-gap array, which the join
            # below handles. These fields are NOT derivable from the gap list — `patch_anchors_used`
            # is step 7's anchored-retry population and `fill_geometry` is the recipe the arm ran —
            # so leaving them out would let a real change pass as a clean diff.
            $run = [pscustomobject]@{}
            foreach ($p in $root.patch.PSObject.Properties) {
                if ($p.Name -eq 'gaps') { continue }
                $run | Add-Member -NotePropertyName $p.Name -NotePropertyValue $p.Value
            }
            $runs[$label] = $run

            $byStart = @{}
            $i = 0
            foreach ($gap in @($root.patch.gaps)) {
                if ($null -eq $gap) { continue }
                $key = Get-StartKey $gap.a_start_secs
                if ($byStart.ContainsKey($key)) {
                    # Two gaps inside 1 ms would make the join ambiguous. Never observed, but a
                    # silent overwrite would drop a gap from the comparison entirely.
                    $dupes.Add("$label@$key")
                }
                # Secs is kept alongside the key so the near-miss pass never re-parses the
                # formatted key (which would read it back under the CURRENT culture, not the
                # invariant one it was written with).
                $byStart[$key] = @{ Idx = $i; Gap = $gap; Secs = [double]$gap.a_start_secs }
                $i++
            }
            $out[$label] = $byStart
        }
    return [pscustomobject]@{ Pairs = $out; Runs = $runs; Files = $files; Dupes = $dupes }
}

# --- load arms -------------------------------------------------------------

foreach ($d in @($Baseline, $Candidate)) {
    if (-not (Test-Path -LiteralPath $d)) { throw "arm directory not found: $d" }
}
$baseDir = (Resolve-Path -LiteralPath $Baseline).Path
$candDir = (Resolve-Path -LiteralPath $Candidate).Path
if ($baseDir -eq $candDir) { throw 'baseline and candidate are the same directory' }

$baseIx = Get-ArmIndex -Dir $baseDir
$candIx = Get-ArmIndex -Dir $candDir
if ($baseIx.Pairs.Count -eq 0) { throw "no <pair>.json patch reports under $baseDir" }
if ($candIx.Pairs.Count -eq 0) { throw "no <pair>.json patch reports under $candDir" }

if ($Tolerance -gt 0) {
    Write-Warning ("-Tolerance {0} asserts numeric fuzz between two arms of one deterministic binary. A real difference at or below it reads as unchanged." -f $Tolerance)
}
foreach ($ix in @($baseIx, $candIx)) {
    if ($ix.Dupes.Count -gt 0) {
        Write-Warning ("gaps collide on the 1 ms join key (pair@key): {0}. The later gap wins; the join is ambiguous there." -f ($ix.Dupes -join ', '))
    }
}

# --- pair selection --------------------------------------------------------

$baseOnlyPairs = @($baseIx.Pairs.Keys | Where-Object { -not $candIx.Pairs.ContainsKey($_) } | Sort-Object)
$candOnlyPairs = @($candIx.Pairs.Keys | Where-Object { -not $baseIx.Pairs.ContainsKey($_) } | Sort-Object)

$labels = @($baseIx.Pairs.Keys | Where-Object { $candIx.Pairs.ContainsKey($_) })
if ($Pairs.Count -gt 0) { $labels = @($labels | Where-Object { $Pairs -contains $_ }) }
if ($ExcludePairs.Count -gt 0) { $labels = @($labels | Where-Object { $ExcludePairs -notcontains $_ }) }
$labels = @($labels | Sort-Object)
# Named-but-absent is checked BEFORE emptiness: a typo'd -Pairs otherwise reports as "the arms
# share no label", which blames the runs for a mistake in the argument.
if ($Pairs.Count -gt 0) {
    $missing = @($Pairs | Where-Object { $labels -notcontains $_ })
    if ($missing.Count -gt 0) { throw ("requested -Pairs missing from an arm: {0}" -f ($missing -join ', ')) }
}
if ($labels.Count -eq 0) { throw 'no pair label is present in both arms after -Pairs / -ExcludePairs' }

# --- join and classify -----------------------------------------------------

# Run-level fields that move mechanically with the per-gap verdicts. `shoulders_abut` is the
# run-level republish of the per-gap abut skips; the counts are recomputed from the gap
# list by PatchSummary::from_outcomes. Anything NOT here is independent and counts as a finding.
$script:DerivedRunFields = @(
    'patched_count', 'patched_marginal_count', 'skipped_count', 'not_planned_count',
    'not_applied_count', 'shoulders_abut'
)

$rows = [System.Collections.Generic.List[object]]::new()
$runRows = [System.Collections.Generic.List[object]]::new()
$ignoredHits = @{}
$joined = 0

foreach ($label in $labels) {
    $b = $baseIx.Pairs[$label]
    $c = $candIx.Pairs[$label]

    # Exact keys can split a gap that moved by a hair: two times 0.02 us apart straddle a rounding
    # boundary and land on adjacent keys, which would surface as TWO unjoined gaps — two spurious
    # findings against a bar of zero. Arms from one binary share a scan and never hit this; a diff
    # against an older run can. So leftovers on both sides get a second pass, matched by nearest
    # time within -JoinToleranceMs. This never joins anything the 1 ms rule would not: the default
    # tolerance IS the key's own resolution, and every rescue is reported rather than absorbed.
    $rescue = @{}
    $takenCand = @{}
    $baseLeft = @($b.Keys | Where-Object { -not $c.ContainsKey($_) } | Sort-Object { $b[$_].Secs })
    $candLeft = @($c.Keys | Where-Object { -not $b.ContainsKey($_) })
    if ($baseLeft.Count -gt 0 -and $candLeft.Count -gt 0 -and $JoinToleranceMs -gt 0) {
        $tol = $JoinToleranceMs / 1000.0
        # Closest pair first, globally - not first-in-time-wins. Taking them in time order lets an
        # early gap claim a candidate 0.9 ms away that a later gap matches to 0.3 ms, leaving the
        # better match unjoined. Leftovers are few, so the full sort costs nothing.
        $cands = [System.Collections.Generic.List[object]]::new()
        foreach ($bk in $baseLeft) {
            foreach ($ck in $candLeft) {
                $d = [math]::Abs($c[$ck].Secs - $b[$bk].Secs)
                if ($d -le $tol) { $cands.Add([pscustomobject]@{ B = $bk; C = $ck; D = $d }) }
            }
        }
        foreach ($m in ($cands | Sort-Object D)) {
            if ($rescue.ContainsKey($m.B) -or $takenCand.ContainsKey($m.C)) { continue }
            $rescue[$m.B] = $m.C
            $takenCand[$m.C] = $m.B
        }
    }

    foreach ($key in @($b.Keys | Sort-Object { $b[$_].Secs })) {
        $ckey = $key
        $nearMissMs = $null
        if (-not $c.ContainsKey($key)) {
            if (-not $rescue.ContainsKey($key)) {
                $rows.Add([pscustomobject]@{
                        Pair       = $label
                        BaseIdx    = $b[$key].Idx
                        CandIdx    = $null
                        Start      = $key
                        Class      = 'unjoined_baseline'
                        StatusBase = (Get-StatusName $b[$key].Gap.status)
                        StatusCand = ''
                        ReasonBase = (Get-ReasonName $b[$key].Gap.status)
                        ReasonCand = ''
                        Diffs      = @()
                        Ignored    = 0
                        NearMissMs = $null
                    })
                continue
            }
            $ckey = $rescue[$key]
            $nearMissMs = ($c[$ckey].Secs - $b[$key].Secs) * 1000.0
        }
        $joined++
        $gb = $b[$key].Gap
        $gc = $c[$ckey].Gap

        $all = [System.Collections.Generic.List[object]]::new()
        Compare-Node -A $gb -B $gc -Path '' -Out $all

        $kept = [System.Collections.Generic.List[object]]::new()
        $nIgnored = 0
        foreach ($d in $all) {
            if (Test-PathMatch $d.Path $IgnorePaths) {
                $nIgnored++
                if ($ignoredHits.ContainsKey($d.Path)) { $ignoredHits[$d.Path]++ } else { $ignoredHits[$d.Path] = 1 }
            }
            else { $kept.Add($d) }
        }

        $sb = Get-StatusName $gb.status
        $sc = Get-StatusName $gc.status
        $rb = Get-ReasonName $gb.status
        $rc = Get-ReasonName $gc.status

        $class = 'unchanged'
        if ($sb -ne $sc) { $class = 'status_flip' }
        elseif ($rb -ne $rc) { $class = 'reason_change' }
        elseif ($kept.Count -gt 0) { $class = "field_change:$sb" }

        $rows.Add([pscustomobject]@{
                Pair       = $label
                BaseIdx    = $b[$key].Idx
                CandIdx    = $c[$ckey].Idx
                Start      = $key
                Class      = $class
                StatusBase = $sb
                StatusCand = $sc
                ReasonBase = $rb
                ReasonCand = $rc
                Diffs      = @($kept)
                Ignored    = $nIgnored
                NearMissMs = $nearMissMs
            })
    }

    # Run-level fields, per pair. Split by whether the field FOLLOWS from the per-gap verdicts:
    # the four counts and the run-level `shoulders_abut` republish move mechanically when a gap
    # flips, so counting them as findings would guarantee noise on a successful Change. Everything
    # else is independent of the gap list and any movement there is a real, separate finding.
    $runDiffs = [System.Collections.Generic.List[object]]::new()
    Compare-Node -A $baseIx.Runs[$label] -B $candIx.Runs[$label] -Path '' -Out $runDiffs
    foreach ($d in $runDiffs) {
        if (Test-PathMatch $d.Path $IgnorePaths) {
            if ($ignoredHits.ContainsKey($d.Path)) { $ignoredHits[$d.Path]++ } else { $ignoredHits[$d.Path] = 1 }
            continue
        }
        $root = ($d.Path -split '[.\[]')[0]
        $derived = $root -in $script:DerivedRunFields
        $runRows.Add([pscustomobject]@{
                Pair    = $label
                Path    = $d.Path
                Base    = $(if ($d.Kind -eq 'added') { '<absent>' } else { $d.Base })
                Cand    = $(if ($d.Kind -eq 'removed') { '<absent>' } else { $d.Cand })
                Derived = $derived
            })
    }

    foreach ($key in @($c.Keys | Where-Object { -not $b.ContainsKey($_) -and -not $takenCand.ContainsKey($_) } | Sort-Object { $c[$_].Secs })) {
        $rows.Add([pscustomobject]@{
                Pair       = $label
                BaseIdx    = $null
                CandIdx    = $c[$key].Idx
                Start      = $key
                Class      = 'unjoined_candidate'
                StatusBase = ''
                StatusCand = (Get-StatusName $c[$key].Gap.status)
                ReasonBase = ''
                ReasonCand = (Get-ReasonName $c[$key].Gap.status)
                Diffs      = @()
                Ignored    = 0
                NearMissMs = $null
            })
    }
}

# --- report ----------------------------------------------------------------

foreach ($r in $rows) {
    # See VERDICT. An unjoined gap is never expected: the arms disagreeing about the gap set is a
    # finding whatever reason the surviving side carries.
    $exp = $false
    if ($r.Class -eq 'status_flip' -or $r.Class -eq 'reason_change') {
        $landed = $r.ReasonCand
        if (-not $landed) { $landed = $r.StatusCand }
        $exp = Test-PathMatch $landed $ExpectReasons
    }
    elseif ($r.Class -like 'field_change:*' -and $ExpectPaths.Count -gt 0) {
        $exp = $true
        foreach ($d in $r.Diffs) { if (-not (Test-PathMatch $d.Path $ExpectPaths)) { $exp = $false; break } }
    }
    $r | Add-Member -NotePropertyName Expected -NotePropertyValue $exp
}
$changed = @($rows | Where-Object { $_.Class -ne 'unchanged' })
$expected = @($changed | Where-Object { $_.Expected })
$unexplained = @($changed | Where-Object { -not $_.Expected })
$runIndependent = @($runRows | Where-Object { -not $_.Derived })
$runDerived = @($runRows | Where-Object { $_.Derived })

Write-Host ''
Write-Host ("baseline  {0}   ({1} pair reports)" -f (Split-Path -Leaf $baseDir), $baseIx.Files)
Write-Host ("candidate {0}   ({1} pair reports)" -f (Split-Path -Leaf $candDir), $candIx.Files)
Write-Host ("compared  {0} pair label(s), {1} joined gap(s), {2} row(s) total" -f $labels.Count, $joined, $rows.Count)
if ($baseOnlyPairs.Count -gt 0) { Write-Warning ("pair label(s) only in baseline: {0}" -f ($baseOnlyPairs -join ', ')) }
if ($candOnlyPairs.Count -gt 0) { Write-Warning ("pair label(s) only in candidate: {0}" -f ($candOnlyPairs -join ', ')) }
Write-Host ''

Write-Host 'class                       gaps'
foreach ($g in @($rows | Group-Object Class | Sort-Object Name)) {
    Write-Host ('  {0,-24} {1,5}' -f $g.Name, $g.Count)
}
Write-Host ''

if ($ignoredHits.Count -gt 0) {
    $tot = (@($ignoredHits.Values) | Measure-Object -Sum).Sum
    Write-Host ("ignored by -IgnorePaths ({0} leaf hit(s)) - the verdict below does NOT cover these:" -f $tot)
    foreach ($k in @($ignoredHits.Keys | Sort-Object)) { Write-Host ('  {0,-48} {1,5}' -f $k, $ignoredHits[$k]) }
    Write-Host ''
}

function Show-Class([string]$Name, [object[]]$Set) {
    if ($Set.Count -eq 0) { return }
    Write-Host ("{0} ({1}):" -f $Name, $Set.Count)
    foreach ($r in ($Set | Select-Object -First $Show)) {
        $verdict = "$($r.StatusBase), $($r.Diffs.Count) field(s)"
        if ($r.Class -eq 'unjoined_baseline') { $verdict = "baseline-only, $($r.StatusBase)" }
        elseif ($r.Class -eq 'unjoined_candidate') { $verdict = "candidate-only, $($r.StatusCand)" }
        elseif ($r.StatusBase -ne $r.StatusCand) { $verdict = "$($r.StatusBase) -> $($r.StatusCand)" }
        elseif ($r.ReasonBase -ne $r.ReasonCand) { $verdict = "$($r.StatusBase): $($r.ReasonBase) -> $($r.ReasonCand)" }
        $idx = $r.CandIdx
        if ($null -ne $r.BaseIdx) { $idx = $r.BaseIdx }
        Write-Host ('  pair {0,-3} g{1,-4} t={2,-12} {3}' -f $r.Pair, $idx, $r.Start, $verdict)
        if ($Detail) {
            foreach ($d in $r.Diffs) {
                $bv = $d.Base
                $cv = $d.Cand
                if ($d.Kind -eq 'added') { $bv = '<absent>' }
                if ($d.Kind -eq 'removed') { $cv = '<absent>' }
                Write-Host ('        {0,-44} {1}  ->  {2}' -f $d.Path, $bv, $cv)
            }
        }
    }
    if ($Set.Count -gt $Show) { Write-Host ("  ... {0} more (raise -Show, or use -CsvOut)" -f ($Set.Count - $Show)) }
    Write-Host ''
}

# One row per path shape over the same-verdict gaps (see CHANGED PATHS in the header).
$fieldRows = @($changed | Where-Object { $_.Class -like 'field_change:*' })
$pathStats = @{}
foreach ($r in $fieldRows) {
    $seen = @{}
    foreach ($d in $r.Diffs) {
        $shape = Get-PathShape $d.Path
        $st = $pathStats[$shape]
        if ($null -eq $st) {
            $st = [pscustomobject]@{
                Path = $shape; Gaps = 0; Leaves = 0; Added = 0; Removed = 0; Up = 0; Down = 0
                Deltas = [System.Collections.Generic.List[double]]::new()
                Moves  = @{}
            }
            $pathStats[$shape] = $st
        }
        $st.Leaves++
        if (-not $seen.ContainsKey($shape)) { $st.Gaps++; $seen[$shape] = $true }
        if ($d.Kind -eq 'added') { $st.Added++; continue }
        if ($d.Kind -eq 'removed') { $st.Removed++; continue }
        $bv = 0.0
        $cv = 0.0
        $inv = [cultureinfo]::InvariantCulture
        if ([double]::TryParse($d.Base, [System.Globalization.NumberStyles]::Float, $inv, [ref]$bv) -and
            [double]::TryParse($d.Cand, [System.Globalization.NumberStyles]::Float, $inv, [ref]$cv)) {
            $delta = $cv - $bv
            $st.Deltas.Add($delta)
            if ($delta -gt 0) { $st.Up++ } elseif ($delta -lt 0) { $st.Down++ }
        }
        else {
            # Booleans and enum names: the transition itself is the summary.
            $mv = "$($d.Base)->$($d.Cand)"
            if ($st.Moves.ContainsKey($mv)) { $st.Moves[$mv]++ } else { $st.Moves[$mv] = 1 }
        }
    }
}
if ($pathStats.Count -gt 0) {
    Write-Host ("CHANGED PATHS over {0} same-verdict gap(s) - indices folded to []; delta = candidate - baseline:" -f $fieldRows.Count)
    Write-Host ('  {0,-52} {1,5} {2,6} {3,11} {4,11} {5,9}  {6}' -f 'path', 'gaps', 'leaves', 'median d', 'max |d|', 'up/down', 'note')
    $ordered = @($pathStats.Values | Sort-Object @{ Expression = 'Gaps'; Descending = $true }, Path)
    foreach ($st in ($ordered | Select-Object -First $Show)) {
        $med = ''
        $max = ''
        $ud = ''
        if ($st.Deltas.Count -gt 0) {
            $sorted = @($st.Deltas | Sort-Object)
            $m = $sorted.Count
            $medV = if ($m % 2) { $sorted[($m - 1) / 2] } else { ($sorted[$m / 2 - 1] + $sorted[$m / 2]) / 2 }
            $maxV = (@($sorted | ForEach-Object { [math]::Abs($_) }) | Measure-Object -Maximum).Maximum
            $med = $medV.ToString('G4', [cultureinfo]::InvariantCulture)
            $max = ([double]$maxV).ToString('G4', [cultureinfo]::InvariantCulture)
            $ud = "$($st.Up)/$($st.Down)"
        }
        $notes = @()
        if ($st.Added -gt 0) { $notes += "added $($st.Added)" }
        if ($st.Removed -gt 0) { $notes += "removed $($st.Removed)" }
        if ($st.Moves.Count -gt 0) {
            $top = @($st.Moves.GetEnumerator() | Sort-Object @{ Expression = 'Value'; Descending = $true }, Name)
            $notes += (@($top | Select-Object -First 3 | ForEach-Object { "$($_.Name) x$($_.Value)" }) -join ' ')
            if ($top.Count -gt 3) { $notes += "+$($top.Count - 3) more" }
        }
        if ($ExpectPaths.Count -gt 0 -and -not (Test-PathMatch $st.Path $ExpectPaths)) { $notes += 'NOT EXPECTED' }
        Write-Host ('  {0,-52} {1,5} {2,6} {3,11} {4,11} {5,9}  {6}' -f $st.Path, $st.Gaps, $st.Leaves, $med, $max, $ud, ($notes -join ', '))
    }
    if ($ordered.Count -gt $Show) { Write-Host ("  ... {0} more path(s) (raise -Show)" -f ($ordered.Count - $Show)) }
    Write-Host ''
}

Show-Class 'STATUS FLIPS' @($changed | Where-Object { $_.Class -eq 'status_flip' })
Show-Class 'REASON CHANGES' @($changed | Where-Object { $_.Class -eq 'reason_change' })
# With -ExpectPaths, list only the gaps that moved somewhere else; the expected ones are counted.
$fieldExpected = @($fieldRows | Where-Object { $_.Expected })
if ($ExpectPaths.Count -gt 0) {
    Show-Class 'FIELD CHANGES OUTSIDE -ExpectPaths' @($fieldRows | Where-Object { -not $_.Expected })
    if ($fieldExpected.Count -gt 0) {
        Write-Host ("field changes inside -ExpectPaths: {0} gap(s) - -CsvOut to list them" -f $fieldExpected.Count)
        Write-Host ''
    }
}
else { Show-Class 'FIELD CHANGES' $fieldRows }
Show-Class 'UNJOINED (baseline)' @($changed | Where-Object { $_.Class -eq 'unjoined_baseline' })
Show-Class 'UNJOINED (candidate)' @($changed | Where-Object { $_.Class -eq 'unjoined_candidate' })

function Show-RunRows([string]$Name, [object[]]$Set) {
    if ($Set.Count -eq 0) { return }
    Write-Host ("{0} ({1}):" -f $Name, $Set.Count)
    foreach ($r in ($Set | Select-Object -First $Show)) {
        Write-Host ('  pair {0,-3} {1,-40} {2}  ->  {3}' -f $r.Pair, $r.Path, $r.Base, $r.Cand)
    }
    if ($Set.Count -gt $Show) { Write-Host ("  ... {0} more (raise -Show)" -f ($Set.Count - $Show)) }
    Write-Host ''
}

# Rescued joins are reported whether or not the gap changed: a shifted gap time is itself a fact
# about the run, and a reader must be able to see that a join was not exact.
$nearMiss = @($rows | Where-Object { $null -ne $_.NearMissMs })
if ($nearMiss.Count -gt 0) {
    Write-Host ("NEAR-MISS JOINS ({0}) - matched within {1} ms, not on an exact key:" -f $nearMiss.Count, $JoinToleranceMs)
    foreach ($r in ($nearMiss | Select-Object -First $Show)) {
        Write-Host ('  pair {0,-3} g{1,-4} t={2,-12} delta {3:g4} ms  ({4})' -f $r.Pair, $r.BaseIdx, $r.Start, $r.NearMissMs, $r.Class)
    }
    if ($nearMiss.Count -gt $Show) { Write-Host ("  ... {0} more (raise -Show)" -f ($nearMiss.Count - $Show)) }
    Write-Host ''
}

Show-RunRows 'RUN-LEVEL, INDEPENDENT OF THE GAP LIST' $runIndependent
if ($Detail) { Show-RunRows 'RUN-LEVEL, DERIVED FROM THE GAP VERDICTS (not findings)' $runDerived }
elseif ($runDerived.Count -gt 0) {
    Write-Host ("run-level derived fields moved on {0} row(s) (counts / shoulders_abut republish) - -Detail to list" -f $runDerived.Count)
    Write-Host ''
}

# A recipe difference means a gap-level diff cannot be attributed to the flag under test.
$geo = @($runIndependent | Where-Object { $_.Path -like 'fill_geometry*' })
if ($geo.Count -gt 0) {
    Write-Warning ("the arms ran DIFFERENT fill geometry ({0} field(s)); a gap-level difference is not attributable to the flag under test alone." -f $geo.Count)
}

if ($CsvOut) {
    $dir = Split-Path -Parent $CsvOut
    if ($dir -and -not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    # Gap rows and run-level rows share one schema so a single CSV carries the whole comparison.
    $csv = [System.Collections.Generic.List[object]]::new()
    foreach ($r in $rows) {
        $paths = @($r.Diffs | ForEach-Object {
                $bv = $_.Base
                $cv = $_.Cand
                if ($_.Kind -eq 'added') { $bv = '<absent>' }
                if ($_.Kind -eq 'removed') { $cv = '<absent>' }
                "$($_.Path)=$bv->$cv"
            }) -join '; '
        $csv.Add([pscustomobject]@{
                pair          = $r.Pair
                scope         = 'gap'
                gap_base      = $r.BaseIdx
                gap_cand      = $r.CandIdx
                a_start_secs  = $r.Start
                class         = $r.Class
                expected      = $r.Expected
                status_base   = $r.StatusBase
                status_cand   = $r.StatusCand
                reason_base   = $r.ReasonBase
                reason_cand   = $r.ReasonCand
                changed_count = $r.Diffs.Count
                ignored_count = $r.Ignored
                near_miss_ms  = $r.NearMissMs
                changed_paths = $paths
            })
    }
    foreach ($r in $runRows) {
        $cls = 'run_level:independent'
        if ($r.Derived) { $cls = 'run_level:derived' }
        $csv.Add([pscustomobject]@{
                pair          = $r.Pair
                scope         = 'run'
                gap_base      = $null
                gap_cand      = $null
                a_start_secs  = ''
                class         = $cls
                expected      = $r.Derived
                status_base   = ''
                status_cand   = ''
                reason_base   = ''
                reason_cand   = ''
                changed_count = 1
                ignored_count = 0
                near_miss_ms  = $null
                changed_paths = "$($r.Path)=$($r.Base)->$($r.Cand)"
            })
    }
    $csv | Export-Csv -LiteralPath $CsvOut -NoTypeInformation -Encoding utf8
    Write-Host "wrote $CsvOut"
    Write-Host ''
}

Write-Host ('VERDICT  {0} changed of {1} joined' -f $changed.Count, $joined)
if ($ExpectReasons.Count -eq 0 -and $ExpectPaths.Count -eq 0) {
    Write-Host '  expected              0  (no -ExpectReasons / -ExpectPaths: the bar is bit-identical)'
}
else {
    $why = @()
    if ($ExpectReasons.Count -gt 0) { $why += ('reasons ' + ($ExpectReasons -join ',')) }
    if ($ExpectPaths.Count -gt 0) { $why += ('paths ' + ($ExpectPaths -join ',')) }
    Write-Host ('  expected          {0,5}  ({1})' -f $expected.Count, ($why -join '; '))
}
Write-Host ('  unexplained gaps  {0,5}' -f $unexplained.Count)
Write-Host ('  run-level         {0,5}  (independent of the gap list)' -f $runIndependent.Count)
Write-Host '                          bar: both zero'
Write-Host ''

if (($unexplained.Count + $runIndependent.Count) -gt 0 -and -not $NoFailOnUnexplained) { exit 1 }
exit 0
