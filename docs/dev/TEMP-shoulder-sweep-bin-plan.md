# TEMP — Promote the listen-WAV shoulder sweep into `clip-sync-repair`

**Status:** draft, not started, 2026-08-21. **Revised 2026-08-21** against the census's Results 7–8
(see §1.1) — the instrument design is unchanged; what changed is the classifier it writes, because
the phenomenon it is pointed at was renamed by measurement after this plan's first draft.
**One deliverable:** a `calibration`-gated
`shoulder-sweep` bin in `clip-sync-repair`, sibling of `listen-registration` and
`equivalence-calibration`. It replaces the gitignored Python pair
`gap-files/scratch_shoulder_sweep.py` + `scratch_shoulder_sweep_batch.py` — the independent
±2000 ms waveform instrument used for the 14-gap census, the map-coordinates fix, and the
collapsed-bridge work.

Parent: [archive/TEMP-lag-decision-map-coordinates-plan.md](archive/TEMP-lag-decision-map-coordinates-plan.md)
(prototype named in its Census section; “keep Python out of the repo”),
[TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md)
(this instrument’s current consumer). Does **not** implement the Collapsed-bridge skip verdict
([BACKLOG.md](../../BACKLOG.md) § Fill placement) — it only makes the measurement that verdict
will keep needing, and per §1.1 that verdict's spec is itself now in question.

> **Verification rule.** A `file:line` reference belongs only in a checklist item that is about
> to be executed; design sections state the decision and its reason. Re-read
> `listen_registration.rs`’s header (independence + WAV geometry) and the Python scripts in
> `gap-files/` before coding — those scripts are gitignored and may already be gone.

---

## 1. Problem

The collapsed-bridge / far-offset census is a **WAV-domain** question: given `--gap-listen`
`_a_surround` / `_b_surround`, where does a 1 s throat-adjacent template actually lock in B,
when the search is allowed to go past production’s ±600 ms `lag_decision` window?

That instrument has lived as Python in gitignored `gap-files/`. Consequences:

- The original script already vanished once and had to be reconstructed (the surviving copy’s
  own header says so). The next `gap-files/` wipe repeats that.
- `listen-registration` is the closest committed sibling and is **not** this instrument: it
  checks engine `donor_registration` on the **pre** shoulder only, ±1 s, 1 ms naive Pearson,
  no peak table, no `bridge_ms`, no census buckets.
- Production `lag_decision` (fine ±600 ms, envelope rescue ±2000 ms at 100 ms bins) is the
  thing under test on far-offset gaps. Reusing `seam_local::lag_correlation_curve_fft` or the
  envelope pass would make the check share code with the estimator it is supposed to
  independently confirm — the same constraint `listen-registration` documents.

The Python is two files only because the batch script `importlib`s the single-gap module.
That is one instrument, two entry points, not two products.

### 1.1 What Results 7–8 changed (revision, 2026-08-21)

This plan's first draft copied the Python's derived buckets verbatim, including
`collapsed_bridge` (`|bridge_ms| ≤ 50`). The census has since retired that predicate as a
*class*:

- **Result 7** — `bridge_ms = gap_ms + step`, where `step = post_lag − pre_lag`. So
  `|bridge_ms| ≈ 0` fires exactly when a pair's map step happens to equal minus that gap's
  duration (31/g7: step −1121.9 ms vs gap 1118.8 ms, 3.1 ms apart). **No positive step can ever
  satisfy it** at any gap duration, so pair 50's +2.789 s step is structurally invisible to it.
  The invariant underneath is the **step boundary**: a gap whose pre and post sit on opposite
  sides of a pair-level map step. `collapsed_bridge` is a lossy proxy for a subset of it.
- **Result 8** — the step boundary is **not measurable from `lag_decision` dumps**. Over 935
  admitted rows the largest trustworthy step in the whole corpus is 63.9 ms; both known steps
  (pair 31 ≈ −1.15 s, pair 50 ≈ +2.789 s) are censored, because a step past the search radius
  destroys the correlation that would measure it. That null is what makes this bin's WAV domain
  the only remaining instrument for the question.

Two consequences for this plan, both applied below:

1. **The classifier must name the step boundary, not the coincidence.** §4 promotes `step_ms` to
   a first-class column and makes `step_boundary` a bucket; `collapsed_bridge` survives only as
   a derived annotation on top of it (§4.3), so a boundary gap is never removed from the
   boundary population by an arithmetic accident. The first draft evaluated `collapsed_bridge`
   *before* the leftover `real_step` bin, which would have hidden 31/g7 — the corpus's only known
   boundary — from the bucket that is now the point.
2. **The known queue is two pairs, and one of them needs a wider sweep than the default.** See
   §2.1 for what this bin can and cannot answer, and §4's `--max-lag-ms` note.

---

## 2. Goal / non-goals

**Goal**

- One `[[bin]]` `shoulder-sweep` in `crates/clip-sync-repair`, `required-features = ["calibration"]`.
- Two modes, matching the two Python scripts:
  - **Single gap / one shoulder** — 1 s A template at the pre or post throat, sample-domain
    Pearson vs B at every integer-sample lag in `±max-lag-ms` (default 2000), 250 ms peak
    exclusion, stdout peak table + floor/prominence.
  - **Corpus** — walk a `--gap-listen` tree, run both shoulders, write `census.csv` /
    `census.json` with the derived buckets of §4.3 (`step_boundary`, `shared_far_offset`,
    `shared_near_false_dead`, `one_live`, `none_live`) plus the pair-level step roll-up of §4.4.
    These are the batch script's buckets with its `real_step` leftover bin promoted and renamed
    to `step_boundary`, and its `collapsed_bridge` demoted to an annotation — see §1.1.
- Independent of production correlation: FFT Pearson lives in the bin (or a bin-local module).
  `rustfft` is already a `clip-sync-repair` dependency.
- Reconstructs the **archived registration-plan** §4.3 / 31/g7 pre-shoulder landmark: peak **r ≈ 0.8287 at ≈ −38.1 ms**,
  single spike, floor < 0.10. That is the media-side reconstruction check, not a CI assertion.
- Media hygiene matches `listen-registration`: print pair directory names and gap indices
  only — never media paths or titles. Census files use `entry_stem`s.

**Non-goals**

- `scratch_patch_registration_sweep.py` (sliding NCC of `_a_patched` vs `_b_surround`). Different
  question; a later subcommand if Result-6-style traces keep being needed.
- `scratch_sliver_check.py` and the JSON-only scratch joins.
- A sixth workspace crate, or hosting this in `clip-sync-repair-harness` (`gap-fingerprint-stats`
  is dump/oracle analysis; this is WAV-domain).
- Folding into `listen-registration`. That bin’s contract is a registration *check* (pre-shoulder,
  ±1 s, vs engine lag). Mixing in a ±2 s peak table and a census would blur it.
- Folding into `--gap-fingerprints` or `--gap-listen`. This is a post-hoc bin on existing listen
  WAVs, the same pattern as `listen-registration`. Fingerprint already has a different estimator
  for the same question (`lag_decision`); putting sample-domain ±2000 ms Pearson on that dump
  would either duplicate the axis or share code with the sensor this tool exists to check.
  `listen.json` records production disposition, not a second lag sensor. The 31/g7 landmark is
  defined on listen *clips* (lag 0 = nominal map at the throat); the same math on the fingerprint
  haystack is a different instrument. `--max-lag-ms` is a knob you re-run without a decode
  (pair 50’s ~2.8 s offset; the envelope-widen ladder). A skip verdict, when it ships, reads
  production `lag_decision`, not this census.
- Changing production `lag_decision`, envelope rescue, or any skip verdict.
- Committing Python, listen WAVs, or census artifacts derived from licensed media.
- Treating census `bucket` as a production tag or a skip-verdict detector. Buckets are derived
  columns on **this** instrument’s peaks — retune the cuts in the JSON `_contract`, do not edit
  the r/lag columns. The collapsed-bridge census already refused to ship a detector off shape
  alone ([TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md)
  Non-goals).

### 2.1 What this bin can and cannot answer (scope, revised 2026-08-21)

**Can: verify the step boundary on the known queue.** Result 8's queue is exactly two pairs —
31 (boundary at g7, interior g8/g10/g11 confirmed) and 50 (interior confirmed, boundary gap not
yet identified). Listen WAVs for pair 31's and pair 50's `patch`-tier gaps already exist
(`gap-files/2026-08-20-collapsed-bridge-listen/`, 9 gaps). Run corpus mode over that tree with
`--max-lag-ms 3000` and read the §4.4 roll-up. **No media run required for this.**

**Cannot: produce a step-boundary base rate.** This bin consumes `--gap-listen` WAVs, and those
exist for roughly 38 gaps across all surviving listen trees (15 + 13 + 9 + 1) against the
corpus's **971** gaps with both anchors. `--corpus` is bounded by which listen trees happen to
exist, not by the corpus. A corpus-wide boundary census therefore needs a corpus-wide
`--gap-listen` media run first (per
[corpus-fingerprint-run-protocol], one pair at a time, ~15 GB RSS) — a cost this plan does not
authorize and the BACKLOG item has not justified. Result 8 already established that the
dump-side base rate is zero and that sizing the BACKLOG item needs this instrument; it did
**not** establish that the sizing is worth a full listen run.

Say so plainly when reporting from `--corpus`: the roll-up's denominator is *gaps with listen
WAVs*, never *gaps in the corpus*. The census note's Result 8 null is a corpus-scale statement;
nothing this bin prints is.

---

## 3. Placement (locked)

```text
crates/clip-sync-repair/
  Cargo.toml          # third calibration [[bin]], required-features = ["calibration"]
  src/bin/
    equivalence_calibration.rs
    listen_registration.rs
    shoulder_sweep.rs     # new
```

**Why here, not harness, not a new crate.** The WAV-domain independent instruments already live
as `clip-sync-repair` bins behind `calibration`. They consume `GapCorpus` + listen WAVs, which
this crate owns. `clip-sync-repair-harness` depending upward is the cycle
[archive/calibration-tooling-plan.md](archive/calibration-tooling-plan.md) avoided by putting
*JSON* stats in the harness; this tool is the other direction (WAV in, census out) and belongs
next to `listen-registration`. A sixth crate would split one `calibration` feature across three
packages for no dep-graph reason.

**Why one bin, two modes, not two bins.** The batch script is the single-gap sweep plus join +
classify. Two binaries would duplicate the FFT Pearson. Lock `--corpus <DIR>` (not a
subcommand) so the default invocation stays “two WAVs + `--shoulder`”, matching the Python
operators already type. `--corpus` and `--shoulder` are mutually exclusive: error if both are
set. Corpus always runs both shoulders.

Default `cargo build` must not grow this binary — same `required-features = ["calibration"]` as
the two siblings.

---

## 4. Instrument contract (locked)

Copied from the reconstructed Python, which itself reconstructed archive registration-plan
§4.3 (that is the *archived* §4.3; unqualified § references in this plan are to this document).
Lag 0 is the nominal map at the chosen throat, same units and sign as
`DonorRegistration::lag_ms` / `listen-registration` (`wav_lag`): **negative means B’s matching
content sits earlier than the nominal map put it.**

| Knob | Default | Meaning |
|---|---|---|
| Template | 1.0 s of A, adjacent to the throat | pre: last 1.0 s before the throat; post: first 1.0 s after it |
| Throat | recovered, not hardcoded 3.0 s | see §4.1 |
| Haystack | B surround, mono downmix (mean of all channels) | same reduction as `listen-registration`; do **not** drop LFE (that is the patched-sweep script) |
| Search | every integer-sample lag in `±max-lag-ms` | default 2000; FFT Pearson + prefix-sum variance, not 1 ms naive steps. **Default is too narrow for pair 50** — see below |
| Peak pick | 250 ms exclusion radius, top N (default 8) | floor = max r outside ±250 ms of the top peak; prominence = top − floor |
| `live` | `peak_r ≥ 0.5` and `prominence ≥ 0.10` | census derived column; findings §6.7 “confident” bar |
| `unique_strict` | `peak_r ≥ 0.8` and same prominence | recorded, not used to *define* `live` |
| **`step_ms`** | `post_lag_ms − pre_lag_ms` | **first-class column** (revision 2026-08-21). The shoulder disagreement itself — the quantity Result 7 identified as the invariant. Sign-agnostic; needs no coincidence |
| `bridge_ms` | `gap_ms + step_ms` | retained as a comparison column, *not* a classifier input. Equivalently `gap_ms + post_lag_ms − pre_lag_ms`; `gap_ms` is WAV-derived from the same `ctx` that placed the throats (§4.1), not dump `duration_secs` |

Buckets are in §4.3; they are ordered so `step_ms` decides first. Non-chained: each throat is
searched independently. Do not feed pre’s lag into post’s haystack origin — that chaining is
what the map-coordinates fix had to undo on the production sensor.

`edge_pinned` if `|peak_lag| ≥ max-lag-ms − 20`, and `clipped_haystack` / `usable_lag_ms` when B
is shorter than the requested sweep. Carry these on the JSON row so a corpus read can see a
window-edge lock without re-running.

**`--max-lag-ms` and the `live` cut interact, and the default loses pair 50.** `live` requires
`peak_r ≥ 0.5` *and* `prominence ≥ 0.10`. Pair 50's true offset is ≈ +2.789 s, so at the default
2000 ms neither shoulder's real peak is inside the sweep, both fail `live`, and the gap buckets
`none_live` — the same null Result 8 got from dumps, reached by a different route and just as
uninformative. Two guards, both required:

- Corpus mode **warns on stderr and sets `sweep_may_be_narrow` on the row** whenever a gap
  buckets `one_live`/`none_live` *and* the best peak of a dead shoulder is `edge_pinned`. That is
  the signature of a true peak outside the window, and it is the only thing distinguishing
  "measured dead" from "not searched far enough."
- The pair-50 runs in the known queue (§2.1) use `--max-lag-ms 3000`. Do not raise the *default*
  past 2000 to accommodate them — the default matches the archived census numbers this
  instrument reconstructs (§6), and a silent default change would make old and new census files
  non-comparable. Pass the flag and record it; `_contract` carries the value used.

### 4.1 Throat / context recovery

Python hardcoded `CONTEXT_SECS = 3.0` (`gap_signature_context_secs` default).
`listen-registration` already does the better thing: recover context from clip length and dump
geometry so a listen run made with a different pad still measures its own throats.

**`ctx` source (precedence).** `--context-secs` always exists and defaults to 3.0. When a
sibling dump JSON is present, **recovered geometry wins**; the flag is only the no-JSON
fallback (it does not override a dump). Sibling JSON is `stem.json` next to
`stem_a_surround.wav` (batch-script naming: strip `_a_surround.wav`, append `.json`).

```text
span = geometry.a_end_secs − geometry.a_start_secs
ctx  = (a_samples / rate − span) / 2     # from A; used for both clips
```

Without a dump, `ctx = --context-secs` (default 3.0).

**Throats follow the Python, not “same sample index.”** Listen A/B can differ in length
because `frame_range` clamps at media edges even though the export aims at the same ±
context. Recovered `ctx` still comes from A. Lag-0 origin on each clip:

```text
pre  throat: ctx samples from the start of that clip
             A template starts at a_pre − 1.0 s; B haystack lag 0 at b_pre − 1.0 s
post throat: ctx samples from the end of that clip
             A template starts at the throat; B haystack lag 0 at the throat
```

Do not place B’s post throat at A’s post sample index. When lengths match this is identical;
when they don’t, end-relative placement is the one the Python (and the listen export’s
“pad each timeline by context”) actually used.

**`gap_ms` for `bridge_ms`** uses that same `ctx`:

```text
gap_ms = (len(A) / rate − 2 · ctx) × 1000
```

Dump `geometry.duration_secs` is a comparison column on the census row (`gap_secs_dump`),
not the bridge input. That is the Python instrument: WAV-implied gap, dump duration joined
beside it.

`--gap-listen` may omit `_b_surround.wav` when the gap never reached the fill plan. Skip that
stem with a stated reason (pair + gap index); do not invent a B clip.

### 4.2 Independence from production correlation

Implement the sweep in the bin:

1. Zero-mean the template; reject silent templates (zero variance).
2. FFT cross-correlation of the zero-mean template against the B haystack (`valid` region).
3. Prefix-sum mean/variance of every length-`m` window of the haystack; Pearson
   `r = corr / (‖t‖ · σ_window)`.
4. `rustfft` is fine; do **not** call `lag_correlation_curve_fft` / `normalized_correlation` /
   `register_donor_window`.

WAV decode may be copied from `listen-registration`’s `read_mono` (hound, all sample formats
that listen actually writes). Sharing WAV I/O with that bin is allowed — WAV I/O is not the
estimator under test. Extracting a calibration-gated lib helper is optional and not required
for v1; duplicating ~40 lines is the same choice `listen-registration` already made versus
production.

Do not add a `csv` crate. `census.json` is the durable record; `census.csv` is a simple
fixed-column write of the same rows (stems + numbers, no titles, so quoting is trivial).

### 4.3 Buckets (revised 2026-08-21)

Evaluated in this order; first match wins. The ordering is the substantive change — `step_ms`
decides before any `bridge_ms` test, so no boundary gap can be diverted out of the boundary
population by the coincidence of §1.1.

| Bucket | Rule | Notes |
|---|---|---|
| `step_boundary` | both `live`, `\|step_ms\| > step_floor` (§4.4) | **the invariant.** Pre and post sit on opposite sides of a pair-level map step. Was the first draft's unnamed `real_step` leftover bin. 31/g7 lands here |
| `shared_far_offset` | both live, `\|step_ms\| ≤ step_floor`, `max(\|lag\|) > 600 ms` | both shoulders agree, both past production's window. 31/g8, g10, g11 and pair 50's interior |
| `shared_near_false_dead` | both live, `\|step_ms\| ≤ step_floor`, in reach | the 12-gap sensor-artifact bucket the map-coordinates fix cleared. Should be empty on any current-binary listen tree |
| `one_live` / `none_live` | not both live | carry `sweep_may_be_narrow` (§4) — a dead shoulder whose best peak is `edge_pinned` is unmeasured, not measured-dead |

**`collapsed_bridge` is an annotation, not a bucket.** It is written as a boolean column
alongside `step_ms` / `bridge_ms`, true when `bucket == step_boundary` and `|bridge_ms| ≤ 50`.
It records the arithmetic coincidence `step_ms ≈ −gap_ms` on a gap that is already classified as
a boundary. It must never gate, reorder, or remove a row. The `_contract` object states this in
one line so a future reader of an old `census.json` does not resurrect it as a class.

`unique_strict` (§4 table) stays recorded and still does not define `live`.

### 4.4 Pair-level step reference and `step_floor` (new, 2026-08-21)

Result 7's definition is *"pre and post disagree by ≈ **the pair's** map step"* — a per-gap
number cannot be classified without a per-pair reference. Corpus mode therefore does a second
pass after all gaps in a pair are swept:

```text
pair_step_median = median(step_ms) over gaps in this pair where both shoulders are live
pair_step_mad    = median(|step_ms − pair_step_median|) over the same gaps
step_floor       = max(6 × pair_step_mad, --step-floor-ms)     # default floor 50 ms
```

`--step-floor-ms` defaults to **50**, not to the census's 2 ms `CLEAN_STEP_MS`. Result 8's
floor sweep is the reason: `6 × MAD` collapses to the floor on most pairs (their steps are all
≈ 0), so a 2 ms floor counts millisecond seam wobble — three orders of magnitude below the
phenomenon — and there is **no plateau between 2 ms and 50 ms** to pick a defensible threshold
from. 50 ms is the smallest floor at which Result 8's dump-side yield was a clean zero, which
makes it the honest starting point for the WAV side. It is a `_contract` field: re-run with a
different value rather than editing census rows.

Per-pair roll-up columns, written to `census.json` under `pairs[]` and printed in the stdout
roll-up:

| Column | Meaning |
|---|---|
| `pair_step_median` / `pair_step_mad` | the reference above, and its spread |
| `step_floor_ms` | the floor actually applied (after the `max`) |
| `boundary_gaps` | gap indices bucketed `step_boundary` |
| `live_gaps` / `total_gaps` | the denominator, so a roll-up read from a thin listen tree is self-labelling (§2.1) |

**Two known-answer expectations**, for whoever runs this first — not CI assertions:

- **Pair 31** should show a boundary at **g7** (pre ≈ −38 ms, post ≈ −1160 ms, `step_ms` ≈
  −1122 ms) with g8/g10/g11 as `shared_far_offset` interior at ≈ −1140 to −1158 ms. `g7` should
  additionally carry `collapsed_bridge: true` (`bridge_ms` ≈ −3 ms) — that annotation appearing
  on a row already bucketed `step_boundary` is the §4.3 contract working.
- **Pair 50** at `--max-lag-ms 3000` should show its interior at ≈ **+2.789 s**. Its boundary gap
  is **not yet identified** and may not have listen WAVs; a run that finds only interior is a
  successful run with an incomplete tree, not a null.

A single pair whose only live gaps are all interior has `pair_step_mad` ≈ 0 and `step_floor` =
the flag default. That is correct — it means the pair shows one consistent step and no boundary
was sampled.

---

## 5. CLI sketch

```text
# Single gap (scratch_shoulder_sweep.py)
cargo run -p clip-sync-repair --features calibration --bin shoulder-sweep -- \
    --shoulder pre <stem>_a_surround.wav <stem>_b_surround.wav
cargo run -p clip-sync-repair --features calibration --bin shoulder-sweep -- \
    --shoulder post --max-lag-ms 2000 --peaks 8 A.wav B.wav

# Corpus (scratch_shoulder_sweep_batch.py). Errors if --shoulder is also set.
cargo run -p clip-sync-repair --features calibration --bin shoulder-sweep -- \
    --corpus gap-files/<listen-run> [--also gap-files/<extra>] [--out DIR]

# The known queue (§2.1) — pair 50's step is ~+2.789 s, past the 2000 ms default.
cargo run -p clip-sync-repair --features calibration --bin shoulder-sweep -- \
    --corpus gap-files/2026-08-20-collapsed-bridge-listen --max-lag-ms 3000
```

`--shoulder` is required in single-gap mode and forbidden with `--corpus`. Corpus always
sweeps pre and post. Corpus discovery matches the batch script and `listen-registration`’s
`collect_gaps`: numbered pair subdirectories **or** a flat directory of `*_a_surround.wav` +
sibling B + sibling `stem.json`. `--out` defaults to `<corpus>/wide-sweep/`. `--also` appends
extra listen dirs (the Python used this for a one-gap overlay of 31/g7).

Stdout for one shoulder keeps the prototype log shape (shoulder, sr, gap, template time,
r at lag 0, peak table, floor/prominence) so existing notes stay comparable. Corpus mode prints
the bucket roll-up plus one summary line per gap (pair, 1-based index, bucket, lags, r,
**`step_ms`**, `bridge_ms`), then the §4.4 per-pair block, then the `live_gaps / total_gaps`
denominator with the §2.1 caveat spelled out in words, and writes:

- `census.csv` — the joinable column set the batch script already used, plus `step_ms`,
  `step_floor_ms`, `collapsed_bridge`, `sweep_may_be_narrow`
- `census.json` — same rows + `pre_peaks` / `post_peaks` + a `pairs[]` array (§4.4) + a
  `_contract` object that quotes the instrument, `max_lag_ms` **as actually run**,
  `step_floor_ms`, the `step_ms` / `bridge_ms` formulas, every derived cut (`live`,
  `step_boundary`, …), and the one-line statement that `collapsed_bridge` is an annotation
  rather than a class (§4.3)

Join dump `lag_decision` (pre/post `frac_lag_ms`, `peak_r`, `verdict`) and `listen.json`
`disposition` when those files exist, as the batch script did — derived comparison columns,
not inputs to the sweep.

Relative dirs resolve against the **repo root**, same as `gap-fingerprint-stats`.

---

## 6. Tests (CI) vs media check (not CI)

Colocated `#[cfg(test)]` in the bin, same pattern as `listen_registration.rs`:

| Test | Pins |
|---|---|
| Known integer-sample shift of a sine recovers at r ≈ 1 | the sweep, not peak-picking |
| Negative lag means B is early | sign convention |
| Peak picker keeps two peaks 300 ms apart and drops one 100 ms apart | 250 ms exclusion |
| `classify`: both live, `\|step_ms\|` over floor → `step_boundary` | **the invariant** (§4.3) |
| `classify`: both live, lags agree, `\|lag\| > 600` → `shared_far_offset` | derived column |
| `classify`: a `step_boundary` row with `bridge_ms` ≈ 0 stays `step_boundary` **and** sets `collapsed_bridge` | §4.3 ordering — the regression this revision exists to prevent |
| `classify`: `\|bridge_ms\|` ≈ 0 never changes a row's bucket | annotation, not a class |
| `step_floor`: all-zero steps → floor is the flag default, not `6 × 0` | §4.4 `max()` |
| `step_floor`: `6 × MAD` above the flag wins | §4.4 |
| Roll-up denominator counts gaps with WAVs, not dump gaps | §2.1 self-labelling |
| `sweep_may_be_narrow` set when a dead shoulder's best peak is `edge_pinned` | §4 narrow-sweep guard |
| Context recovery: clip length = span + 2·ctx places the pre throat at `ctx` | §4.1 |
| Unequal A/B lengths: post throats are each `ctx` from that clip’s end | §4.1, not same index |
| `--corpus` + `--shoulder` is a clap error | CLI mutex |

Synthetic PCM only — `hound` temp WAVs if a file-roundtrip is easier than in-memory, never
licensed media. No new `[[test]]` binary. Tests live in the bin file (`#[cfg(test)]`), same
pattern as `listen_registration.rs`, so default `cargo test -p clip-sync-repair` (lib only,
`autotests = false`) does not grow a new integration target.

**Those tests must run on PR.** `listen-registration`’s colocated tests are *not* in
`test-tier.ps1`, so copying that pattern alone would leave the sweep untested in CI. Add

```powershell
cargo test -p clip-sync-repair --features calibration --bin shoulder-sweep
```

to the existing `pr-repair` calibration invocation (the block that already passes
`--features calibration` and the three `--test` listen/fingerprint binaries). Do not add a
new `[[test]]`. Feature-off `cargo build` still must not emit the bin.

**Media reconstruction (checklist, not CI):** `--shoulder pre` on current 31/g7 listen WAVs
must still show a unique peak near **−38.1 ms, r ≈ 0.8287, floor < 0.10**. If the listen
directory is gone, regenerate with `--gap-listen` / `measure-gap-fingerprints.ps1` rather than
relaxing the landmark. Corpus mode against a saved 14-gap listen tree should reproduce the
three-bucket split — 12 `shared_near_false_dead`, 1 `shared_far_offset` (31/g10), and 31/g7 as
**`step_boundary` carrying `collapsed_bridge: true`** (the first draft's contract would have
bucketed g7 `collapsed_bridge`; that rename is the §4.3 revision, not a measurement change, and
the r/lag numbers must be identical) — **only** if those WAVs were produced by a
pre-map-coordinates binary; on a current dump those 12 are no longer dead. Do not treat a
current-binary re-run of that tree as a regression of this tool.

**Second media checklist item (the point of the revision):** run the known queue of §2.1 and
check the two expectations of §4.4 — pair 31's boundary at g7 with far-offset interior, pair 50's
interior at ≈ +2.789 s under `--max-lag-ms 3000`. Record pair + gap index and the `step_ms`
column in the PR description. This is the first independent confirmation of Result 7's
step-boundary model on anything other than the Python that produced it.

`cargo build` (feature off) has no `shoulder-sweep`. `cargo build --features calibration`
builds it alongside the two siblings.

---

## 7. Docs (at implement, not before)

- `crates/clip-sync-repair/Cargo.toml` — `[[bin]]` comment in the same voice as
  `listen-registration` (what it measures, that it is independent of production lag).
- `docs/dev/development.md` — add the bin to the `calibration` feature row and the
  `cargo run` examples next to `listen-registration`.
- `scripts/test-tier.ps1` — `--bin shoulder-sweep` on the `pr-repair` calibration
  invocation (§6). Not a new `[[test]]`.
- `docs/dev/gap-fingerprint.md` § *`--gap-listen`* — one short pointer: the independent
  wide-window waveform instrument on these WAVs is `shoulder-sweep`; it is not
  `lag_decision` and not `listen-registration`.
- `scripts/README.md` — optional one-liner under investigation harnesses (this is a cargo
  bin, not a new `.ps1`). No measure-script wrapper unless someone is already wrapping
  `listen-registration` the same way (they are not).

Do not rewrite the collapsed-bridge census notes to pretend the Python never existed; a
parenthetical “now `shoulder-sweep`” on the next edit of those TEMP files is enough.

---

## 8. Steps

1. **Wire the bin.** `[[bin]]` + clap args for single-gap. Header comment: what it is, why
   it must not share production correlation, lag-0 = nominal map, media hygiene.
2. **Sweep + peaks.** FFT Pearson, peak table, floor/prominence, stdout log. Colocated
   shift / sign / peak-picker tests.
3. **Context recovery.** Dump geometry when present; `--context-secs` fallback. Skip stems
   with no B surround.
4. **Corpus mode.** Discover pairs, both shoulders, classify (§4.3 ordering), second pass for
   the per-pair step reference and `step_floor` (§4.4), `sweep_may_be_narrow` guard, write
   `census.csv` / `census.json` `pairs[]` + `_contract`, join dump lags + `listen.json`
   disposition when present. Classify and `step_floor` unit tests.
5. **Docs + feature-off build + PR wiring.** `development.md` / Cargo.toml comment /
   fingerprint pointer. Confirm default `cargo build` does not emit `shoulder-sweep`. Add
   `--bin shoulder-sweep` to the `pr-repair` calibration `cargo test` invocation
   (`scripts/test-tier.ps1`).
6. **Media reconstruction.** 31/g7 pre landmark, then the §2.1 known-queue run against
   `2026-08-20-collapsed-bridge-listen` at `--max-lag-ms 3000` and the §4.4 expectations (§6).
   Record the numbers in the PR description (pair + gap index only). If the listen tree is gone,
   regenerate rather than skipping.

---

## 9. Out of scope / parked

| Item | Why parked |
|---|---|
| Sliding patched-vs-B NCC (`scratch_patch_registration_sweep.py`) | Different question (does the *patch* hold one lag). Same bin later as a third mode if it earns a second user. |
| PowerShell wrapper under `scripts/measure/` | Operators already `cargo run --bin listen-registration`; do not invent a third invocation style. |
| Calling production `anchor_eligible` / JSON-only censuses | Different scripts; `scripts/measure/census-residual-measured.ps1` is the pattern. |
| Baking the sweep into `--gap-fingerprints` / `--gap-listen` | Post-hoc on listen WAVs so the window is a knob and the estimator stays off the dump schema. See §2. |
| Widening `--lag-envelope-max-lag-ms` | [TEMP-envelope-adaptive-widen-plan.md](TEMP-envelope-adaptive-widen-plan.md) owns it; this bin is an instrument that plan will re-run, not the Change. |
| Shipping a skip verdict of any shape | BACKLOG’s § Fill placement. Note its spec is currently `\|gap + post_lag − pre_lag\| ≈ 0`, which §1.1 shows is a coincidence-dependent proxy blind to every positive step. **Re-aiming that item at the step boundary is a BACKLOG edit, not this plan's work** — but do not implement the item as written on the strength of this bin's output. |
| A corpus-wide `--gap-listen` run to get a boundary base rate | §2.1. Real work, real cost, not authorized here and not yet justified by the BACKLOG item. |
