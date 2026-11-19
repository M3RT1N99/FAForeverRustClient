//! FAF's game app: where the game is an app of its own (Android), the client
//! starts games and replays in it rather than as a process of its own.
//!
//! The game app keeps its own files: the player's Supreme Commander, FAF's
//! game versions, maps and mods it fetches itself. So a start names what it
//! needs instead of preparing an install, and the game app refuses one it
//! cannot play with its reason. Only the Android shell has one; it hands it
//! over at start-up (`infra::game_app::set_game_app`).

use std::path::PathBuf;

use async_trait::async_trait;

use super::process::{GameExit, GameMod, Rejoin};

/// One start in the game app: what it plays, and what it needs ready for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameAppStart {
    /// FAF's game version (`3841`); `None` for the newest the game app has.
    pub game_version: Option<i32>,
    /// The featured mod. The game app plays `faf` alone for now.
    pub featured_mod: Option<String>,
    /// The game's switches, each a word of its own, from the contract's
    /// whitelist (a live replay's `/replay gpgnet://127.0.0.1:...`).
    pub arguments: Vec<String>,
    /// FAF's map folder name, which the game app fetches where it lacks the
    /// map; see [`contract_map_name`].
    pub map: Option<String>,
    /// The simulation mods the game needs, in order (`MODS` and
    /// `MOD_FILES`), which the game app fetches where it lacks them.
    pub mods: Vec<GameMod>,
    /// The game offers to come back after a crash (`REJOIN_OFFER`), as every
    /// game on its own links does. The game app adds the switches for it.
    pub rejoin_offer: bool,
    /// The seat a game coming back to its match takes (`REJOIN_PEERS` and
    /// `REJOIN_SOURCE`); needs [`Self::rejoin_offer`].
    pub rejoin: Option<Rejoin>,
    /// A replay file in SCFA's format, which the game app is handed to read.
    pub replay: Option<PathBuf>,
}

/// A start the game app took: its game runs.
pub struct GameAppRun {
    /// Resolves when the game is over: `Ok` with how it ended once it ran,
    /// `Err` with why when the game app gave it up before it ran (a map it
    /// could not fetch, say). Dropped by a caller that stops waiting for the
    /// end, which also lets the client be frozen in the back again.
    pub ended: tokio::sync::oneshot::Receiver<Result<GameExit, String>>,
}

#[async_trait]
pub trait GameAppPort: Send + Sync {
    /// Hand `start` to the game app. Resolves once its game runs, or with why
    /// the game app did not start it.
    async fn start(&self, start: GameAppStart) -> Result<GameAppRun, String>;
}

/// A map folder name as the game app's contract takes it: lower case, of
/// `a-z 0-9 _ . -`, at most 64 long, and no `.` or `..`. FAF's folder names
/// fit once lowered; one that does not is left out rather than refused, so
/// a map the phone already has still plays.
pub fn contract_map_name(folder: &str) -> Option<String> {
    let name = folder.to_ascii_lowercase();
    let fits = (1..=64).contains(&name.len())
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
        })
        && name != "."
        && name != "..";
    fits.then_some(name)
}

/// The name in a live replay's `/replay gpgnet://127.0.0.1:<port>/<id>/<name>.SCFAreplay`
/// as the contract takes it: `A-Z a-z 0-9 _ . -`, at most 64. Nobody reads
/// it (the local proxy relays whatever the game asks for), so what does not
/// fit is left out, and an empty name is `live`.
pub fn contract_replay_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "_.-".contains(*c))
        .take(64)
        .collect();
    if kept.is_empty() {
        "live".to_owned()
    } else {
        kept
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_names_are_lowered_and_kept_to_the_contract() {
        assert_eq!(contract_map_name("SCMP_009").as_deref(), Some("scmp_009"));
        assert_eq!(
            contract_map_name("dualgap_adaptive.v0012").as_deref(),
            Some("dualgap_adaptive.v0012")
        );
        for refused in [
            "",
            ".",
            "..",
            "maps/scmp_009",
            "Seton's Clutch",
            &"x".repeat(65),
        ] {
            assert_eq!(contract_map_name(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_replay_name_keeps_what_the_contract_allows() {
        assert_eq!(contract_replay_name("M3RT1N99"), "M3RT1N99");
        assert_eq!(contract_replay_name("[clan] Ötzi"), "clantzi");
        assert_eq!(contract_replay_name("§$%"), "live");
        assert_eq!(contract_replay_name(&"a".repeat(80)).len(), 64);
    }
}
