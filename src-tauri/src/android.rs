//! What the client needs from Android itself, through its own Kotlin in
//! `gen/android`: the browser for sign-in, the share sheet and the device's
//! make for the logs and a problem report, and the FAF game app with its
//! updates.

use std::sync::{Mutex, PoisonError};

use faf_app::infra::game_app_release::{GameAppRelease, GameAppReleases};
use faf_app::infra::log_export::export_client_logs;
use faf_app::infra::log_scrub::Scrubber;
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
            app.manage(GameApp(
                api.register_android_plugin(APP_PACKAGE, "GameAppPlugin")?,
            ));
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
