package com.faforever.rustclient

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat

// The client's process while a game runs in the FAF game app. The client holds
// the lobby's connection and the game's GPGNet link, or serves a live replay's
// stream, on the loopback, while the game app is in front; Android freezes a
// process in the back within seconds and may end it. A special-use foreground
// service keeps it running for as long as the game does: GameAppPlugin starts
// it right before the game's start, and ends it with the start's answer or
// once the client stops waiting for the game. No time limit, unlike the
// browser's short service: a game lasts an hour.
class GameWaitService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val notification = NotificationCompat.Builder(this, CHANNEL)
      .setSmallIcon(R.drawable.ic_stat_game_wait)
      .setContentTitle(getString(R.string.game_wait_title))
      .setContentText(getString(R.string.game_wait_text))
      .setPriority(NotificationCompat.PRIORITY_LOW)
      .setOngoing(true)
      .build()
    // Before Android 14 there is no special-use type, and a service needs none.
    val type = if (Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE else 0
    ServiceCompat.startForeground(this, NOTIFICATION, notification, type)
    return START_NOT_STICKY
  }

  companion object {
    private const val CHANNEL = "game_wait"
    // BrowserWaitService's is 1.
    private const val NOTIFICATION = 2

    // Only while the client is in the foreground: Android starts a foreground
    // service for no other app.
    fun start(context: Context) {
      if (Build.VERSION.SDK_INT >= 26) {
        val channel = NotificationChannel(
          CHANNEL,
          context.getString(R.string.game_wait_channel),
          NotificationManager.IMPORTANCE_LOW,
        )
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
      }
      ContextCompat.startForegroundService(context, Intent(context, GameWaitService::class.java))
    }

    fun stop(context: Context) {
      context.stopService(Intent(context, GameWaitService::class.java))
    }
  }
}
