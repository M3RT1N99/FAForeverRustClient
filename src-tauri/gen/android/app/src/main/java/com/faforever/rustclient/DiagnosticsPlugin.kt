package com.faforever.rustclient

import android.app.Activity
import android.app.ActivityManager
import android.app.usage.UsageStatsManager
import android.content.ClipData
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import androidx.core.content.FileProvider
import androidx.core.os.BundleCompat
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

  // What Android may hold against the client in the back, where a phone run
  // saw its network gone (src-tauri/src/android.rs logs it once a start):
  // Data Saver, the battery optimisation, the battery saver, the background
  // restriction a player can set (API 28) and the app's standby bucket (API
  // 28). Settings of the device and this app: nothing about the player.
  @Command
  fun backgroundLimits(invoke: Invoke) {
    val limits = JSObject()
    val network = activity.getSystemService(ConnectivityManager::class.java)
    limits.put(
      "dataSaver",
      when (network.restrictBackgroundStatus) {
        ConnectivityManager.RESTRICT_BACKGROUND_STATUS_ENABLED -> "on"
        ConnectivityManager.RESTRICT_BACKGROUND_STATUS_WHITELISTED -> "on, this app exempt"
        else -> "off"
      },
    )
    val power = activity.getSystemService(PowerManager::class.java)
    limits.put("batteryOptimized", !power.isIgnoringBatteryOptimizations(activity.packageName))
    limits.put("batterySaver", power.isPowerSaveMode)
    if (Build.VERSION.SDK_INT >= 28) {
      limits.put("backgroundRestricted", activity.getSystemService(ActivityManager::class.java).isBackgroundRestricted)
      val bucket = activity.getSystemService(UsageStatsManager::class.java).appStandbyBucket
      limits.put(
        "standbyBucket",
        when (bucket) {
          UsageStatsManager.STANDBY_BUCKET_ACTIVE -> "active"
          UsageStatsManager.STANDBY_BUCKET_WORKING_SET -> "working set"
          UsageStatsManager.STANDBY_BUCKET_FREQUENT -> "frequent"
          UsageStatsManager.STANDBY_BUCKET_RARE -> "rare"
          // From Android 11; a constant, so naming it costs older ones nothing.
          UsageStatsManager.STANDBY_BUCKET_RESTRICTED -> "restricted"
          else -> bucket.toString()
        },
      )
    }
    invoke.resolve(limits)
  }

  // Hands a file in the app's cache to whatever the player picks: mail,
  // Drive, a messenger, Files, with the game app's logs beside it where it
  // gives them (gameLogs). Off the main thread: the game app writes its zip
  // before it answers, which takes seconds for a long engine log.
  @Command
  fun share(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(ShareArgs::class.java)
    } catch (error: Exception) {
      invoke.reject(error.message)
      return
    }
    Thread {
      try {
        val uris = arrayListOf(
          FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", File(args.path)),
        )
        val gameLogs = gameLogs()
        val gameZip = gameLogs?.let { BundleCompat.getParcelable(it, "uri", Uri::class.java) }
        gameZip?.let { uris.add(it) }
        val send = if (uris.size == 1) {
          Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, uris[0])
        } else {
          Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
        }
        send.type = args.mime
        // The grant travels with the clip, which is what reaches every target;
        // the game app lets this app hand its zip on so.
        send.clipData = ClipData.newRawUri(null, uris[0]).apply {
          uris.drop(1).forEach { addItem(ClipData.Item(it)) }
        }
        send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        val chooser = Intent.createChooser(send, args.title)
        // The game app's reason, where it gave one for writing no zip.
        val shared = JSObject()
        if (gameZip == null) gameLogs?.getString("message")?.let { shared.put("gameLogsMissing", it) }
        activity.runOnUiThread {
          try {
            activity.startActivity(chooser)
            invoke.resolve(shared)
          } catch (error: Exception) {
            invoke.reject(error.message)
          }
        }
      } catch (error: Exception) {
        invoke.reject(error.message)
      }
    }.start()
  }

  // The game app's answer for the same report (its contract, "The logs"): its
  // logs zipped and scrubbed, as a content address it has let this app read
  // and hand on (`uri`), or why it could write no zip (`message`). No answer
  // where it gives none: not installed, from before the call, or a caller it
  // does not allow. Its zip lasts until the next call, so the share hands it
  // on at once.
  private fun gameLogs(): Bundle? = try {
    activity.contentResolver.call(Uri.parse("content://$STATUS_AUTHORITY"), "game_logs", null, null)
  } catch (refused: SecurityException) {
    null
  } catch (unknown: IllegalArgumentException) {
    null
  }

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
