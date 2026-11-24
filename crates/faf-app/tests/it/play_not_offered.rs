//! A build without play: the Android release, which watches replays only.
//!
//! The page offers no way into hosting, joining or the matchmaker there; the
//! core is the lock behind it, and refuses those requests whoever sends them,
//! with a notice that says why rather than a request that goes nowhere.

use std::sync::Arc;

use faf_app::infra::{fake_ports, FakeLobby};
use faf_app::{App, Ports};
use faf_domain::state::{
    HostGameConfig, JoinState, LobbyCommand, LobbyStatus, MatchmakingState, NotificationKind,
};

use crate::support;

fn host_config() -> HostGameConfig {
    HostGameConfig {
        title: "Friday game".into(),
        mod_name: "faf".into(),
        visibility: "public".into(),
        map: "scmp_009".into(),
        password: None,
        enforce_rating_range: false,
        rating_min: None,
        rating_max: None,
    }
}

/// A client on the fake lobby, connected, whose build offers play or not.
async fn connected(play_offered: bool) -> (App, FakeLobby) {
    let lobby = FakeLobby::default();
    // A join the fake would answer at once is kept instead, so a join that
    // got through is still there to be seen.
    lobby.hold_join_answers();
    let app = support::start(Ports {
        lobby: Arc::new(lobby.clone()),
        play_offered,
        ..fake_ports()
    });
    app.dispatch(LobbyCommand::Connect.into()).await.unwrap();
    assert!(
        support::reaches(&app, support::PATIENCE, |state| {
            state.lobby.status == LobbyStatus::Connected && !state.lobby.games.is_empty()
        })
        .await,
        "the fake lobby never connected"
    );
    (app, lobby)
}

#[tokio::test]
async fn a_build_without_play_refuses_hosting_joining_and_queueing() {
    let (app, lobby) = connected(false).await;
    let game = app.snapshot().lobby.games[0].id;
    let requests = [
        LobbyCommand::Host {
            config: host_config(),
        },
        LobbyCommand::Join {
            id: game,
            password: None,
            replace_mods: false,
        },
        LobbyCommand::StartSearch {
            queue_names: vec!["ladder1v1".into()],
        },
        LobbyCommand::Matchmake {
            queue_name: "ladder1v1".into(),
            start: true,
        },
        LobbyCommand::InviteToParty { player_id: 7 },
        LobbyCommand::AcceptPartyInvite {
            player_id: 7,
            notification_id: None,
        },
    ];
    let count = requests.len();
    for request in requests {
        app.dispatch_and_wait(request.into()).await.unwrap();
    }

    assert!(lobby.hosted_configs().is_empty(), "nothing was hosted");
    assert!(lobby.held_joins().is_empty(), "nothing was joined");
    let state = app.snapshot();
    assert_eq!(state.lobby.join, JoinState::Idle);
    assert_eq!(state.lobby.matchmaking, MatchmakingState::Idle);
    let refusals: Vec<_> = state
        .notifications
        .items
        .iter()
        .filter(|item| {
            item.text
                .as_ref()
                .is_some_and(|text| text.key == "notifications.msg.playNotOffered")
        })
        .collect();
    assert_eq!(refusals.len(), count, "each request is told why");
    assert!(refusals
        .iter()
        .all(|item| item.kind == NotificationKind::Error && item.action.is_none()));
}

#[tokio::test]
async fn what_only_ends_play_is_not_refused() {
    let (app, lobby) = connected(false).await;
    // What only ends play is never refused: there is nothing to stop here,
    // and nothing is said about it.
    for request in [
        LobbyCommand::Matchmake {
            queue_name: "ladder1v1".into(),
            start: false,
        },
        LobbyCommand::CancelJoin,
        LobbyCommand::LeaveParty,
    ] {
        app.dispatch_and_wait(request.into()).await.unwrap();
    }
    assert!(app.snapshot().notifications.items.iter().all(|item| {
        item.text
            .as_ref()
            .is_none_or(|text| text.key != "notifications.msg.playNotOffered")
    }));
    assert!(lobby.hosted_configs().is_empty());
}

#[tokio::test]
async fn a_build_that_plays_hosts_as_before() {
    let (app, lobby) = connected(true).await;
    app.dispatch_and_wait(
        LobbyCommand::Host {
            config: host_config(),
        }
        .into(),
    )
    .await
    .unwrap();
    assert!(
        support::reaches(&app, support::PATIENCE, |_| !lobby
            .hosted_configs()
            .is_empty())
        .await,
        "the host request never reached the lobby"
    );
}
