//! The FAF game app's releases, which keep it up to date on Android.
//!
//! On Android the game is an app of its own, `io.github.m3rt1n99.fafre`,
//! published as GitHub pre-releases of [`GAME_APP_REPO`]: each tagged
//! `android-v<version>`, with one asset, `faf-re-android-<version>-arm64-v8a.apk`.
//! This finds the newest and downloads its APK; the Android shell installs it
//! (`GameAppPlugin.installUpdate`).
//!
//! The trust chain is the client's own update's (see [`super::client_update`]):
//! TLS to GitHub, a download URL under the repository's release prefix, and
//! GitHub's published SHA-256, which is required here: an asset without one is
//! not offered. On the device the APK must then name the game app, carry its
//! signing certificate and a higher version code before it is installed, and
//! Android itself refuses an update that another key signed.

use std::path::{Path, PathBuf};

use faf_domain::state::{compare_versions, is_release_version};
use serde_json::Value;

use super::client_update::{file_sha256, sha256_digest};

/// Where the game app is published.
pub const GAME_APP_REPO: &str = "M3RT1N99/faf-re";
/// Its releases' tags are this and the version; the repository's others (the
/// engine's own) are not the game app's.
const TAG_PREFIX: &str = "android-v";
const MAX_RELEASE_LIST_BYTES: u64 = 4 * 1024 * 1024;
/// The APK is a few dozen megabytes. This bounds a broken or hostile answer.
const MAX_APK_BYTES: u64 = 512 * 1024 * 1024;
const USER_AGENT: &str = concat!("faforever-rust-client/", env!("CARGO_PKG_VERSION"));

/// A release of the game app and its APK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameAppRelease {
    /// The tag's version: `0.16.0` for `android-v0.16.0`.
    pub version: String,
    pub download_url: String,
    /// The SHA-256 GitHub published for the APK.
    pub digest: [u8; 32],
}

/// The APK's name in a release, which is also how it is found there.
fn asset_name(version: &str) -> String {
    format!("faf-re-android-{version}-arm64-v8a.apk")
}

/// The newest release, by its version, whose tag follows the rule and whose
/// APK has a published SHA-256 under `download_prefix`. Drafts are left out;
/// the game app's releases are all pre-releases, so those count.
pub fn newest_release(releases: &[Value], download_prefix: &str) -> Option<GameAppRelease> {
    releases
        .iter()
        .filter(|release| !release["draft"].as_bool().unwrap_or(false))
        .filter_map(|release| {
            let version = release["tag_name"].as_str()?.strip_prefix(TAG_PREFIX)?;
            // `is_release_version` also takes a `v` in front, which would put
            // a second one in the file's name.
            if version.starts_with('v') || !is_release_version(version) {
                return None;
            }
            let name = asset_name(version);
            let asset = release["assets"]
                .as_array()?
                .iter()
                .find(|asset| asset["name"].as_str() == Some(name.as_str()))?;
            let download_url = asset["browser_download_url"].as_str()?;
            if !trusted(download_url, download_prefix) {
                return None;
            }
            Some(GameAppRelease {
                version: version.to_owned(),
                download_url: download_url.to_owned(),
                digest: sha256_digest(asset["digest"].as_str()?)?,
            })
        })
        .max_by(|left, right| compare_versions(&left.version, &right.version))
}

/// Whether a download URL is one of the repository's release assets.
fn trusted(url: &str, download_prefix: &str) -> bool {
    // `..` would be normalised away by the URL parser, escaping the prefix.
    url.starts_with(download_prefix) && !url.contains("..")
}

/// GitHub, as the game app's releases are read and downloaded from it.
pub struct GameAppReleases {
    api_base: String,
    repo: String,
    download_prefix: String,
    http: reqwest::Client,
}

impl GameAppReleases {
    /// The game app's own releases. The transport refuses a redirect that
    /// leaves HTTPS; the prefix pins the first URL to the repository.
    pub fn faf() -> Self {
        Self::new(
            "https://api.github.com",
            GAME_APP_REPO,
            format!("https://github.com/{GAME_APP_REPO}/releases/download/"),
            super::http::https_only_download_client(),
        )
    }

    fn new(
        api_base: impl Into<String>,
        repo: impl Into<String>,
        download_prefix: impl Into<String>,
        http: reqwest::Client,
    ) -> Self {
        Self {
            api_base: api_base.into(),
            repo: repo.into(),
            download_prefix: download_prefix.into(),
            http,
        }
    }

    /// The newest release with its APK, or `None` when there is none.
    pub async fn newest(&self) -> Result<Option<GameAppRelease>, String> {
        let url = format!("{}/repos/{}/releases?per_page=30", self.api_base, self.repo);
        let response = self
            .http
            .get(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                format!(
                    "could not reach the game app's releases: {}",
                    super::http::describe_transport_error(&error)
                )
            })?;
        let status = response.status();
        let body = super::vault_install::bounded_body(
            response,
            "the game app's releases",
            MAX_RELEASE_LIST_BYTES,
        )
        .await?;
        if !status.is_success() {
            return Err(format!("the game app's releases returned {status}"));
        }
        let releases: Vec<Value> = serde_json::from_slice(&body)
            .map_err(|error| format!("the game app's releases were not a list: {error}"))?;
        Ok(newest_release(&releases, &self.download_prefix))
    }

    /// The APK of `release` in `directory`, downloaded unless the one there
    /// already has the published SHA-256. Nothing else is kept there: the
    /// folder is this download's alone, and an older APK is of no more use.
    /// A file that does not match the checksum is never kept.
    pub async fn download(
        &self,
        release: &GameAppRelease,
        directory: &Path,
    ) -> Result<PathBuf, String> {
        if !trusted(&release.download_url, &self.download_prefix) {
            return Err(format!(
                "refusing to download {}: it is not a release asset of {}",
                release.download_url, self.repo
            ));
        }
        // The file's name comes from the version, never from the API's asset
        // name, and only a version of this shape can be in it.
        if release.version.starts_with('v') || !is_release_version(&release.version) {
            return Err(format!("{} is not a release version", release.version));
        }
        tokio::fs::create_dir_all(directory)
            .await
            .map_err(|error| format!("could not create the download folder: {error}"))?;
        let target = directory.join(format!("faf-re-android-{}.apk", release.version));
        if file_sha256(&target).await == Some(release.digest) {
            return Ok(target);
        }
        clear_except(directory, &target).await?;

        let partial = target.with_extension("partial");
        let written = self.fetch_to(&release.download_url, &partial).await;
        let outcome = match written {
            Ok(digest) if digest == release.digest => tokio::fs::rename(&partial, &target)
                .await
                .map_err(|error| format!("could not keep the game app's download: {error}")),
            Ok(_) => Err(
                "the game app's download does not match the checksum GitHub published for it"
                    .to_owned(),
            ),
            Err(error) => Err(error),
        };
        if outcome.is_err() {
            let _ = tokio::fs::remove_file(&partial).await;
        }
        outcome.map(|()| target)
    }

    /// Stream `url` into `path`, and return the SHA-256 of what arrived.
    async fn fetch_to(&self, url: &str, path: &Path) -> Result<[u8; 32], String> {
        use futures_util::StreamExt as _;
        use sha2::Digest as _;
        use tokio::io::AsyncWriteExt as _;

        let response = self
            .http
            .get(url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|error| {
                format!(
                    "could not reach the game app's download: {}",
                    super::http::describe_transport_error(&error)
                )
            })?;
        if !response.status().is_success() {
            return Err(format!(
                "the game app's download returned {}",
                response.status()
            ));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_APK_BYTES)
        {
            return Err("the game app's download is larger than an APK can be".to_owned());
        }
        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(|error| format!("could not write the game app's download: {error}"))?;
        let mut hasher = sha2::Sha256::new();
        let mut received = 0_u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                format!(
                    "the game app's download failed: {}",
                    super::http::describe_transport_error(&error)
                )
            })?;
            received = received.saturating_add(chunk.len() as u64);
            if received > MAX_APK_BYTES {
                return Err("the game app's download is larger than an APK can be".to_owned());
            }
            file.write_all(&chunk)
                .await
                .map_err(|error| format!("could not write the game app's download: {error}"))?;
            hasher.update(&chunk);
        }
        file.flush()
            .await
            .map_err(|error| format!("could not write the game app's download: {error}"))?;
        Ok(hasher.finalize().into())
    }
}

/// Remove everything in `directory` but `keep`.
async fn clear_except(directory: &Path, keep: &Path) -> Result<(), String> {
    let mut entries = tokio::fs::read_dir(directory)
        .await
        .map_err(|error| format!("could not read the download folder: {error}"))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| format!("could not read the download folder: {error}"))?
    {
        let path = entry.path();
        if path != keep {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|error| format!("could not clear the download folder: {error}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sha2::Digest as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const PREFIX: &str = "https://github.com/M3RT1N99/faf-re/releases/download/";

    fn hex(digest: [u8; 32]) -> String {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn release(tag: &str, draft: bool, asset: Option<(&str, &str)>) -> Value {
        let assets = asset.map_or_else(Vec::new, |(name, digest)| {
            vec![json!({
                "name": name,
                "browser_download_url": format!("{PREFIX}{tag}/{name}"),
                "digest": digest,
            })]
        });
        json!({ "tag_name": tag, "draft": draft, "prerelease": true, "assets": assets })
    }

    fn digest_of(bytes: &[u8]) -> [u8; 32] {
        sha2::Sha256::digest(bytes).into()
    }

    #[test]
    fn the_newest_release_by_the_tag_rule_with_its_apk_and_checksum() {
        let sum = format!("sha256:{}", hex(digest_of(b"apk")));
        let releases = [
            release(
                "android-v0.15.1",
                false,
                Some(("faf-re-android-0.15.1-arm64-v8a.apk", &sum)),
            ),
            // Newer, but not the game app's: the engine's own tags.
            release(
                "v0.20.0",
                false,
                Some(("faf-re-android-0.20.0-arm64-v8a.apk", &sum)),
            ),
            // Newer, but a draft.
            release(
                "android-v0.18.0",
                true,
                Some(("faf-re-android-0.18.0-arm64-v8a.apk", &sum)),
            ),
            // Newer, but its APK has no published checksum.
            release(
                "android-v0.17.0",
                false,
                Some(("faf-re-android-0.17.0-arm64-v8a.apk", "")),
            ),
            // Newer, but no APK by the rule's name.
            release(
                "android-v0.16.5",
                false,
                Some(("faf-re-android-0.16.5-x86_64.apk", &sum)),
            ),
            release(
                "android-v0.16.0",
                false,
                Some(("faf-re-android-0.16.0-arm64-v8a.apk", &sum)),
            ),
            release(
                "android-vv0.19.0",
                false,
                Some(("faf-re-android-v0.19.0-arm64-v8a.apk", &sum)),
            ),
        ];
        let newest = newest_release(&releases, PREFIX).expect("a release");
        assert_eq!(newest.version, "0.16.0");
        assert_eq!(
            newest.download_url,
            format!("{PREFIX}android-v0.16.0/faf-re-android-0.16.0-arm64-v8a.apk")
        );
        assert_eq!(newest.digest, digest_of(b"apk"));
    }

    #[test]
    fn an_asset_from_anywhere_else_is_not_offered() {
        let sum = format!("sha256:{}", hex(digest_of(b"apk")));
        let mut moved = release(
            "android-v0.16.0",
            false,
            Some(("faf-re-android-0.16.0-arm64-v8a.apk", &sum)),
        );
        moved["assets"][0]["browser_download_url"] =
            json!("https://example.invalid/faf-re-android-0.16.0-arm64-v8a.apk");
        let mut climbing = release(
            "android-v0.16.1",
            false,
            Some(("faf-re-android-0.16.1-arm64-v8a.apk", &sum)),
        );
        climbing["assets"][0]["browser_download_url"] =
            json!(format!("{PREFIX}../../other/releases/download/x.apk"));
        assert_eq!(newest_release(&[moved, climbing], PREFIX), None);
        assert_eq!(newest_release(&[], PREFIX), None);
    }

    /// A local GitHub: the release list `list` makes for its address, and the
    /// APK, each download counted.
    async fn serve(list: impl FnOnce(&str) -> Value, apk: Vec<u8>) -> (String, Arc<AtomicUsize>) {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let downloads = Arc::new(AtomicUsize::new(0));
        let counted = downloads.clone();
        let list = list(&base).to_string();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (list, apk, counted) = (list.clone(), apk.clone(), counted.clone());
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut buffer = [0_u8; 1024];
                    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
                        match socket.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => head.extend_from_slice(&buffer[..read]),
                        }
                    }
                    let body = if head.starts_with(b"GET /repos/") {
                        list.into_bytes()
                    } else {
                        counted.fetch_add(1, Ordering::SeqCst);
                        apk
                    };
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                });
            }
        });
        (base, downloads)
    }

    fn local(base: &str) -> GameAppReleases {
        GameAppReleases::new(
            base,
            GAME_APP_REPO,
            format!("{base}/download/"),
            reqwest::Client::new(),
        )
    }

    fn local_release(base: &str, version: &str, digest: [u8; 32]) -> GameAppRelease {
        GameAppRelease {
            version: version.into(),
            download_url: format!("{base}/download/android-v{version}/{}", asset_name(version)),
            digest,
        }
    }

    #[tokio::test]
    async fn the_newest_release_is_read_from_the_list() {
        let sum = format!("sha256:{}", hex(digest_of(b"apk")));
        let (base, _) = serve(
            |base| {
                let name = asset_name("0.16.0");
                let mut listed = release("android-v0.16.0", false, Some((&name, &sum)));
                listed["assets"][0]["browser_download_url"] =
                    json!(format!("{base}/download/android-v0.16.0/{name}"));
                json!([listed])
            },
            Vec::new(),
        )
        .await;
        let newest = local(&base).newest().await.expect("a list");
        assert_eq!(
            newest.map(|release| release.version).as_deref(),
            Some("0.16.0")
        );
    }

    #[tokio::test]
    async fn a_download_is_kept_only_with_its_published_checksum() {
        let apk = b"PK\x03\x04 the game app".to_vec();
        let (base, downloads) = serve(|_| Value::Null, apk.clone()).await;
        let folder = tempfile::tempdir().expect("a folder");
        std::fs::write(folder.path().join("faf-re-android-0.15.1.apk"), b"older")
            .expect("an older APK");
        let updates = local(&base);

        let wrong = local_release(&base, "0.16.0", digest_of(b"something else"));
        let refused = updates.download(&wrong, folder.path()).await;
        assert!(refused.unwrap_err().contains("does not match"));
        assert_eq!(
            std::fs::read_dir(folder.path())
                .expect("the folder")
                .count(),
            0,
            "nothing kept"
        );

        let right = local_release(&base, "0.16.0", digest_of(&apk));
        let path = updates
            .download(&right, folder.path())
            .await
            .expect("the APK");
        assert_eq!(path, folder.path().join("faf-re-android-0.16.0.apk"));
        assert_eq!(std::fs::read(&path).expect("its bytes"), apk);

        // Asked again, the APK already there is taken without a download.
        assert_eq!(
            updates
                .download(&right, folder.path())
                .await
                .expect("the APK"),
            path
        );
        assert_eq!(downloads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_download_from_anywhere_else_or_by_a_strange_version_is_refused() {
        let (base, downloads) = serve(|_| Value::Null, b"apk".to_vec()).await;
        let folder = tempfile::tempdir().expect("a folder");
        let updates = local(&base);
        let mut elsewhere = local_release(&base, "0.16.0", digest_of(b"apk"));
        elsewhere.download_url =
            "https://example.invalid/faf-re-android-0.16.0-arm64-v8a.apk".into();
        assert!(updates.download(&elsewhere, folder.path()).await.is_err());
        for version in ["0.16.0/x", "v0.16.0", ""] {
            let strange = local_release(&base, version, digest_of(b"apk"));
            assert!(
                updates.download(&strange, folder.path()).await.is_err(),
                "{version}"
            );
        }
        assert_eq!(downloads.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::read_dir(folder.path())
                .expect("the folder")
                .count(),
            0
        );
    }
}
