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

// The client's process while the player is in the browser. Android freezes a
// process whose page another app covers within seconds, and a sign-in waiting
// there on its redirect would answer the browser only once the player came
// back. A short foreground service keeps it running: Android ends one after
// about three minutes, and the client ends it as soon as the player is back
// (MainActivity.onResume). BrowserWaitPlugin starts it.
class BrowserWaitService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val notification = NotificationCompat.Builder(this, CHANNEL)
      .setSmallIcon(R.drawable.ic_stat_browser_wait)
      .setContentTitle(getString(R.string.browser_wait_title))
      .setContentText(getString(R.string.browser_wait_text))
      .setPriority(NotificationCompat.PRIORITY_LOW)
      .setOngoing(true)
      .build()
    // Before Android 14 there is no short service, and a service needs no type.
    val type = if (Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_SHORT_SERVICE else 0
    ServiceCompat.startForeground(this, NOTIFICATION, notification, type)
    return START_NOT_STICKY
  }

  // Android's three minutes are up, and the service has to stop at once.
  // Android 14 calls the first, later versions the second.
  override fun onTimeout(startId: Int) = stopSelf()

  override fun onTimeout(startId: Int, fgsType: Int) = stopSelf()

  companion object {
    private const val CHANNEL = "browser_wait"
    private const val NOTIFICATION = 1

    // Only while the client is in the foreground: Android starts a foreground
    // service for no other app.
    fun start(context: Context) {
      if (Build.VERSION.SDK_INT >= 26) {
        val channel = NotificationChannel(
          CHANNEL,
          context.getString(R.string.browser_wait_channel),
          NotificationManager.IMPORTANCE_LOW,
        )
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
      }
      ContextCompat.startForegroundService(context, Intent(context, BrowserWaitService::class.java))
    }

    fun stop(context: Context) {
      context.stopService(Intent(context, BrowserWaitService::class.java))
    }
  }
}
