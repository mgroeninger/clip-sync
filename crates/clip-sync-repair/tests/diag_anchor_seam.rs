//! Anchor seam candidate / bracket CSV diagnostics (plan P3.6).
//!
//! Tier: **diagnostic** (`diagnostic-tests` feature). Per-gap anchor dumps for human review.
//!
//! PR: **no** — diagnostic tier with `--nocapture`.
//!
//! Run: `cargo test -p clip-sync-repair --features diagnostic-tests --test diag_anchor_seam -- --nocapture`

use clip_sync_repair_fixtures::anchor_seam_diagnostic::print_anchor_seam_diagnostic;
use clip_sync_repair_fixtures::energy_signature_fixtures::{
    build_c3_speech_boundary_asymmetric_post, build_speech_peaks_offset_from_throat,
};

#[test]
fn anchor_seam_speech_peaks_csv() {
    let fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
    print_anchor_seam_diagnostic(&fixture, "speech_peaks");
}

#[test]
fn anchor_seam_c3_csv() {
    let fixture = build_c3_speech_boundary_asymmetric_post(48_000, 1, 0.05);
    print_anchor_seam_diagnostic(&fixture, "c3_asymmetric_post");
}

#[test]
fn anchor_seam_flat_c1_csv() {
    let mut fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
    fixture.a_samples.fill(0.0);
    fixture.b_samples.fill(0.0);
    print_anchor_seam_diagnostic(&fixture, "flat_c1");
}
