package com.faforever.rustclient

import android.graphics.Color
import android.os.Bundle
import android.os.SystemClock
import android.provider.Settings
import android.system.Os
import android.view.ViewGroup
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.lifecycle.Lifecycle
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import kotlin.math.roundToInt

/**
 * The most the system's font size grows the page's text. At 1.3 the shell's
 * and Replays' labels still fit their boxes on a phone lying down (checked
 * on the emulator at 1.0, 1.15 and 1.3); a larger font size is held there.
 */
private const val MAX_TEXT_SCALE = 1.3f

/**
 * The webview's text zoom, in percent, for the system's font scale: the font
 * scale rounded to a percent, up to MAX_TEXT_SCALE. Top-level so a JVM test
 * covers it (TextZoomTest).
 */
internal fun textZoomFor(fontScale: Float): Int = (minOf(fontScale, MAX_TEXT_SCALE) * 100).roundToInt()

/** What becomes of the page once its renderer process has ended. */
internal enum class PageAnswer {
  /** Built again at once: the activity is in front. */
  NOW,
  /** Built again once the activity is in front again: in the back, Android ends a new renderer as it ended this one. */
  IN_FRONT,
  /** Not built again: past the cap, the renderer's end is Android's to handle. */
  ANDROID,
}

/**
 * How long after onRestart an end of the renderer that is not a crash still
 * counts as one in the back (PageRecovery.endedInFront): long enough for a
 * slow phone to hand on an end it got as it thawed, short enough that an end
 * really in front seldom falls in it.
 */
private const val LATE_END_MILLIS = 1_000L

/**
 * The page's way back once its renderer process has ended, for one activity:
 * whether each end came in front, the answer to it, and whether a page that
 * waited is due now that the activity is in front again
 * (MainActivity.onRendererGone says why). Apart from the activity, so a JVM
 * test covers it (PageRecoveryTest). Times are SystemClock.elapsedRealtime's.
 */
internal class PageRecovery {
  private var waiting = false
  private var restartedAt: Long? = null

  /**
   * Whether the renderer ended in front: the activity was [started] when its
   * end came, at [now], unless that end is Android's, not [crashed], and came
   * within LATE_END_MILLIS of onRestart. Android freezes a client it keeps in
   * the back, and the end of a renderer it ended there reaches the client only
   * as it thaws, which can be just after onRestart, the activity started by
   * then. In front, that end would count toward the cap and recreate the
   * activity, which leaves Tauri's plugin manager without its launchers and
   * the page without Back. A crash stays in front, where the cap stops a page
   * that crashes as it is drawn.
   */
  fun endedInFront(started: Boolean, crashed: Boolean, now: Long): Boolean {
    val late = !crashed && restartedAt?.let { (now - it) in 0L..LATE_END_MILLIS } == true
    return started && !late
  }

  /** The renderer ended while the activity was [inFront] or not; [comesBack] is the core's answer, which holds the cap. */
  fun rendererGone(inFront: Boolean, comesBack: Boolean): PageAnswer = when {
    !comesBack -> PageAnswer.ANDROID
    inFront -> PageAnswer.NOW
    else -> {
      waiting = true
      PageAnswer.IN_FRONT
    }
  }

  /** The activity is coming to the front at [now]: whether a page that waited is built now. Once for each wait. */
  fun inFrontAgain(now: Long): Boolean {
    restartedAt = now
    return waiting.also { waiting = false }
  }
}

class MainActivity : TauriActivity() {
  private val page = PageRecovery()

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
  // client with it. So the WebView is let go and a new one built, whose page
  // loads and reads its state from the core, which never stopped, as at a
  // start. The Rust side logs it and caps the rebuilds in front
  // (src-tauri/src/android/renderer.rs); past the cap the answer is Android's
  // own again.
  //
  // In front the activity is built again at once, which wry and tao take as a
  // configuration change: they keep the window and its handlers and create
  // the WebView anew. In the back nothing more happens until the activity is
  // in front again (onRestart): behind a game Android ended each renderer
  // built there within two seconds, and the client with the fourth.
  //
  // An end from the back can reach a client that Android froze there only
  // once it is in front again; it still counts as one in the back
  // (PageRecovery.endedInFront). Its page then waits for the next return:
  // wry's setup cannot run again for an activity already started.
  fun onRendererGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
    val crashed = detail.didCrash()
    val started = lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)
    val inFront = page.endedInFront(started, crashed, SystemClock.elapsedRealtime())
    val comesBack = rendererGone(crashed, detail.rendererPriorityAtExit(), started, inFront)
    val answer = page.rendererGone(inFront, comesBack)
    if (answer == PageAnswer.ANDROID) return false
    (view.parent as? ViewGroup)?.removeView(view)
    view.destroy()
    if (answer == PageAnswer.NOW && !isFinishing && !isDestroyed) recreate()
    return true
  }

  // Coming back from the back, before the activity is started: the one moment
  // wry's setup may run again for it (rebuildPage, in renderer.rs), which
  // builds the page that waited in this same activity. Should it not get
  // under way, the activity is built again instead, as in front.
  override fun onRestart() {
    super.onRestart()
    if (page.inFrontAgain(SystemClock.elapsedRealtime()) && !rebuildPage()) recreate()
  }

  private external fun rendererGone(crashed: Boolean, priorityAtExit: Int, started: Boolean, inFront: Boolean): Boolean

  private external fun rebuildPage(): Boolean
}
