# TEMP — Fingerprint field clarity (rename + co-located contracts)

**Status:** **R1 + C1 + R2 shipped 2026-08-07**; R3 / C2 / C3 open. Working plan for making
`--gap-fingerprints` dumps harder for agents (and humans) to misread by (1) renaming
high-confusion wire fields, then (2) co-locating short measurement contracts next to the values
they describe.

Companion: [gap-fingerprint.md](gap-fingerprint.md) § Shape / § *`equivalence_diagnostic` vs
`equivalence_production`*, [gap-vocabulary.md](gap-vocabulary.md), harness
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
| [gap-fingerprint.md](gap-fingerprint.md) | Shape table; rename § *`equivalence` vs `scan_equivalence`* → *`equivalence_diagnostic` vs `equivalence_production`* **(R1 done)**; Registration & dual-fit subsections for lag/donor; any “formerly `baseline_lag`” note for one release cycle |
| [gap-vocabulary.md](gap-vocabulary.md) | Silence-character pre-gate / fixture mapping that cites the old pair **(R1 done)** |
| [json-output.md](../json-output.md) | Fingerprint dump note that names the authoritative field **(R1 done)** |
| [docs/dev/README.md](README.md) | Link text that still says `` `equivalence` vs `scan_equivalence` `` **(R1 done)** |
| Binary module docs | `equivalence_calibration.rs` header **(R1 done)**; `listen_registration.rs` comments **(R1 done)** |
| Harness `legend_text()` | Use new names so agent-facing text matches dumps — R1: `legend_text()` never named the equivalence pair, so nothing to change; the health-check messages in `check.rs` did and were updated |
| This TEMP | Mark §1 done when shipped; archive after durable docs absorb it |

**Archive policy:** do not rewrite archived TEMP findings to the new names. Live docs get a
short “formerly known as” where a rename would strand search (`baseline_lag` →
`lag_decision`, `scan_equivalence` → `equivalence_production`).

### 1.7 Phasing

| Phase | Scope | Exit |
|-------|--------|------|
| **R1** ✅ | Equivalence pair only (`equivalence` / `scan_equivalence`) | **Shipped 2026-08-07.** Serde aliases + fixture rewrite + live docs; calibration + divergence tests green |
| **R2** ✅ | Lag pair (`lag` / `baseline_lag`) | **Shipped 2026-08-07.** Aliases in all four parsers + `not_measured` path folding + fixture rewrite + live docs; workspace + calibration tests green, `curated.golden.json` bit-stable (see § 1.10) |
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

### 1.9 R1 as shipped (2026-08-07)

All of §1.5 landed as written. Deviations and things worth knowing before R2/R3 repeat the pattern:

- **No `#[serde(rename)]`.** Rust ident == wire key (§4.1), so only `alias` was needed. Adding
  `rename` too would have been redundant noise.
- **`check_scan_equivalence_coverage` → `check_equivalence_production_coverage`**, plus its two
  health-check *messages* and its two tests — the §1.5 "consider" resolved to yes. The harness's
  private `ScanEquivalence` **type** name was left alone: it is not agent-facing and renaming it
  buys nothing.
- **The harness's minimal parser needed its own alias.** `gap_fingerprint_corpus/check.rs` projects
  its own `GapEntry`, so the repair-crate alias does not cover it. R2/R3 must do the same for any
  renamed field the harness reads (`baseline_lag` was one — R2 found **four** such parsers, § 1.10).
- **CLI cutover included the calibration *table*,** not just prose: the column headers are now
  `production(<block>ms)` / `diagnostic` / `Δ(diagnostic−production)` and the verdict strings read
  "production drops, diagnostic keeps". The production column was widened 16 → 18 to fit.
- **`legend_text()` needed no change** — it never named the equivalence pair.
- **`curated.golden.json` was bit-stable** after the fixture key rewrite, with no
  `CURATED_GOLDEN_REGEN`, confirming §1.4's prediction.
- **Alias verified against real pre-rename media dumps**, not only synthetic JSON:
  `equivalence-calibration` on a `gap-files/` corpus from 2026-07-31 populated both verdicts.
  Two round-trip tests pin it (`legacy_equivalence_keys_deserialize_and_reserialize_renamed` in
  `schema.rs`, `legacy_scan_equivalence_key_still_parses` in the harness).
- **The "no dual-write" assertion needs care.** `equivalence_diagnostic` *contains* the old key as a
  substring, so a naive `!json.contains("equivalence")` check can never pass — assert on the quoted
  key (`"equivalence"`) instead.

### 1.10 R2 as shipped (2026-08-07)

`lag` → `lag_editorial`, `baseline_lag` → `lag_decision`. R1's pattern held; the three things R2 hit
that R1 did not are worth carrying into R3:

- **Four parsers needed the alias, not one.** Beyond `GapFingerprint`, the harness has *two*
  (`gap_fingerprint_corpus/analysis.rs`'s `GapEntry` and `check.rs`'s), and two integration tests
  project their own minimal structs (`w5_timing_offset.rs`, `diag_splice_timescale.rs`). Only the
  first was known from § 1.9. **Grep for the wire key across `tests/` too, not just `src/`** — a
  missed one is a silent `None`, which `w5_timing_offset` caught only because it asserts presence.
- **`source.not_measured` paths are *data*, and a serde alias cannot reach them.** The six
  `baseline_lag.*` strings live inside committed corpora. `check.rs` now folds them via
  `canonical_path()` before comparing, and `schema.rs` keeps
  `LEGACY_PROJECTED_LAG_DECISION_FIELDS` as a frozen transcription of the old spelling. The failure
  mode is the dangerous direction: an unrecognized declaration reads as *"this field was
  measured"*, so the health check goes quiet exactly where it should shout.
  **R3 has no equivalent** (`donor_interior` is not in `NOT_MEASURED_BY_PROJECTION`), but check
  before assuming.
- **Aliasing two keys onto two fields invites a copy-paste merge.** `legacy_lag_keys_...` asserts
  each old key lands on its *own* field via distinct `peak_r` values — a test that merely
  round-tripped would pass with both aliases pointing at one field.
- **`projected_lag_decision_paths_name_keys_the_type_emits`** ties the dotted literals to keys the
  types actually serialize, closing the gap the compiler leaves on string paths.
- **`curated.golden.json` was again bit-stable** after the 14-fixture key rewrite, with no
  `CURATED_GOLDEN_REGEN`. The fixtures were rewritten by **text splice** (key rename in place), not
  a typed round trip, which also preserves float bytes.
- **The pre-A2 legacy fixture in `gap_fingerprint_corpus/mod.rs` deliberately keeps `lag`.** It
  models a pre-A2 dump, so leaving it un-rewritten both keeps it honest and exercises the alias.

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
| **C0** ✅ | Spec only (this section); no code |
| **C1** ✅ | **Shipped 2026-08-07.** `MetricContract` + generic `Contracted<T>` + `_contract` on the two equivalence verdict objects — see § 2.11 |
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

### 2.11 C1 as shipped (2026-08-07)

Landed as `application/gap_fingerprint/contract.rs`: `MetricContract`, generic `Contracted<T>`, and
the const contract table. What §2 did not anticipate, and what C2 inherits:

- **The wrapper was mandatory, not stylistic.** §2.5 assumed a `contract` field could be added to each
  opted-in group's own struct. It cannot: `equivalence_diagnostic` and `equivalence_production` are the
  **same type** (`domain::gap_equivalence::GapEquivalenceVerdict`), which is also serialized on the
  production `--json` surface as `GapReport::gap_equivalence`. A field on it would give the pair one
  shared string and leak reader metadata into a domain type and into production output.
- **This recurs for C2, which is why the wrapper is generic.** `lag` / `baseline_lag` share
  `LagFingerprint` (fingerprint-local), and `donor_interior` / `donor_interior_nominal` share
  `DonorInterior` — **also a domain type** (`domain/donor.rs`, used by `dual_fit.rs` and
  `gap_repair_spec.rs`). Four of C2's six groups are pairs over a shared type. C2 should be a field
  *type* change (`Option<X>` → `Option<Contracted<X>>`) plus table entries — no new mechanism.
- **`#[serde(flatten)]` keeps the wire shape.** The group's keys stay where they were, `_contract`
  beside them, so adding a contract is not a schema break. Caveat for C2: flatten is incompatible with
  `deny_unknown_fields`, and it deserializes via a buffered map (fine for JSON, which is all we emit).
- **Read-site churn was contained by accessors**, not by touching every call site: `Contracted<T>`
  implements `Deref`, and `GapFingerprint::equivalence_{production,diagnostic}_verdict()` return the
  bare verdict. Only one call site was *forced* to change (`corpus_pairs` in
  `equivalence_calibration`, which names the type in its signature) — the rest compiled unchanged via
  deref coercion. They have since been moved onto the accessors anyway: **read sites should not know
  the wrapper exists**, and a half-converted file is how the next reader concludes it must care. C2
  should add the same accessor per renamed group and convert its readers in the same change.
- **Strings are `Cow<'static, str>`**, not `&'static str` — `&'static str` cannot `Deserialize`. Cow
  keeps the table `const` while letting a parsed corpus round-trip.
- **Contract text is validated, not just written**, and the validation is **registry-swept** so C2
  cannot bypass it. Contracts are declared through the `contracts!` macro, which enrols each one in
  `SHIPPED_CONTRACTS` as it declares it — a hand-maintained roster was the obvious second place for a
  C2 entry to be forgotten. Swept properties: `CONTRACT_MAX_LEN` (120); no two contracts identical;
  every `not` opens with a **registered sibling wire key** (see below). Add contracts via the macro,
  or they ship unchecked.
- **`not:` is sibling-first, and that is enforced.** C1 first shipped the diagnostic's `not` as
  *"authoritative: skip_equivalent_gaps reads equivalence_production…"* — a property, which only
  parses if the reader mentally prefixes the key, and a summarizing model will not. It now opens with
  the sibling's name like its counterpart. `every_not_names_a_sibling_first` pins the shape.
- **Text that quotes code is guarded against drift.** A `const` table cannot `format!`, so the ±2.0 s
  context window and the wire path `scan_recipe.scan_block_ms` are hardcoded prose.
  `quoted_constants_still_match_the_code` fails if `EQUIVALENCE_CONTEXT_SECS` moves or the recipe key
  is renamed. **C2/R2/R3 note:** any contract quoting a field path needs a line here, or a rename
  silently makes the contract lie — which is worse than having no contract.
- **Flatten was pinned against the committed bytes.** `Contracted`'s `#[serde(flatten)]` deserializes
  through serde's buffered `Content` rather than the struct's field visitor, so it could alter values
  and not merely add a key. `wrapping_in_contracted_did_not_disturb_the_verdict_wire_shape`
  (`tests/equivalence_divergence.rs`) round-trips `band_donor.json` and asserts every key the live
  type models comes back byte-equal. Deliberately not whole-object equality: `silent_core_probes` was
  hard-deleted from the type while fixtures keep the dead key, so a subset is correct. C2 should
  extend the same test rather than trust flatten by inspection.
- **Fixtures and goldens untouched**, per §4.3. `golden_baseline_invariance` stayed green with no
  regen, confirming contracts are not a Tier-1/2 axis.
- **Not yet done (C3):** the strings are not shared with `legend_text()`, and there is no
  `--fingerprint-contracts` knob. Default is effectively `always`.

## 3. Suggested ship order

1. Land **R1** (equivalence rename + aliases + fixtures + live docs).
2. Land **C1** (contracts on the two equivalence objects) while the rename is fresh.
3. Land **R3**, then **C2**.
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
