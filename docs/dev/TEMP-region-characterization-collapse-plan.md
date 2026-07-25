# `RegionCharacterization` collapse — plan

Status: **planned** (not started). Written 2026-07-24.

Delete `RegionCharacterization` and dispatch on `GapRepairSpec.verdict` instead,
making `spec.verdict` the single source of truth for patch-vs-skip at the
characterize→execute boundary. `characterize_region` returns
`(GapRepairSpec, GapTagsPatchContext)`.

Behavior-**identical**: no PCM, no outcome, no report field, no threshold
changes. This is a type-level collapse of a redundant tag, gated on the existing
`patch_audio_integration` byte-parity suite.

Paths below are relative to
`crates/clip-sync-repair/src/application/patch_audio/` unless stated.

---

## 0. Relationship to other plans (read first)

- **Provenance.** This is the deferred **C1 note** from
  [TEMP-patch-audio-bracket-fill-elimination-plan.md](TEMP-patch-audio-bracket-fill-elimination-plan.md)
  §8, which evaluated the deletion, recommended it, and put it out of scope:
  "a characterize-boundary cleanup, not `bracket_fill` elimination". That plan
  is otherwise complete and is the reason this one is now possible — see §1.
- **Not a perf change.** Nothing here is on a hot path; the enum is matched once
  per *region* (tens of times per run), not per bracket. Do not attach a
  measurement claim to it, and do not bundle it with anything from
  [TEMP-anchor-search-perf-baseline.md](TEMP-anchor-search-perf-baseline.md).
- **8d is the counter-precedent, read it before arguing with §1.**
  [archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md)
  §864 (Fingerprint-unification **8d**, 2026-07-08) deliberately *added* type
  structure here: `Skip` went from a bare `RegionPatchOutcome` to a full
  `Skip`-verdict `GapRepairSpec` so "every region now yields a projectable
  spec". This plan does not reverse 8d — the specs stay. It removes the outer
  tag that 8d made redundant.

---

## 1. Why the enum is redundant *now* (and was not before)

The enum survived two independent unifications because each one left it standing
for the *other's* reason:

| State | `Patch` payload | `Skip` payload | Enum justified? |
|-------|-----------------|----------------|-----------------|
| Pre-8d | spec + `bracket_fill: Option<Vec<f32>>` | `RegionPatchOutcome` | **Yes** — different types entirely |
| After 8d (2026-07-08) | spec + `bracket_fill` | `GapRepairSpec` | **Yes** — `Patch` still carries PCM |
| After C1 (`66f889a`, 2026-07-24) | `GapRepairSpec` | `GapRepairSpec` | **No** — identical payloads |

Today (`region.rs:1064`):

```rust
pub(super) enum RegionCharacterization {
    Patch(GapRepairSpec),
    Skip(GapRepairSpec),
}
```

The only information the tag carries is a boolean, and that boolean already
exists one level down as `GapRepairSpec.verdict`
(`domain/gap_repair_spec.rs:222`): `Patch(GapRepairStrategy)` /
`Skip { cell, reason }`. Two sources of truth for one fact.

**Every construction site already sets both consistently.** Two `Patch` sites
(`region.rs:1123` in `finalize_dual_fit`, `region.rs:2119` at the end of
`characterize_region`) each build `verdict: GapRepairVerdict::Patch(..)`; all
four `Skip` sites (`1126`, `1599`, `1640`, `1880`) go through `skip_region_spec`,
which builds a `Skip` verdict. So the invariant holds by construction — it is
just not expressed in the type.

**The type's guarantee is already unused.** `execute_region_spec`
(`region.rs:1279`) re-matches `spec.verdict` itself and ends in:

```rust
GapRepairVerdict::Skip { .. } => {
    unreachable!("skips are not executed — the loop derives their outcome from the spec (§2.5.5)")
}
```

Nothing downstream relies on having arrived through the `Patch` arm. This is the
load-bearing fact for §3: **the collapse adds no guard and relocates no panic**,
because the panic is already there.

---

## 2. Usage inventory (complete)

Four `match` sites. Only two are real dispatch.

| # | Site | Now | After |
|---|------|-----|-------|
| 1 | `mod.rs:269` — two-pass loop, execute half | `Patch → execute_region_spec`, `Skip → skip_outcome_from_spec` | real dispatch, `is_patch()` predicate |
| 2 | `region.rs:2173` — `prepare_region_patch` shim (used by `anchor_retry.rs:199`) | same shape | same |
| 3 | `region.rs:1253` — `outcome_from_characterization`, preview path (`mod.rs:221`) | forwards to `patched_outcome_from_spec` / `skip_outcome_from_spec` | **merges** into one `outcome_from_spec` |
| 4 | `region.rs:2152` — `characterize_all_regions` (`#[cfg(test)]`) | `Patch(spec) => spec, Skip(spec) => spec` | **disappears** |

Site 4 discards the tag outright and is test-only. Site 3's two callees *each*
already match on `spec.verdict`, so it is a re-dispatch on a fact they re-check.

**The `unreachable!()` count is the headline.** Three are in scope:

- `execute_region_spec` (`region.rs:1394`) — Skip arm. **Stays** (see §3.3).
- `patched_outcome_from_spec` (`region.rs:1242`) — Skip arm. **Deleted.**
- `skip_outcome_from_spec` (`region.rs:1172`) — Patch arm. **Deleted.**

`region.rs` holds a fourth (`1266`, `dual_fit_skipped`: "seam_failure_outcome
always yields a skip outcome"). That one asserts a `RegionPatchOutcome` shape,
not a verdict, and is **out of scope** — do not touch it and do not count it.

A single exhaustive match over `Patch`/`Skip` in the merged `outcome_from_spec`
needs no impossible arm, so both go.

**Confirming evidence that the post-collapse call shape is already correct:** the
existing 8g.2 test (`characterize_all_regions_yields_one_consistent_spec_per_region`,
`region.rs:2340`) already takes `matches!(spec.verdict, GapRepairVerdict::Patch(_))`
as its source of truth and already branches
`if is_patch { patched_outcome_from_spec } else { skip_outcome_from_spec }`. The
test was written against the verdict, not the enum, and needs no rewrite.

---

## 3. Design decisions

### 3.1 `is_patch()` on `GapRepairVerdict`, not `matches!` at each site

Add to the existing `impl GapRepairVerdict` (`domain/gap_repair_spec.rs:359`,
already home to `skip` / `skip_with_cell`):

```rust
/// Does this verdict mean the executor runs? The characterize→execute dispatch
/// predicate — `Patch` specs go to `execute_region_spec`, `Skip` specs have
/// their outcome derived from the spec (§2.5.5).
pub fn is_patch(&self) -> bool {
    matches!(self, GapRepairVerdict::Patch(_))
}
```

Two call sites is thin justification for a helper, but the dispatch *meaning*
("does the executor run?") is worth naming once rather than open-coding a
`matches!` twice in application code — and it is where a doc comment about the
§2.5.5 contract belongs.

### 3.2 Merge the two outcome functions

`patched_outcome_from_spec` + `skip_outcome_from_spec` → one
`outcome_from_spec(spec: &GapRepairSpec, sample_rate: u32) -> RegionPatchOutcome`
matching `spec.verdict` once. This replaces both site 3 and the two
`unreachable!()`s.

**Keep `skip_outcome_from_spec` as well.** Sites 1 and 2 call it on the
already-known-Skip branch and have no `sample_rate` need; making them go through
`outcome_from_spec` would widen the call for nothing. Its `unreachable!()` is
what goes — it becomes a private helper called only from the Skip arm of
`outcome_from_spec` and from the two dispatch sites, or (preferred) the Patch arm
of `outcome_from_spec` inlines the patched construction and the Skip arm calls
`skip_outcome_from_spec`. Decide at implementation time; the constraint is that
**no function retains an arm asserting a verdict it cannot receive.**

### 3.3 `execute_region_spec` keeps its `unreachable!()`

It takes an owned `GapRepairSpec` and destructures `verdict` to reach the
strategy fields; it cannot express "Patch-verdict spec" in its signature without
a new type (§3.4). Its Skip arm is therefore still reachable *by type* and must
stay. This is the honest limit of the collapse: **two sources of truth become
one, but one runtime assert remains.**

### 3.4 Explicitly NOT doing: a `PatchSpec` newtype

The stronger version — a boundary type constructible only for a Patch verdict,
so `execute_region_spec` cannot be called wrongly and the third `unreachable!()`
also goes — is **out of scope**. It means either a `PatchSpec(GapRepairSpec)`
newtype with a checked constructor (which just moves the check, now returning
`Option`), or hoisting `GapRepairStrategy` out of the verdict into the spec so
the executor takes the strategy directly. The latter is a domain-type change
touching the fingerprint projection and is a materially bigger plan.

If someone opens that later: this plan is a strict prerequisite, not a
competitor — the newtype wraps *one* dispatch tag, which is exactly what this
plan leaves behind.

---

## 4. Phases

Small enough to be one commit, split only where a phase boundary buys a
reviewable diff.

| Phase | Change |
|-------|--------|
| **R1** | Add `GapRepairVerdict::is_patch` + `outcome_from_spec`; strip the two `unreachable!()`s. Enum still exists; `outcome_from_characterization` becomes a one-line forward. No dispatch change. |
| **R2** | Flip the four sites to `spec.verdict`. `characterize_region` returns `(GapRepairSpec, GapTagsPatchContext)`; delete the enum, `outcome_from_characterization`, and site 4's match. |

R1 is additive and independently valid (build + clippy clean, tests pass with
the enum still in place). R2 is the flip.

**Do not split R2 further.** Deleting the enum and changing
`characterize_region`'s return type are the same edit — an intermediate state
where the enum exists but nothing constructs it fails
`cargo clippy --all-targets -- -D warnings` on a dead-code lint, which CI
enforces. (Same reason F2 and C1 landed together in the parent plan.)

---

## 5. Ground rules

- **Behavior-identical, verified by the existing suites.** No new fixture. The
  gate is `.\scripts\test-tier.ps1 -Tier pr-repair`; `patch_audio_integration`
  (26 tests) is the byte-parity surface and must stay 26/26 with identical
  outcome fields.
- **`#[cfg(test)]` helpers keep their contract.** `characterize_all_regions`
  still returns `Vec<GapRepairSpec>` and the 8g.2 test still asserts
  region-infallibility (`specs.len() == regions.len()`) and Patch-verdict ⟺
  `RegionPatch`. If that test needs *any* edit beyond an import list, stop —
  something is being changed that this plan says is not being changed.
- **No `#[allow]` added.** If a lint fires, fix the code.
- **The three in-scope `unreachable!()`s are the accounting.** Two must be gone
  at the end and the third (`execute_region_spec`) must remain, with its message
  intact. `dual_fit_skipped`'s is unrelated and stays untouched. A *new*
  verdict-asserting `unreachable!()` anywhere means the dispatch was
  reintroduced.
- Per-phase gate: `cargo build --all-targets` → `cargo clippy --all-targets`
  → `.\scripts\test-tier.ps1 -Tier pr-repair`. Stage only phase-relevant files;
  one commit per phase.

---

## 6. What this does not do

- Does not touch `GapRepairSpec`, `GapRepairVerdict`'s variants, or
  `GapRepairStrategy` — only adds an `is_patch` method.
- Does not change the fingerprint projection, which reads specs and is
  indifferent to how the loop dispatched them (that indifference is 8d's
  achievement and the reason this is safe).
- Does not remove `execute_region_spec`'s `unreachable!()` — §3.3, §3.4.
- Does not change `prepare_region_patch`'s signature; the anchored-retry caller
  (`anchor_retry.rs:199`) is untouched.

---

## 7. Ledger

| Phase | Status | Commit | Notes |
|-------|--------|--------|-------|
| R1 | Planned | — | `is_patch` + merged `outcome_from_spec`; 2 of 3 `unreachable!()`s deleted |
| R2 | Planned | — | Dispatch on `spec.verdict`; enum + `outcome_from_characterization` deleted |
