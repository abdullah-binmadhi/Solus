use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    watch_git_head_refs();

    let git_sha = std::env::var("GITHUB_SHA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| git_output(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_string());

    let commit_ts = git_output(&["show", "-s", "--format=%ct", "HEAD"])
        .and_then(|s| s.parse::<i64>().ok())
        .map(|ts| ts.to_string())
        .unwrap_or_else(|| "0".to_string());

    println!("cargo:rustc-env=SOLUS_GIT_SHA={}", git_sha.trim());
    println!("cargo:rustc-env=SOLUS_COMMIT_TS={}", commit_ts);
}

/// Watch both `.git/HEAD` and its active branch ref file so local commits always rebake `SOLUS_GIT_SHA`.
fn watch_git_head_refs() {
    let git_dir = git_output(&["rev-parse", "--git-dir"])
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".git"));

    let head_path = git_dir.join("HEAD");
    if head_path.exists() {
        println!("cargo:rerun-if-changed={}", head_path.display());
        if let Ok(head_contents) = std::fs::read_to_string(&head_path) {
            if let Some(rel_ref) = head_contents.trim().strip_prefix("ref:") {
                let ref_path = git_dir.join(rel_ref.trim());
                if ref_path.exists() {
                    println!("cargo:rerun-if-changed={}", ref_path.display());
                }
            }
        }
    }

    let packed_refs = Path::new(&git_dir).join("packed-refs");
    if packed_refs.exists() {
        println!("cargo:rerun-if-changed={}", packed_refs.display());
    }
}

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
