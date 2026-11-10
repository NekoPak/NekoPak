package io.nekopak.desktop

import android.os.Bundle
import android.view.View
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
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
