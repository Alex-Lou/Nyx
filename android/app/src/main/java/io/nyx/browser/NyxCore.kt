package io.nyx.browser

import android.util.Log

/**
 * Pont vers `nyx-core` (crate Rust `nyx-ffi`, `libnyx_ffi.so`).
 *
 * Les méthodes natives sont thread-safe côté Rust (état global `OnceLock`) :
 * [shouldBlock] est appelé depuis les threads réseau de la WebView.
 * Les façades `safe*` dégradent proprement si la bibliothèque n'a pas pu
 * être chargée (ABI absente) au lieu de faire planter l'app.
 */
object NyxCore {
    private const val TAG = "NyxCore"

    val loaded: Boolean = try {
        System.loadLibrary("nyx_ffi")
        true
    } catch (e: UnsatisfiedLinkError) {
        Log.e(TAG, "libnyx_ffi.so introuvable", e)
        false
    }

    @JvmStatic external fun initFilters(gzLists: Array<ByteArray>): Boolean
    @JvmStatic external fun filtersReady(): Boolean
    @JvmStatic external fun shouldBlock(url: String, sourceUrl: String, resourceType: String): Boolean
    @JvmStatic external fun resourceType(url: String, accept: String): String?
    @JvmStatic external fun resolveInput(text: String): String?
    @JvmStatic external fun isNavigable(url: String): Boolean
    @JvmStatic external fun renderPage(template: String): String?

    /** Barre d'adresse → URL ; `""` si l'entrée est refusée. */
    fun safeResolveInput(text: String): String {
        if (loaded) return resolveInput(text) ?: ""
        val t = text.trim()
        if (t.isEmpty()) return ""
        if (t.startsWith("https://") || t.startsWith("http://")) return t
        return "https://duckduckgo.com/?q=" + android.net.Uri.encode(t)
    }

    /** Décision de blocage d'une sous-ressource (jamais le document principal). */
    fun safeShouldBlock(url: String, sourceUrl: String, accept: String): Boolean {
        if (!loaded) return false
        val type = resourceType(url, accept) ?: "other"
        return shouldBlock(url, sourceUrl, type)
    }

    fun safeRenderPage(template: String): String =
        if (loaded) renderPage(template) ?: template else template
}
