//! Live spectating: relaying a game in progress from the replay server to FA.
//!
//! Separate from file playback because nothing here reads a replay file: the
//! stream is fetched, gated on its first frame and relayed byte for byte over
//! a local transport, and which transport is its own subject.
//!
//! ## Live-watch protocol
//! 1. `GET {user_api}/replay/access` (bearer token) → `{ accessUrl }` (same
//!    endpoint and response shape as the lobby's `/lobby/access`, see
//!    `infra::fetch_access_url`).
//! 2. Open a WebSocket to `accessUrl`; send one binary frame
//!    `G/{uid}/{player}.scfareplay\0` (the player name is irrelevant to the
//!    server, which merges every participant's stream: any non-empty string
//!    works, per the Python client's own comment).
//! 3. Wait for the first binary frame back before launching FA: otherwise FA
//!    connects to an empty stream (same ordering as the Python client).
//! 4. Bring the replay install up to the current build of the game's featured
//!    mod. A live stream carries no version of its own, and FA refuses a
//!    replay whose build does not match what is installed ("Ack! Unable to
//!    load replay from gpgnet"), so this asks for `latest` the way a live
//!    game launch does. It runs *before* the stream is opened: the server
//!    starts sending on the handshake, and a patch download after that point
//!    would be spent buffering frames nobody is reading. FAF's game app
//!    (Android) keeps its own files and is told that version instead.
//! 5. Launch FA with `/replay gpgnet://127.0.0.1:<port>/...` pointed at a
//!    local TCP proxy (mirrors the Java client's `LiveReplayProxyServer`).
//!    The game app is also told the game's mods with their files, read from
//!    the header the stream opens with (`read_stream_mods`).
//! 6. Once FA connects, feed it WebSocket frames until the connection closes.
//!
//! **Transport: a local TCP proxy by default; a named pipe only when the user
//! asks for it.** FA's engine has a confirmed bug: reading a replay's
//! `ScenarioInfo` (game options, often bloated by unused sim-mod options)
//! through a raw TCP/`gpgnet://` socket crashes it with "Premature EOF" once
//! that data exceeds roughly 2047 bytes (FAF Discord/Zulip, Gatsik, Jan 2026).
//! A Windows named pipe avoids that crash: we shipped that fix briefly: but it
//! introduced a worse regression: FA's named-pipe read is a *blocking* call on
//! (what appears to be) its main/render thread, so catching up to a live
//! game's current tick freezes the entire UI rather than just stalling the
//! simulation the way the TCP path does (confirmed both by community reports
//!: Nomander/Nuggets, FAF Discord, Jan 2026: and by reproducing it
//! ourselves). It also ends the replay abruptly, with no army selection or
//! post-game statistics, because FA never sees the stream close (see
//! [`PIPE_TERMINATOR`]). The actual fix for the root cause is upstream, in the
//! FA engine's lobby code (`FAForever/fa#7057`, stripping disabled mods' game
//! options before launch so `ScenarioInfo` never gets oversized in the first
//! place); once that lands, this TCP transport stops hitting the crash for
//! new games entirely, with none of the pipe's freeze downside.
//!
//! Neither transport is right for everyone until then, which is why the pipe
//! is a switch rather than a decision made here: Settings, "Live replays
//! workaround" (`GamePreferences::pipe_live_replay`, the Python client's
//! `game/pipe_live_replay`), off by default. A player whose games crash out of
//! live spectating turns it on and trades the ending for one that starts.
//!
//! Old replays recorded before the upstream fix may still have oversized
//! `ScenarioInfo` and could in principle still hit this crash live-spectating
//! them: but local *file* playback (see below) was never on this code path to
//! begin with: FA reads those directly from disk via `/replay "<path>"`, with
//! no TCP or pipe involved and no size limit or freeze concern either way.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use faf_domain::state::LiveReplayTarget;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, Stream, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;

use crate::infra::session::TokenStore;
use crate::infra::{fetch_access_url, validated_ws_url};
use crate::ports::game_app::{
    contract_map_name, contract_replay_name, map_refusal, GameAppPort, GameAppRun, GameAppStart,
};
use crate::ports::ProcessPort;

use super::names::normalize_mod;
use super::playback::GameAppContent;
use super::preparation::ReplayPreparation;
use super::scfa_header::stream_sim_mods;
use super::ReplayConfig;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long to wait for the replay server's first byte, or for FA to connect
/// to the local TCP proxy, before giving up.
const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// The same for FAF's game app (Android), which fetches a map it lacks
/// before its game loads, trying again for about ten minutes when the
/// network drops.
const GAME_APP_READY_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// How much of a live stream is read before the game app starts, for the
/// mods its header names. The mod table sits behind two short strings, so
/// even a game with dozens of mods has it far inside this.
const STREAM_HEAD_LIMIT: usize = 1024 * 1024;

/// The pipe FA is pointed at when the "Live Replays Workaround" is on. Fixed
/// rather than per-game, and the same name the Python client uses, so the two
/// clients cannot end up streaming into each other's pipe: only one live
/// replay can run at a time either way (the playback lock).
#[cfg(windows)]
const LIVE_REPLAY_PIPE_PATH: &str = r"\\.\pipe\fafreplay";

/// How often a pipe session checks whether the replay window is still open.
#[cfg(windows)]
const REPLAY_EXIT_POLL: Duration = Duration::from_millis(500);

/// How long ending a session waits for the replay server to take the close.
#[cfg(windows)]
const WS_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// How a live replay stream reaches FA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveTransport {
    /// A local TCP proxy behind a `gpgnet://` URL. The default: FA ends the
    /// replay cleanly and the spectator keeps army selection and statistics.
    Tcp,
    /// A Windows named pipe. Sidesteps the engine's oversized-`ScenarioInfo`
    /// bug on the TCP path at the cost of a frozen window while catching up
    /// and an abrupt end. See the module docs.
    #[cfg(windows)]
    Pipe,
}

/// FA does not notice a named pipe closing the way it notices a socket close:
/// it simply hangs. The Python client found that a couple of kilobytes of
/// invalid replay data ends the session instead, and this is the same
/// terminator, byte for byte (`fa/replaylivestreamer.py`). The simulation
/// still stops rather than ending properly, which is the "abrupt end" the
/// setting's hint warns about.
#[cfg(windows)]
const PIPE_TERMINATOR: [u8; 2048] = [0u8; 2048];

/// How much of the stream may wait for FA to read it before the session gives
/// up.
///
/// The queue between the WebSocket and FA exists so the socket keeps being
/// read (and its keepalive pings answered) while FA is busy loading, and that
/// has to be a queue FA can fall well behind: joining a game in progress
/// delivers everything played so far at once. What it must not be is
/// unbounded, because a game that stops reading for good would otherwise grow
/// it for as long as the game is broadcast. A whole recorded game is tens of
/// megabytes, so this is several of them.
const MAX_RELAY_BACKLOG: usize = 256 * 1024 * 1024;

/// The bytes queued for FA and not yet written to it.
#[derive(Clone, Default)]
struct Backlog(Arc<AtomicUsize>);

impl Backlog {
    /// Count `bytes` in, or say the backlog is full.
    fn admit(&self, bytes: usize) -> bool {
        let before = self.0.fetch_add(bytes, Ordering::Relaxed);
        if before.saturating_add(bytes) > MAX_RELAY_BACKLOG {
            tracing::warn!(
                queued = before,
                "the game stopped reading the live replay; ending the session"
            );
            return false;
        }
        true
    }

    fn written(&self, bytes: usize) {
        self.0.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// Relays a live game from the replay server to FA.
///
/// Holds what only live spectating needs: the user API that hands out replay
/// server access and the transport preference. The install it prepares and
/// the process it launches belong to playback, which passes them in while it
/// holds the one-launch-at-a-time lock.
pub(super) struct LiveStreamer {
    http: reqwest::Client,
    tokens: TokenStore,
    /// FAF *user* API base, which serves `/replay/access`.
    user_api_base: String,
    /// Whether a live replay stream is handed to FA over a named pipe instead
    /// of the local TCP proxy: the "Live Replays Workaround" setting, pushed in
    /// by the settings service (`ReplayPlaybackPort::set_live_replay_pipe`).
    pipe_live_replay: std::sync::atomic::AtomicBool,
    /// The pipe relay still running, if any. It owns the server end of
    /// [`LIVE_REPLAY_PIPE_PATH`], so the next live replay cannot create the
    /// pipe until this session has ended: see [`Self::end_session`].
    session: std::sync::Mutex<Option<RelaySession>>,
}

/// A running relay, and the way to end it from outside.
// Only the pipe transport registers one, and that exists on Windows only.
#[cfg_attr(not(windows), allow(dead_code))]
struct RelaySession {
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl LiveStreamer {
    pub(super) fn new(config: &ReplayConfig, tokens: TokenStore) -> Self {
        Self {
            http: crate::infra::http::shared_http_client(),
            tokens,
            user_api_base: config.user_api_base.clone(),
            pipe_live_replay: std::sync::atomic::AtomicBool::new(false),
            session: std::sync::Mutex::new(None),
        }
    }

    /// End the previous live replay's relay, and wait until it has let go of
    /// the pipe.
    ///
    /// Stopping the game is not enough for that. FA is the pipe's *client*;
    /// the server end belongs to the relay, which only noticed FA leaving when
    /// a write failed. A broadcast that went quiet without closing therefore
    /// kept the pipe open for as long as the client ran, and every live replay
    /// after it failed with "All pipe instances are busy" (os error 231).
    pub(super) async fn end_session(&self) {
        let Some(session) = self.session.lock().unwrap().take() else {
            return;
        };
        session.stop.cancel();
        let _ = session.task.await;
    }

    pub(super) fn set_pipe(&self, enabled: bool) {
        self.pipe_live_replay
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Which transport the next live replay uses, resolving the preference
    /// against what this platform can actually do.
    fn live_transport(&self) -> LiveTransport {
        if !self
            .pipe_live_replay
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return LiveTransport::Tcp;
        }
        #[cfg(windows)]
        {
            LiveTransport::Pipe
        }
        #[cfg(not(windows))]
        {
            // The Python client has a FIFO path for this; we do not, and
            // quietly ignoring the setting would be worse than saying so.
            tracing::warn!(
                "the live replay workaround uses a Windows named pipe; \
                 falling back to the TCP proxy on this platform"
            );
            LiveTransport::Tcp
        }
    }

    /// The body of `ReplayPlaybackPort::watch_live`, run while the caller
    /// holds the playback lock. `content` is what the game app is told where
    /// the game is an app of its own, in place of `preparation`.
    pub(super) async fn start(
        &self,
        preparation: &ReplayPreparation,
        process: &Arc<dyn ProcessPort>,
        content: &GameAppContent,
        target: LiveReplayTarget,
        player: String,
    ) -> Result<Option<String>, String> {
        // Before anything is stopped or opened: a game on a map the game app
        // cannot have, a generated one, is refused as a replay file's is.
        if crate::infra::game_app::game_app().is_some() {
            if let Some(refusal) = map_refusal(&target.map) {
                return Err(refusal.to_owned());
            }
        }
        // Same reason as `play_file`, plus one of its own on Windows: the live
        // transport's named pipe is created with `first_pipe_instance`, and
        // the previous live replay's relay may still hold it after its game
        // has gone.
        // Before the stop, for the reason given in `launch_replay_file`.
        let _replacement = super::ReplacementGuard(Arc::clone(process));
        process.stop_replay().await;
        self.end_session().await;
        let token = self
            .tokens
            .get()
            .ok_or_else(|| "not logged in".to_string())?;

        let mod_name = normalize_mod(&target.mod_name);
        // Where the game is an app of its own (Android), it keeps its own
        // files and fetches the map itself: nothing is prepared here.
        let game_app = crate::infra::game_app::game_app();

        // Preparation runs before the stream is opened, deliberately. The
        // replay server starts sending the moment the handshake lands, and FA
        // is launched on the first frame, so a multi-minute patch or map
        // download between those two points would be spent buffering a live
        // stream nobody is reading yet.
        let mut warning = None;
        match preparation.install_dir().filter(|_| game_app.is_none()) {
            Some(target_dir) => {
                // The gap behind "Ack! Unable to load replay from gpgnet".
                // Live spectating never updated the install it launches, so
                // the replay install stayed on whatever build was last staged
                // into it: in practice some old replay's version, since that
                // path pins an exact one and nothing else ever touches this
                // directory. FA refuses a replay whose build does not match,
                // and a live stream is always the *current* build. `play_file`
                // has resolved its replay's exact version since it was
                // written; this is the same step for a stream that carries no
                // version of its own to read, so it asks for `latest` the way
                // a live game launch does.
                //
                // Fatal for the same reason it is fatal in `play_file`:
                // launching anyway just reproduces the refusal, with the user
                // dropped on the main menu and no diagnostic anywhere.
                preparation
                    .update_to_latest(&token, &target_dir, &mod_name)
                    .await
                    .map_err(|e| format!("could not prepare the game for live spectating: {e}"))?;

                // Same gap `play_file` had before it got `ensure_map_available`:
                // live-spectating never staged the game's map at all, so a custom
                // (non-base) map FA can't already find leaves it stuck the same
                // way: confirmed live (`map /maps/hoey.v0002/Hoey.scmap failed.
                // aborting session.`), silently dumping the user back to the main
                // menu with no crash dialog and no error surfaced here either.
                // `target.map` is the FAF technical map name (e.g. `hoey.v0002`),
                // the same shape `ensure_map_available` expects.
                if let Some(warn) = preparation
                    .ensure_replay_map(&target_dir, &target.map)
                    .await?
                {
                    warning = Some(warn);
                }
            }
            None if game_app.is_some() => {}
            None => {
                tracing::warn!(
                    "no replay install is configured, so the engine version and map \
                     were not prepared; FA may refuse to load this live replay"
                );
            }
        }
        // The game app's counterpart of the update above, and asked before
        // the stream opens for the same reason. Fatal for the same reason
        // too: the newest version the game app has is not the one a live
        // game is on once FAF has moved on, and it refuses the stream then.
        let game_version = match &game_app {
            Some(_) => content.live_version(&mod_name).await?,
            None => None,
        };

        let transport = self.live_transport();

        let access_url =
            fetch_access_url(&self.http, &self.user_api_base, "/replay/access", &token).await?;
        let ws_url = validated_ws_url(&access_url)?;

        // Note: ws_url carries a one-time verify token: never log it verbatim.
        let (ws, _) = tokio_tungstenite::connect_async(ws_url.as_str())
            .await
            .map_err(|e| format!("could not open replay websocket: {e}"))?;
        let (mut write, mut read) = ws.split();

        let handshake = format!("G/{}/{}.scfareplay\0", target.uid, player).into_bytes();
        write
            .send(Message::Binary(handshake))
            .await
            .map_err(|e| format!("replay handshake failed: {e}"))?;

        // Gate FA's launch on the first byte actually arriving, mirroring the
        // Python client: otherwise FA connects to a proxy with nothing behind it.
        let mut first = wait_for_first_binary(&mut read).await?;

        let log_path = crate::infra::game_logs::next_path("live-replay", Some(target.uid))?;
        // Spelled the way the launched game can open it, which is not the same
        // string when the game runs through Wine: see
        // `ProcessPort::game_argument_path`. The source is a URL on both
        // transports here and needs no such treatment.
        let log_argument = process.game_argument_path(&log_path);
        // Everything but the replay source is the same on both transports.
        let launch_args = |source: String| {
            vec![
                "/replay".to_string(),
                source,
                "/init".to_string(),
                format!("init_{mod_name}.lua"),
                "/nobugreport".to_string(),
                "/log".to_string(),
                log_argument.clone(),
                "/replayid".to_string(),
                target.uid.to_string(),
            ]
        };

        match transport {
            LiveTransport::Tcp => {
                // Bound straight to port 0 and asked which one it got, rather
                // than reserving a free port and binding it again: in between,
                // anything else could take it.
                let listener = TcpListener::bind(("127.0.0.1", 0))
                    .await
                    .map_err(|e| format!("could not bind local replay proxy: {e}"))?;
                let port = listener
                    .local_addr()
                    .map_err(|e| format!("could not bind local replay proxy: {e}"))?
                    .port();

                let url = |name: &str| {
                    format!("gpgnet://127.0.0.1:{port}/{}/{name}.SCFAreplay", target.uid)
                };
                // The game app's game, served for as long as the relay runs:
                // the client keeps running in the back while it is waited for.
                let mut served = None;
                let accepted = match &game_app {
                    // The game app may fetch the map first, so it connects
                    // once its game has loaded, or it gives up.
                    Some(game_app) => {
                        let stream_mods = read_stream_mods(&mut first, &mut read).await;
                        let (run, mods_warning) = start_live_in_game_app(
                            game_app.as_ref(),
                            content,
                            game_version,
                            &mod_name,
                            &target.map,
                            vec!["/replay".to_owned(), url(&contract_replay_name(&player))],
                            stream_mods,
                        )
                        .await?;
                        if mods_warning.is_some() {
                            warning = mods_warning;
                        }
                        let run = served.insert(run);
                        tokio::select! {
                            accepted = tokio::time::timeout(GAME_APP_READY_TIMEOUT, listener.accept()) => accepted,
                            ended = &mut run.ended => {
                                return Err(match ended {
                                    Ok(Err(reason)) => reason,
                                    _ => "the game app ended before it connected".to_owned(),
                                });
                            }
                        }
                    }
                    None => {
                        process.launch_replay(launch_args(url(&player))).await?;
                        tokio::time::timeout(READY_TIMEOUT, listener.accept()).await
                    }
                };
                let (stream, _) = accepted
                    .map_err(|_| "timed out waiting for the game to connect".to_string())?
                    .map_err(|e| format!("accept failed: {e}"))?;

                tokio::spawn(async move {
                    relay_session(stream, write, read, first).await;
                    // Served to its end: the game app's game needs the client
                    // no more, which stops waiting for it.
                    drop(served);
                });
            }
            #[cfg(windows)]
            LiveTransport::Pipe => {
                // Created before FA is launched: a client opening a pipe that
                // does not exist yet fails outright, where a TCP connect to a
                // bound listener simply waits.
                let pipe = tokio::net::windows::named_pipe::ServerOptions::new()
                    .first_pipe_instance(true)
                    .max_instances(1)
                    .access_outbound(true)
                    .access_inbound(false)
                    .create(LIVE_REPLAY_PIPE_PATH)
                    .map_err(|e| {
                        format!("could not open the live replay pipe: {e} (is another live replay still running?)")
                    })?;

                process
                    .launch_replay(launch_args(LIVE_REPLAY_PIPE_PATH.to_string()))
                    .await?;

                tokio::time::timeout(READY_TIMEOUT, pipe.connect())
                    .await
                    .map_err(|_| "timed out waiting for the game to connect".to_string())?
                    .map_err(|e| format!("the game could not open the replay pipe: {e}"))?;

                let stop = CancellationToken::new();
                let ended = replay_ended(Arc::clone(process), stop.clone());
                let task = tokio::spawn(pipe_relay_session(pipe, write, read, first, ended));
                *self.session.lock().unwrap() = Some(RelaySession { stop, task });
            }
        }
        Ok(warning)
    }
}

/// Wait for the first binary WebSocket frame, ignoring ping/pong/text frames.
async fn wait_for_first_binary(read: &mut SplitStream<WsStream>) -> Result<Vec<u8>, String> {
    match tokio::time::timeout(READY_TIMEOUT, next_binary(read)).await {
        Ok(Some(data)) => Ok(data),
        Ok(None) => Err("replay server closed the connection before sending data".to_string()),
        Err(_) => Err("timed out waiting for the replay server".to_string()),
    }
}

/// The next binary frame, past pings, pongs and text. `None` once the
/// stream fails or ends.
async fn next_binary<S>(read: &mut S) -> Option<Vec<u8>>
where
    S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match read.next().await {
            Some(Ok(Message::Binary(data))) => return Some(data),
            Some(Ok(_)) => continue,
            Some(Err(_)) | None => return None,
        }
    }
}

/// The simulation mods a live game runs with, `(uid, name)`, from the
/// header its stream opens with.
///
/// The game app fetches a game's mods by their uids, and the stream is the
/// one place every live game names them: FAF's API lists no game's mods, so
/// a game picked from its list arrives with none. Reads on from `head`, the
/// frames so far, adding each frame it reads to it, so the relay still sends
/// them first. `None` when the header has not said within
/// [`STREAM_HEAD_LIMIT`] bytes or [`READY_TIMEOUT`], or the stream ended.
async fn read_stream_mods<S>(head: &mut Vec<u8>, read: &mut S) -> Option<Vec<(String, String)>>
where
    S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if let Some(mods) = stream_sim_mods(head) {
            return Some(mods);
        }
        if head.len() >= STREAM_HEAD_LIMIT {
            return None;
        }
        match tokio::time::timeout_at(deadline, next_binary(read)).await {
            Ok(Some(frame)) => head.extend_from_slice(&frame),
            Ok(None) | Err(_) => return None,
        }
    }
}

/// Start a live game of `mod_name` on `map` in the game app, pointed at the
/// local proxy by `arguments`: on `game_version` (`GameAppContent::live_version`)
/// and with `stream_mods`, the mods its stream names, each with its file. The
/// warning says what the game app plays it without.
async fn start_live_in_game_app(
    game_app: &dyn GameAppPort,
    content: &GameAppContent,
    game_version: Option<i32>,
    mod_name: &str,
    map: &str,
    arguments: Vec<String>,
    stream_mods: Option<Vec<(String, String)>>,
) -> Result<(GameAppRun, Option<String>), String> {
    let (mods, warning) = match &stream_mods {
        Some(sim_mods) => content.mods(sim_mods).await,
        None => (
            Vec::new(),
            Some(
                "the game's mods could not be read from its stream, so the game app plays it \
                 without any"
                    .to_owned(),
            ),
        ),
    };
    let start = GameAppStart {
        game_version,
        featured_mod: Some(mod_name.to_owned()),
        arguments,
        map: contract_map_name(map),
        mods,
        ..GameAppStart::default()
    };
    // As `playback::start_in_game_app` logs a replay file's.
    tracing::info!(
        game_version = ?start.game_version,
        featured_mod = mod_name,
        map = ?start.map,
        mods = start.mods.len(),
        mods_read = stream_mods.is_some(),
        "handing the live game to the game app"
    );
    let run = game_app.start(start).await?;
    Ok((run, warning))
}

/// Bidirectionally pipe bytes between FA's local proxy socket and the replay
/// WebSocket (mirrors the Java client's `LiveReplayProxyServer`), starting
/// with the already-buffered first frame.
///
/// The WebSocket side runs in its own task, decoupled from the TCP write via
/// an mpsc queue: mirroring the Python client's `StreamWriter`, which pushes
/// server bytes onto a `Queue` from the Qt event loop and drains it on a
/// separate writer thread. FA is single-threaded and doesn't read from the
/// proxy socket while it's busy loading assets (tens of seconds at startup);
/// without this decoupling, `tcp.write_all` blocking on that meant we also
/// stopped polling the WebSocket, missed its keepalive pings, and the server
/// dropped the connection: the game then hit "Premature EOF" once it finally
/// got around to reading. TCP→WS (the game talking back) doesn't have the
/// same problem since replay playback is receive-only, but it's decoupled the
/// same way for symmetry and to keep this task cheap to reason about.
async fn relay_session(
    tcp: TcpStream,
    mut ws_write: SplitSink<WsStream, Message>,
    mut ws_read: SplitStream<WsStream>,
    first: Vec<u8>,
) {
    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let backlog = Backlog::default();
    if !backlog.admit(first.len()) || tx.send(first).is_err() {
        return;
    }

    // WS → queue. Never blocks on the (possibly slow-to-drain) TCP side, so
    // the server's keepalive pings always get answered promptly. Bounded by
    // bytes rather than by waiting, for the same reason: see
    // [`MAX_RELAY_BACKLOG`].
    let queued = backlog.clone();
    let mut ws_to_queue = tokio::spawn(async move {
        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(Message::Binary(data)) => {
                    if !queued.admit(data.len()) || tx.send(data).is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {} // ping/pong/text: ignore
            }
        }
    });

    // Queue → TCP. Can block on FA being busy without affecting the above.
    let mut queue_to_tcp = tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            if tcp_write.write_all(&data).await.is_err() {
                break;
            }
            backlog.written(data.len());
        }
    });

    // TCP → WS: the game talking back (rare during pure playback, but the
    // wire protocol is nominally bidirectional: see the module docs).
    let mut tcp_to_ws = tokio::spawn(async move {
        let mut buf = [0u8; 8192];
        loop {
            match tcp_read.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if ws_write
                        .send(Message::Binary(buf[..n].to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });

    // How it ends depends on which leg ends first. Dropping a `JoinHandle`
    // does not stop its task, so a leg that has to stop is aborted by name.
    tokio::select! {
        // The broadcast is over. What is still queued goes to FA all the same:
        // the queue leg drains it and then closes the socket, which is how FA
        // learns the stream ended, and the game's side ends with it.
        _ = &mut ws_to_queue => {}
        // FA stopped reading, or hung up. Nothing more read from the server
        // can reach it.
        _ = &mut queue_to_tcp => {
            ws_to_queue.abort();
            tcp_to_ws.abort();
        }
        _ = &mut tcp_to_ws => {
            ws_to_queue.abort();
            queue_to_tcp.abort();
        }
    }
}

/// The named-pipe counterpart of [`relay_session`].
///
/// One leg fewer than the TCP version: the pipe is opened outbound-only, so
/// there is nothing to read back from FA. The other difference is the ending,
/// which the pipe does not deliver on its own (see [`PIPE_TERMINATOR`]), and
/// noticing that FA has gone: with nothing to read from it, a write failing is
/// the only sign the pipe gives, and a broadcast that falls silent never
/// writes again. So the session also ends on `ended`, the replay window
/// closing or [`LiveStreamer::end_session`] asking.
///
/// Both legs run inside this one task rather than each in its own, so the
/// pipe is closed by the time the task has finished: which is what
/// `end_session` waits for before the next live replay creates the pipe anew.
#[cfg(windows)]
async fn pipe_relay_session(
    mut pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    mut ws_write: SplitSink<WsStream, Message>,
    mut ws_read: SplitStream<WsStream>,
    first: Vec<u8>,
    ended: impl std::future::Future<Output = ()>,
) {
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let backlog = Backlog::default();
    if !backlog.admit(first.len()) || tx.send(first).is_err() {
        return;
    }

    // WS -> queue, exactly as on the TCP path: reading the socket must never
    // wait on FA draining the pipe, or the server's keepalive pings go
    // unanswered while the game is catching up. Polled side by side with the
    // pipe leg below, a write FA is slow to take holds up neither.
    let queued = backlog.clone();
    let ws_to_queue = async move {
        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(Message::Binary(data)) => {
                    if !queued.admit(data.len()) || tx.send(data).is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {} // ping/pong/text: ignore
            }
        }
    };

    let queue_to_pipe = async move {
        while let Some(data) = rx.recv().await {
            if pipe.write_all(&data).await.is_err() {
                return;
            }
            backlog.written(data.len());
        }
        // The stream ended rather than the pipe breaking, so FA is still
        // sitting there waiting for more. Unblock it.
        let _ = pipe.write_all(&PIPE_TERMINATOR).await;
        let _ = pipe.shutdown().await;
    };

    tokio::pin!(ws_to_queue, queue_to_pipe, ended);
    tokio::select! {
        // The broadcast is over. The pipe leg still drains what is queued and
        // writes the terminator, which is what ends the game's read, unless
        // the game goes first.
        _ = &mut ws_to_queue => {
            close_ws(&mut ws_write).await;
            tokio::select! {
                _ = &mut queue_to_pipe => {}
                _ = &mut ended => {}
            }
            return;
        }
        // The pipe broke: nothing more read from the server can reach FA.
        _ = &mut queue_to_pipe => {}
        // The window was closed, or the next replay needs the pipe.
        _ = &mut ended => {}
    }
    close_ws(&mut ws_write).await;
}

/// Close the replay socket rather than drop it mid-frame. Nothing reads from
/// FA on the pipe transport, but the server is still owed the close; bounded,
/// because [`LiveStreamer::end_session`] waits on it.
#[cfg(windows)]
async fn close_ws(ws_write: &mut SplitSink<WsStream, Message>) {
    let _ = tokio::time::timeout(WS_CLOSE_TIMEOUT, ws_write.close()).await;
}

/// Resolves once the replay window a pipe session feeds has closed, or the
/// session is told to stop.
///
/// Asked of the process port rather than of the pipe: see
/// [`pipe_relay_session`] for why the pipe cannot tell.
#[cfg(windows)]
async fn replay_ended(process: Arc<dyn ProcessPort>, stop: CancellationToken) {
    let closed = async {
        loop {
            tokio::time::sleep(REPLAY_EXIT_POLL).await;
            if !process.replay_running() {
                return;
            }
        }
    };
    tokio::select! {
        _ = closed => {}
        _ = stop.cancelled() => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::replay::test_support::{
        game_app_content, lua_mod_table, stream_opening, RecordingGameApp,
    };
    use crate::ports::GameMod;

    fn proxy_arguments() -> Vec<String> {
        vec![
            "/replay".to_owned(),
            "gpgnet://127.0.0.1:50123/4711/Ada.SCFAreplay".to_owned(),
        ]
    }

    fn sim_mod(uid: &str, name: &str) -> (String, String) {
        (uid.to_owned(), name.to_owned())
    }

    #[tokio::test]
    async fn a_live_game_plays_the_current_version_with_its_stream_s_mods() {
        let game_app = RecordingGameApp::default();
        let content = game_app_content(
            &[("a1b2-reclaim", "reclaim_turret.v0004.zip")],
            Ok(Some(3841)),
        );
        let version = content.live_version("faf").await.unwrap();
        let (_, warning) = start_live_in_game_app(
            &game_app,
            &content,
            version,
            "faf",
            "SCMP_009",
            proxy_arguments(),
            Some(vec![sim_mod("a1b2-reclaim", "Reclaim Turret")]),
        )
        .await
        .unwrap();
        assert_eq!(warning, None);
        assert_eq!(
            game_app.starts.lock().unwrap().as_slice(),
            [GameAppStart {
                game_version: Some(3841),
                featured_mod: Some("faf".into()),
                arguments: proxy_arguments(),
                map: Some("scmp_009".into()),
                mods: vec![GameMod {
                    uid: "a1b2-reclaim".into(),
                    file: "reclaim_turret.v0004.zip".into(),
                }],
                ..GameAppStart::default()
            }]
        );
    }

    #[tokio::test]
    async fn a_live_game_s_mod_with_no_file_is_played_without_and_named() {
        let game_app = RecordingGameApp::default();
        let content = game_app_content(&[], Ok(Some(3841)));
        let (_, warning) = start_live_in_game_app(
            &game_app,
            &content,
            Some(3841),
            "faf",
            "SCMP_009",
            proxy_arguments(),
            Some(vec![sim_mod("a1b2-reclaim", "Reclaim Turret")]),
        )
        .await
        .unwrap();
        assert_eq!(
            warning.as_deref(),
            Some("the game app plays this replay without its mods: Reclaim Turret")
        );
        assert!(game_app.starts.lock().unwrap()[0].mods.is_empty());
    }

    #[tokio::test]
    async fn a_stream_that_names_no_mods_starts_without_any_and_says_so() {
        let game_app = RecordingGameApp::default();
        let content = game_app_content(&[], Ok(Some(3841)));
        let (_, warning) = start_live_in_game_app(
            &game_app,
            &content,
            Some(3841),
            "faf",
            "SCMP_009",
            proxy_arguments(),
            None,
        )
        .await
        .unwrap();
        assert!(warning.is_some_and(|warning| warning.contains("could not be read")));
        assert_eq!(game_app.starts.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_live_version_that_cannot_be_asked_fails_the_watch() {
        let content = game_app_content(&[], Err("the API did not answer".into()));
        assert_eq!(
            content.live_version("faf").await,
            Err("could not ask which version of faf is current: the API did not answer".into())
        );
    }

    #[tokio::test]
    async fn the_stream_is_read_on_until_its_header_names_the_mods() {
        let opening = stream_opening(&lua_mod_table(&[("a1b2-reclaim", "Reclaim Turret", false)]));
        let (first, second) = opening.split_at(30);
        let mut head = first.to_vec();
        let mut frames = futures_util::stream::iter(vec![
            Ok(Message::Ping(Vec::new())),
            Ok(Message::Binary(second.to_vec())),
            Ok(Message::Binary(b"the command stream".to_vec())),
        ]);
        let mods = read_stream_mods(&mut head, &mut frames).await;
        assert_eq!(mods, Some(vec![sim_mod("a1b2-reclaim", "Reclaim Turret")]));
        assert_eq!(head, opening, "every frame read goes to the game first");
        assert_eq!(
            frames.next().await.and_then(Result::ok),
            Some(Message::Binary(b"the command stream".to_vec())),
            "nothing past the header is taken from the relay"
        );
    }

    #[tokio::test]
    async fn a_stream_that_ends_before_its_header_says_names_no_mods() {
        let opening = stream_opening(&lua_mod_table(&[("a1b2-reclaim", "Reclaim Turret", false)]));
        let mut head = opening[..30].to_vec();
        let mut frames = futures_util::stream::iter(Vec::<
            Result<Message, tokio_tungstenite::tungstenite::Error>,
        >::new());
        assert_eq!(read_stream_mods(&mut head, &mut frames).await, None);
        assert_eq!(head, &opening[..30]);
    }
}
