//! Seam locks — "where does each of A's shoulders match in B?", answered once.
//!
//! Both the production dual-fit repair and the diagnostic `splice_dualfit` need the same two
//! numbers: the lag at which A's pre-gap border matches B, and the lag at which A's post-gap border
//! does. Everything downstream — the donor bridge length, the interior trim, the collapse test, the
//! gate — is arithmetic on those two locks. They were measured in three places
//! (`build_dual_fit_input` + `try_dual_fit`, `splice_dualfit_at`, and the envelope inside
//! `rescue_shoulder`) with comments at each site asserting the copies matched. This module is the
//! one place.
//!
//! **Nominal-first ordering** (`docs/dev/archive/TEMP-lag-nominal-first-plan.md`): measure at the
//! nominal `b_mapped` first; consult the coarse envelope only for a shoulder that reads untrustworthy
//! there, and keep the recentered retry only if it is actually better. A shoulder that already reads
//! well is never touched, so the envelope can never move a good lock onto a worse one.

use crate::domain::lag_envelope::{
    envelope_recenter_frames, envelope_shoulder, EnvelopeCfg, EnvelopeSide,
};
use crate::domain::pcm::interleaved_to_mono;
use crate::domain::seam_local::seam_local_peak;

/// A's two mono seam templates plus the geometry they were cut against.
///
/// Carries the range guard that `build_dual_fit_input` and `splice_dualfit_at` each spelled out
/// separately. **Not** `border_templates_for_gap`: that applies silence-skip and energy trim, which
/// move the pre template off the seam — these must be the raw `mono(refined ± w)` windows, because
/// the lag they produce is the lag a fill is spliced at.
/// **Invariant:** `a_pre.len() == a_post.len() == seam_window_frames`, and `gap_frames > 0`. Every
/// downstream length — the seam search template, the step-real window, the interior-trim edge guard —
/// is `seam_window_frames`, so a template of a different length would silently measure one window and
/// splice against another. The fields are read-only from outside (`pub` getters, private
/// construction) precisely so no caller can build a value that violates it.
pub struct SeamLockTemplates {
    a_pre: Vec<f64>,
    a_post: Vec<f64>,
    gap_frames: usize,
    b_mapped_start: usize,
    seam_window_frames: usize,
}

impl SeamLockTemplates {
    /// The `seam_window_frames` of A **ending at** the gap start.
    pub fn a_pre(&self) -> &[f64] {
        &self.a_pre
    }

    /// The `seam_window_frames` of A **starting at** the gap end.
    pub fn a_post(&self) -> &[f64] {
        &self.a_post
    }

    pub fn gap_frames(&self) -> usize {
        self.gap_frames
    }

    /// Nominal B frame the pre seam butts against.
    pub fn b_mapped_start(&self) -> usize {
        self.b_mapped_start
    }

    pub fn seam_window_frames(&self) -> usize {
        self.seam_window_frames
    }

    /// Build from A's two mono seam borders directly, for callers that already cut them (and for
    /// fixtures). `None` unless the invariant holds — a caller that hands over a border of the wrong
    /// length gets a refusal, not a quietly mismatched fit.
    pub fn from_mono(
        a_pre: Vec<f64>,
        a_post: Vec<f64>,
        gap_frames: usize,
        b_mapped_start: usize,
        seam_window_frames: usize,
    ) -> Option<Self> {
        if a_pre.len() != seam_window_frames
            || a_post.len() != seam_window_frames
            || gap_frames == 0
            || seam_window_frames == 0
        {
            return None;
        }
        Some(Self {
            a_pre,
            a_post,
            gap_frames,
            b_mapped_start,
            seam_window_frames,
        })
    }

    /// `None` when either seam window runs outside A, or the gap is empty — an insufficient shoulder
    /// means no seam-lock claim, which is the same thing both old copies returned.
    pub fn build(
        a_samples: &[f32],
        channels: usize,
        gap_start_frame: usize,
        gap_end_frame: usize,
        b_mapped_start: usize,
        seam_window_frames: usize,
    ) -> Option<Self> {
        let ch = channels.max(1);
        let w = seam_window_frames;
        let a_frames = a_samples.len() / ch;
        let gap_frames = gap_end_frame.saturating_sub(gap_start_frame);
        if gap_start_frame < w || gap_end_frame + w > a_frames || gap_frames == 0 {
            return None;
        }
        let a_pre = interleaved_to_mono(
            &a_samples[(gap_start_frame - w) * ch..gap_start_frame * ch],
            ch,
        );
        let a_post =
            interleaved_to_mono(&a_samples[gap_end_frame * ch..(gap_end_frame + w) * ch], ch);
        Self::from_mono(a_pre, a_post, gap_frames, b_mapped_start, w)
    }

    /// Nominal B center for the post seam.
    pub fn b_post_nominal(&self) -> usize {
        self.b_mapped_start + self.gap_frames
    }
}

/// One shoulder's lock: its correlation, the lag (frames) it matched at relative to its nominal
/// center, and the peak's z-score.
#[derive(Debug, Clone, Copy)]
pub struct SeamLock {
    pub r: f64,
    pub lag: i64,
    pub z: Option<f64>,
    /// `true` when the coarse envelope moved this shoulder off its nominal center.
    pub rescued: bool,
    /// The accepted peak sat on the boundary of the lags actually searched (`SeamPeak::edge_pinned`) —
    /// the search was exhausted, so a larger lag was never scored. Of the **accepted** search: after a
    /// rescue this describes the recentered sweep, not the nominal probe.
    pub edge_pinned: bool,
}

/// Both shoulders, plus everything derivable from them.
#[derive(Debug, Clone, Copy)]
pub struct SeamLocks {
    pub pre: SeamLock,
    pub post: SeamLock,
    /// Matched B frame for each shoulder — the placement a fill would be cut at.
    pub b_pre_seam: usize,
    pub b_post_seam: usize,
    /// Donor material available between the two locks. `≤ 0` means the shoulders abut or cross:
    /// B holds nothing for this gap.
    pub bridge_frames: i64,
    /// Both shoulders clear the fill gate. Computed here so `try_dual_fit` and `splice_dualfit`
    /// cannot disagree about it.
    pub gate_pass: bool,
    /// `nominal + lag` before the `.max(0)` clamp that produces `b_*_seam`. Kept because the clamp can
    /// *manufacture* an abut: two negative seams collapse onto frame 0 and read as `bridge_frames == 0`.
    /// See [`SeamLocks::clamped`].
    pub b_pre_seam_unclamped: i64,
    pub b_post_seam_unclamped: i64,
}

impl SeamLocks {
    /// Qualified collapse: the locks are good enough to believe *and* they leave no donor bridge.
    ///
    /// Order matters and matches `try_dual_fit`'s: `gate_pass` first. Two dead shoulders produce
    /// noise lags, and noise lags cross about half the time — an unqualified collapse would preempt
    /// a legitimate decision on every decorrelated gap.
    pub fn abut(&self) -> bool {
        self.gate_pass && self.bridge_frames <= 0
    }

    /// The abut exists only because of the `.max(0)` clamp: unclamped the shoulders still leave a bridge.
    /// A lookalike tag, not a separate reason — the successor collapse Change reads it before draining.
    pub fn clamped(&self) -> bool {
        self.b_post_seam_unclamped - self.b_pre_seam_unclamped > 0 && self.bridge_frames <= 0
    }
}

/// The two gate floors, together, so a caller cannot pass one and forget the other.
#[derive(Debug, Clone, Copy)]
pub struct SeamGateFloors {
    pub min_fill_correlation: f64,
    pub fill_absolute_floor: f64,
}

impl SeamGateFloors {
    /// `try_dual_fit`'s gate, unchanged: reject when `smin < min_fill_correlation` or
    /// `smin < fill_absolute_floor`.
    ///
    /// Both floors are checked even though one is normally the stricter — they are independently
    /// configurable, so which one binds is not fixed. `smin` is finite by construction
    /// (`seam_local_peak` returns `finite(peak_r)`), so `>=` and `!(<)` agree here and the plain
    /// form is used.
    pub fn passes(&self, smin: f64) -> bool {
        smin >= self.min_fill_correlation && smin >= self.fill_absolute_floor
    }
}

/// Lazily-supplied wider A border for the coarse envelope search only.
///
/// Lazy because it is called only once a shoulder is confirmed untrustworthy at nominal, never up
/// front. `None` disables the widening: the envelope then correlates the seam template itself, which
/// is a *narrower* search than the recipe asks for — so a caller that can produce the wider border
/// should, and both callers that decode A whole use [`wide_a_slicer`].
pub type WideA<'a> = dyn Fn(bool) -> Option<Vec<f64>> + 'a;

/// Coarse-envelope *input* for [`nominal_first_seam_locks`]: knobs plus the lazy wider A border.
///
/// The two travel together — both `None` on the dual-fit path (measure at nominal only), both
/// populated on the patch/fingerprint paths that may rescue. Named so it cannot be confused with
/// the fingerprint dump's `EnvelopeRescue` outcome (`NotConsulted` / `Declined` / `Rescued`).
#[derive(Clone, Copy)]
pub struct EnvelopeSearch<'a> {
    pub cfg: Option<EnvelopeCfg>,
    pub wide_a: Option<&'a WideA<'a>>,
}

impl EnvelopeSearch<'static> {
    /// Measure at the nominal `b_mapped` only — no envelope, no wider border.
    pub fn off() -> Self {
        Self {
            cfg: None,
            wide_a: None,
        }
    }
}

/// The wider A border as a plain slice of already-decoded interleaved A.
///
/// Both `characterize_region` (production) and `splice_dualfit_at` (fingerprint) hold all of A, so
/// their "lazy fetch" is a slice plus a downmix. Shared rather than written twice because the two
/// are compared axis-by-axis: a fingerprint that widened its border differently from production
/// would mispredict exactly the shoulders the envelope exists to rescue. `wide_window` of 0 (envelope
/// off) yields `None` on both sides, and so does a window that runs off either end of A.
pub fn wide_a_slicer(
    a_samples: &[f32],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    wide_window: usize,
) -> impl Fn(bool) -> Option<Vec<f64>> + '_ {
    let ch = channels.max(1);
    let a_frames = a_samples.len() / ch;
    move |pre_shoulder: bool| {
        let (lo, hi) = if pre_shoulder {
            (gap_start_frame.checked_sub(wide_window)?, gap_start_frame)
        } else {
            (gap_end_frame, gap_end_frame.checked_add(wide_window)?)
        };
        (hi <= a_frames && hi > lo).then(|| interleaved_to_mono(&a_samples[lo * ch..hi * ch], ch))
    }
}

/// Measure one shoulder, nominal first.
///
/// `triage_r` is the "do I need help?" line, **not** a gate: a shoulder at or above it keeps its
/// nominal lag and the envelope is never consulted, so it cannot be moved. Below it, the coarse
/// envelope proposes a recenter and the fine search re-runs there — and that retry is kept only if
/// it clears `triage_r` itself. Otherwise the honest nominal reading stands.
///
/// Both callers pass `min_fill_correlation` — the fingerprint from its recipe
/// (`measure/probes.rs`), the patch path from `df.params` (`region/characterize.rs`) — so the
/// dump predicts the repair on this axis. It stays an **argument** rather than a constant because
/// the correct value is not a literal: it is whatever the recipe's fill gate is. Tying triage to
/// the gate floor is what buys the invariant *a lock the gate would accept at its nominal lag is
/// never a rescue candidate, so the envelope can only add a fill that is declined today — it can
/// never move an existing one*, and that holds at any configured `min_fill_correlation` only if
/// the two track each other. (The 1 s `lag_decision` census keeps its own
/// [`crate::domain::lag_envelope::LAG_DECISION_RECOVERABLE_R`] = 0.5; that is a different instrument
/// — a 1 s template at `b_mapped` rather than a 250 ms one at the seam — not this line.)
fn lock_shoulder(
    templates: &SeamLockTemplates,
    b_mono: &[f64],
    max_lag_frames: usize,
    pre_shoulder: bool,
    envelope: EnvelopeSearch<'_>,
    sample_rate: u32,
    triage_r: f64,
) -> Option<SeamLock> {
    let a_seam = if pre_shoulder {
        templates.a_pre()
    } else {
        templates.a_post()
    };
    let nominal_center = if pre_shoulder {
        templates.b_mapped_start()
    } else {
        templates.b_post_nominal()
    };
    // The pre seam's search window starts a full template before its nominal center.
    let anchor_start = if pre_shoulder {
        nominal_center.checked_sub(a_seam.len())?
    } else {
        nominal_center
    };
    let peak = seam_local_peak(a_seam, b_mono, anchor_start, max_lag_frames)?;
    let r = peak.r;
    let nominal = SeamLock {
        r,
        lag: peak.lag,
        z: peak.z,
        rescued: false,
        edge_pinned: peak.edge_pinned(sample_rate),
    };

    let Some(env_cfg) = envelope.cfg else {
        return Some(nominal);
    };
    if r >= triage_r {
        // Trustworthy at nominal — the envelope is never consulted, so this lock cannot be moved.
        // Same *shape* as the 1 s census's `is_recoverable`, but not the same line: that one tests
        // `peak_r >= LAG_DECISION_RECOVERABLE_R`, this one tests the caller's gate floor.
        return Some(nominal);
    }

    let wide = envelope.wide_a.and_then(|f| f(pre_shoulder));
    let a_border: &[f64] = wide.as_deref().unwrap_or(a_seam);
    let peak = envelope_shoulder(
        EnvelopeSide {
            a_border,
            b_signal: b_mono,
            anchor_frame: nominal_center,
            pre_shoulder,
        },
        env_cfg,
        sample_rate,
    );
    let shift = envelope_recenter_frames(peak, sample_rate);
    if shift == 0 {
        return Some(nominal);
    }

    let recentered = (anchor_start as i64 + shift).max(0) as usize;
    let Some(retry) = seam_local_peak(a_seam, b_mono, recentered, max_lag_frames) else {
        return Some(nominal);
    };
    if retry.r < triage_r {
        // Rescued but still untrustworthy — keep the honest nominal rather than a second guess.
        return Some(nominal);
    }
    Some(SeamLock {
        r: retry.r,
        // Report the lag relative to the ORIGINAL nominal center, so `b_*_seam` arithmetic below is
        // identical whether or not a rescue happened.
        lag: shift + retry.lag,
        z: retry.z,
        rescued: true,
        edge_pinned: retry.edge_pinned(sample_rate),
    })
}

/// Both seam locks with the nominal-first ordering, plus `gate_pass` and the bridge.
///
/// `None` only when a seam window is out of range — the same condition on which `try_dual_fit`
/// declined before.
pub fn nominal_first_seam_locks(
    templates: &SeamLockTemplates,
    b_mono: &[f64],
    max_lag_frames: usize,
    envelope: EnvelopeSearch<'_>,
    sample_rate: u32,
    floors: SeamGateFloors,
    triage_r: f64,
) -> Option<SeamLocks> {
    let pre = lock_shoulder(
        templates,
        b_mono,
        max_lag_frames,
        true,
        envelope,
        sample_rate,
        triage_r,
    )?;
    let post = lock_shoulder(
        templates,
        b_mono,
        max_lag_frames,
        false,
        envelope,
        sample_rate,
        triage_r,
    )?;

    let b_pre_seam_unclamped = templates.b_mapped_start() as i64 + pre.lag;
    let b_post_seam_unclamped = templates.b_post_nominal() as i64 + post.lag;
    let b_pre_seam = b_pre_seam_unclamped.max(0) as usize;
    let b_post_seam = b_post_seam_unclamped.max(0) as usize;
    Some(SeamLocks {
        pre,
        post,
        b_pre_seam,
        b_post_seam,
        bridge_frames: b_post_seam as i64 - b_pre_seam as i64,
        gate_pass: floors.passes(pre.r.min(post.r)),
        b_pre_seam_unclamped,
        b_post_seam_unclamped,
    })
}

/// Frozen post-grow ladder (ms) — `docs/dev/archive/TEMP-dual-fit-grow-post-plan.md` §3.2. Do not add, remove,
/// or reorder without updating the plan and [`grow_post_lock`]'s callers.
pub const POST_GROW_LADDER_MS: [u32; 5] = [375, 500, 625, 750, 1000];

/// Admission line for a grown post lock: a rung's `post_r` must clear this to stop the ladder. Leaves
/// the `[0.35, 0.5)` band — 0.35 is what let a junk 250 ms lock through the fill gate (plan §3.2).
///
/// Deliberately independent of the run's configured `min_fill_correlation`/`fill_absolute_floor` —
/// it is not a second copy of the fill gate, and coupling it (e.g. `max(0.5, min_fill_correlation)`)
/// would let a raised floor grow the ladder further with no corpus evidence that is safe. If a run
/// ever configures either floor above 0.5, a grown lock can stop here and still fail that stricter
/// gate; that is intended, not a bug. Revisit only with corpus evidence at a raised floor.
pub const POST_GROW_ADMIT_R: f64 = 0.5;

/// `|Δpre|` snap line (ms): when the 250 ms lock's `|post_lag − pre_lag|` exceeded this, a grown rung
/// must ALSO bring its own `|Δpre|` within it to stop the ladder — not `post_r` alone. Seam-scale, not
/// speech-offset-scale (plan §3.2).
pub const POST_GROW_DELTA_PRE_SNAP_MS: f64 = 10.0;

/// A grown post lock: the raw `a_post` template — deliberately NOT run through [`SeamLockTemplates`],
/// whose invariant requires `a_pre.len() == a_post.len()` and a grown post is by definition wider than
/// the 250 ms `a_pre` (plan §0 lock 2) — plus the [`SeamLock`] it measured and the window width (ms)
/// for the caller's `dual_fit_post_window_ms` stamp.
pub struct GrownPostLock {
    pub a_post: Vec<f64>,
    pub window_frames: usize,
    pub window_ms: u32,
    pub lock: SeamLock,
}

/// Re-lock the **post** seam at the shortest window in [`POST_GROW_LADDER_MS`] that stops the ladder
/// (`docs/dev/archive/TEMP-dual-fit-grow-post-plan.md` §3.2), envelope OFF, same start (`end_frame`) as the
/// 250 ms lock. The caller decides *whether* to grow (the 250 ms `post.r < 0.5` admission test lives in
/// `skip_or_dual_fit`, not here) — this function is a pure measurement, mirroring the fingerprint's
/// `post_window_ladder_at` (`application/gap_fingerprint/measure/probes.rs`) with the same primitive
/// (`seam_local_peak`, envelope off) but stopping at the first qualifying rung instead of collecting all
/// of them.
///
/// `pre_lag` and `original_post_lag` are the 250 ms lock's own lags — used to compute that lock's
/// `|Δpre|` (`(post_lag − pre_lag)` in ms), which decides whether a rung must also snap `|Δpre|` to
/// [`POST_GROW_DELTA_PRE_SNAP_MS`] or `post_r >= `[`POST_GROW_ADMIT_R`]` alone is enough to stop.
///
/// `None` when no rung stops — the caller keeps the original 250 ms locks untouched, including a
/// failing `gate_pass`.
#[allow(clippy::too_many_arguments)]
pub fn grow_post_lock(
    a_samples: &[f32],
    channels: usize,
    end_frame: usize,
    b_mono: &[f64],
    b_mapped_start: usize,
    gap_frames: usize,
    pre_lag: i64,
    max_lag_frames: usize,
    sample_rate: u32,
    original_post_lag: i64,
) -> Option<GrownPostLock> {
    let ch = channels.max(1);
    let rate = f64::from(sample_rate).max(1.0);
    let a_frames = a_samples.len() / ch;
    let anchor_start = b_mapped_start + gap_frames;
    let original_delta_pre_ms = (original_post_lag - pre_lag) as f64 * 1000.0 / rate;
    let require_delta_pre = original_delta_pre_ms.abs() > POST_GROW_DELTA_PRE_SNAP_MS;

    for &window_ms in POST_GROW_LADDER_MS.iter() {
        let w = ((f64::from(window_ms) / 1000.0) * rate).round() as usize;
        if w < 8 || end_frame + w > a_frames {
            continue;
        }
        let a_post = interleaved_to_mono(&a_samples[end_frame * ch..(end_frame + w) * ch], ch);
        let Some(peak) = seam_local_peak(&a_post, b_mono, anchor_start, max_lag_frames) else {
            continue;
        };
        if peak.r < POST_GROW_ADMIT_R {
            continue;
        }
        let delta_pre_ms = (peak.lag - pre_lag) as f64 * 1000.0 / rate;
        if require_delta_pre && delta_pre_ms.abs() > POST_GROW_DELTA_PRE_SNAP_MS {
            continue;
        }
        return Some(GrownPostLock {
            window_frames: a_post.len(),
            a_post,
            window_ms,
            lock: SeamLock {
                r: peak.r,
                lag: peak.lag,
                z: peak.z,
                rescued: false,
                edge_pinned: peak.edge_pinned(sample_rate),
            },
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(r: f64, lag: i64, edge_pinned: bool) -> SeamLock {
        SeamLock {
            r,
            lag,
            z: None,
            rescued: false,
            edge_pinned,
        }
    }

    fn locks(pre_unclamped: i64, post_unclamped: i64, gate_pass: bool) -> SeamLocks {
        let b_pre_seam = pre_unclamped.max(0) as usize;
        let b_post_seam = post_unclamped.max(0) as usize;
        SeamLocks {
            pre: lock(0.9, 0, false),
            post: lock(0.9, 0, false),
            b_pre_seam,
            b_post_seam,
            bridge_frames: b_post_seam as i64 - b_pre_seam as i64,
            gate_pass,
            b_pre_seam_unclamped: pre_unclamped,
            b_post_seam_unclamped: post_unclamped,
        }
    }

    /// `clamped` is the lookalike tag for "the `.max(0)` manufactured this abut": both seams landed left of
    /// B's start, so they collapse onto frame 0 and read as a zero bridge that the unclamped arithmetic does
    /// not show. The successor collapse Change refuses to drain A's PCM on a row carrying this tag.
    #[test]
    fn clamped_marks_an_abut_the_clamp_manufactured() {
        let manufactured = locks(-500, -100, true);
        assert!(manufactured.abut(), "clamped seams both land on frame 0");
        assert_eq!(manufactured.bridge_frames, 0);
        assert!(
            manufactured.clamped(),
            "but unclamped they leave a 400-frame bridge"
        );

        let real = locks(10_000, 9_800, true);
        assert!(real.abut());
        assert!(
            !real.clamped(),
            "a genuine crossing is not a clamp artifact"
        );

        let in_reach = locks(10_000, 10_400, true);
        assert!(!in_reach.abut());
        assert!(
            !in_reach.clamped(),
            "`clamped` never fires without an abut to explain"
        );
    }

    /// `abut` is `gate_pass` **first**: two dead shoulders produce noise lags that cross about half the
    /// time, so an unqualified collapse would preempt a legitimate decision on every decorrelated gap.
    #[test]
    fn unqualified_locks_never_abut() {
        assert!(!locks(10_000, 9_800, false).abut());
        assert!(locks(10_000, 9_800, true).abut());
    }

    /// Deterministic broadband noise — a periodic signal would lock equally well at every period and
    /// prove nothing about *where* a search landed.
    fn noise(len: usize, seed: u32) -> Vec<f64> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f64::from((state >> 16) as i16) / 32_768.0
            })
            .collect()
    }

    /// **The invariant that lets the envelope run in production** (§7 step 1, §9): the patch path
    /// passes `min_fill_correlation` as `triage_r` (as does the fingerprint), so a lock that would pass the gate at its nominal
    /// lag is never a rescue candidate — the envelope is not even consulted — and therefore can never
    /// be relocated onto a decoy. Rescue can only ever *add* a fill that is declined today.
    ///
    /// The fixture is the one §9 names: a partly-degraded shoulder that still reads above the gate
    /// floor at nominal, with a verbatim copy of A's border planted elsewhere in B as a decoy that
    /// correlates far better. With triage at the gate floor the nominal lag survives; at a triage
    /// line above that floor it would not — that arm is asserted too, with a **local** literal
    /// rather than any shipped constant, so the behavior is pinned to the *argument* and the test
    /// does not couple itself to an unrelated instrument's line.
    #[test]
    fn a_gate_passing_nominal_lock_is_never_relocated() {
        let sr = 48_000u32;
        let w = sr as usize / 4; // 250 ms seam window, as the gate uses
        let gap_frames = sr as usize / 2;

        // B: noise, with A's exact pre-border planted 3 s later as a strong decoy.
        let mut b = noise(sr as usize * 8, 0x5eed_1234);
        let nominal_pre_start = sr as usize * 2;
        let template: Vec<f64> = b[nominal_pre_start..nominal_pre_start + w].to_vec();
        let decoy_at = nominal_pre_start + sr as usize * 3;
        b[decoy_at..decoy_at + w].copy_from_slice(&template);

        // A's pre border is that template half-degraded — still well above the 0.35 gate floor at
        // nominal, but below the 0.5 used by the second arm, so that arm has something to move.
        let dirt = noise(w, 0xabcd_0001);
        let a_pre: Vec<f64> = template
            .iter()
            .zip(&dirt)
            .map(|(t, d)| 0.30 * t + 0.70 * d)
            .collect();
        let a_post: Vec<f64> =
            b[nominal_pre_start + w + gap_frames..nominal_pre_start + w + gap_frames + w].to_vec();

        let templates = SeamLockTemplates::from_mono(
            a_pre.clone(),
            a_post,
            gap_frames,
            nominal_pre_start + w,
            w,
        )
        .expect("templates");
        let floors = SeamGateFloors {
            min_fill_correlation: 0.35,
            fill_absolute_floor: 0.12,
        };
        // Wide enough to reach the decoy, so the envelope genuinely could move this lock.
        let envelope = EnvelopeCfg::from_knobs(4_000, 1.0, sr);
        let wide: Box<WideA<'_>> = Box::new(|pre: bool| pre.then(|| a_pre.clone()));

        let at_gate_floor = nominal_first_seam_locks(
            &templates,
            &b,
            (0.6 * f64::from(sr)) as usize,
            EnvelopeSearch {
                cfg: envelope,
                wide_a: Some(wide.as_ref()),
            },
            sr,
            floors,
            0.35,
        )
        .expect("locks");
        assert!(
            at_gate_floor.pre.r >= 0.35,
            "fixture premise: the nominal read must clear the gate floor, got {}",
            at_gate_floor.pre.r
        );
        assert!(
            !at_gate_floor.pre.rescued && at_gate_floor.pre.lag.abs() < w as i64,
            "a gate-passing nominal lock must keep its lag, got rescued={} lag={}",
            at_gate_floor.pre.rescued,
            at_gate_floor.pre.lag
        );

        // Same measurement at a triage line above the gate floor — a value no caller passes, held
        // locally on purpose — which makes the shoulder a rescue candidate.
        let above_floor = 0.5;
        // Whether the envelope actually moves it is the envelope's business — what this pins is that
        // `triage_r` is a live parameter, not decoration.
        let at_recoverable_line = nominal_first_seam_locks(
            &templates,
            &b,
            (0.6 * f64::from(sr)) as usize,
            EnvelopeSearch {
                cfg: envelope,
                wide_a: Some(wide.as_ref()),
            },
            sr,
            floors,
            above_floor,
        )
        .expect("locks");
        assert!(
            at_recoverable_line.pre.r < above_floor,
            "fixture premise: the nominal read must sit in the [0.35, 0.5) band, got {}",
            at_recoverable_line.pre.r
        );
    }

    /// Per-100 ms-bin gain profile — the *energy* contour the coarse envelope locks onto.
    /// Deterministic and non-repeating, so the envelope has one unambiguous peak rather than a comb.
    fn bin_gains(bins: usize, seed: u32) -> Vec<f64> {
        let mut state = seed;
        (0..bins)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                0.05 + f64::from((state >> 24) as u8) / 255.0
            })
            .collect()
    }

    /// **The fallback branch**: a rescue that lands somewhere no better is *discarded*, not reported.
    ///
    /// The envelope is a coarse **energy** instrument, so it can confidently align two shoulders that
    /// share a loudness contour and no waveform at all. On such a reading the recentered fine retry is
    /// exactly as untrustworthy as the nominal one — keeping it would hand `try_dual_fit` a second
    /// guess dressed as a measurement, and would tell the dump `rescued: true` when nothing was
    /// recovered. The honest nominal is the only defensible answer, and it must come back
    /// *bit-identical* to the pre-envelope reading.
    ///
    /// Fixture: A's pre border and B are independent noise (so no lag correlates), but A's border
    /// carries B's bin energy contour displaced by 400 ms — the envelope therefore proposes a large
    /// recenter and the retry there still reads ~0. All three premises are asserted, because a fixture
    /// that never consulted the envelope, or that drew `shift == 0`, would pass this test through an
    /// early return without ever reaching the branch under test.
    #[test]
    fn a_rescue_that_lands_no_better_keeps_the_honest_nominal() {
        let sr = 48_000u32;
        let rate = sr as usize;
        let w = rate / 4; // 250 ms seam template
        let bin = rate / 10; // 100 ms, the envelope's bin
        let gap_frames = rate / 2;
        let wide_len = rate * 2; // 2 s border, of which the envelope reads the last 1 s
        let n = rate * 5; // nominal pre-seam end in B
        let d = bin * 4; // 400 ms: the planted energy displacement
        let seam_max_lag = (0.6 * f64::from(sr)) as usize;
        let floor = 0.35; // the recipe's `min_fill_correlation`, as both callers pass

        // B: one noise realization, shaped by a per-bin energy contour.
        let gains = bin_gains(rate * 10 / bin + 2, 0x60ad_1111);
        let b: Vec<f64> = noise(rate * 10, 0xb00b_2222)
            .iter()
            .enumerate()
            .map(|(i, x)| x * gains[i / bin])
            .collect();

        // A's pre border: an *independent* noise realization wearing B's contour from `d` later, so
        // the envelope has a strong, correct-looking peak and the waveform has none anywhere.
        let x = noise(wide_len, 0x0a0a_3333);
        let a_wide: Vec<f64> = (0..wide_len)
            .map(|j| x[j] * gains[(n - wide_len + j + d) / bin])
            .collect();
        let a_pre: Vec<f64> = a_wide[wide_len - w..].to_vec();
        // The post shoulder is verbatim B, so it locks at nominal and contributes nothing to read.
        let a_post: Vec<f64> = b[n + gap_frames..n + gap_frames + w].to_vec();

        let templates = SeamLockTemplates::from_mono(a_pre.clone(), a_post, gap_frames, n, w)
            .expect("templates");
        let cfg = EnvelopeCfg::from_knobs(1_000, 1.0, sr).expect("envelope enabled");
        let wide: Box<WideA<'_>> = Box::new(move |pre: bool| pre.then(|| a_wide.clone()));

        // Premise 1: the nominal read is below the gate floor, so the envelope IS consulted.
        let anchor_start = n - w;
        let nominal =
            seam_local_peak(&a_pre, &b, anchor_start, seam_max_lag).expect("nominal peak");
        assert!(
            nominal.r < floor,
            "fixture premise: nominal must be a rescue candidate, got r={}",
            nominal.r
        );

        // Premise 2: the envelope proposes a real recenter — `shift == 0` returns early and would
        // never reach the branch. It should land on the planted displacement, not on noise.
        let shift = envelope_recenter_frames(
            envelope_shoulder(
                EnvelopeSide {
                    a_border: &wide(true).expect("wide border"),
                    b_signal: &b,
                    anchor_frame: n,
                    pre_shoulder: true,
                },
                cfg,
                sr,
            ),
            sr,
        );
        assert_ne!(
            shift, 0,
            "fixture premise: the envelope must want to move this lock"
        );
        assert!(
            (shift.abs() - d as i64).abs() <= bin as i64,
            "fixture premise: the envelope should find the planted {d}-frame contour, got {shift}"
        );

        // Premise 3: and the retry there is no better — this is what makes the rescue unrecoverable.
        let retry_at = (anchor_start as i64 + shift).max(0) as usize;
        let retry = seam_local_peak(&a_pre, &b, retry_at, seam_max_lag).expect("retry peak");
        assert!(
            retry.r < floor,
            "fixture premise: the recentered retry must still fail triage, got r={}",
            retry.r
        );

        let locks = nominal_first_seam_locks(
            &templates,
            &b,
            seam_max_lag,
            EnvelopeSearch {
                cfg: Some(cfg),
                wide_a: Some(wide.as_ref()),
            },
            sr,
            SeamGateFloors {
                min_fill_correlation: floor,
                fill_absolute_floor: 0.12,
            },
            floor,
        )
        .expect("locks");

        assert!(
            !locks.pre.rescued,
            "an unrecoverable rescue must not be reported as one"
        );
        assert_eq!(
            locks.pre.r, nominal.r,
            "the fallback must return the nominal reading itself, not the retry's"
        );
        assert_eq!(
            locks.pre.lag, nominal.lag,
            "the fallback must keep the nominal lag, not the recentered one"
        );
        assert!(
            !locks.gate_pass,
            "and a shoulder this dead must not qualify the pair to abut"
        );
    }

    fn frames_at_ms(rate: u32, ms: u64) -> usize {
        ((u64::from(rate) * ms) / 1000) as usize
    }

    /// `grow_post_lock` reads only `a_samples[end_frame..]` — the fixtures below leave everything
    /// before `end_frame` at zero, since the pre shoulder plays no part in this function.
    ///
    /// Quiet-then-speech post collar (mirrors `post_window_ladder`'s fixture in
    /// `application/gap_fingerprint/measure/probes.rs`, `docs/dev/archive/TEMP-dual-fit-grow-post-plan.md`
    /// §3.2): the 250 ms rung (below the ladder's own floor) and the 375/500 ms rungs sit entirely in
    /// a quiet noise floor that never correlates with B; only once a rung reaches far enough to
    /// include the real, B-matching content does `post_r` clear the admit line. The ladder must stop
    /// at the shortest such rung, not the first one tried.
    #[test]
    fn grow_post_lock_stops_at_the_shortest_rung_that_clears_the_admit_line() {
        let sr = 48_000u32;
        let quiet_frames = frames_at_ms(sr, 550);
        let speech_frames = frames_at_ms(sr, 900);
        let post_len = quiet_frames + speech_frames;
        let gap_end = frames_at_ms(sr, 2000);
        let total = gap_end + post_len + frames_at_ms(sr, 200);
        let shift = frames_at_ms(sr, 40) as i64;

        let quiet_noise = noise(quiet_frames, 0x9001);
        let speech_noise = noise(speech_frames, 0x9002);
        let mut a = vec![0.0f32; total];
        for (j, slot) in a[gap_end..gap_end + quiet_frames].iter_mut().enumerate() {
            *slot = (quiet_noise[j] * 0.002) as f32;
        }
        for (j, slot) in a[gap_end + quiet_frames..gap_end + post_len]
            .iter_mut()
            .enumerate()
        {
            *slot = (speech_noise[j] * 0.5) as f32;
        }

        let mut b_mono = noise(total + shift.unsigned_abs() as usize + 8, 0x9003);
        for v in &mut b_mono {
            *v *= 0.002;
        }
        for f in gap_end + quiet_frames..gap_end + post_len {
            b_mono[(f as i64 + shift) as usize] = f64::from(a[f]);
        }

        let max_lag_frames = frames_at_ms(sr, 600);
        let grown = grow_post_lock(
            &a,
            1,
            gap_end,
            &b_mono,
            gap_end - frames_at_ms(sr, 800),
            frames_at_ms(sr, 800),
            0,
            max_lag_frames,
            sr,
            0,
        )
        .expect("a wide-enough rung reaches the real content and clears the admit line");

        assert!(
            grown.lock.r >= POST_GROW_ADMIT_R,
            "the returned rung must itself clear the admit line: r={}",
            grown.lock.r
        );
        assert_eq!(
            grown.window_frames,
            frames_at_ms(sr, u64::from(grown.window_ms)),
            "window_frames must match the stamped window_ms"
        );
        assert!(
            grown.window_ms > 250,
            "the 250 ms nominal window is not itself a ladder rung; the shortest rung is 375 ms"
        );
        let shift_ms = shift as f64 * 1000.0 / f64::from(sr);
        assert!(
            (grown.lock.lag as f64 * 1000.0 / f64::from(sr) - shift_ms).abs() < 2.0,
            "the qualifying rung must lock onto the real content's true shift, got lag_ms={}",
            grown.lock.lag as f64 * 1000.0 / f64::from(sr)
        );

        // Confirm the shorter rungs genuinely fail on their own — otherwise this test would pass
        // even if the ladder stopped at the first rung instead of the shortest *qualifying* one.
        for &short_ms in [375u32, 500].iter() {
            let w = frames_at_ms(sr, u64::from(short_ms));
            let a_post = interleaved_to_mono(&a[gap_end..gap_end + w], 1);
            let peak = seam_local_peak(&a_post, &b_mono, gap_end, max_lag_frames)
                .expect("seam search in range");
            assert!(
                peak.r < POST_GROW_ADMIT_R,
                "fixture premise: the {short_ms}ms rung must not itself qualify, got r={}",
                peak.r
            );
        }
    }

    /// No rung ever correlates with B — a fully decorrelated post shoulder — so the ladder exhausts
    /// itself and `grow_post_lock` reports the honest `None` rather than returning a low-confidence
    /// rung.
    #[test]
    fn grow_post_lock_returns_none_when_no_rung_admits() {
        let sr = 48_000u32;
        let gap_end = frames_at_ms(sr, 2000);
        let post_len = frames_at_ms(sr, 1200);
        let total = gap_end + post_len + frames_at_ms(sr, 100);

        let a_noise = noise(post_len, 0xA001);
        let mut a = vec![0.0f32; total];
        for (j, &v) in a_noise.iter().enumerate() {
            a[gap_end + j] = v as f32;
        }
        let b_mono: Vec<f64> = noise(total, 0xB002);

        let max_lag_frames = frames_at_ms(sr, 600);
        let grown = grow_post_lock(
            &a,
            1,
            gap_end,
            &b_mono,
            gap_end - frames_at_ms(sr, 800),
            frames_at_ms(sr, 800),
            0,
            max_lag_frames,
            sr,
            0,
        );
        assert!(
            grown.is_none(),
            "an unmatched post shoulder must not be reported as a rescued rung"
        );
    }

    /// §3.2: a rung with `post_r >= 0.5` is not enough to stop the ladder when the 250 ms lock's own
    /// `|Δpre|` exceeded [`POST_GROW_DELTA_PRE_SNAP_MS`] — the grown rung must ALSO bring `|Δpre|`
    /// back within the snap line. Same quiet-then-speech shoulder both times; only `pre_lag` changes,
    /// which is what moves `|Δpre|` in and out of the snap line.
    #[test]
    fn grow_post_lock_requires_the_delta_pre_snap_when_the_original_lock_exceeded_it() {
        let sr = 48_000u32;
        let quiet_frames = frames_at_ms(sr, 550);
        let speech_frames = frames_at_ms(sr, 900);
        let post_len = quiet_frames + speech_frames;
        let gap_end = frames_at_ms(sr, 2000);
        let total = gap_end + post_len + frames_at_ms(sr, 200);
        let shift = frames_at_ms(sr, 40) as i64;
        let far = frames_at_ms(sr, 100) as i64;

        let quiet_noise = noise(quiet_frames, 0x9001);
        let speech_noise = noise(speech_frames, 0x9002);
        let mut a = vec![0.0f32; total];
        for (j, slot) in a[gap_end..gap_end + quiet_frames].iter_mut().enumerate() {
            *slot = (quiet_noise[j] * 0.002) as f32;
        }
        for (j, slot) in a[gap_end + quiet_frames..gap_end + post_len]
            .iter_mut()
            .enumerate()
        {
            *slot = (speech_noise[j] * 0.5) as f32;
        }
        let mut b_mono = noise(total + shift.unsigned_abs() as usize + 8, 0x9003);
        for v in &mut b_mono {
            *v *= 0.002;
        }
        for f in gap_end + quiet_frames..gap_end + post_len {
            b_mono[(f as i64 + shift) as usize] = f64::from(a[f]);
        }

        let max_lag_frames = frames_at_ms(sr, 600);
        let b_mapped_start = gap_end - frames_at_ms(sr, 800);
        let gap_frames = frames_at_ms(sr, 800);

        // pre_lag far from the true content's shift and `original_post_lag` set apart from `pre_lag`
        // by `far` (past the 10 ms snap line), so the ORIGINAL 250 ms lock's own |Δpre| already
        // exceeds the snap and `require_delta_pre` turns on. A wide rung locks onto the real shift
        // and reads well above the admit line, but its own |Δpre| (measured against this same
        // `pre_lag`) is also `far` — past the snap — so the whole ladder must report `None`.
        let rejected = grow_post_lock(
            &a,
            1,
            gap_end,
            &b_mono,
            b_mapped_start,
            gap_frames,
            shift - far,
            max_lag_frames,
            sr,
            shift,
        );
        assert!(
            rejected.is_none(),
            "a qualifying r must still be rejected when its own |Δpre| stays past the snap line"
        );

        // Same shoulder, `pre_lag` now equal to the true content's shift: the found rung's |Δpre| is
        // ~0, well within the snap line, so the same rung that failed above must now stop the ladder
        // (regardless of what `require_delta_pre` reads for this call).
        let admitted = grow_post_lock(
            &a,
            1,
            gap_end,
            &b_mono,
            b_mapped_start,
            gap_frames,
            shift,
            max_lag_frames,
            sr,
            shift,
        )
        .expect("with |Δpre| within the snap line, the same rung must now be admitted");
        assert!(admitted.lock.r >= POST_GROW_ADMIT_R);
    }
}
