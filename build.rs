//! Debug builds append the commit `git` prints. Nothing here invents a hash.
//! A release build stays the crate version alone.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let package = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let profile = std::env::var("PROFILE").unwrap_or_default();
    watch_git(Path::new(&manifest));
    let shown = if profile == "release" {
        package
    } else {
        match git_commit(Path::new(&manifest)) {
            Some(sha) => format!("{package} ({sha})"),
            None => package,
        }
    };
    println!("cargo:rustc-env=LOG_BILL_VERSION={shown}");
}

/// Object id from `git rev-parse --verify HEAD`, or nothing if git does not print one.
fn git_commit(manifest: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(manifest)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let sha = text.trim();
    if is_git_object(sha) {
        Some(sha.to_string())
    } else {
        None
    }
}

/// Git prints 40 hex digits (SHA-1) or 64 (SHA-256). Anything else is not a commit.
fn is_git_object(sha: &str) -> bool {
    (sha.len() == 40 || sha.len() == 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Rebuild when HEAD moves, including a linked worktree git dir.
fn watch_git(manifest: &Path) {
    println!("cargo:rerun-if-changed=.git");
    let Some(head) = git_path(manifest, "HEAD") else {
        return;
    };
    println!("cargo:rerun-if-changed={}", head.display());
    let Ok(text) = std::fs::read_to_string(&head) else {
        return;
    };
    let Some(reference) = text.trim().strip_prefix("ref: ") else {
        return;
    };
    if reference.is_empty() || reference.contains('\n') {
        return;
    }
    if let Some(path) = git_path(manifest, reference) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn git_path(manifest: &Path, name: &str) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-path", name])
        .current_dir(manifest)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let rel = text.trim();
    if rel.is_empty() || rel.contains('\n') {
        return None;
    }
    let path = Path::new(rel);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        manifest.join(path)
    })
}
