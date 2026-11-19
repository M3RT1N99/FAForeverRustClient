package com.faforever.rustclient

import android.app.Activity
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

// Keeps the client running while the player is in the browser, where a sign-in
// waits on its redirect (src-tauri/src/android.rs). MainActivity ends it once
// the client is in front again.
@TauriPlugin
class BrowserWaitPlugin(private val activity: Activity) : Plugin(activity) {
  // Called just before the browser opens, while the client is still in front.
  @Command
  fun hold(invoke: Invoke) {
    try {
      BrowserWaitService.start(activity)
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message)
    }
  }

  // The browser did not open, so there is nothing to wait for.
  @Command
  fun release(invoke: Invoke) {
    BrowserWaitService.stop(activity)
    invoke.resolve()
  }
}
