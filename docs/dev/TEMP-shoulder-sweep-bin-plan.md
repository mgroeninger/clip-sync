# TEMP — Promote the listen-WAV shoulder sweep into `clip-sync-repair`

**Status:** draft, not started, 2026-08-21. **One deliverable:** a `calibration`-gated
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
will keep needing.

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

---

## 2. Goal / non-goals

**Goal**

- One `[[bin]]` `shoulder-sweep` in `crates/clip-sync-repair`, `required-features = ["calibration"]`.
- Two modes, matching the two Python scripts:
  - **Single gap / one shoulder** — 1 s A template at the pre or post throat, sample-domain
    Pearson vs B at every integer-sample lag in `±max-lag-ms` (default 2000), 250 ms peak
    exclusion, stdout peak table + floor/prominence.
  - **Corpus** — walk a `--gap-listen` tree, run both shoulders, write `census.csv` /
    `census.json` with the same derived buckets the batch script defined (`collapsed_bridge`,
    `shared_far_offset`, `shared_near_false_dead`, `real_step`, `one_live`, `none_live`).
- Independent of production correlation: FFT Pearson lives in the bin (or a bin-local module).
  `rustfft` is already a `clip-sync-repair` dependency.
- Reconstructs the archived §4.3 / 31/g7 pre-shoulder landmark: peak **r ≈ 0.8287 at ≈ −38.1 ms**,
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
  (pair 50’s ~2.8 s offset; the envelope-widen ladder). A collapsed-bridge skip, when it ships,
  reads production `lag_decision`, not this census.
- Changing production `lag_decision`, envelope rescue, or any skip verdict.
- Committing Python, listen WAVs, or census artifacts derived from licensed media.
- Treating census `bucket` as a production tag or a skip-verdict detector. Buckets are derived
  columns on **this** instrument’s peaks — retune the cuts in the JSON `_contract`, do not edit
  the r/lag columns. The collapsed-bridge census already refused to ship a detector off shape
  alone ([TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md)
  Non-goals).

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

Copied from the reconstructed Python, which itself reconstructed archive registration-plan §4.3.
Lag 0 is the nominal map at the chosen throat, same units and sign as
`DonorRegistration::lag_ms` / `listen-registration` (`wav_lag`): **negative means B’s matching
content sits earlier than the nominal map put it.**

| Knob | Default | Meaning |
|---|---|---|
| Template | 1.0 s of A, adjacent to the throat | pre: last 1.0 s before the throat; post: first 1.0 s after it |
| Throat | recovered, not hardcoded 3.0 s | see §4.1 |
| Haystack | B surround, mono downmix (mean of all channels) | same reduction as `listen-registration`; do **not** drop LFE (that is the patched-sweep script) |
| Search | every integer-sample lag in `±max-lag-ms` | default 2000; FFT Pearson + prefix-sum variance, not 1 ms naive steps |
| Peak pick | 250 ms exclusion radius, top N (default 8) | floor = max r outside ±250 ms of the top peak; prominence = top − floor |
| `live` | `peak_r ≥ 0.5` and `prominence ≥ 0.10` | census derived column; findings §6.7 “confident” bar |
| `unique_strict` | `peak_r ≥ 0.8` and same prominence | recorded, not used to *define* `live` |
| `bridge_ms` | `gap_ms + post_lag_ms − pre_lag_ms` | dual-fit B span; `gap_ms` is WAV-derived from the same `ctx` that placed the throats (§4.1), not dump `duration_secs` |
| `collapsed_bridge` | both `live` and `\|bridge_ms\| ≤ 50` | 31/g7 measured −3..+6 ms; 50 ms sits inside the unpopulated 250 ms agreement band |
| `shared_far_offset` | both live, `\|pre−post\| ≤ 250 ms`, `max(\|lag\|) > 600 ms` | 31/g10 class |
| `shared_near_false_dead` | both live, lags agree within 250 ms, in reach | the 12-gap sensor-artifact bucket the map-coordinates fix cleared |
| `real_step` | both live, lags disagree | leftover both-live class |
| `one_live` / `none_live` | not both live | |

Non-chained: each throat is searched independently. Do not feed pre’s lag into post’s haystack
origin — that chaining is what the map-coordinates fix had to undo on the production sensor.

`edge_pinned` if `|peak_lag| ≥ max-lag-ms − 20`, and `clipped_haystack` / `usable_lag_ms` when B
is shorter than the requested sweep. Carry these on the JSON row so a corpus read can see a
window-edge lock without re-running.

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
```

`--shoulder` is required in single-gap mode and forbidden with `--corpus`. Corpus always
sweeps pre and post. Corpus discovery matches the batch script and `listen-registration`’s
`collect_gaps`: numbered pair subdirectories **or** a flat directory of `*_a_surround.wav` +
sibling B + sibling `stem.json`. `--out` defaults to `<corpus>/wide-sweep/`. `--also` appends
extra listen dirs (the Python used this for a one-gap overlay of 31/g7).

Stdout for one shoulder keeps the prototype log shape (shoulder, sr, gap, template time,
r at lag 0, peak table, floor/prominence) so existing notes stay comparable. Corpus mode prints
the bucket roll-up plus one summary line per gap (pair, 1-based index, bucket, lags, r,
`bridge_ms`) and writes:

- `census.csv` — the joinable column set the batch script already used
- `census.json` — same rows + `pre_peaks` / `post_peaks` + a `_contract` object that quotes
  the instrument, window, `bridge_ms` formula, and every derived cut (`live`,
  `collapsed_bridge`, …)

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
| `classify`: both live, `bridge_ms` ≈ 0 → `collapsed_bridge` | derived column |
| `classify`: both live, lags agree, `\|lag\| > 600` → `shared_far_offset` | derived column |
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
three-bucket split (12 `shared_near_false_dead`, 1 `collapsed_bridge`, 1 `shared_far_offset`)
**only** if those WAVs were produced by a pre-map-coordinates binary — on a current dump those
12 are no longer dead. Do not treat a current-binary re-run of that tree as a regression of
this tool.

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
4. **Corpus mode.** Discover pairs, both shoulders, classify, write `census.csv` /
   `census.json` `_contract`, join dump lags + `listen.json` disposition when present.
   Classify unit tests.
5. **Docs + feature-off build + PR wiring.** `development.md` / Cargo.toml comment /
   fingerprint pointer. Confirm default `cargo build` does not emit `shoulder-sweep`. Add
   `--bin shoulder-sweep` to the `pr-repair` calibration `cargo test` invocation
   (`scripts/test-tier.ps1`).
6. **Media reconstruction.** 31/g7 pre landmark (§6). Record the numbers in the PR
   description (pair + gap index only). If the listen tree is gone, regenerate rather than
   skipping.

---

## 9. Out of scope / parked

| Item | Why parked |
|---|---|
| Sliding patched-vs-B NCC (`scratch_patch_registration_sweep.py`) | Different question (does the *patch* hold one lag). Same bin later as a third mode if it earns a second user. |
| PowerShell wrapper under `scripts/measure/` | Operators already `cargo run --bin listen-registration`; do not invent a third invocation style. |
| Calling production `anchor_eligible` / JSON-only censuses | Different scripts; `scripts/measure/census-residual-measured.ps1` is the pattern. |
| Baking the sweep into `--gap-fingerprints` / `--gap-listen` | Post-hoc on listen WAVs so the window is a knob and the estimator stays off the dump schema. See §2. |
| Widening `--lag-envelope-max-lag-ms` or shipping a `collapsed_bridge` skip | BACKLOG’s next Change; this bin is the instrument that Change will re-run, not the Change. |
