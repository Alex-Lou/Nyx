package io.nyx.browser

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject
import java.io.File
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * Coffre chiffré côté Kotlin : chemins, clé d'appareil (Keystore), et
 * enchaînements sûrs des changements de clé — même ordre que Windows
 * (`vault::keymode::KeyStore`). Toute décision et toute donnée passent par
 * [NyxVault] (Rust).
 *
 * Les opérations lentes (PBKDF2 de SQLCipher, rechiffrement) tournent sur
 * un fil dédié ; les résultats reviennent sur le thread UI.
 */
object VaultRepo {
    private const val TAG = "NyxVault"
    private const val UNAVAILABLE = "Coffre indisponible sur cet appareil"

    private val io: ExecutorService = Executors.newSingleThreadExecutor { r -> Thread(r, "nyx-vault") }
    private val main = Handler(Looper.getMainLooper())

    fun dbFile(context: Context): File = File(context.filesDir, "vault.db")
    fun keyFile(context: Context): File = File(context.filesDir, "vault.key.enc")

    val isOpen: Boolean get() = NyxVault.loaded && NyxVault.isOpen()
    val hasMaster: Boolean get() = isOpen && NyxVault.hasMasterPassword()

    /** État au démarrage. */
    sealed class Start {
        object Opened : Start()
        object FirstLaunch : Start()
        object NeedPassword : Start()
        /** Fichier de clé présent mais inutilisable : on n'efface rien, on demande. */
        data class DeviceKeyFailed(val reason: String) : Start()
        object Unavailable : Start()
    }

    private fun <T> background(work: () -> T, done: (T) -> Unit) {
        io.execute {
            val result = work()
            main.post { done(result) }
        }
    }

    /** Écriture sans retour (historique) : hors du thread UI. */
    fun async(work: () -> Unit) {
        if (!NyxVault.loaded) return
        io.execute {
            try {
                work()
            } catch (t: Throwable) {
                Log.w(TAG, "Écriture du coffre impossible", t)
            }
        }
    }

    // ── Démarrage ─────────────────────────────────────────────────────────

    fun start(context: Context, done: (Start) -> Unit) {
        if (!NyxVault.loaded) return done(Start.Unavailable)
        val app = context.applicationContext
        background({ startNow(app) }, done)
    }

    private fun startNow(context: Context): Start {
        if (NyxVault.isOpen()) return Start.Opened
        val db = dbFile(context)
        val keyFile = keyFile(context)
        return when (NyxVault.startup(db.path, keyFile.exists())) {
            "device" -> {
                val key = try {
                    KeyStoreWrap.read(keyFile)
                } catch (e: Exception) {
                    Log.w(TAG, "Clé d'appareil illisible", e)
                    return Start.DeviceKeyFailed(e.message ?: "clé illisible")
                }
                val err = try {
                    NyxVault.openWithDeviceKey(db.path, key).orEmpty()
                } finally {
                    key.fill(0)
                }
                if (err.isEmpty()) Start.Opened else Start.DeviceKeyFailed(err)
            }
            "master" -> Start.NeedPassword
            else -> Start.FirstLaunch
        }
    }

    /** Fichier de clé sans coffre : mis de côté (jamais supprimé). */
    private fun setAsideOrphanKey(context: Context) {
        val keyFile = keyFile(context)
        if (keyFile.exists() && !dbFile(context).exists()) {
            keyFile.renameTo(File(keyFile.path + ".orphan"))
        }
    }

    /** Premier lancement, « Plus tard » : clé d'appareil écrite ET relue avant la création. */
    fun createWithDeviceKey(context: Context, done: (String) -> Unit) {
        if (!NyxVault.loaded) return done(UNAVAILABLE)
        val app = context.applicationContext
        background({
            setAsideOrphanKey(app)
            val key = NyxVault.generateDeviceKey() ?: return@background "Générateur aléatoire indisponible"
            try {
                KeyStoreWrap.writeChecked(keyFile(app), key)
                val err = NyxVault.createWithDeviceKey(dbFile(app).path, key).orEmpty()
                if (err.isNotEmpty()) KeyStoreWrap.delete(keyFile(app))
                err
            } catch (e: Exception) {
                Log.w(TAG, "Création de la clé d'appareil impossible", e)
                "Keystore Android indisponible : ${e.message ?: e.javaClass.simpleName}"
            } finally {
                key.fill(0)
            }
        }, done)
    }

    /** Premier lancement avec un mot de passe principal (aucun fichier de clé). */
    fun createWithMaster(context: Context, passphrase: String, confirm: String, done: (String) -> Unit) {
        if (!NyxVault.loaded) return done(UNAVAILABLE)
        val app = context.applicationContext
        background({
            setAsideOrphanKey(app)
            NyxVault.createWithPassphrase(dbFile(app).path, passphrase, confirm).orEmpty()
        }, done)
    }

    fun unlock(context: Context, passphrase: String, done: (String) -> Unit) {
        if (!NyxVault.loaded) return done(UNAVAILABLE)
        val app = context.applicationContext
        background({ NyxVault.openWithPassphrase(dbFile(app).path, passphrase).orEmpty() }, done)
    }

    fun checkNewPassphrase(passphrase: String, confirm: String): String =
        if (NyxVault.loaded) NyxVault.checkNewPassphrase(passphrase, confirm).orEmpty() else UNAVAILABLE

    // ── Mot de passe principal (réglages) ────────────────────────────────

    /**
     * Clé d'appareil → mot de passe principal. Rust rechiffre et vérifie
     * (retour à la clé d'appareil en cas d'échec) ; le fichier de clé n'est
     * supprimé qu'APRÈS ce succès.
     */
    fun setMaster(context: Context, passphrase: String, confirm: String, done: (String) -> Unit) {
        if (!isOpen) return done(UNAVAILABLE)
        val app = context.applicationContext
        background({
            val keyFile = keyFile(app)
            val current = try {
                KeyStoreWrap.read(keyFile)
            } catch (e: Exception) {
                return@background "Clé d'appareil illisible : ${e.message ?: e.javaClass.simpleName}"
            }
            val err = try {
                NyxVault.setMaster(passphrase, confirm, current).orEmpty()
            } finally {
                current.fill(0)
            }
            when {
                err.isNotEmpty() -> err
                !KeyStoreWrap.delete(keyFile) ->
                    "Mot de passe défini, mais ${keyFile.name} n'a pas pu être supprimé"
                else -> ""
            }
        }, done)
    }

    fun changeMaster(old: String, passphrase: String, confirm: String, done: (String) -> Unit) {
        if (!isOpen) return done(UNAVAILABLE)
        background({ NyxVault.changeMaster(old, passphrase, confirm).orEmpty() }, done)
    }

    /**
     * Mot de passe principal → nouvelle clé d'appareil. Ordre : mot de passe
     * vérifié, fichier de clé écrit et relu, PUIS rechiffrement vérifié ;
     * tout échec retire le fichier écrit (Rust restaure le mot de passe).
     */
    fun removeMaster(context: Context, current: String, done: (String) -> Unit) {
        if (!isOpen) return done(UNAVAILABLE)
        val app = context.applicationContext
        background({
            val wrong = NyxVault.verifyMaster(current).orEmpty()
            if (wrong.isNotEmpty()) return@background wrong
            val keyFile = keyFile(app)
            val key = NyxVault.generateDeviceKey() ?: return@background "Générateur aléatoire indisponible"
            try {
                KeyStoreWrap.writeChecked(keyFile, key)
                val err = NyxVault.removeMaster(current, key).orEmpty()
                if (err.isNotEmpty()) KeyStoreWrap.delete(keyFile)
                err
            } catch (e: Exception) {
                Log.w(TAG, "Écriture de la clé d'appareil impossible", e)
                "Keystore Android indisponible : ${e.message ?: e.javaClass.simpleName}"
            } finally {
                key.fill(0)
            }
        }, done)
    }

    // ── Réglages ─────────────────────────────────────────────────────────

    data class AppSettings(
        val engine: String,
        val home: String,
        val adblock: Boolean,
        val savePasswords: Boolean,
        val master: Boolean,
    )

    fun settings(): AppSettings {
        val o = parseObject(if (isOpen) NyxVault.settingsJson() else null)
        return AppSettings(
            engine = o.optString("engine", "ddg"),
            home = o.optString("home", "nyx://newtab"),
            adblock = o.optBoolean("adblock", true),
            savePasswords = o.optBoolean("savepw", true),
            master = o.optBoolean("master", false),
        )
    }

    /** `""` = enregistré. */
    fun updateSettings(engine: String, home: String, adblock: Boolean, savePasswords: Boolean): String =
        if (isOpen) NyxVault.settingsUpdate(engine, home, adblock, savePasswords).orEmpty() else UNAVAILABLE

    /** Page d'accueil des nouveaux onglets (`nyx://…` = Nouvel onglet). */
    fun homeUrl(): String = if (NyxVault.loaded) NyxVault.homeUrl().orEmpty() else ""

    // ── Données ──────────────────────────────────────────────────────────

    data class Bookmark(val id: Long, val url: String, val title: String)
    data class Visit(val id: Long, val url: String, val title: String, val visitedAt: Long)
    data class Login(val id: Long, val domain: String, val username: String)

    fun bookmarks(): List<Bookmark> = parseArray(if (isOpen) NyxVault.bookmarksJson() else null) {
        Bookmark(it.getLong("id"), it.getString("url"), it.optString("title"))
    }

    fun history(query: String): List<Visit> = parseArray(if (isOpen) NyxVault.historySearchJson(query) else null) {
        Visit(it.getLong("id"), it.getString("url"), it.optString("title"), it.optLong("visited_at"))
    }

    fun logins(): List<Login> = parseArray(if (isOpen) NyxVault.passwordsJson() else null) {
        Login(it.getLong("id"), it.getString("domain"), it.optString("username"))
    }

    fun bookmarkId(url: String): Long = if (isOpen) NyxVault.bookmarkFind(url) else -1L

    private fun parseObject(json: String?): JSONObject = try {
        JSONObject(json ?: "{}")
    } catch (e: JSONException) {
        JSONObject()
    }

    private fun <T> parseArray(json: String?, map: (JSONObject) -> T): List<T> = try {
        val array = JSONArray(json ?: "[]")
        (0 until array.length()).map { map(array.getJSONObject(it)) }
    } catch (e: JSONException) {
        Log.w(TAG, "Liste du coffre illisible", e)
        emptyList()
    }

    // ── Historique (navigations principales) ─────────────────────────────

    /** Navigation principale terminée/engagée : une visite par page et par onglet. */
    fun recordVisit(tab: BrowserTab, url: String, isReload: Boolean) {
        if (!isOpen || isReload || url == tab.lastRecorded) return
        val scheme = url.substringBefore(':').lowercase()
        if (scheme != "http" && scheme != "https") return
        if (NewTabPage.isNewTab(url)) return
        tab.lastRecorded = url
        val title = tab.title
        async { NyxVault.historyPush(url, title) }
    }

    /** Titre reçu après coup : reporté sur la visite enregistrée par cet onglet. */
    fun retitle(tab: BrowserTab, url: String, title: String) {
        if (!isOpen || title.isBlank() || url != tab.lastRecorded) return
        async { NyxVault.historyRetitle(url, title) }
    }
}
