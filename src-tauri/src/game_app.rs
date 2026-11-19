//! The FAF game app's answers, as `GameAppPlugin` (Kotlin) hands them over,
//! read into the page's types.
//!
//! The Kotlin passes the status provider's rows on as they are, column names
//! and all, so the reading is here, where a test can pin it. Built on every
//! platform for that; only Android calls it.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::cmp::Ordering;

use faf_domain::state::compare_versions;
use faf_ipc::shell::{
    GameAppContract, GameAppData, GameAppEnd, GameAppInstall, GameAppLastGame, GameAppStatus,
};
use serde::Deserialize;

/// Android's `Activity.RESULT_FIRST_USER`: the game app did not start a game,
/// and says why.
const RESULT_FIRST_USER: i64 = 1;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawStatus {
    installed: bool,
    version_name: String,
    version_code: i64,
    contract: Option<String>,
    game_data: Option<RawGameData>,
    last_game: Option<RawLastGame>,
    silent_update: bool,
}

/// The provider's `game_data` columns.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawGameData {
    scfa_archives: i64,
    scfa_fonts: i64,
    scfa_sound_banks: i64,
    scfa_movies: i64,
    game_versions: Option<String>,
    newest_known: Option<i64>,
    job_running: i64,
    job_step: Option<String>,
    game_running: i64,
    engine_commit: Option<String>,
}

/// The provider's `last_game` columns.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawLastGame {
    started_at: Option<i64>,
    ended: i64,
    exit_code: Option<i64>,
    crashed: i64,
    end: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawInstall {
    installed: bool,
    failure: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawEnd {
    result_code: i64,
    failure: Option<String>,
    message: Option<String>,
}

fn int(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

fn text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// What `GameAppPlugin.status` answered.
pub(crate) fn status_from(raw: serde_json::Value) -> Result<GameAppStatus, String> {
    let raw: RawStatus = serde_json::from_value(raw)
        .map_err(|error| format!("the game app's status could not be read: {error}"))?;
    let contract = match raw.contract.as_deref() {
        _ if !raw.installed => GameAppContract::None,
        Some("open") => GameAppContract::Open,
        Some("refused") => GameAppContract::Refused,
        _ => GameAppContract::None,
    };
    let open = contract == GameAppContract::Open;
    Ok(GameAppStatus {
        installed: raw.installed,
        version_name: raw.version_name,
        version_code: int(raw.version_code),
        contract,
        game_data: raw.game_data.filter(|_| open).map(|data| GameAppData {
            scfa_archives: int(data.scfa_archives),
            scfa_fonts: int(data.scfa_fonts),
            scfa_sound_banks: int(data.scfa_sound_banks),
            scfa_movies: int(data.scfa_movies),
            game_versions: data
                .game_versions
                .unwrap_or_default()
                .split(',')
                .filter_map(|version| version.trim().parse().ok())
                .collect(),
            newest_known: data.newest_known.filter(|newest| *newest >= 0).map(int),
            job_step: text(data.job_step).filter(|_| data.job_running != 0),
            game_running: data.game_running != 0,
            engine_commit: text(data.engine_commit).unwrap_or_default(),
        }),
        last_game: raw.last_game.filter(|_| open).and_then(|last| {
            Some(GameAppLastGame {
                // A row with no start is no game.
                started_at: last.started_at.filter(|started| *started > 0)? as f64,
                ended: last.ended != 0,
                exit_code: last.exit_code.map(int),
                crashed: last.crashed != 0,
                end: text(last.end),
            })
        }),
        silent_update: raw.installed && raw.silent_update,
    })
}

/// Why a start cannot be handed to the game app as it is now: it is missing,
/// too old for the launcher contract, refuses this client, or runs a game.
pub(crate) fn refusal_before_start(status: &GameAppStatus) -> Option<&'static str> {
    if !status.installed {
        return Some("FAF's game app is not installed");
    }
    match status.contract {
        GameAppContract::None => Some("FAF's game app is too old to start games from the client"),
        GameAppContract::Refused => Some("FAF's game app does not accept this client"),
        GameAppContract::Open => status
            .game_data
            .as_ref()
            .is_some_and(|data| data.game_running)
            .then_some("a game is already running in FAF's game app"),
    }
}

/// A refusal of the game app's, as the reason a launch failed: its own
/// words, with the code that names the case.
pub(crate) fn failure_reason(failure: &str, message: &str) -> String {
    if message.is_empty() {
        format!("FAF's game app did not start it ({failure})")
    } else {
        format!("FAF's game app did not start it: {message} ({failure})")
    }
}

/// Whether a release `version` is newer than the game app on this device. A
/// version name that is no release version, a build of one's own, is never
/// offered one.
pub(crate) fn is_newer(version: &str, installed: &GameAppStatus) -> bool {
    installed.installed && compare_versions(version, &installed.version_name) == Ordering::Greater
}

/// What `GameAppPlugin.installUpdate` answered. A refusal without its code is
/// Android's own (`INSTALL`).
pub(crate) fn install_from(raw: serde_json::Value) -> GameAppInstall {
    let raw: RawInstall = serde_json::from_value(raw).unwrap_or_default();
    if raw.installed {
        return GameAppInstall::Installed;
    }
    GameAppInstall::Failed {
        failure: text(raw.failure).unwrap_or_else(|| "INSTALL".to_owned()),
        message: text(raw.message).unwrap_or_default(),
    }
}

/// What `GameAppPlugin.ended` answered, for a start or the setup screen.
/// `RESULT_FIRST_USER` is a refusal even without its code: it never means a
/// game ran.
pub(crate) fn end_from(raw: serde_json::Value) -> GameAppEnd {
    let raw: RawEnd = serde_json::from_value(raw).unwrap_or_default();
    if raw.result_code != RESULT_FIRST_USER {
        return GameAppEnd::Ended;
    }
    GameAppEnd::Failed {
        failure: text(raw.failure).unwrap_or_default(),
        message: text(raw.message).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_missing_game_app_has_no_contract_and_no_data() {
        let status = status_from(json!({ "installed": false })).unwrap();
        assert!(!status.installed);
        assert_eq!(status.contract, GameAppContract::None);
        assert_eq!(status.game_data, None);
    }

    #[test]
    fn an_old_or_refusing_game_app_says_so_without_data() {
        let old = status_from(json!({ "installed": true, "versionName": "0.15.1", "versionCode": 4400, "contract": "none" }))
            .unwrap();
        assert_eq!(
            (old.contract, old.version_code),
            (GameAppContract::None, 4400)
        );
        let refused = status_from(
            json!({ "installed": true, "contract": "refused", "gameData": { "scfa_archives": 9 } }),
        )
        .unwrap();
        assert_eq!(refused.contract, GameAppContract::Refused);
        assert_eq!(refused.game_data, None, "a refused caller gets no data");
    }

    #[test]
    fn the_providers_rows_are_read_by_their_column_names() {
        let status = status_from(json!({
            "installed": true,
            "versionName": "0.16.0",
            "versionCode": 4490,
            "contract": "open",
            "gameData": {
                "scfa_archives": 42, "scfa_fonts": 3, "scfa_sound_banks": 0, "scfa_movies": 11,
                "game_versions": "3818, 3819,x", "newest_known": null,
                "job_running": 1, "job_step": "importing",
                "game_running": 0, "engine_commit": "fb096228e",
                "version_code": 4490, "version_name": "0.16.0"
            },
            "lastGame": { "started_at": 1763520000000_i64, "ended": 1, "exit_code": null, "crashed": 0, "end": "END ok" }
        }))
        .unwrap();
        assert_eq!(
            status.game_data,
            Some(GameAppData {
                scfa_archives: 42,
                scfa_fonts: 3,
                scfa_sound_banks: 0,
                scfa_movies: 11,
                game_versions: vec![3818, 3819],
                newest_known: None,
                job_step: Some("importing".into()),
                game_running: false,
                engine_commit: "fb096228e".into(),
            })
        );
        assert_eq!(
            status.last_game,
            Some(GameAppLastGame {
                started_at: 1_763_520_000_000.0,
                ended: true,
                exit_code: None,
                crashed: false,
                end: Some("END ok".into()),
            })
        );
    }

    #[test]
    fn a_start_waits_for_a_game_app_that_can_take_it() {
        let open = |running: i64| {
            status_from(json!({
                "installed": true,
                "contract": "open",
                "gameData": { "scfa_archives": 1, "scfa_fonts": 1, "game_running": running }
            }))
            .unwrap()
        };
        assert_eq!(refusal_before_start(&open(0)), None);
        assert!(
            refusal_before_start(&open(1)).is_some_and(|reason| reason.contains("already running"))
        );
        let old = status_from(json!({ "installed": true, "contract": "none" })).unwrap();
        assert!(refusal_before_start(&old).is_some_and(|reason| reason.contains("too old")));
        let missing = status_from(json!({ "installed": false })).unwrap();
        assert!(
            refusal_before_start(&missing).is_some_and(|reason| reason.contains("not installed"))
        );
        assert_eq!(
            failure_reason("NO_GAME_FILES", "no game files of version 3800"),
            "FAF's game app did not start it: no game files of version 3800 (NO_GAME_FILES)"
        );
    }

    #[test]
    fn only_a_newer_release_version_is_an_update() {
        let installed = |version: &str| {
            status_from(json!({ "installed": true, "versionName": version, "contract": "open" }))
                .unwrap()
        };
        assert!(is_newer("0.16.0", &installed("0.15.1")));
        assert!(is_newer("0.16.0", &installed("0.16.0-rc1")));
        assert!(!is_newer("0.16.0", &installed("0.16.0")));
        assert!(!is_newer("0.15.1", &installed("0.16.0")));
        assert!(
            !is_newer("0.16.0", &installed("my build")),
            "a version of one's own"
        );
        let missing = status_from(json!({ "installed": false })).unwrap();
        assert!(
            !is_newer("0.16.0", &missing),
            "D updates, it does not install"
        );
    }

    #[test]
    fn silent_updates_need_the_game_app() {
        let silent = status_from(json!({ "installed": true, "silentUpdate": true })).unwrap();
        assert!(silent.silent_update);
        let gone = status_from(json!({ "installed": false, "silentUpdate": true })).unwrap();
        assert!(!gone.silent_update);
    }

    #[test]
    fn an_install_says_installed_or_why_not() {
        assert_eq!(
            install_from(json!({ "installed": true })),
            GameAppInstall::Installed
        );
        assert_eq!(
            install_from(
                json!({ "installed": false, "failure": "SIGNER", "message": "signed by d00d" })
            ),
            GameAppInstall::Failed {
                failure: "SIGNER".into(),
                message: "signed by d00d".into()
            }
        );
        assert_eq!(
            install_from(
                json!({ "installed": false, "message": "INSTALL_FAILED_INSUFFICIENT_STORAGE" })
            ),
            GameAppInstall::Failed {
                failure: "INSTALL".into(),
                message: "INSTALL_FAILED_INSUFFICIENT_STORAGE".into()
            }
        );
    }

    #[test]
    fn a_failed_start_carries_its_code_and_an_end_carries_nothing() {
        assert_eq!(
            end_from(
                json!({ "resultCode": 1, "failure": "NO_SCFA", "message": "The game's files are missing." })
            ),
            GameAppEnd::Failed {
                failure: "NO_SCFA".into(),
                message: "The game's files are missing.".into()
            }
        );
        assert_eq!(
            end_from(json!({ "resultCode": 1 })),
            GameAppEnd::Failed {
                failure: String::new(),
                message: String::new()
            },
            "a refusal without its code is still no game"
        );
        assert_eq!(end_from(json!({ "resultCode": 0 })), GameAppEnd::Ended);
        assert_eq!(end_from(json!({ "resultCode": -1 })), GameAppEnd::Ended);
    }
}
