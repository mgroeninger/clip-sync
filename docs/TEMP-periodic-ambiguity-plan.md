# Temporary plan: periodic offset ambiguity

> **Status:** Draft (2026-06-11). Motivated by looped-chirp corpus evidence: discovery aliases to **+13 s** (true **+3 s** + 10 s period); Option A hold-out verify **false-passes** the same period-equivalent wrong Δ. Option B (PCM lag-0) does not break periodic symmetry — see [corpus-validation.md](corpus-validation.md) § Option A false-pass evidence.
>
> Archive to `docs/archive/periodic-ambiguity-plan.md` when shipped.

**Problem:** When audio repeats every **T** seconds (loop, rebroadcast, applause), Chromaprint discovery can report any offset **≡ true (mod T)** with high confidence. Hold-out verification (Option A) extracts B at `window_A + recommended_offset`; a placement error of **N×T** still yields hold-out segments that match at lag 0, so `verified == true` is misleading.

**Goal (v1):** Stop claiming a unique offset when repetition makes it unknowable from fingerprint/lag-0 checks alone; optionally recover the true offset when **non-periodic file structure** (leading silence, unique head) breaks symmetry.

**Non-goals (v1):** Option B lag-0 PCM as primary verify path; runtime query/reference mode ([TEMP-query-reference-alignment-plan.md](TEMP-query-reference-alignment-plan.md)); clearing `recommended_offset_secs` or changing exit codes; committed corpus budget changes.

**Workspace split:** Engine logic in **`crates/clip-sync`**; CLI flags unchanged (reuse `check_clip_repetition`); JSON/human via **`clip-sync-cli`** `output.rs` + `application/report.rs`. Repair consumes `AlignmentResult` only — no repair UI in v1.

**Prerequisite evidence:** `corpus_verify_option_a_false_pass_probe` — +8/+18 s → `verified == false`; +13 s → `verified == true`.

---

## Current codebase baseline

| Area | Path | Current state | Target phase |
|------|------|---------------|--------------|
| Repetition detect | `infrastructure/chromaprint/repetition.rs` | `detect_clip_repetition` → `RepetitionFinding { lag_secs, confidence, … }` | 1 |
| Downgrade | `domain/alignment.rs` `should_downgrade_repetition_confidence` | Fires when `\|offset − lag\| ≤ 1 s` only; **misses** +13 s vs lag 10 s | 1 |
| Discovery merge | `application/align_videos.rs` | Downgrade after `build_alignment_result`; no mod-**T** flag | 1 |
| Hold-out candidates | `domain/policies.rs` `holdout_window_candidates` | Interior-first; `Duration::ZERO` window is **last** fallback | 2 |
| Verification | `application/offset_verification.rs` | Option A lag-0 on offset-shifted B window; repetition halves confidence only when `\|offset − lag\| ≤ 1 s` | 1–2 |
| Report / JSON | `application/report.rs`, `docs/json-output.md` | No `offset_ambiguous_mod_secs` / parallel-recheck fields | 1 |
| Human output | `clip-sync-cli/.../output.rs` | Repetition lines when finding or `--verbose` | 1 |
| Corpus | `corpus_verify_option_a_false_pass_probe` | Documents +13 s false-pass; no discovery assertion on looped pair | 3 |

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Trigger** | Strong repetition on **start clip** (either side): `repetition.a` or `repetition.b` with `confidence ≥ min_repetition_confidence` and `lag_secs ≥ MIN_REPEAT_LAG_SECS` (reuse clip-self-repetition floor, ~5 s). Requires `check_clip_repetition` — same flag as today; no new CLI knob in v1. |
| **Mod-**T** ambiguity flag** | Add `offset_ambiguous_mod_secs: Option<f64>` on `AlignmentResult` when trigger fires — value is the repeat period **T** (prefer `repetition.a.lag_secs`, else `b`; if both, prefer higher confidence). Diagnostic only in Phase 1; drives verify gating in Phase 2. |
| **Extended downgrade** | New `should_downgrade_periodic_ambiguity(rep, offset)`: true when repetition is strong **and** `(offset.abs() % T)` is near **both** `r` and `(r + k*T)` for multiple plausible residues — **v1 simplification:** true when strong repeat exists **and** `offset.abs() ≥ T - 1` (offset not explained solely by “first period” — catches +13 with T=10). Phase 1 may ship broader rule: **any** strong repeat on start clip → set ambiguity flag + halve confidence (even when offset < T). *Rejected for v1:* auto-clearing `recommended_offset_secs`. |
| **Verify gating (Phase 2)** | After Option A scores a candidate, if ambiguity flag would apply (repetition on hold-out fingerprints) **and** no parallel recheck (Phase 3), force `verified = false` with `skip_reason` or post-pass override — **do not** report `verified: true` on periodic material when only offset-shifted lag-0 passed. |
| **Parallel-window recheck (Phase 3)** | When repetition trigger fires, extract **same calendar window** on A and B: `[T, T+L)` on both (B **not** shifted by recommended Δ). Run `find_offset` → `independent_offset_secs`. If `\|independent − recommended\|` is ≈ **N×T** (integer **N ≠ 0**, tolerance 0.5 s), keep ambiguity flag and `verified = false`. If **N = 0**, allow verify pass and clear ambiguity flag for that run. Prefer **edge** `T = 0` (and `T = min` feasible) first — exploits B leading silence on looped fixture (+3 vs +13). |
| **Option B** | **Out of scope** — lag-0 PCM on offset-shifted periodic segments false-passes identically; parallel calendar extract + fingerprint lag search is the disambiguation path. |
| **JSON contract** | Additive optional fields on `AlignmentReport` (v1 per [archive/output-error-contract-plan.md](archive/output-error-contract-plan.md)): `offset_ambiguous_mod_secs`, optional `offset_verification.independent_offset_secs` / `parallel_recheck_delta_secs`. Update `docs/json-output.md` + golden fixtures in Phase 1. |
| **Human output** | One line when flag set: e.g. `Offset ambiguous (repeats every ~10 s) — verify and auto offset may match wrong period`. |
| **Query mode** | Separate plan; periodic ambiguity there uses `ambiguous: bool` on localization — cross-link only. |

---

## Phases

### Phase 1 — Detect and disclose (honesty layer)

**Lib**

- [ ] `domain/alignment.rs` — `offset_ambiguous_mod_secs: Option<f64>` on `AlignmentResult`; helper `periodic_ambiguity_period(start_clip_repetition) -> Option<f64>`
- [ ] Extend downgrade policy: `should_downgrade_periodic_ambiguity` or widen `should_downgrade_repetition_confidence` when strong repeat on start clip (document behaviour in [corpus-validation.md](corpus-validation.md))
- [ ] `application/align_videos.rs` — after repetition merge on start clip, set `result.offset_ambiguous_mod_secs` when trigger fires; apply confidence halving (stack with existing downgrade or single combined pass — pick one, test)
- [ ] `application/report.rs` + `docs/json-output.md` — report field; regenerate CLI golden JSON if present

**CLI**

- [ ] `output.rs` — human warning line when `offset_ambiguous_mod_secs` is `Some`

**Tests**

- [ ] Unit tests on mod-**T** helper (pure tone / synthetic repetition from clip-self-repetition suite)
- [ ] Integration: `repeated_segment_in_clip` or looped pair with `check_clip_repetition` → flag present
- [ ] `corpus_verify_option_a_false_pass_probe` unchanged (+13 still passes Option A internally — gating is Phase 2)

### Phase 2 — Verify must not lie

**Lib**

- [ ] `offset_verification.rs` — after scoring, if strong hold-out repetition **or** `result.offset_ambiguous_mod_secs.is_some()` before verify, override `verified = false` on the reported best attempt (retain confidence/lag in JSON for debugging). Set `skip_reason: Some("periodic content; offset-shifted verify inconclusive")` **or** add `verified_false_reason` — prefer reusing `skip_reason` only when skipped; add optional `verify_inconclusive: bool` if needed
- [ ] Ensure `phase_verbose` mentions periodic override

**Tests**

- [ ] Unit test in `offset_verification.rs`: fake periodic pass → `verified == false` when ambiguity active
- [ ] Extend `corpus_verify_option_a_false_pass_probe` **or** new test `corpus_looped_discovery_alias_verify_gated`: full pipeline with discovery (alias +13) + `verify_offset` → `verified == false` when Phase 2 shipped

### Phase 3 — Edge parallel recheck (disambiguation)

**Lib**

- [ ] `domain/policies.rs` — `parallel_holdout_window_candidates(...)` when repetition detected: **prepend** `T=0` and other edge-feasible windows before interior list; label `ClipLabel::Interior` ok or add `Edge` if report needs it
- [ ] `offset_verification.rs` — when ambiguity trigger: run calendar-parallel extract + `find_offset`; compare to recommended Δ; gate `verified` per decision table above; store `independent_offset_secs` on `OffsetVerification` (or nested struct)
- [ ] Optional: if parallel recheck disagrees by **N×T**, adjust confidence / keep ambiguity; if agrees (**N=0**), clear `offset_ambiguous_mod_secs` and allow `verified == true`

**Corpus**

- [ ] Generated case `looped_chirp_discovery_alias` (or extend probe): assert discovery ≈ +13 s, parallel recheck ≈ +3 s, `verified == false` until recheck agrees; document in [corpus-matrix.md](corpus-matrix.md)

**Tests**

- [ ] `corpus_looped_parallel_recheck_finds_true_offset` — true +3 s rediscovered via edge parallel window
- [ ] Regression: non-periodic `corpus_verify_offset_pass` unchanged

### Phase 4 — Documentation and archive

- [ ] [corpus-validation.md](corpus-validation.md) — replace Follow-up bullet with shipped behaviour; operator guidance
- [ ] [PLAN.md](../PLAN.md) validation flags paragraph — ambiguity + verify gating
- [ ] [BACKLOG.md](../BACKLOG.md) — move periodic item to Completed; archive this plan

---

## Verification flow (after Phase 3)

```mermaid
flowchart TD
  A[Discovery + repetition check] --> B{Strong repeat lag T?}
  B -->|no| C[Existing verify: offset-shifted hold-out Option A]
  B -->|yes| D[Set offset_ambiguous_mod_secs = T]
  D --> E[Parallel extract A and B at same calendar T_edge]
  E --> F[find_offset independent Δ]
  F --> G{|independent - recommended| ≈ N×T, N≠0?}
  G -->|yes| H[verified = false, warn ambiguous]
  G -->|no| I[Option A offset-shifted verify allowed]
  C --> J[Report best attempt]
  I --> J
  H --> J
```

---

## Tests summary

| Concern | Coverage |
|---------|----------|
| Flag set | Strong repeat on start clip → `offset_ambiguous_mod_secs == Some(T)` |
| Downgrade | +13 s offset, T=10 → confidence halved / flag set |
| Verify gate | +13 injected, periodic → `verified == false` (Phase 2+) |
| Parallel recheck | Looped +3 s true → independent ≈ +3 at T=0 (Phase 3) |
| No regression | `corpus_verify_offset_pass`, `repeated_segment_in_clip`, non-looped chirp cases |

---

## Exit criteria

- Operators see explicit **ambiguous mod T** signal when repetition explains multi-offset family.
- `verified: true` never stands alone on strong periodic content without parallel recheck agreement (Phase 2+).
- Looped chirp corpus documents discovery alias **and** path to recover +3 s via edge parallel recheck (Phase 3).
- JSON/human docs updated; BACKLOG periodic item closed.

---

## Cross-plan sequencing

- **Independent** of [TEMP-ac3-backend-plan.md](TEMP-ac3-backend-plan.md).
- **Complements** [TEMP-query-reference-alignment-plan.md](TEMP-query-reference-alignment-plan.md) (short-B localization); share `ambiguous` semantics in docs only until query mode ships.
- Follows [archive/verification-hardening-plan.md](archive/verification-hardening-plan.md) (verify retry, `candidates_tried`); Phase 2 override runs **after** best-attempt selection.
- JSON additions stay **v1 additive** per [archive/output-error-contract-plan.md](archive/output-error-contract-plan.md).
