use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
}

fn main() {
    let git_hash =
        run("git", &["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=SARENA_CLI_GIT_HASH={git_hash}");

    let build_date =
        run("date", &["-u", "+%Y-%m-%dT%H:%M:%S+00:00"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=SARENA_CLI_BUILD_DATE={build_date}");

    // Re-run when HEAD moves (new commit, checkout, ...) so the embedded
    // hash doesn't go stale across incremental builds.
    println!("cargo:rerun-if-changed=../.git/HEAD");
}
