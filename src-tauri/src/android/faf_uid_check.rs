//! A faf-uid test build's check of the faf-uid it packs (the
//! `faf_uid_test_build` cfg in build.rs). At the client's start faf-uid runs
//! once, from where and as the lobby's sign-in would run it but with a session
//! of its own, and the client's log says how that went. The log export adds
//! the outcome to its report, and faf-uid's output, as it is, in a file of its
//! own, for decrypting with the key the test build was made with.
//!
//! The output leaves the device only in an export the player shares. The check
//! sends nothing anywhere, and the lobby's gate (faf-app's
//! `lobby_ws/android_uid.rs`) knows nothing of it: a test build's faf-uid is
//! not FAF's pinned release, so the lobby stays held back.

use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The export's file for faf-uid's output alone, as FAForever/uid's
/// `.github/android_check.py decrypt` reads it.
pub(crate) const OUTPUT_FILE: &str = "faf-uid-output.txt";

/// The session the check passes, where the sign-in passes the one the lobby
/// server gave the connection.
const SESSION: &str = "4711";

/// The check's outcome, once faf-uid has run.
static CHECK: OnceLock<Result<Run, String>> = OnceLock::new();

struct Run {
    status: ExitStatus,
    took: Duration,
    output: Vec<u8>,
    complaint: String,
}

impl Run {
    /// Ok when faf-uid exits cleanly with output: whether that output is the
    /// proof is for the decrypting to say.
    fn verdict(&self) -> &'static str {
        if self.status.success() && !self.output.is_empty() {
            "ok"
        } else {
            "failed"
        }
    }
}

/// Run the check on a thread of its own: the client's start does not wait for
/// faf-uid.
pub(crate) fn start() {
    std::thread::spawn(|| {
        let check = run();
        match &check {
            Ok(run) => {
                let output = String::from_utf8_lossy(&run.output);
                tracing::info!(
                    status = %run.status,
                    took_ms = run.took.as_millis(),
                    output_bytes = run.output.len(),
                    output = %output.trim_end(),
                    stderr = %run.complaint,
                    "faf-uid check: {}",
                    run.verdict()
                );
            }
            Err(reason) => tracing::warn!(%reason, "faf-uid check: failed"),
        }
        let _ = CHECK.set(check);
    });
}

/// faf-uid as the app packs it, `libfafuid.so` in the native library folder
/// MainActivity passes, in the environment MainActivity left with its
/// `FAF_UID_ANDROID_ID`.
fn run() -> Result<Run, String> {
    let folder = std::env::var_os("FAF_NATIVE_LIBRARY_DIR")
        .ok_or("the app passed no native library folder")?;
    let program = PathBuf::from(folder).join("libfafuid.so");
    let started = Instant::now();
    let output = Command::new(&program)
        .arg(SESSION)
        .output()
        .map_err(|error| format!("could not start {}: {error}", program.display()))?;
    Ok(Run {
        status: output.status,
        took: started.elapsed(),
        output: output.stdout,
        complaint: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

/// The check's line in the export's report.
pub(crate) fn report_line() -> String {
    match CHECK.get() {
        None => "faf-uid check: still running\n".to_string(),
        Some(Err(reason)) => format!("faf-uid check: failed: {reason}\n"),
        Some(Ok(run)) => format!(
            "faf-uid check: {} ({}, {} ms, {} bytes of output in {OUTPUT_FILE})\n",
            run.verdict(),
            run.status,
            run.took.as_millis(),
            run.output.len()
        ),
    }
}

/// faf-uid's output as it is, empty until it has run.
pub(crate) fn output() -> &'static [u8] {
    match CHECK.get() {
        Some(Ok(run)) => &run.output,
        _ => &[],
    }
}
