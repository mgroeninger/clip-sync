# Temporary plan: verification & validation hardening

> **Status:** Draft (2026-06-10). Plan 4 of 4 — see [BACKLOG.md](../BACKLOG.md). Follow-ups to the shipped hold-out offset verification ([archive/offset-verification-plan.md](archive/offset-verification-plan.md)) — the "Validation diagnostics — open concerns" table in BACKLOG.

**Problem:** The shipped verification path has order-dependence (first scored hold-out candidate wins even when `verified == false`; headline confidence uses `clips.first()` instead of label selection), an unrecorded false-pass risk (the archived plan's Option A spike checkboxes were never completed), a committed-corpus gap (30 s WAVs vs 60 s min `clip_length` means `verify_offset` never runs on the committed tier), and test-suite duplication around the +3 s chirp scenario.

**Goal:** Label-driven selection everywhere, retry-on-failed-verification with logged window choice, recorded evidence on Option A false passes (gating Option B), a committed-tier verification case if the size budget allows, and deduplicated test roles — plus the outstanding documentation debts (verification cost, repetition-downgrade v1 semantics, stale plan links).

**Workspace split:** Engine changes in **`crates/clip-sync`**; headline-confidence fix also in **`clip-sync-cli`** `output.rs`. Repair untouched except optional fixture-builder adoption.

---

## Current codebase baseline

| Area | Path | Current state | Target phase |
|------|------|---------------|--------------|
| Candidate loop | `crates/clip-sync/src/application/offset_verification.rs` ~116–286 | Per candidate: feasibility → extract A/B → prep → fingerprint → score; extract/prep failures `continue`; **first successful score returns** (~275–285) even when `verified == false` | 2 |
| Candidate generation | `domain/policies.rs` ~279–358 (`holdout_window_candidates`), ~234–276 (`pick_holdout_window`), ~361–372 (feasibility) | Ordered: overlap-safe near-start → +30 s interior → middle-third → dur/6 → post-discovery → `[0, len)`; all labeled `Interior` | 2 |
| Verification method | `offset_verification.rs` ~228–261 | Option A: `aligner.find_offset(&fp_a, &fp_b)`; pass = `confidence >= min_verification_confidence` (0.5) **and** `|offset| <= 0.5 s` (`OFFSET_AGREEMENT_TOLERANCE_SECS`) | 3 |
| Option A spike debt | `docs/archive/offset-verification-plan.md` ~186–192 | Phase 0 false-pass evidence checkboxes **unchecked**; Option B (PCM lag-0, `refine_holdout_segment_lag` exists in `offset_refinement.rs` ~468–487) deferred pending evidence | 3 |
| Headline confidence | `clip-sync-cli/src/infrastructure/cli/output.rs` 30–34 | `result.clips.first()` | 1 |
| Same pattern in corpus | `application/testing/corpus_fixtures.rs` ~557–561, ~599 | `clips.first()` for confidence + repetition assertions | 1 |
| Label selection today | `domain/alignment.rs` ~191–199, ~286, ~329–336, ~363–377 | Four **inline** `.find(\|c\| c.label == ...)` sites; no shared helper | 1 |
| Committed corpus gap | `corpus_fixtures.rs` ~22 (`DEFAULT_TOTAL_SECS = 30`), ~633–670; `application/config.rs` ~9 (`MIN_CLIP_LENGTH` = 60 s) | Committed WAVs 30 s; `verify_offset` would skip ("hold-out window unavailable", `offset_verification.rs` ~79–86); `verify_offset_pass` is generated-tier only (120 s) | 4 |
| Chirp duplication | `audio_fixtures.rs` `write_offset_chirp_wav_pair`; corpus manifest; `offset_verification.rs` tests ~461–560; `align_videos.rs` tests ~1320–1600; repair `cli_wav/cli_mux/scan_gaps` integration tests | Same +3 s scenario in ≥ 4 suites with overlapping assertions | 4 |
| Fixture builder drift | `application/testing/alignment_fixtures.rs` ~11–39 | `minimal_alignment_result` **has zero callers**; hand-built `AlignmentResult` in ≥ 8 test files across all three crates | 4 |
| Repetition downgrade | `align_videos.rs` ~467–479; `domain/alignment.rs` ~126–133, ~176 | `aligned` computed pre-downgrade by design; tested (`align_videos.rs` ~2049–2126) but undocumented in `docs/corpus-validation.md` | 5 |
| Verification cost | `config.rs` ~8 (15 min default), `offset_verification.rs` ~76 | Hold-out segment = full `clip_length`; cost undocumented; no shorter-segment knob | 5 (doc), opt. 6 |
| Stale plan links | `BACKLOG.md` ~65, ~171; `PLAN.md` doc table | Point at `docs/TEMP-offset-verification-plan.md`, which is already archived | 5 |

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Label helper** | Add `AlignmentResult::clip_with_label(ClipLabel) -> Option<&ClipMatch>` (plus `start_clip()` convenience) in `domain/alignment.rs`; replace the four inline finds, the CLI headline site, and the corpus assertion sites. Headline confidence = **start clip by label**, falling back to first clip only if no start clip exists (preserves current behavior for degenerate results). |
| **Retry on failed verification** | When a candidate scores but `verified == false`, **try up to 2 further candidates**. Report the **best attempt by confidence** (ties: earliest candidate). All skip semantics unchanged. *Rejected alternative:* exhausting all candidates — unbounded decode cost on 15-min windows. |
| **Reporting attempts** | `OffsetVerification` gains `candidates_tried: u32` (additive JSON field, `#[serde(default)]`-style compatible). Chosen window start/end already present. Verbose progress line logs each attempted window and its outcome. **Coordinate with the contract plan:** land before its Phase 4 freeze or version the addition. |
| **Option A evidence (gate for Option B)** | Build the false-pass corpus probe the archived plan never ran: generated case with strongly self-similar content (looped chirp) and a deliberately wrong injected offset; record whether Option A passes it. **Only if it false-passes** implement Option B (PCM lag-0 via existing `refine_holdout_segment_lag`) as a confirmation step. Evidence and outcome recorded in `docs/corpus-validation.md` regardless. |
| **Committed-tier verify case** | Gate on size budget (`tests/corpus/README.md`): regenerate one committed pair at **11 025 Hz mono, ~75 s** (≈ 1.7 MB/file) with a manifest case `verify_offset = true`, `clip_length_secs = 60`. If the budget doesn't allow it, explicitly accept generated-only coverage and document that in the manifest README — either way the BACKLOG concern closes with a recorded decision. Existing 30 s fixtures stay (alignment-only roles unchanged). |
| **Test roles (chirp dedupe)** | Corpus = E2E through manifest (committed + generated). `align_videos.rs` keeps **one** real-WAV pipeline test (`execute_detects_known_offset_through_real_wav_pipeline`); other in-crate chirp tests must assert something the corpus can't (unit-level branches) or be deleted. Repair integration tests keep their own copies (different crate, different concern). Document the role split in `docs/corpus-validation.md`. |
| **Fixture builder** | Make `alignment_fixtures` earn its keep: extend `minimal_alignment_result` with the few setters tests actually need (`with_offset`, `with_clips`, `with_verification`), adopt in `crates/clip-sync` and `clip-sync-cli` test files; repair adoption optional (own-crate fixtures acceptable). If after adoption it's still awkward, delete it instead — the failure mode to avoid is a third unused path. |
| **Repetition downgrade docs** | Document in `docs/corpus-validation.md` (and a PLAN output-section note): v1 intentionally computes `aligned` from pre-downgrade confidence; downgrade affects displayed/JSON confidence only. No behavior change. |
| **Verification cost** | Document (README + PLAN clip-window section): `--verify-offset` adds up to ~2 × `clip_length` of decode. A `validation.max_verification_secs` knob is sketched as optional Phase 6 — implement only if the cost note draws real friction. |
| **Hold-out duration source** | Container-vs-extent placement is **owned by** [TEMP-media-session-redesign-plan.md](TEMP-media-session-redesign-plan.md) Phase 4 (`MediaExtent`), not here. This plan touches candidate *selection*, that plan touches candidate *feasibility inputs*. Rebase whichever lands second. |

---

## Phases

### Phase 1 — label-driven selection

- [ ] `clip_with_label` / `start_clip()` helpers + unit tests; replace inline finds in `domain/alignment.rs`.
- [ ] CLI headline confidence (`output.rs` 30–34) and corpus assertions (`corpus_fixtures.rs` ~557–599) use the helper.
- [ ] Regression: human-output test where the first clip is not the start clip.

### Phase 2 — candidate retry + observability

- [ ] Loop change in `apply_offset_verification`: collect scored attempts, stop after first `verified == true` or 3 scored attempts; report best.
- [ ] `candidates_tried` on `OffsetVerification` (+ report DTO if the contract plan landed); verbose per-attempt logging.
- [ ] Unit tests: second candidate verifies after first fails; all candidates fail → best-confidence attempt reported with `verified: false`; skip paths untouched.

### Phase 3 — Option A false-pass evidence

- [ ] Generated corpus case: looped/self-similar content + wrong-offset probe; record pass/fail.
- [ ] Findings → `docs/corpus-validation.md`. If false-pass confirmed: follow-up Option B implementation (PCM lag-0 confirmation via `refine_holdout_segment_lag`) as its own checkbox set; otherwise close the BACKLOG concern with the evidence link.

### Phase 4 — corpus + test hygiene

- [ ] Size-budget check; if green: committed ~75 s 11 025 Hz pair + manifest `verify_offset` case runnable in `corpus_committed_cases`; else documented acceptance.
- [ ] Chirp dedupe per the test-roles decision (delete or sharpen redundant in-crate tests).
- [ ] `alignment_fixtures` builder extension + adoption in lib and CLI test files.

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
| Committed verify | New manifest case in `corpus_committed_cases` (if budget allows) |
| No regressions | `corpus_verify_offset_pass`, full workspace suite at each phase boundary |

## Exit criteria

- No `clips.first()` for headline/assertion purposes anywhere; selection is label-driven.
- Failed verification provably tries alternates; attempts visible in JSON + verbose output.
- Option A false-pass question answered with recorded evidence; Option B implemented or explicitly closed.
- Committed-tier verification decision executed and documented; chirp test roles deduplicated.
- All "Validation diagnostics — open concerns" BACKLOG rows resolved or re-pointed.

## Cross-plan sequencing

- The [contract plan](archive/output-error-contract-plan.md) froze the JSON contract on 2026-06-10 (v1, [json-output.md](json-output.md)). Phase 2's field must be **additive and optional-absent** (stays v1 per the revision procedure there) and must update `docs/json-output.md` + regenerate the golden fixtures.
- `offset_verification.rs` is shared with [TEMP-media-session-redesign-plan.md](TEMP-media-session-redesign-plan.md) Phase 4 — whichever lands second rebases; extent-based placement belongs there, not here.
- Layer-purity shipped 2026-06-11 ([archive/layer-purity-plan.md](archive/layer-purity-plan.md)).
