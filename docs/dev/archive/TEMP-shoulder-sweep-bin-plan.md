# TEMP — Promote the listen-WAV shoulder sweep into `clip-sync-repair`

**Status:** **IMPLEMENTED 2026-08-21, ARCHIVED 2026-08-22** —
`crates/clip-sync-repair/src/bin/shoulder_sweep.rs` (`[[bin]] shoulder-sweep`,
`required-features = ["calibration"]`), 26 colocated tests wired into `pr-repair` via
`--bin shoulder-sweep`. Media acceptance passed on the 31/g7 pre landmark (r 0.8287 at -38.1 ms,
floor 0.0979), and both scratch Python scripts were deleted with it.

> **The contract is not here.** On 2026-08-22 the instrument contract (§1.1, §4–§4.4, §5) was
> promoted to the live doc [shoulder-sweep.md](../shoulder-sweep.md); those sections are stubs
> pointing at it, and the live doc is what the bin is written against and what CI pins. What
> remains here is the plan record: the objective and the reason for each revision, the non-goals
> and placement rationale (§2–§3), the provenance of the transcription and the Python retirement
> (§4.5), the test list and media-acceptance procedure as specified (§6), the doc/step checklists
> (§7–§8), and what stays parked and why (§9). **Section references below use this document's
> original numbering** — see the live doc's header for the map.

Drafted 2026-08-21. **Revised four times on 2026-08-21, once on 2026-08-22.** (1) against the
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
dump "pair field" that does not exist in the schema. (5) **2026-08-22, post-implementation:** two label changes, both matched in the bin. §5's `--pair` was tightened from "overrides both" to "fills in for `.`-labelled rows only, and errors on a tree of pair subdirectories" — an operator-supplied label replacing a discovered one had no precedent in any sibling calibration bin. And step 4 of the chain now falls back to the **root directory's own name** rather than a bare `.`, with the stderr note retained; that is the one idea worth borrowing from `gap-fingerprint-stats`'s `pair_label`, whose parent-relative path label was otherwise rejected (it would break the `pair` column's role as a join key against existing census files, duplicate `source_tree`, and still not retire `--pair`).

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
> [archive/TEMP-step-boundary-census-plan.md](TEMP-step-boundary-census-plan.md) §4–§5,
> which refuses a verdict below 5 live gaps per pair and needs ≥2 settled gaps on each side of a
> candidate. On every listen tree that exists: pair 31 has 4 live gaps (even with `--also`) ⇒ no
> verdict; pair 50 has 6 gaps but no boundary gap among them. **There is no input on disk on which
> that classifier can return a boundary.** Building and unit-testing it now would ship a dead code
> path whose only possible output is `null`. The design is not lost — it is in the archive, which is
> its correct home; §9 records when to come back for it.

Parent: [archive/TEMP-lag-decision-map-coordinates-plan.md](TEMP-lag-decision-map-coordinates-plan.md)
(prototype named in its Census section; "keep Python out of the repo"),
[TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md)
(this instrument's current consumer). Does **not** implement the Collapsed-bridge skip verdict
([BACKLOG.md](../../../BACKLOG.md) § Fill placement) — and per §1.1 that verdict's spec is itself now
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

**Moved to the live doc, 2026-08-22** — [shoulder-sweep.md](../shoulder-sweep.md) §1. In short: `bridge_ms = gap_ms + step`, so
`|bridge_ms| ≈ 0` is an arithmetic coincidence over the real invariant (the step boundary), and
Result 8 showed that boundary is not measurable from `lag_decision` dumps at the current search
radius. That is why §9 parks the classifier and why the bin records `step_ms` without branching
on it.

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
  [archive/TEMP-step-boundary-census-plan.md](TEMP-step-boundary-census-plan.md) §4–§5,
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
[archive/calibration-tooling-plan.md](calibration-tooling-plan.md) avoided by putting
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

## 4. Instrument contract — moved

**Moved to the live doc, 2026-08-22.** The contract this plan transcribed from the Python is now
[shoulder-sweep.md](../shoulder-sweep.md) §2 (§2.1 throat/context recovery, §2.2 independence from production correlation,
§2.3 derived columns, §2.4 the stdout summary block) — that document, not this one, is what the
bin is written against and what CI pins. §4.5 below stays here: it is the provenance record, not
the contract.

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

## 5. CLI sketch — moved

**Moved to the live doc, 2026-08-22** — [shoulder-sweep.md](../shoulder-sweep.md) §3 (CLI, depth-1 discovery, the pair-label
chain, `--also` dedupe, gap numbering, and the census/JSON/log outputs).

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
| Pair label: a root whose own directory name is a pair index labels its flat rows with it, not `.`; `--pair` fills in for `.`-labelled rows; a non-index root with no `--pair` falls back to that root's name plus a stderr note | §5 — the `--also` correctness case, and the chain that replaces the non-existent dump pair field |
| Gap index on stdout and in the logs is the dump `index` (7 for `…_g007_full`), while `fingerprint_gap_1based` is 8 on the same row | §5 numbering rule |
| The same stem under `--corpus` and `--also` emits one row, from the first root, with that `source_tree` and a stderr note | §5 dedupe |
| Rows with no dump sort last within their pair and carry `a_start_secs: null` | §5 ordering under §4.1's optional dump |
| Roll-up denominator counts gaps with WAVs, not dump gaps | §4.4 self-labelling |
| `--pair` at a root of pair subdirectories is an error naming them, and never relabels a row the tree labelled | §5 — `--pair` fills in; it never overrides |
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
| **The step-boundary classifier** (buckets, per-pair persistence, lag plateaus, `step_floor`) | Cut in revision (3). The rule lives in [archive/TEMP-step-boundary-census-plan.md](TEMP-step-boundary-census-plan.md) §4–§5 and is canonical there. **Un-park when a listen tree exists with ≥5 live gaps in one pair, ≥2 of them settled on each side of a candidate** — that is the first moment the rule can return anything but `null`. Until then, `step_ms` on the row is the whole story. Do **not** re-derive the rule from the corpus CSV; that was tried on 2026-08-21 and manufactured a false boundary (parent census, Question 12). |
| The Python's three bucket constants — `AGREE_MS = 250` (shoulders "agree"), `IN_REACH_MS = 600` (production's window), `COLLAPSED_ABS_MS = 50` | Recorded here so §4.5's claim holds — nothing load-bearing in the scripts is undocumented — and so a future classifier starts from the measured constants rather than guessing. None of them is used by the cut-down bin. |
| Lag plateaus / single-linkage clustering as a classifier | Evaluated 2026-08-21 over Result 8's 935-row CSV and **rejected**: it never fired where the step test did not, and it degrades as more of a pair is sampled (pair 19's 13 gaps chain at 42 ms; pair 25's 47 gaps chain across a 331–410 ms spread). A detector that gets less sensitive with more data is the wrong shape. Do not re-propose it. |
| A corpus-wide `--gap-listen` run to get a boundary base rate | Real cost (per [corpus-fingerprint-run-protocol], one pair at a time, ~15 GB RSS), not authorized here, and likely the expensive way round — see the next row. |
| Sizing the BACKLOG item | [BACKLOG.md](../../../BACKLOG.md) § *Fill placement* (flat `--lag-envelope-max-lag-ms` dump). Result 8's censoring is a *search-radius* effect, so a re-fingerprint at a wider envelope reaches a corpus base rate for the price of a dump run (~6.2 MB out), where a corpus-wide listen run does not. Forensic: [TEMP-envelope-adaptive-widen-plan.md](TEMP-envelope-adaptive-widen-plan.md). This bin is an instrument that dump may re-run, not the route to the number. |
| Widening `--lag-envelope-max-lag-ms` | BACKLOG § *Fill placement* (flat dump). This bin does not touch production. |
| Shipping a skip verdict of any shape | BACKLOG's § Fill placement. Its spec is currently `\|gap + post_lag − pre_lag\| ≈ 0`, which §1.1 shows is a coincidence-dependent proxy blind to every positive step. Re-aiming that item at the step boundary is a BACKLOG edit, not this plan's work — and do not implement it as written on the strength of this bin's output. |
| Sliding patched-vs-B NCC (`scratch_patch_registration_sweep.py`) | Different question (does the *patch* hold one lag). Same bin later as a third mode if it earns a second user. Note it is **not** covered by §4.5's retire-the-Python step — that script stays until it has its own plan. |
| PowerShell wrapper under `scripts/measure/` | Operators already `cargo run --bin listen-registration`; do not invent a third invocation style. |
| Census Questions 7 and 11 | Q7's gaps (31/g9, 50/g17, 25/g11) have no listen WAVs in any surviving tree, so `--corpus` cannot reach them without a media run. Q11 is a post-join patch-quality question; wrong instrument. |
