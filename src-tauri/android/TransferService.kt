package io.nekopak.desktop

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat

/**
 * Keeps the process and CPU available while Rust moves local transfer bytes and
 * the activity is hidden. It carries no transfer state: Rust decides when it
 * starts and stops, and is told whenever Android or the user ends it first.
 */
class TransferService : Service() {
    companion object {
        private const val CHANNEL = "nekopak.transfers"
        private const val NOTIFICATION = 4201
        private const val ACTION_PAUSE = "io.nekopak.desktop.action.PAUSE_TRANSFERS"
        private const val EXTRA_ACTIVE = "active"
        private const val EXTRA_PERCENT = "percent"
        // Rust refreshes at least every 30 seconds. A stalled native worker
        // therefore cannot hold the CPU awake for longer than this.
        private const val WAKE_MS = 2 * 60 * 1000L
        private const val ENDED_PAUSE = 1
        private const val ENDED_TIMEOUT = 2
        private const val ENDED_REMOVED = 3

        @Volatile private var instance: TransferService? = null

        @JvmStatic external fun nativeEnded(reason: Int)

        /** False when Android refuses a start, e.g. from the background. */
        @JvmStatic
        fun start(context: Context, active: Int, percent: Int): Boolean = try {
            ContextCompat.startForegroundService(
                context,
                Intent(context, TransferService::class.java).putExtra(EXTRA_ACTIVE, active).putExtra(EXTRA_PERCENT, percent),
            )
            MainActivity.requestNotifications()
            true
        } catch (_: Exception) {
            false
        }

        @JvmStatic
        fun update(context: Context, active: Int, percent: Int): Boolean {
            val service = instance ?: return false
            service.renewWakeLock()
            context.getSystemService(NotificationManager::class.java).notify(NOTIFICATION, service.notification(active, percent))
            return true
        }

        @JvmStatic
        fun stop(context: Context): Boolean {
            instance?.requested = true
            return context.stopService(Intent(context, TransferService::class.java))
        }

        /** The activity is finishing for good; do not outlive its WebView. */
        @JvmStatic
        fun activityFinished() {
            instance?.end(ENDED_REMOVED)
        }
    }

    /** True once Rust asked for the stop or has already been told about it. */
    @Volatile private var requested = false
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        if (Build.VERSION.SDK_INT >= 26) {
            val channel = NotificationChannel(CHANNEL, getString(R.string.nekopak_transfer_channel), NotificationManager.IMPORTANCE_LOW)
            channel.setShowBadge(false)
            getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        }
        wakeLock = getSystemService(PowerManager::class.java)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "NekoPak:transfers").apply { setReferenceCounted(false) }
        instance = this
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_PAUSE) {
            end(ENDED_PAUSE)
            return START_NOT_STICKY
        }
        try {
            val type = if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC else 0
            ServiceCompat.startForeground(
                this, NOTIFICATION,
                notification(intent?.getIntExtra(EXTRA_ACTIVE, 1) ?: 1, intent?.getIntExtra(EXTRA_PERCENT, -1) ?: -1),
                type,
            )
            renewWakeLock()
        } catch (_: Exception) {
            end(ENDED_REMOVED)
        }
        // Transfers are persisted by Rust. A killed process is never revived
        // here without its native state.
        return START_NOT_STICKY
    }

    /** Android 15 limits dataSync services to a daily time budget. */
    override fun onTimeout(startId: Int, fgsType: Int) = end(ENDED_TIMEOUT)

    override fun onTaskRemoved(rootIntent: Intent?) = end(ENDED_REMOVED)

    override fun onDestroy() {
        if (instance === this) instance = null
        wakeLock?.let { if (it.isHeld) it.release() }
        if (!requested) notifyEnded(ENDED_REMOVED)
        super.onDestroy()
    }

    private fun end(reason: Int) {
        if (requested) return
        notifyEnded(reason)
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun notifyEnded(reason: Int) {
        requested = true
        // The native library is loaded by the activity. Without it there is no
        // running transfer to pause either.
        try { nativeEnded(reason) } catch (_: Throwable) {}
    }

    private fun renewWakeLock() {
        try { wakeLock?.acquire(WAKE_MS) } catch (_: Exception) {}
    }

    private fun notification(active: Int, percent: Int): Notification {
        val immutable = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        val builder = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.nekopak_stat_transfer)
            .setContentTitle(getString(R.string.nekopak_transfer_title))
            .setContentText(resources.getQuantityString(R.plurals.nekopak_transfer_active, active, active))
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setProgress(100, percent.coerceIn(0, 100), percent !in 0..100)
            .addAction(
                0, getString(R.string.nekopak_transfer_pause),
                PendingIntent.getService(this, 1, Intent(this, TransferService::class.java).setAction(ACTION_PAUSE), immutable),
            )
        packageManager.getLaunchIntentForPackage(packageName)?.let {
            builder.setContentIntent(PendingIntent.getActivity(this, 0, it, immutable))
        }
        return builder.build()
    }
}
