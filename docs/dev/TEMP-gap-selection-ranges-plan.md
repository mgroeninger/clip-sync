# Gap selection v1.5 — range tokens (DRAFT)

Status: **rule set, not started.** Blocked on v1. One measurement still wanted (§2, ε magnitudes at
real gap edges) — the *scheme* does not depend on it.

Split out of `TEMP-gap-selection-plan.md` on 2026-07-29.

> **What constrains this document — read before re-litigating anything here.**
>
> 1. **Tokens are identities, never counts** ([TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md)
>    § Selection tokens are identities). Every token resolves independently against the whole
>    `GapReport.gaps` and the results are **unioned**; a token never narrows another token's
>    resolution domain. That rule was settled *because of* the containment token below — without it,
>    `--only-gaps 1:42:00..1:50:00,2` is ambiguous (gap #2, or the 2nd gap inside the window?). Do not
>    re-derive it in isolation here.
> 2. **A gap number is run-scoped; a time range is recipe-stable.** Both are identities. They differ
>    only in how long they stay valid. Range tokens exist to give the cross-rescan handle the identity
>    contract asks for — that is the whole point of the feature.
> 3. **Neither token restricts the scan.** They select from gaps already detected across the whole
>    file. Limiting *detection* to a window is a different axis, deliberately out of scope —
>    [TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md) § `--scan-window`.

> **Verification rule.** `file:line` references and claims about current behavior belong in the
> checklist (§5), where they are about to be executed. Elsewhere, state the decision and its reason.

---

## 1. Grammar: same flags, mixed tokens

`--only-gaps` and `--skip-gaps` accept the same token grammar; skip resolves ranges to gap numbers
first, then subtracts. v1 already stores tokens as `Vec<String>` precisely so this adds no config-type
break.

Auto-detect per token:

| Token shape | Resolution |
|-------------|------------|
| Integer `N` | Gap number `N` (report `#`) |
| `START-END` | **Strict identity** — the single gap whose edges both match within ε |
| `START..END` | **Containment** — every gap whose A window lies entirely inside the interval |

Times accept seconds (`6128.25`) or `H:MM:SS` / `H:MM:SS.mmm` / `M:SS`, matching `format_timestamp`
display. Unmatched or empty resolution → error listing detected gaps (no silent skip).

### `START-END` is a gap *identity*, not a *window* — and needs a companion that is

**A `START-END` token selects exactly one gap: the one whose own edges match.** It is a rescan-stable
spelling of a single `#`. It does **not** select every gap falling inside the interval — a token
spanning three gaps matches zero and errors.

That is a defensible default but a bad guess at user intent: `--only-gaps 1:42:00-1:50:00` reads as
"patch everything in that stretch" to almost everyone. Both behaviors are wanted, for different jobs,
and they must not share a syntax:

| Token | Semantics | Serves |
|-------|-----------|--------|
| `START-END` | **Strict identity.** Matches the single gap whose `video_a_start_secs` / `video_a_end_secs` are both within ε. No match → **error**. | The cross-rescan stable handle. Errors *loudly* when the scan recipe moved the gap — the whole reason to prefer ranges over remembered numbers |
| `START..END` | **Containment: full enclosure** (§3). Selects every gap whose A window lies entirely within `[START − ε, END + ε]`. Zero matches → **error** (empty selection) | "patch this whole stretch"; bulk exclusion via `--skip-gaps` |

Keeping them distinct preserves the rule that stale handles must never silently remap: under
containment, a gap that shifted or split still lands inside a wide window and is quietly selected —
acceptable when the user asked for a region, unacceptable when they meant "that specific gap".

Containment tokens are order-insensitive on overlap (a gap matched by several tokens is selected once)
and compose freely with integer and identity tokens in the same list.

## 2. ε is **dual**, keyed on the token's own precision — settled 2026-07-29

A flat 50 ms ε would make the *gap table* — the surface users are told to copy from — unusable for
range tokens. `format_timestamp` is `secs.round()`: whole seconds. `format_time_range` builds the
table's Range column from it, so a copied `1:42:08 – 1:46:00` is up to **±0.5 s** off the stored
floats. Same-recipe re-scans are frame-stable (sub-block refine is deterministic), so the round-trip
hazard is display quantization, not scan jitter.

| Token form | ε | Why |
|------------|---|-----|
| **Fractional** — `6128.25-6360.0`, `H:MM:SS.mmm` | **50 ms** | JSON / precise copy; well under the default `scan_block_ms = 100` |
| **Whole-second** — `H:MM:SS`, `M:SS`, no fraction | **500 ms** | Half the display `round()` quantum, so copying from the table works |
| Never | `TIME_EPS_SECS = 1e-9` | Wall-clock equality guards only — **not** the product ε |

- The trigger is the **token's** spelling, not which output produced it. `format_time_range_verbose`
  already prints `M:SS.mmm` for spans under `VERBOSE_SUBSECOND_SPAN_SECS` (10 s), so short gaps copied
  from verbose output are fractional and pick up the tight ε automatically.
- Docs should steer identity tokens at JSON `video_a_start_secs` / `video_a_end_secs`; the
  whole-second form is a convenience for table copy, not the recommended handle.
- **Identity still requires a unique match.** With `min_gap_ms` defaulting to 500, a ±500 ms window can
  plausibly enclose two gap starts on a dense scan. Two candidates inside ε → **error**, never pick
  one. The message must name the collision *and* point at the fractional / JSON form, or the escape
  hatch is undiscoverable.
- For `START..END` containment the same dual ε applies to each edge. A 500 ms slack on a bulk window is
  benign; stated explicitly so it is not re-derived.

**Still wanted from a corpus case:** confirmation of the two magnitudes at real gap edges (and that
50 ms is not too tight against sub-block refine). The dual *scheme* is settled and does not depend on
it.

## 3. Containment = **full enclosure** — settled 2026-07-29

A gap is selected by `START..END` only when **both** edges lie inside `[START − ε, END + ε]`. A gap
that straddles a window edge is **not** selected.

**This is the rule the crate already uses for the analogous question.**
`GapReport::gap_outside_reference_coverage` calls `interval_fully_within_window`, and its doc comment
states the reasoning: a straddling gap is only partly covered, so it is "conservatively excluded rather
than partially filled." The motivations differ (there it is media availability, here it is user intent)
but the conclusion is the same — straddling is ambiguous, so exclude — and adopting it means **one
containment rule in the crate, not two**. Overlap-as-select would pull in gaps that mostly sit outside
the requested stretch, weakening the "no quiet remap" spirit that motivated splitting `START-END` from
`START..END` in the first place.

**No corpus case was needed to choose the rule.** One is still wanted to validate the diagnostic
wording and ε behavior at real edges.

### Straddlers must be named, and the error is not enough

Naming the excluded gap only in the empty-selection error covers just the zero-match case. If a window
matches two gaps and half-covers a third, there is no error and the exclusion is silent — exactly the
surprise the rule exists to avoid. So:

- **Zero matches** → error (the v1 empty-selection error), naming any gap that overlapped the window
  but was not enclosed.
- **Some matches, with an excluded straddler** → name it on the **selection filter note**, v1's
  unconditional stderr line, which is already emitted at the right point in the run. Shape:

  ```text
  Gap filter: patching 2 of 6 detected gaps (only-gaps: 1:42:00..1:50:00; gap #4 overlaps the
  window but is not fully inside it — not selected)
  ```

  This is the only place the exclusion becomes visible when other gaps matched.

## 4. What v1.5 does **not** change

- Precedence in `build_gap_fill_plan` (fillability / coverage / equivalence still beat
  `GapNotSelected`).
- The resolved type: still a `HashSet<usize>` of 0-based report indices. Ranges resolve to gap numbers
  during `resolve_gap_selection` and nothing downstream can tell how a gap was named.
- `GapSelectionMode`'s shape — tokens were already `Vec<String>` in v1 for exactly this reason.
- Any JSON contract. Range tokens are input-only.

## 5. Checklist

- [ ] Range-token parser: `START-END` vs `START..END` discrimination; seconds and `H:MM:SS[.mmm]` /
      `M:SS` time forms; per-token fractional-vs-whole-second detection driving ε
- [ ] `resolve_gap_selection`: resolve range tokens to 0-based report indices, union with integer
      tokens, same error vocabulary as v1 ("gap number", per the shipped
      [index convention](archive/TEMP-gap-index-convention-plan.md) § 6 deviation 3)
- [ ] Strict-identity errors: no match; **two candidates inside ε** → error naming both and pointing at
      the fractional / JSON form
- [ ] Containment: full enclosure via the existing `interval_fully_within_window` helper — do not write
      a second containment predicate
- [ ] Straddler diagnostic on the selection filter note (§3) and in the empty-selection error
- [ ] Docs: [gap-repair-guide.md](../gap-repair-guide.md) (steer at JSON `video_a_*_secs` for stable
      handles), [cli-output.md](../cli-output.md) flag grammar
- [ ] Corpus case: validate the two ε magnitudes at real gap edges and the straddler wording

## 6. Tests

| Case | Assert |
|------|--------|
| `START-END` strict identity | Matches one gap; a token spanning several gaps errors |
| ε collision | Two candidates inside ε → error naming both, never a silent pick |
| `START..END` containment | Full enclosure; a straddler is **not** selected; no match → error |
| Mixed token list | `--only-gaps 1:42:00..1:50:00,2` selects the window's gaps **plus** report `#2` — the integer is never a position within the window's result set; each token resolves against the full report independently |
| Overlapping containment tokens | A gap matched by several tokens is selected once |
| ε keying | Whole-second token resolves a gap whose true edge is up to 0.5 s from the displayed value; a fractional token 0.4 s off does **not** match (50 ms ε); ε is keyed on the token's spelling, not on which output the value was copied from |
| Straddler note | Window enclosing 2 of 3 gaps with the third straddling an edge: the straddler is not selected **and** is named on the filter note (non-empty selection ⇒ no error fires, so the note is the only visible signal) |
| `--skip-gaps` parity | Same grammar; resolves to the report-set complement of the equivalent `--only-gaps` |
