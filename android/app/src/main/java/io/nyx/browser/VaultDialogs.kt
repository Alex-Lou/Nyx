package io.nyx.browser

import android.content.DialogInterface
import android.text.InputType
import android.view.View
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import com.google.android.material.dialog.MaterialAlertDialogBuilder

/**
 * Dialogues du coffre, partagés par [MainActivity] (bienvenue, déverrouillage)
 * et [SettingsActivity] (définir / changer / retirer le mot de passe
 * principal). Les champs sont vidés dès la réponse ; aucune passphrase
 * n'est conservée par l'UI.
 */
object VaultDialogs {

    /**
     * Démarrage : ouverture silencieuse avec la clé d'appareil, sinon
     * bienvenue (premier lancement) ou saisie du mot de passe principal.
     * `onReady` est appelé une fois le coffre ouvert.
     */
    fun start(activity: AppCompatActivity, onReady: () -> Unit) {
        VaultRepo.start(activity) { state ->
            if (activity.isFinishing || activity.isDestroyed) return@start
            when (state) {
                VaultRepo.Start.Opened -> onReady()
                VaultRepo.Start.FirstLaunch -> welcome(activity, onReady)
                VaultRepo.Start.NeedPassword -> unlock(activity, null, onReady)
                is VaultRepo.Start.DeviceKeyFailed ->
                    unlock(activity, activity.getString(R.string.vault_device_key_failed), onReady)
                VaultRepo.Start.Unavailable -> Unit
            }
        }
    }

    /** Premier lancement : « Plus tard » (clé d'appareil, défaut) ou mot de passe principal. */
    fun welcome(activity: AppCompatActivity, onReady: () -> Unit) {
        MaterialAlertDialogBuilder(activity)
            .setTitle(R.string.vault_welcome_title)
            .setMessage(R.string.vault_welcome_message)
            .setCancelable(false)
            .setPositiveButton(R.string.vault_later) { _, _ ->
                VaultRepo.createWithDeviceKey(activity) { err ->
                    if (err.isEmpty()) onReady() else retryWelcome(activity, err, onReady)
                }
            }
            .setNegativeButton(R.string.vault_set_master) { _, _ -> createMaster(activity, onReady) }
            .show()
    }

    private fun retryWelcome(activity: AppCompatActivity, err: String, onReady: () -> Unit) {
        if (activity.isFinishing) return
        Toast.makeText(activity, err, Toast.LENGTH_LONG).show()
        welcome(activity, onReady)
    }

    private fun createMaster(activity: AppCompatActivity, onReady: () -> Unit) {
        passphraseDialog(
            activity,
            title = activity.getString(R.string.vault_set_master),
            notice = activity.getString(R.string.vault_master_warning),
            fields = listOf(R.string.vault_new_password, R.string.vault_confirm_password),
            cancelLabel = R.string.vault_later,
            onCancel = { welcome(activity, onReady) },
        ) { values, finish ->
            val local = VaultRepo.checkNewPassphrase(values[0], values[1])
            if (local.isNotEmpty()) return@passphraseDialog finish(local)
            VaultRepo.createWithMaster(activity, values[0], values[1]) { err ->
                finish(err)
                if (err.isEmpty()) onReady()
            }
        }
    }

    /** Saisie du mot de passe principal (annulable : navigation sans coffre). */
    fun unlock(activity: AppCompatActivity, notice: String?, onReady: () -> Unit) {
        passphraseDialog(
            activity,
            title = activity.getString(R.string.vault_unlock_title),
            notice = notice ?: activity.getString(R.string.vault_unlock_message),
            fields = listOf(R.string.vault_master_password),
            cancelLabel = R.string.vault_skip,
            onCancel = {
                Toast.makeText(activity, R.string.vault_locked, Toast.LENGTH_SHORT).show()
            },
        ) { values, finish ->
            VaultRepo.unlock(activity, values[0]) { err ->
                finish(err)
                if (err.isEmpty()) onReady()
            }
        }
    }

    // ── Réglages : définir / changer / retirer ────────────────────────────

    fun setMaster(activity: AppCompatActivity, done: () -> Unit) {
        passphraseDialog(
            activity,
            title = activity.getString(R.string.vault_set_master),
            notice = activity.getString(R.string.vault_master_warning),
            fields = listOf(R.string.vault_new_password, R.string.vault_confirm_password),
        ) { values, finish ->
            val local = VaultRepo.checkNewPassphrase(values[0], values[1])
            if (local.isNotEmpty()) return@passphraseDialog finish(local)
            VaultRepo.setMaster(activity, values[0], values[1]) { err ->
                finish(err)
                if (err.isEmpty()) done()
            }
        }
    }

    fun changeMaster(activity: AppCompatActivity, done: () -> Unit) {
        passphraseDialog(
            activity,
            title = activity.getString(R.string.vault_change_master),
            notice = null,
            fields = listOf(R.string.vault_current_password, R.string.vault_new_password, R.string.vault_confirm_password),
        ) { values, finish ->
            val local = VaultRepo.checkNewPassphrase(values[1], values[2])
            if (local.isNotEmpty()) return@passphraseDialog finish(local)
            VaultRepo.changeMaster(values[0], values[1], values[2]) { err ->
                finish(err)
                if (err.isEmpty()) done()
            }
        }
    }

    fun removeMaster(activity: AppCompatActivity, done: () -> Unit) {
        passphraseDialog(
            activity,
            title = activity.getString(R.string.vault_remove_master),
            notice = activity.getString(R.string.vault_remove_message),
            fields = listOf(R.string.vault_current_password),
        ) { values, finish ->
            VaultRepo.removeMaster(activity, values[0]) { err ->
                finish(err)
                if (err.isEmpty()) done()
            }
        }
    }

    // ── Dialogue générique ────────────────────────────────────────────────

    /**
     * Champs mot de passe + message d'erreur. `submit` reçoit les valeurs et
     * une fonction `finish(err)` : `""` ferme le dialogue, sinon l'erreur
     * s'affiche et les champs sont vidés. Les boutons sont inactifs pendant
     * l'opération (PBKDF2 : jusqu'à une seconde).
     */
    private fun passphraseDialog(
        activity: AppCompatActivity,
        title: String,
        notice: String?,
        fields: List<Int>,
        cancelLabel: Int = android.R.string.cancel,
        onCancel: (() -> Unit)? = null,
        submit: (List<String>, (String) -> Unit) -> Unit,
    ) {
        val pad = (20 * activity.resources.displayMetrics.density).toInt()
        val layout = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad / 2, pad, 0)
        }
        if (notice != null) {
            layout.addView(TextView(activity).apply {
                text = notice
                setTextColor(ContextCompat.getColor(activity, R.color.nyx_dim))
                setPadding(0, 0, 0, pad / 2)
            })
        }
        val inputs = fields.map { hint ->
            EditText(activity).apply {
                setHint(hint)
                inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
                importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO
                setSingleLine(true)
                setTextColor(ContextCompat.getColor(activity, R.color.nyx_text))
                setHintTextColor(ContextCompat.getColor(activity, R.color.nyx_dim))
            }.also { layout.addView(it) }
        }
        val error = TextView(activity).apply {
            setTextColor(ContextCompat.getColor(activity, R.color.nyx_error))
            visibility = View.GONE
            setPadding(0, pad / 3, 0, 0)
        }
        layout.addView(error)

        var done = false
        val dialog = MaterialAlertDialogBuilder(activity)
            .setTitle(title)
            .setView(layout)
            .setCancelable(onCancel == null)
            .setPositiveButton(android.R.string.ok, null)
            .setNegativeButton(cancelLabel, null)
            .create()

        fun clear() = inputs.forEach { it.text.clear() }

        dialog.setOnShowListener {
            val ok = dialog.getButton(DialogInterface.BUTTON_POSITIVE)
            val cancel = dialog.getButton(DialogInterface.BUTTON_NEGATIVE)
            cancel.setOnClickListener {
                done = true
                clear()
                dialog.dismiss()
                onCancel?.invoke()
            }
            ok.setOnClickListener {
                val values = inputs.map { it.text.toString() }
                ok.isEnabled = false
                cancel.isEnabled = false
                error.visibility = View.GONE
                submit(values) finish@{ err ->
                    if (done || activity.isFinishing) return@finish
                    ok.isEnabled = true
                    cancel.isEnabled = true
                    if (err.isEmpty()) {
                        done = true
                        clear()
                        dialog.dismiss()
                    } else {
                        clear()
                        error.text = err
                        error.visibility = View.VISIBLE
                        inputs.firstOrNull()?.requestFocus()
                    }
                }
            }
        }
        dialog.setOnDismissListener { if (!done) clear() }
        dialog.show()
    }

    /** Confirmation simple (suppression…). */
    fun confirm(activity: AppCompatActivity, message: String, onYes: () -> Unit) {
        MaterialAlertDialogBuilder(activity)
            .setMessage(message)
            .setPositiveButton(R.string.yes) { _, _ -> onYes() }
            .setNegativeButton(R.string.no, null)
            .show()
    }
}
