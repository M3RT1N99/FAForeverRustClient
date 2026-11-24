use std::{env, path::Path};

fn main() {
    let asset = match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => Some("faf-uid.exe"),
        Ok("macos") => Some("faf-uid-macos"),
        Ok("linux") => Some("faf-uid"),
        _ => None,
    };

    // `pnpm tauri` prepares the native helper before a release bundle. Plain
    // Cargo workflows (tests, clippy, IDE checks) neither need nor package it;
    // debug runs resolve the helper directly from the workspace instead.
    let java_adapter = Path::new("..")
        .join("natives")
        .join("java-ice-adapter")
        .join("faf-ice-adapter.jar");
    let java_runtime = Path::new("..")
        .join("natives")
        .join("jre")
        .join("bin")
        .join(if cfg!(windows) { "java.exe" } else { "java" });
    let release_bundle = env::var("PROFILE").as_deref() == Ok("release");
    if (!release_bundle
        || asset.is_none_or(|name| !Path::new("..").join("natives").join(name).is_file())
        || !java_adapter.is_file()
        || !java_runtime.is_file())
        && env::var_os("TAURI_CONFIG").is_none()
    {
        env::set_var("TAURI_CONFIG", r#"{"bundle":{"resources":[]}}"#);
    }

    // The commit the log and a problem report name (`diagnostics::COMMIT`).
    // Best effort: a build without git, or outside a checkout, says `unknown`.
    let commit = git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=FAF_BUILD_COMMIT={commit}");
    // Run again when this checkout's HEAD moves: a checkout rewrites HEAD, and
    // a commit, reset or checkout adds a line to its reflog. The paths are
    // git's own, because in a worktree `.git` is a file and they lie
    // elsewhere: the `../.git/HEAD` watched before did not exist there, and
    // the commit stayed that of the first build in the target folder.
    let branch = git(&["symbolic-ref", "-q", "HEAD"]);
    let watched = [Some("HEAD"), Some("logs/HEAD"), branch.as_deref()];
    for path in watched
        .into_iter()
        .flatten()
        .filter_map(|path| git(&["rev-parse", "--git-path", path]))
    {
        if Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    // A faf-uid test build: an APK that packs faf-uid builds made elsewhere
    // (`FAF_UID_ANDROID_TEST_BUILDS`, scripts/ensure-faf-uid-android.mjs).
    // Only such a build has `android::faf_uid_check`: the switch that packs a
    // test faf-uid is the one that brings the check, and no other build
    // carries it.
    println!("cargo:rustc-check-cfg=cfg(faf_uid_test_build)");
    println!("cargo:rerun-if-env-changed=FAF_UID_ANDROID_TEST_BUILDS");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android")
        && env::var_os("FAF_UID_ANDROID_TEST_BUILDS").is_some_and(|folder| !folder.is_empty())
    {
        println!("cargo:rustc-cfg=faf_uid_test_build");
    }

    tauri_build::build();
}

/// What `git args` prints, trimmed, when it succeeds and prints anything.
fn git(args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}
