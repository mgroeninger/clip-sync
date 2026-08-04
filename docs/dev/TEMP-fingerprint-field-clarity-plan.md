# TEMP — Fingerprint field clarity (rename + co-located contracts)

**Status:** draft plan, 2026-08-04. Working plan for making `--gap-fingerprints` dumps
harder for agents (and humans) to misread by (1) renaming high-confusion wire fields, then
(2) co-locating short measurement contracts next to the values they describe.

Companion: [gap-fingerprint.md](gap-fingerprint.md) § Shape / § *`equivalence` vs
`scan_equivalence`*, [gap-vocabulary.md](gap-vocabulary.md), harness
`gap_fingerprint_corpus/report.rs` `legend_text()`, curated fixtures under
`crates/clip-sync-repair/tests/gap_corpus/fingerprints/`.

**Why not a Cursor rule alone:** other harnesses (and agents outside Cursor) read the same
JSON. Clarity must travel with the dump.

**Media hygiene:** unchanged. Fingerprint JSON stays sample-free; no titles/paths.

---

## 0. Problem

Agents (and casual readers) treat **field names as the schema**. They invent meanings from
names like `equivalence`, `baseline_lag`, and `donor_interior` without opening
`gap-fingerprint.md` or `legend_text()`.

A corpus-root `_notes` map is too easy to skip. Renames fix the first fork (which field is
authoritative / which placement). Co-located contracts fix the second fork (window /
placement / what it is *not*).

This is the same class of failure that forced the 2026-07-31 retirement of “fine” / “coarse”
wording — and the replacement names still invite a wrong default story.

---

## 1. Rename recommended `GapFingerprint` wire fields

### 1.1 Goal

Put the **production vs diagnostic** and **decision vs editorial / aligned vs nominal** axes
into the JSON keys themselves, so any harness that only sees field names gets a better default.

### 1.2 Rename table (wire JSON + Rust struct fields)

| Current wire / Rust field | New name | Role encoded in the name |
|---------------------------|----------|--------------------------|
| `equivalence` | `equivalence_diagnostic` | Second opinion only; nothing in plan/patch reads it |
| `scan_equivalence` | `equivalence_production` | Authoritative; what `skip_equivalent_gaps` acts on |
| `lag` | `lag_editorial` | Tier-3; best-structure / editorial-bracket placement |
| `baseline_lag` | `lag_decision` | Decision registration at `b_mapped` |
| `donor_interior` | `donor_interior_aligned` | Occupancy on the lag-adjusted bridge span |
| `donor_interior_nominal` | *(unchanged)* | Already explicit; keep |

**Rejected alternatives**

- `equivalence_authoritative` — longer; “production” matches existing docs vocabulary.
- Dual-write old + new keys forever — reintroduces the ambiguous short name on every new dump.
- Renaming `GapReport::gap_equivalence` / domain types — out of scope; only the fingerprint
  **export** keys need to stop lying. Production Rust can keep current names.

### 1.3 What renaming does *not* fix

Window and placement details (“measured on shoulders, never in the gap”; “±600 ms search”;
“throat vs `b_mapped`”). Those need §2 contracts (or docs). Renames only stop the first wrong
fork.

### 1.4 Compatibility

Old corpora and committed fixtures must still load.

| Direction | Policy |
|-----------|--------|
| **Write** | New keys only |
| **Read** | Accept old keys via `#[serde(alias = "...")]` on each renamed field |
| **Dual-write** | No |
| **`curated.golden.json`** | No wire rename — it is a derived `GoldenBaseline` projection (`gross_*`, `aligned_donor_*`, …), not a `GapFingerprint` dump. Regenerating after fixture key updates should be bit-stable if analyzer predicates are unchanged |

Harness `gap_fingerprint_corpus` already projects a **minimal** schema and tolerates unrelated
`GapCorpus` drift; still update its field names / aliases so it prefers the new keys and accepts
the old ones.

### 1.5 Code change checklist

Execute in this order so round-trips stay green.

1. **Schema (`gap_fingerprint/schema.rs`)**
   - Rename the five fields on `GapFingerprint`.
   - Add `#[serde(alias = "<old>")]` on each (and keep `skip_serializing_if` / `default` behavior).
   - Update rustdoc on those fields; point the equivalence pair at the new § title in
     `gap-fingerprint.md`.
   - Add / extend a serde round-trip test: deserialize **old** JSON → serialize **new** keys;
     deserialize new JSON unchanged.

2. **Writers / projectors**
   - `measure.rs` — assignments to `fp.equivalence` / `fp.scan_equivalence` / lag / donor fields.
   - `project.rs` — `GapRepairSpec` → `GapFingerprint` projection and any
     `PROJECTED_BASELINE_LAG_*` / fabricated-lag path names that mention `baseline_lag`.
   - Domain comments that say “stored as `donor_interior`” etc. (`gap_repair_spec.rs`,
     `donor.rs`) — update to the new wire names where they document the dump.

3. **Consumers (Rust identifiers + JSON paths)**
   - `equivalence_calibration` binary — all `fp.equivalence` / `fp.scan_equivalence` reads;
     banner/docs that say “diagnostic vs production”.
   - `listen_registration` — `scan_equivalence.donor_registration` paths.
   - Harness `gap_fingerprint_corpus/{analysis,check,schema,report}.rs` — struct fields,
     `check_scan_equivalence_coverage` naming (consider
     `check_equivalence_production_coverage`), `legend_text()` strings.
   - Tests: `equivalence_divergence.rs`, `curated_fixture_backfill.rs`, any JSON path asserts.

4. **Committed fingerprint fixtures (must update keys)**
   - `tests/gap_corpus/fingerprints/curated/*.json` (not all files carry every field;
     grep for each old key).
   - `tests/gap_corpus/fingerprints/equivalence_divergence/band_donor.json`
   - `tests/gap_corpus/fingerprints/g003_timing_offset.json` (if it carries renamed keys)
   - Prefer a small scripted rewrite (jq / PowerShell) over hand edits; then re-run
     `golden_baseline_invariance` (expect green without `CURATED_GOLDEN_REGEN` if only keys
     changed).

5. **Do not change**
   - `GapReport::gap_equivalence` / scan-path domain API (unless a follow-up explicitly wants it).
   - Archive docs under `docs/dev/archive/` (historical; optional one-line “formerly …” only if
     a live doc still links a section title that moves).
   - CLI alignment/repair JSON goldens (`full_surface_*.json`) — different contract.

### 1.6 Document change checklist

| Doc | Edit |
|-----|------|
| [gap-fingerprint.md](gap-fingerprint.md) | Shape table; rename § *`equivalence` vs `scan_equivalence`* → *`equivalence_diagnostic` vs `equivalence_production`*; Registration & dual-fit subsections for lag/donor; any “formerly `baseline_lag`” note for one release cycle |
| [gap-vocabulary.md](gap-vocabulary.md) | Silence-character pre-gate / fixture mapping that cites the old pair |
| [json-output.md](../json-output.md) | Fingerprint dump note that names the authoritative field |
| [docs/dev/README.md](README.md) | Link text that still says `` `equivalence` vs `scan_equivalence` `` |
| Binary module docs | `equivalence_calibration.rs` header; `listen_registration.rs` comments |
| Harness `legend_text()` | Use new names so agent-facing text matches dumps |
| This TEMP | Mark §1 done when shipped; archive after durable docs absorb it |

**Archive policy:** do not rewrite archived TEMP findings to the new names. Live docs get a
short “formerly known as” where a rename would strand search (`baseline_lag` →
`lag_decision`, `scan_equivalence` → `equivalence_production`).

### 1.7 Phasing

| Phase | Scope | Exit |
|-------|--------|------|
| **R1** | Equivalence pair only (`equivalence` / `scan_equivalence`) | Serde aliases + fixture rewrite + live docs; calibration + divergence tests green |
| **R2** | Lag pair (`lag` / `baseline_lag`) | Same pattern; legend + Registration section updated |
| **R3** | `donor_interior` → `donor_interior_aligned` | Same pattern; golden axes already say `aligned_*` — keep that vocabulary consistent |

R1 is the highest agent-damage fix and the smallest blast radius. Do not bundle R2/R3 into R1
unless review wants one schema bump.

### 1.8 Verification

```powershell
# Schema + unit round-trips
cargo test -p clip-sync-repair gap_fingerprint

# Divergence fixture still loads (aliases or rewritten keys)
cargo test -p clip-sync-repair --test equivalence_divergence

# Curated golden stays bit-stable after fixture key rewrite
cargo test -p clip-sync-repair --test golden_baseline_invariance

# Harness checks that mention equivalence coverage
cargo test -p clip-sync-repair-harness
```

Manual: open one rewritten curated JSON and confirm **only** new keys are present (no dual-write).
Load one **unmodified** old corpus dir (if available under `gap-files/`) and confirm aliases still
deserialize.

---

## 2. Co-locate definition with the value (strongest data fix)

### 2.1 Goal

When a reader (agent or human) opens a metric **object**, the contract for that measurement is
in the **same JSON object** as the numbers — not in a distant root map, not only in
`gap-fingerprint.md`.

Renames (§1) answer “which field?”. Contracts answer “what / where / over what window / what
this is *not*?”.

### 2.2 Why root `_notes` is not enough

- Easy to skip in a large `corpus.json`.
- Far from the value being interpreted.
- Free-form 80-char glosses cannot carry the failure mode this corpus actually has:
  **wrong placement / wrong window** (see band / donor-registration findings).

### 2.3 Contract shape

Add an optional `_contract` object on selected **metric groups** (not on every scalar):

```json
"lag_decision": {
  "_contract": {
    "measures": "per-shoulder waveform lag at the decision seam",
    "placement": "b_mapped (not throat, not editorial bracket)",
    "window": "1s border; ±600ms search; post sequentially centered",
    "not": "lag_editorial (editorial) or residual (throat)"
  },
  "pre": { "peak_r": 0.91, "peak_z": 14.2 }
}
```

**Fixed keys** (structured, not prose blobs):

| Key | Meaning |
|-----|---------|
| `measures` | What the numbers are |
| `placement` | Where on the timeline / which seam geometry |
| `window` | Border, search radius, binning — whatever is load-bearing |
| `not` | Common confusable sibling(s) |

Keep each string **short** (target ≤100 chars; hard cap ≤120). Prefer the four keys over a
single essay field so agents cannot “summarize away” placement.

### 2.4 Which groups get `_contract` (priority)

Emit on write for groups that agents already confuse. Start small:

| Group (post-§1 names) | Why |
|------------------------|-----|
| `equivalence_diagnostic` | Not authoritative; same classifier, residual instrument differences |
| `equivalence_production` | Authoritative production verdict |
| `lag_decision` | Decision registration at `b_mapped` |
| `lag_editorial` | Tier-3 editorial placement (when present) |
| `donor_interior_aligned` | Lag-adjusted bridge |
| `donor_interior_nominal` | Geometry span, registration-independent |
| `residual` | Throat / decision-seam cancellation — not donor interior |
| `seam_probe` | Tier-3; not used by any gate |

Defer: `geometry`, `levels`, `brackets`, `outcome` (either low confusion or already
well-named). Expand only when a misread shows up in practice.

### 2.5 Schema / serde design

- Introduce a small shared type, e.g. `MetricContract { measures, placement, window, not }`
  (all `String`, or `Option<String>` if a key is N/A).
- On each opted-in group: `#[serde(rename = "_contract", skip_serializing_if = "Option::is_none", default)] pub contract: Option<MetricContract>`.
- Leading underscore: convention for “reader metadata”; harness minimal parsers that ignore
  unknown fields stay fine; anything using `deny_unknown_fields` on these objects must allow
  `_contract`.
- **Populate on write** from a single source of truth (const table or `fn contract_for_*()`),
  ideally the same wording fed into `legend_text()` so dump and analyzer legend cannot drift.
- **Deserialize:** `None` when absent (old corpora). Never require `_contract` to classify or
  gate.

### 2.6 Corpus size and repetition

`_contract` is identical for every gap of a given field. Options:

| Approach | Pros | Cons |
|----------|------|------|
| **A. Repeat per gap object** | Maximum agent resistance; definition always beside value | Larger dumps |
| **B. Corpus-level `contracts` map + per-gap `$ref`** | Smaller | Agents skip the map; refs are easy to ignore |
| **C. Hybrid: full `_contract` on first gap only, `$ref` after** | Smaller | Fragile; tools that slice one gap lose the def |

**Recommendation:** **A for opted-in groups** on `--gap-fingerprints` dumps. These dumps are
diagnostic, not a hot path; clarity beats bytes. If size becomes an issue, add a write flag
`--fingerprint-contracts=once|always|off` later — default `always`.

Do **not** put contracts only in a sidecar unless a consumer workflow *forces* opening it
(other harnesses will not).

### 2.7 Relationship to `legend_text()` and docs

| Layer | Role after this work |
|-------|----------------------|
| Wire names (§1) | Which field / which axis |
| `_contract` (§2) | Inline what / where / window / not |
| `legend_text()` | Human CLI roll-up; must stay consistent with contract strings |
| `gap-fingerprint.md` | Full provenance, history, open residuals — not replaced by `_contract` |

`_contract` is a **reminder**, not a substitute for the Registration & dual-fit section.

### 2.8 Phasing relative to renames

| Phase | Work |
|-------|------|
| **C0** | Spec only (this section); no code |
| **C1** | After **R1**: add `MetricContract` + `_contract` on the two equivalence verdict objects |
| **C2** | After **R2/R3**: contracts on lag + donor groups (+ `residual` / `seam_probe` as listed) |
| **C3** | Optional: share string table with `legend_text()`; optional `--fingerprint-contracts` knob |

Do not ship C1 before R1 if the contract `not:` strings would still cite old field names —
rename first, then freeze contract text against the new names.

### 2.9 Verification

- Round-trip: new dump has `_contract`; old dump without it still deserializes.
- Spot-check: one curated fixture either gains contracts on regen, or stays without them and
  still passes (contracts are write-time metadata, not golden Tier-1/2 axes).
- Agent smoke (manual): ask a model to interpret one gap **from JSON alone** and confirm it
  cites placement/window from `_contract` rather than inventing “baseline” / “the equivalence”.

### 2.10 Non-goals

- JSONC / `//` comments (breaks `serde_json` consumers).
- Per-scalar `peak_z_comment` siblings (schema noise; group-level is enough).
- Making `_contract` authoritative for gates or calibration diffs.
- Cursor-only rules as a substitute for dump clarity (may still exist as a supplement; not
  required for this plan’s success).

---

## 3. Suggested ship order

1. Land **R1** (equivalence rename + aliases + fixtures + live docs).
2. Land **C1** (contracts on the two equivalence objects) while the rename is fresh.
3. Land **R2** / **R3**, then **C2**.
4. Promote durable bits into `gap-fingerprint.md`; archive this TEMP.

---

## 4. Resolved decisions

1. **Rust field names match wire** (`equivalence_production`, etc.). No short Rust idents +
   `#[serde(rename = "...")]` for the renamed fields — keep dumps and code greppable as one
   vocabulary.
2. **Immediate CLI cutover** to the new names in `listen_registration` / calibration tables /
   banners / `legend_text()`. Aliases only on deserialize; no one-release dual label in CLI text.
3. **Leave curated fixtures contract-free through C1.** `_contract` is write-time metadata on
   live `--gap-fingerprints` dumps only until an intentional fixture harvest; golden invariance
   must not depend on those strings.
