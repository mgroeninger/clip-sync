# TEMP — Promote the listen-WAV shoulder sweep into `clip-sync-repair`

**Status:** **IMPLEMENTED 2026-08-21** — `crates/clip-sync-repair/src/bin/shoulder_sweep.rs`
(`[[bin]] shoulder-sweep`, `required-features = ["calibration"]`), 25 colocated tests wired into
`pr-repair` via `--bin shoulder-sweep`. Media acceptance passed on the 31/g7 pre landmark
(r 0.8287 at -38.1 ms, floor 0.0979), and both scratch Python scripts were deleted with it. This
document is now the bin's specification, not a work item. Drafted 2026-08-21. **Revised four times on 2026-08-21.** (1) against the
census's Results 7–8, which renamed the phenomenon the instrument points at; (2) against a review of
that revision, which found the known-queue run could not produce its own stated expectations; (3)
**cut to a single objective** — see the box below. Revisions (1) and (2) added a census classifier;
revision (3) removes it, because the correction recorded in
[TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md) established
that no data on disk can exercise it. (4) **implementability pass against the repo and the
scripts** — no scope change: it closed six specification gaps that would have been unrecoverable
once §4.5.5 deletes the Python (the FFT transform details in §4.2, the null floor and the
clipped-haystack `edge_pinned` rule in §4, `peak_z` and column parity in §4.3, and the pair-label
chain, dump-index numbering, and `--also` dedupe in §5). One of those gaps was a fallback to a
dump "pair field" that does not exist in the schema. (5) **2026-08-22, post-implementation:** §5's `--pair` was tightened from "overrides both" to "fills in for `.`-labelled rows only, and errors on a tree of pair subdirectories" — an operator-supplied label replacing a discovered one had no precedent in any sibling calibration bin. The bin matches.

**One deliverable:** a `calibration`-gated `shoulder-sweep` bin in `clip-sync-repair`, sibling of
`listen-registration` and `equivalence-calibration`. It is the independent ±2000 ms waveform
instrument used for the 14-gap census, the map-coordinates fix, and the collapsed-bridge work,
which until 2026-08-21 lived only as gitignored Python in `gap-files/`.

> **Objective, stated once.** *Make the instrument durable and CI-guarded.* Not: census the corpus,
> classify step boundaries, or size the BACKLOG item. The instrument measures — where a 1 s
> throat-adjacent A template locks in B, past production's ±600 ms window — and stops there.
> Everything downstream of a measurement is parked in §9 with the reason.
>
> **Why the cut.** The classifier designed in revisions (1)–(2) required the boundary rule of
> [archive/TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md) §4–§5,
> which refuses a verdict below 5 live gaps per pair and needs ≥2 settled gaps on each side of a
> candidate. On every listen tree that exists: pair 31 has 4 live gaps (even with `--also`) ⇒ no
> verdict; pair 50 has 6 gaps but no boundary gap among them. **There is no input on disk on which
> that classifier can return a boundary.** Building and unit-testing it now would ship a dead code
> path whose only possible output is `null`. The design is not lost — it is in the archive, which is
> its correct home; §9 records when to come back for it.

Parent: [archive/TEMP-lag-decision-map-coordinates-plan.md](archive/TEMP-lag-decision-map-coordinates-plan.md)
(prototype named in its Census section; "keep Python out of the repo"),
[TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md)
(this instrument's current consumer). Does **not** implement the Collapsed-bridge skip verdict
([BACKLOG.md](../../BACKLOG.md) § Fill placement) — and per §1.1 that verdict's spec is itself now
in question.

> **Verification rule.** A `file:line` reference belongs only in a checklist item that is about
> to be executed; design sections state the decision and its reason. Re-read
> `listen_registration.rs`'s header (independence + WAV geometry) before coding.

---

## 1. Problem

The collapsed-bridge / far-offset question is a **WAV-domain** one: given `--gap-listen`
`_a_surround` / `_b_surround`, where does a 1 s throat-adjacent template actually lock in B,
when the search is allowed to go past production's ±600 ms `lag_decision` window?

That instrument has lived as Python in gitignored `gap-files/`. Consequences:

- The original script already vanished once and had to be reconstructed (the surviving copy's
  own header says so). The next `gap-files/` wipe repeats that, and per project policy
  `gap-files/` is deletable at any time.
- `listen-registration` is the closest committed sibling and is **not** this instrument: it
  checks engine `donor_registration` on the **pre** shoulder only, ±1 s, 1 ms naive Pearson,
  no peak table, no floor/prominence.
- Production `lag_decision` (fine ±600 ms, envelope rescue ±2000 ms at 100 ms bins) is the
  thing under test on far-offset gaps. Reusing `seam_local::lag_correlation_curve_fft` or the
  envelope pass would make the check share code with the estimator it is supposed to
  independently confirm — the same constraint `listen-registration` documents.

The Python is two files only because the batch script `importlib`s the single-gap module.
That is one instrument, two entry points, not two products.

### 1.1 What Results 7–8 changed

This plan's first draft copied the Python's derived buckets verbatim, including
`collapsed_bridge` (`|bridge_ms| ≤ 50`). The census has since retired that predicate as a *class*:

- **Result 7** — `bridge_ms = gap_ms + step`, where `step = post_lag − pre_lag`. So
  `|bridge_ms| ≈ 0` fires exactly when a pair's map step happens to equal minus that gap's
  duration (31/g7: step −1121.9 ms vs gap 1118.8 ms, 3.1 ms apart). **No positive step can ever
  satisfy it** at any gap duration, so pair 50's +2.789 s step is structurally invisible to it.
  The invariant underneath is the **step boundary**: a gap whose pre and post sit on opposite
  sides of a pair-level map step.
- **Result 8** — the step boundary is not measurable from `lag_decision` dumps at the current
  search radius. Over 935 admitted rows the largest trustworthy step in the whole corpus is
  63.9 ms; both known steps (pair 31 ≈ −1.15 s, pair 50 ≈ +2.789 s) are censored, because a step
  past the search radius destroys the correlation that would measure it. The censoring is a
  **radius** effect, not a structural one — which is why a wider-envelope re-fingerprint, not this
  bin, is the cheap route to a base rate (§9).

Consequence for this plan, after the revision-(3) cut: `step_ms` and `bridge_ms` are **recorded as
columns**, and nothing in this bin branches on them. `step_ms` is the quantity Result 7 identified
as the invariant and is worth having on every row; `bridge_ms` is retained only so old census files
stay comparable, with a `_contract` line saying it is a coincidence and not a class.

---

## 2. Goal / non-goals

**Goal**

- One `[[bin]]` `shoulder-sweep` in `crates/clip-sync-repair`, `required-features = ["calibration"]`.
- Two modes, matching the two Python entry points:
  - **Single gap / one shoulder** — 1 s A template at the pre or post throat, sample-domain
    Pearson vs B at every integer-sample lag in `±max-lag-ms` (default 2000), 250 ms peak
    exclusion, stdout peak table + floor/prominence.
  - **Corpus** — walk a `--gap-listen` tree, run both shoulders on every gap, write
    `census.csv` / `census.json` of **raw measured rows**: lags, `peak_r`, prominence, `live`,
    `step_ms`, `bridge_ms`, `edge_pinned`, `sweep_may_be_narrow`, `source_tree`. No buckets, no
    per-pair verdict, no classifier (§4.3).
- Independent of production correlation: FFT Pearson lives in the bin (or a bin-local module).
  `rustfft` is already a `clip-sync-repair` dependency.
- Reconstructs the **archived registration-plan** §4.3 / 31/g7 pre-shoulder landmark: peak
  **r ≈ 0.8287 at ≈ −38.1 ms**, single spike, floor < 0.10. That is the media-side acceptance
  check (§6), and it is what proves the Rust matches the Python without the Python.
- Media hygiene matches `listen-registration`: print pair directory names and gap indices
  only — never media paths or titles. Census files use `entry_stem`s.

**Non-goals**

- **Any classification, bucketing, or per-pair roll-up.** Parked in §9 with the reason. A future
  reader wanting the step-boundary rule takes it from
  [archive/TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md) §4–§5,
  which is its canonical home — do not re-derive it (see that census's Question 12 for what
  happens when someone does).
- A corpus base rate of anything. This bin reads listen WAVs, which exist for ~38 gaps against
  the corpus's 971 with both anchors. Its denominator is *gaps with listen WAVs*, never *gaps in
  the corpus*, and `census.json`'s `_contract` says so in one line.
- `scratch_patch_registration_sweep.py` (sliding NCC of `_a_patched` vs `_b_surround`). Different
  question; a later mode if it earns a second user.
- `scratch_sliver_check.py` and the JSON-only scratch joins.
- A sixth workspace crate, or hosting this in `clip-sync-repair-harness` (`gap-fingerprint-stats`
  is dump/oracle analysis; this is WAV-domain).
- Folding into `listen-registration`. That bin's contract is a registration *check* (pre-shoulder,
  ±1 s, vs engine lag). Mixing in a ±2 s peak table would blur it.
- Folding into `--gap-fingerprints` or `--gap-listen`. This is a post-hoc bin on existing listen
  WAVs, the same pattern as `listen-registration`. Fingerprint already has a different estimator
  for the same question (`lag_decision`); putting sample-domain ±2000 ms Pearson on that dump
  would either duplicate the axis or share code with the sensor this tool exists to check.
  `--max-lag-ms` is a knob you re-run without a decode (pair 50's ~2.8 s offset).
- Changing production `lag_decision`, envelope rescue, or any skip verdict.
- Committing Python, listen WAVs, or census artifacts derived from licensed media.

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
*JSON* stats in the harness; this tool is the other direction (WAV in, rows out) and belongs
next to `listen-registration`. A sixth crate would split one `calibration` feature across three
packages for no dep-graph reason.

**Why one bin, two modes, not two bins.** Two binaries would duplicate the FFT Pearson. Lock
`--corpus <DIR>` (not a subcommand) so the default invocation stays "two WAVs + `--shoulder`",
matching what operators already type. `--corpus` and `--shoulder` are mutually exclusive: error if
both are set. Corpus always runs both shoulders.

Default `cargo build` must not grow this binary — same `required-features = ["calibration"]` as
the two siblings.

---

## 4. Instrument contract (locked)

**This section is the specification.** It is transcribed from the working Python (§4.5) so the
bin can be written from this document alone. Lag 0 is the nominal map at the chosen throat, same
units and sign as `DonorRegistration::lag_ms` / `listen-registration` (`wav_lag`): **negative means
B's matching content sits earlier than the nominal map put it.**

| Knob | Default | Meaning |
|---|---|---|
| Template | 1.0 s of A, adjacent to the throat | pre: the last 1.0 s *before* the throat; post: the first 1.0 s *after* it |
| Throat | recovered, not hardcoded 3.0 s | see §4.1 — this is the one deliberate improvement on the Python |
| Haystack | B surround, mono downmix (mean of all channels) | same reduction as `listen-registration`; do **not** drop LFE (that is the patched-sweep script) |
| Search | every integer-sample lag in `±max-lag-ms` | default 2000; FFT Pearson + prefix-sum variance, not 1 ms naive steps |
| Peak pick | 250 ms exclusion radius, top N (default 8) | greedy: take the highest r, then the next highest more than 250 ms from every peak already taken |
| Floor / prominence | floor = max r outside ±250 ms of the **top** peak; prominence = top − floor | the "is this a single spike" number. **Both are `Option`** — see the degenerate case below |
| `peak_z` | `(peak_r − mean r) / stddev r` over the whole swept curve | recorded, defines nothing. `None` when the stddev is ≤ `1e-12`. Carried because existing census CSVs have `pre_z` / `post_z` and dropping the columns would break the join (§4.3) |
| `live` | `peak_r ≥ 0.5` **and** `prominence ≥ 0.10` | derived column; findings §6.7 "confident" bar. A `None` prominence is **not** `live` |
| `unique_strict` | `peak_r ≥ 0.8` and same prominence | recorded, does not define `live` |
| `step_ms` | `post_lag_ms − pre_lag_ms` | the shoulder disagreement — Result 7's invariant. Recorded on every corpus row; **nothing branches on it** (§4.3) |
| `bridge_ms` | `gap_ms + step_ms` | comparison column only. `gap_ms` is WAV-derived from the same `ctx` that placed the throats (§4.1), not dump `duration_secs` |

Non-chained: each throat is searched independently. Do not feed pre's lag into post's haystack
origin — that chaining is what the map-coordinates fix had to undo on the production sensor.

**Degenerate floor.** When no swept lag lies outside ±250 ms of the top peak — a short
`--max-lag-ms`, or a clipped haystack (below) — there is nothing to take a floor over. Emit
`floor_r: null` / `prominence: null` rather than inventing a value, and `live` / `unique_strict` are
then `false` by the "`None` prominence is not `live`" rule above. This is what the Python does
(`if off.any()` → NaN → `null`); the only change is that Rust carries it as `Option` instead of
NaN, so no comparison silently returns `false` through a NaN.

**`edge_pinned` is measured against the range actually swept, not the requested one.** With no
clipping that is `|peak_lag| ≥ max-lag-ms − 20`. When B is shorter than the requested sweep the bin
sets `clipped_haystack` and records `usable_lag_ms` (the `[lo, hi]` the Python prints as a note),
**and `edge_pinned` then tests the peak against those usable bounds** — within 20 ms of either end.
Comparing a clipped sweep's peak to `max-lag-ms` is the failure mode this rule exists to prevent:
the peak sits on the real window edge, `|peak_lag|` is nowhere near `max-lag-ms`, and the
`sweep_may_be_narrow` guard below never fires on the one row whose reading is invalid.

Hard error, not a warning, when A and B sample rates differ. Resampling is out of scope and a
silent mismatch would corrupt every lag.

**`--max-lag-ms` and the `live` cut interact, and the default loses pair 50.** Pair 50's true
offset is ≈ +2.789 s, so at the default 2000 ms neither shoulder's real peak is inside the sweep and
both fail `live`. Two guards:

- Corpus mode **warns on stderr and sets `sweep_may_be_narrow`** on any row where a shoulder is
  not `live` *and* its best peak is `edge_pinned`. That is the signature of a true peak outside the
  window, and it is the only thing distinguishing "measured dead" from "not searched far enough."
- Do not raise the *default* past 2000. It matches the archived census numbers this instrument
  reconstructs (§6), and a silent default change would make old and new census files
  non-comparable. Pass `--max-lag-ms 3000` for pair 50 and record it; `_contract` carries the value
  actually used.

### 4.1 Throat / context recovery

The Python hardcodes `CONTEXT_SECS = 3.0` (`gap_signature_context_secs` default) and applies it to
both clips. `listen-registration` already does the better thing: recover context from clip length
and dump geometry, so a listen run made with a different pad still measures its own throats. **The
bin recovers; this is the one place it deliberately diverges from the Python, and on a default-pad
tree the two agree exactly.**

**`ctx` source (precedence).** `--context-secs` always exists and defaults to 3.0. When a sibling
dump JSON is present, **recovered geometry wins**; the flag is only the no-JSON fallback (it does
not override a dump). Sibling JSON is `stem.json` next to `stem_a_surround.wav` (batch-script
naming: strip `_a_surround.wav`, append `.json`).

```text
span = geometry.a_end_secs − geometry.a_start_secs
ctx  = (a_samples / rate − span) / 2     # recovered from A; used for both clips
```

Without a dump, `ctx = --context-secs` (default 3.0).

**Throats follow the Python, not "same sample index."** Listen A/B can differ in length because
`frame_range` clamps at media edges even though the export aims at the same ± context. Lag-0 origin
on each clip, with `m` = template length in samples:

```text
pre  throat: at ctx samples from the START of that clip.
             A template starts at ctx − m  (the window START is the lag-0 origin)
             B haystack lag 0 at ctx − m
post throat: at ctx samples from the END of that clip.
             A template starts at len(A) − ctx; B haystack lag 0 at len(B) − ctx
```

Do not place B's post throat at A's post sample index. When lengths match this is identical; when
they don't, end-relative placement is the one the Python (and the listen export's "pad each timeline
by context") actually used.

**`gap_ms` for `bridge_ms`** uses that same `ctx`:

```text
gap_ms = (len(A) / rate − 2 · ctx) × 1000
```

Dump `geometry.duration_secs` is a comparison column on the row (`gap_secs_dump`), not the bridge
input. That is the Python instrument: WAV-implied gap, dump duration joined beside it.

`--gap-listen` may omit `_b_surround.wav` when the gap never reached the fill plan. Skip that
stem with a stated reason (pair + gap index); do not invent a B clip.

### 4.2 Independence from production correlation

Implement the sweep in the bin:

1. Zero-mean the template; reject a silent template (zero variance) as an error, not a 0-score row.
2. FFT cross-correlation of the **zero-mean** template against the B haystack, `valid` region only.
   Because the template is zero-mean, the per-window haystack mean drops out of the numerator —
   that is why no windowed-mean subtraction appears in the correlation step.
3. Prefix sums of `hay` and `hay²` give every length-`m` window's variance in O(1):
   `var = Σx² − (Σx)²/m`, floored at `1e-20` before the square root.
4. `r = corr / (‖t‖ · √var)`.
5. `rustfft` is fine; do **not** call `lag_correlation_curve_fft` / `normalized_correlation` /
   `register_donor_window`.

**Step 2 in full, because this document is the only spec.** Four details decide whether the curve
is a correlation or wraparound noise, and all four are easy to get wrong from the prose alone:

```text
n = hay.len(), m = tpl.len()
size  = smallest power of two ≥ n + m          # linear, not circular, convolution
c_full = ifft( fft(hay, zero-padded to size) * fft(reverse(t), zero-padded to size) )
corr   = real(c_full)[m-1 .. n]                # the `valid` region: n-m+1 lags
```

- The template is **time-reversed** before its transform (correlation = convolution with the
  reversed kernel).
- `size` is the next power of two **≥ `n + m`**. Anything smaller wraps the tail of the product
  back onto the head, which still produces a plausible-looking curve.
- The `valid` slice is `[m-1, n)` — lag *k* of that slice is haystack offset *k*, and lag 0 of the
  sweep is `b_anchor` (§4.1), so `lag_ms = (k + clipped_lo − b_anchor) / rate × 1000`.
- **`rustfft`'s inverse is unnormalized** — unlike NumPy's `irfft`, which the Python relies on.
  Scale the inverse by `1 / size`, or the whole curve is off by a factor of `size` and every `r`
  overflows the `live` cut. `rustfft` has no real-input transform, so run the complex FFT on both
  signals with zero imaginary parts and take the real part of the inverse.

WAV decode may be copied from `listen-registration`'s `read_mono` (hound, all sample formats that
listen actually writes: 16-bit, 24-bit packed, 32-bit). Sharing WAV I/O with that bin is allowed —
WAV I/O is not the estimator under test. Extracting a calibration-gated lib helper is optional and
not required for v1; duplicating ~40 lines is the same choice `listen-registration` already made
versus production.

Do not add a `csv` crate. `census.json` is the durable record; `census.csv` is a simple
fixed-column write of the same rows (stems + numbers, no titles, so quoting is trivial).

### 4.3 Derived columns — and why there are no buckets

Every corpus row carries `live`, `unique_strict`, `peak_z`, `step_ms`, `bridge_ms`, `edge_pinned`,
`clipped_haystack` / `usable_lag_ms`, `sweep_may_be_narrow`, `source_tree`, plus the joined dump
columns of §5. **Nothing in the bin branches on `step_ms` or `bridge_ms`.**

**Column parity with the existing census files.** The batch script's CSV header is
`pair, fingerprint_gap_1based, dump_g, a_start_secs, listen_disposition, gap_ms, pre_lag_ms, pre_r,
pre_z, pre_prom, pre_live, post_*, bridge_ms, bucket, dump_pre_ms, dump_pre_r, dump_pre_verdict,
dump_post_*, stem`. The bin keeps all of them except `bucket` (cut with the classifier) and adds
`step_ms`, `unique_strict`, `edge_pinned`, `clipped_haystack`, `usable_lag_ms`,
`sweep_may_be_narrow`, `source_tree`. `pre_z` / `post_z` in particular are **kept** — they are two
accumulators over a curve already in hand, and census CSVs on disk carry them, so dropping them
would silently break a join against those files. Per-shoulder `sr`, `gap_secs`, `template_a_secs`
and `lag0_r` stay where the Python put them: in the `<stem>_pre.txt` / `<stem>_post.txt` logs and
`census.json`, not in the CSV. `dump_pre_prom` / `dump_pre_z` are collected by the Python's
`dump_lag` and then never written to its CSV; the bin does not carry them either — say so here so
§4.5's "nothing load-bearing is undocumented" claim stays true after the scripts are deleted.

The first two revisions of this plan specified a six-bucket classifier (`step_boundary`,
`step_boundary_unconfirmed`, `step_excursion`, `shared_far_offset`, `shared_near_false_dead`,
`one_live` / `none_live`) over a per-pair persistence test. It is cut for the reason in the status
box: the rule it needs cannot return a boundary on any listen tree that exists, so the whole ladder
would ship as a dead path emitting `null`. Rows carrying `step_ms` are sufficient for a human to
classify by eye on a 38-gap tree, and are exactly the input a real classifier would consume later.

`|bridge_ms| ≤ 50` in particular must never become a bucket again — Result 7 (§1.1). If it appears
at all it is a boolean annotation beside `step_ms`, and `_contract` states in one line that it is an
arithmetic coincidence and not a class, so a future reader of an old `census.json` does not
resurrect it.

### 4.4 What replaces the per-pair roll-up

One stdout summary block, no verdicts:

- gaps swept / gaps skipped (with reasons), and `live` shoulder count
- the `live_gaps / total_gaps` denominator, followed by the §2 caveat **in words**: this is gaps
  with listen WAVs, not gaps in the corpus, and nothing here is a base rate
- any row with `sweep_may_be_narrow`, listed by pair + gap index, since that is the one condition
  that invalidates a reading rather than merely describing it

### 4.5 Provenance: how this gets built without importing the Python

`gap-files/scratch_shoulder_sweep.py` and `scratch_shoulder_sweep_batch.py` existed on disk while
this was written and were **deleted on 2026-08-21** once the landmark matched (§8 step 6). They were
gitignored throughout — the constraint was *don't commit Python*, not *don't read it*. The route
was therefore:

1. **Transcribe the contract while the source exists — done.** §4, §4.1, and §4.2 above are the
   full method: template placement and both throat anchors, the mono downmix, the zero-mean FFT
   `valid` cross-correlation (with its transform size, template reversal, and `valid` slice), the
   prefix-sum variance with its `1e-20` floor, the greedy 250 ms peak picker, the floor/prominence
   definition and its null case, `peak_z`, the `live` cut, the sample-rate hard error, and the
   clipped-haystack reporting; §4.3 and §5 carry the census column set, the pair-label chain, the
   dump-index numbering, and the `--also` dedupe rule. A review on 2026-08-21 found six of those
   absent from the first three revisions and they were added then; nothing else in the scripts is
   load-bearing and undocumented here. **This plan is now the durable spec; the scripts are
   disposable.**
2. **Write Rust from this document, not by porting line-by-line.** A port would carry the Python's
   hardcoded `CONTEXT_SECS = 3.0` and its `sys.exit` error handling into a bin that should recover
   geometry (§4.1) and return `Result`. The numeric method is what must match, not the structure.
3. **Prove the match on the landmark, not on the source.** `--shoulder pre` on 31/g7 must produce
   peak **r ≈ 0.8287 at ≈ −38.1 ms**, floor < 0.10 — the archived registration-plan §4.3 number,
   which predates both the Python and this plan. Two independent implementations hitting a
   four-decimal correlation on real media is a stronger equivalence check than reading the source
   would give, and it does not depend on the source surviving.
4. **Pin it in CI with synthetic PCM** (§6) so the instrument stays correct after the WAVs and the
   scripts are both gone.
5. **Delete the two scripts once the landmark check passes**, in the same PR. Leaving them
   invites a future reader to run the Python instead of the bin and get a different `ctx`.

If `gap-files/` is wiped before this is implemented, step 1 has already banked the contract and
step 3 needs only a regenerated listen tree (`--gap-listen`) — not the scripts.

---

## 5. CLI sketch

```text
# Single gap
cargo run -p clip-sync-repair --features calibration --bin shoulder-sweep -- \
    --shoulder pre <stem>_a_surround.wav <stem>_b_surround.wav
cargo run -p clip-sync-repair --features calibration --bin shoulder-sweep -- \
    --shoulder post --max-lag-ms 2000 --peaks 8 A.wav B.wav

# Corpus. Errors if --shoulder is also set.
cargo run -p clip-sync-repair --features calibration --bin shoulder-sweep -- \
    --corpus gap-files/<listen-run> [--also gap-files/<extra>] [--out DIR]

# Flat pair directory whose name is not the pair index: label it explicitly.
# Errors if the directory holds pair subdirectories — those name themselves.
cargo run -p clip-sync-repair --features calibration --bin shoulder-sweep -- \
    --corpus gap-files/2026-08-18-post-sweep --pair 31
```

`--shoulder` is required in single-gap mode and forbidden with `--corpus`. Corpus always sweeps
pre and post.

**Discovery reuses `listen-registration`'s `collect_gaps` semantics, not the Python's.** That
function scans `*_a_surround.wav` in the root itself (labelled `.`) **and** in each immediate
subdirectory (labelled by its directory name), sorted — one level deep, not recursive. The Python
batch script uses a recursive `rglob`; the difference is immaterial for every layout `--gap-listen`
produces (a run directory of numbered pair subdirectories, or a single flat pair directory) and the
shallower walk is the one already proven against these trees. Sibling `_b_surround.wav` and
`stem.json` come from the same directory as the A clip.

**Pair labels.** `collect_gaps` labels a flat root `.`, which is wrong for `--also
gap-files/<run>/31` — that row's pair is 31, and a census that labels it `.` cannot be joined or
merged. **The pair index exists nowhere in the artifacts**: `GapCorpus.source` carries
`FileSource.id` (a content hash such as `919e5368…`), `listen.json` carries `a_id` / `b_id` /
`stem` / `index`, and neither carries a pair — the only other candidate is a media path, which
§2's hygiene rule forbids reading into a label. So the resolution chain is directory- and
operator-supplied, in this order:

1. the subdirectory name, when `collect_gaps` labels the row by one (the normal run-of-pairs case);
2. for `.`-labelled rows, `--pair <label>` — the supported way to label a flat pair directory whose
   name is not the index;
3. for `.`-labelled rows with no `--pair`, the **root directory's own name** when it parses as a
   pair index;
4. `.`, with a one-line stderr note naming the root — never a guess.

**`--pair` fills in; it never overrides.** A label the tree supplied is authoritative, and passing
`--pair` at a run directory that holds pair subdirectories is an **error naming them**, not a silent
flattening of distinct pairs under one label. That keeps the rule the repo already has: no other
calibration bin lets an operator replace a discovered label. `listen-registration` derives the same
label from the same walk and has no such flag at all — and there the label is load-bearing, since it
resolves the `--observe-dir` join (`engine_lag`: `if pair == "." { root } else { root.join(pair) }`),
so an override would silently redirect a join. `gap-fingerprint-stats` (harness `pair_label`) reaches
the flat case a third way, labelling by the path relative to the root's *parent* so a numeric leaf
directory self-labels; step 3 above covers that same case, and `--pair` is only for the residue
neither the subdirectory nor the root name can answer.

Do **not** write a "take it from the dump's pair field" step: there is no such field, and an
implementer who goes looking will find only the hashed ids or the media path.

**Gap numbering: `g7` means dump `index` 7.** The listen stem tag (`…_g007_full`), `listen.json`'s
`index`, and the dump's `gaps[0].index` all agree and are what every note, census, and this plan's
own §6 landmark mean by *31/g7*. The batch script's `fingerprint_gap_1based` is `index + 1` — a
different number for the same gap (8 for the landmark). Keep that column for join compatibility
with existing census files, but **stdout, the `<stem>_*.txt` logs, all stderr notes, and the §6
landmark report use the dump `index`**, unadorned. §5's per-gap stdout line therefore prints the
dump index, not a 1-based one; a row that prints `31 g8` for the landmark gap is a bug, not a
convention.

`--out` defaults to `<corpus>/wide-sweep/`. Rows are emitted sorted by `(pair, a_start_secs)` when
dumps are present; **`a_start_secs` comes from the dump, which §4.1 makes optional**, so rows with
no dump sort last within their pair by stem, and carry `a_start_secs: null`.

`--also` appends extra listen dirs and merges by pair index, so one pair's gaps may come from more
than one tree — the Python used this for a one-gap overlay of 31/g7, which lives in a different tree
from the rest of pair 31. Record which tree each row came from (`source_tree`); a reader comparing
lags across a merged run must be able to see that without re-deriving it.

**Dedupe: first root wins, by stem.** Roots are visited in order (`--corpus`, then each `--also` as
given); a stem already emitted is skipped, and the skip is one stderr line naming the stem, the tree
that won, and the tree that lost. This is the Python's rule (a `seen` set over stems) and it is why
`source_tree` is a single value rather than a list — a stem exported into two trees is one row, from
the first tree, and the operator can see it happened. Same-stem-different-tree is not a merge: the
two exports can differ (different pad, different run), so averaging or preferring the later one
would silently mix instruments.

Stdout for one shoulder keeps the prototype log shape (shoulder, sr, gap, template time, r at lag 0,
peak table, floor/prominence) so existing notes stay comparable. Corpus mode prints one line per gap
(pair, dump gap index per the numbering rule above, both lags, both r, `step_ms`, `bridge_ms`,
`live`), then the §4.4 summary block, and writes:

- `census.csv` — the joinable column set the batch script already used (including `pre_z` /
  `post_z` and `fingerprint_gap_1based`; excluding `bucket`), plus `step_ms`, `live`,
  `unique_strict`, `edge_pinned`, `clipped_haystack`, `usable_lag_ms`, `sweep_may_be_narrow`,
  `source_tree` — the full list is §4.3
- `<stem>_pre.txt` / `<stem>_post.txt` — the single-gap stdout log for each shoulder, verbatim, so
  a corpus run leaves the same artifacts an operator would get by sweeping each gap by hand
- `census.json` — same rows + `pre_peaks` / `post_peaks` + a `_contract` object quoting the
  instrument, `max_lag_ms` **as actually run**, `context_secs` or "recovered", the `step_ms` /
  `bridge_ms` formulas, the `live` cut, the one-line statement that `bridge_ms ≈ 0` is a
  coincidence and not a class (§4.3), and the one-line statement that the denominator is *gaps with
  listen WAVs* and this file is not a base rate (§2)

Join dump `lag_decision` (pre/post `frac_lag_ms`, `peak_r`, `verdict`) and `listen.json`
`disposition` when those files exist, as the batch script did — derived comparison columns, not
inputs to the sweep. `LagDecision::pre_anchor` / `post_anchor` are typed `Vec<LagSummary>`, so the
join is `.first()`; the Python's list-or-object `side0` helper exists only because it read raw JSON
and has no Rust counterpart.

Relative dirs resolve against the **repo root**, same as `gap-fingerprint-stats`.

---

## 6. Tests (CI) vs media check (not CI)

Colocated `#[cfg(test)]` in the bin, same pattern as `listen_registration.rs`. Synthetic PCM only —
`hound` temp WAVs if a file-roundtrip is easier than in-memory, never licensed media.

| Test | Pins |
|---|---|
| Known integer-sample shift of a sine recovers at r ≈ 1 at the expected lag | the sweep, not peak-picking |
| Negative lag means B is early | sign convention (§4) |
| A DC-offset haystack scores the same as a zero-mean one | §4.2 step 2 — the zero-mean template is what makes windowed means drop out |
| A constant (zero-variance) haystack region does not produce NaN or `inf` | §4.2 step 3, the `1e-20` floor |
| Silent template is an error, not a zero-score row | §4.2 step 1 |
| Peak picker keeps two peaks 300 ms apart and drops one 100 ms apart | 250 ms exclusion |
| Floor excludes ±250 ms around the **top** peak, and prominence = top − floor | §4 definitions |
| A sweep with no lag outside ±250 ms of the top peak yields `floor_r: null` / `prominence: null` and is **not** `live` | §4 degenerate floor |
| A haystack long enough to need padding past `n + m` correlates to the same curve as a naive O(nm) reference on a short signal | §4.2 — FFT size, template reversal, `1/size` scaling, `valid` slice, all at once |
| `live` requires **both** `peak_r ≥ 0.5` and `prominence ≥ 0.10` — a broad r = 0.9 ridge is not `live` | §4 cut |
| `step_ms` = post − pre, and no output field changes when it is large | §4.3 — the no-classifier contract |
| `bridge_ms` ≈ 0 changes no other column | §4.3 — never a class again |
| `edge_pinned` at `\|peak_lag\| ≥ max_lag − 20`; `sweep_may_be_narrow` when a non-`live` shoulder's best peak is `edge_pinned` | §4 narrow-sweep guard |
| On a **clipped** haystack, a peak within 20 ms of a `usable_lag_ms` bound is `edge_pinned` (and sets `sweep_may_be_narrow`) even though `\|peak_lag\|` is far from `max_lag` | §4 — the guard's real failure mode |
| `peak_z` is `(peak_r − mean) / stddev` of the swept curve, and `None` on a constant curve | §4 / §4.3 column parity |
| Differing A/B sample rates is an error | §4 |
| B shorter than the sweep sets `clipped_haystack` + a correct `usable_lag_ms` | §4 |
| Context recovery: clip length = span + 2·ctx places the pre throat at `ctx`, template start at `ctx − m` | §4.1 |
| Unequal A/B lengths: post throats are each `ctx` from that clip's end, not the same index | §4.1 |
| No dump ⇒ `--context-secs` fallback; dump present ⇒ recovered geometry wins over the flag | §4.1 precedence |
| Discovery: a flat pair directory and a run of numbered subdirectories both enumerate; a second-level nested tree does **not** | §5 — depth-1, matching `collect_gaps`, unlike the Python's `rglob` |
| Pair label: a root whose own directory name is a pair index labels its flat rows with it, not `.`; `--pair` overrides; a non-index root with no `--pair` is `.` plus a stderr note | §5 — the `--also` correctness case, and the chain that replaces the non-existent dump pair field |
| Gap index on stdout and in the logs is the dump `index` (7 for `…_g007_full`), while `fingerprint_gap_1based` is 8 on the same row | §5 numbering rule |
| The same stem under `--corpus` and `--also` emits one row, from the first root, with that `source_tree` and a stderr note | §5 dedupe |
| Rows with no dump sort last within their pair and carry `a_start_secs: null` | §5 ordering under §4.1's optional dump |
| Roll-up denominator counts gaps with WAVs, not dump gaps | §4.4 self-labelling |
| `--corpus` + `--shoulder` is a clap error | CLI mutex |

No new `[[test]]` binary — tests live in the bin file so default `cargo test -p clip-sync-repair`
(lib only, `autotests = false`) does not grow an integration target.

**Those tests must run on PR.** `listen-registration`'s colocated tests are *not* in
`test-tier.ps1`, so copying that pattern alone would leave the sweep untested in CI. Add

```powershell
cargo test -p clip-sync-repair --features calibration --bin shoulder-sweep
```

to the existing `pr-repair` calibration invocation (the block that already passes
`--features calibration` and the three `--test` listen/fingerprint binaries). Do not add a new
`[[test]]`. Feature-off `cargo build` still must not emit the bin.

**Media acceptance check (checklist, not CI) — this is the equivalence proof of §4.5.3.**
`--shoulder pre` on 31/g7 listen WAVs must show a unique peak near **−38.1 ms, r ≈ 0.8287, floor
< 0.10**. 31/g7 lives in `gap-files/2026-08-18-one-dead-listen-decision-map/31/`, not in the
collapsed-bridge tree. If that directory is gone, regenerate with `--gap-listen` /
`measure-gap-fingerprints.ps1` rather than relaxing the landmark. Record the three numbers in the PR
description (pair + gap index only, no media names).

**Optional second media check.** `--corpus` over any surviving listen tree, confirming it walks,
joins dumps, and writes both files without a panic. Nothing about its *contents* is a result — see
§2 and the `_contract` line. Do not write it up as a finding.

`cargo build` (feature off) has no `shoulder-sweep`. `cargo build --features calibration` builds it
alongside the two siblings.

---

## 7. Docs (at implement, not before)

- `crates/clip-sync-repair/Cargo.toml` — `[[bin]]` comment in the same voice as
  `listen-registration` (what it measures, that it is independent of production lag).
- `docs/dev/development.md` — add the bin to the `calibration` feature row and the `cargo run`
  examples next to `listen-registration`.
- `scripts/test-tier.ps1` — `--bin shoulder-sweep` on the `pr-repair` calibration invocation (§6).
  Not a new `[[test]]`.
- `docs/dev/gap-fingerprint.md` § *`--gap-listen`* — one short pointer: the independent
  wide-window waveform instrument on these WAVs is `shoulder-sweep`; it is not `lag_decision` and
  not `listen-registration`.
- `scripts/README.md` — optional one-liner under investigation harnesses (this is a cargo bin, not
  a new `.ps1`). No measure-script wrapper unless someone is already wrapping `listen-registration`
  the same way (they are not).

Do not rewrite the collapsed-bridge census notes to pretend the Python never existed; a
parenthetical "now `shoulder-sweep`" on the next edit of those TEMP files is enough.

---

## 8. Steps

1. **Wire the bin.** `[[bin]]` + clap args for single-gap. Header comment: what it is, why it must
   not share production correlation, lag-0 = nominal map, media hygiene, and that §4 of this plan is
   its specification.
2. **Sweep + peaks.** FFT Pearson, prefix-sum variance, peak table, floor/prominence, stdout log.
   Colocated shift / sign / DC / zero-variance / silent-template / peak-picker / `live` tests.
3. **Context recovery.** Dump geometry when present, `--context-secs` fallback, both throat
   anchors per §4.1. Skip stems with no B surround. Recovery + precedence tests.
4. **Corpus mode.** Discover pairs (label chain incl. `--pair`, `--also` merge by pair index with
   first-root-wins dedupe carrying `source_tree`, dump-index numbering — all §5),
   both shoulders, derived columns per §4.3, `sweep_may_be_narrow` guard, §4.4 summary, write
   `census.csv` / `census.json` + `_contract`, join dump lags and `listen.json` disposition when
   present. **No classification.** Column and denominator tests.
5. **Docs + feature-off build + PR wiring.** `development.md` / Cargo.toml comment / fingerprint
   pointer. Confirm default `cargo build` does not emit `shoulder-sweep`. Add `--bin shoulder-sweep`
   to the `pr-repair` calibration `cargo test` invocation (`scripts/test-tier.ps1`).
6. **Media acceptance + retire the Python.** Run the 31/g7 pre landmark (§6). On a match, delete
   `gap-files/scratch_shoulder_sweep.py` and `scratch_shoulder_sweep_batch.py` in the same PR
   (§4.5.5) and record the three landmark numbers in the PR description. On a mismatch, do not
   relax the landmark — the discrepancy is the finding.

---

## 9. Out of scope / parked

| Item | Why parked |
|---|---|
| **The step-boundary classifier** (buckets, per-pair persistence, lag plateaus, `step_floor`) | Cut in revision (3). The rule lives in [archive/TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md) §4–§5 and is canonical there. **Un-park when a listen tree exists with ≥5 live gaps in one pair, ≥2 of them settled on each side of a candidate** — that is the first moment the rule can return anything but `null`. Until then, `step_ms` on the row is the whole story. Do **not** re-derive the rule from the corpus CSV; that was tried on 2026-08-21 and manufactured a false boundary (parent census, Question 12). |
| The Python's three bucket constants — `AGREE_MS = 250` (shoulders "agree"), `IN_REACH_MS = 600` (production's window), `COLLAPSED_ABS_MS = 50` | Recorded here so §4.5's claim holds — nothing load-bearing in the scripts is undocumented — and so a future classifier starts from the measured constants rather than guessing. None of them is used by the cut-down bin. |
| Lag plateaus / single-linkage clustering as a classifier | Evaluated 2026-08-21 over Result 8's 935-row CSV and **rejected**: it never fired where the step test did not, and it degrades as more of a pair is sampled (pair 19's 13 gaps chain at 42 ms; pair 25's 47 gaps chain across a 331–410 ms spread). A detector that gets less sensitive with more data is the wrong shape. Do not re-propose it. |
| A corpus-wide `--gap-listen` run to get a boundary base rate | Real cost (per [corpus-fingerprint-run-protocol], one pair at a time, ~15 GB RSS), not authorized here, and likely the expensive way round — see the next row. |
| Sizing the BACKLOG item | [TEMP-envelope-adaptive-widen-plan.md](TEMP-envelope-adaptive-widen-plan.md) owns it. Result 8's censoring is a *search-radius* effect, so a re-fingerprint at a wider envelope reaches a corpus base rate for the price of a dump run (~6.2 MB out), where a corpus-wide listen run does not. This bin is an instrument that plan may re-run, not the route to the number. |
| Widening `--lag-envelope-max-lag-ms` | Same plan owns it; this bin does not touch production. |
| Shipping a skip verdict of any shape | BACKLOG's § Fill placement. Its spec is currently `\|gap + post_lag − pre_lag\| ≈ 0`, which §1.1 shows is a coincidence-dependent proxy blind to every positive step. Re-aiming that item at the step boundary is a BACKLOG edit, not this plan's work — and do not implement it as written on the strength of this bin's output. |
| Sliding patched-vs-B NCC (`scratch_patch_registration_sweep.py`) | Different question (does the *patch* hold one lag). Same bin later as a third mode if it earns a second user. Note it is **not** covered by §4.5's retire-the-Python step — that script stays until it has its own plan. |
| PowerShell wrapper under `scripts/measure/` | Operators already `cargo run --bin listen-registration`; do not invent a third invocation style. |
| Census Questions 7 and 11 | Q7's gaps (31/g9, 50/g17, 25/g11) have no listen WAVs in any surviving tree, so `--corpus` cannot reach them without a media run. Q11 is a post-join patch-quality question; wrong instrument. |
