//! The live list from FAF's API while the lobby is closed.
//!
//! The live view's games used to come from the lobby's `game_info` alone, so
//! where the lobby stays closed (Android, until FAF publishes its faf-uid) the
//! list was empty. While the lobby is closed the API's running games stand
//! in; while it is connected its own feed is the list and the API is not
//! asked. A game picked from the API's list is watched on the same path, the
//! replay delay counted from the start the API gave.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use faf_app::infra::fake_ports;
use faf_app::ports::{ReplayPlaybackPort, ReplayVaultPort, VaultSearchResult};
use faf_app::{App, Ports};
use faf_domain::state::{
    Game, LiveReplayTarget, LobbyCommand, LobbyStatus, LocalReplay, ReplayCommand, ReplayQuery,
    ReplayStatus, VaultStatus, LIVE_REPLAY_DELAY_SECONDS,
};

use crate::support;

/// FAF's API as the live view asks it: the games running now, and how often
/// it was asked for them.
struct RunningVault {
    games: Vec<Game>,
    asked: Arc<Mutex<u32>>,
}

#[async_trait]
impl ReplayVaultPort for RunningVault {
    async fn search_vault(&self, _query: ReplayQuery) -> Result<VaultSearchResult, String> {
        Err("the vault is not searched here".into())
    }
    async fn list_featured_mods(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
    async fn download_vault(&self, _uid: i32) -> Result<LocalReplay, String> {
        Err("nothing is downloaded here".into())
    }
    async fn running_games(&self) -> Result<Vec<Game>, String> {
        *self.asked.lock().unwrap() += 1;
        Ok(self.games.clone())
    }
}

/// Records the live streams it was asked to open.
struct RecordingPlayback {
    watched: Arc<Mutex<Vec<LiveReplayTarget>>>,
}

#[async_trait]
impl ReplayPlaybackPort for RecordingPlayback {
    async fn watch_live(
        &self,
        target: LiveReplayTarget,
        _player: String,
    ) -> Result<Option<String>, String> {
        self.watched.lock().unwrap().push(target);
        Ok(None)
    }
    async fn play_file(&self, _path: PathBuf) -> Result<Option<String>, String> {
        Ok(None)
    }
    async fn watch_vault(&self, _uid: i32) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn set_install_dir(&self, _dir: Option<PathBuf>) {}
}

struct Harness {
    app: App,
    asked: Arc<Mutex<u32>>,
    watched: Arc<Mutex<Vec<LiveReplayTarget>>>,
}

fn start(games: Vec<Game>) -> Harness {
    let asked = Arc::new(Mutex::new(0));
    let watched = Arc::new(Mutex::new(Vec::new()));
    let ports = Ports {
        replay_vault: Arc::new(RunningVault {
            games,
            asked: asked.clone(),
        }),
        replay_playback: Arc::new(RecordingPlayback {
            watched: watched.clone(),
        }),
        ..fake_ports()
    };
    Harness {
        app: support::start(ports),
        asked,
        watched,
    }
}

/// A game the API lists as running, started `age` seconds ago.
fn running(uid: i32, age: u32) -> Game {
    let now = u32::try_from(chrono::Utc::now().timestamp()).unwrap();
    Game {
        id: uid,
        title: format!("game {uid}"),
        host: "Ada".into(),
        players: 2,
        max_players: 2,
        map: "scmp_009".into(),
        mod_name: "faf".into(),
        average_rating: 0,
        rating_type: "global".into(),
        password_protected: false,
        visibility: "public".into(),
        game_type: String::new(),
        launched_at: Some(now - age),
        hosted_at: None,
        rating_min: None,
        rating_max: None,
        enforce_rating_range: false,
        teams: [
            ("2".to_owned(), vec!["Ada".to_owned()]),
            ("3".to_owned(), vec!["Bert".to_owned()]),
        ]
        .into(),
        sim_mods: Default::default(),
    }
}

fn target(uid: i32) -> LiveReplayTarget {
    LiveReplayTarget {
        uid,
        mod_name: "faf".into(),
        map: "scmp_009".into(),
    }
}

impl Harness {
    /// Ask for the API's list and wait for it to arrive.
    async fn load_running_games(&self) -> Vec<Game> {
        self.app
            .dispatch(ReplayCommand::LoadRunningGames.into())
            .await
            .unwrap();
        support::find(&self.app, support::PATIENCE, |state| {
            (state.replays.running_games_status == VaultStatus::Ready)
                .then(|| state.replays.running_games.clone())
        })
        .await
        .expect("the running games never arrived")
    }
}

#[tokio::test]
async fn while_the_lobby_is_closed_the_live_list_comes_from_the_api() {
    let h = start(vec![running(42, 600)]);
    assert_eq!(h.app.snapshot().lobby.status, LobbyStatus::Disconnected);
    let games = h.load_running_games().await;
    assert_eq!(games.iter().map(|game| game.id).collect::<Vec<_>>(), [42]);
    assert!(
        h.app.snapshot().lobby.live_games.is_empty(),
        "the lobby's own list is not touched"
    );
    assert_eq!(*h.asked.lock().unwrap(), 1);
}

#[tokio::test]
async fn while_the_lobby_is_connected_its_feed_is_the_list_and_the_api_is_not_asked() {
    let h = start(vec![running(42, 600)]);
    h.app.dispatch(LobbyCommand::Connect.into()).await.unwrap();
    assert!(
        support::reaches(&h.app, support::PATIENCE, |state| {
            state.lobby.status == LobbyStatus::Connected
        })
        .await,
        "the fake lobby never connected"
    );
    h.app
        .dispatch_and_wait(ReplayCommand::LoadRunningGames.into())
        .await
        .unwrap();
    assert_eq!(*h.asked.lock().unwrap(), 0);
    let state = h.app.snapshot();
    assert_eq!(state.replays.running_games_status, VaultStatus::Idle);
    assert!(state.replays.running_games.is_empty());
}

#[tokio::test]
async fn a_watch_from_an_api_row_keeps_the_delay_and_needs_only_the_game_s_id() {
    let h = start(vec![
        running(42, 10),
        running(43, LIVE_REPLAY_DELAY_SECONDS + 60),
    ]);
    h.load_running_games().await;

    // Started ten seconds ago by the API's clock: refused, whichever route
    // asks, and no stream is opened.
    h.app
        .dispatch_and_wait(ReplayCommand::WatchLive(target(42)).into())
        .await
        .unwrap();
    let state = h.app.snapshot();
    assert!(
        matches!(&state.replays.status, ReplayStatus::Failed { reason } if reason.contains("five minutes")),
        "refused for the delay: {:?}",
        state.replays.status
    );
    assert!(h.watched.lock().unwrap().is_empty());

    // Past the delay: the same watch path as a game the lobby listed.
    h.app
        .dispatch_and_wait(ReplayCommand::WatchLive(target(43)).into())
        .await
        .unwrap();
    assert!(
        support::reaches(&h.app, support::PATIENCE, |state| {
            matches!(
                state.replays.status,
                ReplayStatus::Playing { uid: Some(43) }
            )
        })
        .await,
        "the watch never started"
    );
    assert_eq!(*h.watched.lock().unwrap(), [target(43)]);
}
