//! FAF's faf-uid on Android, and the gate the lobby keeps on it.
//!
//! The lobby's `auth` carries a machine proof that only FAF's own build of
//! faf-uid makes, and FAF's server takes a sign-in with any other proof for a
//! fraud and bans the account. The Android app ships that build as
//! `libfafuid.so` (`scripts/ensure-faf-uid-android.mjs`), since Android runs
//! an app's own programs only from its native library folder. The lobby is
//! contacted only when that file's SHA-256 is the one `android_uid_pin.json`
//! pins for FAF's release, the pin the build downloads it by. The pin is
//! empty until FAF publishes faf-uid for Android (FAForever/uid#18), so the
//! lobby stays held back until then, whatever the APK carries.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

/// FAF's release of faf-uid for Android: its version, for the build, and the
/// SHA-256 of each ABI's file.
const PIN: &str = include_str!("android_uid_pin.json");

/// faf-uid's name in the APK. Android extracts only `lib*.so` files into the
/// native library folder.
const FILE_NAME: &str = "libfafuid.so";

/// This library's ABI as Android and the pin name it. FAF builds faf-uid for
/// these two only.
const ABI: Option<&str> = if cfg!(target_arch = "aarch64") {
    Some("arm64-v8a")
} else if cfg!(target_arch = "x86_64") {
    Some("x86_64")
} else {
    None
};

#[derive(serde::Deserialize)]
struct Pin {
    sha256: BTreeMap<String, String>,
}

/// The SHA-256 `pin` holds for `abi`: empty while FAF has published none. A
/// test keeps the shipped pin readable.
fn pinned_digest(pin: &str, abi: &str) -> String {
    serde_json::from_str::<Pin>(pin)
        .ok()
        .and_then(|pin| pin.sha256.get(abi).cloned())
        .unwrap_or_default()
}

/// FAF's official faf-uid as this app ships it, or `None`, with the reason in
/// the log. MainActivity passes the native library folder as
/// `FAF_NATIVE_LIBRARY_DIR`. Checked once: the file is the installed APK's,
/// and an update, the one thing that changes it, ends this process.
pub(super) fn official() -> Option<&'static Path> {
    static OFFICIAL: OnceLock<Option<PathBuf>> = OnceLock::new();
    OFFICIAL
        .get_or_init(|| {
            let pinned = ABI.map(|abi| pinned_digest(PIN, abi)).unwrap_or_default();
            let folder = std::env::var_os("FAF_NATIVE_LIBRARY_DIR").map(PathBuf::from);
            match verified(&pinned, folder.as_deref()) {
                Ok(path) => {
                    tracing::info!(path = %path.display(), "faf-uid is FAF's official build");
                    Some(path)
                }
                Err(reason) => {
                    tracing::warn!(%reason, "the lobby is held back: no official faf-uid");
                    None
                }
            }
        })
        .as_deref()
}

/// The gate: `folder`'s faf-uid when its SHA-256 is `pinned`. No pin matches
/// nothing.
fn verified(pinned: &str, folder: Option<&Path>) -> Result<PathBuf, String> {
    if pinned.is_empty() {
        return Err("no official faf-uid for Android is pinned (FAForever/uid#18)".into());
    }
    let folder = folder.ok_or("the app passed no native library folder")?;
    let path = folder.join(FILE_NAME);
    let program = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let digest = format!("{:x}", Sha256::digest(&program));
    if digest != pinned {
        return Err(format!(
            "{} is not FAF's official build: SHA-256 {digest}, pinned {pinned}",
            path.display()
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A native library folder holding `program` as faf-uid, removed again
    /// when the test ends.
    struct Folder(PathBuf);

    impl Folder {
        fn with(test: &str, program: &[u8]) -> Self {
            let path =
                std::env::temp_dir().join(format!("faf-uid-gate-{test}-{}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join(FILE_NAME), program).unwrap();
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sha256(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    #[test]
    fn without_a_pin_the_lobby_stays_held_back_whatever_the_app_ships() {
        let folder = Folder::with("no-pin", b"faf-uid");
        let refused = verified("", Some(&folder.0)).unwrap_err();
        assert!(refused.contains("pinned"), "{refused}");
    }

    #[test]
    fn a_build_other_than_the_pinned_one_keeps_it_held_back() {
        let folder = Folder::with("wrong-digest", b"a faf-uid built elsewhere");
        let refused = verified(&sha256(b"FAF's faf-uid"), Some(&folder.0)).unwrap_err();
        assert!(refused.contains("not FAF's official build"), "{refused}");
    }

    #[test]
    fn the_pinned_build_opens_it_and_is_the_program_run() {
        let folder = Folder::with("right-digest", b"FAF's faf-uid");
        let path = verified(&sha256(b"FAF's faf-uid"), Some(&folder.0)).unwrap();
        assert_eq!(path, folder.0.join("libfafuid.so"));
    }

    #[test]
    fn a_pin_opens_nothing_without_the_file() {
        let pinned = sha256(b"FAF's faf-uid");
        assert!(verified(&pinned, None).is_err(), "no folder passed");
        let folder = Folder::with("missing-file", b"");
        std::fs::remove_file(folder.0.join(FILE_NAME)).unwrap();
        assert!(verified(&pinned, Some(&folder.0)).is_err(), "no file");
    }

    #[test]
    fn the_shipped_pin_is_well_formed() {
        let pin: serde_json::Value = serde_json::from_str(PIN).unwrap();
        let version = pin["version"].as_str().expect("a version string");
        let digests = pin["sha256"].as_object().expect("an sha256 object");
        let abis: Vec<&str> = digests.keys().map(String::as_str).collect();
        assert_eq!(abis, ["arm64-v8a", "x86_64"], "FAF's two Android builds");
        for (abi, digest) in digests {
            let digest = digest.as_str().expect("a digest string");
            let is_digest = digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
            assert!(digest.is_empty() || is_digest, "{abi}: {digest:?}");
            assert_eq!(pinned_digest(PIN, abi), digest, "{abi}");
            assert_eq!(
                version.is_empty(),
                digest.is_empty(),
                "{abi}: a digest comes with its release"
            );
        }
    }
}
