package com.faforever.rustclient

import android.graphics.Color
import android.os.Bundle
import android.provider.Settings
import android.system.Os
import android.webkit.WebView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // Before the client starts (TauriActivity's onCreate). It finds its folders as a
    // Linux program does, under HOME, which an app's process does not have: the
    // app's own private folders take its place.
    Os.setenv("HOME", filesDir.absolutePath, true)
    Os.setenv("TMPDIR", cacheDir.absolutePath, true)
    // faf-uid, which the lobby's sign-in runs (crates/faf-app/src/infra/lobby_ws/android_uid.rs):
    // Android runs an app's own programs only from its native library folder, and faf-uid
    // reports the ANDROID_ID only Java can read.
    Os.setenv("FAF_NATIVE_LIBRARY_DIR", applicationInfo.nativeLibraryDir, true)
    Settings.Secure.getString(contentResolver, Settings.Secure.ANDROID_ID)?.let {
      Os.setenv("FAF_UID_ANDROID_ID", it, true)
    }
    // Light icons in both bars, always: the page and the window behind the bars
    // are dark, and the default would follow the system into dark icons
    // whenever it is in light mode.
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
    // From edge to edge the page would lie under the status and navigation bars
    // and the camera's cut-out, which a page made for a desktop window knows
    // nothing of: it is kept between them, and above the keyboard while one is up.
    ViewCompat.setOnApplyWindowInsetsListener(findViewById(android.R.id.content)) { view, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime())
      view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, keyboard.bottom))
      WindowInsetsCompat.CONSUMED
    }
  }

  // The page is laid out at the width its viewport names and drawn at the
  // scale it names, which App.tsx sets from the webview's own width
  // (shared/androidLayout): the makers' responsive layout at the phone's
  // width wherever every page fits it, and drawn smaller below that.
  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webView.settings.useWideViewPort = true
    webView.settings.loadWithOverviewMode = true
  }

  // Back from the browser: a client in front needs no service to keep it
  // running (BrowserWaitService). Here rather than in the plugin, because
  // Tauri never hands a plugin the app's onResume.
  override fun onResume() {
    super.onResume()
    BrowserWaitService.stop(this)
  }
}
