# `shoulder-sweep` — the independent listen-WAV shoulder sweep

**Live doc — current behaviour.** `crates/clip-sync-repair/src/bin/shoulder_sweep.rs`
(`[[bin]] shoulder-sweep`, `required-features = ["calibration"]`) is written against §2 and §3
below, and `pr-repair` pins them (§4). Shipped 2026-08-21. Change this document and the bin
together; if they disagree, one of them is a bug.

**What it measures.** Given a `--gap-listen` pair of `_a_surround.wav` / `_b_surround.wav`: where a
1 s throat-adjacent A template actually locks in B, at every integer-sample lag in ±`--max-lag-ms`
(default 2000) — past production's ±600 ms `lag_decision` window. Both shoulders of a gap,
independently. It **measures and records; it does not classify**: no buckets, no per-pair verdict,
no skip recommendation.

**Independence rule.** Production `lag_decision` (fine ±600 ms, envelope rescue ±2000 ms at 100 ms
bins) is the thing under test on far-offset gaps. The sweep therefore lives in the bin and must not
call `seam_local::lag_correlation_curve_fft`, `normalized_correlation`, or `register_donor_window` —
the same constraint `listen-registration` documents. Sharing WAV I/O with that bin is fine; WAV I/O
is not the estimator under test.

**Media hygiene.** Print pair directory names, gap indices and file stems only — never media paths
or titles. Census files use `entry_stem`s, and outputs land in gitignored `gap-files/`.

**Siblings.** `listen-registration` is a registration *check* (pre shoulder only, ±1 s, vs the
engine lag). `gap-fingerprint-stats` is dump/oracle analysis. This is the WAV-domain wide-window
instrument; keeping the three separate is deliberate.

**Plan record** — why it was built this way, the classifier that was designed and cut, and what
stays parked:
[archive/TEMP-shoulder-sweep-bin-plan.md](archive/TEMP-shoulder-sweep-bin-plan.md). Section map for
older citations: that plan's §1.1 → §1 here, §4 → §2, §4.1–§4.4 → §2.1–§2.4, §5 → §3. Its §2, §3,
§4.5 and §6–§9 stayed with the plan record.

---

## 1. What `bridge_ms` is, and why it is not a class

Earlier versions of this instrument derived a `collapsed_bridge` bucket from `|bridge_ms| ≤ 50`.
The collapsed-bridge census
([TEMP-collapsed-bridge-census-2026-08-20.md](archive/TEMP-collapsed-bridge-census-2026-08-20.md))
retired that predicate as a *class*:

- **Result 7** — `bridge_ms = gap_ms + step`, where `step = post_lag − pre_lag`. So
  `|bridge_ms| ≈ 0` fires exactly when a pair's map step happens to equal minus that gap's
  duration — in the one corpus gap that ever fired it, a −1121.9 ms step against a 1118.8 ms gap,
  3.1 ms apart. **No positive step can ever satisfy it** at any gap duration, so the corpus's
  known ≈ +2.8 s step is structurally invisible to it. The invariant underneath is the **step
  boundary**: a gap whose pre and post sit on opposite sides of a pair-level map step.
- **Result 8** — the step boundary is not measurable from `lag_decision` dumps at the current
  search radius. Over 935 admitted rows the largest trustworthy step in the whole corpus is
  63.9 ms; both known large steps (≈ −1.15 s and ≈ +2.8 s) are censored, because a step past the
  search radius destroys the correlation that would measure it. The censoring is a
  **radius** effect, not a structural one — which is why a wider-envelope re-fingerprint, not this
  bin, is the cheap route to a base rate ([plan record](archive/TEMP-shoulder-sweep-bin-plan.md) §9).

So `step_ms` and `bridge_ms` are **recorded as columns**, and nothing in the bin branches on them.
`step_ms` is the quantity Result 7 identified as the invariant, and is worth having on every row;
`bridge_ms` is retained only so old census files stay comparable, with a `_contract` line saying it
is a coincidence and not a class.

---

## 2. Instrument contract

**This section is the specification, and it is meant to be complete** — the bin can be written,
and rewritten, from this document alone. Lag 0 is the nominal map at the chosen throat, same units
and sign as `DonorRegistration::lag_ms` / `listen-registration` (`wav_lag`): **negative means B's
matching content sits earlier than the nominal map put it.**

| Knob | Default | Meaning |
|---|---|---|
| Template | 1.0 s of A, adjacent to the throat | pre: the last 1.0 s *before* the throat; post: the first 1.0 s *after* it |
| Throat | recovered from clip length and dump geometry, not assumed | see §2.1 — a listen run made with a non-default pad still measures its own throats |
| Haystack | B surround, mono downmix (mean of all channels) | same reduction as `listen-registration`. Do **not** drop LFE — that is a different measurement's convention and would change every `r` here |
| Search | every integer-sample lag in `±max-lag-ms` | default 2000; FFT Pearson + prefix-sum variance, not 1 ms naive steps |
| Peak pick | 250 ms exclusion radius, top N (default 8) | greedy: take the highest r, then the next highest more than 250 ms from every peak already taken |
| Floor / prominence | floor = max r outside ±250 ms of the **top** peak; prominence = top − floor | the "is this a single spike" number. **Both are `Option`** — see the degenerate case below |
| `peak_z` | `(peak_r − mean r) / stddev r` over the whole swept curve | recorded, defines nothing. `None` when the stddev is ≤ `1e-12`. Carried because existing census CSVs have `pre_z` / `post_z` and dropping the columns would break the join (§2.3) |
| `live` | `peak_r ≥ 0.5` **and** `prominence ≥ 0.10` | derived column; findings §6.7 "confident" bar. A `None` prominence is **not** `live` |
| `unique_strict` | `peak_r ≥ 0.8` and same prominence | recorded, does not define `live` |
| `step_ms` | `post_lag_ms − pre_lag_ms` | the shoulder disagreement — Result 7's invariant. Recorded on every corpus row; **nothing branches on it** (§2.3) |
| `bridge_ms` | `gap_ms + step_ms` | comparison column only. `gap_ms` is WAV-derived from the same `ctx` that placed the throats (§2.1), not dump `duration_secs` |

Non-chained: each throat is searched independently. Do not feed pre's lag into post's haystack
origin — that chaining is what the map-coordinates fix had to undo on the production sensor.

**Degenerate floor.** When no swept lag lies outside ±250 ms of the top peak — a short
`--max-lag-ms`, or a clipped haystack (below) — there is nothing to take a floor over. Emit
`floor_r: null` / `prominence: null` rather than inventing a value, and `live` / `unique_strict` are
then `false` by the "`None` prominence is not `live`" rule above. Carrying it as `Option` rather
than a NaN is deliberate: no comparison should be able to return `false` silently through a NaN.

**`edge_pinned` is measured against the range actually swept, not the requested one.** With no
clipping that is `|peak_lag| ≥ max-lag-ms − 20`. When B is shorter than the requested sweep the bin
sets `clipped_haystack` and records `usable_lag_ms` (the `[lo, hi]` actually searched, printed as
a note),
**and `edge_pinned` then tests the peak against those usable bounds** — within 20 ms of either end.
Comparing a clipped sweep's peak to `max-lag-ms` is the failure mode this rule exists to prevent:
the peak sits on the real window edge, `|peak_lag|` is nowhere near `max-lag-ms`, and the
`sweep_may_be_narrow` guard below never fires on the one row whose reading is invalid.

Hard error, not a warning, when A and B sample rates differ. Resampling is out of scope and a
silent mismatch would corrupt every lag.

**`--max-lag-ms` and the `live` cut interact, and the default does not reach every known offset.**
The corpus contains at least one pair whose true offset is ≈ +2.8 s: at the default 2000 ms neither
of its shoulders' real peaks is inside the sweep, so both read as not `live` when in fact neither
was searched far enough. Two guards:

- Corpus mode **warns on stderr and sets `sweep_may_be_narrow`** on any row where a shoulder is
  not `live` *and* its best peak is `edge_pinned`. That is the signature of a true peak outside the
  window, and it is the only thing distinguishing "measured dead" from "not searched far enough."
- Do not raise the *default* past 2000. It is the value every census file on disk was measured
  at, so a silent change would make old and new files non-comparable. Raise it per run instead
  (`--max-lag-ms 3000` for a pair known to sit further out) and record that you did; `_contract`
  carries the value actually used.

### 2.1 Throat / context recovery

A fixed 3.0 s context (`gap_signature_context_secs`'s default) is an assumption, not a
measurement: a listen run exported with a different pad would then be measured at the wrong
throats. **The bin recovers `ctx` instead**, from clip length and dump geometry, as
`listen-registration` does. On a default-pad tree the recovered value and the flag agree exactly.

**`ctx` source (precedence).** `--context-secs` always exists and defaults to 3.0. When a sibling
dump JSON is present, **recovered geometry wins**; the flag is only the no-JSON fallback (it does
not override a dump). Sibling JSON is `stem.json` next to `stem_a_surround.wav` — strip
`_a_surround.wav`, append `.json`.

```text
span = geometry.a_end_secs − geometry.a_start_secs
ctx  = (a_samples / rate − span) / 2     # recovered from A; used for both clips
```

Without a dump, `ctx = --context-secs` (default 3.0).

**Each throat is placed on its own clip, not at the same sample index.** Listen A/B can differ in
length because
`frame_range` clamps at media edges even though the export aims at the same ± context. Lag-0 origin
on each clip, with `m` = template length in samples:

```text
pre  throat: at ctx samples from the START of that clip.
             A template starts at ctx − m  (the window START is the lag-0 origin)
             B haystack lag 0 at ctx − m
post throat: at ctx samples from the END of that clip.
             A template starts at len(A) − ctx; B haystack lag 0 at len(B) − ctx
```

Do not place B's post throat at A's post sample index. When lengths match the two are identical;
when they don't, end-relative placement is the one the listen export's "pad each timeline by
context" implies, and the only one that puts lag 0 on the nominal map.

**`gap_ms` for `bridge_ms`** uses that same `ctx`:

```text
gap_ms = (len(A) / rate − 2 · ctx) × 1000
```

Dump `geometry.duration_secs` is a comparison column on the row (`gap_secs_dump`), not the bridge
input: the gap is WAV-implied, with the dump's duration joined beside it.

`--gap-listen` may omit `_b_surround.wav` when the gap never reached the fill plan. Skip that
stem with a stated reason (pair + gap index); do not invent a B clip.

### 2.2 Independence from production correlation

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
  sweep is `b_anchor` (§2.1), so `lag_ms = (k + clipped_lo − b_anchor) / rate × 1000`.
- **`rustfft`'s inverse is unnormalized**, unlike the inverse real transforms some FFT libraries
  provide. Scale the inverse by `1 / size`, or the whole curve is off by a factor of `size` and
  every `r` overflows the `live` cut. `rustfft` has no real-input transform, so run the complex FFT on both
  signals with zero imaginary parts and take the real part of the inverse.

WAV decode may be copied from `listen-registration`'s `read_mono` (hound, all sample formats that
listen actually writes: 16-bit, 24-bit packed, 32-bit). Sharing WAV I/O with that bin is allowed —
WAV I/O is not the estimator under test. Extracting a calibration-gated lib helper is optional and
not required; duplicating ~40 lines is the same choice `listen-registration` already made versus
production.

Do not add a `csv` crate. `census.json` is the durable record; `census.csv` is a simple
fixed-column write of the same rows (stems + numbers, no titles, so quoting is trivial).

### 2.3 Derived columns — and why there are no buckets

Every corpus row carries `live`, `unique_strict`, `peak_z`, `step_ms`, `bridge_ms`, `edge_pinned`,
`clipped_haystack` / `usable_lag_ms`, `sweep_may_be_narrow`, `source_tree`, plus the joined dump
columns of §3. **Nothing in the bin branches on `step_ms` or `bridge_ms`.**

**Column parity with existing census files.** Census CSVs already on disk carry the header
`pair, fingerprint_gap_1based, dump_g, a_start_secs, listen_disposition, gap_ms, pre_lag_ms, pre_r,
pre_z, pre_prom, pre_live, post_*, bridge_ms, bucket, dump_pre_ms, dump_pre_r, dump_pre_verdict,
dump_post_*, stem`. The bin keeps all of them except `bucket` (cut with the classifier) and adds
`step_ms`, `unique_strict`, `edge_pinned`, `clipped_haystack`, `usable_lag_ms`,
`sweep_may_be_narrow`, `source_tree`. `pre_z` / `post_z` in particular are **kept** — they are two
accumulators over a curve already in hand, and census CSVs on disk carry them, so dropping them
would silently break a join against those files. Per-shoulder `sr`, `gap_secs`, `template_a_secs`
and `lag0_r` stay out of the CSV: they belong to the `<stem>_pre.txt` / `<stem>_post.txt` logs and
`census.json`. The dump's own prominence and z-score are deliberately **not** carried across the
join either — the dump has them, the census does not.

The first two revisions of the plan record specified a six-bucket classifier (`step_boundary`,
`step_boundary_unconfirmed`, `step_excursion`, `shared_far_offset`, `shared_near_false_dead`,
`one_live` / `none_live`) over a per-pair persistence test. It was cut before implementation: the
rule it needs cannot return a boundary on any listen tree that exists, so the whole ladder would
ship as a dead path emitting `null`. Its canonical home is
[archive/TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md) §4–§5; do not
re-derive it, and see the [plan record](archive/TEMP-shoulder-sweep-bin-plan.md) §9 for the
condition that un-parks it. On a tree of a few dozen gaps, rows carrying `step_ms` are enough for
a human to classify by eye, and they are exactly the input a real classifier would consume later.

`|bridge_ms| ≤ 50` in particular must never become a bucket again — Result 7 (§1). If it appears
at all it is a boolean annotation beside `step_ms`, and `_contract` states in one line that it is an
arithmetic coincidence and not a class, so a future reader of an old `census.json` does not
resurrect it.

### 2.4 The stdout summary block

One stdout summary block, no verdicts:

- gaps swept / gaps skipped (with reasons), and `live` shoulder count
- the `live_gaps / total_gaps` denominator, followed by the caveat **in words**: this is gaps
  with listen WAVs, not gaps in the corpus, and nothing here is a base rate
- any row with `sweep_may_be_narrow`, listed by pair + gap index, since that is the one condition
  that invalidates a reading rather than merely describing it

---

## 3. CLI, discovery and outputs

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
    --corpus gap-files/<flat-listen-dir> --pair <index>
```

`--shoulder` is required in single-gap mode and forbidden with `--corpus`. Corpus always sweeps
pre and post.

**Discovery reuses `listen-registration`'s `collect_gaps` semantics.** That function scans
`*_a_surround.wav` in the root itself (labelled `.`) **and** in each immediate subdirectory
(labelled by its directory name), sorted — one level deep, not recursive. Depth 1 covers every
layout `--gap-listen` produces (a run directory of numbered pair subdirectories, or a single flat
pair directory), and a deeper walk would only invent pairs out of nested output directories such as
`wide-sweep/`. Sibling `_b_surround.wav` and `stem.json` come from the same directory as the A
clip.

**Pair labels.** `collect_gaps` labels a flat root `.`, which is wrong when `--also` points at one
pair's directory inside a run — that row has a real pair index, and a census that labels it `.`
cannot be joined or merged. **The pair index exists nowhere in the artifacts**: `GapCorpus.source`
carries `FileSource.id` (a content hash), `listen.json` carries `a_id` / `b_id` / `stem` / `index`,
and neither carries a pair — the only other candidate is a media path, which
the media-hygiene rule forbids reading into a label. So the resolution chain is directory- and
operator-supplied, in this order:

1. the subdirectory name, when `collect_gaps` labels the row by one (the normal run-of-pairs case);
2. for `.`-labelled rows, `--pair <label>` — the supported way to label a flat pair directory whose
   name is not the index;
3. for `.`-labelled rows with no `--pair`, the **root directory's own name** when it parses as a
   pair index;
4. failing that, the **root directory's own name as it stands**, with a one-line stderr note saying
   it is not an index — never a guess dressed as one. A run directory's name identifies rows in a
   merged census where a bare `.` cannot, and the note is what keeps a reader from quoting it as a
   pair. `.` survives only for a root with no name at all.

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
`index`, and the dump's `gaps[0].index` all agree, and that is what every note and census means by
`<pair>/g<n>`. `fingerprint_gap_1based` is `index + 1` — a different number for the same gap, kept
as a column for join compatibility with existing census files and used **nowhere else**. Stdout,
the `<stem>_*.txt` logs and every stderr note print the dump `index`, unadorned; a row that prints
`g8` for gap index 7 is a bug, not a convention.

`--out` defaults to `<corpus>/wide-sweep/`. Rows are emitted sorted by `(pair, a_start_secs)` when
dumps are present; **`a_start_secs` comes from the dump, which §2.1 makes optional**, so rows with
no dump sort last within their pair by stem, and carry `a_start_secs: null`.

`--also` appends extra listen dirs and merges by pair index, so one pair's gaps may come from more
than one tree — a single gap re-exported on its own lands in a different directory from the rest of
its pair, and overlaying the two is the point of the flag. Record which tree each row came from
(`source_tree`); a reader comparing lags across a merged run must be able to see that without
re-deriving it.

**Dedupe: first root wins, by stem.** Roots are visited in order (`--corpus`, then each `--also` as
given); a stem already emitted is skipped, and the skip is one stderr line naming the stem, the tree
that won, and the tree that lost. The rule is a `seen` set over stems, and it is why `source_tree`
is a single value rather than a list — a stem exported into two trees is one row, from
the first tree, and the operator can see it happened. Same-stem-different-tree is not a merge: the
two exports can differ (different pad, different run), so averaging or preferring the later one
would silently mix instruments.

Stdout for one shoulder is a fixed log shape — shoulder, sample rate, gap, template time, r at
lag 0, the peak table, floor/prominence — held stable so notes written against earlier runs stay
comparable. Corpus mode prints one line per gap
(pair, dump gap index per the numbering rule above, both lags, both r, `step_ms`, `bridge_ms`,
`live`), then the §2.4 summary block, and writes:

- `census.csv` — the joinable column set (including `pre_z` / `post_z` and
  `fingerprint_gap_1based`; excluding `bucket`), plus `step_ms`, `live`,
  `unique_strict`, `edge_pinned`, `clipped_haystack`, `usable_lag_ms`, `sweep_may_be_narrow`,
  `source_tree` — the full list is §2.3
- `<stem>_pre.txt` / `<stem>_post.txt` — the single-gap stdout log for each shoulder, verbatim, so
  a corpus run leaves the same artifacts an operator would get by sweeping each gap by hand
- `census.json` — same rows + `pre_peaks` / `post_peaks` + a `_contract` object quoting the
  instrument, `max_lag_ms` **as actually run**, `context_secs` or "recovered", the `step_ms` /
  `bridge_ms` formulas, the `live` cut, the one-line statement that `bridge_ms ≈ 0` is a
  coincidence and not a class (§2.3), and the one-line statement that the denominator is *gaps with
  listen WAVs* and this file is not a base rate

Join dump `lag_decision` (pre/post `frac_lag_ms`, `peak_r`, `verdict`) and `listen.json`
`disposition` when those files exist — derived comparison columns, not inputs to the sweep.
`LagDecision::pre_anchor` / `post_anchor` are typed `Vec<LagSummary>`, so the join is `.first()`.

Relative dirs resolve against the **repo root**, same as `gap-fingerprint-stats`.


---

## 4. What CI pins

Colocated `#[cfg(test)]` tests in the bin — 26 of them, synthetic PCM only, never licensed media.
They cover the FFT Pearson against a naive reference (transform size, template reversal, `1/size`
scaling, `valid` slice), the sign convention, the DC-offset and zero-variance cases, the peak
picker's 250 ms exclusion, the floor/prominence definitions and their null case, `peak_z`, the
`live` cut, `edge_pinned` against the *usable* range, both throat anchors and the `--context-secs`
precedence, depth-1 discovery, the pair-label chain of §3, the `--also` dedupe, and row ordering.

They run on PR through the `pr-repair` calibration invocation in `scripts/test-tier.ps1`:

```powershell
cargo test -p clip-sync-repair --features calibration --bin shoulder-sweep
```

`--bin` is required — `autotests = false` means a bare `cargo test -p clip-sync-repair` never
reaches them. Feature-off `cargo build` must not emit the binary.

**Media acceptance — done, not a standing task.** The instrument was accepted against a landmark
reading that predates it (a unique peak at **−38.1 ms, r 0.8287, floor 0.0979, prominence 0.7308**
on a known gap), reproduced to four decimals on 2026-08-21 and re-confirmed on 2026-08-22. Licensed
media is required to run it, so it cannot be CI and cannot be reproduced from this repo alone; the
gap it uses, the tree it lives in, and the procedure are recorded in the
[plan record](archive/TEMP-shoulder-sweep-bin-plan.md) §6. Re-run it only if the numerics in §2.2
change — and if it ever disagrees, the discrepancy is the finding, not a reason to relax the
landmark.
