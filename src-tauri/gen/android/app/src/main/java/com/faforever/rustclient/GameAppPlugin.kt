package com.faforever.rustclient

import android.app.Activity
import android.app.PendingIntent
import android.content.ActivityNotFoundException
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.database.Cursor
import android.net.ConnectivityManager
import android.net.Uri
import android.os.Build
import androidx.activity.result.ActivityResult
import androidx.core.content.ContextCompat
import androidx.core.content.IntentCompat
import androidx.core.content.pm.PackageInfoCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.security.MessageDigest

// The FAF game app, io.github.m3rt1n99.fafre: the game this client starts on
// Android. Its launcher contract (its android/README.md, "The launcher's
// contract"):
// - the exported `.action.PLAY` starts a game for a result, for callers whose
//   signing certificate it allows, with the `.extra.*` values below;
// - the read-only provider `…status` answers `game_data` and `last_game`;
// - `.action.SETUP_GAME_DATA` opens its own screen that sets up the game data,
//   for any caller, with the step in `.extra.STEP`.
// Its updates come from its GitHub releases (the Rust side finds and
// downloads them) and are installed here, through Android's package installer.
private const val GAME_PACKAGE = "io.github.m3rt1n99.fafre"
private const val STATUS_AUTHORITY = "io.github.m3rt1n99.fafre.status"
private const val ACTION_PLAY = "io.github.m3rt1n99.fafre.action.PLAY"
private const val ACTION_SETUP = "io.github.m3rt1n99.fafre.action.SETUP_GAME_DATA"
private const val EXTRA = "io.github.m3rt1n99.fafre.extra."

// The game app's signing certificate, SHA-256: this PC's debug key, which
// signs its releases (its own Callers.ALLOWED names the same). An update
// signed by anything else is not installed.
private val GAME_SIGNERS = setOf("d6fc846e55052b8ac69985e5b0692a7a5756f9410533f2d0f3dbf6bfc7d3fe4a")

@InvokeArg
class SetupArgs {
  var step: String = "scfa"
}

@InvokeArg
class PlayArgs {
  var gameVersion: Int? = null
  var featuredMod: String? = null
  var arguments: Array<String>? = null
  var map: String? = null
  var mods: Array<String>? = null
  var modFiles: Array<String>? = null
  var rejoinOffer: Boolean = false
  var rejoinPeers: String? = null
  var rejoinSource: Int? = null
}

@InvokeArg
class InstallArgs {
  var path: String = ""
}

@TauriPlugin
class GameAppPlugin(private val activity: Activity) : Plugin(activity) {
  // Whether the game app is there, whether it has the launcher contract and
  // lets this client in, and its two status rows as they are.
  @Command
  fun status(invoke: Invoke) {
    val status = JSObject()
    val info = try {
      activity.packageManager.getPackageInfo(GAME_PACKAGE, 0)
    } catch (missing: PackageManager.NameNotFoundException) {
      null
    }
    status.put("installed", info != null)
    if (info == null) {
      invoke.resolve(status)
      return
    }
    status.put("versionName", info.versionName ?: "")
    status.put("versionCode", PackageInfoCompat.getLongVersionCode(info))
    status.put("silentUpdate", updatesSilently())
    // A game app from before the contract has no provider.
    if (activity.packageManager.resolveContentProvider(STATUS_AUTHORITY, 0) == null) {
      status.put("contract", "none")
      invoke.resolve(status)
      return
    }
    // A caller it does not allow gets an empty cursor.
    val gameData = row("game_data")
    status.put("contract", if (gameData == null) "refused" else "open")
    gameData?.let { status.put("gameData", it) }
    row("last_game")?.let { status.put("lastGame", it) }
    invoke.resolve(status)
  }

  // The game app's own screen for its game data: `scfa` (the player's
  // Supreme Commander) or `game_files` (FAF's newest). For a result, so the
  // answer comes when the player is back (ended); a job begun there goes on
  // in the game app's service, which its status shows.
  @Command
  fun setup(invoke: Invoke) {
    val args = invoke.parseArgs(SetupArgs::class.java)
    val intent = Intent(ACTION_SETUP).setPackage(GAME_PACKAGE).putExtra("${EXTRA}STEP", args.step)
    try {
      startActivityForResult(invoke, intent, "ended")
    } catch (missing: ActivityNotFoundException) {
      invoke.reject("the game app has no setup screen")
    }
  }

  // Start a game; the answer comes when it ends or fails to start (ended).
  @Command
  fun play(invoke: Invoke) {
    val args = invoke.parseArgs(PlayArgs::class.java)
    val intent = Intent(ACTION_PLAY).setPackage(GAME_PACKAGE)
    args.gameVersion?.let { intent.putExtra("${EXTRA}GAME_VERSION", it) }
    args.featuredMod?.let { intent.putExtra("${EXTRA}FEATURED_MOD", it) }
    args.arguments?.let { intent.putExtra("${EXTRA}LAUNCHER_ARGUMENTS", it) }
    args.map?.let { intent.putExtra("${EXTRA}MAP", it) }
    args.mods?.let { intent.putExtra("${EXTRA}MODS", it) }
    args.modFiles?.let { intent.putExtra("${EXTRA}MOD_FILES", it) }
    if (args.rejoinOffer) intent.putExtra("${EXTRA}REJOIN_OFFER", true)
    args.rejoinPeers?.let { intent.putExtra("${EXTRA}REJOIN_PEERS", it) }
    args.rejoinSource?.let { intent.putExtra("${EXTRA}REJOIN_SOURCE", it) }
    try {
      startActivityForResult(invoke, intent, "ended")
    } catch (missing: ActivityNotFoundException) {
      invoke.reject("the game app cannot be started from here")
    }
  }

  // How a start or the setup screen ended: `RESULT_FIRST_USER` with the game
  // app's `failure` code and `message` when it refused, else nothing more.
  @ActivityCallback
  fun ended(invoke: Invoke, result: ActivityResult) {
    val end = JSObject()
    end.put("resultCode", result.resultCode)
    result.data?.getStringExtra("failure")?.let { end.put("failure", it) }
    result.data?.getStringExtra("message")?.let { end.put("message", it) }
    invoke.resolve(end)
  }

  // Whether the phone is on a metered network now: mobile data, a hotspot, a
  // Wi-Fi set so. No network counts as metered: nothing downloads then.
  @Command
  fun network(invoke: Invoke) {
    val connectivity = activity.getSystemService(ConnectivityManager::class.java)
    val answer = JSObject()
    answer.put("metered", connectivity?.isActiveNetworkMetered ?: true)
    invoke.resolve(answer)
  }

  // Install a downloaded APK of the game app through Android's package
  // installer, once it names the game app, carries its signer and a higher
  // version code, and no game runs (the update ends the game app's process).
  // The answer comes when Android is done: at once where it updates without
  // asking, after its question otherwise.
  @Command
  fun installUpdate(invoke: Invoke) {
    val args = invoke.parseArgs(InstallArgs::class.java)
    val apk = File(args.path)
    val refusal = refusalOf(apk)
    if (refusal != null) {
      invoke.resolve(installFailed(refusal.first, refusal.second))
      return
    }
    try {
      commit(invoke, apk)
    } catch (error: Exception) {
      invoke.resolve(installFailed("INSTALL", error.message ?: error.toString()))
    }
  }

  // Android 12 and later update an app without asking whoever installed it
  // last, which this client becomes with its first update.
  private fun updatesSilently(): Boolean {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return false
    return try {
      activity.packageManager.getInstallSourceInfo(GAME_PACKAGE).installingPackageName == activity.packageName
    } catch (missing: PackageManager.NameNotFoundException) {
      false
    }
  }

  // Why `apk` is not to be installed, as a code and the detail, or null.
  private fun refusalOf(apk: File): Pair<String, String>? {
    val archive = archiveInfo(apk) ?: return "PACKAGE" to "not an APK"
    if (archive.packageName != GAME_PACKAGE) return "PACKAGE" to archive.packageName
    val signers = signersOf(archive)
    if (signers.isEmpty() || !GAME_SIGNERS.containsAll(signers)) {
      return "SIGNER" to signers.joinToString(",")
    }
    val installed = try {
      activity.packageManager.getPackageInfo(GAME_PACKAGE, 0)
    } catch (missing: PackageManager.NameNotFoundException) {
      return "PACKAGE" to "the game app is not installed"
    }
    val offered = PackageInfoCompat.getLongVersionCode(archive)
    val present = PackageInfoCompat.getLongVersionCode(installed)
    if (offered <= present) return "NOT_NEWER" to "$offered after $present"
    if (row("game_data")?.optLong("game_running", 0L) == 1L) return "RUNNING" to ""
    return null
  }

  private fun archiveInfo(apk: File): PackageInfo? {
    val manager = activity.packageManager
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      manager.getPackageArchiveInfo(
        apk.path,
        PackageManager.PackageInfoFlags.of(PackageManager.GET_SIGNING_CERTIFICATES.toLong()),
      )
    } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      @Suppress("DEPRECATION")
      manager.getPackageArchiveInfo(apk.path, PackageManager.GET_SIGNING_CERTIFICATES)
    } else {
      @Suppress("DEPRECATION")
      manager.getPackageArchiveInfo(apk.path, PackageManager.GET_SIGNATURES)
    }
  }

  // The SHA-256 of each certificate that signed the APK's contents now; a
  // rotated key's past ones are not asked for.
  private fun signersOf(info: PackageInfo): Set<String> {
    val signatures = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      info.signingInfo?.apkContentsSigners
    } else {
      @Suppress("DEPRECATION")
      info.signatures
    }
    val sha256 = MessageDigest.getInstance("SHA-256")
    return signatures.orEmpty().map { signature ->
      sha256.digest(signature.toByteArray()).joinToString("") { "%02x".format(it) }
    }.toSet()
  }

  // Write the APK into a session of Android's package installer and commit
  // it; its status comes back to a receiver of this session's alone.
  private fun commit(invoke: Invoke, apk: File) {
    val context = activity.applicationContext
    val installer = context.packageManager.packageInstaller
    val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
    params.setAppPackageName(GAME_PACKAGE)
    params.setSize(apk.length())
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
      params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      params.setPackageSource(PackageInstaller.PACKAGE_SOURCE_DOWNLOADED_FILE)
    }
    // A session of an earlier try that Android left open is dropped first:
    // allowing installs from this app ends its process, question and all.
    installer.mySessions
      .filter { it.appPackageName == GAME_PACKAGE }
      .forEach { runCatching { installer.abandonSession(it.sessionId) } }
    val id = installer.createSession(params)
    val action = "${context.packageName}.GAME_APP_INSTALL.$id"
    val receiver = object : BroadcastReceiver() {
      override fun onReceive(received: Context, intent: Intent) {
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
          // Android's question, the first time: its answer comes back here.
          val question = IntentCompat.getParcelableExtra(intent, Intent.EXTRA_INTENT, Intent::class.java)
          if (question != null) {
            activity.startActivity(question)
            return
          }
        }
        context.unregisterReceiver(this)
        if (status == PackageInstaller.STATUS_SUCCESS) {
          val answer = JSObject()
          answer.put("installed", true)
          invoke.resolve(answer)
        } else {
          val message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: ""
          invoke.resolve(installFailed(installFailure(status), message))
        }
      }
    }
    ContextCompat.registerReceiver(context, receiver, IntentFilter(action), ContextCompat.RECEIVER_NOT_EXPORTED)
    try {
      installer.openSession(id).use { session ->
        session.openWrite("game-app.apk", 0, apk.length()).use { out ->
          apk.inputStream().use { it.copyTo(out) }
          session.fsync(out)
        }
        val mutable = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0
        val result = PendingIntent.getBroadcast(
          context,
          id,
          Intent(action).setPackage(context.packageName),
          PendingIntent.FLAG_UPDATE_CURRENT or mutable,
        )
        session.commit(result.intentSender)
      }
    } catch (error: Exception) {
      context.unregisterReceiver(receiver)
      runCatching { installer.abandonSession(id) }
      throw error
    }
  }

  private fun installFailure(status: Int): String = when (status) {
    PackageInstaller.STATUS_FAILURE_ABORTED -> "ABORTED"
    PackageInstaller.STATUS_FAILURE_CONFLICT -> "CONFLICT"
    PackageInstaller.STATUS_FAILURE_STORAGE -> "STORAGE"
    PackageInstaller.STATUS_FAILURE_INCOMPATIBLE -> "INCOMPATIBLE"
    else -> "INSTALL"
  }

  private fun installFailed(failure: String, message: String): JSObject {
    val answer = JSObject()
    answer.put("installed", false)
    answer.put("failure", failure)
    answer.put("message", message)
    return answer
  }

  // One row of the status provider as `column: value`, or null when the
  // provider answered nothing (a caller it does not allow).
  private fun row(path: String): JSObject? {
    val uri = Uri.parse("content://$STATUS_AUTHORITY/$path")
    return try {
      activity.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
        if (!cursor.moveToFirst()) return@use null
        val values = JSObject()
        for (index in 0 until cursor.columnCount) {
          val name = cursor.getColumnName(index)
          when (cursor.getType(index)) {
            Cursor.FIELD_TYPE_INTEGER -> values.put(name, cursor.getLong(index))
            Cursor.FIELD_TYPE_FLOAT -> values.put(name, cursor.getDouble(index))
            Cursor.FIELD_TYPE_STRING -> values.put(name, cursor.getString(index))
            else -> values.put(name, null as Any?)
          }
        }
        values
      }
    } catch (refused: SecurityException) {
      null
    }
  }
}
