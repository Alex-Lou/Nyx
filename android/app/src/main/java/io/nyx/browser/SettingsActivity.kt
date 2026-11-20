package io.nyx.browser

import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.RadioGroup
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.widget.SwitchCompat

/**
 * Réglages Android, persistés dans le coffre (clé `app_settings`, même
 * format que Windows et le desktop) : moteur de recherche, page d'accueil,
 * bloqueur, enregistrement des mots de passe, mot de passe principal.
 * Chaque changement est enregistré aussitôt ; Rust l'applique à la barre
 * d'adresse et au bloqueur.
 */
class SettingsActivity : AppCompatActivity() {

    private lateinit var content: LinearLayout
    private var applying = false

    private lateinit var engines: RadioGroup
    private lateinit var home: EditText
    private lateinit var adblock: SwitchCompat
    private lateinit var savePasswords: SwitchCompat

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = NyxUi.root(this)
        root.addView(NyxUi.header(this, getString(R.string.settings_title)))
        content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        root.addView(
            ScrollView(this).apply { addView(content) },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f),
        )
        setContentView(root)
        build()
    }

    override fun onStart() {
        super.onStart()
        refresh()
    }

    override fun onPause() {
        saveHome()
        super.onPause()
    }

    private fun build() {
        val pad = NyxUi.dp(this, 16)

        content.addView(NyxUi.section(this, getString(R.string.settings_search)))
        engines = RadioGroup(this).apply {
            setPadding(pad, 0, pad, 0)
            for ((id, label) in ENGINES) {
                addView(RadioButton(this@SettingsActivity).apply {
                    text = label
                    tag = id
                    this.id = View.generateViewId()
                    setTextColor(NyxUi.color(this@SettingsActivity, R.color.nyx_text))
                })
            }
            setOnCheckedChangeListener { _, _ -> save() }
        }
        content.addView(engines)

        content.addView(NyxUi.section(this, getString(R.string.settings_home)))
        home = EditText(this).apply {
            setHint(R.string.settings_home_hint)
            setSingleLine(true)
            inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_URI
            imeOptions = EditorInfo.IME_ACTION_DONE
            importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO
            setBackgroundResource(R.drawable.address_bg)
            setTextColor(NyxUi.color(this@SettingsActivity, R.color.nyx_text))
            setHintTextColor(NyxUi.color(this@SettingsActivity, R.color.nyx_dim))
            setPadding(pad, 0, pad, 0)
            setOnEditorActionListener { _, action, _ ->
                if (action == EditorInfo.IME_ACTION_DONE) saveHome()
                false
            }
            setOnFocusChangeListener { _, focused -> if (!focused) saveHome() }
        }
        content.addView(
            home,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, NyxUi.dp(this, 44)).apply {
                setMargins(pad, 0, pad, 0)
            },
        )

        content.addView(NyxUi.section(this, getString(R.string.settings_privacy)))
        adblock = addSwitch(getString(R.string.settings_adblock))
        savePasswords = addSwitch(getString(R.string.settings_save_passwords))

        content.addView(NyxUi.section(this, getString(R.string.settings_master)))
        content.addView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            tag = MASTER_TAG
        })

        content.addView(NyxUi.section(this, getString(R.string.settings_sync)))
        content.addView(NyxUi.row(this).apply {
            alpha = 0.5f
            addView(
                NyxUi.twoLines(this@SettingsActivity, getString(R.string.settings_linked_devices), ""),
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
        })
    }

    private fun addSwitch(label: String): SwitchCompat {
        val s = SwitchCompat(this).apply {
            text = label
            textSize = 15f
            setTextColor(NyxUi.color(this@SettingsActivity, R.color.nyx_text))
            val pad = NyxUi.dp(this@SettingsActivity, 16)
            setPadding(pad, pad / 2, pad, pad / 2)
            setOnCheckedChangeListener { _, _ -> save() }
        }
        content.addView(s, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        return s
    }

    /** Recharge l'écran depuis le coffre. */
    private fun refresh() {
        val open = VaultRepo.isOpen
        val s = VaultRepo.settings()
        applying = true
        for (i in 0 until engines.childCount) {
            val b = engines.getChildAt(i) as RadioButton
            b.isChecked = b.tag == s.engine
            b.isEnabled = open
        }
        home.setText(if (s.home.startsWith("nyx://")) "" else s.home)
        home.isEnabled = open
        adblock.isChecked = s.adblock
        adblock.isEnabled = open
        savePasswords.isChecked = s.savePasswords
        savePasswords.isEnabled = open
        applying = false
        renderMaster(open, s.master)
    }

    private fun renderMaster(open: Boolean, master: Boolean) {
        val box = content.findViewWithTag<LinearLayout>(MASTER_TAG) ?: return
        box.removeAllViews()
        if (!open) {
            box.addView(NyxUi.placeholder(this, getString(R.string.vault_locked)))
            box.addView(NyxUi.textButton(this, getString(R.string.vault_unlock_title)) {
                VaultDialogs.unlock(this, null) { refresh() }
            })
            return
        }
        val desc = getString(if (master) R.string.settings_master_on else R.string.settings_master_off)
        box.addView(TextView(this).apply {
            text = desc
            textSize = 13f
            setTextColor(NyxUi.color(this@SettingsActivity, R.color.nyx_dim))
            val pad = NyxUi.dp(this@SettingsActivity, 16)
            setPadding(pad, 0, pad, 0)
        })
        val buttons = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(NyxUi.dp(this@SettingsActivity, 6), 0, 0, 0)
        }
        val done: () -> Unit = {
            Toast.makeText(this, R.string.settings_master_done, Toast.LENGTH_SHORT).show()
            refresh()
        }
        if (master) {
            buttons.addView(NyxUi.textButton(this, getString(R.string.vault_change_master)) {
                VaultDialogs.changeMaster(this, done)
            })
            buttons.addView(NyxUi.textButton(this, getString(R.string.vault_remove_master)) {
                VaultDialogs.removeMaster(this, done)
            })
        } else {
            buttons.addView(NyxUi.textButton(this, getString(R.string.vault_set_master)) {
                VaultDialogs.setMaster(this, done)
            })
        }
        box.addView(buttons)
    }

    private fun selectedEngine(): String {
        val checked = engines.findViewById<RadioButton>(engines.checkedRadioButtonId)
        return checked?.tag as? String ?: "ddg"
    }

    private fun save() {
        if (applying || !VaultRepo.isOpen) return
        val err = VaultRepo.updateSettings(
            selectedEngine(),
            home.text.toString(),
            adblock.isChecked,
            savePasswords.isChecked,
        )
        if (err.isNotEmpty()) {
            Toast.makeText(this, err, Toast.LENGTH_SHORT).show()
            refresh()
        }
    }

    private fun saveHome() {
        if (applying || !VaultRepo.isOpen) return
        val current = VaultRepo.settings().home
        val typed = home.text.toString().trim()
        val stored = if (current.startsWith("nyx://")) "" else current
        if (typed != stored) save()
    }

    private companion object {
        const val MASTER_TAG = "master"
        val ENGINES = listOf("ddg" to "DuckDuckGo", "brave" to "Brave Search", "ecosia" to "Ecosia")
    }
}
