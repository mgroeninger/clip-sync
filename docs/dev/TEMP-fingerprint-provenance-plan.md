# Fingerprint-dump provenance — let a corpus state what it measured, and on what (DRAFT)

Status: **Track A implemented 2026-07-31 (emit + consume, code-complete, smoke-covered); Track B
implemented 2026-07-31 (emit + consume, unit-covered).**
§2 no longer blocks the next large fingerprint run on *code*, and the media half of the definition of
done is now covered against real containers by a synthetic lossless pair (§5, *Smoke coverage*); a dump
over licensed media remains desirable but is no longer the gate. Both tracks were reviewed and
specified before implementation (readiness review 2026-07-31; the open design decisions it surfaced are
settled inline), and Track A was re-reviewed after landing (§5, *Post-implementation review*). Track B's
readiness pass the same day settled the five open points inline (§3a empty-`a_levels`, §3b A-side
tuple, §3c hard-delete, §3d recipe Δ, §5 DoD). Consumer: any corpus-level analysis pass
(`equivalence-calibration` and successors) that must qualify a result rather than just report it.

Split out of [archive/TEMP-scan-recipe-plan.md](archive/TEMP-scan-recipe-plan.md) on 2026-07-31, where it had
accumulated as §7e plus three rows of the §7c register. It is not a recipe feature: `ScanRecipe`
answers *"what knobs produced this gap list?"* and its `PartialEq` must stay exactly as fine as "same
gap list" (that plan's §2/§7d). Everything here answers a different question on a different artifact —
the fingerprint **corpus dump** — and none of it may join recipe equality.

**Siblings:**
[archive/TEMP-scan-recipe-plan.md](archive/TEMP-scan-recipe-plan.md) (scan-output provenance — **archived**;
the sibling axis, and the only overlap is that both touch `gap_fingerprint/schema.rs`),
[archive/TEMP-equivalence-instrument-convergence.md](archive/TEMP-equivalence-instrument-convergence.md)
(**archived 2026-07-31**; I1/I2/I3 — the ledger that produced §2 and §3),
[archive/TEMP-equivalence-divergence-findings.md](archive/TEMP-equivalence-divergence-findings.md)
(F14/F15 — where the probe scaffolding came from),
[gap-fingerprint.md](gap-fingerprint.md) (durable home for current equivalence behaviour; the dump
schema this plan changes).

> **Verification rule for this document.** Same rule as the recipe plan, adapted: a `file:line`
> reference or a claim about current behavior belongs in §2/§3 (specification) or §5 (the checklist),
> where it is about to be executed and therefore checked. Re-verify a citation when you touch its
> paragraph. Refs below were verified against source on **2026-07-31**, then independently re-audited
> the same day in a readiness review: every `file:line` in §2/§3/§5 confirmed except three, corrected
> in place (the `equivalence_calibration` load sites, `composition.rs`'s crate-root path, and the
> corpus-size claim). The review's specification gaps are resolved inline — §2 *Shape* / token set /
> `is_lossy()` (subsequently **declined** — see §2), §3a *Where each front-end attaches it*, §3b
> population definition. Gap counts quoted in prose (§1.1's 297, §3c's 331) are per-run figures from
> different corpora — not a schema fact and not expected to agree.

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
2. **The probes that formerly supplied measurement provenance were scaffolding with a deletion clock.**
   `silent_core_probes` was marked *"Vestigial — remove on next touch"* and **hard-deleted with Track B**
   (replaced by the nested `measurement` recipe). `noise_floor_probes` is explicitly **retained** for I2
   residual attribution.
3. **Bin-width divergence is detectable only by hand.** I1 (the 50 ms vs 100 ms overlay mismatch) was
   found by reading source, not by querying a corpus — even though the donor-side block counts exist
   for exactly that purpose. §3 closes the asymmetry.

**Evidence class**, carried over from the recipe plan's §7: **Derived** = an investigation needed the
field, or its absence forced a re-run, a bound, or a wrong path. **Speculative** = would likely have
helped, but no measured incident required it.

## 2. Source provenance (`FileSource`) — what media was measured

**The defect.** `FileSource` (`gap_fingerprint/schema.rs:20-29`) declares `container: Option<String>`
and `codec: Option<String>`, and **both are hardcoded `None`** at the only production construction
site, `file_source` (`:86-96`). Because both carry `skip_serializing_if = "Option::is_none"`, the keys
are simply absent from every dump — **no `"codec"` appears in any JSON in the tree** (re-verified
2026-07-31: 1195 files carry `a_source`, zero carry `codec`). The fields are declared, documented
(*"(codec / bitrate / partial clip) a different one"*), and dead.

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
| `bit_depth` | absent | **add** as a string, both sides | the raw losslessness observation; `Option<BitDepth>` is `None` for lossy codecs. Stored, not interpreted — see *Observations, not verdicts* below | **Derived** |
| `native_sample_rate` | absent; `sample_rate` is **A's**, passed for both sides (`measure.rs:2601`), after B was resampled to A | **add** a new field, both sides (see the constraint below — do *not* repoint `sample_rate`) | a rate-converted B is a different measurement subject; the dump currently claims the two matched | **Derived** |
| `native_channels` | absent; `channels` is **A's** for both sides (`measure.rs:2600-2601`), and `resample_interleaved` converts rate only, so `b_samples_full` carries B's count | **add** a new field, both sides | `select_track_for_reference` matches channels *when it can* and otherwise falls back to any decodable track (`track_selection.rs:35-39`, test at `:94`), so the mismatch is reachable, not theoretical | **Derived** |
| `source_audio_bitrate_bps` | computed in `decode_ab`, dropped | **add** | separates "lossy at 640 kb/s" from "lossy at 96 kb/s" when a floor claim is in question | **Speculative** |
| `container` | declared, always `None` | **leave `None`** — closed | `AudioTrack` carries no container field, so nothing can fill it without a second probe. Not implicated by any finding. Document it as reserved-and-unpopulated so the next reader does not re-open it | **Speculative** |

**Observations, not verdicts — the corpus stores what was read, the code decides what it means.**
Every field above is a raw container/track reading. No `lossless` / `resampled` booleans enter the
JSON. The reason is §1.1 itself: `BitDepth::from_codec_params` documents `None` as *"typical for lossy
codecs"* (`bit_depth.rs:20-21`), not guaranteed, so any losslessness proxy is provisional. A frozen
bool that later proves wrong makes every dump wrong and costs a fresh ~15 GB-peak-RSS decode per pair
to correct. Freezing a derived verdict would reproduce, in a new place, exactly the "null result
cannot be re-read later" defect this plan exists to fix — and it is the same reasoning §3c already
used to decline the `profile_db` envelope.

**The losslessness question is answered by grouping on `codec`, not by a predicate.** §1.1 wants to
know which pairs could have reached the −120 clamp. A per-codec census answers that and carries
strictly more information than any `is_lossy()` bool: `aac × 297` says everything `Some(true) × 297`
says, plus *which* codec, and a future FLAC pair appears as `flac` rather than collapsing into an
"unknown" bucket that has discarded the one string a reader needs to resolve it. `codec` is already a
`GapRow` column under the consume half, and the report GROUP BYs on row columns today
(`report.rs:367-377`) — the census is a query, not a new derivation. See *Declined: `is_lossy()`*
below for why the predicate was dropped.

**One method survives: `was_resampled() -> Option<bool>`** on `FileSource` / `SourceMeta` in the
schema crate. It is pure arithmetic over two stored numbers with no vocabulary to maintain, and it has
one subtlety worth encapsulating once rather than re-deriving in two consumers: `None` when
`native_sample_rate` is absent, `Some(native != sample_rate)` otherwise. A bare `bool` would make
every pre-Track-A corpus read "not resampled" rather than "unanswerable" — the §1.1 defect reproduced
in the query layer. Both consumers depend on `clip-sync-repair` (`harness/.../report.rs:8`), so one
derivation serves both.

`bit_depth` is stored as a **string**: `BitDepth` (`bit_depth.rs:9`) derives no
`Serialize`/`Deserialize`, and a small `match` in the schema crate avoids adding serde to an upstream
`clip-sync` domain type for a diagnostic artifact. Storing both it and `codec` is two `Option`s
against a 6.2 MB corpus — cheap enough not to litigate.

**`bit_depth` is stored-for-later, and nothing reads it yet.** With `is_lossy()` declined, no in-tree
consumer touches it; it earns its place as the raw observation for the case the census cannot settle
— an unfamiliar `codec` string that still carries a sample format. Say so plainly rather than
implying a consumer exists.

**The token set is a contract, so pin it here:** `s16` | `s24` | `s32` | `f32` | `other:<bits>`, one
arm per `BitDepth` variant (`bit_depth.rs:10-17`). `other:<bits>` keeps the `u32` that
`BitDepth::Other` carries instead of collapsing every unusual depth to one token. The mapping needs a
round-trip unit test — not because the `match` is hard, but because a later rename would silently
reinterpret every corpus already on disk.

**Declined: `is_lossy()` — 2026-07-31.** An earlier draft of this section specified
`FileSource::is_lossy() -> Option<bool>` (known-lossy / known-lossless / unknown, codec table first,
`bit_depth` as tie-break). It is **dropped**; recorded here so it is not re-proposed. Four reasons,
the third decisive:

1. **It loses information relative to the field it reads.** The unknown bucket discards the codec
   string, which is exactly what a reader needs to resolve the unknown. A GROUP BY on `codec` is a
   better census and needs no derivation.
2. **It is the only part of Track A requiring a maintained vocabulary**, and it would be a *second*
   table shadowing `codec_name` (`probe.rs:253-264`) — guaranteed to drift.
3. **It was already wrong for the population that matters, before being written.** `codec_name` has
   arms for `aac` / `ac3` / `eac3` / `mp3` / `flac` / `vorbis` / `alac` and falls through to
   `format!("{codec}")` for everything else — and at the time it had **no `pcm` arm**, so a real
   PCM/WAV source, the archetype of "could reach the −120 clamp", would land in `None`, not
   `Some(false)`. (PCM has since been mapped — see §5 — but Opus and DTS still fall through.) Fixing that means either adding arms to an
   upstream `clip-sync` file or matching symphonia's `Display` impl, which is not a stable contract.
4. **Evidence class.** `codec` in the dump is **Derived** — I3 forced it. A predicate *over* `codec`
   is **Speculative**: no incident has needed one. §3c's "revisit only if" pattern applies.

**Nothing automated depended on it.** `equivalence-calibration`'s exit code gates on `dangerous > 0`
(`equivalence_calibration.rs:320`, `:356`), and §1.1's problem is interpreting a *zero* — exit 0
either way. The qualification is one line of prose for a human, and
`0 dangerous / N gaps · codecs: aac×N` is that line.

Revisit only if a corpus actually contains a lossless pair *and* a consumer needs the judgment
automated — at which point the predicate can be written against real material instead of a guess.

**Constraint: `id` and `duration_secs` are not repointable — add, never redirect.** `file_source`
derives *three* things from the one `sample_rate` argument: the `id` digest (rate is fed into the FNV
hash, `schema.rs:73-76`), `duration_secs` (`samples.len()/ch/rate`, `:94`), and the field itself. Both
of the first two correctly describe the **decoded** PCM the corpus was measured on, and `id` is load-
bearing well beyond identity: `entry_filename` (`measure.rs:2645-2655`) builds every per-gap filename
from `a_source.id[..8]` + `b_source.id[..4]`, so repointing the digest renames the entire on-disk
corpus and breaks the pair dedup in `analysis.rs`. B's native rate and channels therefore land in
**new** `Option` fields beside the existing ones; `sample_rate`, `channels`, `duration_secs`, and `id`
keep describing the decoded PCM exactly as they do today.

**Shape — two named structs, one new parameter.**

```rust
pub struct SourceDescriptor {                     // per side
    codec: String, bit_depth: Option<BitDepth>,
    native_sample_rate: u32, native_channels: u16, bitrate_bps: Option<u32>,
}
pub struct AbSources { a: SourceDescriptor, b: SourceDescriptor }          // one field on DecodedAb
```

**`pub`, not `pub(crate)`** (corrected at implementation — the draft said `pub(crate)`). Both types
appear in the signature of `pub fn characterize_gaps_from_decode`, and `clip-sync-repair-fixtures`
passes the `None` from another crate, so the type has to be nameable outside. `pub(crate)` does not
compile; re-exported from `application::patch_audio` and `application`.

`file_source` takes one descriptor; `characterize_gaps_from_decode` / `characterize_gaps` take
`sources: Option<&AbSources>`. **One parameter, not four** — `characterize_gaps_from_decode` is
already at seven positional arguments, and four more (two of them `Option`) would be a call site no
one can read. It must be `Option`-shaped: two of the three `characterize_gaps_from_decode` callers
are media-free and have no `AudioTrack` to supply (§5). Capture B's native rate/channels **before**
`decode.rs:96`, where `b_pcm_full.samples` is moved.

**Do not fold the existing bitrates out of `DecodedAb`.** `source_audio_bitrate_{a,b}_bps` are not
diagnostic-only: they flow into `PatchAudioRequest` (`patch_audio/request.rs:16-18`) and out through
the encode path's summary (`repair_videos.rs:182-189`). Moving them would ripple a production output
for a diagnostic feature. Let `SourceDescriptor.bitrate_bps` read the *same locals* already computed
at `decode.rs:54` / `:81` and leave the two existing fields alone — the duplication is deliberate and
worth a comment, because it keeps Track A's blast radius inside the fingerprint path.

**No path, title, or filename enters `FileSource`** — `id` stays the content hash, and the
licensing-safe property of the corpus is unchanged.

**Measurement refuse (shipped 2026-07-31, not part of Track A).** When A and B `native_channels`
disagree, `characterize_gaps*` sets `SourceMeta.incomparable = channel_layout_mismatch` and emits no
gaps — it does not index `b_samples_full` at A's count. Track A only **records** the counts; the
refuse gate is the separate correctness fix.

## 3. Measurement provenance (`GapEquivalenceVerdict`) — how it was measured

Today this is carried by **probes**, which are scaffolding (§1.2). This section is their permanent,
much smaller replacement: not a grid of candidate measurements, but a record of the one measurement
that was actually taken.

### 3a. The measurement recipe on the verdict

**Primary consumer:** `equivalence-calibration`, which already diffs `scan_equivalence` vs
`equivalence` per gap and prints signal Δ (`nf` / `aRMS` / `ds`) but cannot attribute a residual to
an instrument difference without reading probe grids or source. The harness
(`gap-fingerprint-stats`) does **not** project equivalence today — Track B consume lives in
calibration first; GapRow columns for these fields are out of scope until a report section wants
them.

**Shape: one nested `measurement` object on each `GapEquivalenceVerdict`.** Not flat fields on the
verdict, and not on `SourceMeta` / `ScanRecipe` — the two front-ends disagree on axes (I2 context;
donor window), so the recipe is per-verdict. Nesting mirrors `NoiseFloorProbe`'s field names and
makes "the one recipe that classified" visually distinct from the probe **grid** retained for I2.
`Option` + `skip_serializing_if` so old corpora still parse.

| Field | Type | Scan today | Diagnostic today | Why |
|-------|------|------------|------------------|-----|
| `context_secs` | `f64` | `EQUIVALENCE_CONTEXT_SECS` = 2.0 | `gap_signature_context_secs` = 3.0 | **I2** — the one accepted noise-floor residual |
| `bin_ms` | `u64` | `scan_block_ms` | `scan_block_ms` (post-I1) | bin-divergence check with §3b; was the I1 defect |
| `reduction` | `ChannelReduction` | `interleaved` | `interleaved` (post-F15) | reuse the existing enum; frozen so a future drift is visible |
| `a_span` | `SpanKind` | block centres in the raw/core gap (`core`) | same (post-F15) | A-side window; both agree today — record so a future split is audible |
| `donor_span` | `SpanKind` | offset-mapped **core** | nominal `b_mapped` | the remaining donor-window residual (~1 block); decision-relevant near the 0.5 occupancy threshold (F15 g4/g6) |

**One `SpanKind { Core, Nominal }` serves both fields**, even though `a_span` only ever emits `Core`
today. A single-variant A-side enum would have to be widened the first time A splits — exactly the
event the field exists to make visible — and two separate enums would print asymmetric tokens for the
same concept in a calibration diff that sets them side by side.

**Not `span: core | refined`.** Post-F15 both front-ends measure A on the block-confirmed **core** /
raw gap; the live residual is donor **core vs nominal**, not refined. A single `refined` token would
mis-attribute `ds` deltas. The older recipe-plan wording ("core-mapped vs refined-nominal") is
superseded here.

Example (abbreviated):

```json
"scan_equivalence": {
  "class": "shared_silence",
  "a_gap_silent_blocks": 8,
  "a_gap_total_blocks": 10,
  "donor_silent_blocks": 0,
  "donor_total_blocks": 10,
  "measurement": {
    "context_secs": 2.0,
    "bin_ms": 100,
    "reduction": "interleaved",
    "a_span": "core",
    "donor_span": "core"
  },
  "noise_floor_probes": [ "…I2 grid, retained…" ]
},
"equivalence": {
  "measurement": {
    "context_secs": 3.0,
    "bin_ms": 100,
    "reduction": "interleaved",
    "a_span": "core",
    "donor_span": "nominal"
  }
}
```

Population counts (`a_gap_*_blocks`, `donor_*_blocks`) stay **flat** beside the signals — that is
already the audit pattern; the bin check is `a_gap_total_blocks × measurement.bin_ms ≈ span_secs`.
`noise_floor_probes` stays; it is a candidate **grid**, not the live recipe.

- **Class:** **Speculative** as a permanent shape; **Derived** that *some* provenance was required —
  F15 could not attribute its floor deltas until the probes existed.
- **Sequencing:** landed together with the hard-delete of `silent_core_probes` (same change).

**Where each front-end attaches it — neither one can build it where the verdict is built.** Two of
the five fields are out of scope at the construction sites, and the plan must say so or the
implementer discovers it mid-edit:

- **Fine path — attach at the caller, change no signature.** `measure_gap_equivalence`
  never sees `3.0`: `noise_floor_db` arrives already computed from the caller's own context window,
  and `SilentCoreConfig` carries `bin_frames` with no sample rate, so `bin_ms` is not derivable
  inside either. Both values *are* in scope at the call site in `measure.rs` (locals
  `equiv_bin_ms` / `cfg.gap_signature_context_secs` / `ChannelReduction::Interleaved`). So
  `with_measurement(…)` is populated there. Threading `context_secs` into
  `measure_gap_equivalence` would put a value the function never uses into its signature purely to
  echo it back out.
- **Scan path — derive `bin_ms` from the blocks, do not plumb the recipe.** The scan front-end
  (`domain/gap_equivalence.rs:392-452`) has `EQUIVALENCE_CONTEXT_SECS` in scope (`:413`) but no
  block-ms parameter and no `ScanRecipe`. `BlockLevel` carries `start_secs` / `end_secs`
  (`domain/policies/silence.rs:25-27`), so `(end − start) × 1000` off a block gives the bin
  **actually measured**. That is the better number for a provenance field regardless of plumbing
  cost: echoing the configured `scan_block_ms` knob would report the intent, and I1 was a case of
  intent and measurement disagreeing. Build the measurement at `:450-451`, beside
  `with_scan_provenance`.

**`bin_ms` is a property of the level stream, not of the gap population.** Derive it from **any**
block in `a_levels` (`a_levels.first()` is enough) — the width does not depend on whether the gap
contains centres. When `a_levels` is **empty**, leave `measurement: None` entirely; do not invent a
bin and do not widen `bin_ms` to `Option`. Once the object exists, every field is required.

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

**`with_scan_provenance` takes A as a tuple, matching the donor.** Widen the bare
`a_gap_silent_blocks: usize` to `a_gap_blocks: (usize, usize)` — `(silent, total)` — so the
population pair is obvious in the signature and the A/donor asymmetry is not re-encoded as "one
side bare, one side paired":

```rust
pub fn with_scan_provenance(
    mut self,
    gap_floor_db: Option<f64>,
    a_gap_blocks: (usize, usize),              // (silent, total)
    donor_blocks: Option<(usize, usize)>,
) -> Self
```

Two production call sites (`derive_gap_equivalence`, `measure_gap_equivalence`); a bare extra
`usize` is how the asymmetry got here.

**Define the population once, or the check is worthless.** `a_gap_total_blocks` = *blocks whose
centre falls inside the gap, silent or not*. The two front-ends must not pick different denominators,
since `total × bin_ms ≈ span` is precisely a cross-path comparison. Cost differs by side:

- **Fine path:** free — it already computes the total and discards it as `_total`
  (`application/gap_equivalence.rs:388`).
- **Scan path: a new counter, not a field rename.** `gap_silent_blocks()`
  (`domain/gap_equivalence.rs:401-407`) filters on `b.silent && centre ∈ gap`; the total is the same
  closure without the `silent` term. One line, but the checklist should not imply the number already
  exists.

**Also fill donor counts on the diagnostic verdict.** Fine `measure_gap_equivalence` currently
passes `None` for the donor pair into `with_scan_provenance` (`:399-403`), so
`donor_silent_blocks` / `donor_total_blocks` exist only on `scan_equivalence`. The bin- / occupancy-
population checks should work on both sides of a calibration diff.

**Add a counts helper; do not change the fraction's return type.** Widening
`donor_silence_fraction_at_floor` to a tuple touches four test call sites
(`application/gap_equivalence.rs:548`, `:554`, `:577`, `:586`) besides the one production caller at
`:398`. Instead add `donor_silence_counts_at_floor -> Option<(usize, usize)>` and derive the fraction
from it — zero test churn, and it matches how the scan path already tallies `donor_blocks` and then
divides (`:430-448`).

### 3c. Declined / closed on this axis

Recorded so they are not re-proposed as gaps in this plan.

| Signal | Status | Reason |
|--------|--------|--------|
| Span-provenance arg-max (which edge block set the max floor) | **declined** | Same axis, but F15 downgraded it — the mechanism was closed offline. Would only confirm where a fully-silent residual sits |
| Full `levels.profile_db` RMS envelope in dumps | **declined** as permanent emit (`project.rs` drops it; `bin_ms: 0`) | **Derived** need — every NF cross is recomputable offline from one 50 ms envelope, and its absence forced full-pair re-dumps (a fresh decode + characterize at ~15 GB peak RSS each; the artifacts themselves are tiny — the whole 331-gap corpus is 6.2 MB). Declined anyway: thousands of floats per gap, forever, for a scaffold scheduled for deletion. Revisit only if the envelope outlives the probes |
| Soft-retire `silent_core_probes` (empty `Vec` forever) | **declined** — **hard-delete** | This track *is* the vestigial note's "next touch". Delete the field, `SilentCoreProbe`, builder, emit, and probe-only unit tests once `measurement` lands. **Do not rewrite committed fixtures** (`band_donor.json` etc.): serde ignores unknown keys by default, and the divergence tests never read the probes. Dead JSON keys may linger until the next harvest; that is fine. Update [gap-fingerprint.md](gap-fingerprint.md) to say so so soft-retire is not re-proposed |

### 3d. Settled — consume shape (2026-07-31 readiness)

| Item | Disposition |
|------|-------------|
| Recipe Δ print format | **Only diffs, mirror `signal_deltas`.** When both sides carry `measurement`, print fields that differ — in practice `context_secs` and `donor_span` (the live residuals); `bin_ms` / `reduction` / `a_span` only if they actually diverge. Shape like `ctx +1.0  donor core→nominal` beside the existing `nf` / `aRMS` / `ds` line. Never a five-field wall on every agree row |
| Optional I1 bin-check flag | **Optional, out of DoD.** `total_blocks × bin_ms ≈ span` stays a documented check; automating it as a calibration warn is elective |

## 4. What this is not

- **Not `ScanRecipe` members.** None of these change which gaps are detected, so none may enter recipe
  equality — see [archive/TEMP-scan-recipe-plan.md](archive/TEMP-scan-recipe-plan.md) §7d. `FileSource` describes the
  media; §3 describes the instrument; the recipe describes the knobs.
- **Not a corpus-run operating procedure.** What to *check* after a large run is a separate artifact;
  this plan only makes the checks answerable.
- **Not licensing-relevant.** Nothing here adds a path, filename, or title to any artifact.

## 5. Checklist

Two independent tracks — §2 and §3 share no edits and can land in either order or separately. Within
Track A the two halves have **different deadlines**: *emit* must precede the next large run (the
corpus is the perishable artifact), *consume* need not. Track B has no deadline.

**Definition of done (Track A).** Emit: a fresh single-pair dump shows `codec`, `bit_depth`,
`native_sample_rate`, and `native_channels` on **both** sides. The *resample distinction* is proven by
a unit test — build an `AbSources` whose B rate differs from A's and assert
`was_resampled() == Some(true)`, plus `None` on a descriptor-less `FileSource`. Do **not** phrase the
media check as "`native_sample_rate` distinguishable from `sample_rate`": that passes vacuously
whenever the chosen pair's B already matches A's rate, which is the common case. The media run proves
presence; the unit test proves the wiring. Consume: a roll-up over the curated fixtures reports a
**per-codec census** — counts grouped by the stored `codec` string, with codecs outside `codec_name`'s
known set appearing under their own probe string rather than folded into an "unknown" bucket, and
pre-Track-A rows counted explicitly as *absent* rather than silently zero.

**Track A — source provenance (§2)** — *implemented 2026-07-31.* Every box below is code-complete and
covered by unit tests. The wiring is proven media-free by `schema.rs`'s `descriptor_populates_provenance`
/ `was_resampled_*` / `bit_depth_tokens_are_pinned` tests, and the *threading* by `measure.rs`'s
from-decode test (asserts the two sides stay distinguishable and that a descriptor-less call leaves
provenance absent, not defaulted).

*Smoke coverage (added 2026-07-31).* The DoD's media half — "a fresh single-pair dump shows `codec` /
`bit_depth` / `native_sample_rate` / `native_channels` on both sides" — is covered by
`tests/cli_gap_fingerprint_provenance.rs`, which runs the **real binary** (align → scan →
`dump_gap_fingerprints` → `corpus.json`) over
`clip_sync_repair_fixtures::lossless_silence_pair`. Every unit test injects the `SourceDescriptor` it
then asserts on; this one reads it off an actual container probe. The pair is deliberately asymmetric —
A 48 kHz/s16, B 44.1 kHz/s24, both mono — so it pins per-side `bit_depth` and `native_sample_rate`, and
pins `sample_rate` as A's on *both* sides, which is what makes `was_resampled()` read false for A and
true for B. It is PCM WAV because of the codecs this tree decodes (`symphonia` = `default, isomp4, aac,
mp3, mkv, flac`) only FLAC and PCM reproduce zeros exactly, and PCM needs no encoder and so never skips.

The same fixture is the only thing in the tree that can reach the **−120 digital-silence condition** of
§1.1: its gap core is bit-exact zero on both sides (zeros survive the 44.1 → 48 kHz resample — every
filter tap is multiplied by zero, and the core is far longer than any filter tail), so `block_rms_db`
clamps to `BLOCK_LEVEL_FLOOR_DB` and `gap_floor_db` is −120 too. Lossy material structurally cannot:
AAC/MP3/Vorbis reconstruct silence through quantization and the IMDCT and bottom out near −101 dB.

*Two limits, both deliberate.* (a) It is not a substitute for a licensed-media dump — it proves the
probe→`FileSource` wiring against a genuine container, not that the reading is sensible across the codec
variety real media carries. (b) Both sides report the same codec, so the fixture cannot
exercise a mixed-codec pair; codec-name mappings are pinned by `codec_name`'s own unit tests instead.

*Resolved while writing the fixture (2026-07-31): PCM now has arms.* Writing (b) surfaced that PCM fell
through `codec_name` to `format!("{codec}")` and rendered as a bare **hex id** (`0x108` for s16, `0x104`
for s24) — specified behavior for an unmapped codec, but a census over PCM read `0x108→0x104`, which no
reader can interpret. `probe.rs` now maps them, as **family** arms rather than per-id tokens:

- All **36 linear PCM ids** (signed/unsigned int and IEEE float, both endiannesses, interleaved and
  planar) collapse to one `"pcm"`. The id encodes depth, signedness, endianness and planarity together;
  depth is already `bit_depth`'s axis, so per-id tokens would restate it while splitting one logical
  population across dozens of census buckets. They are listed as 36 named constants, not matched as the
  `0x100..=0x123` numeric range — a renumbering would silently mis-bucket, whereas a removed or renamed
  constant is a compile error.
- **`alaw` and `mulaw` are separate arms, deliberately not folded into `pcm`.** Symphonia names them
  `CODEC_ID_PCM_*` and numbers them immediately after the linear block (0x124, 0x125), but G.711
  companding is *lossy* — folding them in would let a census assert losslessness about lossy material,
  which is the one question the census exists to answer.

Blast radius checked before the change: `"pcm"` was *already* the token three call sites expect
(`align_videos.rs:2457,2466`, `locate_query_spike.rs:76`, `media_scan.rs:173`), and all behavioral
branching goes through the typed `AudioCodecId`, never the string.

*Post-implementation review corrections (same day), all applied:* `DecodedAb.sources` is
`#[cfg_attr(not(feature = "calibration"), allow(dead_code))]` — `calibration` is off by default and
`dump_gap_fingerprints` is its only reader, so a default build warned; the harness deserializes the
**emit-side `FileSource` itself** rather than a forked projection, so `was_resampled()`'s
`None`-vs-`false` semantics cannot drift between producer and consumer; `GapRow` carries
`a_/b_native_channels` alongside the rate columns; and the census landed in **two** places — a pair-level
`codecs (a→b)` line in `equivalence_calibration`'s roll-up, and `CorpusReport::codec_census_text()` in
the harness report, which is the one that runs over the curated fixtures the DoD names.

*Rejected from that review:* tightening `SourceDescriptor` / `AbSources` to `pub(crate)`. Not reachable —
`characterize_gaps_from_decode` is `pub` and `clip-sync-repair-fixtures` passes the `None` cross-crate,
so the type must be nameable outside the crate. See §2 *Shape*, which the draft got wrong.

*Settled by that review, recorded so they are not re-proposed:*

| Item | Disposition |
|------|-------------|
| Census shape | **`a→b` pairs are primary**, per-side counts on a second line. A fingerprint is a *pair* measurement, so `flac→aac` and `aac→aac` are different questions and a single-codec count cannot express the distinction. `(absent)` is its own bucket in both, never folded into a zero |
| Resampling in the census | **Its own line**, not folded into the codec line. A rate conversion changes the measured waveform independently of the codec; combining them would hide one axis behind the other |
| Row-level "no provenance" flag on `GapRow`, mirroring `registration_from_legacy_lag` | **Deferred.** The `check.rs` health Warn plus the census's `(absent)` bucket already make an unanswerable corpus say so. A row-level bool earns its place only once a report *filters* on it, which nothing does |
| `bit_depth` round-trip **parser** (string → `BitDepth`) | **Deferred.** The forward pin (`bit_depth_tokens_are_pinned`) is what protects corpora already on disk; a parser is dead code until a consumer reads the token, and none does (§2, *stored-for-later*) |
| Channel-mismatch **measurement** correctness | **Out of scope for provenance, and separately shipped** — Track A records `native_channels`; the refuse gate is §2's *Measurement refuse* |

*Emit* — makes the data exist:

- [x] Carry one `AbSources { a, b }` field of `SourceDescriptor { codec, bit_depth,
      native_sample_rate, native_channels, bitrate_bps }` on `DecodedAb`
      (`application/patch_audio/decode.rs:12-20`); `track_a` / `track_b` are already in scope at
      `:33` / `:60`, and `bitrate_bps` reads the same locals already computed at `:54` / `:81`
      (**leave `source_audio_bitrate_{a,b}_bps` in place** — §2's shape note: they feed
      `PatchAudioRequest` and the encode summary). Capture B's native rate/channels before `:96`
      moves `b_pcm_full.samples`
- [x] Update the **exhaustive** `DecodedAb` destructure in `patch_audio/mod.rs:148-154` — new fields
      break it (the repair path ignores them; it just has to name them or use `..`)
- [x] Give `file_source` (`gap_fingerprint/schema.rs:86-96`) an `Option<&SourceDescriptor>` argument;
      fill `codec` and `bit_depth` (string, tokens pinned in §2: `s16` | `s24` | `s32` | `f32` |
      `other:<bits>`, with a round-trip unit test), add native rate/channels on **both** sides,
      **beside** the existing fields (§2's constraint: `id` / `duration_secs` / `entry_filename` keep
      reading the decoded PCM). Leave `container: None` and document it as reserved — `AudioTrack`
      cannot fill it. No `lossless` / `resampled` booleans — those are methods, next item
- [x] Add `FileSource::was_resampled() -> Option<bool>` (or the `SourceMeta`-level equivalent) in the
      schema crate — `None` when `native_sample_rate` is absent, `Some(native != sample_rate)`
      otherwise. **No `is_lossy()`**: declined in §2 (*Declined: `is_lossy()`*); the losslessness
      question is a GROUP BY on the stored `codec` string, not a predicate
- [x] Fix the **existing** field semantics while touching them: `FileSource`'s doc comment
      (`schema.rs:16-18`) says nothing about what `sample_rate` / `channels` / `duration_secs` describe.
      State that they are the **decoded/analysis** values (A's, for both sides) and that the `native_*`
      fields are the per-side source readings — otherwise the misleading value stays undocumented next
      to the correct one
- [x] Update the two call sites in `characterize_gaps` (`gap_fingerprint/measure.rs:2600-2601`) and
      thread `sources: Option<&AbSources>` — **one** new parameter, not four — through
      `characterize_gaps_from_decode` (`:2224+`, already at seven positional args) and
      `src/composition.rs:155-163` (note: the crate root, *not* a `gap_fingerprint/composition.rs`;
      that module is `measure` / `mod` / `project` / `schema`)
- [x] Keep the parameter optional for the two media-free `characterize_gaps_from_decode` callers that
      have no `AudioTrack`: `clip-sync-repair-fixtures/src/fingerprint_corpus_fixtures.rs:46` (synthetic
      A/B) and `measure.rs:3708-3709`. `src/composition.rs:155` is the only site that can supply one —
      it holds `decoded` from `decode_ab` (`:128`), so the descriptor rides in on `DecodedAb`
- [x] Fixture/test construction sites of `FileSource` (`measure.rs:3820`, `:3828` — the only two in the
      tree) gain the new fields
- [x] Confirm no golden churn: new keys are `Option` with `skip_serializing_if`, and **no golden
      captures the source block at all** — every file containing `a_source` is a fixture *input*
      (`tests/gap_corpus/fingerprints/**`), deserialized, and `curated_fixture_backfill.rs` rewrites
      fixtures only under `CURATED_FIXTURE_BACKFILL=1`

*Consume* — makes the data answer §6. Without these, Track A ships a corpus nothing reads:

The consumer model is a **flat, denormalized row table**: `GapRow`
(`harness/gap_fingerprint_corpus/schema.rs:124-169`) already copies `pair` / `a_id` / `b_id` onto every
gap row, and the report filters rows and GROUP BYs on them (`report.rs:367-377`). Nothing navigates a
nested source object at query time. So a source fact is only answerable once it reaches a **row
column** — deserializing it is not the deliverable.

- [x] `equivalence_calibration.rs` already deserializes the whole `GapCorpus` (`:310` roll-up, `:377`
      single-corpus; the generic loader is `:395-398`) but never touches `source` — the string
      `a_source` does not appear in the file. Print a **codec census** beside the `dangerous` count
      (`… · codecs: aac×N`) so a null result states the population it was measured over. No
      predicate, no exit-code change — the gate is `dangerous > 0` (`:320`, `:356`) and §1.1's
      problem is interpreting a zero
- [x] Harness roll-up: add the fields to the **deliberately minimal** private projection
      (`analysis.rs:18-29`, documented at `:1-4` as existing only to feed `gap_row`) **and** surface
      them as `GapRow` columns — `a_codec` / `b_codec` as plain `Option<String>`, alongside the
      native-rate/channel columns. Extending the deserializer alone changes nothing; the column is
      what the report can GROUP BY, and the census in the DoD *is* that GROUP BY
- [x] Add a `check.rs` health warning for a corpus carrying **no** source provenance, following
      `registration_from_legacy_lag` (`schema.rs:166-169`) — an existing row-level flag whose whole job
      is "this corpus mixes schema generations, the reads aren't comparable". That pattern is the
      §1.1 fix: an unanswerable corpus should **say so on the row**, not leave an absent key to be
      inferred
- [x] **Resolved — leave `manifest.json` alone.** Both current consumers open `corpus.json` anyway
      (`equivalence_calibration` roll-up; harness `analyze_dirs`). Codec-on-manifest is a future
      convenience, not a Track A deliverable
- [x] [gap-fingerprint.md](gap-fingerprint.md): document the new `FileSource` fields, and state
      explicitly that a corpus without them cannot qualify a null result

**Definition of done (Track B).** Emit: a dump shows `measurement` (all five fields) and
`a_gap_total_blocks` on **both** `scan_equivalence` and `equivalence`, donor population counts on
both, and **no** `silent_core_probes` on new dumps. Prove the attach points with unit tests — scan:
`bin_ms` from any `BlockLevel` width, `measurement: None` when `a_levels` is empty; fine:
`with_measurement` at the caller with no `measure_gap_equivalence` signature change; counts: fine
path keeps `_total`, scan path adds the centre-in-gap counter, donor helper feeds
`with_scan_provenance`. Consume: diverge rows print recipe Δ (§3d — diffs only);
[gap-fingerprint.md](gap-fingerprint.md) replaces the `silent_core_probes` section with the permanent
`measurement` fields and notes `total_blocks × bin_ms ≈ span` as the bin-divergence check. The
optional I1 calibration flag is **out of DoD**.

**Track B — measurement provenance (§3)** — *implemented 2026-07-31.*

*Settled 2026-07-31 (readiness), recorded in §3a/§3b/§3c/§3d so they are not re-opened:* A-side
`with_scan_provenance` is a `(silent, total)` tuple; empty `a_levels` ⇒ no `measurement`; hard-delete
`silent_core_probes` (fixtures may keep dead keys); recipe Δ prints diffs only; optional I1 flag out
of DoD.

*Emit:*

- [x] Add `a_gap_total_blocks: Option<usize>` beside `a_gap_silent_blocks`; widen
      `with_scan_provenance` so A is `a_gap_blocks: (usize, usize)` matching the donor pair (§3b).
      Fine path keeps `_total`; scan path adds the centre-in-gap counter
- [x] Fine path: `donor_silence_counts_at_floor` + pass counts through `with_scan_provenance`
- [x] `measurement: Option<EquivalenceMeasurement>` + `SpanKind { Core, Nominal }`; attach at
      `measure.rs` caller (fine) and `derive_gap_equivalence` (scan; `bin_ms` from any
      `BlockLevel`; omit when `a_levels` empty)
- [x] Hard-delete `silent_core_probes` / `SilentCoreProbe` / emit / probe-only tests. **Keep**
      `noise_floor_probes`. Fixtures not rewritten

*Consume* (calibration owns this; harness GapRow projection of equivalence is out of scope):

- [x] `equivalence_calibration` diverge rows: recipe Δ beside signal Δ — **diffs only** (§3d)
- [ ] Optional (out of DoD): flag gaps where `a_gap_total_blocks × measurement.bin_ms` disagrees
      with geometry span (the I1-class check)
- [x] [gap-fingerprint.md](gap-fingerprint.md): `measurement` section replaces `silent_core_probes`;
      hard-delete + lingering fixture keys noted; `total × bin_ms ≈ span` documented

## 6. Downstream

- **The next large fingerprint run** is unblocked (Track A emit landed 2026-07-31; the media half of
  §5's DoD is covered by `cli_gap_fingerprint_provenance`). A run dumped from a build without Track A
  produces another corpus that cannot answer the question motivating the run (§1.1) — confirm the build
  carries it before committing a multi-hour run.
- **Scope of that run is a separate decision, and Track A does not settle it.** Track A changes what the
  run can *report*, not what it can *find*. Re-fingerprinting the existing pairs yields a census reading
  one codec pair and a `0 dangerous` correctly labelled as a null the population could not have broken —
  real progress over a bare count, but still not evidence. Making the null *informative* needs at least
  one pair whose source can reach the −120 clamp, i.e. lossless. That is media acquisition, not code, and
  it is worth establishing whether such a pair exists **before** committing to the run rather than after.
- **`equivalence-calibration`** now qualifies its `0 dangerous / N gaps` verdict by naming the
  population it was measured over (`codecs (a→b): flac→aac 12`), instead of reporting a bare count over
  a corpus whose composition the reader has to already know.
- **Any corpus-level analysis pass** gains two cheap checks: the per-codec census (§2 — **shipped**) and
  bin-width agreement via `a_gap_total_blocks × measurement.bin_ms ≈ span` (§3b — **shipped** on emit;
  optional calibration warn still elective).
- **`equivalence-calibration`** diverge rows now print recipe Δ (`ctx +1.0  donor core→nominal`) beside
  signal Δ, so the two accepted residuals are attributable without reading probe grids or source.

**Emit ≠ delivered — resolved for Track A and Track B.** Track A: harness + calibration census.
Track B: calibration recipe Δ. The principle stands for any future axis: emitting a field is not
delivering it; if a deadline forces a split, ship emit first and record consume as outstanding.
