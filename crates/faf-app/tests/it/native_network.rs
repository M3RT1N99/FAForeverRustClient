//! The game's own network (faf-re's native mode, #274), end to end in this
//! process and on loopback only: clients with the real backend for it, a lobby
//! server that routes the way FAF's does, and games that speak GPGNet to the
//! clients' ports.
//!
//! The server plays faf-server's `GameConnection` (`server/gameconnection.py`):
//! it hosts and seats players, sends each game `HostGame`, `JoinGame` and
//! `ConnectToPeer` as the games reach their lobby, `DisconnectFromPeer` when a
//! player leaves, and passes `IceMsg` from one player's game to another's the
//! way `handle_ice_message` does (the receiver gets `IceMsg [sender id,
//! text]`). The games stand where `ForgedAlliance.exe` would: they connect to
//! the port their client started them with, say `Idle` and `Lobby`, and trade
//! `IceMsg`. Their process is a stand-in the test ends as it likes.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use faf_app::infra::{fake_ports, FakeAuth, FakeIce, NativeNetwork, SelectableIce};
use faf_app::ports::{
    GameExit, GameLaunchParams, GameNetwork, InstallPresence, LobbyPort, LobbySendError,
    LobbyUpdate, ProcessPort, Rejoin, RejoinSeat,
};
use faf_app::{App, Ports};
use faf_domain::protocol::gpgnet::{drain, encode, GpgArg, GpgMessage};
use faf_domain::state::settings::ConnectivityPreferencesPatch;
use faf_domain::state::{
    AuthCommand, Contact, Game, GameLaunch, HostGameConfig, JoinState, LobbyCommand, LobbyStatus,
    Player, PlayerProfile, PlayerVeto, Relation, SettingsCommand,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::support;

const MAP: &str = "scmp_009";

/// The ICE servers every game here is given. No address: none of these games
/// leaves this computer.
fn ice_servers() -> Vec<Value> {
    vec![
        json!({"urls": ["turn:relay.invalid:3478?transport=udp"], "username": "player", "credential": "secret"}),
    ]
}

fn int(value: i32) -> GpgArg {
    GpgArg::Int(value)
}

fn text(value: &str) -> GpgArg {
    GpgArg::Str(value.to_string())
}

fn message(command: &str, args: Vec<GpgArg>) -> GpgMessage {
    GpgMessage::new(command, args)
}

fn host_config() -> HostGameConfig {
    HostGameConfig {
        title: "Friday game".into(),
        mod_name: "faf".into(),
        visibility: "public".into(),
        map: MAP.into(),
        password: None,
        enforce_rating_range: false,
        rating_min: None,
        rating_max: None,
    }
}

fn seat(player_id: i32, source: i32) -> RejoinSeat {
    RejoinSeat { player_id, source }
}

/// A developer's `FAF_ICE_ADAPTER_KIND` names an adapter for every game, and
/// these games are about having none.
fn adapter_named_by_the_environment() -> bool {
    std::env::var("FAF_ICE_ADAPTER_KIND").is_ok_and(|kind| !kind.is_empty())
}

/// FAF's lobby server, as far as games and their connections go.
#[derive(Default)]
struct LobbyServer {
    state: Mutex<ServerState>,
    /// Profiles sent to settle a client: see [`LobbyServer::settle`].
    markers: AtomicI32,
}

#[derive(Default)]
struct ServerState {
    connections: BTreeMap<i32, Connection>,
    games: BTreeMap<i32, Hosted>,
    /// Every game message each client sent, in order.
    received: Vec<(i32, String, Vec<Value>)>,
}

struct Connection {
    login: String,
    updates: Option<mpsc::Sender<LobbyUpdate>>,
    game: Option<i32>,
}

struct Hosted {
    listing: Game,
    host: i32,
    /// The players whose game reached its lobby, in that order.
    in_lobby: Vec<i32>,
}

impl ServerState {
    fn send(&self, player: i32, update: LobbyUpdate) {
        if let Some(updates) = self
            .connections
            .get(&player)
            .and_then(|connection| connection.updates.as_ref())
        {
            updates.try_send(update).expect("the client keeps up");
        }
    }

    fn to_game(&self, player: i32, command: &str, args: Vec<Value>) {
        self.send(
            player,
            LobbyUpdate::GameRelay {
                command: command.into(),
                args,
            },
        );
    }

    fn login(&self, player: i32) -> String {
        self.connections[&player].login.clone()
    }

    fn launch_order(&self, game_id: i32) -> GameLaunch {
        let hosted = &self.games[&game_id];
        GameLaunch {
            uid: game_id,
            mod_name: hosted.listing.mod_name.clone(),
            name: hosted.listing.title.clone(),
            mapname: hosted.listing.map.clone(),
            game_type: "custom".into(),
            rating_type: "global".into(),
            expected_players: None,
            team: None,
            faction: None,
            map_position: None,
            game_options: Default::default(),
            args: vec!["/numgames".into(), "12".into()],
        }
    }

    /// The game reached its lobby: `_handle_lobby_state`. A host is sent its
    /// map; anybody else is linked to the host (`connect_to_host`), then to
    /// every other player already there (`connect_to_peer`).
    fn seat(&mut self, player: i32) {
        let Some(game_id) = self.connections[&player].game else {
            return;
        };
        let (host, others, map) = {
            let hosted = &self.games[&game_id];
            (
                hosted.host,
                hosted.in_lobby.clone(),
                hosted.listing.map.clone(),
            )
        };
        if player == host {
            self.to_game(player, "HostGame", vec![json!(map)]);
        } else {
            self.to_game(
                player,
                "JoinGame",
                vec![json!(self.login(host)), json!(host)],
            );
            self.to_game(
                host,
                "ConnectToPeer",
                vec![json!(self.login(player)), json!(player), json!(true)],
            );
            for peer in others.into_iter().filter(|peer| *peer != host) {
                self.to_game(
                    player,
                    "ConnectToPeer",
                    vec![json!(self.login(peer)), json!(peer), json!(true)],
                );
                self.to_game(
                    peer,
                    "ConnectToPeer",
                    vec![json!(self.login(player)), json!(player), json!(false)],
                );
            }
        }
        self.games.get_mut(&game_id).unwrap().in_lobby.push(player);
    }

    /// The player's game ended (`on_connection_lost`): everybody left in it
    /// hears `DisconnectFromPeer` (`disconnect_all_peers`).
    fn leave(&mut self, player: i32) {
        let Some(game_id) = self.connections.get_mut(&player).unwrap().game.take() else {
            return;
        };
        let hosted = self.games.get_mut(&game_id).unwrap();
        hosted.in_lobby.retain(|seated| *seated != player);
        for peer in hosted.in_lobby.clone() {
            self.to_game(peer, "DisconnectFromPeer", vec![json!(player)]);
        }
    }
}

impl LobbyServer {
    fn host(&self, player: i32, config: HostGameConfig) {
        let mut state = self.state.lock().unwrap();
        let game_id = 4_000 + state.games.len() as i32 + 1;
        let listing = Game {
            id: game_id,
            title: config.title,
            host: state.login(player),
            players: 1,
            max_players: 8,
            map: config.map,
            mod_name: config.mod_name,
            average_rating: 1_000,
            rating_type: "global".into(),
            password_protected: false,
            visibility: config.visibility,
            game_type: "custom".into(),
            launched_at: None,
            hosted_at: None,
            rating_min: None,
            rating_max: None,
            enforce_rating_range: false,
            teams: Default::default(),
            sim_mods: Default::default(),
        };
        state.games.insert(
            game_id,
            Hosted {
                listing: listing.clone(),
                host: player,
                in_lobby: Vec::new(),
            },
        );
        state.connections.get_mut(&player).unwrap().game = Some(game_id);
        let order = state.launch_order(game_id);
        state.send(player, LobbyUpdate::Launch(order));
        for other in state.connections.keys().copied().collect::<Vec<_>>() {
            state.send(
                other,
                LobbyUpdate::GamesChanged {
                    upserted: vec![listing.clone()],
                    removed: Vec::new(),
                },
            );
        }
    }

    fn join(&self, player: i32, game_id: i32) -> bool {
        let mut state = self.state.lock().unwrap();
        if !state.games.contains_key(&game_id) {
            return false;
        }
        state.connections.get_mut(&player).unwrap().game = Some(game_id);
        let order = state.launch_order(game_id);
        state.send(player, LobbyUpdate::Launch(order));
        true
    }

    /// A message for the server from a player's game, as their client relays
    /// it.
    fn game_message(&self, player: i32, command: &str, args: &[Value]) {
        let mut state = self.state.lock().unwrap();
        state
            .received
            .push((player, command.to_string(), args.to_vec()));
        match (command, args) {
            ("GameState", [state_name]) if state_name == "Lobby" => state.seat(player),
            ("GameState", [state_name]) if state_name == "Ended" => state.leave(player),
            // `handle_ice_message`: to the receiver, from the sender.
            ("IceMsg", [receiver, ice_msg]) => {
                let receiver = receiver.as_i64().expect("a player id") as i32;
                state.to_game(receiver, "IceMsg", vec![json!(player), ice_msg.clone()]);
            }
            _ => {}
        }
    }

    /// What `player`'s client relayed from its game, in order.
    fn received_from(&self, player: i32) -> Vec<(String, Vec<Value>)> {
        self.state
            .lock()
            .unwrap()
            .received
            .iter()
            .filter(|(from, _, _)| *from == player)
            .map(|(_, command, args)| (command.clone(), args.clone()))
            .collect()
    }

    /// How many `command` messages `player`'s client relayed from its game.
    fn count_from(&self, player: i32, command: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .received
            .iter()
            .filter(|(from, sent, _)| *from == player && sent == command)
            .count()
    }

    fn said_its_game_ended(&self, player: i32) -> bool {
        self.received_from(player)
            .iter()
            .any(|(command, args)| command == "GameState" && args == &[json!("Ended")])
    }

    /// Send `player`'s game a message of the server's own.
    fn tell(&self, player: i32, command: &str, args: Vec<Value>) {
        self.state.lock().unwrap().to_game(player, command, args);
    }

    /// The same, waiting for room as a socket does: for more than the
    /// client's queue holds at once.
    async fn tell_in_turn(&self, player: i32, command: &str, args: Vec<Value>) {
        let updates = self.state.lock().unwrap().connections[&player]
            .updates
            .clone()
            .expect("the client is connected");
        let relay = LobbyUpdate::GameRelay {
            command: command.into(),
            args,
        };
        updates.send(relay).await.expect("the client is there");
    }

    /// Wait until `client` has handled everything this server sent it so far:
    /// the profile sent last shows in its state only once all before it went
    /// through its lobby loop.
    async fn settle(&self, client: &Client) {
        let marker = 90_000 + self.markers.fetch_add(1, Ordering::SeqCst);
        self.state.lock().unwrap().send(
            client.id,
            LobbyUpdate::PlayersSeen(vec![PlayerProfile {
                id: marker,
                login: format!("marker{marker}"),
                ..PlayerProfile::default()
            }]),
        );
        support::until(&client.app, "the client caught up", |state| {
            state
                .social
                .players
                .iter()
                .any(|profile| profile.id == marker)
        })
        .await;
    }
}

/// One client's connection to the server.
#[derive(Clone)]
struct Seat {
    server: Arc<LobbyServer>,
    id: i32,
    login: String,
}

#[async_trait]
impl LobbyPort for Seat {
    async fn connect(&self) -> mpsc::Receiver<LobbyUpdate> {
        let (updates, received) = mpsc::channel(512);
        let mut state = self.server.state.lock().unwrap();
        let listings = state
            .games
            .values()
            .map(|hosted| hosted.listing.clone())
            .collect();
        updates.try_send(LobbyUpdate::Authenticated).unwrap();
        updates.try_send(LobbyUpdate::Games(listings)).unwrap();
        let game = state
            .connections
            .get(&self.id)
            .and_then(|connection| connection.game);
        state.connections.insert(
            self.id,
            Connection {
                login: self.login.clone(),
                updates: Some(updates),
                game,
            },
        );
        received
    }

    fn join(&self, id: i32, _password: Option<String>) -> bool {
        self.server.join(self.id, id)
    }

    fn host(&self, config: HostGameConfig) -> Result<(), LobbySendError> {
        self.server.host(self.id, config);
        Ok(())
    }

    fn matchmake(&self, _queue_names: &[String], _start: bool) -> Result<(), LobbySendError> {
        Ok(())
    }

    fn leave_party(&self) {}

    fn kick_party_member(&self, _player_id: i32) {}

    fn invite_to_party(&self, _player_id: i32) {}

    fn accept_party_invite(&self, _player_id: i32) -> Result<(), LobbySendError> {
        Ok(())
    }

    fn set_party_factions(&self, _factions: Vec<String>) {}

    fn set_contact_relation(
        &self,
        _contact: Contact,
        _member: bool,
        _remove: Option<Relation>,
    ) -> bool {
        true
    }

    fn set_player_vetoes(&self, _vetoes: Vec<PlayerVeto>) {}

    fn request_avatars(&self) -> bool {
        true
    }

    fn select_avatar(&self, _url: Option<String>) -> bool {
        true
    }

    fn restore_game_session(&self, _game_id: i32) -> bool {
        true
    }

    fn send_game_relay(&self, command: String, args: Vec<Value>) {
        self.server.game_message(self.id, &command, &args);
    }

    fn disconnect(&self) {
        if let Some(connection) = self
            .server
            .state
            .lock()
            .unwrap()
            .connections
            .get_mut(&self.id)
        {
            connection.updates = None;
        }
    }
}

/// The player's computer, as far as the game's process goes: nothing is
/// started, the test connects a game to the port the client started it with,
/// and ends its process as it likes.
struct Machine {
    launches: mpsc::UnboundedSender<GameLaunchParams>,
    end: mpsc::UnboundedSender<GameExit>,
    ends: tokio::sync::Mutex<mpsc::UnboundedReceiver<GameExit>>,
    running: AtomicBool,
}

impl Machine {
    fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<GameLaunchParams>) {
        let (launches, launched) = mpsc::unbounded_channel();
        let (end, ends) = mpsc::unbounded_channel();
        let machine = Self {
            launches,
            end,
            ends: tokio::sync::Mutex::new(ends),
            running: AtomicBool::new(false),
        };
        (Arc::new(machine), launched)
    }

    /// The game's process ends, the way `exit` says.
    fn end(&self, exit: GameExit) {
        self.running.store(false, Ordering::SeqCst);
        self.end.send(exit).unwrap();
    }
}

#[async_trait]
impl ProcessPort for Machine {
    fn supports_live_launch(&self) -> bool {
        true
    }
    fn game_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    async fn launch_game(&self, params: GameLaunchParams) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);
        self.launches.send(params).unwrap();
        Ok(())
    }
    async fn launch_offline(&self, _featured_mod: String, _map: String) -> Result<(), String> {
        Ok(())
    }
    async fn launch_replay(&self, _args: Vec<String>) -> Result<(), String> {
        Ok(())
    }
    fn kill(&self) {
        if self.running.swap(false, Ordering::SeqCst) {
            self.end.send(GameExit::Stopped).unwrap();
        }
    }
    async fn wait_for_exit(&self) -> GameExit {
        let exit = self.ends.lock().await.recv().await;
        exit.unwrap_or(GameExit::Stopped)
    }
    async fn game_is_rebuilt_engine(&self) -> bool {
        true
    }
    fn set_paths(&self, _game_path: String, _replay_game_path: String) {}
    fn set_additional_arguments(&self, _arguments: Vec<String>) {}
    fn game_install_dir(&self) -> Option<PathBuf> {
        Some(PathBuf::from("C:/fake-fa-install"))
    }
    fn replay_install_dir(&self) -> Option<PathBuf> {
        None
    }
    fn installs_present(&self) -> InstallPresence {
        InstallPresence::default()
    }
}

/// A player's client, signed in and connected to the server.
struct Client {
    app: App,
    id: i32,
    login: String,
    machine: Arc<Machine>,
    launched: mpsc::UnboundedReceiver<GameLaunchParams>,
}

impl Client {
    async fn sign_in(server: &Arc<LobbyServer>, id: i32, login: &str) -> Self {
        let (machine, launched) = Machine::new();
        let ice = SelectableIce::new(
            Arc::new(FakeIce),
            Arc::new(FakeIce),
            Arc::new(FakeIce),
            Arc::new(NativeNetwork::with_ice_servers(ice_servers())),
        );
        let ports = Ports {
            auth: Arc::new(FakeAuth {
                player: Player::new(id, login),
                delay: Duration::ZERO,
                fail_with: None,
            }),
            lobby: Arc::new(Seat {
                server: server.clone(),
                id,
                login: login.into(),
            }),
            process: machine.clone(),
            ice: Arc::new(ice),
            ..fake_ports()
        };
        let app = support::start(ports);
        app.dispatch_and_wait(AuthCommand::Login { remember: false }.into())
            .await
            .unwrap();
        app.dispatch(LobbyCommand::Connect.into()).await.unwrap();
        support::until(&app, "the lobby connected", |state| {
            state.lobby.status == LobbyStatus::Connected
        })
        .await;
        Self {
            app,
            id,
            login: login.into(),
            machine,
            launched,
        }
    }

    async fn next_launch(&mut self) -> GameLaunchParams {
        tokio::time::timeout(support::PATIENCE, self.launched.recv())
            .await
            .expect("the client started the game")
            .expect("the client is still there")
    }

    async fn host(&mut self) -> (GameLaunchParams, GameSide) {
        self.app
            .dispatch(
                LobbyCommand::Host {
                    config: host_config(),
                }
                .into(),
            )
            .await
            .unwrap();
        let (launch, mut game) = self.start_game_to_its_lobby().await;
        assert_eq!(game.next().await, message("HostGame", vec![text(MAP)]));
        (launch, game)
    }

    async fn join(&mut self, game_id: i32) -> (GameLaunchParams, GameSide) {
        support::until(&self.app, "the hosted game listed", |state| {
            state.lobby.games.iter().any(|game| game.id == game_id)
        })
        .await;
        self.app
            .dispatch(
                LobbyCommand::Join {
                    id: game_id,
                    password: None,
                    replace_mods: false,
                }
                .into(),
            )
            .await
            .unwrap();
        self.start_game_to_its_lobby().await
    }

    /// Start the game the client launched, and take it through `Idle` to its
    /// lobby, as the engine does. The client answers `Idle` with the ICE
    /// servers and `CreateLobby`.
    async fn start_game_to_its_lobby(&mut self) -> (GameLaunchParams, GameSide) {
        let launch = self.next_launch().await;
        assert_eq!(
            launch.network,
            GameNetwork::Native {
                force_relay: false,
                rejoin: None
            },
            "faf-re's game runs its own links"
        );
        let mut game = GameSide::start(&launch).await;
        game.send("GameState", vec![text("Idle")]).await;
        self.set_up(&mut game).await;
        game.send("GameState", vec![text("Lobby")]).await;
        (launch, game)
    }

    /// What a game is sent before anything else.
    async fn set_up(&self, game: &mut GameSide) {
        let servers = Value::Array(ice_servers()).to_string();
        assert_eq!(
            game.next().await,
            message("IceServers", vec![text(&servers)])
        );
        assert_eq!(
            game.next().await,
            message(
                "CreateLobby",
                vec![int(0), int(0), text(&self.login), int(self.id), int(1)]
            )
        );
    }
}

/// A game, as far as GPGNet goes: what `ForgedAlliance.exe` says on its
/// client's port and hears from it. The two directions are apart, as the
/// engine's are, so it can talk while it listens.
struct GameSide {
    writer: OwnedWriteHalf,
    hearing: Hearing,
}

/// The game's side of the connection that it reads.
struct Hearing {
    reader: OwnedReadHalf,
    buffer: Vec<u8>,
    arrived: VecDeque<GpgMessage>,
}

impl Hearing {
    async fn next(&mut self) -> GpgMessage {
        loop {
            if let Some(message) = self.arrived.pop_front() {
                return message;
            }
            let mut chunk = [0_u8; 4096];
            let read = tokio::time::timeout(support::PATIENCE, self.reader.read(&mut chunk))
                .await
                .expect("the game hears from its client")
                .unwrap();
            assert!(read > 0, "the client closed the game's connection");
            self.buffer.extend_from_slice(&chunk[..read]);
            let drained = drain(&mut self.buffer);
            assert_eq!(drained.invalid, None);
            self.arrived.extend(drained.messages);
        }
    }
}

impl GameSide {
    async fn start(launch: &GameLaunchParams) -> Self {
        let (reader, writer) = TcpStream::connect(("127.0.0.1", launch.game_port))
            .await
            .expect("the client's GPGNet port is open")
            .into_split();
        Self {
            writer,
            hearing: Hearing {
                reader,
                buffer: Vec::new(),
                arrived: VecDeque::new(),
            },
        }
    }

    async fn send(&mut self, command: &str, args: Vec<GpgArg>) {
        self.say(&[message(command, args)]).await;
    }

    async fn say(&mut self, messages: &[GpgMessage]) {
        let bytes: Vec<u8> = messages.iter().flat_map(encode).collect();
        self.writer.write_all(&bytes).await.unwrap();
    }

    async fn next(&mut self) -> GpgMessage {
        self.hearing.next().await
    }

    /// Say `messages` while hearing `count` from the client, at once.
    async fn say_while_hearing(
        &mut self,
        messages: &[GpgMessage],
        count: usize,
    ) -> Vec<GpgMessage> {
        let bytes: Vec<u8> = messages.iter().flat_map(encode).collect();
        let Self { writer, hearing } = self;
        let saying = async { writer.write_all(&bytes).await.unwrap() };
        let listening = async {
            let mut heard = Vec::with_capacity(count);
            while heard.len() < count {
                heard.push(hearing.next().await);
            }
            heard
        };
        tokio::join!(saying, listening).1
    }
}

/// Two players in one game, both games in their lobby and linked.
struct TwoInAGame {
    server: Arc<LobbyServer>,
    ada: Client,
    ada_game: GameSide,
    bert: Client,
    bert_game: GameSide,
}

async fn two_in_a_game() -> TwoInAGame {
    let server = Arc::new(LobbyServer::default());
    let mut ada = Client::sign_in(&server, 101, "Ada").await;
    let mut bert = Client::sign_in(&server, 102, "Bert").await;
    let (hosted, mut ada_game) = ada.host().await;
    let (_, mut bert_game) = bert.join(hosted.game_id).await;
    assert_eq!(
        bert_game.next().await,
        message("JoinGame", vec![text("Ada"), int(101)])
    );
    assert_eq!(
        ada_game.next().await,
        message("ConnectToPeer", vec![text("Bert"), int(102), int(1)])
    );
    TwoInAGame {
        server,
        ada,
        ada_game,
        bert,
        bert_game,
    }
}

/// Whether a game heard what was said to it, naming where it parts from
/// that rather than printing megabytes of candidates.
fn assert_heard(heard: &[GpgMessage], said: &[GpgMessage], who: &str) {
    assert_eq!(heard.len(), said.len(), "{who} heard as many as were said");
    let parted = heard
        .iter()
        .zip(said)
        .position(|(heard, said)| heard != said);
    assert_eq!(parted, None, "{who} heard another message here");
}

/// Three players in one game, every game in its lobby, every link announced.
struct ThreeInAGame {
    server: Arc<LobbyServer>,
    ada: Client,
    ada_game: GameSide,
    bert: Client,
    bert_game: GameSide,
    cleo: Client,
    cleo_launch: GameLaunchParams,
    cleo_game: GameSide,
}

async fn three_in_a_game() -> ThreeInAGame {
    let server = Arc::new(LobbyServer::default());
    let mut ada = Client::sign_in(&server, 101, "Ada").await;
    let mut bert = Client::sign_in(&server, 102, "Bert").await;
    let mut cleo = Client::sign_in(&server, 103, "Cleo").await;

    let (hosted, mut ada_game) = ada.host().await;
    let (_, mut bert_game) = bert.join(hosted.game_id).await;
    assert_eq!(
        bert_game.next().await,
        message("JoinGame", vec![text("Ada"), int(101)])
    );
    assert_eq!(
        ada_game.next().await,
        message("ConnectToPeer", vec![text("Bert"), int(102), int(1)])
    );

    let (cleo_launch, mut cleo_game) = cleo.join(hosted.game_id).await;
    assert_eq!(
        cleo_game.next().await,
        message("JoinGame", vec![text("Ada"), int(101)])
    );
    assert_eq!(
        cleo_game.next().await,
        message("ConnectToPeer", vec![text("Bert"), int(102), int(1)])
    );
    assert_eq!(
        ada_game.next().await,
        message("ConnectToPeer", vec![text("Cleo"), int(103), int(1)])
    );
    assert_eq!(
        bert_game.next().await,
        message("ConnectToPeer", vec![text("Cleo"), int(103), int(0)])
    );
    ThreeInAGame {
        server,
        ada,
        ada_game,
        bert,
        bert_game,
        cleo,
        cleo_launch,
        cleo_game,
    }
}

/// A game offers to come back as it finishes loading. The candidate it sends
/// next is a point by which its client has noted the offer: both travel the
/// one connection, in order.
async fn offer_a_return(
    game: &mut GameSide,
    source: &str,
    peer_game: &mut GameSide,
    peer: i32,
    own: i32,
) {
    game.send("RejoinOffer", vec![text(source), text("101=0,102=1,103=2")])
        .await;
    game.send("IceMsg", vec![int(peer), text("candidate after the offer")])
        .await;
    assert_eq!(
        peer_game.next().await,
        message("IceMsg", vec![int(own), text("candidate after the offer")])
    );
}

#[tokio::test]
async fn a_host_and_a_joiner_link_through_their_games_alone() {
    if adapter_named_by_the_environment() {
        return;
    }
    let server = Arc::new(LobbyServer::default());
    let mut ada = Client::sign_in(&server, 101, "Ada").await;
    let mut bert = Client::sign_in(&server, 102, "Bert").await;

    let (hosted, mut ada_game) = ada.host().await;
    let (_, mut bert_game) = bert.join(hosted.game_id).await;
    assert_eq!(
        bert_game.next().await,
        message("JoinGame", vec![text("Ada"), int(101)]),
        "the server's own form: the game links by player, not by address"
    );
    assert_eq!(
        ada_game.next().await,
        message("ConnectToPeer", vec![text("Bert"), int(102), int(1)]),
        "the offer goes too, as an int"
    );

    // Signalling, both ways, through the server alone.
    ada_game
        .send("IceMsg", vec![int(102), text(r#"{"type":"offer"}"#)])
        .await;
    assert_eq!(
        bert_game.next().await,
        message("IceMsg", vec![int(101), text(r#"{"type":"offer"}"#)])
    );
    let candidates = json!({"type": "candidates", "candidates": vec!["host"; 900]}).to_string();
    assert!(candidates.len() > 4 * 1024);
    bert_game
        .send("IceMsg", vec![int(101), text(&candidates)])
        .await;
    assert_eq!(
        ada_game.next().await,
        message("IceMsg", vec![int(102), text(&candidates)]),
        "a long one arrives whole"
    );
    assert!(
        server
            .received_from(102)
            .contains(&("IceMsg".into(), vec![json!(101), json!(candidates)])),
        "the server got the game's message as the game sent it"
    );
}

/// A game and the lobby say a lot to each other at once, as when a match's
/// links gather their candidates: all of it gets through, in order. The
/// client used to wait for room in the queue to the game in the same loop
/// that hears the game, while the game's connection waited for that loop to
/// hear it, and with both queues full neither moved again: what the lobby
/// said then piled up in front of the lobby connection itself.
///
/// On threads, as the client runs. The game's options, small messages that
/// its connection hands on many to a read, are what filled the queue from
/// the game at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bursts_both_ways_at_once_all_get_through() {
    if adapter_named_by_the_environment() {
        return;
    }
    let server = Arc::new(LobbyServer::default());
    let mut ada = Client::sign_in(&server, 101, "Ada").await;
    let (_, mut ada_game) = ada.host().await;
    const BURST: usize = 300;
    // A candidate list of a device with many addresses: 2.5 KB each.
    let candidates = |from: i32| -> Vec<String> {
        (0..BURST)
            .map(|n| format!("{from}:{n}:{}", "c".repeat(2_500)))
            .collect()
    };
    let (from_ada, from_bert) = (candidates(101), candidates(102));

    // Bert's candidates come from the server as fast as Ada's client takes
    // them, while her game says its options and its candidates to him.
    let lobby = {
        let (server, from_bert) = (server.clone(), from_bert.clone());
        tokio::spawn(async move {
            for candidate in from_bert {
                server
                    .tell_in_turn(101, "IceMsg", vec![json!(102), json!(candidate)])
                    .await;
            }
        })
    };
    let option = |n: usize| format!("Option{n}");
    let said: Vec<GpgMessage> = (0..BURST)
        .map(|n| message("GameOption", vec![text(&option(n)), int(n as i32)]))
        .chain(
            from_ada
                .iter()
                .map(|candidate| message("IceMsg", vec![int(102), text(candidate)])),
        )
        .collect();
    let heard = ada_game.say_while_hearing(&said, BURST).await;
    lobby.await.unwrap();
    let from_the_server: Vec<GpgMessage> = from_bert
        .iter()
        .map(|candidate| message("IceMsg", vec![int(102), text(candidate)]))
        .collect();
    assert_heard(&heard, &from_the_server, "Ada's game");

    support::eventually("the server heard all the game said", || {
        server.count_from(101, "IceMsg") == BURST
    })
    .await;
    let expected = (0..BURST)
        .map(|n| ("GameOption".to_string(), vec![json!(option(n)), json!(n)]))
        .chain(
            from_ada
                .iter()
                .map(|candidate| ("IceMsg".to_string(), vec![json!(102), json!(candidate)])),
        );
    assert!(
        server
            .received_from(101)
            .into_iter()
            .filter(|(command, _)| command == "GameOption" || command == "IceMsg")
            .eq(expected),
        "the server heard what the game said, in order"
    );
}

/// A game that stops reading its connection, hung, holds up nothing else:
/// its client's lobby connection goes on, and what the lobby says to the
/// game waits for it, all of it, in order.
#[tokio::test]
async fn a_game_that_stops_reading_holds_up_nothing() {
    if adapter_named_by_the_environment() {
        return;
    }
    let TwoInAGame {
        server,
        ada,
        mut ada_game,
        bert: _bert,
        mut bert_game,
    } = two_in_a_game().await;
    const COUNT: usize = 500;
    let candidate = "c".repeat(2_500);
    // Ada's game reads nothing while Bert's says all of this to it.
    bert_game
        .say(&vec![
            message("IceMsg", vec![int(101), text(&candidate)]);
            COUNT
        ])
        .await;
    support::eventually("the server passed every one on", || {
        server.count_from(102, "IceMsg") == COUNT
    })
    .await;
    // What the server says after them is handled: Ada's client took every
    // one of them off its lobby connection.
    server.settle(&ada).await;

    // Reading again, the game gets them all.
    let said = vec![message("IceMsg", vec![int(102), text(&candidate)]); COUNT];
    let mut heard = Vec::with_capacity(COUNT);
    for _ in 0..COUNT {
        heard.push(ada_game.next().await);
    }
    assert_heard(&heard, &said, "Ada's game");
}

/// A datagram on the game's lobby port reaches its client as
/// `ProcessNatPacket [address, data]`. The game's own links have no use for
/// it, nor has the server, and JSON has no type for its bytes: it goes no
/// further, and the game's connection goes on as before.
#[tokio::test]
async fn a_game_message_that_carries_data_goes_no_further() {
    if adapter_named_by_the_environment() {
        return;
    }
    let server = Arc::new(LobbyServer::default());
    let mut ada = Client::sign_in(&server, 101, "Ada").await;
    let (_, mut ada_game) = ada.host().await;

    ada_game
        .send(
            "ProcessNatPacket",
            vec![text("127.0.0.1:6000"), GpgArg::Data(vec![0x08, 0x00, 0xff])],
        )
        .await;
    ada_game.send("GameState", vec![text("Launching")]).await;
    support::eventually("the game state after it reached the server", || {
        server
            .received_from(101)
            .contains(&("GameState".into(), vec![json!("Launching")]))
    })
    .await;
    assert_eq!(server.count_from(101, "ProcessNatPacket"), 0);

    server.tell(101, "IceMsg", vec![json!(102), json!("still connected")]);
    assert_eq!(
        ada_game.next().await,
        message("IceMsg", vec![int(102), text("still connected")])
    );
}

#[tokio::test]
async fn a_game_that_crashes_in_its_match_comes_back_to_it() {
    if adapter_named_by_the_environment() {
        return;
    }
    // Every client is held to the end: each one's lobby connection is a
    // player still in the match.
    let ThreeInAGame {
        server,
        ada: _ada,
        mut ada_game,
        bert: _bert,
        bert_game: _bert_game,
        mut cleo,
        cleo_launch,
        mut cleo_game,
    } = three_in_a_game().await;
    offer_a_return(&mut cleo_game, "2", &mut ada_game, 101, 103).await;

    // The crash: the game's connection dies with its process.
    drop(cleo_game);
    cleo.machine.end(GameExit::Crashed);
    let back = cleo.next_launch().await;
    assert_eq!(
        back.network,
        GameNetwork::Native {
            force_relay: false,
            rejoin: Some(Rejoin {
                seats: vec![seat(101, 0), seat(102, 1)],
                source: 2,
            }),
        },
        "the players still in the match, and its own source"
    );
    assert_eq!(back.game_port, cleo_launch.game_port, "the same port");
    assert_eq!(back.args, cleo_launch.args, "its usual arguments");

    // While no game is connected, what a peer's links say to it is dropped:
    // they try again.
    ada_game
        .send("IceMsg", vec![int(103), text("for the crashed game")])
        .await;
    support::eventually("the server passed it on", || {
        server
            .received_from(101)
            .iter()
            .any(|(_, args)| args == &[json!(103), json!("for the crashed game")])
    })
    .await;
    server.settle(&cleo).await;

    // Coming back, it reports no lobby state, so it is sent everything at
    // once: the servers, the lobby, then what the server said in the order
    // it said it.
    let mut cleo_back = GameSide::start(&back).await;
    cleo.set_up(&mut cleo_back).await;
    assert_eq!(
        cleo_back.next().await,
        message("JoinGame", vec![text("Ada"), int(101)])
    );
    assert_eq!(
        cleo_back.next().await,
        message("ConnectToPeer", vec![text("Bert"), int(102), int(1)])
    );

    ada_game
        .send("IceMsg", vec![int(103), text("to the game that came back")])
        .await;
    assert_eq!(
        cleo_back.next().await,
        message("IceMsg", vec![int(101), text("to the game that came back")]),
        "and nothing meant for the game that crashed"
    );
    cleo_back
        .send("IceMsg", vec![int(101), text("back again")])
        .await;
    assert_eq!(
        ada_game.next().await,
        message("IceMsg", vec![int(103), text("back again")])
    );

    // It never left the match.
    assert!(!server.said_its_game_ended(103));
    assert_eq!(cleo.app.snapshot().lobby.join, JoinState::InGame);
}

#[tokio::test]
async fn a_player_who_left_is_forgotten_and_a_game_quit_stays_quit() {
    if adapter_named_by_the_environment() {
        return;
    }
    let ThreeInAGame {
        server,
        ada: _ada,
        mut ada_game,
        mut bert,
        mut bert_game,
        mut cleo,
        cleo_launch: _,
        mut cleo_game,
    } = three_in_a_game().await;
    offer_a_return(&mut bert_game, "1", &mut ada_game, 101, 102).await;
    offer_a_return(&mut cleo_game, "2", &mut ada_game, 101, 103).await;

    // Bert's game quits, as a player's does: he left, so it is not started
    // again, and the server hears the game ended.
    drop(bert_game);
    bert.machine.end(GameExit::Quit);
    assert_eq!(
        ada_game.next().await,
        message("DisconnectFromPeer", vec![int(102)])
    );
    assert_eq!(
        cleo_game.next().await,
        message("DisconnectFromPeer", vec![int(102)])
    );
    assert!(server.said_its_game_ended(102));
    assert!(bert.launched.try_recv().is_err(), "a game quit stays quit");

    // Cleo's crashes afterwards: she comes back to Ada alone.
    drop(cleo_game);
    cleo.machine.end(GameExit::Crashed);
    let back = cleo.next_launch().await;
    assert_eq!(
        back.network,
        GameNetwork::Native {
            force_relay: false,
            rejoin: Some(Rejoin {
                seats: vec![seat(101, 0)],
                source: 2,
            }),
        }
    );
    let mut cleo_back = GameSide::start(&back).await;
    cleo.set_up(&mut cleo_back).await;
    assert_eq!(
        cleo_back.next().await,
        message("JoinGame", vec![text("Ada"), int(101)])
    );
    ada_game.send("IceMsg", vec![int(103), text("hello")]).await;
    assert_eq!(
        cleo_back.next().await,
        message("IceMsg", vec![int(101), text("hello")]),
        "no ConnectToPeer for the player who left"
    );
}

#[tokio::test]
async fn neither_an_ended_match_nor_a_stopped_game_comes_back() {
    if adapter_named_by_the_environment() {
        return;
    }
    let server = Arc::new(LobbyServer::default());
    let mut ada = Client::sign_in(&server, 101, "Ada").await;
    let mut bert = Client::sign_in(&server, 102, "Bert").await;
    let (hosted, mut ada_game) = ada.host().await;
    let (_, mut bert_game) = bert.join(hosted.game_id).await;
    assert_eq!(bert_game.next().await.command, "JoinGame");
    assert_eq!(ada_game.next().await.command, "ConnectToPeer");
    offer_a_return(&mut ada_game, "0", &mut bert_game, 102, 101).await;
    offer_a_return(&mut bert_game, "1", &mut ada_game, 101, 102).await;

    // The match ended, then Bert's game crashed on the score screen.
    bert_game.send("GameEnded", Vec::new()).await;
    support::eventually("the end reached the server", || {
        server
            .received_from(102)
            .iter()
            .any(|(command, _)| command == "GameEnded")
    })
    .await;
    drop(bert_game);
    bert.machine.end(GameExit::Crashed);
    support::eventually("Bert's client let the game go", || {
        server.said_its_game_ended(102)
    })
    .await;
    assert!(bert.launched.try_recv().is_err());

    // Ada stops her game herself.
    ada.app
        .dispatch(LobbyCommand::TerminateGame.into())
        .await
        .unwrap();
    support::eventually("Ada's client let the game go", || {
        server.said_its_game_ended(101)
    })
    .await;
    assert!(ada.launched.try_recv().is_err());
    drop(ada_game);
}

/// An offer the game app would refuse to come back with, a source past 255,
/// brings no return: the crash ends the game, as it does without an offer.
#[tokio::test]
async fn an_offer_out_of_what_a_return_takes_brings_none() {
    if adapter_named_by_the_environment() {
        return;
    }
    let TwoInAGame {
        server,
        ada: _ada,
        mut ada_game,
        mut bert,
        mut bert_game,
    } = two_in_a_game().await;
    offer_a_return(&mut bert_game, "256", &mut ada_game, 101, 102).await;

    drop(bert_game);
    bert.machine.end(GameExit::Crashed);
    support::eventually("Bert's client let the game go", || {
        server.said_its_game_ended(102)
    })
    .await;
    assert!(bert.launched.try_recv().is_err(), "not started again");
}

/// What the server says while the game is down is what the game comes back
/// to: a player who left meanwhile is not linked to again, and one the
/// server named meanwhile is.
#[tokio::test]
async fn what_the_server_says_while_the_game_is_down_is_what_it_comes_back_to() {
    if adapter_named_by_the_environment() {
        return;
    }
    let ThreeInAGame {
        server,
        ada: _ada,
        mut ada_game,
        bert,
        bert_game,
        mut cleo,
        cleo_launch: _,
        mut cleo_game,
    } = three_in_a_game().await;
    offer_a_return(&mut cleo_game, "2", &mut ada_game, 101, 103).await;
    drop(cleo_game);
    cleo.machine.end(GameExit::Crashed);
    let back = cleo.next_launch().await;

    // Meanwhile Bert quits, and the server links Cleo to a player new to her.
    drop(bert_game);
    bert.machine.end(GameExit::Quit);
    assert_eq!(
        ada_game.next().await,
        message("DisconnectFromPeer", vec![int(102)])
    );
    server.tell(
        103,
        "ConnectToPeer",
        vec![json!("Dora"), json!(104), json!(true)],
    );
    server.settle(&cleo).await;

    let mut cleo_back = GameSide::start(&back).await;
    cleo.set_up(&mut cleo_back).await;
    assert_eq!(
        cleo_back.next().await,
        message("JoinGame", vec![text("Ada"), int(101)])
    );
    assert_eq!(
        cleo_back.next().await,
        message("ConnectToPeer", vec![text("Dora"), int(104), int(1)])
    );
    ada_game.send("IceMsg", vec![int(103), text("hello")]).await;
    assert_eq!(
        cleo_back.next().await,
        message("IceMsg", vec![int(101), text("hello")]),
        "and no link to Bert"
    );
}

/// A game that keeps crashing on its way back is let go after three returns,
/// as if it had not offered one: the next crash ends it.
#[tokio::test]
async fn a_game_that_keeps_crashing_is_let_go_after_three_returns() {
    if adapter_named_by_the_environment() {
        return;
    }
    let TwoInAGame {
        server,
        ada: _ada,
        mut ada_game,
        mut bert,
        mut bert_game,
    } = two_in_a_game().await;
    offer_a_return(&mut bert_game, "1", &mut ada_game, 101, 102).await;
    drop(bert_game);

    for _ in 0..3 {
        bert.machine.end(GameExit::Crashed);
        let back = bert.next_launch().await;
        assert!(
            matches!(
                back.network,
                GameNetwork::Native {
                    rejoin: Some(_),
                    ..
                }
            ),
            "{back:?}"
        );
    }
    bert.machine.end(GameExit::Crashed);
    support::eventually("Bert's client let the game go", || {
        server.said_its_game_ended(102)
    })
    .await;
    assert!(bert.launched.try_recv().is_err(), "no fourth return");
}

#[tokio::test]
async fn turned_off_faf_res_game_goes_to_the_adapter_and_still_takes_players_back() {
    if adapter_named_by_the_environment() {
        return;
    }
    let server = Arc::new(LobbyServer::default());
    let mut ada = Client::sign_in(&server, 101, "Ada").await;
    ada.app
        .dispatch(
            SettingsCommand::PatchConnectivity {
                patch: ConnectivityPreferencesPatch {
                    native_network: Some(false),
                    ..ConnectivityPreferencesPatch::default()
                },
            }
            .into(),
        )
        .await
        .unwrap();
    support::until(&ada.app, "the setting stored", |state| {
        !state.settings.connectivity.native_network
    })
    .await;

    ada.app
        .dispatch(
            LobbyCommand::Host {
                config: host_config(),
            }
            .into(),
        )
        .await
        .unwrap();
    assert_eq!(
        ada.next_launch().await.network,
        GameNetwork::Adapter {
            rebuilt_engine: true
        },
        "on the adapter, and told it takes a crashed player back"
    );
}
