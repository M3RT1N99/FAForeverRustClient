//! The game's own network: faf-re's native mode, with no adapter at all (#274).
//!
//! faf-re's game, started with `/nativenet`, runs every link to the other
//! players itself, over ICE that FAF's Java adapter accepts as one of its own.
//! What an adapter did for the client is then left to the client, and that is
//! little (faf-re's `docs/LAUNCHER_NATIVE_NETWORK.md`): it hosts the game's
//! GPGNet port, and passes messages between the game and the lobby server as
//! they are.
//!
//! - On the game's `GameState Idle` it sends `IceServers`, this game's ICE
//!   session from FAF's API as the Java adapter is given it, then the
//!   `CreateLobby` Pioneer's bridge sends. `IceServers` goes again when the
//!   list changes, as it does when the session comes after the game's `Idle`.
//! - Every lobby message for the game goes to it as the server sent it,
//!   `IceMsg` included. GPGNet has no booleans, so `ConnectToPeer`'s offer
//!   goes as 0 or 1.
//! - Every game message goes to the lobby, the game's `IceMsg [receiver,
//!   text]` included, which the server passes on to the receiver's game.
//!
//! A game that crashes in its match can take its seat back (#275). Its links
//! die with it, so the client keeps what the server says only once, the
//! `JoinGame` and `ConnectToPeer` for this game, keeps the port open, drops
//! the `IceMsg` for the game while none is connected, and sends a game
//! started again with `/rejoin` what the first one had: see
//! [`IcePort::begin_rejoin`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use faf_domain::protocol::gpgnet::{GpgArg, GpgMessage};
use faf_domain::state::RelayStatus;
use futures_util::future::BoxFuture;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::AbortHandle;

use crate::infra::connectivity::IceSessionMemory;
use crate::infra::ice_java::{self, IceServers};
use crate::infra::ice_pioneer::{create_lobby, is_game_state, json_from_gpg_args};
use crate::infra::relay::{GameConnection, GpgRelayServer};
use crate::infra::session::TokenStore;
use crate::ports::{
    ConnectivitySession, GameLinks, IceParams, IcePort, Rejoin, RejoinSeat, RelayMsg,
};

/// One game's ICE session, still to be had; refused at once when it cannot
/// be asked for at all.
type SessionFetch =
    dyn Fn(i32) -> Result<BoxFuture<'static, Result<IceServers, String>>, String> + Send + Sync;

pub struct NativeNetwork {
    sessions: Box<SessionFetch>,
    /// Where the relay URLs of each session go, for the connectivity check.
    memory: IceSessionMemory,
    /// The `forceRelay` of the last session. The game is started before its
    /// session arrives, and `/nativenetrelay` is on its command line, so it
    /// is started with what the last session said: see
    /// [`ice_java::finish_late_session`], which the Java adapter's switch
    /// goes through for the same reason.
    force_relay_seen: Arc<AtomicBool>,
    /// The game's GPGNet port.
    port: GpgRelayServer,
    running: Mutex<Option<Running>>,
}

/// The game being played: what bridges it, and what is kept of its match.
struct Running {
    /// The bridge and the session's fetch, stopped with the game.
    tasks: [AbortHandle; 2],
    game: Arc<Mutex<Match>>,
}

impl NativeNetwork {
    /// FAF's: each game's ICE servers come from the API, as the Java
    /// adapter's do.
    pub fn faf(tokens: TokenStore) -> Self {
        let http = super::http::shared_http_client();
        let api_base = super::env_or("FAF_API_BASE", "https://api.faforever.com");
        Self::from_sessions(Box::new(move |game_id| {
            // Without a sign-in there is no session to ask for, and no game.
            let token = tokens.get().ok_or("no access token (not logged in?)")?;
            let (http, api_base) = (http.clone(), api_base.clone());
            Ok(Box::pin(async move {
                ice_java::ice_session(&http, &api_base, &token, game_id).await
            }))
        }))
    }

    /// Every game's ICE servers are `servers`, rather than a session from
    /// FAF's API: for a lobby with no API behind it, a local server or a test.
    pub fn with_ice_servers(servers: Vec<Value>) -> Self {
        Self::from_sessions(Box::new(move |_game_id| {
            let servers = servers.clone();
            Ok(Box::pin(async move {
                Ok(IceServers {
                    servers,
                    force_relay: false,
                })
            }))
        }))
    }

    fn from_sessions(sessions: Box<SessionFetch>) -> Self {
        Self {
            sessions,
            memory: IceSessionMemory::default(),
            force_relay_seen: Arc::new(AtomicBool::new(false)),
            port: GpgRelayServer::default(),
            running: Mutex::new(None),
        }
    }

    /// Share the session memory the connectivity check reads.
    pub fn remembering(mut self, memory: IceSessionMemory) -> Self {
        self.memory = memory;
        self
    }

    /// Stop bridging the game, and stop waiting for its session: a game that
    /// ended or was called off is not failed by a session still on its way.
    fn halt(&self) {
        if let Some(running) = self.running.lock().unwrap().take() {
            for task in running.tasks {
                task.abort();
            }
        }
    }
}

#[async_trait]
impl IcePort for NativeNetwork {
    async fn start(&self, params: IceParams) -> Result<ConnectivitySession, String> {
        let session = (self.sessions)(params.game_id)?;
        // Whatever an earlier game left running is no use to this one.
        self.halt();
        let listener = self.port.listen().await?;
        let game_id = params.game_id;
        // The servers remembered for the last game are not this one's.
        self.memory.remember_game(game_id);
        let force_relay = self.force_relay_seen.load(Ordering::SeqCst);

        let (to_lobby_tx, to_lobby_rx) = mpsc::channel::<RelayMsg>(64);
        let (from_lobby_tx, from_lobby_rx) = mpsc::channel::<RelayMsg>(64);
        let (servers_tx, servers_rx) = watch::channel::<Option<String>>(None);
        let (failed_tx, failed_rx) = oneshot::channel::<String>();

        // The game does not wait for its session: it loads for a quarter of
        // a minute before it says `Idle`, and FAF's icebreaker has taken ten
        // seconds of that on its own (#511).
        let memory = self.memory.clone();
        let seen = self.force_relay_seen.clone();
        let session_task = tokio::spawn(async move {
            if let Some(ice) =
                ice_java::finish_late_session(session, force_relay, &seen, failed_tx).await
            {
                tracing::info!(game_id, "the game's ICE session arrived");
                memory.remember_servers(game_id, &ice.servers);
                let _ = servers_tx.send(Some(Value::Array(ice.servers).to_string()));
            }
        });

        let game = Arc::new(Mutex::new(Match::default()));
        let bridge = Bridge {
            game: game.clone(),
            init_mode: params.init_mode,
            player_login: params.player_login,
            player_id: params.player_id,
            to_lobby: to_lobby_tx,
        };
        let bridge_task = tokio::spawn(bridge.run(listener.events, from_lobby_rx, servers_rx));
        *self.running.lock().unwrap() = Some(Running {
            tasks: [bridge_task.abort_handle(), session_task.abort_handle()],
            game,
        });
        tracing::info!(
            game_id,
            gpgnet_port = listener.port,
            force_relay,
            "the game runs its own links; its GPGNet port is open"
        );

        Ok(ConnectivitySession {
            game_port: listener.port,
            links: GameLinks::Game { force_relay },
            to_lobby: to_lobby_rx,
            from_lobby: from_lobby_tx,
            late_failure: Some(failed_rx),
        })
    }

    fn stop(&self) {
        self.halt();
        self.port.stop();
    }

    fn begin_rejoin(&self) -> Option<Rejoin> {
        let game = self.running.lock().unwrap().as_ref()?.game.clone();
        let mut kept = game.lock().unwrap();
        kept.rejoin()
    }

    /// There is no adapter to ask: the game shows its links in its own
    /// network panel.
    async fn relay_status(&self) -> RelayStatus {
        if self.running.lock().unwrap().is_some() {
            RelayStatus::GameLinks
        } else {
            RelayStatus::Idle
        }
    }
}

/// What the client keeps of a match while it is played: what a game coming
/// back to it needs, which the server says only once.
#[derive(Debug, Default)]
struct Match {
    /// The server's `JoinGame` and `ConnectToPeer` for this game, by player,
    /// in the order they came, without the players it said left.
    peers: Vec<(i32, GpgMessage)>,
    /// The game's last `RejoinOffer`.
    offer: Option<RejoinOffer>,
    /// The game said its match ended (`GameEnded`): a crash after that has
    /// no match to come back to.
    ended: bool,
    /// The next game to connect is this match's, coming back to it.
    returning: bool,
}

impl Match {
    /// Keep a player's `JoinGame` or `ConnectToPeer`, the latest one only, and
    /// forget the player on `DisconnectFromPeer`.
    fn remember(&mut self, message: &RelayMsg) {
        let (id_at, keep) = match message.command.as_str() {
            "JoinGame" | "ConnectToPeer" => (1, true),
            "DisconnectFromPeer" => (0, false),
            _ => return,
        };
        let Some(player_id) = message
            .args
            .get(id_at)
            .and_then(Value::as_i64)
            .and_then(|id| i32::try_from(id).ok())
        else {
            return;
        };
        self.peers.retain(|(known, _)| *known != player_id);
        if keep {
            self.peers.push((player_id, gpg_message(message)));
        }
    }

    /// What the game comes back with, from its offer, for the players still
    /// in the match; and the next game to connect is then taken for it.
    fn rejoin(&mut self) -> Option<Rejoin> {
        if self.ended {
            return None;
        }
        let offer = self.offer.as_ref()?;
        let seats: Vec<RejoinSeat> = self
            .peers
            .iter()
            .filter_map(|(player_id, _)| {
                offer
                    .seats
                    .iter()
                    .find(|seat| seat.player_id == *player_id)
                    .copied()
            })
            .collect();
        if seats.is_empty() {
            return None;
        }
        self.returning = true;
        Some(Rejoin {
            seats,
            source: offer.source,
        })
    }
}

/// What a game said it can come back to its match with: its own source, and
/// every player's by id. It says so only when every player's game can take a
/// player back.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RejoinOffer {
    source: i32,
    seats: Vec<RejoinSeat>,
}

impl RejoinOffer {
    /// `RejoinOffer [own source, "player id=source,..."]`, both as text, as
    /// the game writes it.
    fn parse(args: &[GpgArg]) -> Option<Self> {
        let [source, seats] = args else {
            return None;
        };
        let number = |text: &str| text.trim().parse::<i32>().ok();
        let source = match source {
            GpgArg::Int(source) => *source,
            GpgArg::Str(text) => number(text)?,
        };
        let GpgArg::Str(seats) = seats else {
            return None;
        };
        let seats = seats
            .split(',')
            .map(|seat| {
                let (player_id, source) = seat.split_once('=')?;
                Some(RejoinSeat {
                    player_id: number(player_id)?,
                    source: number(source)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { source, seats })
    }
}

/// One game's messages, between its GPGNet connection and the lobby.
struct Bridge {
    game: Arc<Mutex<Match>>,
    init_mode: i32,
    player_login: String,
    player_id: i32,
    to_lobby: mpsc::Sender<RelayMsg>,
}

/// The game connected right now.
struct Connected {
    to_game: mpsc::Sender<GpgMessage>,
    /// It was sent its servers and `CreateLobby`: from here a new list goes
    /// to it as soon as there is one.
    set_up: bool,
}

impl Bridge {
    async fn run(
        self,
        mut games: mpsc::Receiver<GameConnection>,
        mut from_lobby: mpsc::Receiver<RelayMsg>,
        mut servers: watch::Receiver<Option<String>>,
    ) {
        let mut connected: Option<Connected> = None;
        let mut lobby_open = true;
        let mut servers_open = true;
        loop {
            tokio::select! {
                event = games.recv() => {
                    // The port was stopped.
                    let Some(event) = event else { break };
                    match event {
                        GameConnection::Connected(to_game) => {
                            let mut game = Connected { to_game, set_up: false };
                            self.welcome_back(&mut game, &servers).await;
                            connected = Some(game);
                        }
                        GameConnection::Message(message) => {
                            if is_game_state(&message, "Idle") {
                                if let Some(game) = connected.as_mut() {
                                    self.set_up(game, &servers).await;
                                }
                            }
                            if !self.note(&message) {
                                continue;
                            }
                            let relay = RelayMsg {
                                command: message.command,
                                args: json_from_gpg_args(&message.args),
                            };
                            if self.to_lobby.send(relay).await.is_err() {
                                break; // the launcher is gone
                            }
                        }
                        GameConnection::Closed => connected = None,
                    }
                }
                message = from_lobby.recv(), if lobby_open => {
                    let Some(message) = message else {
                        lobby_open = false;
                        continue;
                    };
                    self.game.lock().unwrap().remember(&message);
                    match &connected {
                        Some(game) => {
                            let _ = game.to_game.send(gpg_message(&message)).await;
                        }
                        // `IceMsg` among them: the links that sent those try
                        // again, and what the game needs of the rest is kept.
                        None => tracing::debug!(
                            command = %message.command,
                            "no game is connected; a lobby message for it is dropped"
                        ),
                    }
                }
                changed = servers.changed(), if servers_open => {
                    if changed.is_err() {
                        // The session ended, with or without servers.
                        servers_open = false;
                        continue;
                    }
                    let list = servers.borrow().clone();
                    if let (Some(game), Some(list)) =
                        (connected.as_ref().filter(|game| game.set_up), list)
                    {
                        let _ = game.to_game.send(ice_servers(list)).await;
                    }
                }
            }
        }
    }

    /// `IceServers` with the list known so far, then `CreateLobby`.
    async fn set_up(&self, game: &mut Connected, servers: &watch::Receiver<Option<String>>) {
        let list = servers.borrow().clone().unwrap_or_else(|| "[]".into());
        let _ = game.to_game.send(ice_servers(list)).await;
        let lobby = create_lobby(self.init_mode, &self.player_login, self.player_id);
        let _ = game.to_game.send(lobby).await;
        game.set_up = true;
    }

    /// A game coming back to its match gets what the first one got, at once:
    /// it reports no `GameState`, because the server would take a host's
    /// `Idle` for its game going back to a lobby.
    async fn welcome_back(&self, game: &mut Connected, servers: &watch::Receiver<Option<String>>) {
        let peers = {
            let mut kept = self.game.lock().unwrap();
            if !std::mem::take(&mut kept.returning) {
                return;
            }
            kept.peers
                .iter()
                .map(|(_, message)| message.clone())
                .collect::<Vec<_>>()
        };
        tracing::info!(
            peers = peers.len(),
            "the game came back to its match; sending it what it had"
        );
        self.set_up(game, servers).await;
        for message in peers {
            let _ = game.to_game.send(message).await;
        }
    }

    /// Note what the game says about its match. False for what it says to
    /// the launcher alone, which the lobby server has no use for.
    fn note(&self, message: &GpgMessage) -> bool {
        match message.command.as_str() {
            "RejoinOffer" => {
                match RejoinOffer::parse(&message.args) {
                    Some(offer) => {
                        tracing::info!(
                            source = offer.source,
                            players = offer.seats.len(),
                            "the game can come back to its match"
                        );
                        self.game.lock().unwrap().offer = Some(offer);
                    }
                    None => tracing::warn!(
                        args = ?message.args,
                        "ignoring a RejoinOffer that cannot be read"
                    ),
                }
                false
            }
            "GameEnded" => {
                self.game.lock().unwrap().ended = true;
                true
            }
            _ => true,
        }
    }
}

fn ice_servers(list: String) -> GpgMessage {
    GpgMessage::new("IceServers", vec![GpgArg::Str(list)])
}

/// A lobby message for the game, as the game takes it: the server's
/// arguments as they are, in GPGNet's two types.
fn gpg_message(message: &RelayMsg) -> GpgMessage {
    GpgMessage::new(
        message.command.clone(),
        message.args.iter().map(gpg_arg).collect(),
    )
}

/// GPGNet has ints and strings. A boolean, `ConnectToPeer`'s offer, goes as
/// 0 or 1; anything else it has no type for goes as its JSON text, as the
/// Java adapter's `IceMsg` record, a JSON object, does.
fn gpg_arg(value: &Value) -> GpgArg {
    match value {
        Value::String(text) => GpgArg::Str(text.clone()),
        Value::Bool(flag) => GpgArg::Int(i32::from(*flag)),
        Value::Number(number) => match number.as_i64().and_then(|n| i32::try_from(n).ok()) {
            Some(number) => GpgArg::Int(number),
            None => GpgArg::Str(number.to_string()),
        },
        other => GpgArg::Str(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use faf_domain::protocol::gpgnet::{drain, encode};
    use serde_json::json;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    fn relay(command: &str, args: Vec<Value>) -> RelayMsg {
        RelayMsg {
            command: command.into(),
            args,
        }
    }

    fn text(value: &str) -> GpgArg {
        GpgArg::Str(value.into())
    }

    #[test]
    fn lobby_messages_reach_the_game_as_the_server_sent_them() {
        assert_eq!(
            gpg_message(&relay(
                "ConnectToPeer",
                vec![json!("Ada"), json!(7), json!(true)]
            )),
            GpgMessage::new(
                "ConnectToPeer",
                vec![text("Ada"), GpgArg::Int(7), GpgArg::Int(1)]
            ),
            "the offer is kept, as an int"
        );
        assert_eq!(
            gpg_message(&relay(
                "ConnectToPeer",
                vec![json!("Ada"), json!(7), json!(false)]
            ))
            .args[2],
            GpgArg::Int(0)
        );
        assert_eq!(
            gpg_message(&relay("JoinGame", vec![json!("Bert"), json!(9)])),
            GpgMessage::new("JoinGame", vec![text("Bert"), GpgArg::Int(9)]),
            "no address is made up for the game: it links by player"
        );
        assert_eq!(
            gpg_message(&relay(
                "IceMsg",
                vec![json!(9), json!("{\"type\":\"offer\"}")]
            )),
            GpgMessage::new("IceMsg", vec![GpgArg::Int(9), text("{\"type\":\"offer\"}")])
        );
        // What the Java adapter of the player at the other end sends is an
        // object; the game reads it from its text.
        assert_eq!(
            gpg_message(&relay(
                "IceMsg",
                vec![json!(9), json!({"type": "candidate"})]
            )),
            GpgMessage::new(
                "IceMsg",
                vec![GpgArg::Int(9), text(r#"{"type":"candidate"}"#)]
            )
        );
        // A message nobody expected goes too, unchanged as far as GPGNet can.
        assert_eq!(
            gpg_message(&relay(
                "Future",
                vec![json!(4_294_967_296_i64), json!(null)]
            )),
            GpgMessage::new("Future", vec![text("4294967296"), text("null")])
        );
    }

    #[test]
    fn a_rejoin_offer_is_read_as_the_game_writes_it() {
        let offer = RejoinOffer::parse(&[text("1"), text("101=0, 102=1,103=2")]).unwrap();
        assert_eq!(offer.source, 1);
        assert_eq!(
            offer.seats,
            [
                RejoinSeat {
                    player_id: 101,
                    source: 0
                },
                RejoinSeat {
                    player_id: 102,
                    source: 1
                },
                RejoinSeat {
                    player_id: 103,
                    source: 2
                },
            ]
        );
        assert_eq!(
            RejoinOffer::parse(&[GpgArg::Int(1), text("101=0")]).map(|offer| offer.source),
            Some(1)
        );
        for broken in [
            vec![text("1")],
            vec![text("one"), text("101=0")],
            vec![text("1"), text("101")],
            vec![text("1"), text("101=0,")],
            vec![text("1"), GpgArg::Int(101)],
        ] {
            assert_eq!(RejoinOffer::parse(&broken), None, "{broken:?}");
        }
    }

    fn offer() -> RejoinOffer {
        RejoinOffer::parse(&[text("2"), text("101=0,102=1,103=2")]).unwrap()
    }

    #[test]
    fn the_match_keeps_its_peers_in_order_and_forgets_who_left() {
        let mut kept = Match::default();
        kept.remember(&relay("HostGame", vec![json!("scmp_009")]));
        kept.remember(&relay("JoinGame", vec![json!("Ada"), json!(101)]));
        kept.remember(&relay(
            "ConnectToPeer",
            vec![json!("Bert"), json!(102), json!(true)],
        ));
        kept.remember(&relay("IceMsg", vec![json!(102), json!("candidate")]));
        assert_eq!(
            kept.peers
                .iter()
                .map(|(id, message)| (*id, message.command.as_str()))
                .collect::<Vec<_>>(),
            [(101, "JoinGame"), (102, "ConnectToPeer")],
            "only what starts a link is kept"
        );

        kept.remember(&relay("DisconnectFromPeer", vec![json!(101)]));
        kept.offer = Some(offer());
        assert_eq!(
            kept.rejoin(),
            Some(Rejoin {
                seats: vec![RejoinSeat {
                    player_id: 102,
                    source: 1
                }],
                source: 2,
            }),
            "the player who left is not named, nor is this one"
        );
        assert!(kept.returning);
    }

    #[test]
    fn a_match_with_nothing_to_come_back_to_offers_no_rejoin() {
        let mut no_offer = Match::default();
        no_offer.remember(&relay("JoinGame", vec![json!("Ada"), json!(101)]));
        assert_eq!(no_offer.rejoin(), None, "the game never offered it");

        let mut ended = Match {
            offer: Some(offer()),
            ended: true,
            ..Match::default()
        };
        ended.remember(&relay("JoinGame", vec![json!("Ada"), json!(101)]));
        assert_eq!(ended.rejoin(), None, "the match is over");

        let mut alone = Match {
            offer: Some(offer()),
            ..Match::default()
        };
        alone.remember(&relay("JoinGame", vec![json!("Ada"), json!(101)]));
        alone.remember(&relay("DisconnectFromPeer", vec![json!(101)]));
        assert_eq!(alone.rejoin(), None, "nobody is left to return to");
        assert!(!alone.returning);
    }

    /// A game on the native port, speaking GPGNet.
    struct Game {
        socket: TcpStream,
        buffer: Vec<u8>,
        arrived: std::collections::VecDeque<GpgMessage>,
    }

    impl Game {
        async fn connect(port: u16) -> Self {
            Self {
                socket: TcpStream::connect(("127.0.0.1", port)).await.unwrap(),
                buffer: Vec::new(),
                arrived: Default::default(),
            }
        }

        async fn send(&mut self, message: GpgMessage) {
            self.socket.write_all(&encode(&message)).await.unwrap();
        }

        async fn next(&mut self) -> GpgMessage {
            loop {
                if let Some(message) = self.arrived.pop_front() {
                    return message;
                }
                let mut chunk = [0_u8; 4096];
                let read =
                    tokio::time::timeout(Duration::from_secs(5), self.socket.read(&mut chunk))
                        .await
                        .expect("the client says something")
                        .unwrap();
                assert!(read > 0, "the client closed the game's connection");
                self.buffer.extend_from_slice(&chunk[..read]);
                let drained = drain(&mut self.buffer);
                assert_eq!(drained.invalid, None);
                self.arrived.extend(drained.messages);
            }
        }
    }

    fn params() -> IceParams {
        IceParams {
            player_id: 101,
            player_login: "Ada".into(),
            game_id: 5,
            init_mode: 0,
            game_title: "Friday 4v4".into(),
            hosted: false,
            rebuilt_engine: true,
        }
    }

    fn idle() -> GpgMessage {
        GpgMessage::new("GameState", vec![text("Idle")])
    }

    #[tokio::test]
    async fn idle_brings_the_servers_then_the_lobby_and_reaches_the_lobby_server() {
        let network = NativeNetwork::with_ice_servers(vec![json!({"urls": ["turn:relay.test"]})]);
        let mut session = network.start(params()).await.unwrap();
        assert_eq!(session.links, GameLinks::Game { force_relay: false });

        let mut game = Game::connect(session.game_port).await;
        game.send(idle()).await;
        assert_eq!(
            game.next().await,
            GpgMessage::new(
                "IceServers",
                vec![text(r#"[{"urls":["turn:relay.test"]}]"#)]
            )
        );
        assert_eq!(
            game.next().await,
            GpgMessage::new(
                "CreateLobby",
                vec![
                    GpgArg::Int(0),
                    GpgArg::Int(0),
                    text("Ada"),
                    GpgArg::Int(101),
                    GpgArg::Int(1)
                ]
            )
        );
        assert_eq!(
            session.to_lobby.recv().await,
            Some(relay("GameState", vec![json!("Idle")]))
        );
        network.stop();
    }

    #[tokio::test]
    async fn a_session_later_than_the_game_is_sent_when_it_arrives() {
        let (arrive, arrived) = oneshot::channel::<()>();
        let arrived = Arc::new(Mutex::new(Some(arrived)));
        let network = NativeNetwork::from_sessions(Box::new(move |_| {
            let arrived = arrived.lock().unwrap().take().expect("one session");
            Ok(Box::pin(async move {
                let _ = arrived.await;
                Ok(IceServers {
                    servers: vec![json!({"urls": "turn:late.test"})],
                    force_relay: false,
                })
            }))
        }));
        let mut session = network.start(params()).await.unwrap();
        let mut game = Game::connect(session.game_port).await;
        game.send(idle()).await;
        assert_eq!(
            game.next().await,
            GpgMessage::new("IceServers", vec![text("[]")]),
            "the game does not wait for its session"
        );
        assert_eq!(game.next().await.command, "CreateLobby");

        arrive.send(()).unwrap();
        assert_eq!(
            game.next().await,
            GpgMessage::new("IceServers", vec![text(r#"[{"urls":"turn:late.test"}]"#)]),
            "and gets the list once there is one"
        );
        assert!(
            session.late_failure.take().unwrap().await.is_err(),
            "a session that arrived fails nothing"
        );
        network.stop();
    }

    #[tokio::test]
    async fn a_session_that_forces_relaying_after_the_start_stops_the_game() {
        let network = NativeNetwork::from_sessions(Box::new(|_| {
            Ok(Box::pin(async {
                Ok(IceServers {
                    servers: Vec::new(),
                    force_relay: true,
                })
            }))
        }));
        let session = network.start(params()).await.unwrap();
        assert_eq!(session.links, GameLinks::Game { force_relay: false });
        let reason = session.late_failure.unwrap().await.unwrap();
        assert!(reason.contains("please join again"), "{reason}");

        // The next game is started relaying only, from the start.
        let session = network.start(params()).await.unwrap();
        assert_eq!(session.links, GameLinks::Game { force_relay: true });
        network.stop();
    }

    #[tokio::test]
    async fn a_session_that_cannot_be_asked_for_refuses_the_start() {
        let network = NativeNetwork::faf(TokenStore::new());
        let refused = network.start(params()).await.err();
        assert_eq!(refused.as_deref(), Some("no access token (not logged in?)"));
        assert_eq!(network.relay_status().await, RelayStatus::Idle);
    }

    #[tokio::test]
    async fn the_live_relay_view_says_the_game_shows_its_links() {
        let network = NativeNetwork::with_ice_servers(Vec::new());
        assert_eq!(network.relay_status().await, RelayStatus::Idle);
        let _session = network.start(params()).await.unwrap();
        assert_eq!(network.relay_status().await, RelayStatus::GameLinks);
        network.stop();
        assert_eq!(network.relay_status().await, RelayStatus::Idle);
        assert_eq!(network.begin_rejoin(), None, "nothing to come back to");
    }
}
