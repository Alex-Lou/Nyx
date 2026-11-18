package io.nyx.browser

import android.content.Context
import android.net.Uri
import android.webkit.WebResourceResponse
import androidx.webkit.WebViewAssetLoader
import java.io.ByteArrayInputStream

/**
 * Page « Nouvel onglet » : `assets/newtab.html` du dépôt, servie par
 * [WebViewAssetLoader] sur une origine https dédiée (jamais `file://`), avec
 * la palette Nyx substituée au placeholder `{{TOKENS}}` (même rendu que le
 * desktop, via `nyx_core::theme`).
 *
 * Seul `newtab.html` est exposé : toute autre ressource de ce domaine
 * répond 404, pour qu'un site distant ne puisse pas sonder les assets.
 */
object NewTabPage {
    /** = `WebViewAssetLoader.DEFAULT_DOMAIN`. */
    const val HOST = "appassets.androidplatform.net"
    const val URL = "https://$HOST/newtab.html"

    @Volatile private var cached: ByteArray? = null

    fun isNewTab(url: String?): Boolean =
        url != null && url.startsWith("https://$HOST/")

    fun isAppAssets(uri: Uri): Boolean =
        uri.scheme == "https" && uri.host == HOST

    fun loader(context: Context): WebViewAssetLoader {
        val appContext = context.applicationContext
        return WebViewAssetLoader.Builder()
            .setDomain(HOST)
            .addPathHandler("/", WebViewAssetLoader.PathHandler { path ->
                if (path == "newtab.html") htmlResponse(appContext) else null
            })
            .build()
    }

    fun notFound(): WebResourceResponse =
        WebResourceResponse(
            "text/plain", "utf-8", 404, "Not Found",
            emptyMap(), ByteArrayInputStream(ByteArray(0)),
        )

    private fun htmlResponse(context: Context): WebResourceResponse {
        val bytes = cached ?: render(context).also { cached = it }
        return WebResourceResponse("text/html", "utf-8", ByteArrayInputStream(bytes))
    }

    private fun render(context: Context): ByteArray {
        val template = context.assets.open("newtab.html").bufferedReader(Charsets.UTF_8).use { it.readText() }
        return NyxCore.safeRenderPage(template).toByteArray(Charsets.UTF_8)
    }
}
