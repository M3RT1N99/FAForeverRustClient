//! What the client needs from Android itself, through its own Kotlin in
//! `gen/android`: the browser for sign-in, the share sheet and the device's
//! make for the logs and a problem report, and the FAF game app.

use faf_app::infra::log_export::export_client_logs;
use faf_app::infra::log_scrub::Scrubber;
use faf_ipc::shell::{GameAppEnd, GameAppLaunch, GameAppStatus, ReportContext};
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

/// The FAF game app: whether it is there, whether it lets this client in, and
/// its game data.
#[tauri::command]
pub(crate) async fn game_app_status(
    game_app: tauri::State<'_, GameApp>,
) -> Result<GameAppStatus, String> {
    let plugin = game_app.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let raw = plugin
            .run_mobile_plugin::<serde_json::Value>("status", ())
            .map_err(|error| format!("could not ask the game app: {error}"))?;
        crate::game_app::status_from(raw)
    })
    .await
    .map_err(|_| "the game app lookup stopped".to_string())?
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
    let plugin = game_app.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        #[derive(Serialize)]
        struct Setup {
            step: String,
        }
        plugin
            .run_mobile_plugin::<serde_json::Value>("setup", Setup { step })
            .map(crate::game_app::end_from)
            .map_err(|error| format!("could not open the game app's setup: {error}"))
    })
    .await
    .map_err(|_| "the game app's setup stopped".to_string())?
}

/// Start a game in the game app and wait until it ends, or until the game
/// app says why it did not start one.
#[tauri::command]
pub(crate) async fn game_app_play(
    launch: GameAppLaunch,
    game_app: tauri::State<'_, GameApp>,
) -> Result<GameAppEnd, String> {
    let plugin = game_app.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        plugin
            .run_mobile_plugin::<serde_json::Value>("play", launch)
            .map(crate::game_app::end_from)
            .map_err(|error| format!("could not start the game app: {error}"))
    })
    .await
    .map_err(|_| "the game stopped answering".to_string())?
}
