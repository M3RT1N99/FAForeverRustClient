package com.faforever.rustclient

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.database.Cursor
import android.net.Uri
import androidx.activity.result.ActivityResult
import androidx.core.content.pm.PackageInfoCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

// The FAF game app, io.github.m3rt1n99.fafre: the game this client starts on
// Android. Its launcher contract (its android/README.md, "The launcher's
// contract"):
// - the exported `.action.PLAY` starts a game for a result, for callers whose
//   signing certificate it allows, with the `.extra.*` values below;
// - the read-only provider `…status` answers `game_data` and `last_game`;
// - `.action.SETUP_GAME_DATA` opens its own screen that sets up the game data,
//   for any caller, with the step in `.extra.STEP`.
private const val GAME_PACKAGE = "io.github.m3rt1n99.fafre"
private const val STATUS_AUTHORITY = "io.github.m3rt1n99.fafre.status"
private const val ACTION_PLAY = "io.github.m3rt1n99.fafre.action.PLAY"
private const val ACTION_SETUP = "io.github.m3rt1n99.fafre.action.SETUP_GAME_DATA"
private const val EXTRA = "io.github.m3rt1n99.fafre.extra."

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
