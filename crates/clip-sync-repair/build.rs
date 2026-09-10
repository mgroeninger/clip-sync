//! Build stamp: capture the git identity of the tree this binary was compiled from.
//!
//! **Why this exists.** A `--gap-fingerprints` corpus records what *media* it came from
//! (`SourceMeta::a_source` / `b_source`) and, since 2026-08-22, one axis of *how* the probe was
//! configured (`source.probe_recipe.border_capture`). Nothing in it recorded which *binary* wrote it,
//! so two arms of the same comparison taken across a code change were indistinguishable except by
//! inferring it from the numbers — which is exactly what cost real time on the 31/g8 capture question.
//!
//! **What it does not do.** It identifies the commit, not the constants. A default that moved without
//! a commit in between is still invisible here; stamping the build-time constants themselves is a
//! separate, larger question and deliberately not attempted.
//!
//! **Staleness.** The stamp is captured when this crate is compiled, so `dirty` means "the working
//! tree had uncommitted changes at that moment", not "at run time". The `rerun-if-changed` lines below
//! cover the cases that matter: committing or checking out moves `HEAD`, staging moves the index, and
//! any source edit re-runs the script because `src/` is watched. An unstaged edit to a file outside
//! `src/` can leave `dirty` reading stale until the next rebuild of this crate. That is a documented
//! limit, not a bug to work around with a run-time `git` call: the binary is routinely run from a
//! directory that is not the repo, and often on media hosts with no git at all.
//!
//! **Absent git is not an error.** A build from an exported tarball, or one where `git` is not on
//! PATH, emits an empty commit and a false dirty flag; the emitting side turns that into an absent
//! `commit` field rather than a fabricated one. Never fail the build over provenance.

use std::path::Path;
use std::process::Command;

fn main() {
    // Emitting any `rerun-if-changed` replaces cargo's default "rerun when any package file changed",
    // so `src` and the manifest have to be named explicitly alongside the git paths.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");

    // Workspace root is two levels up from `crates/clip-sync-repair`.
    let git_dir = Path::new("../../.git");
    if git_dir.exists() {
        println!("cargo:rerun-if-changed=../../.git/HEAD");
        println!("cargo:rerun-if-changed=../../.git/index");
    }

    let commit = git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_default();
    // `--porcelain` prints one line per changed path and nothing at all on a clean tree, so emptiness
    // is the test. A failed call reads as clean rather than dirty: with no commit alongside it the
    // flag says nothing either way, and a spurious `dirty` on every no-git build would be worse.
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.trim().is_empty());

    println!("cargo:rustc-env=CLIP_SYNC_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=CLIP_SYNC_BUILD_DIRTY={}", u8::from(dirty));
}

/// `Some(stdout)` only on a clean exit; `None` when git is missing, the tree is not a repository, or
/// the command failed for any other reason.
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_string())
}
