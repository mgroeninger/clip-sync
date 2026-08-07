# TEMP — Fill normalize: boost-only (never attenuate)

**Status:** draft / not started (2026-08-07).
**One deliverable:** change A-border fill loudness matching so it may **raise** a quiet B fill
toward A's borders, but must **never attenuate** a louder fill.

Companion: [fill_level.rs](../../crates/clip-sync-repair/src/domain/fill_level.rs) module docs,
[json-output.md](../json-output.md) § FillLevelCheck (gunshot-in-conversation counter-example),
[pipeline.md](../pipeline.md) § Fill-level check, archive
[repair-write-path-plan.md](archive/repair-write-path-plan.md) § Normalization.

---

## 1. Goal / non-goals

**Goal**
- When `normalize_fill` is on, `compute_fill_gain(a_border_rms, b_segment_rms, max_db)` still
  computes the A-border / B-fill RMS ratio and still clamps the **upward** side with
  `max_fill_gain_db`, but **never returns a gain &lt; 1.0**.
- Quiet B fills keep today's boost behavior (subject to the existing ±dB clamp on the boost side).
- Loud correct content (e.g. a gunshot inside a talk scene) keeps its donor level instead of being
  dragged down to A shoulder RMS.
- Unit + integration coverage for both directions; durable docs note the policy.

**Non-goals (this plan)**
- Flipping default `normalize_fill` to `false`.
- Residual LSQ `g` (`a ≈ g·b`) as the production gain (needs verdict plumbing + corpus rules).
- B-shoulder / B-relative loudness matching.
- Applying normalize to SilenceSplice / dual-fit (still hard-coded `gain: 1.0`).
- Using `fill_level` / `edge_delta_db` as a veto or gain driver (stays report-only).
- Changing `normalize_window_secs`, border geometry, or crossfade.

**Follow-ups (explicitly later, not this PR)**
- Residual seam `g` for true master-level mismatch without mid-gap RMS.
- B-relative matching as a possible full replacement for A-border RMS.
- Ear-pass whether boost-only leaves any quiet-donor regressions worth a second policy.

---

## 2. Why (failure mode)

Today:

```text
gain = clamp(a_border_rms / b_segment_rms, 10^(-max_db/20), 10^(+max_db/20))
```

`b_segment_rms` is the **whole assembled fill**. A loud mid-gap event inflates that RMS and the
scaler attenuates the entire patch toward talking-level A borders. That is the wrong model for
same-master restore: intentional dynamics inside the hole should survive.

Corpus ear labels already treat “loud event inside a quiet/talk gap, quiet at the seams” as often
**correct** ([json-output.md](../json-output.md) § FillLevelCheck — gunshot in a conversation).
A-border whole-fill normalize fights that case. The damaging direction is **gain &lt; 1**.

Boosting quiet fills toward A remains useful (existing
`patch_audio_normalizes_fill_loudness_to_a_border`); this plan keeps that half.

---

## 3. Decision

| Choice | Decision | Reason |
|--------|----------|--------|
| Policy | **Boost-only** | Smallest change that stops the gunshot-crush failure mode |
| Where | Domain `compute_fill_gain` | Single authority; Bracket characterize and any other callers stay consistent |
| Attenuation | Floor gain at **1.0** after the ratio; still apply `max_fill_gain_db` as an **upper** clamp only | Clear semantics: never quieter than the fill we assembled |
| Config | Keep `normalize_fill` default **true**; keep `max_fill_gain_db` meaning “max boost” | Avoid coupling “stop crushing” with “turn normalize off” |
| Dual-fit | Unchanged (unity) | Out of scope; already never attenuates via this path |
| Stacking | Do **not** also ship residual/`g` or default-off in the same change | One audible policy per deliverable |

Pseudo:

```text
if a_border_rms == 0 or b_segment_rms == 0 → 1.0
raw = a_border_rms / b_segment_rms
max_boost = 10^(max_gain_db/20)
gain = clamp(raw, 1.0, max_boost)   // was clamp(raw, 1/max_boost, max_boost)
```

---

## 4. Touch points

| Area | Change |
|------|--------|
| Domain | `policies::compute_fill_gain` — boost-only clamp; refresh unit tests (`compute_fill_gain_clamps_*`) |
| Characterize | Bracket path keeps calling `compute_fill_gain` unchanged |
| Integration | Keep quiet-B boost test green; add loud-B / would-have-attenuated case asserting gain stays 1.0 (or gap RMS not crushed) |
| Docs | Note boost-only in [pipeline.md](../pipeline.md) / repair config mention; archive write-path wording is historical |
| CLI | No new flag required for v1 (`--no-normalize` still disables entirely) |

Optional later (not required to ship): TOML comment that `max_fill_gain_db` is max **boost** only.

---

## 5. Verification

1. **Unit:** `compute_fill_gain` — quiet B → boost ≤ max; loud B → exactly `1.0`; zero RMS → `1.0`.
2. **Integration:** existing `patch_audio_normalizes_fill_loudness_to_a_border` still passes.
3. **Integration (new):** B fill louder than A borders → patched gap interior not attenuated vs `--no-normalize` (or gain field / RMS within ε of unity path).
4. **Lib/clippy:** `cargo test -p clip-sync-repair --lib` + the touched integration test(s).
5. **Ear / corpus (manual, if media handy):** known gunshot-in-talk (or fill-level shape listen set under `gap-files/`) with `--patch-only` / listen — mid-gap event should no longer sound dragged down; spot-check that quiet-donor boosts still sound intentional.

Acceptance: no production path applies `normalize_gain < 1.0` when `normalize_fill` is on.

---

## 6. Implement checklist

- [ ] Update `compute_fill_gain` to floor at 1.0; adjust domain unit tests for clamp semantics
- [ ] Add integration coverage for the would-attenuate (loud fill) case; keep quiet-boost test
- [ ] Doc note: pipeline / operator-facing normalize wording → boost-only
- [ ] Run lib + touched integration tests
- [ ] (Optional) Quick `--patch-only` / listen spot-check on a known loud-mid-gap patch
- [ ] Archive this TEMP when shipped; durable behaviour lives in pipeline / json-output companion notes

---

## 7. Out of scope reminders

Do not treat high `peak_delta_db` or whole-fill RMS vs A as “too loud → attenuate.” If a later plan
gates on level, use registration-conditioned **edge** shape ([json-output.md](../json-output.md)),
not this normalize path.
