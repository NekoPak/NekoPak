package io.nekopak.desktop

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.view.View
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import java.lang.ref.WeakReference

class MainActivity : TauriActivity() {
    companion object {
        private const val NOTIFICATION_REQUEST = 4202
        @Volatile private var current = WeakReference<MainActivity>(null)

        /**
         * Asks once for the progress notification. Background transfers do
         * not depend on the answer; a refusal only hides the notification.
         */
        fun requestNotifications() {
            val activity = current.get() ?: return
            if (Build.VERSION.SDK_INT < 33 ||
                activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) return
            val host = activity.getSharedPreferences("nekopak_host", Context.MODE_PRIVATE)
            if (host.getBoolean("notifications_requested", false)) return
            host.edit().putBoolean("notifications_requested", true).apply()
            activity.runOnUiThread {
                if (!activity.isFinishing) activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), NOTIFICATION_REQUEST)
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        current = WeakReference(this)
    }

    override fun onDestroy() {
        // A finished activity takes its WebView with it. Transfers are paused
        // durably instead of running without any visible owner.
        if (isFinishing) TransferService.activityFinished()
        if (current.get() === this) current = WeakReference(null)
        super.onDestroy()
    }

    override fun onWebViewCreate(webView: WebView) {
        super.onWebViewCreate(webView)
        // Fit the WebView inside system bars and the IME. CSS also respects
        // safe-area insets on WebViews that supply them.
        // Padding the WebView itself does not change its fixed-position CSS
        // viewport. Resize its content container so onboarding and dialogs also
        // stay below status/cutout bars and above the keyboard.
        val content = findViewById<View>(android.R.id.content)
        ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime())
            view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, keyboard.bottom))
            val visible = insets.isVisible(WindowInsetsCompat.Type.ime())
            webView.evaluateJavascript(
                "window.__NEKOPAK_KEYBOARD_VISIBLE__=$visible;window.dispatchEvent(new CustomEvent('nekopak:keyboard',{detail:{visible:$visible}}));",
                null,
            )
            insets
        }
        ViewCompat.requestApplyInsets(content)
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (ViewCompat.getRootWindowInsets(content)?.isVisible(WindowInsetsCompat.Type.ime()) == true) {
                    WindowInsetsControllerCompat(window, content).hide(WindowInsetsCompat.Type.ime())
                    return
                }
                webView.evaluateJavascript(
                    "(function(){const e=new CustomEvent('nekopak:back',{cancelable:true});return !window.dispatchEvent(e);})()",
                ) { handled ->
                    if (handled != "true") {
                        isEnabled = false
                        onBackPressedDispatcher.onBackPressed()
                        isEnabled = true
                    }
                }
            }
        })
    }
}
