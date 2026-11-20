package io.nyx.browser

import android.app.Dialog
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
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

    /** Dernière URL inscrite à l'historique par cet onglet (pas de doublon consécutif). */
    var lastRecorded: String = ""

    /**
     * Demandes de la page en attente de réponse (dialogue ou permission
     * Android). Toujours soldées — accord ou refus — puis oubliées, au plus
     * tard à la fermeture de l'onglet.
     */
    var permissionRequest: PermissionRequest? = null
    var geoCallback: GeolocationPermissions.Callback? = null
    var geoOrigin: String? = null
    var prompt: Dialog? = null
}
