package io.nyx.browser

import android.net.Uri
import android.util.Log
import android.webkit.WebView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import org.json.JSONException
import org.json.JSONObject

/**
 * Capture et pré-remplissage des mots de passe, comme sur PC :
 *
 * • capture : `assets/password_capture.js` (partagé avec le desktop et
 *   Windows) est injecté en fin de chargement dans le cadre principal des
 *   pages https ; il poste `{nyx:'password', …}` sur l'objet `nyx_password`
 *   de [WebViewCompat.addWebMessageListener] (cadres https seulement). Rust
 *   revalide l'origine de l'émetteur (fournie par la WebView) contre l'hôte
 *   de la page principale courante, puis on demande Oui/Non — rien n'est
 *   enregistré sans « Oui », ni si le réglage est coupé ;
 * • pré-remplissage : https, hôte exact, cadre principal, un seul compte
 *   (`nyx_core::vault_autofill::fill_script`, via [NyxVault.autofillScript]).
 *
 * Sans WEB_MESSAGE_LISTENER (WebView trop ancienne), pas de capture : pas
 * de repli `addJavascriptInterface`, exposé à tous les cadres.
 */
class PasswordCapture(private val activity: AppCompatActivity) {

    private val listenerSupported = WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)
    private var dialog: AlertDialog? = null

    private val captureJs: String? by lazy {
        try {
            val js = activity.assets.open("password_capture.js").bufferedReader(Charsets.UTF_8).use { it.readText() }
            // Injecté une seule fois par document (onPageFinished peut se répéter).
            "if (!window.__nyxPwCapture) { window.__nyxPwCapture = true;\n$js\n}"
        } catch (e: java.io.IOException) {
            Log.e(TAG, "password_capture.js introuvable", e)
            null
        }
    }

    /** À appeler avant le premier chargement de la WebView. */
    fun attach(webView: WebView) {
        if (!NyxVault.loaded || !WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) return
        val listener = WebViewCompat.WebMessageListener { view, message, sourceOrigin, isMainFrame, _ ->
            val data = message.data
            if (isMainFrame && data != null) onMessage(view, data, sourceOrigin)
        }
        try {
            WebViewCompat.addWebMessageListener(webView, CHANNEL, setOf("https://*"), listener)
        } catch (e: IllegalArgumentException) {
            // Règle refusée par une WebView ancienne : toutes origines, le
            // filtre https + hôte est de toute façon refait côté Rust.
            Log.w(TAG, "Règle d'origine https://* refusée, repli sur *", e)
            try {
                WebViewCompat.addWebMessageListener(webView, CHANNEL, setOf("*"), listener)
            } catch (e2: IllegalArgumentException) {
                Log.e(TAG, "Capture des mots de passe indisponible", e2)
            }
        }
    }

    /** Chargement principal terminé : script de capture + pré-remplissage. */
    fun onPageFinished(view: WebView, url: String?) {
        if (url == null || !url.startsWith("https://") || NewTabPage.isNewTab(url) || !VaultRepo.isOpen) return
        if (listenerSupported && VaultRepo.settings().savePasswords) {
            captureJs?.let { view.evaluateJavascript(it, null) }
        }
        val fill = NyxVault.autofillScript(url, false).orEmpty()
        if (fill.isNotEmpty()) view.evaluateJavascript(fill, null)
    }

    private fun onMessage(view: WebView, message: String, sourceOrigin: Uri) {
        if (!VaultRepo.isOpen || activity.isFinishing) return
        val topUrl = view.url ?: return
        val offer = NyxVault.captureOffer(sourceOrigin.toString(), topUrl, message, false).orEmpty()
        if (offer.isEmpty()) return
        val (host, question) = try {
            val o = JSONObject(offer)
            o.getString("host") to o.getString("question")
        } catch (e: JSONException) {
            return
        }
        dialog?.dismiss()
        var accepted = false
        val d = MaterialAlertDialogBuilder(activity)
            .setTitle(R.string.capture_title)
            .setMessage(question)
            .setPositiveButton(R.string.yes) { _, _ -> accepted = true }
            .setNegativeButton(R.string.no, null)
            .create()
        d.setOnDismissListener {
            if (dialog === d) dialog = null
            if (accepted) save(host, message)
        }
        dialog = d
        d.show()
    }

    private fun save(host: String, message: String) {
        val ok = VaultRepo.isOpen && NyxVault.captureSave(host, message)
        Toast.makeText(activity, if (ok) R.string.capture_saved else R.string.capture_failed, Toast.LENGTH_SHORT).show()
    }

    fun release() {
        dialog?.dismiss()
        dialog = null
    }

    private companion object {
        const val TAG = "NyxCapture"
        const val CHANNEL = "nyx_password"
    }
}
