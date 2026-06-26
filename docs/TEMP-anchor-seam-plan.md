# Anchor-based seam placement — plan (DRAFT)

Status: **not started** — design capture from production-gap analysis (symmetric-weak W5 skips on
dropouts with salient speech ±1 s from a silent throat). Motivating case: scan finds a fillable hole;
energy structure slides B; waveform seam grades ~250 ms at the **quiet junction** (`pre/post ≈ 0`) and
skips.

Companions: [seam-scoring.md](seam-scoring.md), [gap-repair-guide.md](gap-repair-guide.md) § W5 /
Vocabulary, [gap-fill-modes.md](gap-fill-modes.md) § extension / `baseline_only`, [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md).

---

## 1. Problem (one paragraph)

Today a **good seam** means `min(pre, post)` Pearson on **fixed** (or incrementally extended) scan
gap edges, using ~250 ms windows after standoff/trim at the dropout **throat**. Placement uses a
**wider** representation (3 s energy envelope or bool pause pattern) to slide B, but approval uses a
**narrow, often silent** slice that may not contain the salient audio an editor would use as the cut
(speech peaks, onset before/after a dropout). When throat and contour disagree, we get **structure
placement + waveform skip** — the wrong decomposition for “find matchable cut points, then fill
between them.”

Incremental `gap_*_extend_*` nudges the hole in ≤500 ms steps after Pearson fails; it does not
**search for anchor pairs** where A and B carry identifiable, matchable signal.

---

## 2. Definition: anchor seam (target)

Per gap, choose four linked points (two editorial boundaries):

| Anchor | Meaning |
|--------|---------|
| **A_pre** | Last kept sample on A before the B fill |
| **A_post** | First kept sample on A after the B fill |
| **B_pre** | B audio that must correspond across **A_pre** |
| **B_post** | B audio that must correspond across **A_post** |

**Same editorial boundary:** `(A_pre ↔ B_pre)` and `(A_post ↔ B_post)` are the same story moment
(modulo clip offset). **Matchable:** short windows at each anchor have **signal** (not codec noise
alone) and **agree** between A and B under at least one metric (envelope, waveform, residual cancel).

The **fill region** on A is `[A_pre, A_post]`; on B it is the mapped interior between the matched
anchors. Seam validation runs **at the chosen anchors**, not by assumption at the scan silence floor.

---

## 3. Non-goals

- **Per-gap chromaprint / landmark FFT** — clip-level offset already exists; too coarse/slow for
  sub-second cuts.
- **Replacing scan** — scan still finds “there is a hole”; anchors **refine where to cut** inside/near
  that hole.
- **Removing waveform Pearson** — keep as a validator when anchors carry waveform; compose with
  envelope/residual when throat Pearson is uninformative.
- **Unbounded gap growth** — anchor brackets must stay near the scan hole (prior + max span).
- **Gate-mode-only trust** — fit mode should get an explicit anchor-trust path, not only legacy
  `structure_trusted` in `fill_mode = gate`.

---

## 4. Current behavior (summary)

```text
scan hole (min_gap_ms floor)
  → refine_gap_frames (silence walk on A)
  → fixed A bracket (± extend grid only under --full / full_grid)
  → structure slide on B (energy/bool, 3 s context)
  → seam Pearson 250 ms at scan throat → tier / skip
```

| Mechanism | Sees salient peaks ~1 s away? | Chooses cut points? |
|-----------|------------------------------|---------------------|
| Energy / bool structure | Often (bins in 3 s context) | No — slides B only |
| Seam Pearson | No (250 ms at throat) | No — grades fixed edges |
| `gap_*_extend_*` | No — local nudge | Slightly — only if grid/retry runs |
| Residual rescue | Raw window at throat | No |

**Default profile (`baseline_only`):** extension grid inactive; failed baseline → skip with no
anchor search.

---

## 5. Proposed design

### 5a. Principle

> **Propose a small set of anchor candidates on A from existing representations; score joint
> (A_pre, A_post, B placement) for matchability; pick the bracket where both boundaries are
> verifiable; fill and splice between them.**

Reuse decode + haystack infrastructure. New logic is **candidate generation**, **matchability
scoring**, and **fit-mode approval** at chosen anchors.

### 5b. Anchor candidates on A (Tier 1 — reuse)

Within `[gap − context, gap + context]` on A (default 3 s, center-weighted channels for 5.1):

| Source | Candidate | Existing code |
|--------|-----------|---------------|
| Bool | Silence ↔ active transitions | `activity_bins`, `build_gap_context_signature` |
| Energy | Local maxima in `pre_energy` / `post_energy` | `energy_bins`, `build_gap_energy_signature` |
| Scan | Refined gap start/end | `refine_gap_frames` (always a fallback candidate) |

**Filter (matchable on A):** bin energy or RMS above `absolute_silence_rms` / scan silence floor;
optional minimum prominence vs neighbors (envelope peak − local median).

Keep **K ≤ 5** candidates per side; always include scan-refined edges as fallback.

### 5c. Match B and score pairs (Tier 1 + optional Tier 2)

For each feasible `(A_pre_anchor, A_post_anchor)` with `A_pre < scan hole < A_post` (or containing
scan interior) and span ≤ `max_anchor_bracket_secs` (config, e.g. 5 s):

1. Build `GapSignature` / energy halves for **that** bracket (not only scan edges).
2. Run existing unified B slide (`UnifiedFillSearchInput`) — same haystack, new signature geometry.
3. **Matchability** at anchors (both required unless policy allows one-strong for short gaps):

| Metric | Use | Existing |
|--------|-----|----------|
| Envelope similarity | Primary placement score | `score_pre/post_energy_match` |
| Waveform Pearson | Validator at anchor windows | `seam_pearson` — window width **adaptive** (e.g. 250 ms–1 s, capped by local energy) |
| Residual headroom | Same-master confirm / rescue | `SeamResidualVerdict`, `apply_residual_to_confidence` |

**Optional Tier 2 (later):** short local PCM xcorr (`PcmCorrelator` port) on center channel at anchor
windows — one lag peak per anchor, few candidates only.

**Ranking:** existing unified score + penalties for distance from scan hole (prior nominal bracket) +
distance from scan center (don’t swallow unrelated speech).

### 5d. Approval (replace throat-only tier)

When **both** anchors pass matchability on B:

- **High / marginal** from anchor Pearson or envelope+residual compose (extend
  `classify_fill_waveform_confidence` / sibling).
- When anchor windows are **low-RMS but residual cancels** → marginal via rescue (same invariant as
  [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) §2).
- When structure confident at anchors but Pearson dead at throat → **`anchor_trusted`** patch tier
  (fit mode; vocabulary tag), with residual veto unchanged.

Emit `A gap (refined)` from **winning anchor bracket**, not only silence walk on scan edges.

### 5e. Relation to extension grid

Anchor search **subsumes** “move the cut to matchable audio” for many W5 cases. Keep `gap_*_extend_*`
grid as a fallback when anchor candidate set is empty or all fail; do not rely on 40 ms steps to reach
peaks ~1 s away.

---

## 6. Implementation phases

| Phase | Scope | Touch |
|-------|--------|-------|
| **P0** | Domain: `AnchorCandidate`, `list_anchor_candidates_a()`, `matchability_at_anchor()` using energy + RMS | `domain/` new module or `gap_energy` + `gap_structure` |
| **P1** | Integrate into fit path: bracket loop before/alongside `evaluate_seam_gate_fit_joint`; config `max_anchor_bracket_secs`, `max_anchors_per_side` | `patch_region.rs`, `gap_fill_fit.rs` |
| **P2** | Adaptive seam window + fit-mode `anchor_trusted`; vocabulary `patch_tier` / `gap_tags` | `policies.rs`, `gap_tags.rs`, `cli-output.md` |
| **P3** | Oracle: synthetic dropout with speech peaks ±1 s from throat; production row from gap-corpus | `tests/` fixtures |
| **P4** (optional) | Local PCM xcorr at anchors | `PcmCorrelator` adapter |

**Default behavior:** ship behind `repair.anchor_seam_mode = off | auto | force` (or profile flag);
`auto` enables when baseline throat `min(pre,post) < marginal_floor` and envelope contour present.

---

## 7. Validation

| Case | Expect |
|------|--------|
| **A1** Speech dropout, peaks ±1 s, same master encodes | Anchor at peaks; patch marginal+; listenable splice |
| **A2** C3 speech boundary (asymmetric post) | Anchors near onset; aligns with bool path; no regression vs W3 |
| **A3** Flat room tone (C1) | Fallback to scan edges; behavior ≈ today |
| **A4** F4 decoy / wrong B slide | Residual veto; no anchor_trusted false patch |
| **A5** `baseline_only` profile | Anchor search runs without requiring `--full` grid |

Track: `patch_tier`, `seam_shape`, `anchor_trusted` (new), wall time per gap (candidate count bounded).

---

## 8. Open questions

1. **Bracket vs hole:** Must anchors **contain** the entire scan hole, or may they **shrink** it when
   interior is silence? (Default: contain — don’t leave unfilled scan silence inside bracket.)
2. **5.1 peak picking:** Per-channel envelope max on center-only vs energy-selected channels?
3. **Interaction with `border_standoff_secs`:** Apply standoff relative to **anchor** edge, not scan
   edge.
4. **CLI:** Expose `anchor_seam_mode` or fold into `--full` / new profile `anchor`?

---

## 9. Related reading

| Doc | Contents |
|-----|----------|
| [seam-scoring.md](seam-scoring.md) | Current seam definition, 250 ms throat |
| [gap-repair-guide.md](gap-repair-guide.md) | W5, tiers, vocabulary |
| [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) | Pearson vs residual on quiet seams |
| [archive/energy-signature-plan.md](archive/energy-signature-plan.md) | Structure tier shipped |
