# Temporary plan: verification & validation hardening

> **Status:** In progress (2026-06-11). Plan 4 of 4 — see [BACKLOG.md](../BACKLOG.md). Follow-ups to the shipped hold-out offset verification ([archive/offset-verification-plan.md](archive/offset-verification-plan.md)) — the "Validation diagnostics — open concerns" table in BACKLOG.

**Problem:** The shipped verification path has order-dependence (first scored hold-out candidate wins even when `verified == false`; headline confidence uses `clips.first()` instead of label selection), an unrecorded false-pass risk (the archived plan's Option A spike checkboxes were never completed), a committed-corpus gap (30 s WAVs vs 60 s min `clip_length` means `verify_offset` never runs on the committed tier), and test-suite duplication around the +3 s chirp scenario.

**Goal:** Label-driven selection everywhere, retry-on-failed-verification with logged window choice, recorded evidence on Option A false passes (gating Option B), a committed-tier verification case if the size budget allows, and deduplicated test roles — plus the outstanding documentation debts (verification cost, repetition-downgrade v1 semantics, stale plan links).

**Workspace split:** Engine changes in **`crates/clip-sync`**; headline-confidence fix also in **`clip-sync-cli`** `output.rs`. Repair untouched except optional fixture-builder adoption.

---

## Current codebase baseline

| Area | Path | Current state | Target phase |
|------|------|---------------|--------------|
| Candidate loop | `crates/clip-sync/src/application/offset_verification.rs` ~92–261 | Per candidate: feasibility → extract A/B → prep → fingerprint → score; extract/prep failures `continue`; **first successful score returns** (~251–261) even when `verified == false` | 2 |
| Candidate generation | `domain/policies.rs` ~234–334 (`holdout_window_candidates`, `pick_holdout_window`), ~336–347 (feasibility) | Ordered: overlap-safe near-start → +30 s interior → middle-third → dur/6 → post-discovery → `[0, len)`; all labeled `Interior` | 2 |
| Verification method | `offset_verification.rs` ~236–261 | Option A: `aligner.find_offset(&fp_a, &fp_b)`; pass = `confidence >= min_verification_confidence` (0.5) **and** `\|offset\| <= 0.5 s` (`OFFSET_AGREEMENT_TOLERANCE_SECS`) | 3 |
| Option A spike debt | `docs/archive/offset-verification-plan.md` ~186–192 | Phase 0 false-pass evidence checkboxes **unchecked**; Option B (PCM lag-0, `refine_holdout_segment_lag` exists in `offset_refinement.rs` ~468–487) deferred pending evidence | 3 |
| Headline confidence | `clip-sync-cli/src/infrastructure/cli/output.rs` 35–39 | `AlignmentReport::from(result)` then `clips.first()` | 1 |
| Same pattern in corpus | `application/testing/corpus_fixtures.rs` ~572–576 (confidence), ~614 (repetition) | `clips.first()` for confidence + repetition assertions | 1 |
| Label selection today | `domain/alignment.rs` ~204–212, ~298–299, ~341–349, ~376–390 | Four **inline** `.find(\|c\| c.label == ...)` sites; no shared helper | 1 |
| Committed corpus gap | `corpus_fixtures.rs` ~22 (`DEFAULT_TOTAL_SECS = 30`), ~648–670; `application/config.rs` ~9 (`MIN_CLIP_LENGTH` = 60 s) | Committed WAVs 30 s (~3.36 MB total); `verify_offset` would skip; `verify_offset_pass` is generated-tier only (120 s) | 4 |
| Chirp duplication | `audio_fixtures.rs` `write_offset_chirp_wav_pair`; corpus manifest; `offset_verification.rs` tests; `align_videos.rs` tests ~1433–1499, ~1502+ | Same +3 s scenario in ≥ 4 suites with overlapping assertions | 4 |
| Fixture builder drift | `application/testing/alignment_fixtures.rs` ~11–39 | `minimal_alignment_result` **has zero callers**; hand-built `AlignmentResult` in ≥ 8 test files across all three crates | 4 |
| Repetition downgrade | `align_videos.rs` ~467–479; `domain/alignment.rs` ~126–133, ~176 | `aligned` computed pre-downgrade by design; tested (`align_videos.rs` ~2049–2126) but undocumented in `docs/corpus-validation.md` | 5 |
| Verification cost | `config.rs` ~8 (15 min default), `offset_verification.rs` ~76 | Hold-out segment = full `clip_length`; cost undocumented; no shorter-segment knob | 5 (doc), opt. 6 |
| Stale plan links | `BACKLOG.md` ~65, ~171; `PLAN.md` doc table | Point at `docs/TEMP-offset-verification-plan.md`, which is already archived | 5 |

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Label helper** | Add `clip_with_label(clips, label)` free function plus `AlignmentResult::clip_with_label` / `start_clip()` in `domain/alignment.rs`; replace the four inline finds, the CLI headline site, and the corpus assertion sites (confidence **and** repetition). Headline confidence = **start clip by label**, falling back to first clip only if no start clip exists (preserves current behavior for degenerate results). |
| **CLI headline fix** | `format_human_output` reads confidence from the **domain** `&AlignmentResult` via `start_clip()` **before** `AlignmentReport::from` — do not patch the report DTO. |
| **Retry on failed verification** | When a candidate scores but `verified == false`, **try up to 2 further candidates**. Report the **best attempt by confidence** (ties: earliest candidate). All skip semantics unchanged. *Rejected alternative:* exhausting all candidates — unbounded decode cost on 15-min windows. |
| **Reporting attempts** | `OffsetVerification` gains `candidates_tried: u32` (additive JSON field, optional-absent under v1). Chosen window start/end already present. Verbose `phase_verbose` line per attempted window and outcome; **not** shown in compact human output. Contract plan shipped — propagate through domain → report DTO → `docs/json-output.md` → golden fixture (see Phase 2 touch list). |
| **Option A evidence (gate for Option B)** | Build the false-pass corpus probe the archived plan never ran: generated case with strongly self-similar content (looped chirp) and a deliberately wrong injected offset; record whether Option A passes it. **Only if it false-passes** implement Option B (PCM lag-0 via existing `refine_holdout_segment_lag`) as a confirmation step. Evidence and outcome recorded in `docs/corpus-validation.md` regardless. |
| **Committed-tier verify case** | **Pre-flight (2026-06-11):** current committed WAVs ≈ 3.36 MB; adding a new 75 s pair (+≈ 3.15 MB) → ≈ 6.5 MB, **over the 5 MB README cap** while keeping all 30 s fixtures. **Default:** accept generated-only verify coverage (`verify_offset_pass`, `mkv_tail_decodable_extent_gap`) and document in `tests/corpus/README.md` — closes BACKLOG either way. Only add committed verify fixtures if the budget is raised or an existing pair is extended in place (not added alongside). |
| **Test roles (chirp dedupe)** | Corpus = E2E through manifest (committed + generated). `align_videos.rs` keeps **one** real-WAV pipeline test (`execute_detects_known_offset_through_real_wav_pipeline`); other in-crate chirp tests must assert something the corpus can't (unit-level branches) or be deleted. Repair integration tests keep their own copies (different crate, different concern). Document the role split in `docs/corpus-validation.md`. **Delete candidate:** `execute_runs_offset_verification_when_flag_on` (overlaps `corpus_verify_offset_pass`). **Keep:** `offset_verification.rs` unit tests (pass/fail/skip branches); `run_cross_layer_chirp_alignment_with` tests (PCM refine / high-rate, not verify dedupe). |
| **Fixture builder** | Make `alignment_fixtures` earn its keep: extend `minimal_alignment_result` with the few setters tests actually need (`with_offset`, `with_clips`, `with_verification`), adopt in `crates/clip-sync` and `clip-sync-cli` test files; repair adoption optional (own-crate fixtures acceptable). If after adoption it's still awkward, delete it instead — the failure mode to avoid is a third unused path. |
| **Repetition downgrade docs** | Document in `docs/corpus-validation.md` (and a PLAN output-section note): v1 intentionally computes `aligned` from pre-downgrade confidence; downgrade affects displayed/JSON confidence only. No behavior change. |
| **Verification cost** | Document (README + PLAN clip-window section): `--verify-offset` adds up to ~2 × `clip_length` of decode. A `validation.max_verification_secs` knob is sketched as optional Phase 6 — implement only if the cost note draws real friction. |
| **Hold-out duration source** | Container-vs-extent placement shipped in [archive/media-session-redesign-plan.md](archive/media-session-redesign-plan.md) Phase 4 (`MediaExtent`). This plan touches candidate *selection*; that plan touched candidate *feasibility inputs*. |

---

## Phases

### Phase 1 — label-driven selection

- [x] `clip_with_label` / `start_clip()` helpers + unit tests; replace inline finds in `domain/alignment.rs`.
- [x] CLI headline confidence: read `start_clip()` on domain `&AlignmentResult` in `output.rs` **before** `AlignmentReport::from`.
- [x] Corpus confidence (~572–576) and repetition (~614) assertions use `start_clip()` (fallback to first clip where degenerate).
- [x] Regression: human-output test where the first clip is not the start clip (`cli_output.rs`).

### Phase 2 — candidate retry + observability

- [x] Loop change in `apply_offset_verification`: collect scored attempts, stop after first `verified == true` or 3 scored attempts; report best.
- [x] `candidates_tried` on `OffsetVerification` + full propagation (touch list below); `phase_verbose` per-attempt logging.
- [x] Unit tests: second candidate verifies after first fails; all candidates fail → best-confidence attempt reported with `verified: false`; skip paths untouched.

**Phase 2 `candidates_tried` touch list**

| Layer | File |
|-------|------|
| Domain | `domain/alignment.rs` — `OffsetVerification` field |
| JSON DTO | `application/report.rs` — `OffsetVerificationReport` + `From` |
| Contract | `docs/json-output.md` — additive optional-absent field |
| Goldens | `clip-sync-cli/tests/fixtures/full_surface_alignment.json` (regenerate if fixture includes `offset_verification`) |
| Human output | **no** compact headline change; verbose `phase_verbose` only |

### Phase 3 — Option A false-pass evidence

- [x] Generated corpus case: looped/self-similar content + wrong-offset probe; record pass/fail.
- [x] Findings → `docs/corpus-validation.md`. **No false-pass observed** — Option B (PCM lag-0 via `refine_holdout_segment_lag`) remains deferred; BACKLOG concern closes with evidence link.

**Option B follow-up (deferred — only if future probe false-passes):**

- [ ] Add PCM lag-0 confirmation step after Option A in `apply_offset_verification`
- [ ] Corpus case proving Option B catches the false-pass scenario

### Phase 4 — corpus + test hygiene

- [x] **Budget pre-flight:** document generated-only verify acceptance in `tests/corpus/README.md` (default path per decision above). Only add committed ~75 s pair if budget raised or pair extended in place.
- [x] Chirp dedupe: delete `execute_runs_offset_verification_when_flag_on`; sharpen or keep remaining in-crate tests per test-roles decision.
- [x] `alignment_fixtures` builder extension + adoption in lib and CLI test files.

### Phase 5 — documentation debts

- [ ] Repetition-downgrade v1 note; verification cost note; test-role split — all in `docs/corpus-validation.md` / PLAN / README as decided.
- [ ] Fix stale `TEMP-offset-verification-plan.md` links in BACKLOG (~65, ~171) and PLAN doc table → `docs/archive/offset-verification-plan.md`.
- [ ] Close the BACKLOG "Validation diagnostics" rows this plan covers.

### Phase 6 (optional, gated) — shorter verification segment

- [ ] Only on demonstrated friction: `validation.max_verification_secs` knob capping hold-out segment length; corpus case proving pass behavior at the shorter length.

---

## Tests

| Concern | Coverage |
|---------|----------|
| Label selection | Unit tests on helpers; CLI human-output regression with reordered clips |
| Retry | Multi-candidate unit tests with fake sessions (extend existing `offset_verification.rs` test harness ~323+) |
| False pass | Generated corpus probe; outcome recorded as doc + assertion |
| Committed verify | Documented generated-only acceptance (default); manifest case only if budget allows |
| No regressions | `corpus_verify_offset_pass`, full workspace suite at each phase boundary |

## Exit criteria

- No `clips.first()` for headline/assertion purposes anywhere; selection is label-driven.
- Failed verification provably tries alternates; attempts visible in JSON + verbose output.
- Option A false-pass question answered with recorded evidence; Option B implemented or explicitly closed.
- Committed-tier verification decision executed and documented; chirp test roles deduplicated.
- All "Validation diagnostics — open concerns" BACKLOG rows resolved or re-pointed.

## Cross-plan sequencing

- The [contract plan](archive/output-error-contract-plan.md) froze the JSON contract on 2026-06-10 (v1, [json-output.md](json-output.md)). Phase 2's field must be **additive and optional-absent** (stays v1 per the revision procedure there) and must update `docs/json-output.md` + regenerate the golden fixtures.
- `offset_verification.rs` extent-based placement is shipped ([archive/media-session-redesign-plan.md](archive/media-session-redesign-plan.md) Phase 4); this plan covers remaining candidate selection and corpus gaps.
- Layer-purity shipped 2026-06-11 ([archive/layer-purity-plan.md](archive/layer-purity-plan.md)).
