//! Runtime choice between the connectivity backends.
//!
//! FAF has two ICE adapters: the long-standing Java `faf-ice-adapter`, which is
//! the production default, and the experimental Go faf-pioneer backend.
//!
//! Until now it was reachable only through `FAF_ICE_ADAPTER_KIND`, which meant
//! that in practice nobody knew the second adapter existed. This holds *both*
//! and dispatches per launch, so the Settings toggle takes effect on the next
//! game instead of requiring a restart.
//!
//! faf-re's game needs no adapter: it can run its links itself (its native
//! mode), which is the fourth backend here. Its links speak the Java
//! adapter's protocol, so they stand in for the Java adapter, and only for
//! it: see `SelectableIce::selected`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use faf_domain::state::{IceAdapter, RelayStatus};

use crate::infra::env_or;
use crate::ports::{ConnectivitySession, IceDebugWindows, IceParams, IcePort, Rejoin};

/// Development override. When set, it wins over the stored preference: a
/// developer testing one backend should not have it silently swapped when
/// settings load.
const OVERRIDE_ENV: &str = "FAF_ICE_ADAPTER_KIND";

/// Read the override, if one is set. `None` means "use the preference".
pub(crate) fn adapter_override() -> Option<IceAdapter> {
    let raw = env_or(OVERRIDE_ENV, "");
    (!raw.is_empty()).then(|| parse_adapter(&raw))
}

/// Map the override's spelling onto a backend.
///
/// Anything unrecognised uses the production Java adapter. An explicit Go
/// spelling is required to select the experimental backend.
pub(crate) fn parse_adapter(raw: &str) -> IceAdapter {
    match raw.trim().to_ascii_lowercase().as_str() {
        "go" | "pioneer" | "faf-pioneer" => IceAdapter::Go,
        "exp_java" | "exp-java" | "expjava" => IceAdapter::ExpJava,
        _ => IceAdapter::Java,
    }
}

pub struct SelectableIce {
    java: Arc<dyn IcePort>,
    exp_java: Arc<dyn IcePort>,
    go: Arc<dyn IcePort>,
    /// The game's own links, no adapter at all.
    native: Arc<dyn IcePort>,
    /// The user's choice for joining. Ignored while [`adapter_override`]
    /// returns a value.
    preferred: Mutex<IceAdapter>,
    /// The user's choice for hosting, Java or Go. Ignored while
    /// [`adapter_override`] returns a value.
    preferred_host: Mutex<IceAdapter>,
    /// The player's "Game's own network": on by default, as the setting is.
    native_enabled: AtomicBool,
    /// Every game runs its own links: a platform with no adapter to start.
    native_forced: AtomicBool,
    /// Which backend was last started, so [`IcePort::stop`] reaches the one
    /// that is actually running even if the preference changed meanwhile.
    running: Mutex<Option<Backend>>,
}

/// A backend this can start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Adapter(IceAdapter),
    /// The game's own links.
    Native,
}

impl Backend {
    fn label(self) -> &'static str {
        match self {
            Self::Adapter(adapter) => adapter.label(),
            Self::Native => "The game's own network",
        }
    }
}

impl SelectableIce {
    pub fn new(
        java: Arc<dyn IcePort>,
        exp_java: Arc<dyn IcePort>,
        go: Arc<dyn IcePort>,
        native: Arc<dyn IcePort>,
    ) -> Self {
        Self {
            java,
            exp_java,
            go,
            native,
            preferred: Mutex::new(IceAdapter::default()),
            preferred_host: Mutex::new(IceAdapter::Java),
            native_enabled: AtomicBool::new(true),
            native_forced: AtomicBool::new(false),
            running: Mutex::new(None),
        }
    }

    /// The backend a game starts on: its own links on a platform that has
    /// nothing else; the development override, which names an adapter; else
    /// the hosting preference for a game this client hosted, or the joining
    /// preference, with `Dynamic` following the host's mark.
    ///
    /// Where that comes to the Java adapter and the game can run its own
    /// links, it does, unless the player turned that off. Its links speak the
    /// Java adapter's protocol, so a Java player at the other end connects
    /// as before; a host marked as on another adapter would not be reached,
    /// so those games keep that adapter.
    fn selected(&self, params: &IceParams) -> Backend {
        if self.native_forced.load(Ordering::SeqCst) {
            return Backend::Native;
        }
        if let Some(adapter) = adapter_override() {
            return Backend::Adapter(adapter.for_game(&params.game_title));
        }
        let adapter = if params.hosted {
            self.hosting_adapter()
        } else {
            self.preferred.lock().unwrap().for_game(&params.game_title)
        };
        if adapter == IceAdapter::Java
            && params.rebuilt_engine
            && self.native_enabled.load(Ordering::SeqCst)
        {
            return Backend::Native;
        }
        Backend::Adapter(adapter)
    }

    fn backend(&self, backend: Backend) -> &Arc<dyn IcePort> {
        match backend {
            // `for_game` never returns `Dynamic`; Java is the safe reading of
            // an undecided choice all the same.
            Backend::Adapter(IceAdapter::Java | IceAdapter::Dynamic) => &self.java,
            Backend::Adapter(IceAdapter::ExpJava) => &self.exp_java,
            Backend::Adapter(IceAdapter::Go) => &self.go,
            Backend::Native => &self.native,
        }
    }
}

#[async_trait]
impl IcePort for SelectableIce {
    async fn start(&self, params: IceParams) -> Result<ConnectivitySession, String> {
        let backend = self.selected(&params);
        tracing::info!(backend = backend.label(), "starting selected ICE backend");
        // No fallback to the other backend, Pioneer is experimental. That is
        // this code's rule and not the player's concern, so the error leaves
        // it out and names only what failed.
        let session = self
            .backend(backend)
            .start(params)
            .await
            .map_err(|error| format!("{} could not start: {error}", backend.label()))?;
        // Only recorded on success: a failed start leaves nothing to stop, and
        // claiming otherwise would send a later `stop` to the wrong backend.
        *self.running.lock().unwrap() = Some(backend);
        Ok(session)
    }

    fn stop(&self) {
        // Stop whichever one is up, not whichever one is currently preferred,
        // the user may have switched between starting a game and leaving it.
        match self.running.lock().unwrap().take() {
            Some(backend) => self.backend(backend).stop(),
            // Nothing recorded: a start that failed part-way may still have
            // left a process behind, so stop them all rather than guess.
            None => {
                self.java.stop();
                self.exp_java.stop();
                self.go.stop();
                self.native.stop();
            }
        }
    }

    fn set_native_network(&self, enabled: bool) {
        self.native_enabled.store(enabled, Ordering::SeqCst);
    }

    fn force_native_network(&self) {
        self.native_forced.store(true, Ordering::SeqCst);
    }

    /// Only the game's own links can bring a game back to its match.
    fn begin_rejoin(&self) -> Option<Rejoin> {
        let running = *self.running.lock().unwrap();
        match running {
            Some(Backend::Native) => self.native.begin_rejoin(),
            _ => None,
        }
    }

    fn set_backend(&self, adapter: IceAdapter) {
        *self.preferred.lock().unwrap() = adapter;
    }

    fn set_host_backend(&self, adapter: IceAdapter) {
        *self.preferred_host.lock().unwrap() = adapter;
    }

    fn hosting_adapter(&self) -> IceAdapter {
        match adapter_override().unwrap_or_else(|| *self.preferred_host.lock().unwrap()) {
            IceAdapter::Go => IceAdapter::Go,
            IceAdapter::ExpJava => IceAdapter::ExpJava,
            _ => IceAdapter::Java,
        }
    }

    /// Pushed to all backends rather than only the selected one: the
    /// preference can change between this call and the next launch.
    fn set_debug_windows(&self, windows: IceDebugWindows) {
        self.java.set_debug_windows(windows);
        self.exp_java.set_debug_windows(windows);
        self.go.set_debug_windows(windows);
    }

    /// Asked of whichever backend is running. Pioneer has no status call, and
    /// saying so is more use than an empty table that looks like no peers.
    async fn relay_status(&self) -> RelayStatus {
        let running = *self.running.lock().unwrap();
        match running {
            Some(Backend::Adapter(IceAdapter::Go)) => RelayStatus::Unsupported {
                adapter: IceAdapter::Go,
            },
            Some(backend) => self.backend(backend).relay_status().await,
            None => RelayStatus::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::RelayMsg;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tokio::sync::mpsc;

    #[derive(Default)]
    struct Counting {
        started: AtomicU32,
        stopped: AtomicU32,
        fail: bool,
    }

    #[async_trait]
    impl IcePort for Counting {
        async fn start(&self, _params: IceParams) -> Result<ConnectivitySession, String> {
            self.started.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err("no adapter".into());
            }
            let (_to_lobby_tx, to_lobby) = mpsc::channel::<RelayMsg>(1);
            let (from_lobby, _from_lobby_rx) = mpsc::channel::<RelayMsg>(1);
            Ok(ConnectivitySession {
                game_port: 1,
                links: crate::ports::GameLinks::Adapter,
                to_lobby,
                from_lobby,
                late_failure: None,
            })
        }
        fn stop(&self) {
            self.stopped.fetch_add(1, Ordering::SeqCst);
        }
        fn begin_rejoin(&self) -> Option<Rejoin> {
            Some(Rejoin {
                seats: Vec::new(),
                source: 0,
            })
        }
    }

    fn params() -> IceParams {
        titled("Friday 4v4")
    }

    fn titled(title: &str) -> IceParams {
        IceParams {
            player_id: 7,
            player_login: "Ada".into(),
            game_id: 1,
            init_mode: 0,
            game_title: title.into(),
            hosted: false,
            rebuilt_engine: false,
        }
    }

    /// The same game with faf-re's executable.
    fn rebuilt(params: IceParams) -> IceParams {
        IceParams {
            rebuilt_engine: true,
            ..params
        }
    }

    struct Backends {
        java: Arc<Counting>,
        exp_java: Arc<Counting>,
        go: Arc<Counting>,
        native: Arc<Counting>,
        ice: SelectableIce,
    }

    fn backends() -> Backends {
        let java = Arc::new(Counting::default());
        let exp_java = Arc::new(Counting::default());
        let go = Arc::new(Counting::default());
        let native = Arc::new(Counting::default());
        let ice = SelectableIce::new(java.clone(), exp_java.clone(), go.clone(), native.clone());
        Backends {
            java,
            exp_java,
            go,
            native,
            ice,
        }
    }

    fn pair() -> (Arc<Counting>, Arc<Counting>, Arc<Counting>, SelectableIce) {
        let all = backends();
        (all.java, all.exp_java, all.go, all.ice)
    }

    fn started(backend: &Counting) -> u32 {
        backend.started.load(Ordering::SeqCst)
    }

    #[tokio::test]
    async fn faf_res_game_runs_its_own_links_where_it_would_run_on_java() {
        let all = backends();
        if adapter_override().is_some() {
            return;
        }
        all.ice.start(rebuilt(params())).await.unwrap();
        assert_eq!(started(&all.native), 1, "the default: Dynamic, unmarked");
        all.ice.stop();
        assert_eq!(all.native.stopped.load(Ordering::SeqCst), 1);

        all.ice.set_backend(IceAdapter::Java);
        all.ice.start(rebuilt(params())).await.unwrap();
        assert_eq!(started(&all.native), 2, "an explicit Java");
        all.ice.stop();

        all.ice.start(params()).await.unwrap();
        assert_eq!(started(&all.java), 1, "the original game: nothing changes");
        assert_eq!(started(&all.native), 2);
    }

    #[tokio::test]
    async fn a_host_on_another_adapter_keeps_that_adapter() {
        // Its links would not reach a host on Pioneer, which the mark says
        // it is.
        let all = backends();
        if adapter_override().is_some() {
            return;
        }
        all.ice
            .start(rebuilt(titled("Friday 4v4 [go-adapter]")))
            .await
            .unwrap();
        assert_eq!(started(&all.go), 1);
        all.ice.stop();
        all.ice
            .start(rebuilt(titled("Friday 4v4 [exp-java-adapter]")))
            .await
            .unwrap();
        assert_eq!(started(&all.exp_java), 1);
        all.ice.stop();

        // Hosting on Go marks the title, so the host runs Go.
        all.ice.set_host_backend(IceAdapter::Go);
        all.ice
            .start(IceParams {
                hosted: true,
                ..rebuilt(titled("Friday 4v4 [go-adapter]"))
            })
            .await
            .unwrap();
        assert_eq!(started(&all.go), 2);
        assert_eq!(started(&all.native), 0);
    }

    #[tokio::test]
    async fn turned_off_the_game_goes_to_the_adapter() {
        let all = backends();
        if adapter_override().is_some() {
            return;
        }
        all.ice.set_native_network(false);
        all.ice.start(rebuilt(params())).await.unwrap();
        assert_eq!(started(&all.java), 1);
        assert_eq!(started(&all.native), 0);
    }

    #[tokio::test]
    async fn a_platform_without_adapters_runs_every_game_on_its_own_links() {
        let all = backends();
        all.ice.force_native_network();
        all.ice.set_native_network(false);
        all.ice
            .start(titled("Friday 4v4 [go-adapter]"))
            .await
            .unwrap();
        assert_eq!(started(&all.native), 1);
        assert_eq!(started(&all.go) + started(&all.java), 0);
    }

    #[tokio::test]
    async fn only_the_games_own_links_bring_a_game_back() {
        let all = backends();
        if adapter_override().is_some() {
            return;
        }
        assert_eq!(all.ice.begin_rejoin(), None, "nothing running");
        all.ice.start(params()).await.unwrap();
        assert_eq!(all.ice.begin_rejoin(), None, "an adapter cannot");
        all.ice.stop();
        all.ice.start(rebuilt(params())).await.unwrap();
        assert!(all.ice.begin_rejoin().is_some());
    }

    #[tokio::test]
    async fn java_is_the_default_backend() {
        let (java, _exp_java, go, ice) = pair();
        ice.start(params()).await.unwrap();
        assert_eq!(java.started.load(Ordering::SeqCst), 1);
        assert_eq!(go.started.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn dynamic_joins_a_marked_lobby_on_go_and_every_other_on_java() {
        let (java, exp_java, go, ice) = pair();
        ice.set_backend(IceAdapter::Dynamic);
        ice.start(titled("Friday 4v4 [go-adapter]")).await.unwrap();
        assert_eq!(go.started.load(Ordering::SeqCst), 1, "the host is on Go");
        ice.stop();
        ice.start(titled("Friday 4v4 [exp-java-adapter]"))
            .await
            .unwrap();
        assert_eq!(
            exp_java.started.load(Ordering::SeqCst),
            1,
            "the host is on Exp Java"
        );
        ice.stop();
        ice.start(titled("Friday 4v4")).await.unwrap();
        assert_eq!(java.started.load(Ordering::SeqCst), 1, "no mark, so Java");
        assert_eq!(go.started.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_explicit_choice_ignores_the_mark() {
        let (java, _exp_java, go, ice) = pair();
        ice.set_backend(IceAdapter::Java);
        ice.start(titled("Friday 4v4 [go-adapter]")).await.unwrap();
        assert_eq!(java.started.load(Ordering::SeqCst), 1);
        assert_eq!(go.started.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn hosting_has_its_own_choice() {
        let (_java, _exp_java, _go, ice) = pair();
        if adapter_override().is_some() {
            return;
        }
        assert_eq!(ice.hosting_adapter(), IceAdapter::Java, "the default");
        ice.set_backend(IceAdapter::Go);
        assert_eq!(
            ice.hosting_adapter(),
            IceAdapter::Java,
            "joining on Go says nothing about hosting"
        );
        ice.set_host_backend(IceAdapter::Go);
        assert_eq!(ice.hosting_adapter(), IceAdapter::Go);
        ice.set_host_backend(IceAdapter::ExpJava);
        assert_eq!(ice.hosting_adapter(), IceAdapter::ExpJava);
        ice.set_host_backend(IceAdapter::Dynamic);
        assert_eq!(
            ice.hosting_adapter(),
            IceAdapter::Java,
            "nobody to follow when hosting"
        );
    }

    #[tokio::test]
    async fn a_hosted_game_starts_on_the_hosting_choice() {
        // Joining on explicit Java while hosting on Go: the title carries the
        // mark, and the host's own game has to run on what the mark says.
        let (java, _exp_java, go, ice) = pair();
        if adapter_override().is_some() {
            return;
        }
        ice.set_backend(IceAdapter::Java);
        ice.set_host_backend(IceAdapter::Go);
        ice.start(IceParams {
            hosted: true,
            ..titled("Friday 4v4 [go-adapter]")
        })
        .await
        .unwrap();
        assert_eq!(go.started.load(Ordering::SeqCst), 1);
        assert_eq!(java.started.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn choosing_java_takes_effect_on_the_next_launch() {
        // The point of holding both: no restart.
        let (java, _exp_java, go, ice) = pair();
        ice.set_backend(IceAdapter::Java);
        ice.start(params()).await.unwrap();
        assert_eq!(java.started.load(Ordering::SeqCst), 1);
        assert_eq!(go.started.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn stop_reaches_the_backend_that_is_actually_running() {
        // Switching the preference mid-game must not leave the started
        // adapter alive while stopping an idle one.
        let (java, _exp_java, go, ice) = pair();
        ice.start(params()).await.unwrap();
        ice.set_backend(IceAdapter::Go);
        ice.stop();

        assert_eq!(java.stopped.load(Ordering::SeqCst), 1, "the running one");
        assert_eq!(go.stopped.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn stopping_without_a_started_backend_stops_them_all() {
        // A start that failed part-way may still have left a process behind.
        let all = backends();
        all.ice.stop();
        for backend in [&all.java, &all.exp_java, &all.go, &all.native] {
            assert_eq!(backend.stopped.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn a_failed_default_start_does_not_fall_back_to_experimental_pioneer() {
        let java = Arc::new(Counting {
            fail: true,
            ..Counting::default()
        });
        let exp_java = Arc::new(Counting::default());
        let go = Arc::new(Counting::default());
        let ice = SelectableIce::new(
            java.clone(),
            exp_java.clone(),
            go.clone(),
            Arc::new(Counting::default()),
        );

        let error = match ice.start(params()).await {
            Ok(_) => panic!("experimental fallback unexpectedly started"),
            Err(error) => error,
        };
        ice.stop();
        assert_eq!(java.started.load(Ordering::SeqCst), 1);
        assert_eq!(go.started.load(Ordering::SeqCst), 0);
        assert_eq!(java.stopped.load(Ordering::SeqCst), 1);
        assert_eq!(exp_java.stopped.load(Ordering::SeqCst), 1);
        assert_eq!(go.stopped.load(Ordering::SeqCst), 1);
        assert_eq!(error, "Java (faf-ice-adapter) could not start: no adapter");
    }

    #[tokio::test]
    async fn the_relay_status_comes_from_the_running_backend() {
        let (_java, _exp_java, _go, ice) = pair();
        assert_eq!(
            ice.relay_status().await,
            RelayStatus::Idle,
            "nothing running"
        );
        if adapter_override().is_some() {
            return;
        }
        ice.set_backend(IceAdapter::Go);
        ice.start(params()).await.unwrap();
        assert_eq!(
            ice.relay_status().await,
            RelayStatus::Unsupported {
                adapter: IceAdapter::Go
            }
        );
        ice.stop();
        assert_eq!(ice.relay_status().await, RelayStatus::Idle, "stopped");
    }

    #[test]
    fn the_override_spelling_is_forgiving() {
        assert_eq!(parse_adapter("go"), IceAdapter::Go);
        assert_eq!(parse_adapter("  GO "), IceAdapter::Go);
        assert_eq!(parse_adapter("pioneer"), IceAdapter::Go);
        assert_eq!(parse_adapter("faf-pioneer"), IceAdapter::Go);
        assert_eq!(parse_adapter("exp_java"), IceAdapter::ExpJava);
        assert_eq!(parse_adapter("exp-java"), IceAdapter::ExpJava);
        assert_eq!(parse_adapter("java"), IceAdapter::Java);
    }

    #[test]
    fn an_unrecognised_override_uses_the_production_backend() {
        for raw in ["", "golang", "rust", "  "] {
            assert_eq!(parse_adapter(raw), IceAdapter::Java, "{raw:?}");
        }
    }
}
