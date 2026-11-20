package io.nyx.browser

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject
import java.util.concurrent.CopyOnWriteArraySet
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * Appareils liés côté Kotlin : fils d'arrière-plan, verrou multicast Wi-Fi
 * pendant la découverte mDNS, détection du Wi-Fi, et diffusion « données
 * synchronisées » aux écrans ouverts. Toute décision passe par [NyxSync]
 * (Rust). Aucun service d'arrière-plan : synchronisation à l'ouverture /
 * au retour dans l'app (sur Wi-Fi) et à la demande.
 */
object SyncRepo {
    private const val TAG = "NyxSync"
    private const val UNAVAILABLE = "Synchronisation indisponible sur cet appareil"
    /** Pas plus d'une synchronisation automatique par minute. */
    private const val AUTO_INTERVAL_MS = 60_000L

    private val io: ExecutorService = Executors.newSingleThreadExecutor { r -> Thread(r, "nyx-sync") }
    private val pairIo: ExecutorService = Executors.newSingleThreadExecutor { r -> Thread(r, "nyx-pair") }
    private val main = Handler(Looper.getMainLooper())
    private val listeners = CopyOnWriteArraySet<(Last) -> Unit>()

    @Volatile private var lastAuto = 0L
    @Volatile private var syncing = false

    data class Device(val id: String, val name: String, val addedAt: Long, val revoked: Boolean, val me: Boolean)
    data class Last(val atMs: Long, val ok: Boolean, val message: String, val changed: Boolean)

    sealed class PairResult {
        data class Confirm(val code: String, val pc: String) : PairResult()
        data class Done(val pc: String) : PairResult()
        data class Error(val message: String) : PairResult()
    }

    private val available: Boolean get() = NyxSync.loaded && VaultRepo.isOpen

    private fun <T> background(executor: ExecutorService, work: () -> T, done: (T) -> Unit) {
        executor.execute {
            val result = work()
            main.post { done(result) }
        }
    }

    /** Écoute des synchronisations terminées (thread UI). */
    fun addListener(l: (Last) -> Unit) { listeners.add(l) }
    fun removeListener(l: (Last) -> Unit) { listeners.remove(l) }

    // ── Réseau ───────────────────────────────────────────────────────────

    /** Réseau actif = Wi-Fi (la synchronisation est réservée au réseau local). */
    fun onWifi(context: Context): Boolean {
        val cm = context.applicationContext.getSystemService(ConnectivityManager::class.java) ?: return false
        val caps = cm.getNetworkCapabilities(cm.activeNetwork) ?: return false
        return caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)
    }

    /** Exécute [work] avec un verrou multicast Wi-Fi (réception mDNS). */
    private fun <T> withMulticast(context: Context, work: () -> T): T {
        val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        val lock = try {
            wifi?.createMulticastLock("nyx-sync")?.apply {
                setReferenceCounted(false)
                acquire()
            }
        } catch (e: RuntimeException) {
            Log.w(TAG, "Verrou multicast indisponible", e)
            null
        }
        try {
            return work()
        } finally {
            try {
                if (lock?.isHeld == true) lock.release()
            } catch (e: RuntimeException) {
                Log.w(TAG, "Libération du verrou multicast", e)
            }
        }
    }

    // ── Synchronisation ──────────────────────────────────────────────────

    private fun parseLast(json: String?): Last? = try {
        if (json.isNullOrEmpty()) null else JSONObject(json).let {
            Last(it.optLong("at_ms"), it.optBoolean("ok"), it.optString("message"), it.optBoolean("changed"))
        }
    } catch (e: JSONException) {
        null
    }

    /** Dernière synchronisation connue (persistée dans le coffre). */
    fun lastSync(): Last? = if (available) parseLast(NyxSync.lastSync()) else null

    /** « Synchroniser maintenant ». `done` sur le thread UI. */
    fun syncNow(context: Context, done: (Last) -> Unit) {
        if (!available) return done(Last(System.currentTimeMillis(), false, UNAVAILABLE, false))
        val app = context.applicationContext
        syncing = true
        background(io, {
            val last = withMulticast(app) { parseLast(NyxSync.syncNow()) }
            last ?: Last(System.currentTimeMillis(), false, "Synchronisation impossible", false)
        }) { last ->
            syncing = false
            for (l in listeners) l(last)
            done(last)
        }
    }

    /**
     * Synchronisation automatique (ouverture / retour dans l'app) : sur
     * Wi-Fi seulement, s'il existe au moins un appareil lié, au plus une
     * fois par minute. Silencieuse.
     */
    fun autoSync(context: Context) {
        if (!available || syncing || !onWifi(context)) return
        val now = SystemClock.elapsedRealtime()
        if (lastAuto != 0L && now - lastAuto < AUTO_INTERVAL_MS) return
        lastAuto = now
        val app = context.applicationContext
        background(io, {
            if (devicesNow().none { !it.me && !it.revoked }) null
            else withMulticast(app) { parseLast(NyxSync.syncNow()) }
        }) { last ->
            if (last != null) for (l in listeners) l(last)
        }
    }

    // ── Appareils ────────────────────────────────────────────────────────

    private fun devicesNow(): List<Device> = try {
        val array = JSONArray(if (available) NyxSync.devicesJson() ?: "[]" else "[]")
        (0 until array.length()).map {
            val o = array.getJSONObject(it)
            Device(o.getString("id"), o.optString("name"), o.optLong("addedAt"), o.optBoolean("revoked"), o.optBoolean("me"))
        }
    } catch (e: JSONException) {
        Log.w(TAG, "Liste des appareils illisible", e)
        emptyList()
    }

    fun devices(done: (List<Device>) -> Unit) = background(io, { devicesNow() }, done)

    fun rename(name: String, done: (String) -> Unit) {
        if (!available) return done(UNAVAILABLE)
        background(io, { NyxSync.setDeviceName(name).orEmpty() }, done)
    }

    fun revoke(id: String, done: (String) -> Unit) {
        if (!available) return done(UNAVAILABLE)
        background(io, { NyxSync.revoke(id).orEmpty() }, done)
    }

    // ── Appairage (deux temps) ───────────────────────────────────────────

    private fun parsePair(json: String?): PairResult = try {
        val o = JSONObject(json ?: "{}")
        when (o.optString("state")) {
            "confirm" -> PairResult.Confirm(o.optString("code"), o.optString("pc"))
            "done" -> PairResult.Done(o.optString("pc"))
            "pending" -> PairResult.Error("Le PC ne répond pas. Vérifiez que les deux appareils sont sur le même Wi-Fi.")
            else -> PairResult.Error(o.optString("message").ifEmpty { "Appairage impossible" })
        }
    } catch (e: JSONException) {
        PairResult.Error("Appairage impossible")
    }

    /** Étape 1 : connexion au PC, rend le code à 6 chiffres (ou l'erreur). */
    fun pairBegin(offer: String, deviceName: String, done: (PairResult) -> Unit) {
        if (!available) return done(PairResult.Error(UNAVAILABLE))
        background(pairIo, { parsePair(NyxSync.pairBegin(offer, deviceName)) }, done)
    }

    /** Étape 2 : réponse de l'utilisateur, rend le résultat final. */
    fun pairConfirm(yes: Boolean, done: (PairResult) -> Unit) {
        if (!NyxSync.loaded) return done(PairResult.Error(UNAVAILABLE))
        background(pairIo, { parsePair(NyxSync.pairConfirm(yes)) }, done)
    }

    /** Écran quitté pendant l'appairage : refus (non bloquant). */
    fun pairCancel() {
        if (NyxSync.loaded) NyxSync.pairCancel()
    }
}
