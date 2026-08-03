use std::process::Command;

/// Bakes a build identity into the component so a redeploy is visible in the UI
/// even when nobody remembered to pass `--variable version=...`.
///
/// `spin.toml` sets `SPIN_BUILD_ID` to a fresh timestamp on every build, and the
/// `rerun-if-env-changed` line below turns that into an actual recompile.
fn main() {
    println!("cargo:rerun-if-env-changed=SPIN_BUILD_ID");

    let build_id = std::env::var("SPIN_BUILD_ID").unwrap_or_else(|_| "dev".to_string());
    println!("cargo:rustc-env=BUILD_ID={build_id}");

    let git_sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=GIT_SHA={git_sha}");
}
