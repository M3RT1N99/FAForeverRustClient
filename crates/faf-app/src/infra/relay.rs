//! Real GPGNet relay server.
//!
//! Binds a loopback TCP server that the ICE adapter connects to (as its
//! `--gpgnet-client-port`). Bytes from the adapter are decoded with
//! [`faf_domain::protocol::gpgnet`] and pushed on `from_adapter`; messages on
//! `to_adapter` are encoded and written back. The launcher bridges these channels
//! to the lobby relay. Only one adapter connection is expected per game; when it
//! drops, or sends framing that can never parse, the relay ends.
//!
//! The same server is the game's own GPGNet port when the game runs its links
//! itself ([`GpgRelayServer::listen`]). There the game connects, and a game
//! that crashed in its match connects again, so it serves one connection after
//! another until it is stopped.

use std::sync::{Arc, Mutex};

use faf_domain::protocol::gpgnet::{self, encode, GpgMessage};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// The live relay's endpoints, handed to the adapter that owns it.
pub struct RelayChannels {
    /// Port the server is listening on: passed to the adapter as its GPGNet
    /// client port.
    pub port: u16,
    /// Messages decoded from the adapter (to be relayed to the lobby).
    pub from_adapter: mpsc::Receiver<GpgMessage>,
    /// Messages to encode and send to the adapter (relayed from the lobby).
    /// Unbounded, as a game's are: see [`GameConnection::Connected`].
    pub to_adapter: mpsc::UnboundedSender<GpgMessage>,
    /// Resolves when the adapter has connected to this parent relay. Pioneer
    /// opens its game-facing GPGNet listener immediately before this connection,
    /// so this is the safe readiness gate before launching Forged Alliance.
    pub ready: oneshot::Receiver<()>,
}

/// What a listener that outlives its connections reports: see
/// [`GpgRelayServer::listen`].
#[derive(Debug)]
pub enum GameConnection {
    /// A game connected. What is sent here reaches it while it stays, and is
    /// dropped with the connection: a game that comes back gets nothing the
    /// one before it was meant to. Dropping the sender closes the connection.
    ///
    /// Unbounded, so that sending to a game never waits on it: whoever sends
    /// here also hears the game, whose connection waits to be heard. With a
    /// bound, a full queue left each waiting on the other for good, and the
    /// lobby connection stalled behind them. What the lobby sends a game
    /// comes at the server's pace, and a game that stops reading loses its
    /// backlog with its connection.
    Connected(mpsc::UnboundedSender<GpgMessage>),
    /// A message from the connected game.
    Message(GpgMessage),
    /// The connected game went away.
    Closed,
}

impl From<GpgMessage> for GameConnection {
    fn from(message: GpgMessage) -> Self {
        Self::Message(message)
    }
}

/// The live listener's endpoints, handed to the backend that owns it.
pub struct GameListener {
    /// The game's GPGNet port: `/gpgnet 127.0.0.1:<port>`.
    pub port: u16,
    pub events: mpsc::Receiver<GameConnection>,
}

/// The loopback TCP server the Go (faf-pioneer) adapter connects to, and the
/// game itself when it runs its own links.
///
/// Deliberately *not* behind a port trait. It was one when the launcher owned
/// the relay and either adapter backend might have supplied it; today it is an
/// implementation detail of the backends that construct it concretely:
/// [`super::PioneerAdapter`], and [`super::NativeNetwork`], whose game takes
/// the adapter's place. The Java backend ([`super::JavaAdapter`]) speaks
/// JSON-RPC and has no relay of its own, and the `FakeRelay` that existed to
/// fill the seam was reachable from no test.
///
/// The backends remain available side by side; the choice is made by the
/// persisted setting or `FAF_ICE_ADAPTER_KIND`. Pioneer is explicit-only and
/// is never an automatic fallback for the production Java backend.
#[derive(Default)]
pub struct GpgRelayServer {
    cancel: Arc<Mutex<Option<CancellationToken>>>,
}

impl GpgRelayServer {
    /// Bind the relay server and return its port plus the two message channels.
    /// Errors if the socket can't be bound.
    pub async fn start(&self) -> Result<RelayChannels, String> {
        let (listener, port, token) = self.bind().await?;

        // from_adapter: server → launcher.  to_adapter: launcher → server.
        let (from_tx, from_rx) = mpsc::channel::<GpgMessage>(64);
        let (to_tx, to_rx) = mpsc::unbounded_channel::<GpgMessage>();
        let (ready_tx, ready_rx) = oneshot::channel();

        tokio::spawn(async move {
            run_relay(listener, from_tx, to_rx, ready_tx, token).await;
        });

        Ok(RelayChannels {
            port,
            from_adapter: from_rx,
            to_adapter: to_tx,
            ready: ready_rx,
        })
    }

    /// Bind the game's own GPGNet port, served until [`Self::stop`]: one
    /// connection at a time, and the next one after it closes, because a game
    /// that crashed in its match comes back to the same port (#275).
    pub async fn listen(&self) -> Result<GameListener, String> {
        let (listener, port, token) = self.bind().await?;
        let (events_tx, events) = mpsc::channel(64);
        tokio::spawn(serve_games(listener, events_tx, token));
        Ok(GameListener { port, events })
    }

    /// A loopback socket, and the token that stops whatever serves it. The
    /// previous server stops: a second game must not leave the first one's
    /// port open.
    async fn bind(&self) -> Result<(TcpListener, u16, CancellationToken), String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| format!("could not bind relay socket: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("could not read relay port: {e}"))?
            .port();
        tracing::info!(port, "GPGNet relay server bound");

        let token = CancellationToken::new();
        if let Some(prev) = self.cancel.lock().unwrap().replace(token.clone()) {
            tracing::debug!("cancelling previous relay server instance");
            prev.cancel();
        }
        Ok((listener, port, token))
    }

    /// Tear down the server and any adapter connection. Idempotent.
    pub fn stop(&self) {
        if let Some(token) = self.cancel.lock().unwrap().take() {
            tracing::info!("GPGNet relay server stop requested");
            token.cancel();
        }
    }
}

/// Accept the adapter connection and pump messages both ways until either side
/// closes or `cancel` fires.
async fn run_relay(
    listener: TcpListener,
    from_adapter: mpsc::Sender<GpgMessage>,
    mut to_adapter: mpsc::UnboundedReceiver<GpgMessage>,
    ready: oneshot::Sender<()>,
    cancel: CancellationToken,
) {
    let socket = tokio::select! {
        _ = cancel.cancelled() => {
            tracing::info!("GPGNet relay cancelled before adapter connected");
            return;
        },
        accepted = listener.accept() => match accepted {
            Ok((socket, _)) => socket,
            Err(e) => {
                tracing::error!(error = %e, "ICE relay accept failed");
                return;
            }
        }
    };
    tracing::info!("ICE adapter connected to local relay");
    let _ = ready.send(());
    let reason = match pump(socket, &from_adapter, &mut to_adapter, &cancel).await {
        Pumped::Stopped => "stop() called (cancellation token fired)",
        Pumped::Unheard => "from_adapter channel closed (launcher dropped its receiver)",
        Pumped::Closed(reason) => reason,
    };
    tracing::info!(reason, "GPGNet relay closed");
}

/// Serve the game's connections one after another until `cancel` fires or
/// nobody listens to `events` any more.
async fn serve_games(
    listener: TcpListener,
    events: mpsc::Sender<GameConnection>,
    cancel: CancellationToken,
) {
    loop {
        let socket = tokio::select! {
            _ = cancel.cancelled() => return,
            accepted = listener.accept() => match accepted {
                Ok((socket, _)) => socket,
                // The port has to stay open for a game coming back, so a
                // failed accept is waited out rather than the end of it.
                Err(error) => {
                    tracing::warn!(%error, "GPGNet accept failed; listening on");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            }
        };
        tracing::info!("the game connected to its GPGNet port");
        // Unbounded: nothing that sends to the game may wait on it (see
        // `GameConnection::Connected`).
        let (to_game, mut outgoing) = mpsc::unbounded_channel();
        if events
            .send(GameConnection::Connected(to_game))
            .await
            .is_err()
        {
            return;
        }
        match pump(socket, &events, &mut outgoing, &cancel).await {
            Pumped::Stopped | Pumped::Unheard => return,
            Pumped::Closed(reason) => {
                tracing::info!(reason, "the game's GPGNet connection closed");
                if events.send(GameConnection::Closed).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Why [`pump`] stopped.
enum Pumped {
    /// `stop` was called.
    Stopped,
    /// Nobody reads what the connection says any more.
    Unheard,
    /// The connection is over, for the reason given.
    Closed(&'static str),
}

/// Pump one connection both ways: what it sends, decoded, to `incoming`, and
/// what arrives on `outgoing`, encoded, to it. Until either side closes or
/// `cancel` fires; the socket closes on return.
async fn pump<T: From<GpgMessage>>(
    socket: TcpStream,
    incoming: &mpsc::Sender<T>,
    outgoing: &mut mpsc::UnboundedReceiver<GpgMessage>,
    cancel: &CancellationToken,
) -> Pumped {
    let (mut read_half, mut write_half) = socket.into_split();
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Pumped::Stopped,
            message = outgoing.recv() => {
                let Some(message) = message else {
                    return Pumped::Closed("whoever sent to it dropped its sender");
                };
                tracing::trace!(
                    command = %message.command,
                    args = ?message.args,
                    "relay: client -> connection"
                );
                if let Err(error) = write_half.write_all(&encode(&message)).await {
                    tracing::warn!(%error, "GPGNet write failed");
                    return Pumped::Closed("write to the socket failed");
                }
            }
            read = read_half.read(&mut chunk) => {
                let n = match read {
                    Ok(0) => return Pumped::Closed("the other end closed the connection (EOF)"),
                    Ok(n) => n,
                    Err(error) => {
                        tracing::warn!(%error, "GPGNet read failed");
                        return Pumped::Closed("read from the socket failed");
                    }
                };
                buffer.extend_from_slice(&chunk[..n]);
                let (messages, failure) = take_messages(&mut buffer, MAX_BUFFERED_BYTES);
                for message in messages {
                    tracing::trace!(
                        command = %message.command,
                        args = ?message.args,
                        "relay: connection -> client"
                    );
                    if incoming.send(message.into()).await.is_err() {
                        return Pumped::Unheard;
                    }
                }
                if let Some(failure) = failure {
                    tracing::warn!(
                        reason = failure,
                        buffered = buffer.len(),
                        "GPGNet relay received a stream it cannot parse; dropping the connection"
                    );
                    return Pumped::Closed("invalid GPGNet framing");
                }
            }
        }
    }
}

/// The most the relay holds for a message that has not finished arriving.
///
/// The decoder already refuses any frame longer than this, so an honest stream
/// never gets near it; the check here is what keeps that a guarantee about
/// memory rather than an assumption about the decoder.
const MAX_BUFFERED_BYTES: usize = gpgnet::MAX_FRAME_BYTES;

/// Take every complete message off `buffer`, and say whether the stream must
/// be dropped.
///
/// GPGNet has no outer frame, so after a corrupt length or type there is no
/// way to find where the next message starts. Waiting for more bytes would
/// only grow the buffer towards a message that will never complete, and the
/// game would sit in its lobby with no word of why. Dropping the connection
/// ends the session where the logs can say so.
fn take_messages(buffer: &mut Vec<u8>, cap: usize) -> (Vec<GpgMessage>, Option<&'static str>) {
    let drained = gpgnet::drain(buffer);
    let failure = drained
        .invalid
        .or_else(|| (buffer.len() > cap).then_some("incomplete message exceeds the buffer cap"));
    (drained.messages, failure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn starting_binds_a_real_loopback_port() {
        // The port is handed to the Go adapter as `--gpgnet-client-port`, so a
        // zero here would silently point it at nothing.
        let relay = GpgRelayServer::default();
        let channels = relay.start().await.expect("the relay should bind");
        assert!(channels.port > 0);
        relay.stop();
    }

    #[tokio::test]
    async fn stopping_is_idempotent_and_safe_before_a_start() {
        let relay = GpgRelayServer::default();
        relay.stop();
        let _ = relay.start().await.expect("the relay should bind");
        relay.stop();
        relay.stop();
    }

    #[tokio::test]
    async fn restarting_replaces_the_previous_server() {
        // A second game must not leave the first relay listening.
        let relay = GpgRelayServer::default();
        let first = relay.start().await.expect("bind").port;
        let second = relay.start().await.expect("rebind").port;
        assert_ne!(first, second, "each run gets its own socket");
        relay.stop();
    }

    #[test]
    fn an_incomplete_message_past_the_cap_ends_the_stream() {
        use faf_domain::protocol::gpgnet::GpgArg;

        let message = GpgMessage::new("GameState", vec![GpgArg::Str("Lobby".into())]);
        let bytes = encode(&message);

        // Under the cap, a partial frame is simply kept for the next read.
        let mut buffer = bytes[..bytes.len() - 1].to_vec();
        let (messages, failure) = take_messages(&mut buffer, 64);
        assert!(messages.is_empty());
        assert_eq!(failure, None);
        assert_eq!(buffer.len(), bytes.len() - 1);

        // The same partial frame over a smaller cap is a reason to hang up.
        let (_, failure) = take_messages(&mut buffer, 8);
        assert_eq!(failure, Some("incomplete message exceeds the buffer cap"));

        // Complete messages leave nothing behind, so they never trip it.
        let mut buffer = bytes.repeat(4);
        let (messages, failure) = take_messages(&mut buffer, 8);
        assert_eq!(messages, vec![message; 4]);
        assert_eq!(failure, None);
    }

    /// A corrupt frame from the adapter drops the connection after delivering
    /// what arrived intact before it, instead of buffering forever.
    #[tokio::test]
    async fn invalid_framing_from_the_adapter_tears_the_connection_down() {
        use faf_domain::protocol::gpgnet::GpgArg;
        use std::time::Duration;

        let relay = GpgRelayServer::default();
        let mut channels = relay.start().await.expect("bind");
        let mut adapter = tokio::net::TcpStream::connect(("127.0.0.1", channels.port))
            .await
            .expect("connect to the relay");

        let intact = GpgMessage::new("GameState", vec![GpgArg::Str("Idle".into())]);
        let mut bytes = encode(&intact);
        // A string length of -1: nothing after it can be parsed.
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        adapter.write_all(&bytes).await.expect("send to the relay");

        let limit = Duration::from_secs(5);
        assert_eq!(
            tokio::time::timeout(limit, channels.from_adapter.recv())
                .await
                .expect("delivered"),
            Some(intact)
        );
        assert_eq!(
            tokio::time::timeout(limit, channels.from_adapter.recv())
                .await
                .expect("closed"),
            None,
            "the relay ends after invalid framing"
        );
        let mut rest = [0_u8; 16];
        let read = tokio::time::timeout(limit, adapter.read(&mut rest))
            .await
            .expect("the socket is closed rather than left hanging");
        assert!(matches!(read, Ok(0) | Err(_)));
        relay.stop();
    }

    async fn next_event(listener: &mut GameListener) -> GameConnection {
        tokio::time::timeout(std::time::Duration::from_secs(5), listener.events.recv())
            .await
            .expect("an event")
            .expect("the listener is still serving")
    }

    /// The port a game that crashed in its match comes back to (#275): served
    /// again once its first connection is gone, and whatever was still on
    /// its way to the first one goes with it.
    #[tokio::test]
    async fn the_games_port_takes_the_game_back_after_it_closes() {
        use faf_domain::protocol::gpgnet::GpgArg;

        let server = GpgRelayServer::default();
        let mut listener = server.listen().await.expect("bind");

        let first = tokio::net::TcpStream::connect(("127.0.0.1", listener.port))
            .await
            .expect("connect");
        let GameConnection::Connected(to_first) = next_event(&mut listener).await else {
            panic!("the first game was not announced");
        };
        drop(first);
        assert!(matches!(
            next_event(&mut listener).await,
            GameConnection::Closed
        ));
        // Sent too late for the first game; the second must not get it.
        let _ = to_first.send(GpgMessage::new("IceMsg", vec![GpgArg::Int(2)]));

        let mut second = tokio::net::TcpStream::connect(("127.0.0.1", listener.port))
            .await
            .expect("the port is still open");
        let GameConnection::Connected(to_second) = next_event(&mut listener).await else {
            panic!("the second game was not announced");
        };
        let idle = GpgMessage::new("GameState", vec![GpgArg::Str("Idle".into())]);
        second.write_all(&encode(&idle)).await.unwrap();
        assert!(matches!(
            next_event(&mut listener).await,
            GameConnection::Message(message) if message == idle
        ));

        let lobby = GpgMessage::new("CreateLobby", vec![GpgArg::Int(0)]);
        to_second.send(lobby.clone()).unwrap();
        let mut received = Vec::new();
        let mut chunk = [0_u8; 256];
        while received.len() < encode(&lobby).len() {
            let read =
                tokio::time::timeout(std::time::Duration::from_secs(5), second.read(&mut chunk))
                    .await
                    .expect("the game hears from the client")
                    .unwrap();
            assert!(read > 0, "the connection closed");
            received.extend_from_slice(&chunk[..read]);
        }
        assert_eq!(
            received,
            encode(&lobby),
            "only what was sent to this game arrives"
        );
        server.stop();
    }

    /// A message longer than one read arrives whole, however the socket cuts
    /// it: an ICE candidate list runs past 4 KB.
    #[tokio::test]
    async fn a_message_longer_than_a_read_arrives_whole() {
        use faf_domain::protocol::gpgnet::GpgArg;

        let server = GpgRelayServer::default();
        let mut listener = server.listen().await.expect("bind");
        let mut game = tokio::net::TcpStream::connect(("127.0.0.1", listener.port))
            .await
            .expect("connect");
        // Held: letting go of it is letting go of the game.
        let GameConnection::Connected(_to_game) = next_event(&mut listener).await else {
            panic!("the game was not announced");
        };

        let long = GpgMessage::new(
            "IceMsg",
            vec![GpgArg::Int(7), GpgArg::Str("c".repeat(10_000))],
        );
        let bytes = encode(&long);
        for piece in bytes.chunks(3_000) {
            game.write_all(piece).await.unwrap();
            game.flush().await.unwrap();
            tokio::task::yield_now().await;
        }
        assert!(matches!(
            next_event(&mut listener).await,
            GameConnection::Message(message) if message == long
        ));
        server.stop();
    }

    #[tokio::test]
    async fn readiness_resolves_only_after_the_adapter_connects() {
        let relay = GpgRelayServer::default();
        let channels = relay.start().await.expect("bind");
        let port = channels.port;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), channels.ready)
                .await
                .is_err()
        );

        // Start a fresh relay because timing out consumes the first receiver.
        let channels = relay.start().await.expect("rebind");
        let connection = tokio::net::TcpStream::connect(("127.0.0.1", channels.port));
        let (connected, ready) = tokio::join!(connection, channels.ready);
        assert!(connected.is_ok());
        assert!(ready.is_ok());
        assert_ne!(port, channels.port);
        relay.stop();
    }
}
