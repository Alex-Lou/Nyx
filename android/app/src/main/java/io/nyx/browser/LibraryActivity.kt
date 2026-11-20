package io.nyx.browser

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.PersistableBundle
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_WEAK
import androidx.biometric.BiometricManager.Authenticators.DEVICE_CREDENTIAL
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.core.widget.doAfterTextChanged
import java.text.DateFormat
import java.util.Date

/**
 * Favoris / Historique / Mots de passe (un écran, trois onglets). Un appui
 * sur un favori ou une visite renvoie son URL à [MainActivity].
 *
 * Mots de passe : masqués par défaut ; « Afficher » et « Copier » exigent
 * le verrouillage de l'appareil (BiometricPrompt, biométrie ou code) quand
 * il est configuré. Révélés jusqu'à ce que l'écran passe en arrière-plan.
 */
class LibraryActivity : AppCompatActivity() {

    private lateinit var list: LinearLayout
    private lateinit var tabsBar: LinearLayout
    private lateinit var search: EditText
    private lateinit var clearButton: TextView
    private var mode = MODE_BOOKMARKS

    /** Identifiants révélés (id → mot de passe), oubliés en arrière-plan. */
    private val revealed = mutableMapOf<Long, String>()
    private var pendingAuth: (() -> Unit)? = null
    private lateinit var prompt: BiometricPrompt

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        mode = savedInstanceState?.getString(KEY_MODE) ?: intent.getStringExtra(EXTRA_MODE) ?: MODE_BOOKMARKS

        prompt = BiometricPrompt(this, ContextCompat.getMainExecutor(this), object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                val next = pendingAuth
                pendingAuth = null
                next?.invoke()
            }

            override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                pendingAuth = null
            }
        })

        val root = NyxUi.root(this)
        clearButton = NyxUi.textButton(this, getString(R.string.history_clear)) { confirmClearHistory() }
        root.addView(NyxUi.header(this, getString(R.string.library_title), clearButton))

        tabsBar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(NyxUi.dp(this@LibraryActivity, 8), 0, NyxUi.dp(this@LibraryActivity, 8), 0)
        }
        for ((key, label) in listOf(
            MODE_BOOKMARKS to R.string.bookmarks_title,
            MODE_HISTORY to R.string.history_title,
            MODE_PASSWORDS to R.string.passwords_title,
        )) {
            tabsBar.addView(
                NyxUi.textButton(this, getString(label)) { switchMode(key) }.apply { tag = key },
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
        }
        root.addView(tabsBar)

        search = EditText(this).apply {
            setHint(R.string.history_search_hint)
            setSingleLine(true)
            imeOptions = EditorInfo.IME_ACTION_SEARCH
            importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO
            setBackgroundResource(R.drawable.address_bg)
            setTextColor(NyxUi.color(this@LibraryActivity, R.color.nyx_text))
            setHintTextColor(NyxUi.color(this@LibraryActivity, R.color.nyx_dim))
            val pad = NyxUi.dp(this@LibraryActivity, 14)
            setPadding(pad, 0, pad, 0)
            doAfterTextChanged { if (mode == MODE_HISTORY) render() }
        }
        root.addView(
            search,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, NyxUi.dp(this, 44)).apply {
                val m = NyxUi.dp(this@LibraryActivity, 12)
                setMargins(m, m / 2, m, m / 2)
            },
        )

        list = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        root.addView(
            ScrollView(this).apply { addView(list) },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f),
        )
        setContentView(root)
    }

    /** Données reçues d'un autre appareil : liste relue. */
    private val onSynced: (SyncRepo.Last) -> Unit = { if (it.changed) render() }

    override fun onStart() {
        super.onStart()
        SyncRepo.addListener(onSynced)
        render()
    }

    override fun onStop() {
        SyncRepo.removeListener(onSynced)
        // Plus rien de révélé quand l'écran n'est plus visible.
        revealed.clear()
        super.onStop()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putString(KEY_MODE, mode)
    }

    private fun switchMode(next: String) {
        if (next == mode) return
        mode = next
        revealed.clear()
        render()
    }

    private fun render() {
        for (i in 0 until tabsBar.childCount) {
            val tab = tabsBar.getChildAt(i) as TextView
            val color = if (tab.tag == mode) R.color.nyx_aurora else R.color.nyx_dim
            tab.setTextColor(NyxUi.color(this, color))
        }
        search.visibility = if (mode == MODE_HISTORY) View.VISIBLE else View.GONE
        clearButton.visibility = if (mode == MODE_HISTORY && VaultRepo.isOpen) View.VISIBLE else View.GONE
        list.removeAllViews()
        if (!VaultRepo.isOpen) {
            list.addView(NyxUi.placeholder(this, getString(R.string.vault_locked)))
            list.addView(NyxUi.textButton(this, getString(R.string.vault_unlock_title)) {
                VaultDialogs.unlock(this, null) { render() }
            })
            return
        }
        when (mode) {
            MODE_HISTORY -> renderHistory()
            MODE_PASSWORDS -> renderPasswords()
            else -> renderBookmarks()
        }
    }

    private fun renderBookmarks() {
        val items = VaultRepo.bookmarks()
        if (items.isEmpty()) return list.addView(NyxUi.placeholder(this, getString(R.string.bookmarks_empty)))
        for (b in items) {
            val row = NyxUi.row(this)
            val text = NyxUi.twoLines(this, b.title.ifBlank { b.url }, b.url)
            text.setOnClickListener { openUrl(b.url) }
            row.addView(text, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            row.addView(NyxUi.iconButton(this, R.drawable.ic_close, getString(R.string.delete)) {
                if (NyxVault.bookmarkDelete(b.id)) render()
            })
            list.addView(row)
        }
    }

    private fun renderHistory() {
        val items = VaultRepo.history(search.text.toString())
        if (items.isEmpty()) return list.addView(NyxUi.placeholder(this, getString(R.string.history_empty)))
        val format = DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT)
        for (h in items) {
            val row = NyxUi.row(this)
            val subtitle = format.format(Date(h.visitedAt * 1000)) + " · " + h.url
            val text = NyxUi.twoLines(this, h.title.ifBlank { h.url }, subtitle)
            text.setOnClickListener { openUrl(h.url) }
            row.addView(text, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            list.addView(row)
        }
    }

    private fun renderPasswords() {
        val items = VaultRepo.logins()
        if (items.isEmpty()) return list.addView(NyxUi.placeholder(this, getString(R.string.passwords_empty)))
        for (p in items) {
            val row = NyxUi.row(this)
            val secret = revealed[p.id]
            val subtitle = (p.username.ifEmpty { "—" }) + "  ·  " + (secret ?: MASK)
            row.addView(
                NyxUi.twoLines(this, p.domain, subtitle),
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
            val toggle = if (secret == null) R.string.password_show else R.string.password_hide
            row.addView(NyxUi.textButton(this, getString(toggle)) {
                if (revealed.remove(p.id) != null) {
                    render()
                } else {
                    authenticated { reveal(p) }
                }
            })
            row.addView(NyxUi.textButton(this, getString(R.string.password_copy)) { authenticated { copy(p) } })
            row.addView(NyxUi.iconButton(this, R.drawable.ic_close, getString(R.string.delete)) {
                VaultDialogs.confirm(this, getString(R.string.password_delete_confirm, p.domain)) {
                    revealed.remove(p.id)
                    if (NyxVault.passwordDelete(p.id)) render()
                }
            })
            list.addView(row)
        }
    }

    private fun reveal(p: VaultRepo.Login) {
        val pw = NyxVault.passwordReveal(p.domain, p.id) ?: return
        revealed[p.id] = pw
        render()
    }

    private fun copy(p: VaultRepo.Login) {
        val pw = NyxVault.passwordReveal(p.domain, p.id) ?: return
        val clip = ClipData.newPlainText(p.domain, pw)
        // Android 13+ : masqué dans l'aperçu du presse-papiers.
        clip.description.extras = PersistableBundle().apply { putBoolean(EXTRA_IS_SENSITIVE, true) }
        (getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(clip)
        Toast.makeText(this, R.string.password_copied, Toast.LENGTH_SHORT).show()
    }

    /**
     * Exige le verrouillage de l'appareil s'il est configuré ; sinon (aucun
     * code ni biométrie), l'action est directe — comme sur PC.
     */
    private fun authenticated(action: () -> Unit) {
        val authenticators = BIOMETRIC_WEAK or DEVICE_CREDENTIAL
        if (BiometricManager.from(this).canAuthenticate(authenticators) != BiometricManager.BIOMETRIC_SUCCESS) {
            action()
            return
        }
        pendingAuth = action
        val info = BiometricPrompt.PromptInfo.Builder()
            .setTitle(getString(R.string.password_auth_title))
            .setSubtitle(getString(R.string.password_auth_subtitle))
            .setAllowedAuthenticators(authenticators)
            .build()
        prompt.authenticate(info)
    }

    private fun confirmClearHistory() {
        VaultDialogs.confirm(this, getString(R.string.history_clear_confirm)) {
            if (NyxVault.historyClear()) {
                Toast.makeText(this, R.string.history_cleared, Toast.LENGTH_SHORT).show()
            }
            render()
        }
    }

    private fun openUrl(url: String) {
        setResult(RESULT_OK, Intent().putExtra(EXTRA_URL, url))
        finish()
    }

    companion object {
        const val EXTRA_MODE = "io.nyx.browser.library.MODE"
        const val EXTRA_URL = "io.nyx.browser.library.URL"
        const val MODE_BOOKMARKS = "bookmarks"
        const val MODE_HISTORY = "history"
        const val MODE_PASSWORDS = "passwords"
        private const val KEY_MODE = "mode"
        private const val MASK = "••••••••"

        /** = `ClipDescription.EXTRA_IS_SENSITIVE` (API 33), clé lue telle quelle. */
        private const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
    }
}
