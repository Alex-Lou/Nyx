package io.nyx.browser

import android.webkit.WebView

/** Un onglet = une WebView + l'état affiché dans la barre et le sélecteur. */
class BrowserTab(val webView: WebView) {
    /**
     * URL du document principal, lue depuis les threads réseau
     * (`shouldInterceptRequest`) — les méthodes de WebView, elles, ne sont
     * appelables que sur le thread UI.
     */
    @Volatile var pageUrl: String = ""
    var title: String = ""
    var progress: Int = 100
}
