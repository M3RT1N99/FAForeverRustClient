//! What the client needs from Android itself, through its own Kotlin in
//! `gen/android`.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;
use tauri_plugin_opener::OpenerExt;

/// The app's Kotlin package, and the plugin's class in it.
const APP_PACKAGE: &str = "com.faforever.rustclient";
const PLUGIN_CLASS: &str = "BrowserWaitPlugin";

/// Sign-in's browser.
///
/// Android has no launcher command for a URL, only the intent the opener
/// plugin sends. It also freezes a process whose page the browser covers,
/// which would leave the sign-in's redirect unanswered until the player came
/// back. So the client is first kept running (a short foreground service the
/// Kotlin ends once the player is back), then the page opens in a Custom Tab,
/// which closing returns from to the client.
pub(crate) fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("browser-wait")
        .setup(|app, api| {
            let wait = api.register_android_plugin(APP_PACKAGE, PLUGIN_CLASS)?;
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
