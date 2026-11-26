package game.zombiebox.mobile

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

/** Serves bundled atlas assets and Rust county data on one offline origin. */
class CountyActivity : Activity() {
    private lateinit var atlas: WebView
    private val origin = "https://atlas.zombiebox.invalid"
    private var currentSeed: String = "42"
    private var activeRun: Boolean = false
    private var playerX: Int = 0
    private var playerY: Int = 0
    private var playerFloor: Int = 0

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        currentSeed = sanitizeSeed(state?.getString("seed") ?: intent.getStringExtra("seed")) ?: "42"
        activeRun = state?.getBoolean("active", false) ?: intent.getBooleanExtra("active", false)
        playerX = state?.getInt("px", 0) ?: intent.getIntExtra("px", 0)
        playerY = state?.getInt("py", 0) ?: intent.getIntExtra("py", 0)
        playerFloor = state?.getInt("pf", 0) ?: intent.getIntExtra("pf", 0)

        atlas = WebView(this)
        atlas.settings.apply {
            javaScriptEnabled = true
            allowFileAccess = false
            allowContentAccess = false
        }
        atlas.addJavascriptInterface(AtlasBridge(), "ZombieboxAtlasBridge")
        atlas.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                val url = request.url
                if (url.scheme != "https" || url.host != "atlas.zombiebox.invalid") return true
                if (request.isForMainFrame && url.path == "/") finish()
                return url.path != "/county" && url.path != "/county.html"
            }

            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse {
                val url = request.url
                fun error(status: Int, reason: String) = WebResourceResponse(
                    "text/plain", "UTF-8", status, reason, emptyMap(), reason.byteInputStream()
                )
                if (url.scheme != "https" || url.host != "atlas.zombiebox.invalid") return error(404, "Unknown origin")
                val path = url.path.orEmpty()
                if (path.startsWith("/api/county/")) {
                    val value = path.removePrefix("/api/county/")
                    val seed = value.takeIf { it.isNotEmpty() && it.all { c -> c in '0'..'9' } }?.toULongOrNull()
                        ?: return error(400, "Invalid seed")
                    return WebResourceResponse("application/json", "UTF-8", NativeGame.countySurvey(seed.toLong()).inputStream())
                }
                val (file, mime) = when (path) {
                    "/county", "/county.html" -> "county.html" to "text/html"
                    "/county-map.js" -> "county-map.js" to "text/javascript"
                    "/county.js" -> "county.js" to "text/javascript"
                    "/county.css" -> "county.css" to "text/css"
                    else -> return error(404, "Unknown asset")
                }
                return WebResourceResponse(mime, "UTF-8", assets.open(file))
            }
        }
        setContentView(atlas)
        atlas.loadUrl(
            "$origin/county.html?seed=$currentSeed&active=${if (activeRun) 1 else 0}&px=$playerX&py=$playerY&pf=$playerFloor"
        )
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            WindowCompat.getInsetsController(window, window.decorView).apply {
                systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                hide(WindowInsetsCompat.Type.systemBars())
            }
        }
    }

    override fun onSaveInstanceState(state: Bundle) {
        super.onSaveInstanceState(state)
        val urlSeed = atlas.url?.let { Uri.parse(it).getQueryParameter("seed") }
        state.putString("seed", sanitizeSeed(urlSeed) ?: currentSeed)
        state.putBoolean("active", activeRun)
        state.putInt("px", playerX)
        state.putInt("py", playerY)
        state.putInt("pf", playerFloor)
    }

    override fun onDestroy() {
        atlas.destroy()
        super.onDestroy()
    }

    private fun sanitizeSeed(raw: String?): String? =
        raw?.trim()?.takeIf { it.isNotEmpty() && it.all { c -> c in '0'..'9' } }?.toULongOrNull()?.toString()

    private inner class AtlasBridge {
        @JavascriptInterface
        fun teleport(seed: String?, x: Int, y: Int, floor: Int) {
            val cleanSeed = sanitizeSeed(seed) ?: return
            runOnUiThread {
                setResult(
                    RESULT_OK,
                    Intent()
                        .putExtra("seed", cleanSeed)
                        .putExtra("x", x)
                        .putExtra("y", y)
                        .putExtra("floor", floor),
                )
                finish()
            }
        }

        @JavascriptInterface
        fun close() {
            runOnUiThread {
                setResult(RESULT_CANCELED)
                finish()
            }
        }
    }
}
