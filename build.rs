use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=.git/HEAD");

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
