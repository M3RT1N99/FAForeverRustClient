package com.faforever.rustclient

import android.graphics.Color
import android.os.Bundle
import android.provider.Settings
import android.system.Os
import android.view.ViewGroup
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import kotlin.math.roundToInt

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // Before the client starts (TauriActivity's onCreate). Its folders, HOME and TMPDIR,
    // it takes itself (adopt_app_folders in src-tauri/src/android.rs).
    // faf-uid, which the lobby's sign-in runs (crates/faf-app/src/infra/lobby_ws/android_uid.rs):
    // Android runs an app's own programs only from its native library folder, and faf-uid
    // reports the ANDROID_ID only Java can read.
    Os.setenv("FAF_NATIVE_LIBRARY_DIR", applicationInfo.nativeLibraryDir, true)
    Settings.Secure.getString(contentResolver, Settings.Secure.ANDROID_ID)?.let {
      Os.setenv("FAF_UID_ANDROID_ID", it, true)
    }
    // Light icons in both bars, always: the page is kept clear of the bars (below),
    // so what shows behind them is the window, which the app's theme makes dark
    // whatever the system's mode or the client's own theme. The default style
    // would follow the system into dark icons whenever it is in light mode.
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
  // scale it names, which the page sets from the webview's own size
  // (ui/src/shared/androidLayout): at 100 % times the interface scale,
  // whatever the font size.
  //
  // The system's font size grows the text alone, as Chrome grows a page's
  // text with it: through the webview's own text zoom, while the layout keeps
  // its size. Up to MAX_TEXT_SCALE, past which labels outgrow the boxes the
  // layout keeps for them; a smaller font is followed all the way down. The
  // font size and the zoom made of it are handed to the page before it loads,
  // for the line the client log gets for its layout (a document-start
  // script). The font size is read here once: a new one recreates the
  // activity, and with it this webview.
  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webView.settings.useWideViewPort = true
    webView.settings.loadWithOverviewMode = true
    val fontScale = resources.configuration.fontScale
    val textZoom = textZoomFor(fontScale)
    webView.settings.textZoom = textZoom
    if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
      WebViewCompat.addDocumentStartJavaScript(
        webView,
        "window.fafAndroidFontScale = $fontScale; window.fafAndroidTextZoom = $textZoom;",
        setOf("*"),
      )
    }
  }

  // Back from the browser: a client in front needs no service to keep it
  // running (BrowserWaitService). Here rather than in the plugin, because
  // Tauri never hands a plugin the app's onResume.
  override fun onResume() {
    super.onResume()
    BrowserWaitService.stop(this)
  }

  // The page's renderer process has ended: it crashed, or Android ended it for
  // its memory while the client was in the back, which a game in front makes
  // likely. Called by the WebView's client, the generated RustWebViewClient,
  // through the code BuildTask hands wry for it. A WebView whose renderer is
  // gone cannot be used again, and left unanswered Android ends the whole
  // client with it. So the WebView is let go and the activity built again,
  // which wry and tao take as a configuration change: they keep the window and
  // its handlers and create the WebView anew, whose page loads and reads its
  // state from the core, which never stopped, as at a start. The Rust side logs
  // it and caps the rebuilds (src-tauri/src/android/renderer.rs); past the cap
  // the answer is Android's own again.
  fun onRendererGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
    if (!rendererGone(detail.didCrash(), detail.rendererPriorityAtExit())) return false
    (view.parent as? ViewGroup)?.removeView(view)
    view.destroy()
    if (!isFinishing && !isDestroyed) recreate()
    return true
  }

  private external fun rendererGone(crashed: Boolean, priorityAtExit: Int): Boolean

  companion object {
    /**
     * The most the system's font size grows the page's text. At 1.3 the shell's
     * and Replays' labels still fit their boxes on a phone lying down (checked
     * on the emulator at 1.0, 1.15 and 1.3); a larger font size is held there.
     */
    private const val MAX_TEXT_SCALE = 1.3f

    /** The webview's text zoom, in percent, for the system's font scale. */
    private fun textZoomFor(fontScale: Float): Int = (minOf(fontScale, MAX_TEXT_SCALE) * 100).roundToInt()
  }
}
