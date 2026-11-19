//! An online game in FAF's game app, where the game is an app of its own
//! (Android): the start its launcher contract takes for a launch, and the
//! game it runs, as far as the launcher can see it.
//!
//! The game app keeps its own files and adds its own switches (`/init`,
//! `/log`, the rejoin's), so a start names what the game needs and carries
//! the launcher's own switches alone, each value an element of its own, by
//! the contract's whitelist (faf-re's `android/README.md`, "The launcher's
//! contract"). Its game runs every link itself: a platform with a game app
//! has no adapter to run them.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{oneshot, Notify};

use crate::ports::game_app::{contract_map_name, GameAppRun, GameAppStart};
use crate::ports::{GameExit, GameLaunchParams, GameNetwork};

/// The game app's start for `params`, its replay recorded at `savereplay`
/// where a recorder listens.
pub(super) fn start_for(
    params: &GameLaunchParams,
    savereplay: Option<&str>,
) -> Result<GameAppStart, String> {
    let GameNetwork::Native {
        force_relay,
        rejoin,
    } = &params.network
    else {
        return Err(
            "FAF's game app runs the game's links itself, and this game was to have an adapter"
                .to_owned(),
        );
    };
    Ok(GameAppStart {
        game_version: params.content.game_version,
        featured_mod: Some(params.featured_mod.clone()),
        arguments: arguments(params, savereplay, *force_relay),
        map: params.content.map.as_deref().and_then(contract_map_name),
        mods: params.content.mods.clone(),
        // Every game on its own links offers to come back, as on the desktop
        // (`network_arguments`).
        rejoin_offer: true,
        rejoin: rejoin.clone(),
        replay: None,
    })
}

/// The launcher's switches: the launch's own (the player's rating, games,
/// country and clan, and a matchmaker game's seat), `/savereplay`,
/// `/gpgnet` and the links'. Built for the contract rather than taken from a
/// desktop command line, which also has the install's switches (`/init`,
/// `/log`, `/prefs`, the player's launch options) and the ones the game app
/// adds itself.
fn arguments(params: &GameLaunchParams, savereplay: Option<&str>, force_relay: bool) -> Vec<String> {
    let mut arguments = params.args.clone();
    if let Some(url) = savereplay {
        arguments.extend(["/savereplay".to_owned(), url.to_owned()]);
    }
    arguments.extend([
        "/gpgnet".to_owned(),
        format!("127.0.0.1:{}", params.game_port),
        "/nativenet".to_owned(),
    ]);
    if force_relay {
        arguments.push("/nativenetrelay".to_owned());
    }
    arguments
}

/// The game app's game while one runs.
///
/// Its end comes as the start's answer ([`GameAppRun::ended`]), which is
/// recorded and announced the way a process's exit is: into the launcher's
/// `game_exit`, then `exited`, so that one wait serves both.
pub(super) struct GameAppGame {
    current: Arc<Mutex<Option<Run>>>,
    starts: AtomicU64,
    exited: Arc<Notify>,
    exit: Arc<Mutex<GameExit>>,
}

/// A start whose end is waited for.
struct Run {
    /// Its number, so that an older start's end clears nothing.
    start: u64,
    /// Dropped to stop waiting, which drops the start's answer with it.
    stop: oneshot::Sender<()>,
}

impl GameAppGame {
    pub(super) fn new(exited: Arc<Notify>, exit: Arc<Mutex<GameExit>>) -> Self {
        Self {
            current: Arc::new(Mutex::new(None)),
            starts: AtomicU64::new(1),
            exited,
            exit,
        }
    }

    pub(super) fn running(&self) -> bool {
        self.current.lock().unwrap().is_some()
    }

    /// Wait for the end of the game `run` started.
    pub(super) fn track(&self, run: GameAppRun) {
        let start = self.starts.fetch_add(1, Ordering::Relaxed);
        let (stop, stopped) = oneshot::channel();
        *self.current.lock().unwrap() = Some(Run { start, stop });
        let current = self.current.clone();
        let exited = self.exited.clone();
        let exit = self.exit.clone();
        tokio::spawn(async move {
            let ended = tokio::select! {
                ended = run.ended => match ended {
                    Ok(Ok(ended)) => ended,
                    Ok(Err(reason)) => GameExit::Refused(reason),
                    // The game app's side went without an answer: nobody can
                    // say how its game ended.
                    Err(_) => GameExit::Stopped,
                },
                // `stop`: not waited for any more.
                _ = stopped => return,
            };
            let mut current = current.lock().unwrap();
            if current.as_ref().is_some_and(|run| run.start == start) {
                // Before the game reads as gone, so that whoever finds it so
                // reads how it ended too.
                *exit.lock().unwrap() = ended;
                *current = None;
                drop(current);
                exited.notify_waiters();
            }
        });
    }

    /// Stop waiting for the running game, which then reads as stopped.
    ///
    /// The game app's game itself cannot be ended from here: Android lets
    /// one app end no other app's activity, and the game app's contract has
    /// no command for it. It goes on until its player leaves it; the client
    /// stops waiting for it, and so no longer keeps running for it in the
    /// back.
    pub(super) fn stop(&self) {
        let mut current = self.current.lock().unwrap();
        let Some(run) = current.take() else {
            return;
        };
        *self.exit.lock().unwrap() = GameExit::Stopped;
        drop(current);
        drop(run.stop);
        self.exited.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{GameContent, GameMod, Rejoin, RejoinSeat, ReplayMetadata};

    fn params(network: GameNetwork) -> GameLaunchParams {
        GameLaunchParams {
            game_id: 4711,
            game_port: 6112,
            init_mode: 1,
            featured_mod: "faf".into(),
            player_id: 101,
            player_login: "Ada".into(),
            // What `services::launcher` hands over for a matchmaker game.
            args: [
                "/numgames", "12", "/mean", "1500", "/deviation", "500", "/country", "DE",
                "/clan", "FAF", "/team", "2", "/players", "2", "/startspot", "1",
            ]
            .map(String::from)
            .to_vec(),
            replay: ReplayMetadata::default(),
            network,
            content: GameContent {
                game_version: Some(3841),
                map: Some("SCMP_009".into()),
                mods: vec![GameMod {
                    uid: "a0b1c2d3-0000-0000-0000-000000000001".into(),
                    file: "score_board.v0019.zip".into(),
                }],
            },
        }
    }

    fn native(force_relay: bool, rejoin: Option<Rejoin>) -> GameNetwork {
        GameNetwork::Native {
            force_relay,
            rejoin,
        }
    }

    /// The contract's whitelist of `LAUNCHER_ARGUMENTS`: each switch once,
    /// spelled so, then its value, by the patterns of its table.
    fn whitelisted(arguments: &[String]) -> Result<(), String> {
        fn digits(text: &str, most: usize) -> bool {
            (1..=most).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_digit())
        }
        fn port(text: &str) -> bool {
            digits(text, 5) && text.parse::<u16>().is_ok_and(|port| port > 0)
        }
        fn loopback(text: &str) -> Option<&str> {
            text.strip_prefix("127.0.0.1:")
                .or_else(|| text.strip_prefix("localhost:"))
        }
        fn decimal(text: &str) -> bool {
            let unsigned = text.strip_prefix('-').unwrap_or(text);
            match unsigned.split_once('.') {
                Some((whole, fraction)) => digits(whole, 10) && digits(fraction, 20),
                None => digits(unsigned, 10),
            }
        }
        fn savereplay(text: &str) -> bool {
            let Some(rest) = text.strip_prefix("gpgnet://").and_then(loopback) else {
                return false;
            };
            let mut parts = rest.splitn(3, '/');
            let (Some(at), Some(game), Some(file)) = (parts.next(), parts.next(), parts.next())
            else {
                return false;
            };
            port(at)
                && digits(game, 10)
                && file.strip_suffix(".SCFAreplay").is_some_and(|name| {
                    (1..=64).contains(&name.len())
                        && name
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
                })
        }
        let mut seen = Vec::new();
        let mut arguments = arguments.iter();
        while let Some(switch) = arguments.next() {
            if seen.contains(switch) {
                return Err(format!("{switch} twice"));
            }
            seen.push(switch.clone());
            if matches!(switch.as_str(), "/nativenet" | "/nativenetrelay") {
                continue;
            }
            let value = arguments
                .next()
                .ok_or_else(|| format!("{switch} without its value"))?;
            let fits = match switch.as_str() {
                "/gpgnet" => loopback(value).is_some_and(port),
                "/savereplay" => savereplay(value),
                "/mean" | "/deviation" => decimal(value),
                "/numgames" | "/team" | "/players" | "/startspot" => digits(value, 4),
                "/country" => {
                    value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_uppercase())
                }
                "/clan" => {
                    (1..=8).contains(&value.len())
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                }
                _ => return Err(format!("{switch} is not on the whitelist")),
            };
            if !fits {
                return Err(format!("{switch} {value}"));
            }
        }
        Ok(())
    }

    #[test]
    fn a_start_carries_the_whitelisted_switches_alone_each_value_on_its_own() {
        let start = start_for(
            &params(native(true, None)),
            Some("gpgnet://127.0.0.1:50123/4711/Ada.SCFAreplay"),
        )
        .unwrap();
        assert_eq!(whitelisted(&start.arguments), Ok(()));
        assert_eq!(
            start.arguments[16..],
            [
                "/savereplay",
                "gpgnet://127.0.0.1:50123/4711/Ada.SCFAreplay",
                "/gpgnet",
                "127.0.0.1:6112",
                "/nativenet",
                "/nativenetrelay",
            ]
        );
        for refused in ["/init", "/log", "/con", "/rejoinoffer", "/rejoin", "/nobugreport"] {
            assert!(!start.arguments.iter().any(|argument| argument == refused), "{refused}");
        }
    }

    #[test]
    fn the_whitelist_check_refuses_what_the_game_app_refuses() {
        let check = |arguments: &[&str]| {
            whitelisted(&arguments.iter().map(|argument| argument.to_string()).collect::<Vec<_>>())
        };
        assert!(check(&["/gpgnet", "127.0.0.1:6112 /nativenet"]).is_err(), "two in one");
        assert!(check(&["/gpgnet", "game.invalid:6112"]).is_err(), "off the loopback");
        assert!(check(&["/nativenet", "/nativenet"]).is_err(), "twice");
        assert!(check(&["/uef"]).is_err(), "not on it yet");
        assert!(check(&["/numgames", "10000"]).is_err());
        assert!(check(&["/mean", "1500.25", "/deviation", "-3"]).is_ok());
    }

    #[test]
    fn a_start_names_what_the_game_needs_and_offers_its_return() {
        let start = start_for(&params(native(false, None)), None).unwrap();
        assert_eq!(start.game_version, Some(3841));
        assert_eq!(start.featured_mod.as_deref(), Some("faf"));
        assert_eq!(start.map.as_deref(), Some("scmp_009"), "as the contract spells it");
        assert_eq!(start.mods, params(native(false, None)).content.mods);
        assert!(start.rejoin_offer);
        assert_eq!(start.rejoin, None);
        assert_eq!(start.replay, None);
        assert!(!start.arguments.contains(&"/savereplay".to_owned()), "no recorder");
        assert!(!start.arguments.contains(&"/nativenetrelay".to_owned()));
    }

    #[test]
    fn a_return_names_its_seat_and_its_source() {
        let rejoin = Rejoin {
            seats: vec![
                RejoinSeat {
                    player_id: 102,
                    source: 0,
                },
                RejoinSeat {
                    player_id: 103,
                    source: 2,
                },
            ],
            source: 1,
        };
        let start = start_for(&params(native(false, Some(rejoin.clone()))), None).unwrap();
        assert_eq!(start.rejoin.as_ref(), Some(&rejoin));
        assert_eq!(rejoin.peers(), "102=0,103=2");
        assert!(start.rejoin_offer, "the contract needs both on a return");
        assert_eq!(whitelisted(&start.arguments), Ok(()));
    }

    #[test]
    fn a_game_for_an_adapter_is_not_started_in_the_game_app() {
        let start = start_for(
            &params(GameNetwork::Adapter {
                rebuilt_engine: true,
            }),
            None,
        );
        assert!(start.is_err());
    }

    fn game() -> (GameAppGame, Arc<Notify>, Arc<Mutex<GameExit>>) {
        let exited = Arc::new(Notify::new());
        let exit = Arc::new(Mutex::new(GameExit::Quit));
        (GameAppGame::new(exited.clone(), exit.clone()), exited, exit)
    }

    #[tokio::test]
    async fn the_answer_is_how_the_game_ended() {
        for (answer, expected) in [
            (Ok(GameExit::Crashed), GameExit::Crashed),
            (Err("no such map (NO_MAP)".to_owned()), GameExit::Refused("no such map (NO_MAP)".into())),
        ] {
            let (game, exited, exit) = game();
            let (end, ended) = oneshot::channel();
            game.track(GameAppRun { ended });
            assert!(game.running());
            let announced = exited.notified();
            end.send(answer).unwrap();
            announced.await;
            assert!(!game.running());
            assert_eq!(*exit.lock().unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn stopping_marks_the_game_stopped_and_drops_the_wait() {
        let (game, _, exit) = game();
        let (mut end, ended) = oneshot::channel::<Result<GameExit, String>>();
        game.track(GameAppRun { ended });
        game.stop();
        assert!(!game.running());
        assert_eq!(*exit.lock().unwrap(), GameExit::Stopped);
        // The waiting task let go of the answer: the game app's side sees
        // nobody waiting any more.
        tokio::time::timeout(std::time::Duration::from_secs(5), end.closed())
            .await
            .expect("the wait was dropped");
    }
}
