package io.nyx.browser

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.text.InputFilter
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanIntentResult
import com.journeyapps.barcodescanner.ScanOptions
import java.text.DateFormat
import java.util.Date

/**
 * « Appareils liés » : nom de ce téléphone, appareils du profil (Retirer,
 * avec confirmation), association à un ordinateur (scan du QR affiché par
 * Nyx Windows, ou saisie du code `nyxpair:…`), puis comparaison du code à
 * 6 chiffres ; « Synchroniser maintenant » et état de la dernière
 * synchronisation. Toute la logique est en Rust ([NyxSync]).
 */
class DevicesActivity : AppCompatActivity() {

    private lateinit var content: LinearLayout
    private lateinit var nameField: EditText
    private lateinit var list: LinearLayout
    private lateinit var status: TextView
    private var progress: AlertDialog? = null
    /** Appairage en cours (code pas encore confirmé) : annulé si l'écran part. */
    private var pairing = false

    private val onSynced: (SyncRepo.Last) -> Unit = { refresh() }

    private val scan = registerForActivityResult(ScanContract()) { result: ScanIntentResult ->
        val text = result.contents
        if (text.isNullOrBlank()) return@registerForActivityResult
        startPairing(text)
    }

    private val cameraPermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) launchScan() else {
                Toast.makeText(this, R.string.devices_camera_denied, Toast.LENGTH_LONG).show()
                askCode()
            }
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = NyxUi.root(this)
        root.addView(NyxUi.header(this, getString(R.string.devices_title)))
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
        SyncRepo.addListener(onSynced)
        refresh()
    }

    override fun onStop() {
        SyncRepo.removeListener(onSynced)
        super.onStop()
    }

    override fun onDestroy() {
        if (pairing) SyncRepo.pairCancel()
        progress?.dismiss()
        super.onDestroy()
    }

    private fun pad(): Int = NyxUi.dp(this, 16)

    private fun text(value: String, size: Float = 13f, dim: Boolean = true): TextView = TextView(this).apply {
        text = value
        textSize = size
        setTextColor(NyxUi.color(this@DevicesActivity, if (dim) R.color.nyx_dim else R.color.nyx_text))
        setPadding(pad(), NyxUi.dp(this@DevicesActivity, 4), pad(), NyxUi.dp(this@DevicesActivity, 4))
    }

    private fun build() {
        content.addView(NyxUi.section(this, getString(R.string.devices_this_device)))
        nameField = EditText(this).apply {
            setSingleLine(true)
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            imeOptions = EditorInfo.IME_ACTION_DONE
            importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO
            filters = arrayOf<InputFilter>(InputFilter.LengthFilter(64))
            setBackgroundResource(R.drawable.address_bg)
            setTextColor(NyxUi.color(this@DevicesActivity, R.color.nyx_text))
            setHintTextColor(NyxUi.color(this@DevicesActivity, R.color.nyx_dim))
            setHint(R.string.devices_name_hint)
            setPadding(pad(), 0, pad(), 0)
            setOnEditorActionListener { _, action, _ ->
                if (action == EditorInfo.IME_ACTION_DONE) saveName()
                false
            }
        }
        val nameRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(pad(), 0, NyxUi.dp(this@DevicesActivity, 6), 0)
        }
        nameRow.addView(nameField, LinearLayout.LayoutParams(0, NyxUi.dp(this, 44), 1f))
        nameRow.addView(NyxUi.textButton(this, getString(R.string.devices_rename)) { saveName() })
        content.addView(nameRow)

        content.addView(NyxUi.section(this, getString(R.string.devices_list)))
        list = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        content.addView(list)

        content.addView(NyxUi.section(this, getString(R.string.devices_pair_section)))
        content.addView(text(getString(R.string.devices_pair_help)))
        val pairButtons = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(NyxUi.dp(this@DevicesActivity, 6), 0, 0, 0)
        }
        pairButtons.addView(NyxUi.textButton(this, getString(R.string.devices_pair_scan)) { scanQr() })
        pairButtons.addView(NyxUi.textButton(this, getString(R.string.devices_pair_code)) { askCode() })
        content.addView(pairButtons)

        content.addView(NyxUi.section(this, getString(R.string.settings_sync)))
        status = text("")
        content.addView(status)
        content.addView(text(getString(R.string.devices_sync_help)))
        content.addView(NyxUi.textButton(this, getString(R.string.devices_sync_now)) { syncNow() }.apply {
            gravity = Gravity.START or Gravity.CENTER_VERTICAL
            setPadding(pad(), NyxUi.dp(this@DevicesActivity, 10), pad(), NyxUi.dp(this@DevicesActivity, 10))
        })
    }

    // ── Affichage ────────────────────────────────────────────────────────

    private fun refresh() {
        if (!VaultRepo.isOpen) {
            list.removeAllViews()
            list.addView(NyxUi.placeholder(this, getString(R.string.vault_locked)))
            status.text = ""
            return
        }
        SyncRepo.devices { devices -> if (!isFinishing && !isDestroyed) render(devices) }
        status.text = describe(SyncRepo.lastSync())
    }

    private fun describe(last: SyncRepo.Last?): String {
        if (last == null || last.atMs == 0L) return getString(R.string.devices_never_synced)
        val at = DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT).format(Date(last.atMs))
        return getString(R.string.devices_last_sync, at, last.message)
    }

    private fun render(devices: List<SyncRepo.Device>) {
        val me = devices.firstOrNull { it.me }
        if (!nameField.hasFocus()) {
            val name = me?.name.orEmpty()
            nameField.setText(if (name.isEmpty() || name == "Nyx") defaultName() else name)
        }
        list.removeAllViews()
        val others = devices.filter { !it.me && !it.revoked }
        if (others.isEmpty()) {
            list.addView(text(getString(R.string.devices_none)))
            return
        }
        val dateFormat = DateFormat.getDateInstance(DateFormat.MEDIUM)
        for (d in others) {
            val row = NyxUi.row(this)
            val added = if (d.addedAt > 0) getString(R.string.devices_added, dateFormat.format(Date(d.addedAt))) else ""
            row.addView(
                NyxUi.twoLines(this, d.name.ifEmpty { getString(R.string.devices_unnamed) }, added),
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
            row.addView(NyxUi.textButton(this, getString(R.string.devices_remove)) { confirmRevoke(d) })
            list.addView(row)
        }
    }

    private fun defaultName(): String =
        listOf(Build.MANUFACTURER, Build.MODEL).filter { !it.isNullOrBlank() }.distinct().joinToString(" ")
            .replaceFirstChar { it.uppercase() }
            .ifEmpty { getString(R.string.devices_default_name) }

    private fun toast(message: String) {
        if (message.isNotEmpty()) Toast.makeText(this, message, Toast.LENGTH_LONG).show()
    }

    // ── Nom, retrait, synchronisation ────────────────────────────────────

    private fun saveName() {
        val name = nameField.text.toString().trim()
        if (name.isEmpty() || !VaultRepo.isOpen) return
        SyncRepo.rename(name) { err ->
            toast(err.ifEmpty { getString(R.string.devices_renamed) })
            nameField.clearFocus()
            refresh()
        }
    }

    private fun confirmRevoke(device: SyncRepo.Device) {
        MaterialAlertDialogBuilder(this)
            .setTitle(getString(R.string.devices_remove_title, device.name))
            .setMessage(R.string.devices_remove_message)
            .setNegativeButton(android.R.string.cancel, null)
            .setPositiveButton(R.string.devices_remove) { _, _ ->
                SyncRepo.revoke(device.id) { err ->
                    toast(err.ifEmpty { getString(R.string.devices_removed) })
                    refresh()
                }
            }
            .show()
    }

    private fun syncNow() {
        if (!VaultRepo.isOpen) return
        if (!SyncRepo.onWifi(this)) toast(getString(R.string.devices_no_wifi))
        status.text = getString(R.string.devices_syncing)
        SyncRepo.syncNow(this) { last -> toast(last.message) }
    }

    // ── Appairage ────────────────────────────────────────────────────────

    private fun scanQr() {
        if (!VaultRepo.isOpen) return
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
            launchScan()
        } else {
            cameraPermission.launch(Manifest.permission.CAMERA)
        }
    }

    private fun launchScan() {
        val options = ScanOptions()
            .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
            .setPrompt(getString(R.string.devices_scan_prompt))
            .setBeepEnabled(false)
            .setOrientationLocked(false)
        try {
            scan.launch(options)
        } catch (e: RuntimeException) {
            toast(getString(R.string.devices_scan_unavailable))
            askCode()
        }
    }

    /** Repli sans caméra : coller la chaîne `nyxpair:v1:…` affichée sous le QR. */
    private fun askCode() {
        if (!VaultRepo.isOpen) return
        val field = EditText(this).apply {
            setHint(R.string.devices_code_hint)
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or
                InputType.TYPE_TEXT_FLAG_MULTI_LINE
            importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO
            minLines = 2
        }
        val box = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(NyxUi.dp(this@DevicesActivity, 20), NyxUi.dp(this@DevicesActivity, 8), NyxUi.dp(this@DevicesActivity, 20), 0)
            addView(field)
        }
        MaterialAlertDialogBuilder(this)
            .setTitle(R.string.devices_pair_code)
            .setMessage(R.string.devices_code_message)
            .setView(box)
            .setNegativeButton(android.R.string.cancel, null)
            .setPositiveButton(R.string.devices_pair_go) { _, _ ->
                val code = field.text.toString().filterNot { it.isWhitespace() }
                if (code.isNotEmpty()) startPairing(code)
            }
            .show()
    }

    private fun showProgress(message: String, cancellable: Boolean) {
        progress?.dismiss()
        val box = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(NyxUi.dp(this@DevicesActivity, 24), NyxUi.dp(this@DevicesActivity, 20), NyxUi.dp(this@DevicesActivity, 24), NyxUi.dp(this@DevicesActivity, 8))
            addView(ProgressBar(this@DevicesActivity))
            addView(TextView(this@DevicesActivity).apply {
                text = message
                textSize = 15f
                setPadding(NyxUi.dp(this@DevicesActivity, 16), 0, 0, 0)
            })
        }
        val builder = MaterialAlertDialogBuilder(this).setView(box).setCancelable(false)
        if (cancellable) {
            builder.setNegativeButton(android.R.string.cancel) { _, _ ->
                SyncRepo.pairCancel()
                pairing = false
            }
        }
        progress = builder.show()
    }

    private fun hideProgress() {
        progress?.dismiss()
        progress = null
    }

    private fun alive(): Boolean = !isFinishing && !isDestroyed

    private fun startPairing(offer: String) {
        if (pairing) return
        pairing = true
        showProgress(getString(R.string.devices_connecting), cancellable = true)
        SyncRepo.pairBegin(offer.trim(), nameField.text.toString().trim().ifEmpty { defaultName() }) { result ->
            if (!alive()) return@pairBegin
            hideProgress()
            when (result) {
                is SyncRepo.PairResult.Confirm -> confirmCode(result)
                is SyncRepo.PairResult.Done -> finished(result.pc)
                is SyncRepo.PairResult.Error -> failed(result.message)
            }
        }
    }

    private fun confirmCode(c: SyncRepo.PairResult.Confirm) {
        if (!pairing) {
            // Annulé pendant la connexion : le refus a déjà été transmis.
            SyncRepo.pairConfirm(false) { }
            return
        }
        val code = if (c.code.length == 6) c.code.substring(0, 3) + " " + c.code.substring(3) else c.code
        val big = TextView(this).apply {
            text = code
            textSize = 34f
            gravity = Gravity.CENTER
            letterSpacing = 0.12f
            setTextColor(NyxUi.color(this@DevicesActivity, R.color.nyx_aurora))
            setPadding(0, NyxUi.dp(this@DevicesActivity, 16), 0, NyxUi.dp(this@DevicesActivity, 8))
        }
        MaterialAlertDialogBuilder(this)
            .setTitle(R.string.devices_confirm_title)
            .setMessage(getString(R.string.devices_confirm_message, c.pc))
            .setView(big)
            .setCancelable(false)
            .setNegativeButton(android.R.string.cancel) { _, _ -> answer(false) }
            .setPositiveButton(R.string.devices_codes_match) { _, _ -> answer(true) }
            .show()
    }

    private fun answer(yes: Boolean) {
        if (yes) showProgress(getString(R.string.devices_finishing), cancellable = false)
        SyncRepo.pairConfirm(yes) { result ->
            pairing = false
            if (!alive()) return@pairConfirm
            hideProgress()
            when (result) {
                is SyncRepo.PairResult.Done -> finished(result.pc)
                is SyncRepo.PairResult.Error -> if (yes) failed(result.message) else toast(getString(R.string.devices_pair_cancelled))
                is SyncRepo.PairResult.Confirm -> failed(getString(R.string.devices_pair_failed))
            }
        }
    }

    private fun finished(pc: String) {
        pairing = false
        refresh()
        MaterialAlertDialogBuilder(this)
            .setTitle(R.string.devices_paired_title)
            .setMessage(getString(R.string.devices_paired_message, pc))
            .setPositiveButton(android.R.string.ok, null)
            .show()
        // Première synchronisation tout de suite (le PC est sur ce Wi-Fi).
        status.text = getString(R.string.devices_syncing)
        SyncRepo.syncNow(this) { last -> toast(last.message) }
    }

    private fun failed(message: String) {
        pairing = false
        MaterialAlertDialogBuilder(this)
            .setTitle(R.string.devices_pair_failed)
            .setMessage(message)
            .setPositiveButton(android.R.string.ok, null)
            .show()
    }
}
