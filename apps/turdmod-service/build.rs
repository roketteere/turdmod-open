// Stamps the build with the git short-SHA (+ "-dirty" if the tree has uncommitted
// changes) so /health and /status can report it. Lets us verify two hosts run the
// SAME engine build without a manual version bump. @inv: repo .git is two levels up
// (apps/turdmod-service → repo root).
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let sha = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain"]).map(|s| !s.is_empty()).unwrap_or(false);
    let stamp = if dirty { format!("{sha}-dirty") } else { sha };
    println!("cargo:rustc-env=GIT_SHA={stamp}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
}
