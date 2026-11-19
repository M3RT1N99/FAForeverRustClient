//! What the client needs from Android itself, through its own Kotlin in
//! `gen/android`: the browser for sign-in, the share sheet and the device's
//! make for the logs and a problem report, and the FAF game app with its
//! updates.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use faf_app::infra::game_app_release::{GameAppRelease, GameAppReleases};
use faf_app::infra::log_export::export_client_logs;
use faf_app::infra::log_scrub::Scrubber;
use faf_app::ports::game_app::{GameAppPort, GameAppRun, GameAppStart};
use faf_ipc::shell::{
    GameAppEnd, GameAppInstall, GameAppLaunch, GameAppStatus, GameAppUpdate, ReportContext,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{Manager, Wry};
use tauri_plugin_opener::OpenerExt;

/// The app's Kotlin package.
const APP_PACKAGE: &str = "com.faforever.rustclient";

/// The app's `DiagnosticsPlugin`: the share sheet and the device's make.
pub(crate) struct Diagnostics(PluginHandle<Wry>);

/// The app's `GameAppPlugin`: the FAF game app's status, setup and start.
pub(crate) struct GameApp(PluginHandle<Wry>);

/// Sign-in's browser, and the diagnostics and game app plugins.
///
/// Android has no launcher command for a URL, only the intent the opener
/// plugin sends. It also freezes a process whose page the browser covers,
/// which would leave the sign-in's redirect unanswered until the player came
/// back. So the client is first kept running (a short foreground service the
/// Kotlin ends once the player is back), then the page opens in a Custom Tab,
/// which closing returns from to the client.
pub(crate) fn init() -> TauriPlugin<Wry> {
    Builder::new("android")
        .setup(|app, api| {
            let wait = api.register_android_plugin(APP_PACKAGE, "BrowserWaitPlugin")?;
            app.manage(Diagnostics(
                api.register_android_plugin(APP_PACKAGE, "DiagnosticsPlugin")?,
            ));
            let game_app = api.register_android_plugin(APP_PACKAGE, "GameAppPlugin")?;
            faf_app::infra::game_app::set_game_app(Arc::new(GameAppLauncher {
                plugin: game_app.clone(),
                replays: app.path().app_cache_dir()?.join("replays"),
            }));
            app.manage(GameApp(game_app));
            app.manage(GameAppUpdates::faf());
            let app = app.clone();
            faf_app::infra::set_browser_opener(move |url| {
                // Before the browser covers the page: Android starts a
                // foreground service only for an app in the foreground.
                if let Err(error) = wait.run_mobile_plugin::<()>("hold", ()) {
                    tracing::warn!(%error, "could not keep the client running for the browser");
                }
                let opened = app
                    .opener()
                    .open_url(url, Some("inAppBrowser"))
                    .map_err(std::io::Error::other);
                // No browser, so the client stays in front and never comes
                // back to end the wait: it ends here.
                if opened.is_err() {
                    let _ = wait.run_mobile_plugin::<()>("release", ());
                }
                opened
            });
            Ok(())
        })
        .build()
}

/// What `DiagnosticsPlugin.deviceInfo` answers.
#[derive(Deserialize)]
struct DeviceInfo {
    release: String,
    sdk: i32,
    manufacturer: String,
    model: String,
}

/// What `DiagnosticsPlugin.share` takes.
#[derive(Serialize)]
struct ShareArgs {
    path: String,
    mime: &'static str,
    title: String,
}

fn context_of(plugin: &PluginHandle<Wry>) -> Result<ReportContext, String> {
    let device: DeviceInfo = plugin
        .run_mobile_plugin("deviceInfo", ())
        .map_err(|error| format!("could not read the device: {error}"))?;
    Ok(ReportContext {
        version: env!("CARGO_PKG_VERSION").to_string(),
        commit: option_env!("FAF_BUILD_COMMIT")
            .unwrap_or("unknown")
            .to_string(),
        system: format!("Android {} (API {})", device.release, device.sdk),
        device: format!("{} {}", device.manufacturer, device.model),
    })
}

/// The hosts of a server the client was pointed at instead of FAF's. A test
/// server's address must not leave the device in a log.
fn test_servers() -> Vec<String> {
    const OVERRIDES: [&str; 7] = [
        "FAF_API_BASE",
        "FAF_HYDRA_BASE",
        "FAF_LOBBY_URL",
        "FAF_USER_API_BASE",
        "FAF_CONTENT_BASE",
        "FAF_REPLAY_VAULT_BASE",
        "FAF_IRC_HOST",
    ];
    OVERRIDES
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .filter_map(|value| match tauri::Url::parse(&value) {
            Ok(url) => url.host_str().map(str::to_owned),
            // `FAF_IRC_HOST` is a bare host.
            Err(_) => Some(value.trim().to_owned()),
        })
        .filter(|host| !host.is_empty() && !host.ends_with("faforever.com"))
        .collect()
}

/// Which build on which device, for a problem report's form.
#[tauri::command]
pub(crate) async fn report_context(
    diagnostics: tauri::State<'_, Diagnostics>,
) -> Result<ReportContext, String> {
    let plugin = diagnostics.0.clone();
    tauri::async_runtime::spawn_blocking(move || context_of(&plugin))
        .await
        .map_err(|_| "the device lookup stopped".to_string())?
}

/// Zip the client's logs, scrubbed (`faf_app::infra::log_scrub`), into the
/// app's cache and hand the zip to Android's share sheet under `title`.
#[tauri::command]
pub(crate) async fn share_client_logs(
    title: String,
    app: tauri::AppHandle,
    core: tauri::State<'_, crate::Core>,
    diagnostics: tauri::State<'_, Diagnostics>,
) -> Result<(), String> {
    let log_dir = crate::commands::log_directory("client", &app)?;
    // The cache, which the app's FileProvider shares (`res/xml/file_paths.xml`).
    let out_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("could not find the cache folder: {error}"))?
        .join("exports");
    let own_login = core
        .0
        .with_state(|state| state.auth.player.as_ref().map(|player| player.name.clone()));
    let plugin = diagnostics.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let context = context_of(&plugin)?;
        let report = format!(
            "FAForever client {} ({})\n{}\n{}\n",
            context.version, context.commit, context.system, context.device
        );
        let home = std::env::var("HOME").ok();
        let mut scrubber = Scrubber::new(own_login.as_deref(), home.as_deref(), test_servers());
        let path = export_client_logs(
            &log_dir,
            &out_dir,
            "faf-client-logs",
            &report,
            &mut scrubber,
        )?;
        plugin
            .run_mobile_plugin::<()>(
                "share",
                ShareArgs {
                    path: path.to_string_lossy().into_owned(),
                    mime: "application/zip",
                    title,
                },
            )
            .map_err(|error| format!("could not share the logs: {error}"))
    })
    .await
    .map_err(|_| "the log export stopped".to_string())?
}

/// Call `command` on a Kotlin plugin, off the async runtime: the call blocks
/// until the plugin answers, which for a start, the setup screen or an
/// install is when the player is back.
async fn call<A, R>(plugin: &PluginHandle<Wry>, command: &'static str, args: A) -> Result<R, String>
where
    A: Serialize + Send + 'static,
    R: DeserializeOwned + Send + 'static,
{
    let plugin = plugin.clone();
    tauri::async_runtime::spawn_blocking(move || {
        plugin
            .run_mobile_plugin::<R>(command, args)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| format!("the app's {command} stopped"))?
}

async fn game_app_status_of(game_app: &GameApp) -> Result<GameAppStatus, String> {
    let raw = call(&game_app.0, "status", ())
        .await
        .map_err(|error| format!("could not ask the game app: {error}"))?;
    crate::game_app::status_from(raw)
}

/// The FAF game app: whether it is there, whether it lets this client in, and
/// its game data.
#[tauri::command]
pub(crate) async fn game_app_status(
    game_app: tauri::State<'_, GameApp>,
) -> Result<GameAppStatus, String> {
    game_app_status_of(&game_app).await
}

/// Open the game app's own screen for its game data, `scfa`, the player's
/// Supreme Commander, or `game_files`, FAF's newest, and wait until the
/// player is back from it.
#[tauri::command]
pub(crate) async fn game_app_setup(
    step: String,
    game_app: tauri::State<'_, GameApp>,
) -> Result<GameAppEnd, String> {
    if !matches!(step.as_str(), "scfa" | "game_files") {
        return Err(format!("no such setup step: {step}"));
    }
    #[derive(Serialize)]
    struct Setup {
        step: String,
    }
    call(&game_app.0, "setup", Setup { step })
        .await
        .map(crate::game_app::end_from)
        .map_err(|error| format!("could not open the game app's setup: {error}"))
}

/// Start a game in the game app and wait until it ends, or until the game
/// app says why it did not start one.
#[tauri::command]
pub(crate) async fn game_app_play(
    launch: GameAppLaunch,
    game_app: tauri::State<'_, GameApp>,
) -> Result<GameAppEnd, String> {
    call(&game_app.0, "play", launch)
        .await
        .map(crate::game_app::end_from)
        .map_err(|error| format!("could not start the game app: {error}"))
}

/// The game app as `faf_app` starts replays in it: its PLAY, with a replay
/// file handed over through this app's file provider.
struct GameAppLauncher {
    plugin: PluginHandle<Wry>,
    /// Where a replay waits for the game app to read it: the cache, which the
    /// file provider shares (`res/xml/file_paths.xml`).
    replays: PathBuf,
}

/// How often the game app's status is asked while a start is handed over.
const TAKEOVER_POLL: Duration = Duration::from_millis(500);
/// How long a start may take to be handed over before the client stops
/// waiting: the game app's game process is up within a second or two.
const TAKEOVER_TIMEOUT: Duration = Duration::from_secs(60);

/// What `GameAppPlugin.play` takes for a start of `faf_app`'s.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayStart {
    game_version: Option<i32>,
    featured_mod: Option<String>,
    arguments: Option<Vec<String>>,
    map: Option<String>,
    replay_path: Option<String>,
}

impl GameAppLauncher {
    /// Copy `replay` where the file provider shares it, as the only replay
    /// there: the game app copies it when its game starts, and no start is
    /// handed over while a game runs.
    async fn hand_over(&self, replay: &Path) -> Result<String, String> {
        let _ = tokio::fs::remove_dir_all(&self.replays).await;
        tokio::fs::create_dir_all(&self.replays)
            .await
            .map_err(|error| format!("could not make the replay's folder: {error}"))?;
        let copy = self.replays.join("replay.scfareplay");
        tokio::fs::copy(replay, &copy)
            .await
            .map_err(|error| format!("could not hand the replay over: {error}"))?;
        Ok(copy.to_string_lossy().into_owned())
    }

    async fn game_running(&self) -> bool {
        let status: Result<serde_json::Value, String> = call(&self.plugin, "status", ()).await;
        status
            .and_then(crate::game_app::status_from)
            .is_ok_and(|status| status.game_data.is_some_and(|data| data.game_running))
    }
}

/// How a PLAY ended, as `faf_app` reads it: over, or why not.
fn outcome_of(
    answered: Result<Result<serde_json::Value, String>, tokio::task::JoinError>,
) -> Result<(), String> {
    match answered {
        Ok(Ok(raw)) => match crate::game_app::end_from(raw) {
            GameAppEnd::Ended => Ok(()),
            GameAppEnd::Failed { failure, message } => {
                Err(crate::game_app::failure_reason(&failure, &message))
            }
        },
        Ok(Err(error)) => Err(format!("could not start FAF's game app: {error}")),
        Err(_) => Err("the start in FAF's game app stopped".to_owned()),
    }
}

#[async_trait::async_trait]
impl GameAppPort for GameAppLauncher {
    /// The start is the game app's once its game process runs, which its
    /// status says, or once it answered, which it does at once when it
    /// refuses. The answer that ends the game resolves the run's `ended`.
    async fn start(&self, start: GameAppStart) -> Result<GameAppRun, String> {
        let raw = call(&self.plugin, "status", ())
            .await
            .map_err(|error| format!("could not ask FAF's game app: {error}"))?;
        if let Some(refusal) =
            crate::game_app::refusal_before_start(&crate::game_app::status_from(raw)?)
        {
            return Err(refusal.to_owned());
        }
        let replay_path = match &start.replay {
            Some(replay) => Some(self.hand_over(replay).await?),
            None => None,
        };
        let play = PlayStart {
            game_version: start.game_version,
            featured_mod: start.featured_mod,
            arguments: (!start.arguments.is_empty()).then_some(start.arguments),
            map: start.map,
            replay_path,
        };
        let plugin = self.plugin.clone();
        let mut answer = tokio::spawn(async move { call(&plugin, "play", play).await });
        let (sender, ended) = tokio::sync::oneshot::channel();
        let deadline = tokio::time::Instant::now() + TAKEOVER_TIMEOUT;
        loop {
            tokio::select! {
                answered = &mut answer => {
                    // Refused, or over before its game was seen running.
                    outcome_of(answered)?;
                    let _ = sender.send(Ok(()));
                    return Ok(GameAppRun { ended });
                }
                () = tokio::time::sleep(TAKEOVER_POLL) => {
                    if self.game_running().await || tokio::time::Instant::now() >= deadline {
                        tokio::spawn(async move {
                            let outcome = outcome_of(answer.await);
                            if let Err(reason) = &outcome {
                                tracing::warn!(%reason, "the game app gave a start up");
                            }
                            let _ = sender.send(outcome);
                        });
                        return Ok(GameAppRun { ended });
                    }
                }
            }
        }
    }
}

/// The game app's releases, and the newest the last check found, which is
/// the one an update installs.
pub(crate) struct GameAppUpdates {
    releases: GameAppReleases,
    newest: Mutex<Option<GameAppRelease>>,
}

impl GameAppUpdates {
    fn faf() -> Self {
        Self {
            releases: GameAppReleases::faf(),
            newest: Mutex::default(),
        }
    }
}

/// Look for a release of the game app newer than the one on this device.
#[tauri::command]
pub(crate) async fn game_app_update_check(
    game_app: tauri::State<'_, GameApp>,
    updates: tauri::State<'_, GameAppUpdates>,
) -> Result<Option<GameAppUpdate>, String> {
    let status = game_app_status_of(&game_app).await?;
    let newest = updates.releases.newest().await?;
    *updates
        .newest
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = newest.clone();
    let Some(newest) =
        newest.filter(|release| crate::game_app::is_newer(&release.version, &status))
    else {
        return Ok(None);
    };
    #[derive(Deserialize)]
    struct Network {
        metered: bool,
    }
    let network: Network = call(&game_app.0, "network", ())
        .await
        .map_err(|error| format!("could not ask about the network: {error}"))?;
    Ok(Some(GameAppUpdate {
        version: newest.version,
        metered: network.metered,
    }))
}

/// Download the release `version` the last check found, and have Android
/// install it once its APK has the published checksum, the game app's name,
/// its signer and a higher version code. Never under a game: Android ends
/// the game app's process to update it.
#[tauri::command]
pub(crate) async fn game_app_update_install(
    version: String,
    app: tauri::AppHandle,
    game_app: tauri::State<'_, GameApp>,
    updates: tauri::State<'_, GameAppUpdates>,
) -> Result<GameAppInstall, String> {
    let release = updates
        .newest
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .filter(|release| release.version == version)
        .ok_or_else(|| format!("version {version} of the game app was not found; look again"))?;
    // Asked before the download too, which a game makes pointless; the
    // plugin asks again right before the install, which is what counts.
    let status = game_app_status_of(&game_app).await?;
    if status.game_data.is_some_and(|data| data.game_running) {
        return Ok(GameAppInstall::Failed {
            failure: "RUNNING".into(),
            message: String::new(),
        });
    }
    let folder = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("could not find the cache folder: {error}"))?
        .join("game-app");
    let apk = updates.releases.download(&release, &folder).await?;
    #[derive(Serialize)]
    struct Install {
        path: String,
    }
    let path = apk.to_string_lossy().into_owned();
    let installed = call(&game_app.0, "installUpdate", Install { path })
        .await
        .map(crate::game_app::install_from)
        .map_err(|error| format!("could not install the game app: {error}"))?;
    // Kept otherwise, so a second try after a refusal (the player said no
    // to Android's question) needs no second download.
    if installed == GameAppInstall::Installed {
        let _ = tokio::fs::remove_file(&apk).await;
    }
    Ok(installed)
}
