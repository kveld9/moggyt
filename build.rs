use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=MOGGYT_VERSION");
    if Path::new(".git/refs/tags").exists() {
        println!("cargo:rerun-if-changed=.git/refs/tags");
    }
    println!("cargo:rustc-env=MOGGYT_VERSION={}", raw_version());
    slint_build::compile("ui/shell.slint").expect("failed to compile Slint UI definition");
}

/// Resolves the version string the built binary reports.
///
/// Release builds inject `MOGGYT_VERSION` from the pushed tag, because CI checkouts are
/// shallow and source archives carry no git history. Local builds fall back to the most
/// recent `v*` tag, and trees without tags keep the crate version.
fn raw_version() -> String {
    std::env::var("MOGGYT_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(git_describe_version)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

fn git_describe_version() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let tag = String::from_utf8(output.stdout).ok()?;
    let tag = tag.trim().to_owned();
    (!tag.is_empty()).then_some(tag)
}
