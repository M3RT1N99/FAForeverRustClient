package com.faforever.rustclient

import android.app.Activity
import android.content.ClipData
import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class ShareArgs {
  lateinit var path: String
  lateinit var mime: String
  lateinit var title: String
}

// The client's logs leave Android through the share sheet, and a problem
// report says which device wrote them (src-tauri/src/android.rs).
@TauriPlugin
class DiagnosticsPlugin(private val activity: Activity) : Plugin(activity) {
  // The release, the API level, the maker and the model: nothing about the
  // player.
  @Command
  fun deviceInfo(invoke: Invoke) {
    val device = JSObject()
    device.put("release", Build.VERSION.RELEASE)
    device.put("sdk", Build.VERSION.SDK_INT)
    device.put("manufacturer", Build.MANUFACTURER)
    device.put("model", Build.MODEL)
    invoke.resolve(device)
  }

  // Hands a file in the app's cache to whatever the player picks: mail,
  // Drive, a messenger, Files. The game's logs go along once the game app
  // hands them over (gameLogs).
  @Command
  fun share(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(ShareArgs::class.java)
      val uris = arrayListOf(
        FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", File(args.path)),
      )
      gameLogs()?.let { uris.add(it) }
      val send = if (uris.size == 1) {
        Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, uris[0])
      } else {
        Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
      }
      send.type = args.mime
      // The grant travels with the clip, which is what reaches every target.
      send.clipData = ClipData.newRawUri(null, uris[0]).apply {
        uris.drop(1).forEach { addItem(ClipData.Item(it)) }
      }
      send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      activity.startActivity(Intent.createChooser(send, args.title))
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message)
    }
  }

  // The game app's results zip, as a content address with a read grant, once
  // its exported action for it exists (after E2b). Until then there is none.
  private fun gameLogs(): Uri? = null

  // The licences of everything the app is built from, which its build writes
  // into the APK's assets (scripts/android-notices); no text where a build
  // carries none.
  @Command
  fun notices(invoke: Invoke) {
    val answer = JSObject()
    try {
      activity.assets.open("NOTICE.txt").bufferedReader(Charsets.UTF_8).use { answer.put("text", it.readText()) }
    } catch (missing: java.io.IOException) {
      // The answer says so by having no text.
    }
    invoke.resolve(answer)
  }
}
