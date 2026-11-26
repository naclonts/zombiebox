package game.zombiebox.mobile

import android.annotation.SuppressLint
import android.app.Activity
import android.os.Bundle
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

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        atlas = WebView(this)
        atlas.settings.apply {
            javaScriptEnabled = true
            allowFileAccess = false
            allowContentAccess = false
        }
        atlas.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                val url = request.url
                if (url.path == "/" || url.path == "/index.html") {
                    finish()
                    return true
                }
                return url.scheme != "https" || url.host != "atlas.zombiebox.invalid" ||
                    url.path !in setOf("/county", "/county.html")
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
        atlas.loadUrl("$origin/county.html?seed=${state?.getString("seed") ?: "42"}")
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
        state.putString("seed", atlas.url?.let { android.net.Uri.parse(it).getQueryParameter("seed") }
            ?.toULongOrNull()?.toString())
    }

    override fun onDestroy() {
        atlas.destroy()
        super.onDestroy()
    }
}
