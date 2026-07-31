# Fingerprint-dump provenance — let a corpus state what it measured, and on what (DRAFT)

Status: **open, 2026-07-31 — §2 blocks the next large fingerprint run.** Consumer: any corpus-level
analysis pass (`equivalence-calibration` and successors) that must qualify a result rather than just
report it.

Split out of [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) on 2026-07-31, where it had
accumulated as §7e plus three rows of the §7c register. It is not a recipe feature: `ScanRecipe`
answers *"what knobs produced this gap list?"* and its `PartialEq` must stay exactly as fine as "same
gap list" (that plan's §2/§7d). Everything here answers a different question on a different artifact —
the fingerprint **corpus dump** — and none of it may join recipe equality.

**Siblings:**
[TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) (scan-output provenance; the sibling axis, and
the only overlap is that both touch `gap_fingerprint/schema.rs`),
[archive/TEMP-equivalence-instrument-convergence.md](archive/TEMP-equivalence-instrument-convergence.md)
(**archived 2026-07-31**; I1/I2/I3 — the ledger that produced §2 and §3),
[archive/TEMP-equivalence-divergence-findings.md](archive/TEMP-equivalence-divergence-findings.md)
(F14/F15 — where the probe scaffolding came from),
[gap-fingerprint.md](gap-fingerprint.md) (durable home for current equivalence behaviour; the dump
schema this plan changes).

> **Verification rule for this document.** Same rule as the recipe plan, adapted: a `file:line`
> reference or a claim about current behavior belongs in §2/§3 (specification) or §5 (the checklist),
> where it is about to be executed and therefore checked. Re-verify a citation when you touch its
> paragraph. Refs below were verified against source on **2026-07-31**.

---

## 1. Why

The fingerprint dump records *verdicts and signals*. It does not record **what it measured on**
(§2) or **how it measured** (§3). Three consequences, one of them already realized:

1. **A null result cannot be read.** The instrument-convergence ledger closed I3 — the fine donor
   predicate read digital silence as *occupied* where scan read it as *silent*, the dangerous
   direction. Its corroborating corpus statistic, `0 dangerous / 297 gaps`, is **uninterpretable as
   written**: every corpus pair is lossy AAC bottoming out near −101 dB, so none of them can reach the
   −120 clamp the defect requires. A null measurement is evidence only if the corpus could have
   produced the condition — and the dump cannot say which pairs could. **This is realized, not
   hypothetical**, and it is why §2 blocks the next run.
2. **The probes that currently supply measurement provenance are scaffolding with a deletion clock.**
   `silent_core_probes` is marked *"Vestigial — remove on next touch"*
   (`domain/gap_equivalence.rs:124-131`); `noise_floor_probes` is explicitly **retained** for I2
   residual attribution (`:132-136`). When the first goes, the ability to attribute a scan↔fine delta
   goes with it unless §3 lands something permanent first.
3. **Bin-width divergence is detectable only by hand.** I1 (the 50 ms vs 100 ms overlay mismatch) was
   found by reading source, not by querying a corpus — even though the donor-side block counts exist
   for exactly that purpose. §3 closes the asymmetry.

**Evidence class**, carried over from the recipe plan's §7: **Derived** = an investigation needed the
field, or its absence forced a re-run, a bound, or a wrong path. **Speculative** = would likely have
helped, but no measured incident required it.

## 2. Source provenance (`FileSource`) — what media was measured

**The defect.** `FileSource` (`gap_fingerprint/schema.rs:20-29`) declares `container: Option<String>`
and `codec: Option<String>`, and **both are hardcoded `None`** at the only production construction
site, `file_source` (`:79-89`). Because both carry `skip_serializing_if = "Option::is_none"`, the keys
are simply absent from every dump — no `"codec"` appears anywhere in the 331-gap corpus. The fields
are declared, documented (*"(codec / bitrate / partial clip) a different one"*), and dead.

**The data already exists one call up.** `decode_ab` (`application/patch_audio/decode.rs:24-105`)
selects `track_a` / `track_b` as `AudioTrack`, which carries `codec: String` (**not** optional) and
`bit_depth: Option<BitDepth>` — whose doc comment already states *"`None` for lossy codecs (AAC, MP3,
AC-3, Opus, Vorbis)"*, i.e. the losslessness proxy is a field read, not a new measurement. It also
computes `source_audio_bitrate_{a,b}_bps`. `DecodedAb` keeps the bitrates and **drops the tracks**, so
`characterize_gaps` (`gap_fingerprint/measure.rs:2598-2604`) reaches `file_source` with nothing but
PCM.

| Field | Today | Action | Why | Class |
|-------|-------|--------|-----|-------|
| `codec` | declared, always `None` | **fill** from `AudioTrack::codec` | I3's null result cannot be qualified without it | **Derived** |
| `lossless` (or echo `bit_depth`) | absent | **add** | `bit_depth.is_none()` is the existing lossy proxy; "could this corpus produce the condition?" is a per-pair question | **Derived** |
| `b_source.sample_rate` | **wrong** — `file_source` is called with A's rate for both sides (`measure.rs:2601`), after B was resampled to A | **fix**: record B's native rate | a rate-converted B is a different measurement subject; the dump currently claims the two matched | **Derived** |
| `source_audio_bitrate_bps` | computed in `decode_ab`, dropped | **add** | separates "lossy at 640 kb/s" from "lossy at 96 kb/s" when a floor claim is in question | **Speculative** |
| `container` | declared, always `None` | fill only if free | not implicated by any finding; `AudioTrack` does not carry it | **Speculative** |

**Shape.** Thread the two `AudioTrack`s (or a small `{codec, bit_depth}` descriptor per side) onto
`DecodedAb`, give `file_source` a per-side descriptor argument, and drop the shared-`sample_rate` call
shape. **No path, title, or filename enters `FileSource`** — `id` stays the content hash, and the
licensing-safe property of the corpus is unchanged.

## 3. Measurement provenance (`GapEquivalenceVerdict`) — how it was measured

Today this is carried by **probes**, which are scaffolding (§1.2). This section is their permanent,
much smaller replacement: not a grid of candidate measurements, but a record of the one measurement
that was actually taken.

### 3a. The measurement recipe on the verdict

Four axes, matching what the probes vary: **context secs**, **bin ms**, **channel reduction**, and
**span** (`core` | `refined`). Post-I1 the two front-ends agree on bin and reduction and differ only
on context (±2.0 s scan vs ±3.0 s fine — the accepted I2 residual, 0.606 dB median). That agreement
is exactly what makes permanent fields cheap now and expensive to reconstruct later.

The **span** axis absorbs what the recipe plan's §7c listed separately as *"donor / A window identity
used for each fraction (core-mapped vs refined-nominal)"* — F15's third donor axis. It is the same
field found from a different direction, and it is decision-relevant near the 0.5 occupancy threshold
(F15's g4/g6), so it is recorded once here rather than twice.

- **Class:** **Speculative** as a permanent shape; **Derived** that *some* provenance was required —
  F15 could not attribute its floor deltas until the probes existed.
- **Sequencing:** must land before `silent_core_probes` is deleted, or the attribution capability is
  lost in the gap between.

### 3b. `a_gap_total_blocks` — close the A/donor asymmetry

**Class: Derived** (promoted from Speculative 2026-07-31).

`a_gap_silent_blocks` ships alone (`domain/gap_equivalence.rs:113-116`), documented as *"the
population behind `a_gap_rms_db` … and `gap_floor_db`"*. Two fields below it, the donor pair
(`:117-123`) carries a sharper doc: *"a fraction alone cannot distinguish `1/10` from `1.1/11`, **which
matters when comparing paths that bin the same span differently**."*

That sentence describes I1. The donor side received a bin-mismatch detector; the A side did not.
With the total, `total_blocks × bin_ms ≈ span_secs` is a one-line arithmetic check that catches a
bin-width divergence **corpus-wide**, without re-deriving anything from source — the cheapest
available detector for precisely the defect class I1 turned out to be. I1 was instead found by
reading code, which is what makes this Derived rather than Speculative.

It also completes the obvious symmetry: A gets a silent fraction that mirrors the donor's, so the two
sides of the classifier's inputs become comparable populations rather than one fraction and one count.

`with_scan_provenance` (`domain/gap_equivalence.rs:245-253`) already takes the donor pair as a tuple
and `a_gap_silent_blocks` as a bare `usize`; the total joins there.

### 3c. Declined / closed on this axis

Recorded so they are not re-proposed as gaps in this plan.

| Signal | Status | Reason |
|--------|--------|--------|
| Span-provenance arg-max (which edge block set the max floor) | **declined** | Same axis, but F15 downgraded it — the mechanism was closed offline. Would only confirm where a fully-silent residual sits |
| Full `levels.profile_db` RMS envelope in dumps | **declined** as permanent emit (`project.rs` drops it; `bin_ms: 0`) | **Derived** need — every NF cross is recomputable offline from one 50 ms envelope, and its absence forced full-pair re-dumps (a fresh decode + characterize at ~15 GB peak RSS each; the artifacts themselves are tiny — the whole 331-gap corpus is 6.2 MB). Declined anyway: thousands of floats per gap, forever, for a scaffold scheduled for deletion. Revisit only if the envelope outlives the probes |

## 4. What this is not

- **Not `ScanRecipe` members.** None of these change which gaps are detected, so none may enter recipe
  equality — see [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) §7d. `FileSource` describes the
  media; §3 describes the instrument; the recipe describes the knobs.
- **Not a corpus-run operating procedure.** What to *check* after a large run is a separate artifact;
  this plan only makes the checks answerable.
- **Not licensing-relevant.** Nothing here adds a path, filename, or title to any artifact.

## 5. Checklist

Two independent tracks — §2 and §3 share no edits and can land in either order or separately. §2 is
the one with a deadline (before the next large run).

**Track A — source provenance (§2)**

- [ ] Carry the selected tracks (or a `{codec, bit_depth, native_sample_rate}` descriptor per side)
      on `DecodedAb` (`application/patch_audio/decode.rs:12-20`); `track_a` / `track_b` are already in
      scope at `:33` / `:60`, and the bitrates are already computed at `:54` / `:81`
- [ ] Give `file_source` (`gap_fingerprint/schema.rs:79-89`) a per-side descriptor argument; fill
      `codec`, add the losslessness field, stop passing A's rate for B
- [ ] Update the two call sites in `characterize_gaps` (`gap_fingerprint/measure.rs:2600-2601`) and
      thread the descriptor from `characterize_gaps_from_decode` (`:2224+`) and
      `composition.rs:162-170`
- [ ] Fixture/test construction sites of `FileSource` (`measure.rs:3821-3836`) gain the new fields
- [ ] Confirm no golden churn: new keys are `Option` with `skip_serializing_if`, and curated corpus
      fixtures are deserialized, never byte-compared against a fresh dump
- [ ] [gap-fingerprint.md](gap-fingerprint.md): document the new `FileSource` fields, and state
      explicitly that a corpus without them cannot qualify a null result

**Track B — measurement provenance (§3)**

- [ ] Add `a_gap_total_blocks: Option<usize>` beside `a_gap_silent_blocks`
      (`domain/gap_equivalence.rs:113-116`); populate via `with_scan_provenance` (`:245-253`), which
      already carries the donor counts as a tuple. Both front-ends fill it
- [ ] Add the permanent measurement-recipe fields (context secs, bin ms, reduction, span) to
      `GapEquivalenceVerdict`, populated by both front-ends from the values they actually used
- [ ] Only then remove `silent_core_probes` + `SilentCoreProbe` + `with_silent_core_probes`
      (`domain/gap_equivalence.rs:124-131`, `:265-272`) per its vestigial note. **Keep**
      `noise_floor_probes` (`:132-136`, `:274-278`) — retained for I2 attribution
- [ ] [gap-fingerprint.md](gap-fingerprint.md) § *`equivalence` vs `scan_equivalence`*: replace the
      probe description with the permanent fields; note `total_blocks × bin_ms ≈ span` as the
      bin-divergence check

## 6. Downstream

- **The next large fingerprint run** should not start until Track A lands — a run dumped without it
  produces another corpus that cannot answer the question motivating the run (§1.1).
- **`equivalence-calibration`** can then qualify its `0 dangerous / N gaps` verdict by the population
  that could have produced the condition, instead of reporting a bare count over an all-lossy corpus.
- **Any corpus-level analysis pass** gains two cheap arithmetic checks it does not have today:
  losslessness census (§2) and bin-width agreement (§3b).
