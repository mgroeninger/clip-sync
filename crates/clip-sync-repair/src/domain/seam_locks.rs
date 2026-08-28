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
        let a_pre =
            interleaved_to_mono(&a_samples[(gap_start_frame - w) * ch..gap_start_frame * ch], ch);
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
    fn passes(&self, smin: f64) -> bool {
        smin >= self.min_fill_correlation && smin >= self.fill_absolute_floor
    }
}

/// Lazily-supplied wider A border for the coarse envelope search only.
///
/// On the patch path A is fully decoded, so the wider window is a plain slice and this is `None`.
/// The fingerprint's B-side capture cannot slice, so it passes a closure — and it is called only
/// once a shoulder is confirmed untrustworthy at nominal, never up front.
pub type WideA<'a> = dyn Fn(bool) -> Option<Vec<f64>> + 'a;

/// Measure one shoulder, nominal first.
///
/// `triage_r` is the "do I need help?" line, **not** a gate: a shoulder at or above it keeps its
/// nominal lag and the envelope is never consulted, so it cannot be moved. Below it, the coarse
/// envelope proposes a recenter and the fine search re-runs there — and that retry is kept only if
/// it clears `triage_r` itself. Otherwise the honest nominal reading stands.
///
/// Callers pass different values on purpose: the fingerprint passes
/// [`crate::domain::lag_envelope::SEAM_LOCK_RECOVERABLE_R`] (0.5) to preserve `lag_decision`'s
/// behavior; the patch path passes `min_fill_correlation`, so that a lock the gate would accept at
/// nominal is never a rescue candidate and can never be relocated.
#[allow(clippy::too_many_arguments)]
fn lock_shoulder(
    a_seam: &[f64],
    b_mono: &[f64],
    nominal_center: usize,
    max_lag_frames: usize,
    pre_shoulder: bool,
    envelope: Option<EnvelopeCfg>,
    wide_a: Option<&WideA<'_>>,
    sample_rate: u32,
    triage_r: f64,
) -> Option<SeamLock> {
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

    let Some(env_cfg) = envelope else {
        return Some(nominal);
    };
    if r >= triage_r {
        // Trustworthy at nominal — the envelope is never consulted, so this lock cannot be moved.
        // Same predicate as the fingerprint's `is_recoverable` (`peak_r >= SEAM_LOCK_RECOVERABLE_R`).
        return Some(nominal);
    }

    let wide = wide_a.and_then(|f| f(pre_shoulder));
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
#[allow(clippy::too_many_arguments)]
pub fn nominal_first_seam_locks(
    templates: &SeamLockTemplates,
    b_mono: &[f64],
    max_lag_frames: usize,
    envelope: Option<EnvelopeCfg>,
    wide_a: Option<&WideA<'_>>,
    sample_rate: u32,
    floors: SeamGateFloors,
    triage_r: f64,
) -> Option<SeamLocks> {
    let pre = lock_shoulder(
        templates.a_pre(),
        b_mono,
        templates.b_mapped_start(),
        max_lag_frames,
        true,
        envelope,
        wide_a,
        sample_rate,
        triage_r,
    )?;
    let post = lock_shoulder(
        templates.a_post(),
        b_mono,
        templates.b_post_nominal(),
        max_lag_frames,
        false,
        envelope,
        wide_a,
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
}
