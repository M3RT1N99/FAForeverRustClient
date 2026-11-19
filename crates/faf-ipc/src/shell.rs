//! What only the Tauri shell builds or reads, as it crosses to and from the
//! page: the revisioned event stream and its snapshot, and the shell's own
//! commands outside [`AppCommand`](faf_domain::AppCommand).
//!
//! Here rather than in the shell because this crate cannot depend on the
//! shell, and a type the bindings export cannot see is a type the frontend
//! has to write out by hand. What `faf-app`'s adapters build is in
//! [`faf_domain::native`].
//!
//! Specta refuses `u64` (a value past 2^53 would not survive as a JavaScript
//! number), so the revisions below are exported as a plain `number`: one per
//! state change, they never come near it.

use faf_domain::frontend::FrontendUpdate;
use faf_domain::protocol::log_analysis::LogIssue;
use faf_domain::state::Theme;
use faf_domain::AppState;
use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::Number;

/// One ordered shell-to-webview stream. Recovery snapshots deliberately use
/// the same channel as deltas so the webview cannot observe a post-recovery
/// event and then roll itself back to an older snapshot from another channel.
#[derive(Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FrontendMessage {
    Event {
        #[specta(type = Number)]
        revision: u64,
        event: Box<FrontendUpdate>,
    },
    Snapshot {
        #[specta(type = Number)]
        revision: u64,
        state: Box<AppState>,
    },
}

/// The page's hydration snapshot, and the last event revision it contains:
/// the runtime's own `faf_app::VersionedSnapshot`, as the `snapshot` command
/// answers with it.
#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VersionedSnapshot {
    #[specta(type = Number)]
    pub revision: u64,
    pub state: AppState,
}

/// The newest file of one log folder, for the diagnostics page.
#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogPreview {
    pub file_name: String,
    /// The newest 512 KiB of the file.
    pub content: String,
    /// Known problems recognised in this log (see
    /// [`faf_domain::protocol::log_analysis`]). Analysed in the shell rather
    /// than in the frontend so the whole file is scanned: `content` is
    /// truncated, and the trace that explains a crash is routinely older than
    /// what it keeps, so an issue can be reported that the excerpt does not
    /// contain.
    pub issues: Vec<LogIssue>,
}

/// Which build on which device a problem report is about: what the report's
/// form and the exported logs say, and nothing about the player.
#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportContext {
    /// The client's version.
    pub version: String,
    /// The commit it was built from, or `unknown`.
    pub commit: String,
    /// The operating system and its version, such as `Android 16 (API 36)`.
    pub system: String,
    /// The maker and model, such as `samsung SM-S908B`.
    pub device: String,
}

/// The FAF game app on Android (`io.github.m3rt1n99.fafre`), the game this
/// client starts there, as its launcher contract answers.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameAppStatus {
    pub installed: bool,
    pub version_name: String,
    pub version_code: i32,
    pub contract: GameAppContract,
    /// Its game data, when it answers this client.
    pub game_data: Option<GameAppData>,
    /// The last game it ran, when it answers this client and ran one.
    pub last_game: Option<GameAppLastGame>,
    /// Whether Android lets this client update it without asking: Android 12
    /// or later, and this client installed it last.
    pub silent_update: bool,
}

/// Whether the installed game app can be started from this client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GameAppContract {
    /// Not installed, or a version from before the launcher contract.
    None,
    /// It has the contract, but does not allow this client: its signing
    /// certificate is not on the game app's list.
    Refused,
    Open,
}

/// The game app's `game_data`: what the game needs, and how much of it is
/// there.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameAppData {
    /// The player's Supreme Commander: how many of its archives, fonts, sound
    /// banks and movies the game app holds. None of a kind is a setup to do.
    pub scfa_archives: i32,
    pub scfa_fonts: i32,
    pub scfa_sound_banks: i32,
    pub scfa_movies: i32,
    /// FAF's game versions it has ready, oldest first.
    pub game_versions: Vec<i32>,
    /// FAF's newest version at its last check, if it checked.
    pub newest_known: Option<i32>,
    /// What its setup is doing now, if anything.
    pub job_step: Option<String>,
    pub game_running: bool,
    pub engine_commit: String,
}

/// The game app's `last_game`.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameAppLastGame {
    /// When it started, in milliseconds since 1970.
    pub started_at: f64,
    pub ended: bool,
    pub exit_code: Option<i32>,
    pub crashed: bool,
    /// The report's last line, the game's own words on how it ended.
    pub end: Option<String>,
}

/// How to start a game in the game app (its `.action.PLAY`). Every field may
/// be left out: an offline game with the newest version is none of them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameAppLaunch {
    pub game_version: Option<i32>,
    pub featured_mod: Option<String>,
    /// The game's switches, each a word of its own.
    pub arguments: Option<Vec<String>>,
    /// FAF's map folder name, which the game app fetches when the phone lacks it.
    pub map: Option<String>,
    /// The uids of the mods the game needs.
    pub mods: Option<Vec<String>>,
    /// Each of `mods`' file on FAF's content server, in their order: the game
    /// app has no way to look a mod up by its uid.
    pub mod_files: Option<Vec<String>>,
    #[serde(default)]
    pub rejoin_offer: bool,
    pub rejoin_peers: Option<String>,
    pub rejoin_source: Option<i32>,
}

/// How a game the game app was asked for ended, or its setup screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum GameAppEnd {
    /// It ran, and it is over; or the player is back from the setup.
    Ended,
    /// It did not run: the game app's code (`NO_SCFA`, `CALLER`, ...) and its
    /// reason in words.
    Failed { failure: String, message: String },
}

/// A release of the game app newer than the one on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameAppUpdate {
    /// Its version, `0.17.0`.
    pub version: String,
    /// Whether the phone is on a metered network now (mobile data, a hotspot,
    /// a Wi-Fi set so): the download is then the player's to start.
    pub metered: bool,
}

/// How an update of the game app ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum GameAppInstall {
    /// Android installed it.
    Installed,
    /// It was not installed: why, as this client's code (`RUNNING`, `PACKAGE`,
    /// `SIGNER`, `NOT_NEWER`, `ABORTED`, `CONFLICT`, `STORAGE`, `INSTALL`),
    /// and Android's own words where it gave some.
    Failed { failure: String, message: String },
}

/// Which rendering engine the client actually ended up in.
///
/// Facts only. Which release is new enough is deliberately a frontend
/// decision, because the features at stake are the ones its stylesheets use.
#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WebviewEngine {
    /// `windows`, `macos`, `linux`, or `other`.
    pub platform: &'static str,
    /// WebKitGTK `major.minor.micro`; only Linux has a version worth reporting.
    pub webkit_version: Option<String>,
}

/// One card of the desktop notification popup: bounded presentation data for
/// an existing notification id. No actions or URLs.
#[derive(Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPopupCard {
    pub id: String,
    pub title: String,
    pub body: String,
    pub action_label: Option<String>,
    pub tone: String,
    #[serde(default)]
    #[specta(optional)]
    pub image_url: Option<String>,
}

/// What the popup window shows: at most three cards, newest first.
#[derive(Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPopupBatch {
    #[specta(type = Number)]
    pub revision: u64,
    pub cards: Vec<NotificationPopupCard>,
    pub duration_seconds: u16,
    /// Only local test cards may opt out of expiry; live cards keep their timer.
    pub held_preview_ids: Vec<String>,
    pub theme: Theme,
    pub dismiss_label: String,
}

/// A display the popup can be placed on, by the name the settings keep.
#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationDisplay {
    pub name: String,
    pub primary: bool,
}

/// What the page asks a file dialog to look like. Nothing in it decides what
/// may be picked: that is the player, in the dialog the shell opens, so a path
/// the page sends back later can be checked against what was picked.
#[derive(Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PickRequest {
    #[serde(default)]
    #[specta(optional)]
    pub title: Option<String>,
    #[serde(default)]
    #[specta(optional)]
    pub filters: Vec<PickFilter>,
    /// Pick a folder rather than a file.
    #[serde(default)]
    #[specta(optional)]
    pub directory: bool,
    /// Where the dialog starts: a folder, or a file to preselect.
    #[serde(default)]
    #[specta(optional)]
    pub default_path: Option<String>,
}

/// One filter of the dialog, as the page describes it.
#[derive(Deserialize, Type)]
pub struct PickFilter {
    pub name: String,
    pub extensions: Vec<String>,
}
